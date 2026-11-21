//! Bounded image opening for the viewer. Input pixels stay separate from view transforms.
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage};
use std::{
    fs::{self, File},
    io::{BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 384 * 1024 * 1024;
const MAX_SIDE: u32 = 32_768;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_THUMBNAIL_SIDE: u32 = 1024;
const MAX_SVG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SVG_PIXELS: u64 = 16 * 1024 * 1024;

pub(crate) struct Decoded {
    pub pixels: RgbaImage,
    pub format: String,
}

pub(crate) const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "apng", "webp", "gif", "tif", "tiff", "bmp", "ico", "pnm", "pbm",
    "pgm", "ppm", "pam", "tga", "dds", "hdr", "ff", "svg", "svgz", "heic", "heif", "heics", "avif",
    "jp2", "j2k", "jpf", "jpx", "psd", "raw", "dng", "arw", "cr2", "cr3", "nef", "raf", "orf",
    "rw2", "jxl", "exr",
];

/// For folder browsing only. Explicit opens also detect renamed files by signature.
pub(crate) fn recognized_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

pub(crate) fn decode(path: &Path) -> Result<Decoded, String> {
    validate_file(fs::metadata(path).map_err(read_error)?)?;
    let input = File::open(path).map_err(read_error)?;
    validate_file(input.metadata().map_err(read_error)?)?;
    let mut reader = BufReader::new(input);
    let mut probe = Vec::with_capacity(512);
    reader
        .by_ref()
        .take(512)
        .read_to_end(&mut probe)
        .map_err(read_error)?;
    reader.seek(SeekFrom::Start(0)).map_err(read_error)?;
    let extension = path.extension().and_then(|ext| ext.to_str());
    let detected = image::guess_format(&probe).ok();
    let exact_format = detected.or_else(|| extension.and_then(ImageFormat::from_extension));
    let native = native_label(&probe, None).or_else(|| match detected {
        Some(ImageFormat::Avif) => Some("AVIF"),
        Some(ImageFormat::OpenExr) => Some("OpenEXR"),
        // TIFF-based RAW images can include a small ordinary TIFF preview.
        Some(ImageFormat::Tiff) => {
            native_label(&probe, extension).filter(|label| *label == "Camera RAW")
        }
        None => native_label(&probe, extension),
        _ => None,
    });
    if let Some(label) = native {
        let bytes = read_bounded(reader, MAX_FILE_BYTES)?;
        return platform_image(&bytes, label).map(|pixels| Decoded {
            pixels,
            format: label.into(),
        });
    }

    if let Some(format) = exact_format {
        return decode_raster(reader, format);
    }

    let gzip = probe.starts_with(&[0x1f, 0x8b]);
    if svg_extension(extension) || looks_like_svg(&probe) || gzip {
        let bytes = if gzip {
            read_bounded(flate2::read::GzDecoder::new(reader), MAX_SVG_BYTES)?
        } else {
            read_bounded(reader, MAX_SVG_BYTES)?
        };
        let pixels = decode_svg(&bytes)?;
        return Ok(Decoded {
            pixels,
            format: "SVG".into(),
        });
    }

    Err("Seyir could not identify a supported image format in this file.".into())
}

pub(crate) fn thumbnail(path: &Path, max_edge: u32) -> Result<RgbaImage, String> {
    let max_edge = max_edge.clamp(1, MAX_THUMBNAIL_SIDE);
    // Use the same bounded SVG renderer for previews and full-size opens.
    #[cfg(target_os = "macos")]
    if !svg_extension(path.extension().and_then(|ext| ext.to_str())) {
        validate_file(fs::metadata(path).map_err(read_error)?)?;
        if let Ok(pixels) = macos::thumbnail(path, max_edge) {
            return Ok(pixels);
        }
    }

    let pixels = decode(path)?.pixels;
    if pixels.width().max(pixels.height()) <= max_edge {
        return Ok(pixels);
    }
    Ok(DynamicImage::ImageRgba8(pixels)
        .thumbnail(max_edge, max_edge)
        .into_rgba8())
}

fn read_error(error: std::io::Error) -> String {
    format!("Could not read the image. {error}")
}

fn validate_file(input: fs::Metadata) -> Result<(), String> {
    if !input.is_file() {
        return Err("Choose an image file.".into());
    }
    if input.len() > MAX_FILE_BYTES {
        return Err("Images larger than 256 MiB exceed the file limit.".into());
    }
    Ok(())
}

fn decode_raster<R: std::io::BufRead + Seek>(
    reader: R,
    format: ImageFormat,
) -> Result<Decoded, String> {
    let mut reader = ImageReader::with_format(reader, format);
    reader.limits(decoder_limits());
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("Could not open the {format:?} image. {error}"))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err("The decoded image would exceed Seyir’s memory limit.".into());
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Could not decode the {format:?} image. {error}"))?;
    image.apply_orientation(orientation);
    Ok(Decoded {
        pixels: image.into_rgba8(),
        format: raster_name(format).into(),
    })
}

