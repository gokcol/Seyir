//! Locate the XMP packet and rewrite only its container bytes.
//! Adobe XMP Storage in Files governs JPEG and PNG, and RFC 9649 governs WebP.
use super::io_error;
use std::{
    io::{Read, Write},
    ops::Range,
};

const JPEG_XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const JPEG_EXTENDED_XMP: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_XMP: &[u8] = b"XML:com.adobe.xmp\0";
const MAX_XMP_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) enum Format {
    Jpeg,
    Png,
    Webp,
}

pub(super) struct Layout {
    pub packet: Option<Vec<u8>>,
    pub edit_note: Option<String>,
    replace: Range<usize>,
    webp: Option<WebpHeader>,
}

enum WebpHeader {
    Existing,
    Add([u8; 10]),
}

impl Layout {
    fn new(insertion: usize) -> Self {
        Self {
            packet: None,
            edit_note: None,
            replace: insertion..insertion,
            webp: None,
        }
    }

    fn found(&mut self, replacement: Range<usize>, packet: Vec<u8>) -> Result<(), String> {
        if self.packet.is_some() {
            return Err("This file has multiple XMP packets. Metadata is read-only.".into());
        }
        if packet.len() > MAX_XMP_BYTES {
            return Err("This file's XMP packet is too large to edit.".into());
        }
        self.packet = Some(packet);
        self.replace = replacement;
        Ok(())
    }
}

impl Format {
    pub fn detect(signature: &[u8]) -> Option<Self> {
        if signature.starts_with(b"\xff\xd8\xff") {
            Some(Self::Jpeg)
        } else if signature.starts_with(PNG_HEADER) {
            Some(Self::Png)
        } else if signature.starts_with(b"RIFF") && signature.get(8..12) == Some(b"WEBP") {
            Some(Self::Webp)
        } else {
            None
        }
    }

    pub fn inspect(self, data: &[u8]) -> Result<Layout, String> {
        match self {
            Self::Jpeg => jpeg(data),
            Self::Png => png(data),
            Self::Webp => webp(data),
        }
    }

    pub fn rewrite(
        self,
        original: &[u8],
        layout: &Layout,
        packet: &[u8],
        out: &mut impl Write,
    ) -> Result<(), String> {
        if packet.len() > MAX_XMP_BYTES {
            return Err("The edited XMP packet is too large.".into());
        }
        let prefix = &original[..layout.replace.start];
        let suffix = &original[layout.replace.end..];
        match self {
            Self::Jpeg => {
                let len = u16::try_from(JPEG_XMP.len() + packet.len() + 2)
                    .map_err(|_| "The edited XMP packet is too large for JPEG.".to_owned())?;
                write_slices(
                    out,
                    &[
                        prefix,
                        &[0xff, 0xe1],
                        &len.to_be_bytes(),
                        JPEG_XMP,
                        packet,
                        suffix,
                    ],
                )
            }
            Self::Png => {
                let fields = [0_u8; 4]; // Uncompressed UTF-8, empty language and translated keyword.
                let len = (PNG_XMP.len() + fields.len() + packet.len()) as u32;
                let mut crc = crc32fast::Hasher::new();
                for part in [b"iTXt", PNG_XMP, &fields, packet] {
                    crc.update(part);
                }
                write_slices(
                    out,
                    &[
                        prefix,
                        &len.to_be_bytes(),
                        b"iTXt",
                        PNG_XMP,
                        &fields,
                        packet,
                        &crc.finalize().to_be_bytes(),
                        suffix,
                    ],
                )
            }
            Self::Webp => {
                let header = layout.webp.as_ref().ok_or_else(malformed)?;
                let added = if matches!(header, WebpHeader::Add(_)) {
                    18
                } else {
                    0
                };
                let padding: &[u8] = if packet.len().is_multiple_of(2) {
                    &[]
                } else {
                    &[0]
                };
                let riff_size = u32::try_from(
                    prefix.len() + suffix.len() + packet.len() + padding.len() + added,
                )
                .map_err(|_| "The edited WebP is too large.".to_owned())?;
                write_slices(out, &[b"RIFF", &riff_size.to_le_bytes(), b"WEBP"])?;
                match header {
                    WebpHeader::Existing => {
                        write_slices(out, &[&prefix[12..20], &[prefix[20] | 0x04], &prefix[21..]])?
                    }
                    WebpHeader::Add(header) => write_slices(
                        out,
                        &[b"VP8X", &10_u32.to_le_bytes(), header, &prefix[12..]],
                    )?,
                }
                write_slices(
                    out,
                    &[
                        b"XMP ",
                        &(packet.len() as u32).to_le_bytes(),
                        packet,
                        padding,
                        suffix,
                    ],
                )
            }
        }
    }
}

