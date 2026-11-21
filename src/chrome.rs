//! Compact pictograms and accessible controls for Seyir’s toolbar.
use crate::{canvas::Canvas, theme};
use eframe::egui::{
    self, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

const TILE_SIZE: f32 = 33.;
const ROW_HEIGHT: f32 = 41.;

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    File,
    OpenImage,
    OpenFolder,
    Save,
    SaveAs,
    Print,
    FolderPane,
    Previous,
    Next,
    ZoomOut,
    Fit,
    Actual,
    ZoomIn,
    Rotate,
    RotateLeft,
    Crop,
    FlipHorizontal,
    FlipVertical,
    Undo,
    Copy,
    Edit,
    Info,
    Fullscreen,
    Play,
    More,
    Settings,
}

pub(crate) enum TransformAction {
    Crop,
    FlipHorizontal,
    FlipVertical,
    Undo,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TransformTools {
    pub enabled: bool,
    pub can_undo: bool,
}

impl TransformTools {
    pub(crate) fn show(self, ui: &mut Ui) -> Option<TransformAction> {
        [
            (Icon::Crop, "Crop…", self.enabled, TransformAction::Crop),
            (
                Icon::FlipHorizontal,
                "Flip Horizontal",
                self.enabled,
                TransformAction::FlipHorizontal,
            ),
            (
                Icon::FlipVertical,
                "Flip Vertical",
                self.enabled,
                TransformAction::FlipVertical,
            ),
            (
                Icon::Undo,
                "Undo Image Edit",
                self.can_undo,
                TransformAction::Undo,
            ),
        ]
        .into_iter()
        .find_map(|(icon, label, enabled, action)| {
            menu_item(ui, icon, label, enabled, "").then_some(action)
        })
    }
}

pub(crate) struct Tile<'a> {
    icon: Icon,
    label: &'a str,
    hint: Option<&'a str>,
    enabled: bool,
    selected: Option<bool>,
    caption: bool,
    icon_only: bool,
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
            icon_only: false,
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

    pub(crate) fn icon_only(mut self) -> Self {
        self.icon_only = true;
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
        let settings = matches!(self.icon, Icon::Settings);
        let long_caption = self.caption && !self.icon_only;
        let short_caption =
            !self.icon_only && !self.caption && !settings && !ui.layout().is_vertical();
        let caption = if long_caption {
            self.label
        } else if short_caption {
            self.icon.toolbar_label()
        } else {
            ""
        };
        let galley = (!caption.is_empty()).then(|| {
            ui.painter().layout_no_wrap(
                caption.into(),
                FontId::proportional(if short_caption { 10. } else { 12.5 }),
                ui.visuals().text_color(),
            )
        });
        let caption_width = galley.as_ref().map_or(0., |label| label.size().x + 6.);
        let dropdown_width = if self.dropdown { 10. } else { 0. };
        let icon_width = if settings { 30. } else { TILE_SIZE };
        let width = if short_caption {
            icon_width.max(caption_width)
        } else {
            icon_width + caption_width
        };
        let height = if settings {
            26.
        } else if short_caption {
            ROW_HEIGHT
        } else {
            TILE_SIZE
        };
        let size = vec2((width + dropdown_width).max(self.min_width), height);
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
            let pressed = clickable && response.is_pointer_button_down_on();
            let selected = clickable && self.selected == Some(true);
            let visuals = ui.visuals();
            let surface = visuals.panel_fill;
            let tone = self.icon.ink(ui);
            let painter = ui.painter();
            let face = response.rect.shrink(1.);
            let mut ink = tone;
            if selected {
                let (fill, active_ink) =
                    theme::selected_tool_colors(surface, tone, visuals.text_color());
                painter.rect_filled(face, 6, fill);
                ink = active_ink;
            } else if pressed || (clickable && response.hovered()) {
                let intensity = if pressed { 0.08 } else { 0.06 };
                painter.rect_filled(face, 6, surface.lerp_to_gamma(tone, intensity));
            } else if !clickable {
                ink = surface.lerp_to_gamma(tone, 0.43);
            }
            if response.has_focus() {
                painter.rect_stroke(face, 6, Stroke::new(1_f32, tone), StrokeKind::Inside);
            }
            let center = if short_caption {
                response.rect.center() - vec2(dropdown_width * 0.5, 7.)
            } else if long_caption {
                pos2(
                    response.rect.left() + icon_width * 0.5,
                    response.rect.center().y,
                )
            } else {
                response.rect.center() - vec2(dropdown_width * 0.5, 0.)
            };
            let nudge = if pressed { 0.65 } else { 0. };
            self.icon.paint(painter, center + vec2(0., nudge), ink);
            if let Some(label) = galley {
                let at = if short_caption {
                    pos2(center.x, response.rect.center().y + 11.) - label.size() * 0.5
                } else {
                    pos2(response.rect.left() + 29., center.y - label.size().y * 0.5)
                };
                let label_ink = if selected {
                    visuals.text_color()
                } else if clickable {
                    visuals.weak_text_color()
                } else {
                    surface.lerp_to_gamma(visuals.weak_text_color(), 0.52)
                };
                painter.galley_with_override_text_color(at, label, label_ink);
            }
            if self.dropdown {
                let x = face.max.x - 10.;
                let y = center.y;
                painter.add(Shape::line(
                    vec![
                        pos2(x - 2.8, y - 1.2),
                        pos2(x, y + 1.5),
                        pos2(x + 2.8, y - 1.2),
                    ],
                    Stroke::new(1.3_f32, ink),
                ));
            }
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

pub(crate) fn row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let size = vec2(ui.available_width(), ROW_HEIGHT);
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(10., 6.);
        add(ui);
    });
}

