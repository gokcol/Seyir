//! Pan, zoom and rotate the viewport without modifying the source pixels.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use image::{Rgba, RgbaImage};

#[derive(Clone, Default)]
pub struct Canvas {
    pub zoom: Option<f32>,
    pub angle: f32,
    offset: Vec2,
    display_zoom: f32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TransitionStyle {
    #[default]
    Instant,
    Fade,
    Slide,
    Zoom,
    Wipe,
    Push,
}

impl TransitionStyle {
    pub(crate) const ALL: [Self; 6] = [
        Self::Instant,
        Self::Fade,
        Self::Slide,
        Self::Zoom,
        Self::Wipe,
        Self::Push,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Instant => "Instant",
            Self::Fade => "Fade",
            Self::Slide => "Slide",
            Self::Zoom => "Zoom",
            Self::Wipe => "Wipe",
            Self::Push => "Push",
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Fade => "fade",
            Self::Slide => "slide",
            Self::Zoom => "zoom",
            Self::Wipe => "wipe",
            Self::Push => "push",
        }
    }
}

pub(crate) struct Transition<'a> {
    pub style: TransitionStyle,
    pub previous: &'a Canvas,
    pub texture: egui::TextureId,
    pub size: [u32; 2],
    pub progress: f32,
    pub forward: bool,
}

#[derive(Clone, Copy)]
struct Layer {
    clip: Rect,
    offset: Vec2,
    scale: f32,
    opacity: f32,
}

impl Layer {
    fn new(view: Rect) -> Self {
        Self {
            clip: view,
            offset: Vec2::ZERO,
            scale: 1.,
            opacity: 1.,
        }
    }
}

impl Transition<'_> {
    fn layers(&self, view: Rect) -> (Layer, Layer) {
        let mut outgoing = Layer::new(view);
        let mut incoming = Layer::new(view);
        let time = self.progress.clamp(0., 1.);
        let progress = time * time * (3. - 2. * time);
        let direction = if self.forward { 1. } else { -1. };
        match self.style {
            TransitionStyle::Instant => outgoing.opacity = 0.,
            TransitionStyle::Fade | TransitionStyle::Zoom => {
                outgoing.opacity = 1. - progress;
                incoming.opacity = progress;
                if self.style == TransitionStyle::Zoom {
                    outgoing.scale = 1. + direction * 0.06 * progress;
                    incoming.scale = 1. - direction * 0.06 * (1. - progress);
                }
            }
            TransitionStyle::Slide | TransitionStyle::Wipe | TransitionStyle::Push => {
                if self.forward {
                    let edge = view.right() - view.width() * progress;
                    outgoing.clip.max.x = edge;
                    incoming.clip.min.x = edge;
                } else {
                    let edge = view.left() + view.width() * progress;
                    outgoing.clip.min.x = edge;
                    incoming.clip.max.x = edge;
                }
                if self.style != TransitionStyle::Wipe {
                    incoming.offset.x = direction * view.width() * (1. - progress);
                }
                if self.style == TransitionStyle::Push {
                    outgoing.offset.x = -direction * view.width() * progress;
                }
            }
        }
        (outgoing, incoming)
    }
}

struct Quad {
    center: Pos2,
    half: Vec2,
    sin: f32,
    cos: f32,
}

impl Quad {
    fn contains(&self, pos: Pos2) -> bool {
        let delta = pos - self.center;
        let image_x = delta.x * self.cos + delta.y * self.sin;
        let image_y = -delta.x * self.sin + delta.y * self.cos;
        image_x.abs() <= self.half.x && image_y.abs() <= self.half.y
    }

    fn draw(&self, ui: &egui::Ui, texture: egui::TextureId, layer: Layer) {
        if layer.opacity <= 0. || layer.clip.width() <= 0. {
            return;
        }
        let tint = Color32::from_white_alpha((255. * layer.opacity).round() as u8);
        let half = self.half;
        let mut mesh = egui::Mesh::with_texture(texture);
        for (corner, uv) in [
            (egui::vec2(-half.x, -half.y), egui::pos2(0., 0.)),
            (egui::vec2(half.x, -half.y), egui::pos2(1., 0.)),
            (egui::vec2(half.x, half.y), egui::pos2(1., 1.)),
            (egui::vec2(-half.x, half.y), egui::pos2(0., 1.)),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: self.center
                    + egui::vec2(
                        corner.x * self.cos - corner.y * self.sin,
                        corner.x * self.sin + corner.y * self.cos,
                    ),
                uv,
                color: tint,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        let bounds = Rect::from_points(&[
            mesh.vertices[0].pos,
            mesh.vertices[1].pos,
            mesh.vertices[2].pos,
            mesh.vertices[3].pos,
        ]);
        if layer.clip.intersects(bounds) {
            ui.painter()
                .with_clip_rect(layer.clip)
                .add(egui::Shape::mesh(mesh));
        }
    }
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

    pub(crate) fn paint_still(&self, ui: &mut egui::Ui, texture: egui::TextureId, size: [u32; 2]) {
        let (view, _) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            egui::Sense::hover(),
        );
        let image = egui::vec2(size[0] as f32, size[1] as f32);
        let layer = Layer::new(view);
        self.quad(view, image, ui.ctx().pixels_per_point(), layer)
            .draw(ui, texture, layer);
    }