fn write_slices(out: &mut impl Write, data: &[&[u8]]) -> Result<(), String> {
    for part in data {
        out.write_all(part).map_err(io_error)?;
    }
    Ok(())
}

fn malformed() -> String {
    "The image container is damaged or uses an unsupported metadata layout.".into()
}

fn jpeg(data: &[u8]) -> Result<Layout, String> {
    let mut cursor = 2;
    let mut layout = Layout::new(cursor);
    loop {
        let start = cursor;
        if data.get(cursor) != Some(&0xff) {
            return Err(malformed());
        }
        while data.get(cursor) == Some(&0xff) {
            cursor += 1;
        }
        let marker = *data.get(cursor).ok_or_else(malformed)?;
        cursor += 1;
        match marker {
            0xda => {
                if layout.packet.is_none() {
                    layout.replace = start..start;
                }
                return Ok(layout);
            }
            0 | 0xd0..=0xd9 => return Err(malformed()),
            0x01 => continue,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes(
            data.get(cursor..cursor + 2)
                .ok_or_else(malformed)?
                .try_into()
                .unwrap(),
        ));
        if length < 2 {
            return Err(malformed());
        }
        let end = cursor.checked_add(length).ok_or_else(malformed)?;
        let contents = data.get(cursor + 2..end).ok_or_else(malformed)?;
        if marker == 0xe1 {
            if let Some(packet) = contents.strip_prefix(JPEG_XMP) {
                layout.found(start..end, packet.to_vec())?;
            } else if contents.starts_with(JPEG_EXTENDED_XMP) {
                layout.edit_note = Some("Extended JPEG XMP is read-only.".into());
            } else if contents.starts_with(b"MPF\0") {
                layout.edit_note = Some("Multi-picture JPEG metadata is read-only.".into());
            }
        } else if marker == 0xe2 && contents.starts_with(b"MPF\0") {
            // The second image's offsets would need to be recalculated.
            layout.edit_note = Some("Multi-picture JPEG metadata is read-only.".into());
        }
        cursor = end;
    }
}

fn png(data: &[u8]) -> Result<Layout, String> {
    let mut cursor = PNG_HEADER.len();
    let mut layout = Layout::new(cursor);
    loop {
        let header = data.get(cursor..cursor + 8).ok_or_else(malformed)?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let contents_start = cursor + 8;
        let contents_end = contents_start.checked_add(length).ok_or_else(malformed)?;
        let end = contents_end.checked_add(4).ok_or_else(malformed)?;
        let contents = data
            .get(contents_start..contents_end)
            .ok_or_else(malformed)?;
        let checksum = data.get(contents_end..end).ok_or_else(malformed)?;
        match &header[4..] {
            b"IEND" if length == 0 => {
                if layout.packet.is_none() {
                    layout.replace = cursor..cursor;
                }
                return Ok(layout);
            }
            b"iTXt" => {
                if let Some(text) = contents.strip_prefix(PNG_XMP) {
                    let crc = crc32fast::hash(&data[cursor + 4..contents_end]);
                    if checksum != crc.to_be_bytes() {
                        return Err("The PNG metadata checksum is invalid.".into());
                    }
                    let mut fields = text.get(2..).ok_or_else(malformed)?;
                    for _ in 0..2 {
                        let end = fields.iter().position(|n| *n == 0).ok_or_else(malformed)?;
                        fields = &fields[end + 1..];
                    }
                    let packet = match text.get(..2) {
                        Some([0, 0]) if fields.len() <= MAX_XMP_BYTES => fields.to_vec(),
                        Some([1, 0]) => {
                            let mut decoded = Vec::new();
                            flate2::read::ZlibDecoder::new(fields)
                                .take((MAX_XMP_BYTES + 1) as u64)
                                .read_to_end(&mut decoded)
                                .map_err(io_error)?;
                            decoded
                        }
                        _ => return Err("This PNG has unsupported XMP text encoding.".into()),
                    };
                    layout.found(cursor..end, packet)?;
                }
            }
            _ => {}
        }
        cursor = end;
    }
}

