//! Receive Finder document opens on winit's existing AppKit delegate.
use crate::OpenQueue;
use eframe::{UserEvent, egui::ViewportId};
use objc2::{
    class, ffi, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use std::{
    ffi::CStr,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Instant,
};
use winit::event_loop::EventLoopProxy;

struct Handler {
    files: OpenQueue,
    wake: EventLoopProxy<UserEvent>,
    context: Mutex<Option<eframe::egui::Context>>,
}

static HANDLER: OnceLock<Handler> = OnceLock::new();

pub(crate) fn register(
    files: OpenQueue,
    wake: EventLoopProxy<UserEvent>,
) -> Result<(), &'static str> {
    // EventLoop construction and delegate access run on the AppKit main thread.
    let application: *mut AnyObject =
        unsafe { msg_send![class!(NSApplication), sharedApplication] };
    if application.is_null() {
        return Err("Seyir could not register for Finder document opens.");
    }
    let delegate: *mut AnyObject = unsafe { msg_send![application, delegate] };
    let delegate = unsafe { delegate.as_ref() }.ok_or("Seyir's AppKit delegate is unavailable.")?;
    let class = delegate.class();
    // The selector is optional. Preserve any implementation supplied by winit.
    let method = sel!(application:openFiles:);
    if class.instance_method(method).is_some() {
        return Err("The AppKit delegate already handles document opens.");
    }
    HANDLER
        .set(Handler {
            files,
            wake,
            context: Mutex::new(None),
        })
        .map_err(|_| "Finder document handling is already registered.")?;
    let function = unsafe {
        std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            Imp,
        >(open_files)
    };
    let added = unsafe {
        ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            method,
            function,
            c"v@:@@".as_ptr(),
        )
    };
    if !added.as_bool() {
        return Err("Seyir could not attach its Finder document handler.");
    }
    Ok(())
}

pub(crate) fn set_context(context: &eframe::egui::Context) {
    if let Some(handler) = HANDLER.get()
        && let Ok(mut slot) = handler.context.lock()
    {
        *slot = Some(context.clone());
    }
}

extern "C-unwind" fn open_files(
    _delegate: *mut AnyObject,
    _selector: Sel,
    application: *mut AnyObject,
    filenames: *mut AnyObject,
) {
    let mut opened = false;
    if let Some(handler) = HANDLER.get()
        && let Ok(mut queue) = handler.files.lock()
        && !filenames.is_null()
    {
        // AppKit supplies an NSArray<NSString*> that remains alive for this callback.
        let count: usize = unsafe { msg_send![filenames, count] };
        for index in 0..count.min(64) {
            let filename: *mut AnyObject = unsafe { msg_send![filenames, objectAtIndex: index] };
            if !filename.is_null() {
                let bytes: *const std::ffi::c_char =
                    unsafe { msg_send![filename, fileSystemRepresentation] };
                if !bytes.is_null() {
                    let file = unsafe { CStr::from_ptr(bytes) };
                    let path = PathBuf::from(std::ffi::OsStr::from_bytes(file.to_bytes()));
                    if path.is_absolute() {
                        queue.push_back(path);
                        opened = true;
                    }
                }
            }
        }
        if opened {
            let cumulative_pass_nr = handler
                .context
                .lock()
                .ok()
                .and_then(|slot| {
                    slot.as_ref()
                        .map(|context| context.cumulative_pass_nr_for(ViewportId::ROOT))
                })
                .unwrap_or(0);
            let _ = handler.wake.send_event(UserEvent::RequestRepaint {
                viewport_id: ViewportId::ROOT,
                when: Instant::now(),
                cumulative_pass_nr,
            });
        }
    }
    if !application.is_null() {
        let reply: usize = if opened { 0 } else { 2 };
        unsafe {
            let _: () = msg_send![application, replyToOpenOrPrint: reply];
        }
    }
}