pub(crate) fn group(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    if ui.layout().is_vertical() {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 3.;
            add(ui);
        });
    } else {
        let size = vec2(ui.available_size_before_wrap().x, ROW_HEIGHT);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            add(ui);
        });
    }
}

pub(crate) fn brand(ui: &mut Ui, texture: &egui::TextureHandle) -> Response {
    ui.add(egui::Image::new((texture.id(), Vec2::splat(ROW_HEIGHT))).alt_text("Seyir"))
}

pub(crate) fn file_menu(ui: &mut Ui, add: impl FnMut(&mut Ui)) -> Response {
    let file = Tile::new(Icon::File, "File").button(ui);
    let open = file.clicked().then_some(egui::SetOpenCommand::Toggle);
    tool_menu(ui, &file, open, add);
    file
}

pub(crate) fn copy_menu(ui: &mut Ui, enabled: bool, add: impl FnMut(&mut Ui)) -> Response {
    let copy = Tile::new(Icon::Copy, "Copy Image")
        .enabled(enabled)
        .button(ui);
    if copy.enabled() {
        let keyboard = copy.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10));
        let open = keyboard.then_some(egui::SetOpenCommand::Bool(true));
        tool_menu(ui, &copy, open, add);
    }
    copy
}

fn tool_menu(
    ui: &mut Ui,
    button: &Response,
    open: Option<egui::SetOpenCommand>,
    mut add: impl FnMut(&mut Ui),
) {
    const WIDTH: f32 = 181.;
    let frame = theme::floating(ui.ctx());
    let align = if ui.layout().is_vertical() {
        egui::RectAlign::RIGHT_START
    } else {
        egui::RectAlign::BOTTOM_START
    };
    let mut contents = |ui: &mut Ui| {
        ui.set_min_width(WIDTH);
        add(ui);
    };
    let popup = egui::Popup::menu(button)
        .open_memory(open)
        .align(align)
        .gap(2.)
        .frame(frame);
    if popup.show(&mut contents).is_none() {
        let mut tooltip = egui::Tooltip::for_enabled(button).width(WIDTH);
        tooltip.popup = tooltip
            .popup
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .frame(frame)
            .align(align)
            .gap(2.);
        tooltip.show(contents);
    }
}

pub(crate) fn menu_item(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    shortcut: &str,
) -> bool {
    if menu_button(ui, icon, label, enabled, shortcut).clicked() {
        ui.close();
        true
    } else {
        false
    }
}

fn menu_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, shortcut: &str) -> Response {
    let icon_id = egui::Id::new("menu-icon");
    let icon_slot = egui::Atom::custom(icon_id, vec2(20., 19.));
    let mut button = egui::Button::new((icon_slot, label))
        .frame_when_inactive(false)
        .min_size(vec2(0., 23.));
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    let item = ui.add_enabled_ui(enabled, |ui| button.atom_ui(ui)).inner;
    if let Some(rect) = item.rect(icon_id) {
        let ink = icon.ink(ui);
        let ink = if item.response.enabled() {
            ink
        } else {
            ui.visuals().window_fill().lerp_to_gamma(ink, 0.43)
        };
        icon.paint(ui.painter(), rect.center(), ink);
    }
    item.response
}

