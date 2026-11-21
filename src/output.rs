//! Size-aware image transfer and atomic file output.
use crate::{canvas, metadata};
use image::{
    ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, RgbaImage,
    codecs::{
        jpeg::{JpegDecoder, JpegEncoder},
        png::{PngDecoder, PngEncoder},
        webp::{WebPDecoder, WebPEncoder},
    },
    imageops::{self, FilterType},
};
use std::{
    fs::{self, File, Metadata},
    io::{self, Cursor, Read, Write},
    path::Path,
};

const MAX_SIDE: u32 = 32_768;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Size {
    #[default]
    Original,
    Percent(u32),
    Fit {
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    WebP,
}

impl Format {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ImageFormat::from_extension(ext)? {
            ImageFormat::Png if ext.eq_ignore_ascii_case("png") => Some(Self::Png),
            ImageFormat::Jpeg => Some(Self::Jpeg),
            ImageFormat::WebP => Some(Self::WebP),
            _ => None,
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::WebP => ImageFormat::WebP,
        }
    }
}

pub(crate) fn preview_dimensions(
    source: &RgbaImage,
    angle: f32,
    size: Size,
) -> Result<(u32, u32), String> {
    let (width, height) = canvas::oriented_size(source.dimensions(), angle);
    if width == 0 || height == 0 {
        return Err("Cannot output an empty image.".into());
    }
    let (w, h) = (u64::from(width), u64::from(height));
    if w * h > 100_000_000 {
        return Err("The rotated result is too large. Reset rotation and try again.".into());
    }
    let dims = match size {
        Size::Original => return Ok((width, height)),
        Size::Percent(percent) if percent > 0 => {
            let factor = u64::from(percent);
            ((w * factor + 50) / 100, (h * factor + 50) / 100)
        }
        Size::Fit { width, height } if width > 0 && height > 0 => {
            let (max_w, max_h) = (u64::from(width), u64::from(height));
            if max_w * h <= max_h * w {
                (max_w, (h * max_w + w / 2) / w)
            } else {
                ((w * max_h + h / 2) / h, max_h)
            }
        }
        _ => return Err("Enter a size greater than zero.".into()),
    };
    let (w, h) = (dims.0.max(1), dims.1.max(1));
    if w > u64::from(MAX_SIDE) || h > u64::from(MAX_SIDE) || w * h > MAX_PIXELS {
        return Err("The output is too large. Choose a smaller size.".into());
    }
    Ok((w as u32, h as u32))
}

pub(crate) fn render(source: &RgbaImage, angle: f32, size: Size) -> Result<RgbaImage, String> {
    let dimensions = preview_dimensions(source, angle, size)?;
    let mut image = canvas::oriented_pixels(source, angle)?;
    if image.dimensions() == dimensions {
        return Ok(image);
    }

    // Filter premultiplied color so transparent borders cannot bleed into visible pixels.
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut scaled = imageops::resize(&image, dimensions.0, dimensions.1, FilterType::Lanczos3);
    for pixel in scaled.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            let unscaled = u16::from(*channel) * 255 + alpha / 2;
            *channel = unscaled.checked_div(alpha).unwrap_or(0).min(255) as u8;
        }
    }
    Ok(scaled)
}

pub(crate) fn save_as(
    target: &Path,
    source: &RgbaImage,
    angle: f32,
    size: Size,
    replace: bool,
) -> Result<(), String> {
    let format = output_format(target)?;
    let original = target_metadata(target)?;
    if original.is_some() && !replace {
        return Err(
            "An image already exists there. Choose a new name or confirm replacement.".into(),
        );
    }
    let image = render(source, angle, size)?;
    write_file(target, original, |out| {
        encode(format, &image, Embedded::default(), out)
    })
}

pub(crate) fn save_original(
    target: &Path,
    source: &RgbaImage,
    edited: &RgbaImage,
    angle: f32,
) -> Result<(), String> {
    let format = output_format(target)?;
    let original = target_metadata(target)?.ok_or("The source image was removed.")?;
    let (embedded, packet) = inspect_original(target, &original, format, source)?;
    let image = render(edited, angle, Size::Original)?;
    write_file(target, Some(original), |out| match packet {
        Some(packet) => {
            let mut encoded = Vec::new();
            encode(format, &image, embedded, &mut encoded)?;
            metadata::write_reencoded_xmp(&encoded, &packet, out)
        }
        None => encode(format, &image, embedded, out),
    })
}

fn output_format(path: &Path) -> Result<Format, String> {
    Format::from_path(path).ok_or_else(|| "Choose a PNG, JPEG, or WebP filename in Save As.".into())
}

