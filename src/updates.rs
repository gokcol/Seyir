//! Release discovery runs off the UI thread. The user opens the update source.
use crate::viewer::panel_ui::{Glyph, dialog_button};
use eframe::egui;
use std::{sync::mpsc, thread};

#[cfg(target_os = "macos")]
mod github;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
const RELEASES_URL: &str = "https://github.com/gokcol/Seyir/releases";
#[cfg(target_os = "windows")]
const STORE_UPDATES_URL: &str = "ms-windows-store://downloadsandupdates";

#[derive(Debug)]
enum Update {
    Current,
    #[cfg(target_os = "macos")]
    Available {
        version: String,
        url: String,
    },
    #[cfg(target_os = "windows")]
    StoreUpdateAvailable,
    #[cfg(target_os = "windows")]
    StoreInstallRequired,
}

type CheckResult = Result<Update, String>;

#[derive(Default)]
pub(crate) struct State {
    pending: Option<mpsc::Receiver<CheckResult>>,
    result: Option<CheckResult>,
    #[cfg(target_os = "windows")]
    open_error: Option<String>,
}

impl State {
    fn start(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        #[cfg(target_os = "windows")]
        let parent = {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            frame
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                    _ => None,
                })
        };
        #[cfg(not(target_os = "windows"))]
        let _ = frame;
        self.result = None;
        match thread::Builder::new()
            .name("seyir-update-check".into())
            .spawn(move || {
                #[cfg(target_os = "macos")]
                let result = github::check();
                #[cfg(target_os = "windows")]
                let result = windows::check(parent);
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => self.pending = Some(receiver),
            Err(_) => self.result = Some(Err("Could not start the update check.".into())),
        }
    }

    fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        self.result = Some(match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The update check stopped. Try again.".into())
            }
        });
        self.pending = None;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        self.poll();
        ui.strong(concat!("Seyir ", env!("CARGO_PKG_VERSION")));
        #[cfg(target_os = "macos")]
        let source = "GitHub Releases";
        #[cfg(target_os = "windows")]
        let source = "Microsoft Store";
        ui.weak(format!("Update source · {source}"));
        ui.add_space(8.);
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking for updates…");
            });
        } else if dialog_button(ui, Glyph::Refresh, "Check for Updates", true).clicked() {
            self.start(ui.ctx(), frame);
        }

        if let Some(result) = &self.result {
            ui.add_space(6.);
            match result {
                Ok(Update::Current) => {
                    ui.label("Seyir is up to date.");
                }
                #[cfg(target_os = "macos")]
                Ok(Update::Available { version, .. }) => {
                    ui.strong(format!("Seyir {version} is available."));
                }
                #[cfg(target_os = "windows")]
                Ok(Update::StoreUpdateAvailable) => {
                    ui.strong("A Seyir update is available in Microsoft Store.");
                }
                #[cfg(target_os = "windows")]
                Ok(Update::StoreInstallRequired) => {
                    ui.label(
                        "In-app update detection requires a Microsoft Store installation of Seyir.",
                    );
                }
                Err(message) => {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
            }
        }

        ui.add_space(6.);
        #[cfg(target_os = "macos")]
        {
            let (label, url) = match &self.result {
                Some(Ok(Update::Available { url, .. })) => ("View release", url.as_str()),
                _ => ("Release notes", RELEASES_URL),
            };
            if dialog_button(ui, Glyph::Reveal, label, false).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        #[cfg(target_os = "windows")]
        {
            if dialog_button(ui, Glyph::Reveal, "Open Store Updates", false).clicked() {
                self.open_error = windows::open_store().err();
            }
            if let Some(message) = &self.open_error {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        }
    }
}