fn decoder_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(format!(
            "Image dimensions {width} × {height} exceed Seyir’s display limits."
        ));
    }
    Ok(())
}

fn read_bounded(reader: impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read image data. {error}"))?;
    if bytes.len() as u64 > max {
        return Err(format!(
            "Image data exceeds the {} MiB file limit.",
            max >> 20
        ));
    }
    Ok(bytes)
}

fn svg_extension(extension: Option<&str>) -> bool {
    extension.is_some_and(|ext| ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz"))
}

fn looks_like_svg(probe: &[u8]) -> bool {
    let text = String::from_utf8_lossy(probe);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("<svg")
        || (text.starts_with("<?xml") || text.starts_with("<!--")) && text.contains("<svg")
}

fn native_label(probe: &[u8], extension: Option<&str>) -> Option<&'static str> {
    // ISO BMFF major brand is at offset 8; skip the minor version at 12.
    if probe.get(4..8) == Some(b"ftyp")
        && let Some(header) = probe.get(..4)
    {
        let header = u32::from_be_bytes(header.try_into().ok()?) as usize;
        let brands = probe.get(..header.min(probe.len()))?;
        let mut brands = brands
            .get(8..12)?
            .as_chunks::<4>()
            .0
            .iter()
            .chain(brands.get(16..).unwrap_or_default().as_chunks::<4>().0);
        if brands
            .clone()
            .any(|brand| matches!(brand, b"avif" | b"avis"))
        {
            return Some("AVIF");
        }
        if brands.any(|brand| {
            matches!(
                brand,
                b"heic"
                    | b"heix"
                    | b"hevc"
                    | b"hevx"
                    | b"heis"
                    | b"heim"
                    | b"hevs"
                    | b"hevm"
                    | b"mif1"
                    | b"msf1"
            )
        }) {
            return Some("HEIF");
        }
    }
    match extension?.to_ascii_lowercase().as_str() {
        "heic" | "heif" | "heics" => Some("HEIF"),
        "avif" => Some("AVIF"),
        "jp2" | "j2k" | "jpf" | "jpx" => Some("JPEG 2000"),
        "psd" => Some("Photoshop (composite)"),
        "raw" | "dng" | "arw" | "cr2" | "cr3" | "nef" | "raf" | "orf" | "rw2" => Some("Camera RAW"),
        "jxl" => Some("JPEG XL"),
        "exr" => Some("OpenEXR"),
        _ => None,
    }
}

fn raster_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Png => "PNG",
        ImageFormat::WebP => "WebP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Hdr => "Radiance HDR",
        ImageFormat::Farbfeld => "farbfeld",
        _ => "Image",
    }
}

fn platform_image(bytes: &[u8], format: &str) -> Result<RgbaImage, String> {
    #[cfg(target_os = "macos")]
    let result = macos::decode(bytes);
    #[cfg(target_os = "windows")]
    let result = windows::decode(bytes);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let _ = bytes;
        Err("This platform has no native image decoder.".into())
    };
    result.map_err(|error| format!("{format} uses an installed system codec. {error}"))
}

