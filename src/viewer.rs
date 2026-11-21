use crate::{
    OpenQueue, bediz,
    canvas::{self, Canvas},
    decode::{self, Decoded},
    gallery,
    layout::{self, Layout},
    metadata::{self, EditableMetadata, ImageMetadata},
    work::{self, Entry, ImageCache, Latest},
};
use eframe::egui::{self, Color32, RichText, Vec2};
use image::RgbaImage;
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Command,
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Wake, Waker},
};

#[path = "viewer_panels.rs"]
pub(crate) mod panel_ui;

enum Reply {
    Image(u64, Result<Loaded, String>),
    Folder(u64, Result<Vec<Entry>, String>),
    Thumbnails(Vec<(PathBuf, Option<RgbaImage>)>),
    Info(u64, Result<ImageMetadata, String>),
    SavedInfo(u64, Result<ImageMetadata, String>),
    Pixels(Result<RgbaImage, String>, bool),
    Bediz(Option<bool>),
    Sent(Result<String, String>),
}

struct Loaded {
    image: Arc<Decoded>,
    preview: Option<RgbaImage>,
}

type FolderRequest = (u64, PathBuf);

#[derive(Clone, Copy)]
enum PickKind {
    Image,
    Folder,
}

struct Picker {
    kind: PickKind,
    file: Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>,
}

struct Repaint(egui::Context);
impl Wake for Repaint {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

#[derive(Default)]
struct InfoPane {
    visible: bool,
    loaded: Option<ImageMetadata>,
    draft: Option<EditableMetadata>,
    saving: bool,
    error: Option<String>,
}

struct ThumbnailCache {
    images: HashMap<PathBuf, Option<egui::TextureHandle>>,
    order: VecDeque<PathBuf>,
    busy: bool,
}

impl ThumbnailCache {
    fn new() -> Self {
        Self {
            images: HashMap::new(),
            order: VecDeque::new(),
            busy: false,
        }
    }

    fn insert(&mut self, path: PathBuf, texture: Option<egui::TextureHandle>) {
        if self.images.contains_key(&path) {
            return;
        }
        if self.order.len() == 160
            && let Some(old) = self.order.pop_front()
        {
            self.images.remove(&old);
        }
        self.order.push_back(path.clone());
        self.images.insert(path, texture);
    }
}

pub struct Viewer {
    incoming: OpenQueue,
    replies: mpsc::Receiver<Reply>,
    sender: mpsc::Sender<Reply>,
    decode: Latest<FolderRequest>,
    folders: Latest<FolderRequest>,
    nearby: Latest<Vec<PathBuf>>,
    thumbs: Latest<Vec<PathBuf>>,
    details: Latest<FolderRequest>,
    picker: Option<Picker>,
    image_request: u64,
    folder_request: u64,
    file: Option<PathBuf>,
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    picture: Option<(Arc<Decoded>, egui::TextureHandle)>,
    textures: VecDeque<(Arc<Decoded>, egui::TextureHandle)>,
    thumbnails: ThumbnailCache,
    canvas: Canvas,
    app_icon: egui::TextureHandle,
    layout: Layout,
    next_layout: Option<Layout>,
    info: InfoPane,
    browser: bool,
    sort_newest: bool,
    focus_folder_file: bool,
    clipboard: Option<arboard::Clipboard>,
    copying: bool,
    bediz: Option<bool>,
    status: String,
    error: bool,
    loading: bool,
}

impl Viewer {
    pub fn new(creation: &eframe::CreationContext<'_>, incoming: OpenQueue) -> Self {
        let ctx = &creation.egui_ctx;
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let layout = Layout::load();
        crate::theme::apply(ctx, layout);
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/seyir.png"))
            .expect("Seyir icon");
        let app_icon = ctx.load_texture(
            "Seyir app mark",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        let (tx, rx) = mpsc::channel();
        let cache = Arc::new(Mutex::new(ImageCache::default()));
        let max_side = ctx.input(|i| i.max_texture_side).clamp(1024, 8192) as u32;

        let decode = {
            let post = post(tx.clone(), ctx.clone());
            let cache = cache.clone();
            Latest::start(move |(token, path): FolderRequest| {
                let result = ImageCache::load(&cache, &path).map(|image| {
                    let preview = (image.pixels.width().max(image.pixels.height()) > max_side)
                        .then(|| scaled(&image.pixels, max_side));
                    Loaded { image, preview }
                });
                post(Reply::Image(token, result));
            })
        };
        let folders = {
            let post = post(tx.clone(), ctx.clone());
            Latest::start(move |(token, path): FolderRequest| {
                post(Reply::Folder(token, work::folder(&path)))
            })
        };
        let nearby = Latest::start(move |paths: Vec<PathBuf>| {
            for path in paths {
                // Do not speculatively decompress arbitrarily large files.
                if path.metadata().is_ok_and(|m| m.len() < 12 * 1024 * 1024) {
                    let _ = ImageCache::load(&cache, &path);
                }
            }
        });
        let thumbs = {
            let post = post(tx.clone(), ctx.clone());
            Latest::start(move |paths: Vec<PathBuf>| {
                let images = paths
                    .into_iter()
                    .map(|path| {
                        let image = decode::thumbnail(&path, 110).ok();
                        (path, image)
                    })
                    .collect();
                post(Reply::Thumbnails(images));
            })
        };
        let details = {
            let post = post(tx.clone(), ctx.clone());
            Latest::start(move |(token, path): FolderRequest| {
                post(Reply::Info(token, metadata::read(&path)));
            })
        };
        let check_bediz = post(tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            check_bediz(Reply::Bediz(
                bediz::detect().map(|app| app.supports_file_open()),
            ));
        });
        Self {
            incoming,
            replies: rx,
            sender: tx,
            decode,
            folders,
            nearby,
            thumbs,
            details,
            picker: None,
            image_request: 0,
            folder_request: 0,
            file: None,
            folder: None,
            entries: Vec::new(),
            picture: None,
            textures: VecDeque::new(),
            thumbnails: ThumbnailCache::new(),
            canvas: Canvas::default(),
            app_icon,
            layout,
            next_layout: None,
            info: InfoPane {
                visible: layout == Layout::StudioDesk,
                ..InfoPane::default()
            },
            browser: layout != Layout::FocusMode,
            sort_newest: false,
            focus_folder_file: false,
            clipboard: None,
            copying: false,
            bediz: None,
            status: String::new(),
            error: false,
            loading: false,
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::Image(token, result) if token == self.image_request => {
                    self.loading = false;
                    match result {
                        Ok(loaded) => {
                            let texture = self.texture(ctx, &loaded);
                            self.picture = Some((loaded.image, texture));
                            self.status.clear();
                            self.error = false;
                            self.prepare_neighbors();
                        }
                        Err(error) => self.report(Err(error)),
                    }
                }
                Reply::Folder(token, result) if token == self.folder_request => match result {
                    Ok(mut items) => {
                        work::sort_entries(&mut items, self.sort_newest);
                        self.entries = items;
                        self.focus_folder_file = true;
                        if self.file.is_none()
                            && let Some(path) = self
                                .entries
                                .iter()
                                .find(|entry| !entry.directory)
                                .map(|entry| entry.path.clone())
                        {
                            self.open_file(path);
                        } else {
                            self.prepare_neighbors();
                        }
                    }
                    Err(error) => self.report(Err(error)),
                },
                Reply::Thumbnails(images) => {
                    self.thumbnails.busy = false;
                    for (path, image) in images {
                        let texture = image
                            .as_ref()
                            .map(|pixels| load_texture(ctx, "Seyir thumbnail", pixels));
                        self.thumbnails.insert(path, texture);
                    }
                }
                Reply::Info(token, result) if token == self.image_request => {
                    self.receive_info(result)
                }
                Reply::SavedInfo(token, result) if token == self.image_request => {
                    self.info.saving = false;
                    if result.is_ok() {
                        self.report(Ok("Metadata saved.".into()));
                    }
                    self.receive_info(result);
                }
                Reply::SavedInfo(_, result) => {
                    self.info.saving = false;
                    self.report(result.map(|_| "Metadata saved.".into()));
                }
                Reply::Pixels(result, show_bediz) => {
                    self.copying = false;
                    let result = result.and_then(|image| self.copy_to_clipboard(image));
                    if show_bediz && result.is_ok() {
                        let post = post(self.sender.clone(), ctx.clone());
                        std::thread::spawn(move || {
                            let result = bediz::detect()
                                .ok_or("Bediz is no longer installed.".into())
                                .and_then(|app| app.show())
                                .map(|_| "Paste the copied image into Bediz.".into());
                            post(Reply::Sent(result));
                        });
                    }
                    self.report(result);
                }
                Reply::Bediz(compatibility) => self.bediz = compatibility,
                Reply::Sent(result) => self.report(result),
                _ => {}
            }
        }
    }

