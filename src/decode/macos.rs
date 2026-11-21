//! Direct ImageIO/Core Graphics decoding for formats supplied by macOS.
use super::{straight_pixels, validate_dimensions};
use image::RgbaImage;
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL, CGPoint,
    CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceThumbnailMaxPixelSize,
};
use std::{os::unix::ffi::OsStrExt, path::Path};

pub(super) fn decode(bytes: &[u8]) -> Result<RgbaImage, String> {
    let data = CFData::from_bytes(bytes);
    // SAFETY: ImageIO receives immutable encoded image bytes and no options.
    let source = unsafe { CGImageSource::with_data(&data, None) }
        .ok_or("ImageIO could not open the file.")?;
    read_source(&source, None)
}

pub(super) fn thumbnail(path: &Path, max_edge: u32) -> Result<RgbaImage, String> {
    let path = std::path::absolute(path).map_err(|error| error.to_string())?;
    let bytes = path.as_os_str().as_bytes();
    let length = isize::try_from(bytes.len()).map_err(|_| "The image path is too long.")?;
    // SAFETY: The immutable path buffer contains exactly length bytes.
    let url =
        unsafe { CFURL::from_file_system_representation(None, bytes.as_ptr(), length, false) }
            .ok_or("ImageIO could not read this path.")?;
    // SAFETY: ImageIO receives a retained local file URL and no options.
    let source =
        unsafe { CGImageSource::with_url(&url, None) }.ok_or("ImageIO could not open the file.")?;
    read_source(&source, Some(max_edge))
}

fn read_source(source: &CGImageSource, max_edge: Option<u32>) -> Result<RgbaImage, String> {
    // SAFETY: The count needs no options or indexing and source is retained.
    if unsafe { source.count() } == 0 {
        return Err("ImageIO found no image in the file.".into());
    }
    // SAFETY: The source has at least one frame. ImageIO declares the property
    // dictionary keys as CFString. Values are checked before extracting numbers.
    let properties = unsafe { source.properties_at_index(0, None) }
        .ok_or("ImageIO could not read image dimensions.")?;
    let properties: CFRetained<CFDictionary<CFString, CFType>> =
        unsafe { CFRetained::cast_unchecked(properties) };
    let dimension = |key: &CFString| {
        properties
            .get(key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
            .and_then(|number| u32::try_from(number).ok())
            .ok_or("ImageIO returned invalid image dimensions.")
    };
    // SAFETY: These documented property keys are immutable CFStrings.
    let width = dimension(unsafe { kCGImagePropertyPixelWidth })?;
    let height = dimension(unsafe { kCGImagePropertyPixelHeight })?;
    validate_dimensions(width, height)?;

    // Use the full source to honor the requested detail and orientation.
    let side = width.max(height);
    let side = CFNumber::new_i64(i64::from(side.min(max_edge.unwrap_or(side))));
    let yes = CFBoolean::new(true);
    let options = CFDictionary::<CFString, CFType>::from_slices(
        // SAFETY: ImageIO exports these immutable CFString option keys.
        &unsafe {
            [
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ]
        },
        &[yes.as_ref(), yes.as_ref(), side.as_ref()],
    );
    // SAFETY: Index zero was validated. The option dictionary has exactly the
    // documented Boolean and CFNumber values, retained throughout this call.
    let image = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }
        .ok_or("The installed macOS codec could not decode this image.")?;
    let width = u32::try_from(CGImage::width(Some(&image)))
        .map_err(|_| "ImageIO returned an oversized image.")?;
    let height = u32::try_from(CGImage::height(Some(&image)))
        .map_err(|_| "ImageIO returned an oversized image.")?;
    validate_dimensions(width, height)?;

    // SAFETY: The exported name is Apple's retained standard sRGB color space.
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or("Could not create an sRGB color space.")?;
    let stride = width as usize * 4;
    let mut pixels = vec![0; stride * height as usize];
    // SAFETY: The vector is initialized, large enough for the validated image,
    // and the pointer stays valid without aliases until the context is dropped.
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            width as usize,
            height as usize,
            8,
            stride,
            Some(&srgb),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )
    }
    .ok_or("Could not allocate the native image buffer.")?;
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(f64::from(width), f64::from(height)),
        ),
        Some(&image),
    );
    drop(context);
    straight_pixels(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    #[test]
    fn draws_native_rgba_in_top_left_order() {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 0, 128, 0, 0, 255, 255],
                1,
                2,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!(decoded.dimensions(), (1, 2));
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(decoded.get_pixel(0, 1).0, [0, 0, 255, 255]);
    }
}
