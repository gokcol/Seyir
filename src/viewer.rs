use crate::{
    OpenQueue, bediz,
    canvas::{self, Canvas, TransitionStyle},
    decode::{self, Decoded},
    gallery,
    layout::{self, Layout, Preferences, SlideshowDefaults},
    metadata::{self, EditableMetadata, ImageMetadata},
    output::{self, Size},
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
    time::Duration,
};

#[path = "viewer_edit.rs"]
mod image_edit;
#[path = "viewer_panels.rs"]
pub(crate) mod panel_ui;

enum Reply {
    Image(u64, Result<Loaded, String>),
    Folder(u64, Result<Vec<Entry>, String>),
    Thumbnails(Vec<(PathBuf, Option<RgbaImage>)>),
    Info(u64, Result<ImageMetadata, String>),
    SavedInfo(u64, Result<ImageMetadata, String>),
    Pixels(Result<RgbaImage, String>, bool),
    Printable(Result<RgbaImage, String>, String),
    SavedImage(Result<PathBuf, String>, bool),
    CropPreview(u64, Result<Loaded, String>),
    ImageEdited(u64, Result<Loaded, String>),
    Bediz(Option<bool>),
    Sent(Result<String, String>),
}

struct Loaded {
    image: Arc<Decoded>,
    preview: Option<RgbaImage>,
}

impl Loaded {
    fn new(image: Arc<Decoded>, max_side: u32) -> Self {
        let preview = (image.pixels.width().max(image.pixels.height()) > max_side)
            .then(|| scaled(&image.pixels, max_side));
        Self { image, preview }
    }
}

type FolderRequest = (u64, PathBuf);

enum PickKind {
    Image,
    Folder,
    SaveAs(ExportImage, Size),
}

struct Picker {
    kind: PickKind,
    file: Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>,
}

#[derive(Clone, Copy)]
enum ExportKind {
    Copy,
    SaveAs,
}

struct ExportImage {
    path: PathBuf,
    source: Arc<Decoded>,
    original: Arc<Decoded>,
    angle: f32,
}

struct ExportOptions {
    kind: ExportKind,
    image: ExportImage,
    size: Size,
}

struct Slideshow {
    current: SlideshowDefaults,
    paused: bool,
    advance_at: Option<f64>,
    restore_window: bool,
}

