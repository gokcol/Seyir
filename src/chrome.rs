//! Compact pictograms and accessible controls for Seyir’s toolbar.
use crate::canvas::Canvas;
use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

const TILE_HEIGHT: f32 = 33.;
const GROUP_MARGIN_Y: i8 = 3;
const GROUP_STROKE: f32 = 1.;
const ROW_HEIGHT: f32 = TILE_HEIGHT + 2. * (GROUP_MARGIN_Y as f32 + GROUP_STROKE);

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
    RotateLeft,
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
    min_width: f32,
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
            min_width: 0.,
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

    fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
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
        let icon_width = 33.0_f32;
        let size = vec2(
            (icon_width + caption_width + dropdown_width).max(self.min_width),
            TILE_HEIGHT,
        );
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

pub(crate) fn row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let size = vec2(ui.available_width(), ROW_HEIGHT);
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(7., 6.);
        add(ui);
    });
}

pub(crate) fn group(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let vertical = ui.layout().is_vertical();
    let theme = ui.visuals();
    let edge = theme.panel_fill.lerp_to_gamma(theme.text_color(), 0.12);
    let color = theme.panel_fill.lerp_to_gamma(theme.extreme_bg_color, 0.34);
    egui::Frame::new()
        .corner_radius(11)
        .fill(color)
        .stroke(Stroke::new(GROUP_STROKE, edge))
        .inner_margin(egui::Margin::symmetric(4, GROUP_MARGIN_Y))
        .show(ui, |ui| {
            if vertical {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    add(ui);
                });
                return;
            }
            let size = vec2(ui.available_width(), TILE_HEIGHT);
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(size, layout, |ui| {
                ui.spacing_mut().item_spacing.x = 4.;
                add(ui);
            });
        });
}

pub(crate) fn brand(ui: &mut Ui, texture: &egui::TextureHandle) -> Response {
    ui.add(egui::Image::new((texture.id(), Vec2::splat(ROW_HEIGHT))).alt_text("Seyir"))
}

pub(crate) fn rotation(ui: &mut Ui, canvas: &mut Canvas, enabled: bool) -> Response {
    let rotate = Tile::new(Icon::Rotate, "Rotate right 90°")
        .enabled(enabled)
        .button(ui);
    if rotate.clicked() {
        canvas.rotate_by(90.);
    }
    if !enabled {
        return rotate;
    }

    const PANEL_WIDTH: f32 = 218.;
    let accent = Icon::Rotate.ink(ui);
    let frame = egui::Frame::popup(ui.style())
        .corner_radius(13)
        .inner_margin(12)
        .fill(ui.visuals().window_fill().lerp_to_gamma(accent, 0.035));
    let align = if ui.layout().is_vertical() {
        egui::RectAlign::RIGHT_START
    } else {
        egui::RectAlign::BOTTOM_START
    };
    let keyboard = rotate.has_focus()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10));
    let keyboard_popup = egui::Popup::from_response(&rotate)
        .open_memory(keyboard.then_some(egui::SetOpenCommand::Bool(true)))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(align)
        .gap(3.)
        .frame(frame);
    if keyboard_popup
        .show(|ui| rotation_panel(ui, canvas, accent, PANEL_WIDTH, keyboard))
        .is_none()
    {
        let mut tooltip = egui::Tooltip::for_enabled(&rotate).width(PANEL_WIDTH);
        tooltip.popup = tooltip.popup.frame(frame).align(align).gap(3.);
        tooltip.show(|ui| rotation_panel(ui, canvas, accent, PANEL_WIDTH, false));
    }
    rotate
}

