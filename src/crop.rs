//! Interactive selection of a crop in the current image orientation.
use crate::{
    theme,
    viewer::panel_ui::{Glyph, dialog_button, dialog_title},
};
use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Stroke, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CropAction {
    Apply {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Cancel,
}

#[derive(Default)]
pub(crate) struct CropSelection {
    selection: Option<Rect>,
    drag: Option<Drag>,
}

#[derive(Clone, Copy)]
enum Drag {
    Draw { anchor: Pos2 },
    Move { origin: Pos2, rect: Rect },
}

impl CropSelection {
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        texture: egui::TextureId,
        size: [u32; 2],
    ) -> Option<CropAction> {
        if size.contains(&0) {
            return Some(CropAction::Cancel);
        }
        let available = ctx.content_rect().size();
        let preview = vec2(
            (available.x - 100.).clamp(260., 840.),
            (available.y - 170.).clamp(100., 550.),
        );
        let modal = egui::Modal::new(egui::Id::new("crop-image"))
            .frame(theme::floating(ctx).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(preview.x);
                dialog_title(
                    ui,
                    Glyph::Crop,
                    "Crop Image",
                    Some("Drag to select an area."),
                );
                ui.add_space(9.);
                self.preview(ui, texture, size, preview);
                ui.add_space(10.);

                let bounds = self.selection.and_then(|rect| pixel_bounds(rect, size));
                let mut action = None;
                ui.horizontal(|ui| {
                    if let Some(CropAction::Apply { width, height, .. }) = bounds {
                        ui.label(format!("{width} × {height} px"));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(bounds.is_some(), |ui| {
                                dialog_button(ui, Glyph::Check, "Apply Crop", true)
                            })
                            .inner
                            .clicked()
                        {
                            action = bounds;
                        }
                        if dialog_button(ui, Glyph::Close, "Cancel", false).clicked() {
                            action = Some(CropAction::Cancel);
                        }
                        if ui
                            .add_enabled_ui(self.selection.is_some(), |ui| {
                                dialog_button(ui, Glyph::Refresh, "Reset", false)
                            })
                            .inner
                            .clicked()
                        {
                            *self = Self::default();
                        }
                    });
                });
                action
            });
        modal
            .inner
            .or_else(|| modal.should_close().then_some(CropAction::Cancel))
    }

    fn preview(&mut self, ui: &mut egui::Ui, texture: egui::TextureId, size: [u32; 2], view: Vec2) {
        let (stage, response) = ui.allocate_exact_size(view, egui::Sense::hover());
        let image = vec2(size[0] as f32, size[1] as f32);
        let fitted = image * ((stage.size() - Vec2::splat(16.)) / image).min_elem();
        let image = Rect::from_center_size(stage.center(), fitted);
        let interaction = ui.interact(
            image,
            response.id.with("image"),
            egui::Sense::click_and_drag(),
        );
        interaction.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Image, ui.is_enabled(), "Crop preview")
        });
        let (origin, pointer, down) = ui.input(|input| {
            (
                input.pointer.press_origin(),
                input.pointer.interact_pos(),
                input.pointer.primary_down(),
            )
        });
        if interaction.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = origin
        {
            let normalized = to_image(image, origin);
            let corner = self.selection.and_then(|rect| {
                corner_at(rect, image, origin).map(|index| corners(rect)[(index + 2) % 4])
            });
            self.drag = Some(if let Some(anchor) = corner {
                Drag::Draw { anchor }
            } else if let Some(rect) = self.selection.filter(|rect| rect.contains(normalized)) {
                Drag::Move {
                    origin: normalized,
                    rect,
                }
            } else {
                Drag::Draw { anchor: normalized }
            });
        }
        if let Some(drag) = self.drag {
            if let Some(pos) = pointer {
                let pos = to_image(image, pos);
                self.selection = Some(match drag {
                    Drag::Draw { anchor } => Rect::from_two_pos(anchor, pos),
                    Drag::Move { origin, rect } => move_inside(rect, pos - origin),
                });
            }
            if !down {
                self.drag = None;
            }
        }

        if let Some(pos) = interaction.hover_pos() {
            let cursor = match self.selection {
                Some(rect) => match corner_at(rect, image, pos) {
                    Some(0 | 2) => CursorIcon::ResizeNwSe,
                    Some(_) => CursorIcon::ResizeNeSw,
                    None if rect.contains(to_image(image, pos)) => CursorIcon::Move,
                    None => CursorIcon::Crosshair,
                },
                None => CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        let painter = ui.painter();
        painter.rect_filled(stage, 8, Color32::from_rgb(29, 36, 40));
        painter.image(
            texture,
            image,
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            Color32::WHITE,
        );
        if let Some(selection) = self.selection {
            draw_selection(painter, image, from_image(image, selection));
        }
    }
}

