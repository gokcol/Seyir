//! Decode HEIF and other optional formats through codecs installed on Windows.
use super::validate_dimensions;
use image::RgbaImage;
use std::time::{Duration, Instant};
use windows::{
    Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode,
        ExifOrientationMode,
    },
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

const TIMEOUT: Duration = Duration::from_secs(30);

// The generated APIs use distinct WinRT operation types. Keep each operation
// concrete and avoid introducing an additional COM dependency for waiting.
macro_rules! wait {
    ($operation:expr, $deadline:expr) => {{
        let operation = $operation.map_err(codec_error)?;
        loop {
            if operation.Status().map_err(codec_error)?.0 != 0 {
                break;
            }
            if Instant::now() >= $deadline {
                let _ = operation.Cancel();
                return Err("The Windows image codec timed out.".into());
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        operation.GetResults().map_err(codec_error)?
    }};
}

pub(super) fn decode(bytes: &[u8]) -> Result<RgbaImage, String> {
    // The decode worker owns this apartment. Declare it before WinRT handles so
    // those handles finish dropping before the worker uninitializes COM.
    let _apartment = Apartment::new()?;
    let deadline = Instant::now() + TIMEOUT;
    let stream = InMemoryRandomAccessStream::new().map_err(codec_error)?;
    let output = stream.GetOutputStreamAt(0).map_err(codec_error)?;
    let writer = DataWriter::CreateDataWriter(&output).map_err(codec_error)?;
    writer.WriteBytes(bytes).map_err(codec_error)?;
    let written = wait!(writer.StoreAsync(), deadline);
    if written as usize != bytes.len() {
        return Err("Windows could not load the full image file.".into());
    }
    writer.DetachStream().map_err(codec_error)?;
    stream.Seek(0).map_err(codec_error)?;
    let decoder = wait!(BitmapDecoder::CreateAsync(&stream), deadline);
    validate_dimensions(
        decoder.PixelWidth().map_err(codec_error)?,
        decoder.PixelHeight().map_err(codec_error)?,
    )?;
    let width = decoder.OrientedPixelWidth().map_err(codec_error)?;
    let height = decoder.OrientedPixelHeight().map_err(codec_error)?;
    validate_dimensions(width, height)?;

    let transform = BitmapTransform::new().map_err(codec_error)?;
    let provider = wait!(
        decoder.GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb
        ),
        deadline
    );
    let data = provider.DetachPixelData().map_err(codec_error)?;
    if data.len() != width as usize * height as usize * 4 {
        return Err("The Windows image codec returned an incomplete pixel buffer.".into());
    }
    RgbaImage::from_raw(width, height, data.to_vec())
        .ok_or_else(|| "The Windows image codec returned an invalid pixel buffer.".into())
}

fn codec_error(error: windows::core::Error) -> String {
    format!("Windows could not use its image codec. {error}")
}

struct Apartment;

impl Apartment {
    fn new() -> Result<Self, String> {
        // SAFETY: This worker has its own apartment. Each successful call is
        // balanced once by Drop, including the already-initialized success code.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(codec_error)?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after successful initialization.
        unsafe { RoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    #[test]
    fn installed_windows_codecs_preserve_rgba_layout() {
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
        assert_eq!(decoded.as_raw(), &[255, 0, 0, 128, 0, 0, 255, 255]);
    }
}
