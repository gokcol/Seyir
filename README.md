# Seyir 1.0

Seyir is a native image viewer for macOS and Windows. Open one image and use the arrow keys to browse its neighbors. **Show Folder** opens a sidebar with thumbnails and selects the image already on screen.

- Hover over **File** to open an image or folder, save, export, or print. You can also drop an image into the window or open it from Finder or Explorer. The folder browser starts at your home folder and follows the parent of the image you open.
- **← / →** browse files in the folder using the selected order. The folder sidebar can sort by natural filename order or most recently modified.
- **Fit** or **1:1** shows the fitted image or its actual pixels. Zoom from 10% to 1000% with the toolbar or scroll over the image. Drag to pan. Click **Rotate** to turn the image 90° clockwise. Hover over it for angle controls, Crop, Flip Horizontal, Flip Vertical, and Undo Image Edit. From the focused Rotate button, Shift+F10 opens the same controls by keyboard.
- **Copy Image** copies the full-size image at its displayed angle. Hover over Copy for **Copy Resized…**. **Save As…** exports PNG, JPEG, or WebP with the same resize choices. Both offer percentages and custom dimensions that preserve the aspect ratio.
- **Save Image** applies the displayed edits to supported single-image PNG, JPEG, and WebP originals after confirmation. **Print** opens the system print dialog for the displayed image.
- **Image Properties** shows dimensions, format, file size, modification date, and available EXIF or color profile information. Reveal the source in Finder or Explorer from the same panel.
- **Save metadata** writes title, description, author, copyright, and comma-separated keywords as embedded XMP in JPEG, PNG, and WebP. It preserves the encoded image and unrelated metadata. Changes are applied atomically and stale edits are rejected. The Info panel explains when a file cannot be edited.
- **Edit in Bediz** sends supported originals to a Bediz build with file handoff support. For viewer-only formats and manual rotation, choose **Open rendered PNG copy**. Installed older builds offer **Copy and open Bediz**, followed by Paste.
- Right-click an image to navigate, choose a transition, rotate, copy, edit in Bediz, save, or print.
- **Slideshow** plays the current folder in order. Change the interval, loop, and transition while it plays. Escape ends the show. **Full Screen** hides the viewer controls and leaves the image context menu available. Escape or F11 returns to the window.

Open **Settings** from the gear, or with **⌘+,** on macOS or **Ctrl+,** on Windows. **Interface** switches among six layouts and controls the folder and properties panels. **Floating Controls** provides an icon-only dock with a separate **More Tools** panel. Image Properties starts off by default. **Playback** sets folder and slideshow transitions independently, along with the slideshow interval and looping.

## Formats

Seyir ships decoding for JPEG, PNG, WebP, GIF, TIFF, BMP, ICO, PNM, TGA, DDS, Radiance HDR, farbfeld, and SVG/SVGZ. It displays the first frame or page of animated and multi-page files. Native codecs can add HEIC/HEIF, AVIF, JPEG 2000, PSD composites, camera RAW, and other formats available on the system. Windows codec coverage can vary with installed image extensions. Unsupported files report an error in the viewer.

Decoding and indexing run off the UI thread. A bounded image cache and nearby-image loading keep browsing responsive. Folder thumbnails load as they scroll into view. Display textures for very large graphics may be downsampled to fit the GPU, while the full decoded image remains available for copying.

## Open With

- **macOS.** Put `Seyir.app` in Applications. In Finder, right-click an image and choose **Open With → Seyir**. Finder lists Seyir as an alternate viewer.
- **Windows.** Extract the portable ZIP to the folder where you want to keep Seyir. Double-click **Register Open With.cmd** once, then right-click an image in Explorer and choose **Open with → Seyir**. Run the registration again from the new folder if you move Seyir. Use **Remove Open With.cmd** before removing the app. Registration applies to your Windows account and preserves your current defaults.

## Build

Use Rust 1.93.1 or newer. Run `cargo run --locked` from this repository. Pass an image path after `--` to open it at launch.

| Platform | Native package |
| --- | --- |
| macOS 14 or later | Run `scripts/bundle-macos.sh` to build a locally signed `Seyir.app` and ZIP in `dist/`. |
| Windows x64 | Run `cargo build --locked --release` to produce `target/release/seyir.exe`. |

The **Native builds** GitHub Actions workflow runs tests on both platforms and uploads the Mac application and Windows portable ZIP. GitHub requires sign-in to download workflow artifacts.

For a source check, run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked --all-targets`.
