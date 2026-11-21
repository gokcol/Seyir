//! Virtualized thumbnail layouts backed by the viewer's shared thumbnail cache.
use crate::{
    viewer::panel_ui::{self, Glyph},
    work::Entry,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
};

pub(crate) const FILMSTRIP_HEIGHT: f32 = 68.0;
const FILMSTRIP_CARD_WIDTH: f32 = 154.0;
const FILMSTRIP_CARD_HEIGHT: f32 = 62.0;
const GRID_MIN_WIDTH: f32 = 118.0;
const GAP: f32 = 8.0;
const THUMBNAIL_BATCH: usize = 6;

type Images = HashMap<PathBuf, Option<egui::TextureHandle>>;

#[derive(Default)]
pub(crate) struct Outcome {
    pub selected: Option<(PathBuf, bool)>,
    pub missing: Vec<PathBuf>,
}

pub(crate) fn filmstrip(
    ui: &mut egui::Ui,
    entries: &[Entry],
    file: Option<&Path>,
    images: &Images,
    focus: bool,
) -> Outcome {
    let stride = FILMSTRIP_CARD_WIDTH + GAP;
    let strip = egui::ScrollArea::horizontal()
        .id_salt("filmstrip-thumbnails")
        .max_height(FILMSTRIP_HEIGHT)
        .auto_shrink([false, true]);
    let strip = if focus {
        let offset = selected_offset(entries, file, 1, stride, ui.available_width());
        strip.horizontal_scroll_offset(offset)
    } else {
        strip
    };
    let mut outcome = Outcome::default();
    strip.show_viewport(ui, |ui, viewport| {
        let content_width = ((entries.len() as f32 * stride) - GAP).max(0.0);
        ui.set_min_size(egui::vec2(content_width, FILMSTRIP_HEIGHT));
        let origin = ui.min_rect().left_top() + egui::vec2(0.0, 2.0);
        for index in visible(viewport.min.x, viewport.max.x, stride, entries.len()) {
            let rect = Rect::from_min_size(
                origin + egui::vec2(index as f32 * stride, 0.0),
                egui::vec2(FILMSTRIP_CARD_WIDTH, FILMSTRIP_CARD_HEIGHT),
            );
            let preview = Rect::from_center_size(
                rect.left_center() + egui::vec2(37.0, 0.0),
                egui::vec2(60.0, 46.0),
            );
            let name = Rect::from_min_max(
                rect.left_top() + egui::vec2(75.0, 0.0),
                rect.right_bottom() - egui::vec2(8.0, 0.0),
            );
            outcome.card(
                ui,
                &entries[index],
                file,
                images,
                Card {
                    rect,
                    preview,
                    name,
                },
            );
        }
    });
    outcome
}

pub(crate) fn contact_sheet(
    ui: &mut egui::Ui,
    entries: &[Entry],
    file: Option<&Path>,
    images: &Images,
    focus: bool,
) -> Outcome {
    let width = (ui.available_width() - ui.spacing().scroll.allocated_width()).max(1.0);
    let columns = (((width + GAP) / (GRID_MIN_WIDTH + GAP)).floor() as usize).max(1);
    let card_width = ((width + GAP) / columns as f32 - GAP).max(1.0);
    let preview_height = (card_width * 0.64).clamp(45.0, 112.0);
    let card_height = preview_height + 29.0;
    let row_stride = card_height + GAP;
    let row_count = entries.len().div_ceil(columns);
    let scroll = egui::ScrollArea::vertical()
        .id_salt("contact-sheet-thumbnails")
        .auto_shrink([false, false]);
    let scroll = if focus {
        let offset = selected_offset(entries, file, columns, row_stride, ui.available_height());
        scroll.vertical_scroll_offset(offset)
    } else {
        scroll
    };
    let mut outcome = Outcome::default();
    scroll.show_viewport(ui, |ui, viewport| {
        ui.set_min_height((row_count as f32 * row_stride - GAP).max(0.0));
        let origin = ui.min_rect().left_top();
        for row in visible(viewport.min.y, viewport.max.y, row_stride, row_count) {
            let row_start = row * columns;
            let row_end = (row_start + columns).min(entries.len());
            for (column, entry) in entries[row_start..row_end].iter().enumerate() {
                let rect = Rect::from_min_size(
                    origin
                        + egui::vec2(column as f32 * (card_width + GAP), row as f32 * row_stride),
                    egui::vec2(card_width, card_height),
                );
                // Keep the inset valid even while the parent pane is being collapsed.
                let inset = (card_width * 0.12).min(6.0);
                let preview = Rect::from_min_size(
                    rect.min + egui::vec2(inset, 6.0),
                    egui::vec2(card_width - 2.0 * inset, preview_height),
                );
                let name = Rect::from_min_max(
                    egui::pos2(preview.left(), preview.bottom() + 1.0),
                    egui::pos2(preview.right(), rect.bottom() - 3.0),
                );
                outcome.card(
                    ui,
                    entry,
                    file,
                    images,
                    Card {
                        rect,
                        preview,
                        name,
                    },
                );
            }
        }
    });
    outcome
}