pub(crate) fn rotation_item(
    ui: &mut Ui,
    canvas: &mut Canvas,
    enabled: bool,
    tools: TransformTools,
) -> Option<TransformAction> {
    let row = menu_button(ui, Icon::Rotate, "Rotation", enabled, "›");
    if !row.enabled() {
        return None;
    }
    let accent = Icon::Rotate.ink(ui);
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::SubMenu::new()
        .config(config)
        .show(ui, &row, |ui| {
            rotation_panel(ui, canvas, accent, false, tools)
        })
        .and_then(|response| response.inner)
}

pub(crate) fn rotation(
    ui: &mut Ui,
    canvas: &mut Canvas,
    enabled: bool,
    tools: TransformTools,
) -> (Response, Option<TransformAction>) {
    let rotate = Tile::new(Icon::Rotate, "Rotate right 90°")
        .enabled(enabled)
        .button(ui);
    if rotate.clicked() {
        canvas.rotate_by(90.);
    }
    if !enabled {
        return (rotate, None);
    }

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
    if let Some(popup) =
        keyboard_popup.show(|ui| rotation_panel(ui, canvas, accent, keyboard, tools))
    {
        return (rotate, popup.inner);
    }
    let mut tooltip = egui::Tooltip::for_enabled(&rotate).width(ROTATION_WIDTH);
    tooltip.popup = tooltip.popup.frame(frame).align(align).gap(3.);
    let action = tooltip
        .show(|ui| rotation_panel(ui, canvas, accent, false, tools))
        .and_then(|response| response.inner);
    (rotate, action)
}

const ROTATION_WIDTH: f32 = 218.;

fn rotation_panel(
    ui: &mut Ui,
    canvas: &mut Canvas,
    accent: Color32,
    focus_left: bool,
    tools: TransformTools,
) -> Option<TransformAction> {
    ui.set_min_width(ROTATION_WIDTH);
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

    let button_width = (ROTATION_WIDTH - ui.spacing().item_spacing.x) * 0.5;
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
    ui.spacing_mut().slider_width = ROTATION_WIDTH;
    ui.visuals_mut().selection.bg_fill = ui.visuals().window_fill().lerp_to_gamma(accent, 0.43);
    ui.add(
        egui::Slider::new(&mut canvas.angle, -180.0..=180.0)
            .fixed_decimals(1)
            .show_value(false)
            .trailing_fill(true),
    )
    .labelled_by(angle_label);
    ui.separator();
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        tools.show(ui)
    })
    .inner
}