fn corners(rect: Rect) -> [Pos2; 4] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
}

fn corner_at(rect: Rect, image: Rect, pos: Pos2) -> Option<usize> {
    corners(from_image(image, rect))
        .into_iter()
        .enumerate()
        .map(|(index, corner)| (index, pos.distance_sq(corner)))
        .filter(|(_, distance)| *distance <= 100.)
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
}

fn to_image(image: Rect, pos: Pos2) -> Pos2 {
    let span = (pos.clamp(image.min, image.max) - image.min) / image.size();
    pos2(span.x, span.y)
}

fn from_image(image: Rect, rect: Rect) -> Rect {
    Rect::from_min_max(
        image.min + rect.min.to_vec2() * image.size(),
        image.min + rect.max.to_vec2() * image.size(),
    )
}

fn move_inside(rect: Rect, delta: Vec2) -> Rect {
    let min = -rect.min.to_vec2();
    let max = Vec2::splat(1.) - rect.max.to_vec2();
    rect.translate(delta.clamp(min, max))
}

fn pixel_bounds(rect: Rect, [width, height]: [u32; 2]) -> Option<CropAction> {
    if width == 0 || height == 0 || rect.width() <= 0. || rect.height() <= 0. {
        return None;
    }
    let edge = |fraction: f32, span: u32| (f64::from(fraction) * f64::from(span)).round() as u32;
    let x = edge(rect.min.x, width).min(width - 1);
    let y = edge(rect.min.y, height).min(height - 1);
    let right = edge(rect.max.x, width).clamp(x + 1, width);
    let bottom = edge(rect.max.y, height).clamp(y + 1, height);
    Some(CropAction::Apply {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn draw_selection(painter: &egui::Painter, image: Rect, rect: Rect) {
    let painter = painter.with_clip_rect(image);
    let shade = Color32::from_black_alpha(125);
    for edge in [
        Rect::from_min_max(image.min, pos2(image.right(), rect.top())),
        Rect::from_min_max(pos2(image.left(), rect.bottom()), image.max),
        Rect::from_min_max(pos2(image.left(), rect.top()), rect.left_bottom()),
        Rect::from_min_max(rect.right_top(), pos2(image.right(), rect.bottom())),
    ] {
        painter.rect_filled(edge, 0, shade);
    }
    let guide = Stroke::new(0.75_f32, Color32::from_white_alpha(145));
    for fraction in [1. / 3., 2. / 3.] {
        let x = rect.left() + fraction * rect.width();
        let y = rect.top() + fraction * rect.height();
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], guide);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], guide);
    }
    painter.rect_stroke(
        rect,
        0,
        Stroke::new(3_f32, Color32::from_black_alpha(190)),
        egui::StrokeKind::Middle,
    );
    painter.rect_stroke(
        rect,
        0,
        Stroke::new(1.25_f32, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    for corner in corners(rect) {
        painter.rect(
            Rect::from_center_size(corner, Vec2::splat(8.)),
            2,
            Color32::WHITE,
            Stroke::new(1_f32, Color32::from_black_alpha(155)),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_bounded_in_full_size_pixels_when_preview_moves() {
        let rect = Rect::from_min_max(pos2(0.25, 0.25), pos2(0.75, 0.75));
        let rect = move_inside(rect, vec2(2., -2.));
        assert_eq!(
            pixel_bounds(rect, [3200, 2400]),
            Some(CropAction::Apply {
                x: 1600,
                y: 0,
                width: 1600,
                height: 1200
            })
        );
        assert_eq!(
            pixel_bounds(rect, [1, 1]),
            Some(CropAction::Apply {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            })
        );
        assert_eq!(
            pixel_bounds(Rect::from_min_max(Pos2::ZERO, Pos2::ZERO), [1, 1]),
            None
        );
    }

    #[test]
    fn dragging_past_preview_crops_to_image_edges() {
        let ctx = egui::Context::default();
        let mut selection = CropSelection::default();
        let texture = egui::TextureId::Managed(9);
        let size = [960, 640];
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700., 500.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        selection.preview(ui, texture, size, vec2(430., 300.));
                    });
                },
            )
        };
        let image = frame(Vec::new())
            .shapes
            .into_iter()
            .find_map(|shape| match shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
                _ => None,
            })
            .unwrap();
        let start = image.min + image.size() * 0.25;
        let end = image.max + vec2(28., 28.);
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(start)]);
        frame(vec![click(start, true)]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![click(end, false)]);
        assert_eq!(
            selection
                .selection
                .and_then(|rect| pixel_bounds(rect, size)),
            Some(CropAction::Apply {
                x: 240,
                y: 160,
                width: 720,
                height: 480,
            })
        );
        assert!(selection.drag.is_none());
    }
}