struct ImageTransition {
    style: TransitionStyle,
    previous: Canvas,
    texture: egui::TextureHandle,
    size: [u32; 2],
    forward: bool,
    started: Option<f64>,
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
    export: Option<ExportOptions>,
    save_confirmation: Option<ExportImage>,
    saving_image: bool,
    printing: bool,
    image_request: u64,
    folder_request: u64,
    file: Option<PathBuf>,
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    preview_first_file: bool,
    picture: Option<(Arc<Decoded>, egui::TextureHandle)>,
    original: Option<Arc<Decoded>>,
    image_undo: Option<image_edit::UndoImage>,
    editing_image: bool,
    crop: Option<image_edit::CropSession>,
    max_texture_side: u32,
    textures: VecDeque<(Arc<Decoded>, egui::TextureHandle)>,
    thumbnails: ThumbnailCache,
    canvas: Canvas,
    app_icon: egui::TextureHandle,
    layout: Layout,
    next_layout: Option<Layout>,
    settings_open: bool,
    settings_playback: bool,
    browse_transition_default: TransitionStyle,
    browse_transition: TransitionStyle,
    slideshow_defaults: SlideshowDefaults,
    slideshow: Option<Slideshow>,
    transition: Option<ImageTransition>,
    info: InfoPane,
    browser: bool,
    folder_preference: Option<bool>,
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
        let preferences = Preferences::load();
        let layout = preferences.layout;
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
                let result =
                    ImageCache::load(&cache, &path).map(|image| Loaded::new(image, max_side));
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
        let mut viewer = Self {
            incoming,
            replies: rx,
            sender: tx,
            decode,
            folders,
            nearby,
            thumbs,
            details,
            picker: None,
            export: None,
            save_confirmation: None,
            saving_image: false,
            printing: false,
            image_request: 0,
            folder_request: 0,
            file: None,
            folder: None,
            entries: Vec::new(),
            preview_first_file: false,
            picture: None,
            original: None,
            image_undo: None,
            editing_image: false,
            crop: None,
            max_texture_side: max_side,
            textures: VecDeque::new(),
            thumbnails: ThumbnailCache::new(),
            canvas: Canvas::default(),
            app_icon,
            layout,
            next_layout: None,
            settings_open: false,
            settings_playback: false,
            browse_transition_default: preferences.browse_transition,
            browse_transition: preferences.browse_transition,
            slideshow_defaults: preferences.slideshow,
            slideshow: None,
            transition: None,
            info: InfoPane {
                visible: preferences.image_info,
                ..InfoPane::default()
            },
            browser: preferences
                .folder
                .unwrap_or(layout.shows_folder_by_default()),
            folder_preference: preferences.folder,
            sort_newest: false,
            focus_folder_file: false,
            clipboard: None,
            copying: false,
            bediz: None,
            status: String::new(),
            error: false,
            loading: false,
        };
        if let Some(dirs) = directories::BaseDirs::new() {
            viewer.index_folder(dirs.home_dir().to_owned());
        }
        viewer
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::Image(token, result) if token == self.image_request => {
                    self.loading = false;
                    match result {
                        Ok(loaded) => {
                            let texture = self.texture(ctx, &loaded);
                            self.original = Some(loaded.image.clone());
                            self.picture = Some((loaded.image, texture));
                            if let Some(transition) = &mut self.transition {
                                transition.started = Some(ctx.input(|input| input.time));
                            }
                            self.status.clear();
                            self.error = false;
                            self.prepare_neighbors();
                        }
                        Err(error) => {
                            self.transition = None;
                            self.report(Err(error));
                        }
                    }
                }
                Reply::Folder(token, result) if token == self.folder_request => match result {
                    Ok(mut items) => {
                        work::sort_entries(&mut items, self.sort_newest);
                        self.entries = items;
                        self.focus_folder_file = true;
                        if self.preview_first_file
                            && self.file.is_none()
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
                Reply::Printable(result, title) => {
                    self.printing = false;
                    match result.and_then(|pixels| crate::printing::print(&pixels, &title)) {
                        Ok(true) => self.report(Ok("Image sent to printer.".into())),
                        Ok(false) => {}
                        Err(error) => self.report(Err(error)),
                    }
                }
                Reply::SavedImage(result, original) => {
                    self.saving_image = false;
                    match result {
                        Ok(path) => {
                            if path.parent() == self.folder.as_deref() {
                                self.thumbnails.images.remove(&path);
                                self.thumbnails.order.retain(|cached| cached != &path);
                                self.refresh_folder();
                            }
                            if original && self.file.as_ref() == Some(&path) {
                                self.picture = None;
                                self.open_file(path.clone());
                            }
                            self.report(Ok(format!("Saved {}.", path_name(&path))));
                        }
                        Err(error) => self.report(Err(error)),
                    }
                }
                Reply::CropPreview(token, result) if token == self.image_request => {
                    self.receive_crop(ctx, result);
                }
                Reply::ImageEdited(token, result) if token == self.image_request => {
                    self.receive_image_edit(ctx, result);
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
        self.set_browser_visible(true);
        if self.folder.as_ref() == Some(&path) && self.file.is_some() {
            self.refresh_folder();
            self.focus_folder_file = true;
            return;
        }
        self.file = None;
        self.picture = None;
        self.clear_image_edits();
        self.transition = None;
        self.loading = false;
        self.image_request = self.image_request.wrapping_add(1);
        self.clear_info();
        self.index_folder(path);
        self.preview_first_file = true;
    }

    fn index_folder(&mut self, path: PathBuf) {
        self.folder = Some(path);
        self.entries.clear();
        self.preview_first_file = false;
        self.refresh_folder();
    }

    fn refresh_folder(&mut self) {
        if let Some(path) = &self.folder {
            self.folder_request = self.folder_request.wrapping_add(1);
            self.folders.submit((self.folder_request, path.clone()));
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        self.open_file_with_transition(path, TransitionStyle::Instant, true);
    }

    fn open_file_with_transition(&mut self, path: PathBuf, style: TransitionStyle, forward: bool) {
        if self.file.as_ref() == Some(&path) && self.picture.is_some() {
            return;
        }
        if let Some(show) = &mut self.slideshow {
            show.advance_at = None;
        }
        self.transition = if style == TransitionStyle::Instant {
            None
        } else if let Some((image, texture)) = &self.picture {
            Some(ImageTransition {
                style,
                previous: self.canvas.clone(),
                texture: texture.clone(),
                size: [image.pixels.width(), image.pixels.height()],
                forward,
                started: None,
            })
        } else {
            self.transition.take().map(|mut transition| {
                transition.forward = forward;
                transition.style = style;
                transition.started = None;
                transition
            })
        };
        if let Some(parent) = path.parent()
            && self.folder.as_deref() != Some(parent)
        {
            self.index_folder(parent.to_owned());
        }
        self.preview_first_file = false;
        self.file = Some(path.clone());
        self.picture = None;
        self.clear_image_edits();
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
        self.set_info_visible(!self.info.visible);
    }

    fn set_info_visible(&mut self, visible: bool) {
        if self.info.visible == visible {
            return;
        }
        self.info.visible = visible;
        if visible && self.info.loaded.is_none() {
            self.read_info();
        }
        self.persist_settings();
    }

    fn set_browser_visible(&mut self, visible: bool) {
        if self.browser == visible {
            return;
        }
        self.browser = visible;
        self.folder_preference = Some(visible);
        self.focus_folder_file = true;
        self.persist_settings();
    }

    fn change_layout(&mut self, ctx: &egui::Context, layout: Layout) {
        self.layout = layout;
        crate::theme::apply(ctx, layout);
        self.browser = self
            .folder_preference
            .unwrap_or(layout.shows_folder_by_default());
        self.focus_folder_file = true;
        self.persist_settings();
    }

    fn persist_settings(&mut self) {
        let preferences = Preferences {
            layout: self.layout,
            folder: self.folder_preference,
            image_info: self.info.visible,
            browse_transition: self.browse_transition_default,
            slideshow: self.slideshow_defaults,
        };
        if let Err(error) = preferences.save() {
            self.report(Err(format!("Could not save settings: {error}")));
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
        let image = matches!(kind, PickKind::Image);
        let file = Box::pin(async move {
            let handle = if image {
                dialog.set_title("Open image").pick_file().await
            } else {
                dialog.set_title("Open image folder").pick_folder().await
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
                    PickKind::Image => {
                        self.stop_slideshow(ctx);
                        self.open_file(path);
                    }
                    PickKind::Folder => {
                        self.stop_slideshow(ctx);
                        self.open_folder(path);
                    }
                    PickKind::SaveAs(image, size) => self.save_as(ctx, image, size, path),
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
            self.navigate_to(path, forward);
        }
    }

    fn navigate_to(&mut self, path: PathBuf, forward: bool) {
        let style = self
            .slideshow
            .as_ref()
            .map_or(self.browse_transition, |show| show.current.transition);
        self.open_file_with_transition(path, style, forward);
    }

    fn browse_image(&mut self, path: PathBuf) {
        let forward = self.current_index().is_none_or(|index| {
            self.entries[index + 1..]
                .iter()
                .any(|entry| entry.path == path)
        });
        self.navigate_to(path, forward);
    }

    fn prepare_neighbors(&self) {
        let paths: Vec<PathBuf> = [self.neighbor(true), self.neighbor(false)]
            .into_iter()
            .flatten()
            .collect();
        self.nearby.submit(paths);
    }

    fn has_slideshow_images(&self) -> bool {
        self.picture.is_some() && (self.neighbor(false).is_some() || self.neighbor(true).is_some())
    }

    fn start_slideshow(&mut self, ctx: &egui::Context) {
        if self.slideshow.is_some() || !self.has_slideshow_images() {
            return;
        }
        let restore_window = ctx.input(|input| input.viewport().fullscreen != Some(true));
        self.slideshow = Some(Slideshow {
            current: self.slideshow_defaults,
            paused: false,
            advance_at: None,
            restore_window,
        });
        self.transition = None;
        self.canvas.fit();
        if restore_window {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        ctx.request_repaint();
    }

    fn stop_slideshow(&mut self, ctx: &egui::Context) {
        if let Some(show) = self.slideshow.take() {
            self.transition = None;
            if show.restore_window {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
            ctx.request_repaint();
        }
    }

    fn tick_slideshow(&mut self, ctx: &egui::Context) {
        let Some(show) = &mut self.slideshow else {
            return;
        };
        if show.paused
            || self.loading
            || self.picker.is_some()
            || self.settings_open
            || self.export.is_some()
            || self.save_confirmation.is_some()
            || self.editing_image
            || self.printing
            || egui::Popup::is_any_open(ctx)
        {
            show.advance_at = None;
            return;
        }
        let now = ctx.input(|input| input.time);
        let due = *show
            .advance_at
            .get_or_insert(now + f64::from(show.current.seconds));
        if due > now {
            ctx.request_repaint_after(Duration::from_secs_f64(due - now));
            return;
        }
        let repeat = show.current.repeat;
        let next = self.neighbor(true).or_else(|| {
            repeat
                .then(|| self.entries.iter().find(|entry| !entry.directory))
                .flatten()
                .filter(|entry| Some(&entry.path) != self.file.as_ref())
                .map(|entry| entry.path.clone())
        });
        if let Some(path) = next {
            self.navigate_to(path, true);
        } else if let Some(show) = &mut self.slideshow {
            show.paused = true;
            show.advance_at = None;
        }
    }

    fn slideshow_bar(&mut self, ui: &mut egui::Ui) {
        use panel_ui::{Glyph, dialog_button};
        let (can_previous, can_next) = (
            self.neighbor(false).is_some(),
            self.neighbor(true).is_some(),
        );
        let Some(show) = &mut self.slideshow else {
            return;
        };
        let mut direction = None;
        let mut stop = false;
        let mut toggle_play = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Slideshow");
            ui.separator();
            if ui
                .add_enabled(can_previous, egui::Button::new("‹"))
                .on_hover_text("Previous image")
                .clicked()
            {
                direction = Some(false);
            }
            let (icon, action) = if !show.paused {
                (Glyph::Pause, "Pause")
            } else if !show.current.repeat && !can_next {
                (Glyph::Refresh, "Replay")
            } else {
                (Glyph::Play, "Play")
            };
            toggle_play = dialog_button(ui, icon, action, false)
                .on_hover_text("Space")
                .clicked();
            if ui
                .add_enabled(can_next, egui::Button::new("›"))
                .on_hover_text("Next image")
                .clicked()
            {
                direction = Some(true);
            }
            ui.separator();
            if slideshow_controls(ui, &mut show.current, "current-slideshow-transition") {
                show.advance_at = None;
            }
            ui.separator();
            stop = dialog_button(ui, Glyph::Close, "End Show", false)
                .on_hover_text("Esc")
                .clicked();
        });
        if let Some(forward) = direction {
            self.navigate(forward);
        }
        if toggle_play {
            self.toggle_slideshow_pause();
        }
        if stop {
            self.stop_slideshow(ui.ctx());
        }
    }

    fn toggle_slideshow_pause(&mut self) {
        let can_next = self.neighbor(true).is_some();
        let Some(show) = &mut self.slideshow else {
            return;
        };
        let replay = show.paused && !show.current.repeat && !can_next;
        show.paused = !show.paused;
        show.advance_at = None;
        if replay
            && let Some(path) = self
                .entries
                .iter()
                .find(|entry| !entry.directory)
                .map(|entry| entry.path.clone())
        {
            self.navigate_to(path, true);
        }
    }

    fn copy(&mut self, ctx: &egui::Context, show_bediz: bool) {
        if let Some(image) = self.output_image() {
            self.copy_image(ctx, image, Size::Original, show_bediz);
        }
    }

    fn copy_image(
        &mut self,
        ctx: &egui::Context,
        image: ExportImage,
        size: Size,
        show_bediz: bool,
    ) {
        if self.copying {
            return;
        }
        let post = post(self.sender.clone(), ctx.clone());
        self.copying = true;
        std::thread::spawn(move || {
            post(Reply::Pixels(
                output::render(&image.source.pixels, image.angle, size),
                show_bediz,
            ))
        });
    }

    fn output_image(&self) -> Option<ExportImage> {
        Some(ExportImage {
            path: self.file.clone()?,
            source: self.picture.as_ref()?.0.clone(),
            original: self.original.clone()?,
            angle: self.canvas.angle,
        })
    }

    fn print_image(&mut self, ctx: &egui::Context) {
        if self.printing {
            return;
        }
        if let Some(image) = self.output_image() {
            self.printing = true;
            let post = post(self.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                let result = canvas::oriented_pixels(&image.source.pixels, image.angle);
                post(Reply::Printable(result, path_name(&image.path)));
            });
        }
    }

    fn can_save_image(&self) -> bool {
        self.picture.is_some()
            && (self.canvas.angle != 0. || self.has_pixel_edits())
            && !self.editing_image
            && !self.saving_image
            && !self.info.saving
            && self
                .file
                .as_deref()
                .and_then(output::Format::from_path)
                .is_some()
    }

    fn confirm_save_image(&mut self) {
        if self.can_save_image() {
            self.save_confirmation = self.output_image();
        }
    }

    fn open_export(&mut self, kind: ExportKind) {
        let busy = match kind {
            ExportKind::Copy => self.copying,
            ExportKind::SaveAs => self.saving_image || self.picker.is_some(),
        };
        if !busy && let Some(image) = self.output_image() {
            self.export = Some(ExportOptions {
                kind,
                image,
                size: Size::Original,
            });
        }
    }

    fn pick_export_path(&mut self, image: ExportImage, size: Size) {
        let stem = image.path.file_stem().unwrap_or_default().to_string_lossy();
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title("Save image as")
            .set_file_name(format!("{stem} copy.png"))
            .add_filter("Images (PNG, JPEG, WebP)", &["png", "jpg", "jpeg", "webp"]);
        if let Some(folder) = image.path.parent() {
            dialog = dialog.set_directory(folder);
        }
        let file =
            Box::pin(async move { dialog.save_file().await.map(|file| file.path().to_owned()) });
        self.picker = Some(Picker {
            kind: PickKind::SaveAs(image, size),
            file,
        });
    }

    fn save_as(
        &mut self,
        ctx: &egui::Context,
        image: ExportImage,
        size: Size,
        mut target: PathBuf,
    ) {
        let replace = target.extension().is_some() && target.exists();
        if target.extension().is_none() {
            target.set_extension("png");
        }
        if image.path == target
            || (replace
                && target
                    .canonicalize()
                    .ok()
                    .is_some_and(|path| image.path.canonicalize().ok() == Some(path)))
        {
            self.report(Err("Choose a new filename for the exported image.".into()));
            return;
        }
        self.saving_image = true;
        let post = post(self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = output::save_as(&target, &image.source.pixels, image.angle, size, replace)
                .map(|_| target);
            post(Reply::SavedImage(result, false));
        });
    }

    fn export_options(&mut self, ctx: &egui::Context) {
        use panel_ui::{Glyph, dialog_button, dialog_title};
        let Some(options) = &mut self.export else {
            return;
        };
        let (title, submit, glyph) = match options.kind {
            ExportKind::Copy => ("Copy Image", "Copy", Glyph::Copy),
            ExportKind::SaveAs => ("Save Image As", "Choose File…", Glyph::Export),
        };
        let image = &options.image;
        let response = egui::Modal::new(egui::Id::new("seyir-export-options"))
            .frame(crate::theme::floating(ctx).inner_margin(egui::Margin::same(18)))
            .show(ctx, |ui| {
                ui.set_width(306.);
                dialog_title(ui, glyph, title, Some(&path_name(&image.path)));
                ui.add_space(12.);
                ui.strong("Output size");
                let dimensions =
                    canvas::oriented_size(image.source.pixels.dimensions(), image.angle);
                let size = &mut options.size;
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(size, Size::Original, "Original");
                    ui.selectable_value(size, Size::Percent(75), "75%");
                    ui.selectable_value(size, Size::Percent(50), "50%");
                    ui.selectable_value(size, Size::Percent(25), "25%");
                    if ui
                        .selectable_label(matches!(size, Size::Fit { .. }), "Custom")
                        .clicked()
                        && !matches!(size, Size::Fit { .. })
                    {
                        *size = Size::Fit {
                            width: dimensions.0,
                            height: dimensions.1,
                        };
                    }
                });
                if let Size::Fit { width, height } = size {
                    ui.add_space(5.);
                    ui.horizontal(|ui| {
                        ui.label("Fit within");
                        ui.add(egui::DragValue::new(width).range(1..=32_768));
                        ui.label("×");
                        ui.add(egui::DragValue::new(height).range(1..=32_768));
                        ui.weak("px");
                    });
                }
                let preview = output::preview_dimensions(&image.source.pixels, image.angle, *size);
                ui.add_space(4.);
                match &preview {
                    Ok((width, height)) => {
                        ui.weak(format!("{width} × {height} px"));
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                }
                ui.add_space(12.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let proceed = ui
                        .add_enabled_ui(preview.is_ok(), |ui| {
                            dialog_button(ui, glyph, submit, true)
                        })
                        .inner
                        .clicked();
                    if dialog_button(ui, Glyph::Close, "Cancel", false).clicked() {
                        ui.close();
                    }
                    proceed
                })
                .inner
            });
        if response.inner {
            let options = self.export.take().expect("open output options");
            match options.kind {
                ExportKind::Copy => self.copy_image(ctx, options.image, options.size, false),
                ExportKind::SaveAs => self.pick_export_path(options.image, options.size),
            }
        } else if response.should_close() {
            self.export = None;
        }
    }

    fn save_original_prompt(&mut self, ctx: &egui::Context) {
        use panel_ui::{Glyph, dialog_button, dialog_title};
        let Some(image) = &self.save_confirmation else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("seyir-save-original"))
            .frame(crate::theme::floating(ctx).inner_margin(egui::Margin::same(18)))
            .show(ctx, |ui| {
                ui.set_width(328.);
                dialog_title(ui, Glyph::Save, "Save Image", Some(&path_name(&image.path)));
                ui.add_space(10.);
                ui.label("Apply the displayed edits to the original image?");
                ui.weak("Seyir will re-encode the original file. Some format-specific metadata may be lost.");
                ui.add_space(12.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = dialog_button(ui, Glyph::Save, "Replace Original", true).clicked();
                    if dialog_button(ui, Glyph::Close, "Cancel", false).clicked() {
                        ui.close();
                    }
                    save
                }).inner
            });
        if response.inner {
            let image = self
                .save_confirmation
                .take()
                .expect("open save confirmation");
            self.saving_image = true;
            let post = post(self.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                let result = output::save_original(
                    &image.path,
                    &image.original.pixels,
                    &image.source.pixels,
                    image.angle,
                )
                .map(|_| image.path);
                post(Reply::SavedImage(result, true));
            });
        } else if response.should_close() {
            self.save_confirmation = None;
        }
    }

    fn edit(&mut self, ctx: &egui::Context, rendered_copy: bool) {
        let Some(path) = self.file.clone() else {
            return;
        };
        let Some((source, _)) = &self.picture else {
            return;
        };
        let source = source.clone();
        let rendered_copy = rendered_copy || self.has_pixel_edits();
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
        if self.picker.is_some()
            || self.export.is_some()
            || self.save_confirmation.is_some()
            || self.crop.is_some()
        {
            return;
        }
        if self.slideshow.is_none() && ctx.input(|input| input.viewport().fullscreen == Some(true))
        {
            if !egui::Popup::is_any_open(ctx)
                && ctx.input(|input| {
                    input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::F11)
                })
            {
                self.toggle_fullscreen(ctx);
            }
            return;
        }
        let settings_shortcut =
            egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma);
        if ctx.input_mut(|input| input.consume_shortcut(&settings_shortcut)) {
            self.settings_open = !self.settings_open;
            return;
        }
        if self.settings_open {
            return;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let text_focused = focused.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if text_focused || egui::Popup::is_any_open(ctx) {
            return;
        }
        let pressed = |key| ctx.input(|input| input.key_pressed(key));
        if self.slideshow.is_some() {
            if pressed(egui::Key::Escape) {
                self.stop_slideshow(ctx);
            } else if pressed(egui::Key::Space) && focused.is_none() {
                self.toggle_slideshow_pause();
            } else if pressed(egui::Key::ArrowLeft) {
                self.navigate(false);
            } else if pressed(egui::Key::ArrowRight) {
                self.navigate(true);
            }
            return;
        }
        let command = |key| {
            ctx.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key))
            })
        };
        if command(egui::Key::O) {
            self.pick(PickKind::Image);
        }
        if command(egui::Key::P) {
            self.print_image(ctx);
        }
        let save_as = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::S,
        );
        if ctx.input_mut(|input| input.consume_shortcut(&save_as)) {
            self.open_export(ExportKind::SaveAs);
        } else if command(egui::Key::S) {
            self.confirm_save_image();
        }
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
            self.toggle_fullscreen(ctx);
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let full = ctx.input(|input| input.viewport().fullscreen == Some(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
    }

    fn settings(&mut self, ctx: &egui::Context) {
        use panel_ui::{Glyph, dialog_button, dialog_title};
        let frame = crate::theme::floating(ctx).inner_margin(egui::Margin::same(18));
        let mut browser = self.browser;
        let mut image_info = self.info.visible;
        let mut browse_transition = self.browse_transition_default;
        let mut slideshow = self.slideshow_defaults;
        let response = egui::Modal::new(egui::Id::new("seyir-settings"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(338.);
                dialog_title(ui, Glyph::Settings, "Settings", None);
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    if dialog_button(ui, Glyph::Interface, "Interface", !self.settings_playback)
                        .clicked()
                    {
                        self.settings_playback = false;
                    }
                    if dialog_button(ui, Glyph::Play, "Playback", self.settings_playback).clicked()
                    {
                        self.settings_playback = true;
                    }
                });
                ui.add_space(6.);
                let selected = egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height((ctx.content_rect().height() - 158.).max(160.))
                    .show(ui, |ui| {
                        if self.settings_playback {
                            ui.strong("Folder navigation");
                            ui.horizontal(|ui| {
                                ui.label("Transition");
                                transition_picker(
                                    ui,
                                    "folder-default-transition",
                                    &mut browse_transition,
                                );
                            });
                            ui.add_space(8.);
                            ui.separator();
                            ui.strong("Slideshow defaults");
                            ui.horizontal_wrapped(|ui| {
                                slideshow_controls(
                                    ui,
                                    &mut slideshow,
                                    "slideshow-default-transition",
                                );
                            });
                            return None;
                        }
                        ui.strong("Interface layout");
                        ui.add_space(3.);
                        let selected = layout::choices(ui, self.layout);
                        ui.add_space(8.);
                        ui.separator();
                        ui.strong("Panels");
                        ui.add_space(3.);
                        panel_ui::setting_switch(
                            ui,
                            panel_ui::Glyph::Folder,
                            "Folder browser",
                            &mut browser,
                        );
                        panel_ui::setting_switch(
                            ui,
                            panel_ui::Glyph::Info,
                            "Image properties",
                            &mut image_info,
                        );
                        selected
                    })
                    .inner;
                ui.add_space(12.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if dialog_button(ui, Glyph::Check, "Done", true).clicked() {
                        ui.close();
                    }
                });
                selected
            });
        self.set_browser_visible(browser);
        self.set_info_visible(image_info);
        if browse_transition != self.browse_transition_default
            || slideshow != self.slideshow_defaults
        {
            if browse_transition != self.browse_transition_default {
                self.browse_transition = browse_transition;
            }
            self.browse_transition_default = browse_transition;
            self.slideshow_defaults = slideshow;
            self.persist_settings();
        }
        if let Some(selected) = response.inner {
            self.next_layout = Some(selected);
            ctx.request_repaint();
        }
        if response.should_close() {
            self.settings_open = false;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, floating: bool) {
        use crate::chrome;

        let width = ui.available_width();
        chrome::row(ui, |ui| {
            if !floating {
                chrome::brand(ui, &self.app_icon);
            }
            self.toolbar_tools(ui, !floating && width >= 870.);
        });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            crate::chrome::brand(ui, &self.app_icon);
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
            chrome::file_menu(ui, |ui| self.file_actions(ui));
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
                self.set_browser_visible(!self.browser);
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
            let tools = self.image_transform_tools();
            if let (_, Some(action)) = chrome::rotation(ui, &mut self.canvas, has_picture, tools) {
                self.apply_image_transform(ui.ctx(), action);
            }
        });
        chrome::group(ui, |ui| {
            if chrome::copy_menu(ui, has_picture && !self.copying, |ui| self.copy_actions(ui))
                .clicked()
            {
                self.copy(ui.ctx(), false);
            }
            if let Some(compatible) = self.bediz {
                if compatible {
                    let original = self.file.as_deref().is_some_and(bediz_original_supported)
                        && !self.has_pixel_edits();
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
            if Tile::new(Icon::Play, "Start Slideshow")
                .enabled(self.slideshow.is_none() && self.has_slideshow_images())
                .show(ui)
                .clicked()
            {
                self.start_slideshow(ui.ctx());
            }
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
                self.toggle_fullscreen(ui.ctx());
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
                self.browse_image(path);
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
                self.browse_image(path);
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
                ui.add_enabled_ui(!self.info.saving && !self.saving_image, |ui| {
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
            self.stop_slideshow(ctx);
            self.open_path(path);
        }
        self.shortcuts(ctx);
        self.tick_slideshow(ctx);

        self.workspace(ctx);
        if self.settings_open {
            self.settings(ctx);
        } else if self.export.is_some() {
            self.export_options(ctx);
        } else if self.save_confirmation.is_some() {
            self.save_original_prompt(ctx);
        } else if self.crop.is_some() {
            self.show_crop(ctx);
        }
    }
}

impl Viewer {
    fn workspace(&mut self, ctx: &egui::Context) {
        use crate::theme;

        let layout = self.layout;
        let playing = self.slideshow.is_some();
        let fullscreen = ctx.input(|input| input.viewport().fullscreen == Some(true));
        if playing || fullscreen {
            if playing {
                egui::TopBottomPanel::bottom("slideshow-controls")
                    .frame(theme::bar(ctx, 7))
                    .show(ctx, |ui| self.slideshow_bar(ui));
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::canvas(ctx, layout)))
                .show(ctx, |ui| self.image_stage(ui, !playing));
            return;
        }
        let window_width = ctx.content_rect().width();
        let floating = layout == Layout::FloatingControls;
        if !matches!(
            layout,
            Layout::ToolRail | Layout::FocusMode | Layout::FloatingControls
        ) {
            egui::TopBottomPanel::top("toolbar")
                .frame(theme::bar(
                    ctx,
                    if layout == Layout::StudioDesk { 9 } else { 7 },
                ))
                .show(ctx, |ui| self.toolbar(ui, false));
        }
        if !floating {
            egui::TopBottomPanel::bottom("status")
                .frame(theme::bar(ctx, 5))
                .show(ctx, |ui| self.status_bar(ui));
        }

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
                Layout::FocusMode | Layout::FloatingControls => {}
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
            .show(ctx, |ui| self.image_stage(ui, false))
            .response
            .rect;
        if folder && matches!(layout, Layout::FocusMode | Layout::FloatingControls) {
            self.floating_folder(ctx, stage);
        }
        if layout == Layout::FocusMode {
            self.focus_tools(ctx, stage);
        } else if floating {
            self.floating_controls(ctx, stage);
        }
        if info && layout != Layout::StudioDesk {
            let mut open = true;
            egui::Window::new("Image Properties")
                .id(egui::Id::new("floating-image-info"))
                .open(&mut open)
                .default_pos(stage.right_top() + egui::vec2(-270., 10.))
                .constrain_to(stage)
                .default_width(246.)
                .min_width(204.)
                .max_height((stage.height() - 30.).max(140.))
                .frame(theme::floating(ctx))
                .show(ctx, |ui| self.inspector(ui, true));
            if !open {
                self.set_info_visible(false);
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        use crate::chrome::{Icon, Tile};

        let settings_hint = if cfg!(target_os = "macos") {
            "Settings · ⌘,"
        } else {
            "Settings · Ctrl+,"
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Tile::new(Icon::Settings, "Settings…")
                .hint(settings_hint)
                .show(ui)
                .clicked()
            {
                self.settings_open = true;
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
        });
    }

    fn floating_folder(&mut self, ctx: &egui::Context, stage: egui::Rect) {
        use crate::theme;

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

    fn focus_tools(&mut self, ctx: &egui::Context, stage: egui::Rect) {
        use crate::theme;

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

    fn floating_controls(&mut self, ctx: &egui::Context, stage: egui::Rect) {
        use crate::chrome::{Icon, Tile};
        use crate::theme;

        let popup_id = egui::Id::new("floating-options");
        let options_open = egui::Popup::is_id_open(ctx, popup_id);
        let has_picture = self.picture.is_some();
        let dock = egui::Area::new(egui::Id::new("floating-controls"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(stage.center_bottom() - egui::vec2(0., 16.))
            .constrain_to(stage)
            .show(ctx, |ui| {
                theme::floating(ctx)
                    .corner_radius(16)
                    .inner_margin(egui::Margin::symmetric(9, 5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.;
                            if !has_picture
                                && Tile::new(Icon::OpenImage, "Open Image…")
                                    .icon_only()
                                    .show(ui)
                                    .clicked()
                            {
                                self.pick(PickKind::Image);
                            }
                            if Tile::new(Icon::ZoomOut, "Zoom Out")
                                .icon_only()
                                .enabled(has_picture)
                                .show(ui)
                                .clicked()
                            {
                                self.canvas.step_zoom(0.8);
                            }
                            if Tile::new(Icon::ZoomIn, "Zoom In")
                                .icon_only()
                                .enabled(has_picture)
                                .show(ui)
                                .clicked()
                            {
                                self.canvas.step_zoom(1.25);
                            }
                            ui.separator();
                            if Tile::new(Icon::Previous, "Previous Image")
                                .icon_only()
                                .hint("Previous Image · Left arrow")
                                .enabled(self.neighbor(false).is_some())
                                .show(ui)
                                .clicked()
                            {
                                self.navigate(false);
                            }
                            if Tile::new(Icon::Next, "Next Image")
                                .icon_only()
                                .hint("Next Image · Right arrow")
                                .enabled(self.neighbor(true).is_some())
                                .show(ui)
                                .clicked()
                            {
                                self.navigate(true);
                            }
                            ui.separator();
                            let more = Tile::new(Icon::More, "More Tools")
                                .icon_only()
                                .selected(options_open)
                                .show(ui);
                            if Tile::new(Icon::Play, "Start Slideshow")
                                .icon_only()
                                .enabled(self.has_slideshow_images())
                                .show(ui)
                                .clicked()
                            {
                                self.start_slideshow(ctx);
                            }
                            let fullscreen =
                                ui.input(|input| input.viewport().fullscreen == Some(true));
                            let label = if fullscreen {
                                "Exit Full Screen · F11"
                            } else {
                                "Full Screen · F11"
                            };
                            if Tile::new(Icon::Fullscreen, label)
                                .icon_only()
                                .selected(fullscreen)
                                .show(ui)
                                .clicked()
                            {
                                self.toggle_fullscreen(ctx);
                            }
                            more
                        })
                        .inner
                    })
                    .inner
            });
        egui::Popup::menu(&dock.inner)
            .id(popup_id)
            .anchor(dock.response.rect)
            .align(egui::RectAlign::TOP)
            .gap(8.)
            .frame(theme::floating(ctx).inner_margin(egui::Margin::symmetric(13, 10)))
            .show(|ui| self.floating_options(ui));

        if !self.loading
            && !self.status.is_empty()
            && !(self.error && self.picture.is_none() && self.file.is_some())
        {
            egui::Area::new(egui::Id::new("floating-action-status"))
                .order(egui::Order::Foreground)
                .interactable(false)
                .pivot(egui::Align2::CENTER_TOP)
                .fixed_pos(stage.center_top() + egui::vec2(0., 12.))
                .constrain_to(stage)
                .show(ctx, |ui| {
                    theme::floating(ctx)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_max_width((stage.width() - 36.).clamp(170., 440.));
                            let ink = if self.error {
                                ui.visuals().error_fg_color
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.label(RichText::new(&self.status).size(12.).color(ink));
                        });
                });
        }
    }

    fn floating_options(&mut self, ui: &mut egui::Ui) {
        use crate::chrome::{self, Icon, menu_item};

        let column = egui::Layout::top_down_justified(egui::Align::Min);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(9., 3.);
            ui.with_layout(column, |ui| {
                ui.set_width(165.);
                ui.strong("File");
                self.file_actions(ui);
            });
            ui.separator();
            ui.with_layout(column, |ui| {
                ui.set_width(183.);
                ui.strong("Image");
                let has_picture = self.picture.is_some();
                if menu_item(ui, Icon::Fit, "Fit Image", has_picture, "0") {
                    self.canvas.fit();
                }
                if menu_item(ui, Icon::Actual, "Actual Pixels", has_picture, "1") {
                    self.canvas.set_zoom(1.);
                }
                let tools = self.image_transform_tools();
                if let Some(action) =
                    chrome::rotation_item(ui, &mut self.canvas, has_picture, tools)
                {
                    self.apply_image_transform(ui.ctx(), action);
                }
                self.copy_actions(ui);
                self.bediz_menu_action(ui, has_picture);
            });
            ui.separator();
            ui.with_layout(column, |ui| {
                ui.set_width(165.);
                ui.strong("View");
                let folder = if self.browser {
                    "Hide Folder Browser"
                } else {
                    "Show Folder Browser"
                };
                if menu_item(ui, Icon::FolderPane, folder, self.folder.is_some(), "") {
                    self.set_browser_visible(!self.browser);
                }
                let info = if self.info.visible {
                    "Hide Image Properties"
                } else {
                    "Show Image Properties"
                };
                if menu_item(ui, Icon::Info, info, self.file.is_some(), "") {
                    self.toggle_info();
                }
                let settings = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::Comma,
                ));
                if menu_item(ui, Icon::Settings, "Settings…", true, &settings) {
                    self.settings_open = true;
                }
            });
        });
    }

    fn image_stage(&mut self, ui: &mut egui::Ui, quiet: bool) {
        if let Some((image, texture)) = &self.picture {
            let progress = self.transition.as_ref().and_then(|transition| {
                transition
                    .started
                    .map(|start| ((ui.input(|input| input.time) - start) / 0.24) as f32)
            });
            if progress.is_some_and(|progress| progress >= 1.) {
                self.transition = None;
            }
            let transition = self
                .transition
                .as_ref()
                .zip(progress)
                .map(|(state, progress)| {
                    ui.ctx().request_repaint();
                    canvas::Transition {
                        style: state.style,
                        previous: &state.previous,
                        texture: state.texture.id(),
                        size: state.size,
                        progress,
                        forward: state.forward,
                    }
                });
            let (canvas, image_clicked) = self.canvas.paint(
                ui,
                texture.id(),
                [image.pixels.width(), image.pixels.height()],
                transition,
            );
            if canvas.secondary_clicked() && !image_clicked {
                egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&canvas));
            } else {
                egui::Popup::context_menu(&canvas).show(|ui| self.image_context_menu(ui));
            }
        } else if let Some(transition) = &self.transition {
            transition
                .previous
                .paint_still(ui, transition.texture.id(), transition.size);
        } else if quiet {
            let (_, canvas) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
            egui::Popup::context_menu(&canvas).show(|ui| {
                self.file_actions(ui);
                self.fullscreen_action(ui);
            });
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

    fn image_transform_tools(&self) -> crate::chrome::TransformTools {
        crate::chrome::TransformTools {
            enabled: self.can_transform_image(),
            can_undo: self.image_undo.is_some(),
        }
    }

    fn image_transform_actions(&mut self, ui: &mut egui::Ui) {
        if let Some(action) = self.image_transform_tools().show(ui) {
            self.apply_image_transform(ui.ctx(), action);
        }
    }

    fn apply_image_transform(
        &mut self,
        ctx: &egui::Context,
        action: crate::chrome::TransformAction,
    ) {
        use crate::chrome::TransformAction;

        egui::Popup::close_all(ctx);
        match action {
            TransformAction::Crop => self.start_crop(ctx),
            TransformAction::FlipHorizontal => self.flip_image(ctx, true),
            TransformAction::FlipVertical => self.flip_image(ctx, false),
            TransformAction::Undo => self.undo_image_edit(),
        }
    }

    fn bediz_menu_action(&mut self, ui: &mut egui::Ui, has_picture: bool) {
        use crate::chrome::{Icon, menu_item};

        let fallback = self.bediz == Some(false);
        let label = if fallback {
            "Copy and Open Bediz"
        } else {
            "Edit in Bediz"
        };
        let compatible = self.bediz == Some(true) || (fallback && !self.copying);
        if menu_item(ui, Icon::Edit, label, has_picture && compatible, "") {
            if fallback {
                self.copy(ui.ctx(), true);
            } else {
                let original = self.file.as_deref().is_some_and(bediz_original_supported)
                    && !self.has_pixel_edits();
                self.edit(ui.ctx(), !original || self.canvas.angle != 0.);
            }
        }
    }

    fn file_actions(&mut self, ui: &mut egui::Ui) {
        use crate::chrome::{Icon, menu_item};

        let open = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::O,
        ));
        if menu_item(ui, Icon::OpenImage, "Open Image…", true, &open) {
            self.pick(PickKind::Image);
        }
        if menu_item(ui, Icon::OpenFolder, "Open Folder…", true, "") {
            self.pick(PickKind::Folder);
        }
        ui.separator();
        if menu_item(ui, Icon::Save, "Save Image", self.can_save_image(), "") {
            self.confirm_save_image();
        }
        let has_picture = self.picture.is_some();
        if menu_item(
            ui,
            Icon::SaveAs,
            "Save As…",
            has_picture && !self.saving_image && self.picker.is_none(),
            "",
        ) {
            self.open_export(ExportKind::SaveAs);
        }
        if menu_item(ui, Icon::Print, "Print…", has_picture && !self.printing, "") {
            self.print_image(ui.ctx());
        }
    }

    fn copy_actions(&mut self, ui: &mut egui::Ui) {
        use crate::chrome::{Icon, menu_item};

        let can_copy = self.picture.is_some() && !self.copying;
        let copy = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::C,
        ));
        if menu_item(ui, Icon::Copy, "Copy Image", can_copy, &copy) {
            self.copy(ui.ctx(), false);
        }
        if menu_item(ui, Icon::Copy, "Copy Resized…", can_copy, "") {
            self.open_export(ExportKind::Copy);
        }
    }

    fn image_context_menu(&mut self, ui: &mut egui::Ui) {
        use crate::chrome::{Icon, menu_item};

        ui.set_min_width(195.);
        ui.spacing_mut().item_spacing.y = 2.;
        if menu_item(
            ui,
            Icon::Previous,
            "Previous Image",
            self.neighbor(false).is_some(),
            "←",
        ) {
            self.navigate(false);
        }
        if menu_item(
            ui,
            Icon::Next,
            "Next Image",
            self.neighbor(true).is_some(),
            "→",
        ) {
            self.navigate(true);
        }
        if self.slideshow.is_some() {
            if menu_item(ui, Icon::Play, "End Slideshow", true, "Esc") {
                self.stop_slideshow(ui.ctx());
            }
        } else if menu_item(
            ui,
            Icon::Play,
            "Start Slideshow",
            self.has_slideshow_images(),
            "",
        ) {
            self.start_slideshow(ui.ctx());
        }
        let current_transition = self
            .slideshow
            .as_mut()
            .map_or(&mut self.browse_transition, |show| {
                &mut show.current.transition
            });
        ui.menu_button("Transition", |ui| {
            for style in canvas::TransitionStyle::ALL {
                if ui
                    .selectable_label(*current_transition == style, style.label())
                    .clicked()
                {
                    *current_transition = style;
                    ui.close();
                }
            }
        });
        ui.separator();
        self.bediz_menu_action(ui, self.picture.is_some());
        self.copy_actions(ui);
        ui.separator();
        ui.menu_button("Rotate and Edit", |ui| {
            if menu_item(ui, Icon::RotateLeft, "Rotate Left 90°", true, "") {
                self.canvas.rotate_by(-90.);
            }
            if menu_item(ui, Icon::Rotate, "Rotate Right 90°", true, "") {
                self.canvas.rotate_by(90.);
            }
            if menu_item(
                ui,
                Icon::Rotate,
                "Reset Rotation",
                self.canvas.angle != 0.,
                "",
            ) {
                self.canvas.angle = 0.;
            }
            ui.separator();
            self.image_transform_actions(ui);
        });
        ui.menu_button("Zoom", |ui| {
            if menu_item(ui, Icon::ZoomIn, "Zoom In", true, "") {
                self.canvas.step_zoom(1.25);
            }
            if menu_item(ui, Icon::ZoomOut, "Zoom Out", true, "") {
                self.canvas.step_zoom(0.8);
            }
            if menu_item(ui, Icon::Fit, "Fit Image", true, "") {
                self.canvas.fit();
            }
            if menu_item(ui, Icon::Actual, "Actual Pixels", true, "") {
                self.canvas.set_zoom(1.);
            }
        });
        ui.separator();
        self.file_actions(ui);
        self.fullscreen_action(ui);
    }

    fn fullscreen_action(&mut self, ui: &mut egui::Ui) {
        if self.slideshow.is_some() {
            return;
        }
        let full = ui.input(|input| input.viewport().fullscreen == Some(true));
        let label = if full {
            "Exit Full Screen"
        } else {
            "Full Screen"
        };
        let shortcut = if full { "Esc" } else { "F11" };
        if crate::chrome::menu_item(ui, crate::chrome::Icon::Fullscreen, label, true, shortcut) {
            self.toggle_fullscreen(ui.ctx());
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

fn transition_picker(ui: &mut egui::Ui, id: &str, selected: &mut TransitionStyle) {
    egui::ComboBox::from_id_salt(id)
        .width(86.)
        .selected_text(selected.label())
        .show_ui(ui, |ui| {
            for style in TransitionStyle::ALL {
                ui.selectable_value(selected, style, style.label());
            }
        });
}

fn slideshow_controls(ui: &mut egui::Ui, settings: &mut SlideshowDefaults, id: &str) -> bool {
    let before = *settings;
    ui.label("Interval");
    ui.add(
        egui::DragValue::new(&mut settings.seconds)
            .range(1..=60)
            .suffix(" s"),
    );
    ui.checkbox(&mut settings.repeat, "Loop");
    ui.label("Transition");
    transition_picker(ui, id, &mut settings.transition);
    *settings != before
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_more_opens_settings_on_an_empty_canvas_at_minimum_size() {
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut viewer = Viewer::new(&creation, OpenQueue::default());
        let stage = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(680., 420.));
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(stage),
                    events,
                    ..Default::default()
                },
                |ctx| viewer.floating_controls(ctx, stage),
            )
        };
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let label = |output: &egui::FullOutput, label: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
        };

        frame(vec![]);
        let output = frame(vec![]);
        assert!(
            label(&output, "More").is_none(),
            "the dock uses icons alone"
        );
        let more_dots: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(dot) if dot.radius == 1.45 => Some(dot.center),
                _ => None,
            })
            .collect();
        assert_eq!(more_dots.len(), 3);
        let more = more_dots[1];
        frame(vec![egui::Event::PointerMoved(more), click(more, true)]);
        frame(vec![click(more, false)]);
        let output = frame(vec![]);
        assert!(label(&output, "Open Image…").is_some());
        let settings = label(&output, "Settings…").expect("More shows Settings without an image");
        let popup = ctx
            .read_response(egui::Id::new("floating-options"))
            .expect("the options panel has a distinct popup");
        assert!(stage.contains_rect(popup.rect));
        assert!(popup.rect.bottom() < more.y);

        frame(vec![
            egui::Event::PointerMoved(settings),
            click(settings, true),
        ]);
        frame(vec![click(settings, false)]);
        assert!(viewer.settings_open);
    }
}