fn target_metadata(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            Err("Choose a regular image file. Linked files cannot be replaced.".into())
        }
        Ok(metadata) if metadata.permissions().readonly() => Err("This image is read-only.".into()),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(file_error(error)),
    }
}

fn write_file(
    target: &Path,
    original: Option<Metadata>,
    write: impl FnOnce(&mut File) -> Result<(), String>,
) -> Result<(), String> {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(file_error)?;
    write(tmp.as_file_mut())?;
    tmp.as_file_mut().flush().map_err(file_error)?;
    if let Some(original) = &original {
        tmp.as_file()
            .set_permissions(original.permissions())
            .map_err(file_error)?;
    }
    tmp.as_file().sync_all().map_err(file_error)?;
    if let Some(original) = original {
        let current = target_metadata(target)?.ok_or("The image was removed before saving.")?;
        if metadata::Revision::from(&current) != metadata::Revision::from(&original) {
            return Err("The image changed on disk. Open it again before saving.".into());
        }
        tmp.persist(target)
            .map_err(|error| file_error(error.error))?;
    } else {
        tmp.persist_noclobber(target)
            .map_err(|error| file_error(error.error))?;
    }
    Ok(())
}

#[derive(Default)]
struct Embedded {
    icc: Option<Vec<u8>>,
    exif: Option<Vec<u8>>,
    grayscale: bool,
}

fn inspect_original(
    target: &Path,
    original: &Metadata,
    format: Format,
    source: &RgbaImage,
) -> Result<(Embedded, Option<Vec<u8>>), String> {
    const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
    let file = File::open(target).map_err(file_error)?;
    if metadata::Revision::from(&file.metadata().map_err(file_error)?)
        != metadata::Revision::from(original)
    {
        return Err("The image changed on disk. Open it again before saving.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("This source is too large to replace. Use Save As.".into());
    }
    if image::guess_format(&bytes).ok() != Some(format.image_format()) {
        return Err("The source format and extension do not match. Use Save As.".into());
    }
    let packet = metadata::xmp_for_reencoding(&bytes)?;
    let input = Cursor::new(bytes.as_slice());
    let animated = "This source has multiple frames. Use Save As to export the displayed frame.";
    let mut decoder: Box<dyn ImageDecoder> = match format {
        Format::Png => {
            let decoder = PngDecoder::new(input).map_err(encode_error)?;
            if decoder.is_apng().map_err(encode_error)? {
                return Err(animated.into());
            }
            Box::new(decoder)
        }
        Format::Jpeg => Box::new(JpegDecoder::new(input).map_err(encode_error)?),
        Format::WebP => {
            let decoder = WebPDecoder::new(input).map_err(encode_error)?;
            if decoder.has_animation() {
                return Err(animated.into());
            }
            Box::new(decoder)
        }
    };
    if !matches!(
        decoder.color_type(),
        ColorType::Rgb8 | ColorType::Rgba8 | ColorType::L8 | ColorType::La8
    ) {
        return Err("This source uses a higher bit depth. Use Save As to export a copy.".into());
    }
    let grayscale = matches!(decoder.color_type(), ColorType::L8 | ColorType::La8);
    let (width, height) = decoder.dimensions();
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("The source changed dimensions. Open it again before saving.".into());
    }
    let icc = decoder.icc_profile().map_err(encode_error)?;
    let expected_color = if grayscale { b"GRAY" } else { b"RGB " };
    if icc
        .as_ref()
        .is_some_and(|icc| icc.get(16..20) != Some(expected_color))
    {
        return Err(
            "This source uses a different color space. Use Save As to export a copy.".into(),
        );
    }
    let mut exif = decoder.exif_metadata().map_err(encode_error)?;
    let orientation = decoder.orientation().map_err(encode_error)?;
    if let Some(exif) = exif.as_mut() {
        let _ = image::metadata::Orientation::remove_from_exif_chunk(exif);
    }
    let mut current = DynamicImage::from_decoder(decoder).map_err(encode_error)?;
    current.apply_orientation(orientation);
    if &current.into_rgba8() != source {
        return Err("The image changed on disk. Open it again before saving.".into());
    }
    Ok((
        Embedded {
            icc,
            exif,
            grayscale,
        },
        packet,
    ))
}

