//! Layout choices and per-user settings shared by both platforms.
use crate::{canvas::TransitionStyle, theme};
use eframe::egui::{self, Color32, FontId, Rect, Stroke, StrokeKind, Ui, vec2};
use std::{fs, io, io::Write as _, path::Path, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Layout {
    #[default]
    StudioDesk,
    Filmstrip,
    ContactSheet,
    FocusMode,
    ToolRail,
    FloatingControls,
}

impl Layout {
    const ALL: [Self; 6] = [
        Self::StudioDesk,
        Self::Filmstrip,
        Self::ContactSheet,
        Self::FocusMode,
        Self::ToolRail,
        Self::FloatingControls,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::StudioDesk => "Studio Desk",
            Self::Filmstrip => "Filmstrip",
            Self::ContactSheet => "Contact Sheet",
            Self::FocusMode => "Focus Mode",
            Self::ToolRail => "Tool Rail",
            Self::FloatingControls => "Floating Controls",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::StudioDesk => "studio-desk",
            Self::Filmstrip => "filmstrip",
            Self::ContactSheet => "contact-sheet",
            Self::FocusMode => "focus-mode",
            Self::ToolRail => "tool-rail",
            Self::FloatingControls => "floating-controls",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::StudioDesk => "Side panels beside the canvas",
            Self::Filmstrip => "Neighboring images beneath the canvas",
            Self::ContactSheet => "Thumbnail grid with a large preview",
            Self::FocusMode => "Large canvas with floating controls",
            Self::ToolRail => "Vertical controls and a right-side folder",
            Self::FloatingControls => "Compact toolbar with a second row of tools",
        }
    }

    pub(crate) fn shows_folder_by_default(self) -> bool {
        !matches!(self, Self::FocusMode | Self::FloatingControls)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Preferences {
    pub layout: Layout,
    pub folder: Option<bool>,
    pub image_info: bool,
    pub browse_transition: TransitionStyle,
    pub slideshow: SlideshowDefaults,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlideshowDefaults {
    pub seconds: u32,
    pub repeat: bool,
    pub transition: TransitionStyle,
}

impl Default for SlideshowDefaults {
    fn default() -> Self {
        Self {
            seconds: 3,
            repeat: true,
            transition: TransitionStyle::Fade,
        }
    }
}

impl Preferences {
    pub(crate) fn load() -> Self {
        preference_path()
            .as_deref()
            .map(Self::load_at)
            .unwrap_or_default()
    }

    pub(crate) fn save(self) -> io::Result<()> {
        let path = preference_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Settings folder unavailable")
        })?;
        self.save_at(&path)
    }

    fn load_at(path: &Path) -> Self {
        let mut preferences = Self::default();
        let Ok(value) = fs::read_to_string(path) else {
            return preferences;
        };
        let mut lines = value.lines().map(str::trim);
        if let Some(id) = lines.next() {
            preferences.layout = Layout::ALL
                .into_iter()
                .find(|layout| layout.id() == id)
                .unwrap_or_default();
        }
        for setting in lines {
            let Some((key, value)) = setting.split_once('=') else {
                continue;
            };
            let transition = || {
                TransitionStyle::ALL
                    .into_iter()
                    .find(|style| style.id() == value)
            };
            match key {
                "folder" => preferences.folder = show_or_hide(value),
                "image-info" => preferences.image_info = show_or_hide(value).unwrap_or(false),
                "browse-transition" => {
                    preferences.browse_transition = transition().unwrap_or_default()
                }
                "slideshow-seconds" => {
                    if let Ok(seconds @ 1..=60) = value.parse() {
                        preferences.slideshow.seconds = seconds;
                    }
                }
                "slideshow-repeat" => {
                    if let Ok(repeat) = value.parse() {
                        preferences.slideshow.repeat = repeat;
                    }
                }
                "slideshow-transition" => {
                    if let Some(style) = transition() {
                        preferences.slideshow.transition = style;
                    }
                }
                _ => {}
            }
        }
        preferences
    }

    fn save_at(self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Missing settings folder")
        })?;
        fs::create_dir_all(parent)?;
        let mut replacement = tempfile::NamedTempFile::new_in(parent)?;
        writeln!(replacement, "{}", self.layout.id())?;
        if let Some(visible) = self.folder {
            writeln!(
                replacement,
                "folder={}",
                if visible { "show" } else { "hide" }
            )?;
        }
        if self.image_info {
            writeln!(replacement, "image-info=show")?;
        }
        writeln!(
            replacement,
            "browse-transition={}",
            self.browse_transition.id()
        )?;
        writeln!(replacement, "slideshow-seconds={}", self.slideshow.seconds)?;
        writeln!(replacement, "slideshow-repeat={}", self.slideshow.repeat)?;
        writeln!(
            replacement,
            "slideshow-transition={}",
            self.slideshow.transition.id()
        )?;
        replacement.as_file().sync_all()?;
        replacement.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn show_or_hide(value: &str) -> Option<bool> {
    match value {
        "show" => Some(true),
        "hide" => Some(false),
        _ => None,
    }
}

fn preference_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("io.github", "gokcol", "Seyir")?;
    Some(dirs.config_dir().join("layout"))
}

