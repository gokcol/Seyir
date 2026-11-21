//! Per-user history of successfully opened images.
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};

const MAX_RECENT_IMAGES: usize = 12;
const MAX_STORE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
pub(super) struct Recents {
    store: Option<PathBuf>,
    paths: Vec<PathBuf>,
    pending: Vec<PathBuf>,
}

impl Recents {
    pub(super) fn load() -> Self {
        directories::ProjectDirs::from("io.github", "gokcol", "Seyir")
            .map(|dirs| Self::at(dirs.config_dir().join("recent-images")))
            .unwrap_or_default()
    }

    fn at(store: PathBuf) -> Self {
        let paths = match read_paths(&store) {
            Ok(paths) => paths,
            Err(error) => {
                eprintln!("Could not read recent images: {error}");
                Vec::new()
            }
        };
        Self {
            store: Some(store),
            paths,
            pending: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, path: &Path) -> io::Result<()> {
        if !path.is_absolute()
            || (self.pending.is_empty() && self.paths.first().is_some_and(|first| first == path))
        {
            return Ok(());
        }
        move_to_front(&mut self.paths, path);
        if let Some(store) = &self.store {
            move_to_front(&mut self.pending, path);
            self.paths = update(store, &self.pending)?;
            self.pending.clear();
        }
        Ok(())
    }

    pub(super) fn available(&self) -> impl Iterator<Item = &Path> {
        self.paths
            .iter()
            .map(PathBuf::as_path)
            .filter(|path| path.is_file())
    }
}

fn move_to_front(paths: &mut Vec<PathBuf>, path: &Path) {
    paths.retain(|candidate| candidate != path);
    paths.insert(0, path.to_owned());
    paths.truncate(MAX_RECENT_IMAGES);
}

fn update(store: &Path, pending: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let parent = store
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing history directory"))?;
    fs::create_dir_all(parent)?;
    // Lock a separate file so atomically replacing the history keeps the lock valid.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(store.with_extension("lock"))?;
    lock.lock()?;
    let mut paths = read_paths(store)?;
    for path in pending.iter().rev() {
        move_to_front(&mut paths, path);
    }
    let mut replacement = tempfile::NamedTempFile::new_in(parent)?;
    for path in &paths {
        write_path(&mut replacement, path)?;
    }
    replacement.as_file().sync_all()?;
    replacement.persist(store).map_err(|error| error.error)?;
    Ok(paths)
}

fn read_paths(store: &Path) -> io::Result<Vec<PathBuf>> {
    let file = match File::open(store) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut data = Vec::new();
    file.take(MAX_STORE_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_STORE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "History is too large",
        ));
    }
    #[cfg(unix)]
    let units = data;
    #[cfg(windows)]
    let units = {
        let words = data.chunks_exact(2);
        if !words.remainder().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid UTF-16 history",
            ));
        }
        words
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect::<Vec<_>>()
    };
    if !units.is_empty() && units.last() != Some(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Incomplete history",
        ));
    }
    // Native path units separated by NUL preserve newlines and paths which are not valid UTF-8.
    let mut paths = Vec::new();
    for units in units
        .split(|unit| *unit == 0)
        .filter(|units| !units.is_empty())
    {
        #[cfg(unix)]
        let path = PathBuf::from(OsString::from_vec(units.to_vec()));
        #[cfg(windows)]
        let path = PathBuf::from(OsString::from_wide(units));
        if path.is_absolute() && !paths.contains(&path) {
            paths.push(path);
            if paths.len() == MAX_RECENT_IMAGES {
                break;
            }
        }
    }
    Ok(paths)
}

#[cfg(unix)]
fn write_path(output: &mut impl Write, path: &Path) -> io::Result<()> {
    output.write_all(path.as_os_str().as_bytes())?;
    output.write_all(&[0])
}

#[cfg(windows)]
fn write_path(output: &mut impl Write, path: &Path) -> io::Result<()> {
    let bytes: Vec<u8> = path
        .as_os_str()
        .encode_wide()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    output.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_paths_and_merges_history_from_multiple_viewers() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path().join("recent-images");
        let mut main = Recents::at(store.clone());
        let mut another = Recents::at(store.clone());
        let first = directory.path().join("İstanbul 🖼️.png");
        let second = directory.path().join("Çiçek.png");
        fs::write(&first, []).unwrap();
        fs::write(&second, []).unwrap();
        main.record(&first).unwrap();
        another.record(&second).unwrap();
        assert_eq!(read_paths(&store).unwrap(), [second.clone(), first.clone()]);
        another.record(&first).unwrap();
        fs::remove_file(&second).unwrap();

        let mut reopened = Recents::at(store.clone());
        assert_eq!(reopened.available().collect::<Vec<_>>(), [first.as_path()]);
        assert_eq!(read_paths(&store).unwrap(), [first, second]);
        for i in 0..MAX_RECENT_IMAGES {
            reopened
                .record(&directory.path().join(format!("Image {i}.png")))
                .unwrap();
        }
        assert_eq!(read_paths(&store).unwrap().len(), MAX_RECENT_IMAGES);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_newlines_and_non_unicode_bytes_in_native_paths() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path().join("recent-images");
        let image = directory
            .path()
            .join(OsString::from_vec(b"Line\n\xff name.png".to_vec()));
        Recents::at(store.clone()).record(&image).unwrap();
        assert_eq!(read_paths(&store).unwrap(), [image]);
    }
}
