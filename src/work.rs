//! Coalesced workers keep file I/O and decoding out of the UI event loop.
use crate::decode::{self, Decoded};
use std::{
    cmp::Ordering,
    collections::VecDeque,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::SystemTime,
};
use unicode_normalization::{UnicodeNormalization, is_nfc};

struct Pending<T> {
    job: Option<T>,
    closed: bool,
}

/// Each worker keeps only the newest queued request while its current job finishes.
pub struct Latest<T> {
    pending: Arc<(Mutex<Pending<T>>, Condvar)>,
}

impl<T: Send + 'static> Latest<T> {
    pub fn start(mut run: impl FnMut(T) + Send + 'static) -> Self {
        let pending = Arc::new((
            Mutex::new(Pending {
                job: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let thread_pending = Arc::clone(&pending);
        thread::spawn(move || {
            let (lock, wake) = &*thread_pending;
            loop {
                let Ok(guard) = lock.lock() else { break };
                let Ok(mut state) =
                    wake.wait_while(guard, |state| !state.closed && state.job.is_none())
                else {
                    break;
                };
                if state.closed {
                    break;
                }
                if let Some(job) = state.job.take() {
                    drop(state);
                    run(job);
                }
            }
        });
        Self { pending }
    }

    pub fn submit(&self, job: T) {
        if let Ok(mut state) = self.pending.0.lock() {
            state.job = Some(job);
            self.pending.1.notify_one();
        }
    }
}

impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.pending.0.lock() {
            state.closed = true;
            state.job = None;
            self.pending.1.notify_one();
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(path: &Path) -> Result<Stamp, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("Cannot read image: {error}"))?;
    if !metadata.is_file() {
        return Err("Choose an image file.".into());
    }
    Ok(Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

struct Cached {
    path: PathBuf,
    stamp: Stamp,
    image: Arc<Decoded>,
}

#[derive(Default)]
pub struct ImageCache {
    entries: VecDeque<Cached>,
    bytes: usize,
}

impl ImageCache {
    const BUDGET: usize = 144 * 1024 * 1024;

    pub fn load(cache: &Mutex<Self>, path: &Path) -> Result<Arc<Decoded>, String> {
        let before = stamp(path)?;
        if let Ok(mut cache) = cache.lock()
            && let Some(index) = cache
                .entries
                .iter()
                .position(|item| item.path == path && item.stamp == before)
            && let Some(item) = cache.entries.remove(index)
        {
            let image = Arc::clone(&item.image);
            cache.entries.push_back(item);
            return Ok(image);
        }
        let image = Arc::new(decode::decode(path)?);
        if stamp(path)? != before {
            return Err("The image changed while opening. Open it again to refresh.".into());
        }
        let bytes = image.pixels.as_raw().len();
        if bytes <= Self::BUDGET
            && let Ok(mut cache) = cache.lock()
        {
            if let Some(index) = cache.entries.iter().position(|item| item.path == path)
                && let Some(item) = cache.entries.remove(index)
            {
                cache.bytes -= item.image.pixels.as_raw().len();
            }
            while (cache.bytes + bytes > Self::BUDGET || cache.entries.len() >= 5)
                && let Some(item) = cache.entries.pop_front()
            {
                cache.bytes -= item.image.pixels.as_raw().len();
            }
            cache.bytes += bytes;
            cache.entries.push_back(Cached {
                path: path.to_owned(),
                stamp: before,
                image: image.clone(),
            });
        }
        Ok(image)
    }
}

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    modified: Option<SystemTime>,
}

pub fn display_name(name: &OsStr) -> String {
    let text = name.to_string_lossy();
    if is_nfc(&text) {
        text.into_owned()
    } else {
        text.nfc().collect()
    }
}

pub fn folder(path: &Path) -> Result<Vec<Entry>, String> {
    let folders = fs::read_dir(path).map_err(|error| format!("Cannot open folder: {error}"))?;
    let mut entries = Vec::new();
    for entry in folders.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Keep folder browsing shallow. Links to files and folders are resolved on demand.
        let directory = kind.is_dir() || (kind.is_symlink() && path.is_dir());
        let name = entry.file_name();
        if directory && name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        if !directory && !(decode::recognized_path(&path) && (kind.is_file() || path.is_file())) {
            continue;
        }
        entries.push(Entry {
            path,
            name: display_name(&name),
            directory,
            modified: entry.metadata().ok().and_then(|info| info.modified().ok()),
        });
    }
    sort_entries(&mut entries, false);
    Ok(entries)
}

pub fn sort_entries(entries: &mut [Entry], newest: bool) {
    entries.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| {
                if newest {
                    right.modified.cmp(&left.modified)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| natural(&left.name, &right.name))
    });
}

fn natural(left: &str, right: &str) -> Ordering {
    let mut a = left.as_bytes();
    let mut b = right.as_bytes();
    while let (Some(x), Some(y)) = (a.first(), b.first()) {
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let a_len = a.iter().take_while(|v| v.is_ascii_digit()).count();
            let b_len = b.iter().take_while(|v| v.is_ascii_digit()).count();
            let digits =
                |bytes: &[u8]| bytes.iter().position(|v| *v != b'0').unwrap_or(bytes.len());
            let a_digits = &a[digits(&a[..a_len])..a_len];
            let b_digits = &b[digits(&b[..b_len])..b_len];
            let order = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits));
            if order != Ordering::Equal {
                return order;
            }
            a = &a[a_len..];
            b = &b[b_len..];
        } else {
            let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
            if order != Ordering::Equal {
                return order;
            }
            a = &a[1..];
            b = &b[1..];
        }
    }
    a.len().cmp(&b.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_turkish_labels_and_preserves_folder_paths() {
        let root = tempfile::tempdir().unwrap();
        for label in ["Bahçeşehir Üniversitesi", "Işık ve İstanbul"] {
            let decomposed: String = label.nfd().collect();
            fs::create_dir(root.path().join(&decomposed)).unwrap();
        }
        let entries = folder(root.path()).unwrap();
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert!(entry.directory && entry.path.is_dir());
            let filename = entry.path.file_name().unwrap();
            assert!(
                fs::read_dir(root.path())
                    .unwrap()
                    .flatten()
                    .any(|actual| actual.file_name() == filename)
            );
            assert!(matches!(
                entry.name.as_str(),
                "Bahçeşehir Üniversitesi" | "Işık ve İstanbul"
            ));
            assert_eq!(entry.name, display_name(filename));
        }
    }

    #[test]
    fn filenames_sort_numerically_without_overflow() {
        let mut paths = [
            "img-100.png",
            "img-2.png",
            "img-10.png",
            "img-9999999999999999999999999999999999999.png",
        ];
        paths.sort_by(|left, right| natural(left, right));
        assert_eq!(&paths[..3], &["img-2.png", "img-10.png", "img-100.png"]);
    }

    #[test]
    fn dropped_work_finishes_current_and_keeps_newest_request() {
        use std::sync::mpsc;
        let (entered, requests) = mpsc::channel();
        let (resume, proceed) = mpsc::channel();
        let worker = Latest::start(move |number| {
            let _ = entered.send(number);
            if number == 1 {
                let _ = proceed.recv();
            }
        });
        worker.submit(1);
        assert_eq!(requests.recv().unwrap(), 1);
        worker.submit(2);
        worker.submit(3);
        resume.send(()).unwrap();
        assert_eq!(requests.recv().unwrap(), 3);
    }
}