fn webp(data: &[u8]) -> Result<Layout, String> {
    let len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if len.checked_add(8) != Some(data.len()) {
        return Err("The WebP RIFF length does not match the file length.".into());
    }
    let mut cursor = 12;
    let mut layout = Layout::new(data.len());
    let mut canvas = None;
    let mut extra_features = false;
    while cursor < data.len() {
        let chunk = data.get(cursor..cursor + 8).ok_or_else(malformed)?;
        let length = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as usize;
        let contents_start = cursor + 8;
        let contents_end = contents_start.checked_add(length).ok_or_else(malformed)?;
        let end = contents_end.checked_add(length % 2).ok_or_else(malformed)?;
        if end > data.len() {
            return Err(malformed());
        }
        let contents = &data[contents_start..contents_end];
        match &chunk[..4] {
            b"XMP " => {
                if length > MAX_XMP_BYTES {
                    return Err("This file's XMP packet is too large to edit.".into());
                }
                layout.found(cursor..end, contents.to_vec())?;
            }
            b"VP8X" => {
                if cursor != 12 || length != 10 {
                    return Err(malformed());
                }
                layout.webp = Some(WebpHeader::Existing);
            }
            b"VP8 " if layout.webp.is_none() => {
                let header = contents.get(..10).ok_or_else(malformed)?;
                if header[3..6] != [0x9d, 0x01, 0x2a] {
                    return Err(malformed());
                }
                let width =
                    u32::from(u16::from_le_bytes(header[6..8].try_into().unwrap()) & 0x3fff);
                let height =
                    u32::from(u16::from_le_bytes(header[8..10].try_into().unwrap()) & 0x3fff);
                canvas = Some((
                    width.checked_sub(1).ok_or_else(malformed)?,
                    height.checked_sub(1).ok_or_else(malformed)?,
                    false,
                ));
            }
            b"VP8L" if layout.webp.is_none() => {
                if contents.first() != Some(&0x2f) {
                    return Err(malformed());
                }
                let packed = u32::from_le_bytes(
                    contents
                        .get(1..5)
                        .ok_or_else(malformed)?
                        .try_into()
                        .unwrap(),
                );
                if packed >> 29 != 0 {
                    return Err(malformed());
                }
                canvas = Some((
                    packed & 0x3fff,
                    (packed >> 14) & 0x3fff,
                    (packed >> 28) & 1 != 0,
                ));
            }
            b"ANIM" | b"ANMF" | b"ALPH" | b"ICCP" | b"EXIF" => extra_features = true,
            _ => {}
        }
        cursor = end;
    }
    if layout.webp.is_none() {
        if extra_features {
            return Err(malformed());
        }
        let (width_minus_one, height_minus_one, alpha) = canvas.ok_or_else(malformed)?;
        let mut header = [0; 10];
        header[0] = if alpha { 0x14 } else { 0x04 };
        header[4..7].copy_from_slice(&width_minus_one.to_le_bytes()[..3]);
        header[7..10].copy_from_slice(&height_minus_one.to_le_bytes()[..3]);
        layout.webp = Some(WebpHeader::Add(header));
    }
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editable_formats_round_trip_packets_and_preserve_pixels() {
        let rgba =
            image::RgbaImage::from_fn(28, 20, |x, y| image::Rgba([x as u8, y as u8, 40, 170]));
        for (format, codec) in [
            (Format::Jpeg, image::ImageFormat::Jpeg),
            (Format::Png, image::ImageFormat::Png),
            (Format::Webp, image::ImageFormat::WebP),
        ] {
            let mut output = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8())
                .write_to(&mut output, codec)
                .unwrap();
            let original = output.into_inner();
            let image = image::load_from_memory(&original).unwrap().to_rgba8();
            let packet = b"<xmp>new metadata</xmp>";
            let mut encoded = Vec::new();
            format
                .rewrite(
                    &original,
                    &format.inspect(&original).unwrap(),
                    packet,
                    &mut encoded,
                )
                .unwrap();
            let layout = format.inspect(&encoded).unwrap();
            assert_eq!(layout.packet.as_deref(), Some(packet.as_slice()));
            assert_eq!(image::load_from_memory(&encoded).unwrap().to_rgba8(), image);
            let mut rewritten = Vec::new();
            format
                .rewrite(&encoded, &layout, b"<edited/>", &mut rewritten)
                .unwrap();
            assert_eq!(
                format.inspect(&rewritten).unwrap().packet.as_deref(),
                Some(b"<edited/>".as_slice())
            );
            assert_eq!(
                image::load_from_memory(&rewritten).unwrap().to_rgba8(),
                image
            );
        }
    }
}