fn decode_svg(bytes: &[u8]) -> Result<RgbaImage, String> {
    use resvg::{tiny_skia, usvg};
    let xml = std::str::from_utf8(bytes).map_err(|_| "This SVG is not UTF-8.".to_owned())?;
    let options = roxmltree::ParsingOptions {
        nodes_limit: 120_000,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(xml, options)
        .map_err(|error| format!("Could not read SVG. {error}"))?;
    if document.root_element().tag_name().name() != "svg" {
        return Err("The file does not contain an SVG image.".into());
    }
    if document
        .descendants()
        .any(|node| node.ancestors().take(65).count() == 65)
    {
        return Err("The SVG has too many nested elements.".into());
    }

    let embedded_pixels = Mutex::new(MAX_SVG_PIXELS);
    let options = usvg::Options {
        // Vector artwork without lettering skips system font enumeration.
        fontdb: if document.descendants().any(|node| node.has_tag_name("text")) {
            svg_fonts()
        } else {
            Arc::default()
        },
        image_href_resolver: usvg::ImageHrefResolver {
            // Restrict data URLs to supported raster formats. In particular, a
            // nested SVG or an external resource cannot bypass the input limits.
            resolve_data: Box::new(|_, data, _| {
                let format = image::guess_format(&data).ok()?;
                if !matches!(
                    format,
                    ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP
                ) {
                    return None;
                }
                let (width, height) = ImageReader::with_format(Cursor::new(data.as_ref()), format)
                    .into_dimensions()
                    .ok()?;
                validate_dimensions(width, height).ok()?;
                let area = u64::from(width) * u64::from(height);
                let mut remaining = embedded_pixels.lock().ok()?;
                *remaining = remaining.checked_sub(area)?;
                match format {
                    ImageFormat::Png => Some(usvg::ImageKind::PNG(data)),
                    ImageFormat::Jpeg => Some(usvg::ImageKind::JPEG(data)),
                    ImageFormat::Gif => Some(usvg::ImageKind::GIF(data)),
                    ImageFormat::WebP => Some(usvg::ImageKind::WEBP(data)),
                    _ => None,
                }
            }),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let scene = usvg::Tree::from_xmltree(&document, &options)
        .map_err(|error| format!("Could not render SVG. {error}"))?;
    let size = scene.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;
    validate_dimensions(width, height)?;
    if u64::from(width) * u64::from(height) > MAX_SVG_PIXELS {
        return Err("The SVG canvas exceeds Seyir’s 16 megapixel rendering limit.".into());
    }
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or("Could not allocate the SVG canvas.")?;
    resvg::render(
        &scene,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    straight_pixels(width, height, pixmap.take())
}

fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        db.set_sans_serif_family("Arial");
        db.set_serif_family("Times New Roman");
        db.set_monospace_family("Courier New");
        Arc::new(db)
    }))
}

/// Both native Apple bitmaps and resvg return premultiplied RGBA. Decode it once
/// into the straight alpha expected by the viewer and clipboard.
fn straight_pixels(width: u32, height: u32, mut pixels: Vec<u8>) -> Result<RgbaImage, String> {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if (1..255).contains(&alpha) {
            for value in &mut pixel[..3] {
                *value = ((u32::from(*value) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "The decoder returned an incomplete image.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_native_brands_and_prefers_avif() {
        let avif = b"\0\0\0\x1cftypmif1\0\0\0\0mif1heicavif";
        let heif = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";
        assert_eq!(native_label(avif, Some("jpg")), Some("AVIF"));
        assert_eq!(native_label(heif, None), Some("HEIF"));
        assert_eq!(native_label(b"hello world", None), None);
    }

    #[test]
    fn respects_svg_transparency_and_rejects_huge_canvas() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="2">
          <rect width="1" height="2" fill="red" opacity="0.5"/>
        </svg>"#;
        let pixels = decode_svg(svg).unwrap();
        assert_eq!(pixels.dimensions(), (3, 2));
        assert_eq!(pixels.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(pixels.get_pixel(2, 0).0, [0, 0, 0, 0]);
        assert!(
            decode_svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="9000" height="9000"/>"#)
                .is_err()
        );
    }

    #[test]
    fn orients_jpeg_using_exif() {
        use image::{ExtendedColorType, ImageEncoder};
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(file.as_file());
        let exif = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\x01\x03\x00\x01\x00\x00\x00\x06\x00\x00\x00\x00\x00\x00\x00";
        encoder.set_exif_metadata(exif.to_vec()).unwrap();
        encoder
            .write_image(&[255, 0, 0, 0, 0, 255], 1, 2, ExtendedColorType::Rgb8)
            .unwrap();
        let decoded = decode(file.path()).unwrap();
        assert_eq!(decoded.pixels.dimensions(), (2, 1));
    }

    #[test]
    fn follows_content_over_an_incorrect_extension() {
        use image::{ExtendedColorType, ImageEncoder};
        let file = tempfile::Builder::new().suffix(".jpeg").tempfile().unwrap();
        image::codecs::png::PngEncoder::new(file.as_file())
            .write_image(&[8, 16, 32, 128], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let decoded = decode(file.path()).unwrap();
        assert_eq!(decoded.format, "PNG");
        assert_eq!(decoded.pixels.as_raw(), &[8, 16, 32, 128]);
    }

    #[test]
    fn thumbnails_fit_the_edge_and_do_not_upscale() {
        use image::{ExtendedColorType, ImageEncoder};
        let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        image::codecs::png::PngEncoder::new(file.as_file())
            .write_image(&[240; 48 * 24 * 4], 48, 24, ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(thumbnail(file.path(), 16).unwrap().dimensions(), (16, 8));
        assert_eq!(thumbnail(file.path(), 200).unwrap().dimensions(), (48, 24));
    }
}