fn selected_offset(
    entries: &[Entry],
    file: Option<&Path>,
    columns: usize,
    stride: f32,
    view: f32,
) -> f32 {
    let index = file.and_then(|file| entries.iter().position(|entry| entry.path == file));
    index.map_or(0.0, |index| {
        let center = (index / columns) as f32 * stride + stride * 0.5;
        (center - view * 0.5).max(0.0)
    })
}

fn visible(min: f32, max: f32, stride: f32, count: usize) -> Range<usize> {
    let start = ((min / stride).floor() as usize).min(count.saturating_sub(1));
    let end = ((max / stride).ceil() as usize)
        .saturating_add(1)
        .min(count);
    start..end
}

struct Card {
    rect: Rect,
    preview: Rect,
    name: Rect,
}

impl Outcome {
    fn card(
        &mut self,
        ui: &egui::Ui,
        entry: &Entry,
        file: Option<&Path>,
        images: &Images,
        card: Card,
    ) {
        let Card {
            rect,
            preview,
            name,
        } = card;
        if !ui.is_rect_visible(rect) {
            return;
        }
        let selected = Some(entry.path.as_path()) == file;
        let response = ui.interact(rect, ui.make_persistent_id(&entry.path), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                &entry.name,
            )
        });
        let visuals = ui.visuals();
        let line = if selected || response.has_focus() {
            visuals.selection.stroke
        } else {
            visuals.window_stroke
        };
        let fill = if selected || response.hovered() {
            visuals.selection.bg_fill
        } else {
            visuals.faint_bg_color
        };
        let painter = ui.painter();
        painter.rect(rect, 9, fill, line, StrokeKind::Inside);
        let preview_fill = visuals
            .panel_fill
            .lerp_to_gamma(visuals.text_color(), 0.075);
        painter.rect_filled(preview, 6, preview_fill);

        if !entry.directory
            && !images.contains_key(&entry.path)
            && self.missing.len() < THUMBNAIL_BATCH
        {
            self.missing.push(entry.path.clone());
        }
        if !entry.directory
            && let Some(Some(texture)) = images.get(&entry.path)
        {
            let area = preview.shrink2(egui::vec2(3.0_f32.min(preview.width() * 0.25), 3.0));
            let pixels = texture.size_vec2();
            let scale = (area.size() / pixels).min_elem();
            painter.image(
                texture.id(),
                Rect::from_center_size(area.center(), pixels * scale),
                Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)),
                Color32::WHITE,
            );
        } else if preview.width() >= 18.0 {
            panel_ui::glyph(
                ui,
                Rect::from_center_size(
                    preview.center(),
                    Vec2::splat(25.0_f32.min(preview.size().min_elem())),
                ),
                if entry.directory {
                    Glyph::Folder
                } else {
                    Glyph::Image
                },
                if entry.directory {
                    visuals.selection.stroke.color
                } else {
                    visuals.weak_text_color()
                },
            );
        }
        if selected && preview.width() >= 24.0 {
            let mark = preview.right_top() + egui::vec2(-9.0, 9.0);
            painter.circle(
                mark,
                7.0,
                visuals.faint_bg_color,
                Stroke::new(1.0_f32, line.color),
            );
            panel_ui::glyph(
                ui,
                Rect::from_center_size(mark, Vec2::splat(13.0)),
                Glyph::Check,
                line.color,
            );
        }
        panel_ui::clipped_text(ui, name, &entry.name, 11.5, visuals.text_color());
        if response.clicked() {
            self.selected = Some((entry.path.clone(), entry.directory));
        }
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(&entry.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galleries_scroll_to_selection_and_only_request_visible_thumbnails() {
        let folder = tempfile::tempdir().unwrap();
        for index in 0..120 {
            std::fs::File::create(folder.path().join(format!("image-{index:03}.png"))).unwrap();
        }
        let entries = crate::work::folder(folder.path()).unwrap();
        let selected = entries[84].path.as_path();
        let cache = Images::new();

        for render in [filmstrip, contact_sheet] {
            let context = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(330.0, 235.0))),
                ..egui::RawInput::default()
            };
            let _ = context.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let result = render(ui, &entries, Some(selected), &cache, true);
                    assert!(result.missing.iter().any(|path| path == selected));
                    assert!(result.missing.len() <= THUMBNAIL_BATCH);
                    assert!(!result.missing.contains(&entries[0].path));
                });
            });
        }
    }
}
