//! Opens originals in a compatible Bediz installation.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Bediz {
    target: Target,
    file_open: bool,
}

#[derive(Clone, Debug)]
enum Target {
    #[cfg(target_os = "macos")]
    Bundle(PathBuf),
    #[cfg(target_os = "windows")]
    Executable(PathBuf),
    #[cfg(target_os = "windows")]
    Store,
}

pub fn detect() -> Option<Bediz> {
    let candidates = platform::candidates();
    candidates
        .iter()
        .find(|candidate| candidate.file_open)
        .cloned()
        .or_else(|| candidates.into_iter().next())
}

impl Bediz {
    pub fn supports_file_open(&self) -> bool {
        self.file_open
    }

    pub fn open(&self, path: &Path) -> Result<(), String> {
        if !self.file_open {
            return Err("This Bediz version needs an update for direct file opening.".into());
        }
        if !self.can_open_path(path) {
            return Err("Bediz can't open this file type directly. Copy the image first.".into());
        }
        let path = path
            .canonicalize()
            .map_err(|error| format!("Could not locate the image: {error}"))?;
        if !path.is_file() {
            return Err("Choose an image file to open in Bediz.".into());
        }
        platform::launch(&self.target, Some(&path))
    }

    pub fn can_open_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                [
                    "png", "jpeg", "jpg", "webp", "gif", "bmp", "tif", "tiff", "heif", "heic",
                    "myimage", "snagx",
                ]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
            })
    }

    /// Brings an older Bediz forward for an explicitly requested clipboard handoff.
    pub fn show(&self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        if !self.file_open
            && let Some(result) = platform::show_existing()
        {
            return result;
        }
        platform::launch(&self.target, None)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    const APP_NAMES: [&str; 3] = ["Bediz.app", "SkyImage.app", "MyImage.app"];

    fn app(path: PathBuf) -> Option<Bediz> {
        if !path.join("Contents/MacOS/MyImage").is_file() {
            return None;
        }
        let value = |key| {
            Command::new("/usr/libexec/PlistBuddy")
                .args(["-c", key])
                .arg(path.join("Contents/Info.plist"))
                .output()
                .ok()
                .filter(|result| result.status.success())
                .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        };
        if value("Print CFBundleIdentifier").as_deref() != Some("com.myimage.app") {
            return None;
        }
        let file_open = value("Print BedizOpenProtocol").as_deref() == Some("1");
        Some(Bediz {
            target: Target::Bundle(path),
            file_open,
        })
    }

    pub(super) fn candidates() -> Vec<Bediz> {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(dirs) = directories::BaseDirs::new() {
            roots.push(dirs.home_dir().join("Applications"));
        }
        std::env::var_os("BEDIZ_APP")
            .map(PathBuf::from)
            .into_iter()
            .chain(
                roots
                    .into_iter()
                    .flat_map(|dir| APP_NAMES.map(|name| dir.join(name))),
            )
            .filter_map(app)
            .collect()
    }

    pub(super) fn launch(target: &Target, path: Option<&Path>) -> Result<(), String> {
        let Target::Bundle(bundle) = target;
        let mut command = Command::new("/usr/bin/open");
        if path.is_some() {
            // LaunchServices only delivers --args on a new launch. The new
            // process acts as a messenger when a compatible editor is running.
            command.arg("-n");
        }
        command.arg("-a").arg(bundle);
        if let Some(path) = path {
            command.args(["--args", "--bediz-open"]).arg(path);
        }
        let result = command
            .output()
            .map_err(|error| format!("Could not open Bediz: {error}"))?;
        if result.status.success() {
            Ok(())
        } else {
            let error = String::from_utf8_lossy(&result.stderr);
            Err(format!("Could not open Bediz: {}", error.trim()))
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_executable(path: PathBuf) -> Option<PathBuf> {
    const NAMES: [&str; 2] = ["Bediz.exe", "MyImage.exe"];

    if path.is_dir() {
        return NAMES
            .into_iter()
            .map(|name| path.join(name))
            .find(|candidate| candidate.is_file());
    }

    let name = path.file_name()?.to_str()?;
    (path.is_file()
        && NAMES
            .iter()
            .any(|supported| name.eq_ignore_ascii_case(supported)))
    .then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_directory_prefers_bediz_and_accepts_legacy_installs() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = directory.path().join("MyImage.exe");
        let current = directory.path().join("Bediz.exe");

        std::fs::write(&legacy, []).unwrap();
        assert_eq!(
            windows_executable(directory.path().to_owned()),
            Some(legacy.clone())
        );

        std::fs::write(&current, []).unwrap();
        assert_eq!(
            windows_executable(directory.path().to_owned()),
            Some(current.clone())
        );
        assert_eq!(windows_executable(current.clone()), Some(current));
        assert_eq!(windows_executable(legacy.clone()), Some(legacy));
    }

    #[test]
    fn windows_detection_ignores_other_names_and_missing_files() {
        let directory = tempfile::tempdir().unwrap();
        let other = directory.path().join("other.exe");
        let missing = directory.path().join("Bediz.exe");
        std::fs::write(&other, []).unwrap();

        assert_eq!(windows_executable(other), None);
        assert_eq!(windows_executable(missing), None);
        assert_eq!(windows_executable(directory.path().to_owned()), None);
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::{
        fs,
        os::windows::ffi::{OsStrExt, OsStringExt},
        process::Command,
    };
    use windows::{
        Win32::{
            Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
            Storage::Packaging::Appx::{GetPackagePathByFullName, GetPackagesByPackageFamily},
            System::{
                Com::{
                    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoUninitialize,
                },
                Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
            },
            UI::{
                Shell::{AO_NONE, ApplicationActivationManager, IApplicationActivationManager},
                WindowsAndMessaging::{
                    FindWindowW, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
                    ShowWindowAsync,
                },
            },
        },
        core::{PCWSTR, PWSTR, w},
    };

    // The published Microsoft Store product's package family and app ID.
    const FAMILY: PCWSTR = w!("OrhanGkl.MyImageOG_vq1xk3rng529a");
    const APP_ID: PCWSTR = w!("OrhanGkl.MyImageOG_vq1xk3rng529a!MyImage");

    fn has_protocol(directory: &Path) -> bool {
        fs::read_to_string(directory.join("bediz-open-protocol.txt"))
            .is_ok_and(|text| text.trim() == "1")
    }

    fn file(path: PathBuf) -> Option<Bediz> {
        let path = windows_executable(path)?;
        Some(Bediz {
            file_open: has_protocol(path.parent()?),
            target: Target::Executable(path),
        })
    }

    fn installer_path() -> Option<PathBuf> {
        const UNINSTALL_KEY: PCWSTR = w!(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{B9CF57F1-A083-4B7D-BE98-B1F0C466EBB4}_is1"
        );
        let mut path = vec![0u16; 32768];
        let mut byte_count = (path.len() * 2) as u32;
        // SAFETY: The byte count describes the writable buffer and the value is a string.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                UNINSTALL_KEY,
                w!("InstallLocation"),
                RRF_RT_REG_SZ,
                None,
                Some(path.as_mut_ptr().cast()),
                Some(&mut byte_count),
            )
        };
        if result != ERROR_SUCCESS {
            return None;
        }
        let end = path.iter().position(|word| *word == 0)?;
        let path = PathBuf::from(std::ffi::OsString::from_wide(&path[..end]));
        path.is_absolute().then_some(path)
    }

    fn store() -> Option<Bediz> {
        let mut count = 0;
        let mut length = 0;
        // SAFETY: Query with null output pointers to get the buffer sizes.
        let result =
            unsafe { GetPackagesByPackageFamily(FAMILY, &mut count, None, &mut length, None) };
        if result != ERROR_INSUFFICIENT_BUFFER
            || !(1..=64).contains(&count)
            || !(1..=16384).contains(&length)
        {
            return None;
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: Both buffers were sized using the API result above.
        let result = unsafe {
            GetPackagesByPackageFamily(
                FAMILY,
                &mut count,
                Some(names.as_mut_ptr()),
                &mut length,
                Some(PWSTR(buffer.as_mut_ptr())),
            )
        };
        if result != ERROR_SUCCESS || count == 0 {
            return None;
        }
        let compatible = names.into_iter().take(count as usize).any(|name| {
            if name.is_null() {
                return false;
            }
            let mut length = 0;
            // SAFETY: name points into the still-live buffer owned by this function.
            let result = unsafe { GetPackagePathByFullName(PCWSTR(name.0), &mut length, None) };
            if result != ERROR_INSUFFICIENT_BUFFER || !(2..=32768).contains(&length) {
                return false;
            }
            let mut path = vec![0u16; length as usize];
            // SAFETY: path contains length writable UTF-16 words.
            let result = unsafe {
                GetPackagePathByFullName(
                    PCWSTR(name.0),
                    &mut length,
                    Some(PWSTR(path.as_mut_ptr())),
                )
            };
            if result != ERROR_SUCCESS {
                return false;
            }
            let end = path
                .iter()
                .position(|word| *word == 0)
                .unwrap_or(path.len());
            has_protocol(Path::new(&std::ffi::OsString::from_wide(&path[..end])))
        });
        Some(Bediz {
            file_open: compatible,
            target: Target::Store,
        })
    }

    pub(super) fn candidates() -> Vec<Bediz> {
        std::env::var_os("BEDIZ_APP")
            .map(PathBuf::from)
            .into_iter()
            .chain(installer_path())
            .chain(
                std::env::var_os("LOCALAPPDATA")
                    .map(|dir| PathBuf::from(dir).join("Programs/MyImage")),
            )
            .filter_map(file)
            .chain(store())
            .collect()
    }

    pub(super) fn show_existing() -> Option<Result<(), String>> {
        let window = [w!("Bediz"), w!("SkyImage"), w!("MyImage")]
            .into_iter()
            // SAFETY: The names are static UTF-16 and the class filter is optional.
            .find_map(|title| unsafe { FindWindowW(PCWSTR::null(), title) }.ok())?;
        // SAFETY: These operations use an OS-issued handle from FindWindowW.
        if !unsafe { IsWindowVisible(window) }.as_bool() {
            return Some(Err(
                "Image copied. Open Bediz from its tray icon, then paste.".into(),
            ));
        }
        if unsafe { IsIconic(window) }.as_bool() {
            let _ = unsafe { ShowWindowAsync(window, SW_RESTORE) };
        }
        if unsafe { SetForegroundWindow(window) }.as_bool() {
            Some(Ok(()))
        } else {
            Some(Err("Image copied. Switch to Bediz, then paste.".into()))
        }
    }

    fn launch_store(path: Option<&Path>) -> Result<(), String> {
        let mut arguments: Vec<u16> = Vec::new();
        if let Some(path) = path {
            arguments.extend("--bediz-open \"".encode_utf16());
            arguments.extend(path.as_os_str().encode_wide());
            arguments.push(b'"' as u16);
        }
        arguments.push(0);
        // SAFETY: Initialize COM on the current thread before activating the
        // packaged Win32 application registered for this user.
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let balance = initialized.is_ok();
        if initialized.is_err() && initialized != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
            return Err(format!(
                "Could not initialize Bediz activation: {initialized:?}"
            ));
        }
        let result = (|| {
            let manager: IApplicationActivationManager = unsafe {
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
            }?;
            unsafe { manager.ActivateApplication(APP_ID, PCWSTR(arguments.as_ptr()), AO_NONE) }
        })();
        if balance {
            // SAFETY: Balance this thread's successful CoInitializeEx call.
            unsafe { CoUninitialize() };
        }
        result
            .map(|_| ())
            .map_err(|error| format!("Could not open Store Bediz: {error}"))
    }

    pub(super) fn launch(target: &Target, path: Option<&Path>) -> Result<(), String> {
        match target {
            Target::Executable(executable) => {
                let mut command = Command::new(executable);
                if let Some(path) = path {
                    command.arg("--bediz-open").arg(path);
                }
                command
                    .spawn()
                    .map(|_| ())
                    .map_err(|error| format!("Could not open Bediz: {error}"))
            }
            Target::Store => launch_store(path),
        }
    }
}