    fn receive_info(&mut self, result: Result<ImageMetadata, String>) {
        match result {
            Ok(info) => {
                self.info.draft = info.editable.clone();
                self.info.loaded = Some(info);
                self.info.error = None;
            }
            Err(error) => self.info.error = Some(error),
        }
    }

    fn copy_to_clipboard(&mut self, image: RgbaImage) -> Result<String, String> {
        if self.clipboard.is_none() {
            self.clipboard =
                Some(arboard::Clipboard::new().map_err(|e| format!("Clipboard unavailable: {e}"))?);
        }
        let clipboard = self.clipboard.as_mut().expect("initialized clipboard");
        clipboard
            .set_image(arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: Cow::Owned(image.into_raw()),
            })
            .map_err(|e| format!("Cannot copy image: {e}"))?;
        Ok("Image copied to clipboard.".into())
    }

    fn texture(&mut self, ctx: &egui::Context, loaded: &Loaded) -> egui::TextureHandle {
        if let Some(index) = self
            .textures
            .iter()
            .position(|(candidate, _)| Arc::ptr_eq(candidate, &loaded.image))
            && let Some((image, texture)) = self.textures.remove(index)
        {
            self.textures.push_back((image, texture.clone()));
            return texture;
        }
        let pixels = loaded.preview.as_ref().unwrap_or(&loaded.image.pixels);
        let texture = load_texture(ctx, "Seyir image", pixels);
        const BUDGET: usize = 80 * 1024 * 1024;
        let bytes = pixels.as_raw().len();
        if bytes <= BUDGET {
            while (self.textures.len() >= 3
                || self
                    .textures
                    .iter()
                    .map(|(image, _)| image.pixels.as_raw().len())
                    .sum::<usize>()
                    + loaded.image.pixels.as_raw().len()
                    > BUDGET)
                && self.textures.pop_front().is_some()
            {}
            // A large source is held by the active picture only.
            if loaded.image.pixels.as_raw().len() <= BUDGET {
                self.textures
                    .push_back((loaded.image.clone(), texture.clone()));
            }
        }
        texture
    }

    fn report(&mut self, result: Result<String, String>) {
        self.error = result.is_err();
        self.status = match result {
            Ok(message) | Err(message) => message,
        };
    }

    fn open_path(&mut self, path: PathBuf) {
        let path = absolute(path);
        if path.is_dir() {
            self.open_folder(path);
        } else {
            self.open_file(path);
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        self.browser = true;
        if self.folder.as_ref() == Some(&path) && self.file.is_some() {
            self.refresh_folder();
            self.focus_folder_file = true;
            return;
        }
        self.file = None;
        self.picture = None;
        self.loading = false;
        self.image_request = self.image_request.wrapping_add(1);
        self.clear_info();
        self.index_folder(path);
    }

    fn index_folder(&mut self, path: PathBuf) {
        self.folder = Some(path);
        self.entries.clear();
        self.refresh_folder();
    }

    fn refresh_folder(&mut self) {
        if let Some(path) = &self.folder {
            self.folder_request = self.folder_request.wrapping_add(1);
            self.folders.submit((self.folder_request, path.clone()));
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        if self.file.as_ref() == Some(&path) && self.picture.is_some() {
            return;
        }
        if let Some(parent) = path.parent()
            && self.folder.as_deref() != Some(parent)
        {
            self.index_folder(parent.to_owned());
        }
        self.file = Some(path.clone());
        self.picture = None;
        self.loading = true;
        self.error = false;
        self.focus_folder_file = true;
        self.status = "Opening image…".into();
        self.canvas = Canvas::default();
        self.image_request = self.image_request.wrapping_add(1);
        self.clear_info();
        self.decode.submit((self.image_request, path));
        if self.info.visible {
            self.read_info();
        }
    }

    fn clear_info(&mut self) {
        self.info.loaded = None;
        self.info.draft = None;
        self.info.error = None;
    }

    fn read_info(&self) {
        if let Some(path) = &self.file {
            self.details.submit((self.image_request, path.clone()));
        }
    }

    fn toggle_info(&mut self) {
        self.info.visible = !self.info.visible;
        if self.info.visible && self.info.loaded.is_none() {
            self.read_info();
        }
    }

    fn change_layout(&mut self, ctx: &egui::Context, layout: Layout) {
        self.layout = layout;
        crate::theme::apply(ctx, layout);
        self.browser = layout != Layout::FocusMode;
        self.info.visible = layout == Layout::StudioDesk;
        self.focus_folder_file = true;
        if self.info.visible && self.info.loaded.is_none() {
            self.read_info();
        }
        if let Err(error) = layout.save() {
            self.report(Err(format!(
                "Could not save the layout preference: {error}"
            )));
        }
    }

    fn pick(&mut self, kind: PickKind) {
        if self.picker.is_some() {
            return;
        }
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(folder) = &self.folder {
            dialog = dialog.set_directory(folder);
        }
        let file = Box::pin(async move {
            let handle = match kind {
                PickKind::Image => dialog.set_title("Open image").pick_file().await,
                PickKind::Folder => dialog.set_title("Open image folder").pick_folder().await,
            };
            handle.map(|handle| handle.path().to_owned())
        });
        self.picker = Some(Picker { kind, file });
    }

    fn poll_picker(&mut self, ctx: &egui::Context) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let wake = Waker::from(Arc::new(Repaint(ctx.clone())));
        if let Poll::Ready(path) = picker.file.as_mut().poll(&mut Context::from_waker(&wake)) {
            let kind = self.picker.take().expect("active picker").kind;
            if let Some(path) = path {
                match kind {
                    PickKind::Image => self.open_file(path),
                    PickKind::Folder => self.open_folder(path),
                }
            }
        }
    }

    fn current_index(&self) -> Option<usize> {
        let path = self.file.as_ref()?;
        self.entries
            .iter()
            .position(|entry| !entry.directory && &entry.path == path)
    }

    fn neighbor(&self, forward: bool) -> Option<PathBuf> {
        let index = self.current_index()?;
        let candidate = if forward {
            self.entries[index + 1..]
                .iter()
                .find(|entry| !entry.directory)
        } else {
            self.entries[..index]
                .iter()
                .rev()
                .find(|entry| !entry.directory)
        };
        candidate.map(|entry| entry.path.clone())
    }

    fn navigate(&mut self, forward: bool) {
        if let Some(path) = self.neighbor(forward) {
            self.open_file(path);
        }
    }

    fn prepare_neighbors(&self) {
        let paths: Vec<PathBuf> = [self.neighbor(true), self.neighbor(false)]
            .into_iter()
            .flatten()
            .collect();
        self.nearby.submit(paths);
    }

    fn copy(&mut self, ctx: &egui::Context, show_bediz: bool) {
        if self.copying {
            return;
        }
        let Some((image, _)) = &self.picture else {
            return;
        };
        let image = image.clone();
        let angle = self.canvas.angle;
        let post = post(self.sender.clone(), ctx.clone());
        self.copying = true;
        std::thread::spawn(move || {
            post(Reply::Pixels(
                canvas::oriented_pixels(&image.pixels, angle),
                show_bediz,
            ))
        });
    }

    fn edit(&mut self, ctx: &egui::Context, rendered_copy: bool) {
        let Some(path) = self.file.clone() else {
            return;
        };
        let Some((source, _)) = &self.picture else {
            return;
        };
        let source = source.clone();
        let angle = self.canvas.angle;
        let post = post(self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = (|| {
                let bediz = bediz::detect().ok_or("Bediz is no longer installed.")?;
                let path = if rendered_copy {
                    let image = canvas::oriented_pixels(&source.pixels, angle)?;
                    let mut copy = tempfile::Builder::new()
                        .prefix("Seyir-")
                        .suffix(".png")
                        .tempfile()
                        .map_err(|e| format!("Cannot start the PNG copy: {e}"))?;
                    image
                        .write_to(copy.as_file_mut(), image::ImageFormat::Png)
                        .map_err(|e| format!("Cannot write the PNG copy: {e}"))?;
                    let (_, path) = copy
                        .keep()
                        .map_err(|e| format!("Cannot keep the PNG copy: {e}"))?;
                    path
                } else {
                    path
                };
                bediz.open(&path)?;
                Ok("Sent to Bediz.".into())
            })();
            post(Reply::Sent(result));
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() {
            return;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let text_focused = focused.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if ctx.input(|input| input.key_pressed(egui::Key::O) && input.modifiers.command) {
            self.pick(PickKind::Image);
        }
        if text_focused || egui::Popup::is_any_open(ctx) {
            return;
        }
        let pressed = |key| ctx.input(|input| input.key_pressed(key));
        if pressed(egui::Key::ArrowLeft) {
            self.navigate(false);
        }
        if pressed(egui::Key::ArrowRight) {
            self.navigate(true);
        }
        if pressed(egui::Key::Plus) || pressed(egui::Key::Equals) {
            self.canvas.step_zoom(1.25);
        }
        if pressed(egui::Key::Minus) {
            self.canvas.step_zoom(0.8);
        }
        if pressed(egui::Key::Num0) {
            self.canvas.fit();
        }
        if pressed(egui::Key::Num1) {
            self.canvas.set_zoom(1.0);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::C) && input.modifiers.command) {
            self.copy(ctx, false);
        }
        if pressed(egui::Key::F11) {
            fullscreen(ctx);
        }
        if pressed(egui::Key::Escape)
            && ctx.input(|input| input.viewport().fullscreen == Some(true))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    fn layout_picker(&mut self, ui: &mut egui::Ui, compact: bool) {
        if let Some(selected) = layout::picker(ui, self.layout, compact) {
            self.next_layout = Some(selected);
            ui.ctx().request_repaint();
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, floating: bool) {
        use crate::chrome;

        let width = ui.available_width();
        chrome::row(ui, |ui| {
            if !floating {
                chrome::brand(ui, &self.app_icon);
            }
            self.layout_picker(ui, floating || width < 950.);
            self.toolbar_tools(ui, !floating && width >= 870.);
        });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            crate::chrome::brand(ui, &self.app_icon);
            self.layout_picker(ui, true);
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("tool-rail-scroll")
            .show(ui, |ui| {
                ui.vertical_centered(|ui| self.toolbar_tools(ui, false));
            });
    }

    fn toolbar_tools(&mut self, ui: &mut egui::Ui, show_edit_caption: bool) {
        use crate::chrome::{self, Icon, Tile};

        let has_picture = self.picture.is_some();
        chrome::group(ui, |ui| {
            if Tile::new(Icon::OpenImage, "Open image…").show(ui).clicked() {
                self.pick(PickKind::Image);
            }
            if Tile::new(Icon::OpenFolder, "Open folder…")
                .show(ui)
                .clicked()
            {
                self.pick(PickKind::Folder);
            }
            let toggle = if self.browser {
                "Hide Folder"
            } else {
                "Show Folder"
            };
            let hint = if self.browser {
                "Hide Folder · Close the folder browser"
            } else {
                "Show Folder · Browse the current image’s parent folder"
            };
            if Tile::new(Icon::FolderPane, toggle)
                .hint(hint)
                .enabled(self.folder.is_some())
                .selected(self.browser && self.folder.is_some())
                .show(ui)
                .clicked()
            {
                self.browser = !self.browser;
                self.focus_folder_file = true;
            }
        });
        chrome::group(ui, |ui| {
            if Tile::new(Icon::Previous, "Previous image")
                .hint("Previous image · Left arrow")
                .enabled(self.neighbor(false).is_some())
                .show(ui)
                .clicked()
            {
                self.navigate(false);
            }
            if Tile::new(Icon::Next, "Next image")
                .hint("Next image · Right arrow")
                .enabled(self.neighbor(true).is_some())
                .show(ui)
                .clicked()
            {
                self.navigate(true);
            }
        });
        chrome::group(ui, |ui| {
            if Tile::new(Icon::ZoomOut, "Zoom out")
                .enabled(has_picture)
                .show(ui)
                .clicked()
            {
                self.canvas.step_zoom(0.8);
            }
            if Tile::new(Icon::Fit, "Fit image")
                .hint("Fit image · 0")
                .enabled(has_picture)
                .show(ui)
                .clicked()
            {
                self.canvas.fit();
            }
            if Tile::new(Icon::Actual, "Actual pixels")
                .hint("Actual pixels · 1")
                .enabled(has_picture)
                .show(ui)
                .clicked()
            {
                self.canvas.set_zoom(1.);
            }
            if Tile::new(Icon::ZoomIn, "Zoom in")
                .enabled(has_picture)
                .show(ui)
                .clicked()
            {
                self.canvas.step_zoom(1.25);
            }
            chrome::rotation(ui, &mut self.canvas, has_picture);
        });
        chrome::group(ui, |ui| {
            if Tile::new(Icon::Copy, "Copy Image")
                .enabled(has_picture && !self.copying)
                .show(ui)
                .clicked()
            {
                self.copy(ui.ctx(), false);
            }
            if let Some(compatible) = self.bediz {
                if compatible {
                    let original = self.file.as_deref().is_some_and(bediz_original_supported);
                    let edit = Tile::new(Icon::Edit, "Edit in Bediz")
                        .caption(show_edit_caption)
                        .enabled(has_picture);
                    if original && self.canvas.angle == 0. {
                        if edit.show(ui).clicked() {
                            self.edit(ui.ctx(), false);
                        }
                    } else {
                        let edit = edit.dropdown().show(ui);
                        egui::Popup::menu(&edit).show(|ui| {
                            if original && ui.button("Open original").clicked() {
                                self.edit(ui.ctx(), false);
                                ui.close();
                            }
                            if ui.button("Open rendered PNG copy").clicked() {
                                self.edit(ui.ctx(), true);
                                ui.close();
                            }
                        });
                    }
                } else if Tile::new(Icon::Edit, "Copy and Open Bediz")
                    .hint("Copy and open Bediz · This Bediz build uses Paste to receive images")
                    .caption(show_edit_caption)
                    .enabled(has_picture)
                    .show(ui)
                    .clicked()
                {
                    self.copy(ui.ctx(), true);
                }
            }
        });
        chrome::group(ui, |ui| {
            if Tile::new(Icon::Info, "Info")
                .enabled(self.file.is_some())
                .selected(self.info.visible && self.file.is_some())
                .show(ui)
                .clicked()
            {
                self.toggle_info();
            }
            let full = ui.input(|input| input.viewport().fullscreen == Some(true));
            let hint = if full {
                "Exit full screen · F11"
            } else {
                "Full screen · F11"
            };
            if Tile::new(Icon::Fullscreen, "Full Screen")
                .hint(hint)
                .selected(full)
                .show(ui)
                .clicked()
            {
                fullscreen(ui.ctx());
            }
        });
    }

    fn folder_title(&self, ui: &mut egui::Ui) {
        use panel_ui::Glyph;
        let Some(folder) = &self.folder else {
            panel_ui::title(ui, Glyph::Folder, "Folder", None);
            return;
        };
        let folder_name = path_name(folder);
        let folder_path = folder.display().to_string();
        panel_ui::title(
            ui,
            Glyph::Folder,
            "Folder",
            Some((&folder_name, &folder_path)),
        );
    }

    fn folder_actions(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        use panel_ui::Glyph;
        let parent = self
            .folder
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned);
        let mut open_folder = None;
        let before = self.sort_newest;
        let mut refresh = false;
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 28.;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled_ui(parent.is_some(), |ui| {
                        panel_ui::action(ui, Glyph::Up, "Parent folder", None, false)
                    })
                    .inner
                    .clicked()
                {
                    open_folder = parent;
                }
                let refresh_slot = 28. + ui.spacing().item_spacing.x;
                egui::ComboBox::from_id_salt("folder-order")
                    .width((ui.available_width() - refresh_slot).clamp(61., 116.))
                    .truncate()
                    .selected_text(if before { "Newest" } else { "Name" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sort_newest, false, "Name");
                        ui.selectable_value(&mut self.sort_newest, true, "Newest first");
                    })
                    .response
                    .on_hover_text("Folder sort order");
                refresh =
                    panel_ui::action(ui, Glyph::Refresh, "Refresh folder", None, false).clicked();
            });
        });
        if refresh {
            self.refresh_folder();
        }
        if before != self.sort_newest {
            work::sort_entries(&mut self.entries, self.sort_newest);
            self.focus_folder_file = true;
            self.prepare_neighbors();
        }
        open_folder
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        use panel_ui::Glyph;
        const ROW_HEIGHT: f32 = 43.;
        self.folder_title(ui);
        ui.add_space(9.);
        if self.folder.is_none() {
            ui.weak("Open a folder to browse its images.");
            return;
        }
        if let Some(parent) = self.folder_actions(ui) {
            self.open_folder(parent);
            return;
        }
        ui.add_space(5.);
        ui.separator();
        ui.add_space(3.);

        let mut missing = Vec::new();
        let mut scroll = egui::ScrollArea::vertical().id_salt("folder-list");
        if self.focus_folder_file {
            if let Some(index) = self.current_index() {
                let row_height = ROW_HEIGHT + ui.spacing().item_spacing.y;
                scroll = scroll.vertical_scroll_offset(
                    (index as f32 * row_height - ui.available_height() / 2.).max(0.),
                );
            }
            self.focus_folder_file = false;
        }
        let clicked = scroll.show_rows(ui, ROW_HEIGHT, self.entries.len(), |ui, rows| {
            let mut chosen = None;
            for entry in &self.entries[rows] {
                let selected = self.file.as_ref() == Some(&entry.path);
                let response = ui
                    .push_id(&entry.path, |ui| {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                            egui::Sense::click(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                ui.is_enabled(),
                                selected,
                                &entry.name,
                            )
                        });
                        let visuals = ui.visuals();
                        let accent = visuals.selection.stroke.color;
                        let active = response.hovered() || response.is_pointer_button_down_on();
                        if selected || active || response.has_focus() {
                            ui.painter().rect(
                                rect,
                                8,
                                if selected {
                                    visuals.selection.bg_fill
                                } else {
                                    visuals.faint_bg_color
                                },
                                if response.has_focus() {
                                    visuals.selection.stroke
                                } else {
                                    egui::Stroke::NONE
                                },
                                egui::StrokeKind::Inside,
                            );
                        }
                        if selected {
                            let rail = egui::Rect::from_min_size(
                                rect.left_center() + egui::vec2(0., -12.),
                                egui::vec2(2.5, 24.),
                            );
                            ui.painter().rect_filled(rail, 2, accent);
                        }
                        let preview = egui::Rect::from_center_size(
                            rect.left_center() + egui::vec2(23., 0.),
                            Vec2::splat(34.),
                        );
                        let thumb_bg = if active {
                            visuals.widgets.hovered.weak_bg_fill
                        } else {
                            visuals.faint_bg_color
                        };
                        ui.painter().rect_filled(preview, 7, thumb_bg);
                        if entry.directory {
                            panel_ui::glyph(ui, preview.shrink(7.), Glyph::Folder, accent);
                        } else if let Some(Some(texture)) = self.thumbnails.images.get(&entry.path)
                        {
                            let factor = 31. / texture.size_vec2().max_elem();
                            let fitted = egui::Rect::from_center_size(
                                preview.center(),
                                texture.size_vec2() * factor,
                            );
                            ui.painter().image(
                                texture.id(),
                                fitted,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                Color32::WHITE,
                            );
                        } else {
                            panel_ui::glyph(
                                ui,
                                preview.shrink(7.),
                                Glyph::Image,
                                visuals.weak_text_color(),
                            );
                            if !self.thumbnails.images.contains_key(&entry.path)
                                && missing.len() < 6
                            {
                                missing.push(entry.path.clone());
                            }
                        }

                        let trailing = if entry.directory { 22. } else { 8. };
                        let name_rect = egui::Rect::from_min_max(
                            rect.left_top() + egui::vec2(49., 0.),
                            rect.right_bottom() - egui::vec2(trailing, 0.),
                        );
                        panel_ui::clipped_text(
                            ui,
                            name_rect,
                            &entry.name,
                            12.5,
                            visuals.text_color(),
                        );
                        if entry.directory {
                            let marker = egui::Rect::from_center_size(
                                rect.right_center() - egui::vec2(10., 0.),
                                Vec2::splat(14.),
                            );
                            panel_ui::glyph(ui, marker, Glyph::Chevron, visuals.weak_text_color());
                        }
                        response
                            .on_hover_text(entry.name.as_str())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                    })
                    .inner;
                if response.clicked() {
                    chosen = Some((entry.path.clone(), entry.directory));
                }
            }
            chosen
        });
        self.request_thumbnails(missing);
        if let Some((path, directory)) = clicked.inner {
            if directory {
                self.open_folder(path);
            } else {
                self.open_file(path);
            }
        }
    }

    fn request_thumbnails(&mut self, missing: Vec<PathBuf>) {
        if !self.thumbnails.busy && !missing.is_empty() {
            self.thumbnails.busy = true;
            self.thumbs.submit(missing);
        }
    }

    fn thumbnail_browser(&mut self, ui: &mut egui::Ui, grid: bool) {
        if grid {
            self.folder_title(ui);
            ui.add_space(9.);
            if let Some(parent) = self.folder_actions(ui) {
                self.open_folder(parent);
                return;
            }
        } else {
            let name = self.folder.as_deref().map(path_name).unwrap_or_default();
            let actions = ui.horizontal(|ui| {
                let name_width = (ui.available_width() - 204.).max(56.);
                ui.add_sized(
                    [name_width, 28.],
                    egui::Label::new(RichText::new(&name).strong()).truncate(),
                )
                .on_hover_text(&name);
                self.folder_actions(ui)
            });
            if let Some(parent) = actions.inner {
                self.open_folder(parent);
                return;
            }
        }
        ui.add_space(5.);
        if self.entries.is_empty() {
            ui.weak("There are no images or folders to show.");
            return;
        }
        let focus = std::mem::take(&mut self.focus_folder_file);
        let show = if grid {
            gallery::contact_sheet
        } else {
            gallery::filmstrip
        };
        let outcome = show(
            ui,
            &self.entries,
            self.file.as_deref(),
            &self.thumbnails.images,
            focus,
        );
        self.request_thumbnails(outcome.missing);
        if let Some((path, directory)) = outcome.selected {
            if directory {
                self.open_folder(path);
            } else {
                self.open_file(path);
            }
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui, floating: bool) {
        use panel_ui::Glyph;
        let path = self.file.clone();
        let filename = path.as_deref().map(path_name);
        let file_hint = path.as_ref().map(|path| path.display().to_string());
        if floating {
            if let Some((name, hint)) = filename.as_deref().zip(file_hint.as_deref()) {
                ui.add(egui::Label::new(RichText::new(name).weak()).truncate())
                    .on_hover_text(hint);
            }
        } else {
            panel_ui::title(
                ui,
                Glyph::Info,
                "Image info",
                filename.as_deref().zip(file_hint.as_deref()),
            );
        }
        if let Some(path) = &path {
            ui.add_space(8.);
            ui.horizontal_wrapped(|ui| {
                if panel_ui::action(ui, Glyph::Reveal, reveal_label(), Some("Reveal"), false)
                    .clicked()
                    && let Err(error) = reveal(path)
                {
                    self.report(Err(error));
                }
                if panel_ui::action(ui, Glyph::Copy, "Copy path", Some("Copy path"), false)
                    .clicked()
                {
                    ui.ctx().copy_text(path.display().to_string());
                }
            });
        }
        ui.add_space(5.);
        ui.separator();
        ui.add_space(3.);
        if let Some(error) = &self.info.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(info) = &self.info.loaded else {
            if self.file.is_some() && self.info.error.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Reading image info…");
                });
            } else if self.file.is_none() {
                ui.weak("Open an image to see its details.");
            }
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("image-info-body")
            .show(ui, |ui| {
                let dimensions = info.dimensions.or_else(|| {
                    self.picture
                        .as_ref()
                        .map(|(image, _)| [image.pixels.width(), image.pixels.height()])
                });
                let format = self
                    .picture
                    .as_ref()
                    .map_or(info.format.as_str(), |(image, _)| &image.format);
                let bytes = bytes_label(info.byte_len);
                ui.columns(2, |columns| {
                    panel_ui::stat(&mut columns[0], "Format", format);
                    panel_ui::stat(&mut columns[1], "File size", &bytes);
                });
                ui.add_space(5.);
                if let Some([width, height]) = dimensions {
                    panel_ui::detail(ui, "Dimensions", &format!("{width} × {height} px"));
                }
                if let Some(modified) = info.modified {
                    let date: chrono::DateTime<chrono::Local> = modified.into();
                    panel_ui::detail(ui, "Modified", &date.format("%Y-%m-%d %H:%M").to_string());
                }
                for (label, value) in &info.details {
                    panel_ui::detail(ui, label, value);
                }

                ui.add_space(7.);
                ui.separator();
                ui.add_space(5.);
                ui.label(RichText::new("Metadata").size(14.).strong());
                if let Some(note) = &info.edit_note {
                    ui.small(note);
                }
                let Some(draft) = &mut self.info.draft else {
                    return;
                };
                ui.add_enabled_ui(!self.info.saving, |ui| {
                    panel_ui::field(ui, "Title", &mut draft.title, false);
                    panel_ui::field(ui, "Description", &mut draft.description, true);
                    panel_ui::field(ui, "Author", &mut draft.author, false);
                    panel_ui::field(ui, "Copyright", &mut draft.copyright, false);
                    panel_ui::field(ui, "Keywords (comma separated)", &mut draft.keywords, false);
                    let changed = info.editable.as_ref() != Some(draft);
                    ui.add_space(3.);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled_ui(changed, |ui| {
                                panel_ui::action(
                                    ui,
                                    Glyph::Check,
                                    "Save metadata",
                                    Some("Save metadata"),
                                    true,
                                )
                            })
                            .inner
                            .clicked()
                            && let Some(path) = self.file.clone()
                        {
                            self.info.saving = true;
                            self.info.error = None;
                            let previous = info.clone();
                            let updated = draft.clone();
                            let token = self.image_request;
                            let post = post(self.sender.clone(), ui.ctx().clone());
                            std::thread::spawn(move || {
                                post(Reply::SavedInfo(
                                    token,
                                    metadata::write(&path, &previous, &updated),
                                ));
                            });
                        }
                        if ui
                            .add_enabled_ui(changed, |ui| {
                                panel_ui::action(
                                    ui,
                                    Glyph::Refresh,
                                    "Reset metadata",
                                    Some("Reset"),
                                    false,
                                )
                            })
                            .inner
                            .clicked()
                            && let Some(original) = &info.editable
                        {
                            *draft = original.clone();
                        }
                    });
                });
                if self.info.saving {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Saving metadata…");
                    });
                }
            });
    }
}

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Some(layout) = self.next_layout.take() {
            self.change_layout(ctx, layout);
        }
        self.poll_picker(ctx);
        self.receive(ctx);
        let path = self
            .incoming
            .lock()
            .ok()
            .and_then(|mut files| {
                let chosen = files.pop_front();
                files.clear();
                chosen
            })
            .or_else(|| {
                ctx.input(|i| {
                    i.raw
                        .dropped_files
                        .iter()
                        .find_map(|item| item.path.clone())
                })
            });
        if let Some(path) = path {
            self.open_path(path);
        }
        self.shortcuts(ctx);

        self.workspace(ctx);
    }
}