pub(crate) fn choices(ui: &mut Ui, current: Layout) -> Option<Layout> {
    let mut selection = None;
    for layout in Layout::ALL {
        if choice(ui, layout, current) && layout != current {
            selection = Some(layout);
        }
    }
    selection
}

fn choice(ui: &mut Ui, layout: Layout, current: Layout) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 44.), egui::Sense::click());
    let selected = layout == current;
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            response.enabled(),
            selected,
            format!("{} · {}", layout.name(), layout.description()),
        )
    });
    let visual = ui.visuals();
    let accent = visual.selection.stroke.color;
    let (selected_fill, selected_ink) =
        theme::selected_tool_colors(visual.window_fill(), accent, visual.text_color());
    let diagram_ink = if selected { selected_ink } else { accent };
    let interaction = ui.style().interact(&response);
    if selected || response.hovered() || response.has_focus() {
        let fill = if selected {
            selected_fill
        } else {
            interaction.weak_bg_fill
        };
        let stroke = if response.has_focus() {
            visual.selection.stroke
        } else {
            Stroke::NONE
        };
        ui.painter().rect(rect, 8, fill, stroke, StrokeKind::Inside);
    }
    diagram(ui, layout, rect.left_center() + vec2(18., 0.), diagram_ink);
    ui.painter().text(
        rect.left_top() + vec2(36., 7.),
        egui::Align2::LEFT_TOP,
        layout.name(),
        FontId::proportional(13.),
        visual.text_color(),
    );
    ui.painter().text(
        rect.left_top() + vec2(36., 25.),
        egui::Align2::LEFT_TOP,
        layout.description(),
        FontId::proportional(10.5),
        visual.weak_text_color(),
    );
    if selected {
        let center = rect.right_center() - vec2(12., 0.);
        ui.painter().add(egui::Shape::line(
            vec![
                center + vec2(-4., 0.),
                center + vec2(-1., 3.),
                center + vec2(4.5, -3.5),
            ],
            Stroke::new(1.5_f32, diagram_ink),
        ));
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn diagram(ui: &Ui, layout: Layout, center: egui::Pos2, accent: Color32) {
    let picture = ui.painter();
    let frame = Rect::from_center_size(center, vec2(19., 15.));
    picture.rect_stroke(frame, 2, Stroke::new(1.25_f32, accent), StrokeKind::Inside);
    let block = |x, y, w, h| {
        let at = frame.min + vec2(x, y);
        picture.rect_filled(Rect::from_min_size(at, vec2(w, h)), 0.6, accent);
    };
    match layout {
        Layout::StudioDesk => {
            block(2.5, 2.5, 2.75, 10.);
            block(13.75, 2.5, 2.75, 10.);
        }
        Layout::Filmstrip => {
            for x in [2.5, 7.75, 13.] {
                block(x, 9.5, 3.5, 3.);
            }
        }
        Layout::ContactSheet => {
            for (x, y) in [(2.5, 2.5), (7., 2.5), (2.5, 8.5), (7., 8.5)] {
                block(x, y, 3.25, 4.);
            }
        }
        Layout::FocusMode => block(5.5, 9.5, 8., 2.75),
        Layout::ToolRail => {
            for y in [2.5, 6.25, 10.] {
                block(2.5, y, 2.5, 2.5);
            }
            block(13.75, 2.5, 2.75, 10.);
        }
        Layout::FloatingControls => {
            block(4., 9.5, 11., 2.75);
            block(8., 5., 7., 2.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Preferences};

    #[test]
    fn layout_preference_survives_replacement_and_defaults_when_missing_or_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let setting = directory.path().join("config/layout");
        assert_eq!(Preferences::load_at(&setting), Preferences::default());
        for layout in Layout::ALL {
            let preferences = Preferences {
                layout,
                folder: Some(false),
                image_info: true,
                browse_transition: crate::canvas::TransitionStyle::Wipe,
                slideshow: super::SlideshowDefaults {
                    seconds: 16,
                    repeat: false,
                    transition: crate::canvas::TransitionStyle::Push,
                },
            };
            preferences.save_at(&setting).unwrap();
            assert_eq!(Preferences::load_at(&setting), preferences);
        }
        std::fs::write(&setting, "filmstrip\n").unwrap();
        assert_eq!(
            Preferences::load_at(&setting),
            Preferences {
                layout: Layout::Filmstrip,
                ..Preferences::default()
            }
        );
        std::fs::write(&setting, "unknown layout").unwrap();
        assert_eq!(Preferences::load_at(&setting), Preferences::default());
    }
}