fn encode(
    format: Format,
    image: &RgbaImage,
    embedded: Embedded,
    output: impl Write,
) -> Result<(), String> {
    let (width, height) = image.dimensions();
    match format {
        Format::Png if embedded.grayscale => {
            let gray: Vec<u8> = image.pixels().flat_map(|p| [p[0], p[3]]).collect();
            write_pixels(
                PngEncoder::new(output),
                &gray,
                (width, height),
                ColorType::La8,
                embedded,
            )
        }
        Format::Png => write_pixels(
            PngEncoder::new(output),
            image.as_raw(),
            (width, height),
            ColorType::Rgba8,
            embedded,
        ),
        Format::WebP => write_pixels(
            WebPEncoder::new_lossless(output),
            image.as_raw(),
            (width, height),
            ColorType::Rgba8,
            embedded,
        ),
        Format::Jpeg => {
            let white_background = |color: u8, alpha: u8| {
                let alpha = u16::from(alpha);
                ((u16::from(color) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            };
            let (bytes, color_type) = if embedded.grayscale {
                (
                    image
                        .pixels()
                        .map(|p| white_background(p[0], p[3]))
                        .collect::<Vec<_>>(),
                    ColorType::L8,
                )
            } else {
                (
                    image
                        .pixels()
                        .flat_map(|p| {
                            [p[0], p[1], p[2]].map(|channel| white_background(channel, p[3]))
                        })
                        .collect(),
                    ColorType::Rgb8,
                )
            };
            write_pixels(
                JpegEncoder::new_with_quality(output, 92),
                &bytes,
                (width, height),
                color_type,
                embedded,
            )
        }
    }
}

fn write_pixels(
    mut encoder: impl ImageEncoder,
    bytes: &[u8],
    (width, height): (u32, u32),
    color_type: ColorType,
    embedded: Embedded,
) -> Result<(), String> {
    if let Some(icc) = embedded.icc {
        encoder.set_icc_profile(icc).map_err(encode_error)?;
    }
    if let Some(exif) = embedded.exif {
        encoder.set_exif_metadata(exif).map_err(encode_error)?;
    }
    encoder
        .write_image(bytes, width, height, color_type.into())
        .map_err(encode_error)
}

fn file_error(error: io::Error) -> String {
    format!("Could not save the image. {error}")
}

fn encode_error(error: impl std::fmt::Display) -> String {
    format!("Could not process the image. {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_uses_rotated_dimensions_and_does_not_bleed_transparent_rgb() {
        let image = RgbaImage::from_fn(6, 4, |x, _| {
            image::Rgba(if x < 3 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 0]
            })
        });
        assert_eq!(
            preview_dimensions(
                &image,
                90.,
                Size::Fit {
                    width: 2,
                    height: 9
                }
            )
            .unwrap(),
            (2, 3)
        );
        let result = render(&image, 90., Size::Percent(50)).unwrap();
        assert_eq!(result.dimensions(), (2, 3));
        assert!(result.pixels().all(|p| p[2] == 0));
        assert_eq!(image.dimensions(), (6, 4));
    }

    #[test]
    fn saving_checks_replacement_and_preserves_source_xmp() {
        let folder = tempfile::tempdir().unwrap();
        let dst = folder.path().join("original.png");
        let image = RgbaImage::from_pixel(8, 4, image::Rgba([7, 16, 99, 255]));
        save_as(&dst, &image, 0., Size::Original, false).unwrap();
        assert!(save_as(&dst, &image, 90., Size::Original, false).is_err());
        let edited = imageops::crop_imm(&image, 1, 1, 5, 2).to_image();
        assert!(save_original(&dst, &RgbaImage::new(8, 4), &edited, 90.).is_err());
        let packet = b"<xmp>keep this data</xmp>";
        let original = fs::read(&dst).unwrap();
        metadata::write_reencoded_xmp(&original, packet, &mut File::create(&dst).unwrap()).unwrap();
        save_original(&dst, &image, &edited, 90.).unwrap();
        assert_eq!(
            image::open(&dst).unwrap().into_rgba8(),
            imageops::rotate90(&edited)
        );
        assert_eq!(
            metadata::xmp_for_reencoding(&fs::read(&dst).unwrap())
                .unwrap()
                .as_deref(),
            Some(packet.as_slice())
        );
    }

    #[test]
    fn save_as_writes_jpeg_on_white_and_lossless_webp_with_alpha() {
        let folder = tempfile::tempdir().unwrap();
        let image = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 0]));
        let jpeg = folder.path().join("photo.jpg");
        let webp = folder.path().join("image.webp");
        save_as(&jpeg, &image, 0., Size::Original, false).unwrap();
        save_as(&webp, &image, 0., Size::Original, false).unwrap();
        assert_eq!(
            image::open(jpeg).unwrap().into_rgb8().get_pixel(0, 0).0,
            [255, 255, 255]
        );
        assert_eq!(
            image::open(webp).unwrap().into_rgba8().get_pixel(0, 0)[3],
            0
        );
    }
}
