//! Query the Microsoft Store for updates to the current Store package.

use super::{STORE_UPDATES_URL, Update};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::{
    ApplicationModel::{Package, PackageSignatureKind},
    Services::Store::StoreContext,
    Win32::{
        Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, ERROR_TIMEOUT,
            HWND,
        },
        Storage::Packaging::Appx::GetCurrentPackageFullName,
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        UI::{
            Shell::{IInitializeWithWindow, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::{HSTRING, Interface, PCWSTR, w},
};

const TIMEOUT: Duration = Duration::from_secs(25);

struct WinRtApartment;

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        // SAFETY: This value is only created after a successful initialization on this thread.
        unsafe { RoUninitialize() };
    }
}

pub(super) fn check(parent: Option<isize>) -> Result<Update, String> {
    let mut size = 0;
    // SAFETY: The null output buffer queries the required size and package identity.
    match unsafe { GetCurrentPackageFullName(&mut size, None) } {
        APPMODEL_ERROR_NO_PACKAGE => return Ok(Update::StoreInstallRequired),
        ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => (),
        _ => return Err("Windows could not identify this installation.".into()),
    }

    // SAFETY: A dedicated worker initializes WinRT and drops its interfaces before uninitializing.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|_| "Could not initialize the Microsoft Store check.")?;
    let _apartment = WinRtApartment;
    query(parent).map_err(|error| format!("Couldn't check Microsoft Store: {error}"))
}

fn query(parent: Option<isize>) -> windows::core::Result<Update> {
    let package = Package::Current()?;
    if package.SignatureKind()? != PackageSignatureKind::Store || package.IsDevelopmentMode()? {
        return Ok(Update::StoreInstallRequired);
    }
    let family = package.Id()?.FamilyName()?;
    let context = StoreContext::GetDefault()?;
    if let Some(parent) = parent {
        // SAFETY: The app HWND owns any native Store dialog during the lookup.
        unsafe {
            context
                .cast::<IInitializeWithWindow>()?
                .Initialize(HWND(parent as *mut _))?;
        }
    }
    let operation = context.GetAppAndOptionalStorePackageUpdatesAsync()?;
    let started = Instant::now();
    // WinRT AsyncStatus::Started has ABI value 0.
    while operation.Status()?.0 == 0 {
        if started.elapsed() >= TIMEOUT {
            let _ = operation.Cancel();
            return Err(windows::core::Error::new(
                windows::core::HRESULT::from_win32(ERROR_TIMEOUT.0),
                "The Microsoft Store did not respond in time.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    for update in operation.GetResults()? {
        if update.Package()?.Id()?.FamilyName()? == family {
            return Ok(Update::StoreUpdateAvailable);
        }
    }
    Ok(Update::Current)
}

pub(super) fn open_store() -> Result<(), String> {
    let uri = HSTRING::from(STORE_UPDATES_URL);
    // SAFETY: This opens a fixed Store URI through its registered handler, with no command arguments.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(uri.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("Could not open Microsoft Store.".into())
    }
}
