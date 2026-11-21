//! Image inspection and lossless in-file XMP updates.
mod container;
mod xmp;

use image::ImageDecoder;
use std::{
    fs::{self, File, Metadata},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAX_EDIT_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EditableMetadata {
    pub title: String,
    pub description: String,
    pub author: String,
    pub copyright: String,
    /// Comma-separated subject keywords.
    pub keywords: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ImageMetadata {
    pub byte_len: u64,
    pub modified: Option<SystemTime>,
    pub dimensions: Option<[u32; 2]>,
    pub format: String,
    pub details: Vec<(String, String)>,
    pub editable: Option<EditableMetadata>,
    pub edit_note: Option<String>,
    path: PathBuf,
    revision: Revision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Revision {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: [u64; 2],
    #[cfg(unix)]
    changed: [i64; 2],
}

impl Revision {
    fn from(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            identity: [metadata.dev(), metadata.ino()],
            #[cfg(unix)]
            changed: [metadata.ctime(), metadata.ctime_nsec()],
        }
    }

    fn check(&self, metadata: &Metadata) -> Result<(), String> {
        if self == &Self::from(metadata) {
            Ok(())
        } else {
            Err("The image changed on disk. Reload its information before editing again.".into())
        }
    }
}

pub(crate) fn read(path: &Path) -> Result<ImageMetadata, String> {
    let path = path.canonicalize().map_err(io_error)?;
    let mut file = File::open(&path).map_err(io_error)?;
    let original = file.metadata().map_err(io_error)?;
    if !original.is_file() {
        return Err("Image information requires a regular file.".into());
    }
    let revision = Revision::from(&original);

    let mut signature = [0; 12];
    let mut signature_length = 0;
    while signature_length < signature.len() {
        let received = file
            .read(&mut signature[signature_length..])
            .map_err(io_error)?;
        if received == 0 {
            break;
        }
        signature_length += received;
    }
    let format = image::guess_format(&signature[..signature_length])
        .ok()
        .map(|format| format!("{format:?}"))
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_uppercase)
        })
        .unwrap_or_else(|| "Image".into());

    let reader = File::open(&path).ok().and_then(|input| {
        image::ImageReader::new(BufReader::new(input))
            .with_guessed_format()
            .ok()
    });
    let mut dimensions = None;
    let mut details = Vec::new();
    if let Some(mut decoder) = reader.and_then(|input| input.into_decoder().ok()) {
        let (width, height) = decoder.dimensions();
        dimensions = Some([width, height]);
        details.push((
            "Pixel type".into(),
            format!("{:?}", decoder.original_color_type()),
        ));
        if let Ok(Some(exif)) = decoder.exif_metadata() {
            details.extend(exif_details(&exif));
        }
        if let Ok(Some(icc)) = decoder.icc_profile() {
            details.push((
                "ICC profile".into(),
                format!("Embedded, {} bytes", icc.len()),
            ));
        }
    }

    let mut editable = None;
    let mut edit_note = None;
    if let Some(kind) = container::Format::detect(&signature[..signature_length]) {
        if revision.len > MAX_EDIT_FILE_BYTES {
            edit_note =
                Some("Embedded metadata editing is available for files up to 128 MiB.".into());
        } else {
            let bytes = read_editable_bytes(&path, &revision)?;
            match kind.inspect(&bytes).and_then(|layout| {
                let fields = layout.packet.as_deref().map(xmp::read).transpose()?;
                Ok((fields.unwrap_or_default(), layout.edit_note))
            }) {
                Ok((fields, restriction)) => {
                    if restriction.is_some() || original.permissions().readonly() {
                        append_xmp_details(&mut details, &fields);
                        edit_note = restriction.or_else(|| Some("This file is read-only.".into()));
                    } else {
                        editable = Some(fields);
                    }
                }
                Err(error) => edit_note = Some(error),
            }
        }
    } else {
        edit_note = Some("Embedded metadata editing supports JPEG, PNG and WebP.".into());
    }

    revision.check(&fs::metadata(&path).map_err(io_error)?)?;
    Ok(ImageMetadata {
        byte_len: revision.len,
        modified: revision.modified,
        dimensions,
        format,
        details,
        editable,
        edit_note,
        path,
        revision,
    })
}

pub(crate) fn write(
    path: &Path,
    previous: &ImageMetadata,
    updated: &EditableMetadata,
) -> Result<ImageMetadata, String> {
    let path = path.canonicalize().map_err(io_error)?;
    let Some(original_fields) = previous.editable.as_ref().filter(|_| path == previous.path) else {
        return Err("Reload editable image information before saving.".into());
    };
    if original_fields == updated {
        return read(&path);
    }
    let original = read_editable_bytes(&path, &previous.revision)?;
    let kind = container::Format::detect(&original)
        .ok_or_else(|| "This image format does not support writing metadata.".to_owned())?;
    let layout = kind.inspect(&original)?;
    if let Some(reason) = layout.edit_note.as_ref() {
        return Err(reason.clone());
    }
    let xmp = xmp::update(layout.packet.as_deref(), original_fields, updated)?;
    let parent = path
        .parent()
        .ok_or_else(|| "The image has no containing folder.".to_owned())?;
    let permissions = fs::metadata(&path).map_err(io_error)?.permissions();
    if permissions.readonly() {
        return Err("This file is read-only.".into());
    }
    let mut replacement = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    kind.rewrite(
        &original,
        &layout,
        xmp.as_bytes(),
        replacement.as_file_mut(),
    )?;
    replacement.as_file_mut().flush().map_err(io_error)?;
    replacement
        .as_file()
        .set_permissions(permissions)
        .map_err(io_error)?;
    replacement.as_file().sync_all().map_err(io_error)?;
    verify_original(&path, &previous.revision, &original)?;
    replacement
        .persist(&path)
        .map_err(|error| io_error(error.error))?;
    read(&path)
}

