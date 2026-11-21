//! Layout choices and the per-user preference shared by both platforms.
use crate::theme;
use eframe::egui::{self, Color32, FontId, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use std::{fs, io, io::Write as _, path::Path, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Layout {
    #[default]
    StudioDesk,
    Filmstrip,
    ContactSheet,
    FocusMode,
    ToolRail,
}

impl Layout {
    const ALL: [Self; 5] = [
        Self::StudioDesk,
        Self::Filmstrip,
        Self::ContactSheet,
        Self::FocusMode,
        Self::ToolRail,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::StudioDesk => "Studio Desk",
            Self::Filmstrip => "Filmstrip",
            Self::ContactSheet => "Contact Sheet",
            Self::FocusMode => "Focus Mode",
            Self::ToolRail => "Tool Rail",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::StudioDesk => "studio-desk",
            Self::Filmstrip => "filmstrip",
            Self::ContactSheet => "contact-sheet",
            Self::FocusMode => "focus-mode",
            Self::ToolRail => "tool-rail",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::StudioDesk => "Folder and image info beside the canvas",
            Self::Filmstrip => "Neighboring images beneath the canvas",
            Self::ContactSheet => "Thumbnail grid with a large preview",
            Self::FocusMode => "Large canvas with floating controls",
            Self::ToolRail => "Vertical controls and a right-side folder",
        }
    }

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
        fs::read_to_string(path)
            .ok()
            .and_then(|value| {
                Self::ALL
                    .into_iter()
                    .find(|layout| layout.id() == value.trim())
            })
            .unwrap_or_default()
    }

    fn save_at(self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Missing settings folder")
        })?;
        fs::create_dir_all(parent)?;
        let mut replacement = tempfile::NamedTempFile::new_in(parent)?;
        writeln!(replacement, "{}", self.id())?;
        replacement.as_file().sync_all()?;
        replacement.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn preference_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("io.github", "gokcol", "Seyir")?;
    Some(dirs.config_dir().join("layout"))
}

pub(crate) fn picker(ui: &mut Ui, current: Layout, compact: bool) -> Option<Layout> {
    let title = FontId::proportional(13.);
    let ink = ui.visuals().text_color();
    let accent = ui.visuals().selection.stroke.color;
    let label = (!compact).then(|| {
        ui.painter()
            .layout_no_wrap(current.name().into(), title, ink)
    });
    let width = label.as_ref().map_or(33., |label| 40. + label.size().x);
    let (rect, selector) = ui.allocate_exact_size(vec2(width, 33.), egui::Sense::click());
    selector.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            selector.enabled(),
            format!("Layout · {}", current.name()),
        )
    });
    let visuals = ui.style().interact(&selector);
    let edge = if selector.has_focus() {
        ui.visuals().selection.stroke
    } else {
        visuals.bg_stroke
    };
    ui.painter().rect(
        rect.shrink2(vec2(0., 1.)),
        8,
        visuals.weak_bg_fill,
        edge,
        StrokeKind::Inside,
    );
    diagram(ui, current, rect.left_center() + vec2(16.5, 0.), accent);
    if let Some(label) = label {
        let label_at = pos2(rect.left() + 31., rect.center().y - label.size().y / 2.);
        ui.painter().galley(label_at, label, ink);
    }

    let selection = egui::Popup::menu(&selector)
        .width(274.)
        .show(|ui| {
            ui.set_min_width(266.);
            let mut selection = None;
            for layout in Layout::ALL {
                if choice(ui, layout, current) {
                    selection = Some(layout);
                }
            }
            selection
        })
        .and_then(|popup| popup.inner);
    selector
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Choose a layout");
    selection.filter(|layout| *layout != current)
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
    let activated = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    if activated {
        ui.close();
    }
    activated
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
    }
}

#[cfg(test)]
mod tests {
    use super::Layout;

    #[test]
    fn layout_preference_survives_replacement_and_defaults_when_missing_or_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let setting = directory.path().join("config/layout");
        assert_eq!(Layout::load_at(&setting), Layout::StudioDesk);
        for layout in Layout::ALL {
            layout.save_at(&setting).unwrap();
            assert_eq!(Layout::load_at(&setting), layout);
        }
        std::fs::write(&setting, "unknown layout").unwrap();
        assert_eq!(Layout::load_at(&setting), Layout::StudioDesk);
    }
}