fn rotation_panel(ui: &mut Ui, canvas: &mut Canvas, accent: Color32, width: f32, focus_left: bool) {
    ui.set_min_width(width);
    ui.spacing_mut().item_spacing = vec2(8., 7.);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Rotation").strong().size(14.));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reset = ui.add_enabled(
                canvas.angle != 0.,
                egui::Button::new("Reset rotation").frame(false),
            );
            if reset.clicked() {
                canvas.angle = 0.;
            }
        });
    });

    let button_width = (width - ui.spacing().item_spacing.x) * 0.5;
    ui.horizontal(|ui| {
        let left = Tile::new(Icon::RotateLeft, "90° left")
            .caption(true)
            .min_width(button_width)
            .button(ui);
        if focus_left {
            left.request_focus();
        }
        if left.clicked() {
            canvas.rotate_by(-90.);
        }
        if Tile::new(Icon::Rotate, "90° right")
            .caption(true)
            .min_width(button_width)
            .button(ui)
            .clicked()
        {
            canvas.rotate_by(90.);
        }
    });

    let angle_label = ui
        .horizontal(|ui| {
            let angle_label = ui.label(egui::RichText::new("Fine angle").weak()).id;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [62., 24.],
                    egui::DragValue::new(&mut canvas.angle)
                        .range(-180.0..=180.0)
                        .speed(0.2)
                        .fixed_decimals(1)
                        .suffix("°"),
                )
                .labelled_by(angle_label);
            });
            angle_label
        })
        .inner;
    ui.spacing_mut().slider_width = width;
    ui.visuals_mut().selection.bg_fill = ui.visuals().window_fill().lerp_to_gamma(accent, 0.43);
    ui.add(
        egui::Slider::new(&mut canvas.angle, -180.0..=180.0)
            .fixed_decimals(1)
            .show_value(false)
            .trailing_fill(true),
    )
    .labelled_by(angle_label);
}

impl Icon {
    fn ink(self, ui: &Ui) -> Color32 {
        let (light, dark) = match self {
            Self::OpenFolder | Self::FolderPane | Self::Edit => ([130, 77, 14], [255, 193, 106]),
            Self::ZoomOut | Self::ZoomIn | Self::Fit | Self::Actual | Self::Fullscreen => {
                ([10, 108, 106], [116, 229, 215])
            }
            Self::Previous | Self::Next => ([58, 78, 123], [170, 190, 235]),
            Self::Rotate | Self::RotateLeft => ([107, 61, 166], [203, 167, 255]),
            Self::OpenImage | Self::Copy | Self::Info => ([44, 84, 166], [149, 188, 255]),
        };
        let [r, g, b] = if ui.visuals().dark_mode { dark } else { light };
        Color32::from_rgb(r, g, b)
    }

    fn paint(self, painter: &Painter, center: Pos2, ink: Color32) {
        let direction = if matches!(self, Self::RotateLeft) {
            -1.
        } else {
            1.
        };
        let point = |x, y| center + vec2((x - 12.) * direction, y - 12.) * 0.9;
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
            Self::Rotate | Self::RotateLeft => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_logo_and_buttons_share_a_centerline() {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "test logo",
            egui::ColorImage::filled([1, 1], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                row(ui, |ui| {
                    let logo = brand(ui, &texture).rect.center().y;
                    for icon in [Icon::OpenImage, Icon::Rotate, Icon::Info] {
                        group(ui, |ui| {
                            let button = Tile::new(icon, "Test button").button(ui).rect.center().y;
                            assert!((logo - button).abs() <= 0.5, "logo {logo}, button {button}");
                        });
                    }
                });
            });
        });
    }

    #[test]
    fn rotation_panel_opens_on_hover_and_accepts_pointer_clicks() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let frame = |canvas: &mut Canvas, time, events| {
            let mut anchor = Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640., 480.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("rotation-test").show(ctx, |ui| {
                        ui.horizontal(|ui| anchor = rotation(ui, canvas, true).rect);
                    });
                },
            );
            let left = output.shapes.iter().find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.job.text == "90° left" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            });
            (anchor.center(), left)
        };
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        let (rotate, left) = frame(&mut canvas, 0., vec![]);
        assert!(left.is_none());
        frame(&mut canvas, 0.2, vec![egui::Event::PointerMoved(rotate)]);
        let _ = frame(&mut canvas, 0.9, vec![]);
        let (_, left) = frame(&mut canvas, 1., vec![]);
        let left = left.expect("dwelling over Rotate should show the left turn");
        assert_eq!(canvas.angle, 0.);

        let (_, open) = frame(&mut canvas, 1.1, vec![egui::Event::PointerMoved(left)]);
        assert!(open.is_some(), "moving into the panel must keep it open");
        frame(&mut canvas, 1.2, vec![click(left, true)]);
        frame(&mut canvas, 1.25, vec![click(left, false)]);
        assert_eq!(canvas.angle, -90.);

        frame(&mut canvas, 1.4, vec![egui::Event::PointerMoved(rotate)]);
        frame(&mut canvas, 1.5, vec![click(rotate, true)]);
        let (_, open) = frame(&mut canvas, 1.55, vec![click(rotate, false)]);
        assert_eq!(canvas.angle, 0.);
        assert!(open.is_none(), "a normal click only rotates the image");
    }
}