fn read_editable_bytes(path: &Path, revision: &Revision) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(io_error)?;
    revision.check(&file.metadata().map_err(io_error)?)?;
    let mut bytes = Vec::new();
    let mut capped = (&file).take(MAX_EDIT_FILE_BYTES + 1);
    capped.read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 != revision.len || bytes.len() as u64 > MAX_EDIT_FILE_BYTES {
        return Err("The image size changed or exceeds the metadata editing limit.".into());
    }
    revision.check(&file.metadata().map_err(io_error)?)?;
    Ok(bytes)
}

fn verify_original(path: &Path, revision: &Revision, original: &[u8]) -> Result<(), String> {
    let mut file = File::open(path).map_err(io_error)?;
    revision.check(&file.metadata().map_err(io_error)?)?;
    let mut buffer = [0; 64 * 1024];
    for chunk in original.chunks(buffer.len()) {
        let window = &mut buffer[..chunk.len()];
        file.read_exact(window).map_err(io_error)?;
        if window != chunk {
            return Err(
                "The image changed on disk. Reload its information before editing again.".into(),
            );
        }
    }
    revision.check(&file.metadata().map_err(io_error)?)?;
    revision.check(&fs::metadata(path).map_err(io_error)?)
}

fn append_xmp_details(details: &mut Vec<(String, String)>, fields: &EditableMetadata) {
    for (key, value) in [
        ("Title", &fields.title),
        ("Description", &fields.description),
        ("Author", &fields.author),
        ("Copyright", &fields.copyright),
        ("Keywords", &fields.keywords),
    ] {
        if !value.is_empty() {
            details.push((key.into(), value.clone()));
        }
    }
}

pub(super) fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn exif_details(bytes: &[u8]) -> Vec<(String, String)> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    let le = match bytes.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Vec::new(),
    };
    let u16_at = |offset: usize| {
        let word: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(word)
        } else {
            u16::from_be_bytes(word)
        })
    };
    let u32_at = |offset: usize| {
        let word: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(word)
        } else {
            u32::from_be_bytes(word)
        })
    };
    if u16_at(2) != Some(42) {
        return Vec::new();
    }
    let mut entries = Vec::new();
    let mut directory = u32_at(4);
    let mut subdirectory = None;
    for _ in 0..2 {
        let Some(offset) = directory.and_then(|value| usize::try_from(value).ok()) else {
            break;
        };
        let Some(count) = u16_at(offset) else { break };
        for index in 0..usize::from(count.min(4096)) {
            let Some(entry) = offset.checked_add(2 + index * 12) else {
                break;
            };
            let Some(tag) = u16_at(entry) else { break };
            let Some(kind) = u16_at(entry + 2) else { break };
            let Some(count) = u32_at(entry + 4) else {
                break;
            };
            if tag == 0x8769 && kind == 4 && count == 1 {
                subdirectory = u32_at(entry + 8);
                continue;
            }
            let label = match tag {
                0x010f => "Camera make",
                0x0110 => "Camera model",
                0x0131 => "Software",
                0x0132 => "EXIF modified",
                0x013b => "EXIF artist",
                0x8298 => "EXIF copyright",
                0x9003 => "Date taken",
                0xa434 => "Lens model",
                _ => continue,
            };
            if kind != 2 || !(1..=4096).contains(&count) {
                continue;
            }
            let source = if count <= 4 {
                Some(entry + 8)
            } else {
                u32_at(entry + 8).and_then(|offset| usize::try_from(offset).ok())
            };
            if let Some(value) =
                source.and_then(|source| bytes.get(source..source.checked_add(count as usize)?))
            {
                let value = value.split(|byte| *byte == 0).next().unwrap_or_default();
                if !value.is_empty() {
                    entries.push((
                        label.to_owned(),
                        String::from_utf8_lossy(value).into_owned(),
                    ));
                }
            }
        }
        directory = subdirectory.take();
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_save_updates_metadata_without_changing_image_and_rejects_stale_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("example.png");
        let original = image::RgbaImage::from_pixel(32, 24, image::Rgba([13, 64, 208, 140]));
        original.save(&path).unwrap();
        let before = read(&path).unwrap();
        assert_eq!(before.dimensions, Some([32, 24]));
        assert_eq!(before.editable, Some(EditableMetadata::default()));

        let fields = EditableMetadata {
            title: "Sunlight through trees".into(),
            description: "April test photo".into(),
            author: "Orhan".into(),
            copyright: "2026".into(),
            keywords: "trees, test".into(),
        };
        let after = write(&path, &before, &fields).unwrap();
        assert_eq!(after.editable.as_ref(), Some(&fields));
        assert_eq!(image::open(&path).unwrap().to_rgba8(), original);
        assert_eq!(read(&path).unwrap().editable.as_ref(), Some(&fields));
        assert!(
            write(
                &path,
                &before,
                &EditableMetadata {
                    title: "stale".into(),
                    ..fields.clone()
                }
            )
            .is_err()
        );
        assert_eq!(read(&path).unwrap().editable, Some(fields));
    }
}
