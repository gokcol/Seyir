//! Pan, zoom and rotate the viewport without modifying the source pixels.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use image::{Rgba, RgbaImage};

#[derive(Default)]
pub struct Canvas {
    pub zoom: Option<f32>,
    pub angle: f32,
    offset: Vec2,
    display_zoom: f32,
}

impl Canvas {
    pub fn fit(&mut self) {
        self.zoom = None;
        self.offset = Vec2::ZERO;
    }

    pub fn zoom_ratio(&self) -> f32 {
        self.zoom.unwrap_or(self.display_zoom.max(f32::EPSILON))
    }

    pub fn set_zoom(&mut self, value: f32) {
        if value.is_finite() {
            self.zoom = Some(value.clamp(0.1, 10.0));
        }
    }

    pub fn step_zoom(&mut self, factor: f32) {
        self.set_zoom(self.zoom_ratio() * factor);
    }

    pub fn rotate_by(&mut self, angle: f32) {
        self.angle = normalize_angle(self.angle + angle);
    }

    pub fn paint(&mut self, ui: &mut egui::Ui, texture: egui::TextureId, size: [u32; 2]) {
        let (view, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            egui::Sense::drag(),
        );
        let painter = ui.painter().with_clip_rect(view);
        painter.rect_filled(view, 0, crate::theme::CANVAS);
        let image = egui::vec2(size[0] as f32, size[1] as f32);
        let ratio = ui.ctx().pixels_per_point();
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let rotated_size = egui::vec2(
            image.x * cos.abs() + image.y * sin.abs(),
            image.x * sin.abs() + image.y * cos.abs(),
        );

        self.display_zoom = ((view.size() - Vec2::splat(24.)).max(Vec2::splat(1.)) / rotated_size)
            .min_elem()
            .min(1.0 / ratio)
            * ratio;
        if response.hovered() {
            let (scroll, pinch, pointer) = ui.input(|input| {
                (
                    input.raw_scroll_delta.y,
                    input.zoom_delta(),
                    input.pointer.hover_pos(),
                )
            });
            let factor = if (pinch - 1.).abs() > f32::EPSILON {
                pinch
            } else {
                (scroll * 0.003).exp()
            };
            if factor != 1.0 {
                let before = self.zoom_ratio();
                self.set_zoom(before * factor);
                if let Some(pointer) = pointer {
                    let anchor = pointer - view.center();
                    self.offset = anchor + (self.offset - anchor) * (self.zoom_ratio() / before);
                }
            }
        }
        self.offset += response.drag_delta();
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let half = image * (self.zoom_ratio() / ratio / 2.0);
        let center = view.center() + self.offset;
        let rotate =
            |at: Vec2| center + egui::vec2(at.x * cos - at.y * sin, at.x * sin + at.y * cos);
        let mut mesh = egui::Mesh::with_texture(texture);
        for (corner, uv) in [
            (egui::vec2(-half.x, -half.y), egui::pos2(0., 0.)),
            (egui::vec2(half.x, -half.y), egui::pos2(1., 0.)),
            (egui::vec2(half.x, half.y), egui::pos2(1., 1.)),
            (egui::vec2(-half.x, half.y), egui::pos2(0., 1.)),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rotate(corner),
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        let bounds = Rect::from_points(&mesh.vertices.iter().map(|v| v.pos).collect::<Vec<Pos2>>());
        if view.intersects(bounds) {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

fn normalize_angle(value: f32) -> f32 {
    (value + 180.).rem_euclid(360.) - 180.
}

/// Full-resolution pixels for transfer. Quarter turns are exact. Interpolated
/// angles use premultiplied alpha so transparent edges stay clean.
pub fn oriented_pixels(source: &RgbaImage, angle: f32) -> Result<RgbaImage, String> {
    let angle = normalize_angle(angle);
    match angle {
        0. => Ok(source.clone()),
        90. => Ok(image::imageops::rotate90(source)),
        -90. => Ok(image::imageops::rotate270(source)),
        -180. => Ok(image::imageops::rotate180(source)),
        _ => {
            let (sin, cos) = f64::from(angle).to_radians().sin_cos();
            let (width, height) = (f64::from(source.width()), f64::from(source.height()));
            let output_width = (width * cos.abs() + height * sin.abs()).ceil() as u32;
            let output_height = (width * sin.abs() + height * cos.abs()).ceil() as u32;
            if u64::from(output_width) * u64::from(output_height) > 100_000_000 {
                return Err(
                    "The rotated result is too large to copy. Reset rotation and try again.".into(),
                );
            }
            Ok(RgbaImage::from_fn(output_width, output_height, |x, y| {
                let x = f64::from(x) + 0.5 - f64::from(output_width) / 2.;
                let y = f64::from(y) + 0.5 - f64::from(output_height) / 2.;
                let source_x = x * cos + y * sin + width / 2. - 0.5;
                let source_y = -x * sin + y * cos + height / 2. - 0.5;
                sample(source, source_x, source_y)
            }))
        }
    }
}

fn sample(image: &RgbaImage, x: f64, y: f64) -> Rgba<u8> {
    let (left, top) = (x.floor(), y.floor());
    let mut channels = [0.; 4];
    for (row, weight_y) in [(top, 1. - (y - top)), (top + 1., y - top)] {
        for (column, weight_x) in [(left, 1. - (x - left)), (left + 1., x - left)] {
            if row < 0.
                || column < 0.
                || row >= f64::from(image.height())
                || column >= f64::from(image.width())
            {
                continue;
            }
            let pixel = image.get_pixel(column as u32, row as u32);
            let weight = weight_x * weight_y * f64::from(pixel[3]);
            for index in 0..3 {
                channels[index] += f64::from(pixel[index]) * weight;
            }
            channels[3] += weight;
        }
    }
    let alpha = channels[3];
    if alpha <= f64::EPSILON {
        return Rgba([0; 4]);
    }
    Rgba([
        (channels[0] / alpha).round() as u8,
        (channels[1] / alpha).round() as u8,
        (channels[2] / alpha).round() as u8,
        alpha.round() as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_never_changes_source_and_quarter_turn_is_exact() {
        let image = RgbaImage::from_fn(3, 2, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let quarter = oriented_pixels(&image, 90.).unwrap();
        assert_eq!(quarter.dimensions(), (2, 3));
        assert_eq!(quarter.get_pixel(1, 0), image.get_pixel(0, 0));
        let tilted = oriented_pixels(&image, 45.).unwrap();
        assert_eq!(tilted.dimensions(), (4, 4));
        assert!(tilted.pixels().any(|pixel| pixel[3] == 0));
        assert_eq!(image.dimensions(), (3, 2));
    }
}
