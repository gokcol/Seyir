//! Compact pictograms and accessible controls for Seyir’s toolbar.
use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    OpenImage,
    OpenFolder,
    FolderPane,
    Previous,
    Next,
    ZoomOut,
    Fit,
    Actual,
    ZoomIn,
    Rotate,
    RotateOptions,
    Copy,
    Edit,
    Info,
    Fullscreen,
}

pub(crate) struct Tile<'a> {
    icon: Icon,
    label: &'a str,
    hint: Option<&'a str>,
    enabled: bool,
    selected: Option<bool>,
    caption: bool,
    dropdown: bool,
}

impl<'a> Tile<'a> {
    pub(crate) fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            hint: None,
            enabled: true,
            selected: None,
            caption: false,
            dropdown: false,
        }
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    pub(crate) fn caption(mut self, visible: bool) -> Self {
        self.caption = visible;
        self
    }

    pub(crate) fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    pub(crate) fn dropdown(mut self) -> Self {
        self.dropdown = true;
        self
    }

    pub(crate) fn show(self, ui: &mut Ui) -> Response {
        let hint = self.hint.unwrap_or(self.label);
        let response = self.button(ui);
        if response.enabled() {
            response.on_hover_text(hint)
        } else {
            response.on_disabled_hover_text(hint)
        }
    }

    pub(crate) fn button(self, ui: &mut Ui) -> Response {
        let galley = self.caption.then(|| {
            ui.painter().layout_no_wrap(
                self.label.into(),
                FontId::proportional(12.5),
                self.icon.ink(ui),
            )
        });
        let caption_width = galley.as_ref().map_or(0., |label| label.size().x + 6.);
        let dropdown_width = if self.dropdown { 10. } else { 0. };
        let icon_width = if matches!(self.icon, Icon::RotateOptions) {
            20.
        } else {
            33.
        };
        let size = vec2(icon_width + caption_width + dropdown_width, 33.);
        let response = ui
            .add_enabled_ui(self.enabled, |ui| {
                ui.allocate_exact_size(size, Sense::click())
            })
            .inner
            .1;
        let clickable = response.enabled();
        response.widget_info(|| match self.selected {
            Some(selected) => {
                WidgetInfo::selected(WidgetType::Checkbox, clickable, selected, self.label)
            }
            None => WidgetInfo::labeled(WidgetType::Button, clickable, self.label),
        });
        if ui.is_rect_visible(response.rect) {
            let hovered = clickable && response.hovered();
            let pressed = response.is_pointer_button_down_on();
            let selected = self.selected == Some(true);
            let dark = ui.visuals().dark_mode;
            let surface = ui.visuals().panel_fill;
            let color = self.icon.ink(ui);
            let intensity = if selected {
                0.26
            } else if hovered || response.has_focus() {
                0.19
            } else if clickable {
                0.10
            } else {
                0.015
            };
            let shadow = if dark {
                Color32::from_black_alpha(57)
            } else {
                Color32::from_rgba_unmultiplied(23, 42, 72, 20)
            };
            let face = response.rect.shrink2(vec2(0., 1.));
            let face = if pressed {
                face.translate(vec2(0., 1.))
            } else {
                face
            };
            let painter = ui.painter();
            if clickable && !pressed {
                painter.rect_filled(face.translate(vec2(0., 1.5)), 9, shadow);
            }
            let edge = if response.has_focus() {
                Stroke::new(1.5_f32, color)
            } else {
                Stroke::new(1_f32, surface.lerp_to_gamma(color, intensity + 0.08))
            };
            painter.rect(
                face,
                9,
                surface.lerp_to_gamma(color, intensity),
                edge,
                StrokeKind::Inside,
            );
            if clickable && !pressed {
                let shine = if dark { 27 } else { 115 };
                painter.line_segment(
                    [
                        pos2(face.min.x + 8., face.min.y + 2.),
                        pos2(face.max.x - 8., face.min.y + 2.),
                    ],
                    Stroke::new(1_f32, Color32::from_white_alpha(shine)),
                );
            }
            let ink = if clickable {
                color
            } else {
                surface.lerp_to_gamma(ui.visuals().text_color(), 0.38)
            };
            let center = pos2(face.left() + icon_width * 0.5, face.center().y);
            self.icon.paint(painter, center, ink);
            if let Some(label) = galley {
                let at = pos2(face.left() + 29., face.center().y - label.size().y / 2.);
                painter.galley_with_override_text_color(at, label, ink);
            }
            if self.dropdown {
                let x = face.max.x - 10.;
                let y = face.center().y;
                painter.add(Shape::line(
                    vec![
                        pos2(x - 2.8, y - 1.2),
                        pos2(x, y + 1.5),
                        pos2(x + 2.8, y - 1.2),
                    ],
                    Stroke::new(1.3_f32, ink),
                ));
            }
            if selected {
                painter.circle_filled(pos2(face.max.x - 5.5, face.min.y + 5.5), 1.75, ink);
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

pub(crate) fn group(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let theme = ui.visuals();
    let edge = theme.panel_fill.lerp_to_gamma(theme.text_color(), 0.12);
    let color = theme.panel_fill.lerp_to_gamma(theme.extreme_bg_color, 0.34);
    egui::Frame::new()
        .corner_radius(11)
        .fill(color)
        .stroke(Stroke::new(1_f32, edge))
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.;
                add(ui);
            });
        });
}

pub(crate) fn brand(ui: &mut Ui, texture: &egui::TextureHandle) {
    ui.add(egui::Image::new((texture.id(), Vec2::splat(44.))).alt_text("Seyir"));
}