impl Icon {
    fn toolbar_label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::OpenImage => "Open",
            Self::OpenFolder => "Folder",
            Self::Save | Self::SaveAs => "Save",
            Self::Print => "Print",
            Self::FolderPane => "Browse",
            Self::Previous => "Prev",
            Self::Next => "Next",
            Self::ZoomOut => "Zoom−",
            Self::Fit => "Fit",
            Self::Actual => "Actual",
            Self::ZoomIn => "Zoom+",
            Self::Rotate | Self::RotateLeft => "Rotate",
            Self::Crop => "Crop",
            Self::FlipHorizontal | Self::FlipVertical => "Flip",
            Self::Undo => "Undo",
            Self::Copy => "Copy",
            Self::Edit => "Bediz",
            Self::Info => "Info",
            Self::Fullscreen => "Screen",
            Self::Play => "Slide",
            Self::More => "More",
            Self::Settings => "",
        }
    }

    fn ink(self, ui: &Ui) -> Color32 {
        let (light, dark) = match self {
            Self::OpenFolder | Self::FolderPane | Self::Edit | Self::Save | Self::SaveAs => {
                ([130, 77, 14], [255, 193, 106])
            }
            Self::ZoomOut
            | Self::ZoomIn
            | Self::Fit
            | Self::Actual
            | Self::Fullscreen
            | Self::Print
            | Self::Play => ([10, 108, 106], [116, 229, 215]),
            Self::Previous | Self::Next | Self::More | Self::Settings => {
                ([58, 78, 123], [170, 190, 235])
            }
            Self::Rotate
            | Self::RotateLeft
            | Self::Crop
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::Undo => ([107, 61, 166], [203, 167, 255]),
            Self::File | Self::OpenImage | Self::Copy | Self::Info => {
                ([44, 84, 166], [149, 188, 255])
            }
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
            Self::File => {
                line(&[
                    (6., 3.),
                    (14., 3.),
                    (19., 8.),
                    (19., 21.),
                    (6., 21.),
                    (6., 3.),
                ]);
                line(&[(14., 3.), (14., 8.), (19., 8.)]);
                line(&[(9.2, 12.5), (15.5, 12.5)]);
                line(&[(9.2, 16.5), (15.5, 16.5)]);
            }
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
            Self::Save | Self::SaveAs => {
                let x = if matches!(self, Self::SaveAs) {
                    9.5
                } else {
                    12.
                };
                line(&[(x, 3.), (x, 13.)]);
                line(&[(x - 4., 9.3), (x, 13.3), (x + 4., 9.3)]);
                line(&[(4., 14.5), (4., 20.), (20., 20.), (20., 14.5)]);
                if matches!(self, Self::SaveAs) {
                    line(&[(16., 6.5), (22., 6.5)]);
                    line(&[(19., 3.5), (19., 9.5)]);
                }
            }
            Self::Print => {
                line(&[(7., 9.), (7., 3.5), (17., 3.5), (17., 9.)]);
                line(&[
                    (7., 16.5),
                    (3.5, 16.5),
                    (3.5, 9.),
                    (20.5, 9.),
                    (20.5, 16.5),
                    (17., 16.5),
                ]);
                outline(7., 14., 17., 21.);
                dot(17., 11.5, 1.);
            }
            Self::FolderPane => {
                outline(3.5, 5., 20.5, 19.);
                line(&[(9.2, 5.), (9.2, 19.)]);
                line(&[(12.4, 9.2), (17.6, 9.2)]);
                line(&[(12.4, 14.8), (17.6, 14.8)]);
            }
            Self::Play => {
                painter.add(Shape::convex_polygon(
                    vec![point(8., 4.), point(20., 12.), point(8., 20.)],
                    ink,
                    Stroke::NONE,
                ));
            }
            Self::More => {
                dot(5., 12., 1.45);
                dot(12., 12., 1.45);
                dot(19., 12., 1.45);
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
            Self::Crop => {
                line(&[(7., 3.), (7., 17.), (21., 17.)]);
                line(&[(3., 7.), (17., 7.), (17., 21.)]);
                line(&[(17., 7.), (20.5, 3.5)]);
            }
            Self::FlipHorizontal | Self::FlipVertical => {
                let transform = |(x, y)| {
                    if matches!(self, Self::FlipVertical) {
                        (24. - y, x)
                    } else {
                        (x, y)
                    }
                };
                for shape in [
                    [(3., 7.), (8.5, 12.), (3., 17.), (3., 7.)],
                    [(21., 7.), (15.5, 12.), (21., 17.), (21., 7.)],
                ] {
                    line(&shape.map(transform));
                }
                for y in [4.5, 9.5, 14.5, 19.5] {
                    line(&[(12., y), (12., y + 1.5)].map(transform));
                }
            }
            Self::Undo => {
                line(&[(9., 4.5), (4., 9.5), (9., 14.5)]);
                line(&[
                    (4., 9.5),
                    (13.5, 9.5),
                    (17., 10.5),
                    (19., 13.),
                    (19., 16.),
                    (17., 18.5),
                    (13.5, 20.),
                ]);
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
            Self::Settings => {
                let perimeter = (0..24)
                    .map(|step| {
                        let angle = std::f32::consts::TAU * step as f32 / 24.;
                        let radius = if matches!(step % 4, 1 | 2) { 8.1 } else { 6.1 };
                        center + Vec2::angled(angle) * radius
                    })
                    .collect();
                painter.add(Shape::closed_line(perimeter, stroke));
                circle(12., 12., 2.7);
            }
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
        let groups: [&[Icon]; 5] = [
            &[Icon::File, Icon::FolderPane],
            &[Icon::Previous, Icon::Next],
            &[
                Icon::ZoomOut,
                Icon::Fit,
                Icon::Actual,
                Icon::ZoomIn,
                Icon::Rotate,
            ],
            &[Icon::Copy, Icon::Edit],
            &[Icon::Info, Icon::Fullscreen, Icon::Play],
        ];
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(636., 420.))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                row(ui, |ui| {
                    let edge = ui.max_rect().right();
                    let logo = brand(ui, &texture).rect.center().y;
                    for tools in groups {
                        group(ui, |ui| {
                            for icon in tools.iter().copied() {
                                let mut tile = Tile::new(icon, "Test button");
                                if matches!(icon, Icon::Edit) {
                                    tile = tile.dropdown();
                                }
                                let rect = tile.button(ui).rect;
                                let button = rect.center().y;
                                assert!(
                                    (logo - button).abs() <= 0.5,
                                    "logo {logo}, button {button}"
                                );
                                assert!(rect.right() <= edge);
                            }
                        });
                    }
                });
            });
        });
    }

    #[test]
    fn file_actions_are_clickable_after_opening_on_hover_or_keyboard() {
        let ctx = egui::Context::default();
        let mut opened = 0;
        let mut frame = |time, events, focus| {
            let mut anchor = Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640., 480.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("file-test").show(ctx, |ui| {
                        let file = file_menu(ui, |ui| {
                            if menu_item(ui, Icon::OpenImage, "Open Image…", true, "") {
                                opened += 1;
                            }
                        });
                        anchor = file.rect;
                        if focus {
                            file.request_focus();
                        }
                    });
                },
            );
            let item = output.shapes.iter().find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.job.text == "Open Image…" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            });
            (anchor.center(), item)
        };
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        let (file, item) = frame(0., vec![], false);
        assert!(item.is_none());
        frame(0.2, vec![egui::Event::PointerMoved(file)], false);
        frame(0.9, vec![], false);
        let item = frame(1., vec![], false).1.expect("File menu on hover");
        frame(1.1, vec![egui::Event::PointerMoved(item)], false);
        frame(1.2, vec![click(item, true)], false);
        frame(1.25, vec![click(item, false)], true);

        let keyboard = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(1.3, vec![keyboard], false);
        let item = frame(1.35, vec![], false)
            .1
            .expect("focused File menu opens with Enter");
        frame(1.4, vec![egui::Event::PointerMoved(item)], false);
        frame(1.45, vec![click(item, true)], false);
        frame(1.5, vec![click(item, false)], false);
        assert_eq!(opened, 2);
    }

    #[test]
    fn rotation_panel_opens_on_hover_and_accepts_pointer_clicks() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let frame = |canvas: &mut Canvas, time, events| {
            let mut anchor = Rect::NOTHING;
            let mut action = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640., 480.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("rotation-test").show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            let tools = TransformTools {
                                enabled: true,
                                can_undo: false,
                            };
                            let (response, chosen) = rotation(ui, canvas, true, tools);
                            anchor = response.rect;
                            action = chosen;
                        });
                    });
                },
            );
            let label = |label| {
                output.shapes.iter().find_map(|shape| match &shape.shape {
                    Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
            };
            (anchor.center(), label("90° left"), label("Crop…"), action)
        };
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        let (rotate, left, _, _) = frame(&mut canvas, 0., vec![]);
        assert!(left.is_none());
        frame(&mut canvas, 0.2, vec![egui::Event::PointerMoved(rotate)]);
        let _ = frame(&mut canvas, 0.9, vec![]);
        let (_, left, crop, _) = frame(&mut canvas, 1., vec![]);
        let left = left.expect("dwelling over Rotate should show the left turn");
        assert!(crop.is_some(), "Crop shares the rotation panel");
        assert_eq!(canvas.angle, 0.);

        let (_, open, _, _) = frame(&mut canvas, 1.1, vec![egui::Event::PointerMoved(left)]);
        assert!(open.is_some(), "moving into the panel must keep it open");
        frame(&mut canvas, 1.2, vec![click(left, true)]);
        frame(&mut canvas, 1.25, vec![click(left, false)]);
        assert_eq!(canvas.angle, -90.);

        frame(&mut canvas, 1.4, vec![egui::Event::PointerMoved(rotate)]);
        frame(&mut canvas, 1.5, vec![click(rotate, true)]);
        let (_, open, _, _) = frame(&mut canvas, 1.55, vec![click(rotate, false)]);
        assert_eq!(canvas.angle, 0.);
        assert!(open.is_none(), "a normal click only rotates the image");

        frame(
            &mut canvas,
            1.7,
            vec![egui::Event::PointerMoved(rotate + vec2(90., 0.))],
        );
        frame(&mut canvas, 1.9, vec![egui::Event::PointerMoved(rotate)]);
        frame(&mut canvas, 2.6, vec![]);
        let (_, _, crop, _) = frame(&mut canvas, 2.7, vec![]);
        let crop = crop.expect("hovering again restores the transform actions");
        frame(&mut canvas, 2.8, vec![egui::Event::PointerMoved(crop)]);
        frame(&mut canvas, 2.9, vec![click(crop, true)]);
        let (_, _, _, action) = frame(&mut canvas, 2.95, vec![click(crop, false)]);
        assert!(matches!(action, Some(TransformAction::Crop)));
    }
}
