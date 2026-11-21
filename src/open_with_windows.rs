//! Register Seyir as an optional image handler for the current Windows user.
use std::{env, ffi::OsStr, io, os::windows::ffi::OsStrExt, path::Path, process};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegSetKeyValueW,
        },
        UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify},
    },
    core::PCWSTR,
};

const CLASSES: &str = r"Software\Classes";
const APPLICATION: &str = r"Software\Classes\Applications\seyir.exe";
const PROG_ID: &str = "Seyir.Image.1";
// Advertise the built-in decoders. Optional Windows codecs vary by machine.
const EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".jfif", ".png", ".apng", ".webp", ".gif", ".tif", ".tiff", ".bmp", ".ico",
    ".pnm", ".pbm", ".pgm", ".ppm", ".pam", ".tga", ".dds", ".hdr", ".ff", ".svg", ".svgz",
];

pub(crate) fn run_if_requested() {
    let mut args = env::args_os().skip(1);
    let Some(action) = args.next() else { return };
    if args.next().is_some() {
        return;
    }
    let result = if action == "--register-open-with" {
        env::current_exe().and_then(|exe| register(&exe))
    } else if action == "--unregister-open-with" {
        unregister()
    } else {
        return;
    };

    // SAFETY: The association-change event uses no item pointers.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    if let Err(error) = result {
        eprintln!("Could not update Seyir’s Open with registration: {error}");
        process::exit(1);
    }
    process::exit(0);
}

fn register(executable: &Path) -> io::Result<()> {
    let prog_id = format!(r"{CLASSES}\{PROG_ID}");
    let app_name = wide("Seyir");
    let command = executable_value(executable.as_os_str(), " \"%1\"");
    let icon = executable_value(executable.as_os_str(), ",0");
    set_string(&prog_id, "", &app_name)?;
    set_string(APPLICATION, "FriendlyAppName", &app_name)?;
    for key in [&*prog_id, APPLICATION] {
        set_string(&format!(r"{key}\DefaultIcon"), "", &icon)?;
        set_string(&format!(r"{key}\shell\open\command"), "", &command)?;
    }
    // Register only the named OpenWithProgids values, never the extension default.
    let supported_types = format!(r"{APPLICATION}\SupportedTypes");
    for extension in EXTENSIONS {
        set_string(&supported_types, extension, &[0])?;
        set_string(&open_with_key(extension), PROG_ID, &[0])?;
    }
    Ok(())
}

fn unregister() -> io::Result<()> {
    let name = wide(PROG_ID);
    for extension in EXTENSIONS {
        let key = wide(&open_with_key(extension));
        // SAFETY: Both strings are terminated and live through the call.
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
            )
        };
        allow_missing(status)?;
    }
    // Only the application-specific entries are deleted as trees. Shared file
    // extension keys and values registered by other applications stay in place.
    for key in [wide(&format!(r"{CLASSES}\{PROG_ID}")), wide(APPLICATION)] {
        // SAFETY: The key is a terminated UTF-16 string.
        allow_missing(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) })?;
    }
    Ok(())
}

fn open_with_key(extension: &str) -> String {
    format!(r"{CLASSES}\{extension}\OpenWithProgids")
}

fn executable_value(executable: &OsStr, suffix: &str) -> Vec<u16> {
    // Encode the original Windows path directly, including unpaired surrogates.
    std::iter::once(u16::from(b'"'))
        .chain(executable.encode_wide())
        .chain("\"".encode_utf16())
        .chain(suffix.encode_utf16())
        .chain([0])
        .collect()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn set_string(key: &str, name: &str, data: &[u16]) -> io::Result<()> {
    let key = wide(key);
    let name = wide(name);
    // SAFETY: The strings are terminated UTF-16 and the length covers the
    // full REG_SZ buffer, including its final null.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            size_of_val(data) as u32,
        )
    };
    io_result(status)
}

fn allow_missing(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    io_result(status)
}

fn io_result(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_command_quotes_both_paths() {
        let exe = OsStr::new(r"C:\Seyir (portable) & Görüntü\seyir.exe");
        assert_eq!(
            executable_value(exe, " \"%1\""),
            wide(r#""C:\Seyir (portable) & Görüntü\seyir.exe" "%1""#),
        );
    }
}