impl Viewer {
    fn workspace(&mut self, ctx: &egui::Context) {
        use crate::theme;

        let layout = self.layout;
        let window_width = ctx.content_rect().width();
        if !matches!(layout, Layout::ToolRail | Layout::FocusMode) {
            egui::TopBottomPanel::top("toolbar")
                .frame(theme::bar(
                    ctx,
                    if layout == Layout::StudioDesk { 9 } else { 7 },
                ))
                .show(ctx, |ui| self.toolbar(ui, false));
        }
        egui::TopBottomPanel::bottom("status")
            .frame(theme::bar(ctx, 5))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(path) = &self.file {
                        ui.strong(path_name(path));
                    }
                    if let Some((image, _)) = &self.picture {
                        ui.weak(format!(
                            "{} × {} px · {} · {:.0}% · {:.1}°",
                            image.pixels.width(),
                            image.pixels.height(),
                            image.format,
                            self.canvas.zoom_ratio() * 100.,
                            self.canvas.angle
                        ));
                    }
                    if !self.status.is_empty() {
                        let text = RichText::new(&self.status);
                        ui.label(if self.error {
                            text.color(ui.visuals().error_fg_color)
                        } else {
                            text.weak()
                        });
                    }
                });
            });

        if layout == Layout::ToolRail {
            egui::SidePanel::left("tool-rail")
                .exact_width(54.)
                .frame(theme::bar(ctx, 6).inner_margin(egui::Margin::symmetric(4, 6)))
                .show(ctx, |ui| self.tool_rail(ui));
        }
        let folder = self.browser && self.folder.is_some();
        let info = self.info.visible && self.file.is_some();
        if folder {
            match layout {
                Layout::StudioDesk => {
                    egui::SidePanel::left("studio-folder")
                        .default_width(216.)
                        .width_range(166. ..=(window_width * 0.29).clamp(166., 360.))
                        .frame(theme::bar(ctx, 12))
                        .show(ctx, |ui| self.browser(ui));
                }
                Layout::Filmstrip => {
                    egui::TopBottomPanel::bottom("filmstrip")
                        .exact_height(gallery::FILMSTRIP_HEIGHT + 48.)
                        .frame(theme::bar(ctx, 6))
                        .show(ctx, |ui| self.thumbnail_browser(ui, false));
                }
                Layout::ContactSheet => {
                    egui::SidePanel::left("contact-sheet")
                        .default_width((window_width * 0.36).clamp(230., 470.))
                        .width_range(230. ..=(window_width * 0.58).clamp(230., 520.))
                        .frame(theme::bar(ctx, 12))
                        .show(ctx, |ui| self.thumbnail_browser(ui, true));
                }
                Layout::ToolRail => {
                    egui::SidePanel::right("rail-folder")
                        .default_width(234.)
                        .width_range(170. ..=390.)
                        .frame(theme::bar(ctx, 12))
                        .show(ctx, |ui| self.browser(ui));
                }
                Layout::FocusMode => {}
            }
        }
        if info && layout == Layout::StudioDesk {
            egui::SidePanel::right("studio-image-info")
                .default_width(250.)
                .width_range(200. ..=(window_width * 0.34).clamp(200., 390.))
                .frame(theme::bar(ctx, 12))
                .show(ctx, |ui| self.inspector(ui, false));
        }

        let mut canvas_frame = egui::Frame::new().fill(theme::canvas(ctx, layout));
        if layout == Layout::FocusMode {
            canvas_frame = canvas_frame.inner_margin(egui::Margin {
                top: if folder { 122 } else { 0 },
                bottom: if ctx.content_rect().width() < 654. {
                    108
                } else {
                    70
                },
                ..egui::Margin::ZERO
            });
        }
        let stage = egui::CentralPanel::default()
            .frame(canvas_frame)
            .show(ctx, |ui| self.image_stage(ui))
            .response
            .rect;
        if layout == Layout::FocusMode {
            self.focus_tools(ctx, stage, folder);
        }
        if info && layout != Layout::StudioDesk {
            let mut open = true;
            egui::Window::new("Image info")
                .id(egui::Id::new("floating-image-info"))
                .open(&mut open)
                .default_pos(stage.right_top() + egui::vec2(-270., 10.))
                .constrain_to(stage)
                .default_width(246.)
                .min_width(204.)
                .max_height((stage.height() - 30.).max(140.))
                .frame(theme::floating(ctx))
                .show(ctx, |ui| self.inspector(ui, true));
            self.info.visible = open;
        }
    }

    fn focus_tools(&mut self, ctx: &egui::Context, stage: egui::Rect, folder: bool) {
        use crate::theme;

        if folder {
            egui::Area::new(egui::Id::new("focus-folder"))
                .order(egui::Order::Foreground)
                .fixed_pos(stage.left_top() + egui::vec2(10., 10.))
                .constrain_to(stage)
                .show(ctx, |ui| {
                    theme::floating(ctx).show(ui, |ui| {
                        ui.set_width((stage.width() - 38.).clamp(290., 482.));
                        self.thumbnail_browser(ui, false);
                    });
                });
        }
        egui::Area::new(egui::Id::new("focus-tools"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(stage.center_bottom() - egui::vec2(0., 12.))
            .constrain_to(stage)
            .show(ctx, |ui| {
                theme::floating(ctx).show(ui, |ui| {
                    ui.set_width((stage.width() - 36.).clamp(300., 636.));
                    self.toolbar(ui, true);
                });
            });
    }

    fn image_stage(&mut self, ui: &mut egui::Ui) {
        if let Some((image, texture)) = &self.picture {
            self.canvas.paint(
                ui,
                texture.id(),
                [image.pixels.width(), image.pixels.height()],
            );
        } else {
            ui.add_space((ui.available_height() * 0.5 - 74.).max(0.));
            ui.vertical_centered(|ui| {
                if self.loading {
                    ui.spinner();
                    ui.label(RichText::new("Opening image…").color(crate::theme::CANVAS_INK));
                } else if self.error && self.file.is_some() {
                    ui.label(RichText::new(&self.status).color(crate::theme::CANVAS_INK));
                } else {
                    ui.image((self.app_icon.id(), Vec2::splat(76.)));
                    ui.add_space(8.);
                    let open = egui::Button::new(
                        RichText::new("Open an image")
                            .strong()
                            .color(Color32::from_rgb(219, 254, 251)),
                    )
                    .fill(Color32::from_rgb(36, 73, 86))
                    .corner_radius(8)
                    .min_size(egui::vec2(136., 36.));
                    if ui.add(open).clicked() {
                        self.pick(PickKind::Image);
                    }
                    ui.label(
                        RichText::new("or drop it here")
                            .color(crate::theme::CANVAS_INK)
                            .size(12.),
                    );
                }
            });
        }
    }
}

fn post(tx: mpsc::Sender<Reply>, ctx: egui::Context) -> impl Fn(Reply) {
    move |reply| {
        if tx.send(reply).is_ok() {
            ctx.request_repaint();
        }
    }
}

fn scaled(image: &RgbaImage, maximum: u32) -> RgbaImage {
    let ratio = f64::from(maximum) / f64::from(image.width().max(image.height()));
    let dimension = |size| (f64::from(size) * ratio).round().max(1.) as u32;
    image::imageops::resize(
        image,
        dimension(image.width()),
        dimension(image.height()),
        image::imageops::FilterType::Triangle,
    )
}

fn load_texture(ctx: &egui::Context, label: &str, pixels: &RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(
        label,
        egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().map_or_else(|_| path.clone(), |folder| folder.join(&path))
    }
}

fn path_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn bytes_label(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} bytes")
    }
}

fn fullscreen(ctx: &egui::Context) {
    let active = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!active));
}

fn bediz_original_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|part| part.to_str())
        .is_some_and(|ext| {
            [
                "png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "webp", "heic", "heif",
            ]
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

#[cfg(target_os = "macos")]
fn reveal_label() -> &'static str {
    "Reveal in Finder"
}
#[cfg(target_os = "windows")]
fn reveal_label() -> &'static str {
    "Reveal in Explorer"
}

#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot reveal file: {e}"))
}
#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg("/select,")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot reveal file: {e}"))
}
