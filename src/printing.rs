//! Native one-page printing of the rendered image.
use image::RgbaImage;

pub(crate) fn print(pixels: &RgbaImage, title: &str) -> Result<bool, String> {
    if pixels.width() == 0 || pixels.height() == 0 {
        return Err("There are no image pixels to print.".into());
    }
    platform::print(pixels, title)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::RgbaImage;
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSImage, NSImageScaling, NSImageView, NSPaperOrientation, NSPrintInfo, NSPrintOperation,
        NSPrintingPaginationMode,
    };
    use objc2_core_foundation::CFData;
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB,
    };
    use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize, NSString};

    pub(super) fn print(pixels: &RgbaImage, title: &str) -> Result<bool, String> {
        let mtm = MainThreadMarker::new().ok_or("Printing must start on the main thread.")?;
        if NSPrintOperation::currentOperation(mtm).is_some() {
            return Err("A print operation is already open.".into());
        }
        let data = CFData::from_bytes(pixels.as_raw());
        let provider = CGDataProvider::with_cf_data(Some(&data))
            .ok_or("Could not prepare the image for printing.")?;
        // SAFETY: Core Graphics exports this immutable color space name.
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
            .ok_or("Could not prepare the print color space.")?;
        let (width, height) = (pixels.width() as usize, pixels.height() as usize);
        let bitmap = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);
        // SAFETY: The provider retains tightly packed immutable RGBA8 data. No decode table is used.
        let image = unsafe {
            CGImage::new(
                width,
                height,
                8,
                32,
                width * 4,
                Some(&space),
                bitmap,
                Some(&provider),
                std::ptr::null(),
                true,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .ok_or("Could not create the printable image.")?;

        let info = NSPrintInfo::sharedPrintInfo().copy();
        info.setOrientation(if width > height {
            NSPaperOrientation::Landscape
        } else {
            NSPaperOrientation::Portrait
        });
        info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        info.setVerticalPagination(NSPrintingPaginationMode::Fit);
        info.setHorizontallyCentered(true);
        info.setVerticallyCentered(true);

        // Fit an aspect-matched view to the selected paper without resampling the source image.
        let paper = info.paperSize();
        let scale = paper.width.max(paper.height).max(1.) / width.max(height) as f64;
        let size = NSSize::new(width as f64 * scale, height as f64 * scale);
        let image = NSImage::initWithCGImage_size(NSImage::alloc(), &image, size);
        let view =
            NSImageView::initWithFrame(NSImageView::alloc(mtm), NSRect::new(NSPoint::ZERO, size));
        view.setImage(Some(&image));
        view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        let job = NSPrintOperation::printOperationWithView_printInfo(&view, &info);
        job.setJobTitle(Some(&NSString::from_str(title)));
        job.setShowsPrintPanel(true);
        Ok(job.runOperation())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::RgbaImage;
    use windows::{
        Win32::{
            Foundation::GlobalFree,
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps,
                HALFTONE, HDC, HORZRES, LOGPIXELSX, LOGPIXELSY, SP_USERABORT, SRCCOPY,
                SetBrushOrgEx, SetStretchBltMode, StretchDIBits, VERTRES,
            },
            Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
            UI::{
                Controls::Dialogs::{
                    CommDlgExtendedError, PD_HIDEPRINTTOFILE, PD_NOPAGENUMS, PD_NOSELECTION,
                    PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW,
                },
                Input::KeyboardAndMouse::GetActiveWindow,
            },
        },
        core::PCWSTR,
    };

    struct Dialog(PRINTDLGW);

    impl Drop for Dialog {
        fn drop(&mut self) {
            let PRINTDLGW {
                hDC,
                hDevMode,
                hDevNames,
                ..
            } = self.0;
            // SAFETY: These resources were allocated by PrintDlgW and remain owned by this dialog.
            unsafe {
                if !hDC.is_invalid() {
                    let _ = DeleteDC(hDC);
                }
                if !hDevMode.is_invalid() {
                    let _ = GlobalFree(Some(hDevMode));
                }
                if !hDevNames.is_invalid() {
                    let _ = GlobalFree(Some(hDevNames));
                }
            }
        }
    }

    pub(super) fn print(pixels: &RgbaImage, title: &str) -> Result<bool, String> {
        let mut dialog = Dialog(PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            // SAFETY: Queries the active window for the calling GUI thread.
            hwndOwner: unsafe { GetActiveWindow() },
            Flags: PD_RETURNDC
                | PD_NOPAGENUMS
                | PD_NOSELECTION
                | PD_HIDEPRINTTOFILE
                | PD_USEDEVMODECOPIESANDCOLLATE,
            nCopies: 1,
            ..Default::default()
        });
        // SAFETY: The dialog structure is initialized with a valid size and no callbacks.
        if !unsafe { PrintDlgW(&mut dialog.0) }.as_bool() {
            // SAFETY: Reads the result of the failed dialog call on this thread.
            let code = unsafe { CommDlgExtendedError() }.0;
            return if code == 0 {
                Ok(false)
            } else {
                Err(format!(
                    "Could not open the Windows print dialog (error {code:#x})."
                ))
            };
        }

        write_page(dialog.0.hDC, pixels, title)
    }

    fn write_page(dc: HDC, pixels: &RgbaImage, title: &str) -> Result<bool, String> {
        if dc.is_invalid() {
            return Err("The printer did not return a device context.".into());
        }
        let width = i32::try_from(pixels.width()).map_err(|_| "The image is too wide to print.")?;
        let height =
            i32::try_from(pixels.height()).map_err(|_| "The image is too tall to print.")?;
        // SAFETY: The dialog owns the valid printer DC for all queries and drawing below.
        let (area_width, area_height, dpi_x, dpi_y) = unsafe {
            (
                GetDeviceCaps(Some(dc), HORZRES),
                GetDeviceCaps(Some(dc), VERTRES),
                GetDeviceCaps(Some(dc), LOGPIXELSX).max(1),
                GetDeviceCaps(Some(dc), LOGPIXELSY).max(1),
            )
        };
        if area_width <= 0 || area_height <= 0 {
            return Err("The printer did not report a printable page area.".into());
        }
        let margin_x = (dpi_x / 4).min(area_width / 4);
        let margin_y = (dpi_y / 4).min(area_height / 4);
        let (scaled_w, scaled_h) = (
            f64::from(width) * f64::from(dpi_x),
            f64::from(height) * f64::from(dpi_y),
        );
        let scale = (f64::from(area_width - margin_x * 2) / scaled_w)
            .min(f64::from(area_height - margin_y * 2) / scaled_h);
        let draw_width = ((scaled_w * scale).round() as i32).max(1);
        let draw_height = ((scaled_h * scale).round() as i32).max(1);

        let mut bgrx = Vec::with_capacity(pixels.as_raw().len());
        for pixel in pixels.pixels() {
            let [red, green, blue, alpha] = pixel.0;
            let alpha = u16::from(alpha);
            let on_white = |channel: u8| {
                ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            };
            bgrx.extend([on_white(blue), on_white(green), on_white(red), 0]);
        }
        let bitmap = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let name: Vec<u16> = title.encode_utf16().chain([0]).collect();
        let document = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: The job takes a live DC and a document with a NUL-terminated name.
        let job = unsafe { StartDocW(dc, &document) };
        if job == SP_USERABORT {
            return Ok(false);
        }
        if job <= 0 {
            return Err("Could not start the print job.".into());
        }
        // SAFETY: The bitmap is a tightly packed top-down BGRX buffer matching its header.
        // StartPage and EndPage surround drawing, and any partial job is aborted on failure.
        unsafe {
            if StartPage(dc) <= 0 {
                AbortDoc(dc);
                return Err("Could not start the printed page.".into());
            }
            SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
            let lines = StretchDIBits(
                dc,
                (area_width - draw_width) / 2,
                (area_height - draw_height) / 2,
                draw_width,
                draw_height,
                0,
                0,
                width,
                height,
                Some(bgrx.as_ptr().cast()),
                &bitmap,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            if lines <= 0 || EndPage(dc) <= 0 || EndDoc(dc) <= 0 {
                AbortDoc(dc);
                return Err("Could not complete the print job.".into());
            }
        }
        Ok(true)
    }
}
