#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod bediz;
mod canvas;
mod chrome;
mod crop;
mod decode;
mod gallery;
mod layout;
#[cfg(target_os = "macos")]
mod mac_open;
mod metadata;
#[cfg(target_os = "windows")]
mod open_with_windows;
mod output;
mod printing;
mod theme;
mod updates;
mod viewer;
mod work;

use eframe::{NativeOptions, UserEvent, egui};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use winit::event_loop::EventLoop;

// Enable the platform graphics backends for eframe’s wgpu renderer.
use wgpu as _;

type OpenQueue = Arc<Mutex<VecDeque<PathBuf>>>;

fn main() -> eframe::Result {
    #[cfg(target_os = "windows")]
    open_with_windows::run_if_requested();

    let files: OpenQueue = Arc::new(Mutex::new(
        std::env::args_os()
            .skip(1)
            .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
            .map(PathBuf::from)
            .collect(),
    ));
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let wake = event_loop.create_proxy();
    #[cfg(target_os = "macos")]
    if let Err(error) = mac_open::register(files.clone(), wake) {
        eprintln!("{error}");
    }
    #[cfg(not(target_os = "macos"))]
    drop(wake);
    let options = NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("Seyir ", env!("CARGO_PKG_VERSION")))
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/seyir.png"))
                    .expect("Seyir icon is a valid PNG"),
            )
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([680.0, 420.0]),
        ..Default::default()
    };
    let mut app = eframe::create_native(
        "Seyir",
        options,
        Box::new(|context| {
            #[cfg(target_os = "macos")]
            mac_open::set_context(&context.egui_ctx);
            Ok(Box::new(viewer::Viewer::new(context, files)))
        }),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