    pub(crate) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        texture: egui::TextureId,
        size: [u32; 2],
        transition: Option<Transition<'_>>,
    ) -> (egui::Response, bool) {
        let (view, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            egui::Sense::click_and_drag(),
        );
        let image = egui::vec2(size[0] as f32, size[1] as f32);
        let ratio = ui.ctx().pixels_per_point();
        self.display_zoom = self.fit_zoom(view, image, ratio);
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
        if response.dragged_by(egui::PointerButton::Primary) {
            self.offset += response.drag_delta();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let layer = if let Some(transition) = transition {
            let (outgoing, incoming) = transition.layers(view);
            let [width, height] = transition.size;
            let image = egui::vec2(width as f32, height as f32);
            transition.previous.quad(view, image, ratio, outgoing).draw(
                ui,
                transition.texture,
                outgoing,
            );
            incoming
        } else {
            Layer::new(view)
        };
        let quad = self.quad(view, image, ratio, layer);
        quad.draw(ui, texture, layer);

        let image_secondary_click = response.secondary_clicked()
            && layer.opacity > 0.
            && response
                .interact_pointer_pos()
                .is_some_and(|pos| layer.clip.contains(pos) && quad.contains(pos));
        (response, image_secondary_click)
    }

    fn fit_zoom(&self, view: Rect, image: Vec2, ratio: f32) -> f32 {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let rotated_size = egui::vec2(
            image.x * cos.abs() + image.y * sin.abs(),
            image.x * sin.abs() + image.y * cos.abs(),
        );
        ((view.size() - Vec2::splat(24.)).max(Vec2::splat(1.)) / rotated_size)
            .min_elem()
            .min(1.0 / ratio)
            * ratio
    }

    fn quad(&self, view: Rect, image: Vec2, ratio: f32, layer: Layer) -> Quad {
        let zoom = self
            .zoom
            .unwrap_or_else(|| self.fit_zoom(view, image, ratio));
        let (sin, cos) = self.angle.to_radians().sin_cos();
        Quad {
            center: view.center() + self.offset + layer.offset,
            half: image * (zoom / ratio / 2.0 * layer.scale),
            sin,
            cos,
        }
    }
}

fn normalize_angle(value: f32) -> f32 {
    (value + 180.).rem_euclid(360.) - 180.
}

pub(crate) fn oriented_size((width, height): (u32, u32), angle: f32) -> (u32, u32) {
    match normalize_angle(angle) {
        0. | -180. => (width, height),
        90. | -90. => (height, width),
        angle => {
            let (sin, cos) = f64::from(angle).to_radians().sin_cos();
            let (width, height) = (f64::from(width), f64::from(height));
            (
                (width * cos.abs() + height * sin.abs()).ceil() as u32,
                (width * sin.abs() + height * cos.abs()).ceil() as u32,
            )
        }
    }
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
            let (output_width, output_height) = oriented_size(source.dimensions(), angle);
            if u64::from(output_width) * u64::from(output_height) > 100_000_000 {
                return Err(
                    "The rotated result is too large. Reset rotation and try again.".into(),
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
    fn context_click_only_hits_the_displayed_quadrilateral() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas {
            angle: 45.,
            ..Default::default()
        };
        let frame = |canvas: &mut Canvas, events| {
            let mut hit = false;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(300., 300.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        (_, hit) = canvas.paint(ui, egui::TextureId::Managed(0), [120, 80], None);
                    });
                },
            );
            hit
        };
        let secondary_click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        frame(&mut canvas, vec![]);
        for (pos, expected_hit) in [
            (egui::pos2(150., 150.), true),
            (egui::pos2(100., 100.), false),
            (egui::pos2(20., 20.), false),
        ] {
            assert!(!frame(
                &mut canvas,
                vec![egui::Event::PointerMoved(pos), secondary_click(pos, true)]
            ));
            assert_eq!(
                frame(&mut canvas, vec![secondary_click(pos, false)]),
                expected_hit,
                "wrong hit result at {pos:?}"
            );
        }
        assert_eq!(canvas.offset, Vec2::ZERO);
    }

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