impl Icon {
    fn ink(self, ui: &Ui) -> Color32 {
        let (light, dark) = match self {
            Self::OpenFolder | Self::FolderPane | Self::Edit => ([130, 77, 14], [255, 193, 106]),
            Self::ZoomOut | Self::ZoomIn | Self::Fit | Self::Actual | Self::Fullscreen => {
                ([10, 108, 106], [116, 229, 215])
            }
            Self::Previous | Self::Next => ([58, 78, 123], [170, 190, 235]),
            Self::Rotate | Self::RotateOptions => ([107, 61, 166], [203, 167, 255]),
            Self::OpenImage | Self::Copy | Self::Info => ([44, 84, 166], [149, 188, 255]),
        };
        let [r, g, b] = if ui.visuals().dark_mode { dark } else { light };
        Color32::from_rgb(r, g, b)
    }

    fn paint(self, painter: &Painter, center: Pos2, ink: Color32) {
        let point = |x, y| center + vec2(x - 12., y - 12.) * 0.9;
        let stroke = Stroke::new(1.65_f32, ink);
        let line = |points: &[(f32, f32)]| {
            painter.add(Shape::line(
                points.iter().map(|&(x, y)| point(x, y)).collect(),
                stroke,
            ));
        };
        let outline = |x1, y1, x2, y2| {
            painter.rect_stroke(
                Rect::from_min_max(point(x1, y1), point(x2, y2)),
                2,
                stroke,
                StrokeKind::Middle,
            );
        };
        let dot = |x, y, radius| {
            painter.circle_filled(point(x, y), radius, ink);
        };
        let circle = |x, y, radius| {
            painter.circle_stroke(point(x, y), radius, stroke);
        };
        let corners = |outer: f32, inner: f32| {
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                line(&[
                    (12. + x * outer, 12. + y * inner),
                    (12. + x * outer, 12. + y * outer),
                    (12. + x * inner, 12. + y * outer),
                ]);
            }
        };
        match self {
            Self::OpenImage => {
                outline(3.5, 4.5, 20.5, 19.5);
                dot(8.5, 9., 1.5);
                line(&[
                    (4., 16.5),
                    (8.5, 12.8),
                    (12., 15.5),
                    (16.3, 11.5),
                    (20., 15.),
                ]);
            }
            Self::OpenFolder => {
                line(&[
                    (3.5, 19.),
                    (3.5, 5.5),
                    (9.3, 5.5),
                    (12., 8.),
                    (19.5, 8.),
                    (19.5, 11.),
                ]);
                line(&[(3.5, 19.), (6.5, 11.), (22., 11.), (19., 19.), (3.5, 19.)]);
            }
            Self::FolderPane => {
                outline(3.5, 5., 20.5, 19.);
                line(&[(9.2, 5.), (9.2, 19.)]);
                line(&[(12.4, 9.2), (17.6, 9.2)]);
                line(&[(12.4, 14.8), (17.6, 14.8)]);
            }
            Self::Previous => line(&[(14.3, 5.8), (8.1, 12.), (14.3, 18.2)]),
            Self::Next => line(&[(9.7, 5.8), (15.9, 12.), (9.7, 18.2)]),
            Self::ZoomOut | Self::ZoomIn => {
                circle(10.2, 10.2, 5.3);
                line(&[(14.6, 14.6), (20.1, 20.1)]);
                line(&[(7.4, 10.2), (13., 10.2)]);
                if matches!(self, Self::ZoomIn) {
                    line(&[(10.2, 7.4), (10.2, 13.)]);
                }
            }
            Self::Fit => {
                corners(9., 5.);
                outline(8., 8.6, 16., 15.4);
            }
            Self::Actual => {
                outline(2.5, 5., 21.5, 19.);
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "1:1",
                    FontId::monospace(8.1),
                    ink,
                );
            }
            Self::Rotate => {
                let arc = (0..=16)
                    .map(|step| {
                        let angle = (40. + 265. * step as f32 / 16.).to_radians();
                        (12. + 7.2 * angle.cos(), 12. + 7.2 * angle.sin())
                    })
                    .collect::<Vec<_>>();
                line(&arc);
                let tip = arc[16];
                line(&[(12.1, 6.2), tip, (16.7, 2.8)]);
                dot(12., 12., 1.6);
            }
            Self::RotateOptions => line(&[(8.5, 10.5), (12., 14.), (15.5, 10.5)]),
            Self::Copy => {
                line(&[
                    (7.5, 16.5),
                    (3.5, 16.5),
                    (3.5, 3.5),
                    (15.5, 3.5),
                    (15.5, 7.5),
                ]);
                outline(8.1, 8.1, 20.5, 20.5);
            }
            Self::Edit => {
                line(&[
                    (13., 4.5),
                    (3.5, 4.5),
                    (3.5, 20.5),
                    (19.5, 20.5),
                    (19.5, 13.),
                ]);
                line(&[
                    (8.3, 15.7),
                    (9.4, 11.4),
                    (17.8, 3.),
                    (21., 6.2),
                    (12.6, 14.6),
                    (8.3, 15.7),
                ]);
                line(&[(15.8, 5.), (19., 8.2)]);
            }
            Self::Info => {
                circle(12., 12., 8.1);
                line(&[(12., 11.5), (12., 16.3)]);
                dot(12., 8., 1.15);
            }
            Self::Fullscreen => corners(8.5, 3.4),
        }
    }
}
