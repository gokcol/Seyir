//! Compact drawing controls shared by the two viewer sidebars.
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
pub(crate) enum Glyph {
    Folder,
    Image,
    Info,
    Up,
    Refresh,
    Copy,
    Reveal,
    Check,
    Chevron,
    Settings,
    Save,
    Export,
    Close,
    Crop,
    Interface,
    Play,
    Pause,
}

pub(crate) fn glyph(ui: &egui::Ui, rect: Rect, kind: Glyph, ink: Color32) {
    let painter = ui.painter();
    let scale = rect.size().min_elem() / 20.;
    let pos = |x: f32, y: f32| rect.center() + Vec2::new(x, y) * scale;
    let stroke = Stroke::new(1.5 * scale, ink);
    let line = |points: &[(f32, f32)]| {
        painter.line(points.iter().map(|&(x, y)| pos(x, y)).collect(), stroke);
    };
    match kind {
        Glyph::Folder => {
            line(&[
                (-8., 6.),
                (-8., -6.),
                (-2., -6.),
                (0., -4.),
                (8., -4.),
                (8., 6.),
                (-8., 6.),
            ]);
            line(&[(-8., -1.), (8., -1.)]);
        }
        Glyph::Image => {
            painter.rect_stroke(
                Rect::from_min_max(pos(-7., -6.5), pos(7., 6.5)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(pos(3., -2.5), 1.1 * scale, ink);
            line(&[(-6., 4.), (-2., 0.), (1., 3.), (3., 1.5), (6., 4.5)]);
        }
        Glyph::Info => {
            painter.circle_stroke(rect.center(), 7. * scale, stroke);
            painter.circle_filled(pos(0., -3.1), 1.05 * scale, ink);
            line(&[(0., -0.8), (0., 3.6)]);
        }
        Glyph::Up => {
            line(&[(-5., -1.), (0., -6.), (5., -1.)]);
            line(&[(0., -6.), (0., 6.)]);
        }
        Glyph::Refresh => {
            let points: Vec<_> = (0..24)
                .map(|step| {
                    let angle = -(step as f32) * 4.9 / 23. - 0.1;
                    pos(angle.cos() * 6.5, angle.sin() * 6.5)
                })
                .collect();
            painter.line(points, stroke);
            line(&[(6.5, -5.5), (6.5, -0.5), (1.5, -0.5)]);
        }
        Glyph::Copy => {
            line(&[(-2., 3.), (-6., 3.), (-6., -7.), (1., -7.), (1., -3.)]);
            painter.rect_stroke(
                Rect::from_min_max(pos(-2., -3.), pos(6., 7.)),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Glyph::Reveal => {
            line(&[(0., -6.), (-6., -6.), (-6., 6.), (6., 6.), (6., 0.)]);
            line(&[(1., -6.), (6., -6.), (6., -1.)]);
            line(&[(6., -6.), (-1., 1.)]);
        }
        Glyph::Check => line(&[(-6., 0.), (-1.7, 4.3), (6., -4.)]),
        Glyph::Chevron => line(&[(-2.5, -4.5), (2., 0.), (-2.5, 4.5)]),
        Glyph::Settings => {
            let mut outline = Vec::with_capacity(25);
            for tooth in 0..6 {
                for (offset, radius) in [(-0.48, 5.8), (-0.29, 7.8), (0.29, 7.8), (0.48, 5.8)] {
                    let angle = std::f32::consts::TAU * (tooth as f32 + offset) / 6.;
                    let (sin, cos) = angle.sin_cos();
                    outline.push(pos(cos * radius, sin * radius));
                }
            }
            outline.push(outline[0]);
            painter.line(outline, stroke);
            painter.circle_stroke(rect.center(), 2.6 * scale, stroke);
        }
        Glyph::Save => {
            line(&[
                (-7., -7.),
                (4., -7.),
                (7., -4.),
                (7., 7.),
                (-7., 7.),
                (-7., -7.),
            ]);
            line(&[(-3., -7.), (-3., -2.), (3., -2.), (3., -7.)]);
            line(&[(-4., 7.), (-4., 1.5), (4., 1.5), (4., 7.)]);
        }
        Glyph::Export => {
            line(&[(-7., 1.), (-7., 7.), (7., 7.), (7., 1.)]);
            line(&[(-4., -3.), (0., -7.), (4., -3.)]);
            line(&[(0., -7.), (0., 3.)]);
        }
        Glyph::Close => {
            line(&[(-4.5, -4.5), (4.5, 4.5)]);
            line(&[(4.5, -4.5), (-4.5, 4.5)]);
        }
        Glyph::Crop => {
            line(&[(-4., -8.), (-4., 4.), (8., 4.)]);
            line(&[(-8., -4.), (4., -4.), (4., 8.)]);
        }
        Glyph::Interface => {
            painter.rect_stroke(
                Rect::from_min_max(pos(-8., -6.), pos(8., 6.)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(-3., -6.), (-3., 6.)]);
        }
        Glyph::Play => line(&[(-4., -6.), (6., 0.), (-4., 6.), (-4., -6.)]),
        Glyph::Pause => {
            line(&[(-3., -5.), (-3., 5.)]);
            line(&[(3., -5.), (3., 5.)]);
        }
    }
}

pub(crate) fn dialog_title(ui: &mut egui::Ui, icon: Glyph, title: &str, subtitle: Option<&str>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.;
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(46.), Sense::hover());
        let visual = ui.visuals();
        let accent = visual.selection.stroke.color;
        let (face, ink) =
            crate::theme::selected_tool_colors(visual.panel_fill, accent, visual.text_color());
        ui.painter().rect(
            badge,
            13,
            face,
            Stroke::new(1.0_f32, face.lerp_to_gamma(accent, 0.2)),
            egui::StrokeKind::Inside,
        );
        glyph(ui, badge.shrink(10.), icon, ink);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            ui.add(egui::Label::new(RichText::new(title).size(19.).strong()).wrap());
            if let Some(subtitle) = subtitle {
                ui.add(egui::Label::new(RichText::new(subtitle).size(12.).weak()).wrap());
            }
        });
    });
}

pub(crate) fn dialog_button(
    ui: &mut egui::Ui,
    icon: Glyph,
    label: &str,
    primary: bool,
) -> egui::Response {
    let visual = ui.visuals();
    let text_ink = visual.text_color();
    let font = FontId::proportional(12.5);
    let label = ui.painter().layout_no_wrap(label.into(), font, text_ink);
    let width = (label.size().x + 46.).max(86.);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 35.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label.job.text)
    });
    let visual = ui.visuals();
    let accent = visual.selection.stroke.color;
    let (accent_face, icon_ink) =
        crate::theme::selected_tool_colors(visual.panel_fill, accent, text_ink);
    let base = if primary {
        accent_face
    } else {
        visual.window_fill()
    };
    let fill = if response.is_pointer_button_down_on() {
        base.lerp_to_gamma(accent, 0.18)
    } else if response.hovered() {
        base.lerp_to_gamma(accent, 0.1)
    } else {
        base
    };
    let edge = if response.has_focus() {
        visual.selection.stroke
    } else if primary {
        Stroke::new(1.0_f32, base.lerp_to_gamma(accent, 0.28))
    } else {
        visual.window_stroke
    };
    ui.painter()
        .rect(rect, 8, fill, edge, egui::StrokeKind::Inside);
    let start = rect.center().x - (label.size().x + 25.) * 0.5;
    let icon_rect =
        Rect::from_center_size(Pos2::new(start + 9., rect.center().y), Vec2::splat(18.));
    glyph(
        ui,
        icon_rect,
        icon,
        if primary { icon_ink } else { text_ink },
    );
    let label_pos = Pos2::new(start + 25., rect.center().y - label.size().y * 0.5);
    ui.painter().galley(label_pos, label, text_ink);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn setting_switch(ui: &mut egui::Ui, icon: Glyph, label: &str, enabled: &mut bool) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 33.), Sense::click());
    if response.clicked() {
        *enabled = !*enabled;
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *enabled, label)
    });
    let visual = ui.visuals();
    let accent = visual.selection.stroke.color;
    let (tint, ink) =
        crate::theme::selected_tool_colors(visual.panel_fill, accent, visual.text_color());
    let focus = if response.has_focus() {
        visual.selection.stroke
    } else {
        Stroke::NONE
    };
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect(rect, 8, tint, focus, egui::StrokeKind::Inside);
    }
    glyph(
        ui,
        Rect::from_center_size(rect.left_center() + Vec2::new(18., 0.), Vec2::splat(18.)),
        icon,
        ink,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(36., 0.),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.),
        visual.text_color(),
    );
    let pill = Rect::from_center_size(
        rect.right_center() - Vec2::new(23., 0.),
        Vec2::new(30., 17.),
    );
    let track = if *enabled { ink } else { visual.window_fill() };
    let border = if *enabled {
        Stroke::NONE
    } else {
        visual.window_stroke
    };
    let dot = if *enabled {
        visual.panel_fill
    } else {
        visual.weak_text_color()
    };
    ui.painter()
        .rect(pill, 9, track, border, egui::StrokeKind::Inside);
    let offset = if *enabled { 6.5 } else { -6.5 };
    ui.painter()
        .circle_filled(pill.center() + Vec2::new(offset, 0.), 5., dot);
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

pub(crate) fn title(ui: &mut egui::Ui, icon: Glyph, title: &str, subtitle: Option<(&str, &str)>) {
    ui.horizontal(|ui| {
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(34.), Sense::hover());
        ui.painter()
            .rect_filled(badge, 9, ui.visuals().selection.bg_fill);
        glyph(
            ui,
            badge.shrink(7.),
            icon,
            ui.visuals().selection.stroke.color,
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.;
            ui.add(egui::Label::new(RichText::new(title).size(16.).strong()).truncate());
            if let Some((label, hint)) = subtitle {
                ui.add(egui::Label::new(RichText::new(label).size(11.5).weak()).truncate())
                    .on_hover_text(hint);
            }
        });
    });
}

pub(crate) fn action(
    ui: &mut egui::Ui,
    icon: Glyph,
    label: &str,
    caption: Option<&str>,
    emphasized: bool,
) -> egui::Response {
    let width = caption.map_or(28., |text| {
        ui.painter()
            .layout_no_wrap(
                text.into(),
                FontId::proportional(11.5),
                ui.visuals().text_color(),
            )
            .size()
            .x
            + 31.
    });
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let visuals = ui.visuals();
    let interaction = ui.style().interact(&response);
    let ink = if emphasized {
        visuals.selection.stroke.color
    } else {
        interaction.fg_stroke.color
    };
    let fill = if response.hovered() || response.is_pointer_button_down_on() {
        interaction.weak_bg_fill
    } else if emphasized {
        visuals.selection.bg_fill
    } else {
        visuals.faint_bg_color
    };
    ui.painter().rect(
        rect,
        6,
        fill,
        if response.has_focus() {
            visuals.selection.stroke
        } else {
            interaction.bg_stroke
        },
        egui::StrokeKind::Inside,
    );
    if let Some(caption) = caption {
        let center = rect.left_center() + Vec2::new(13., 0.);
        glyph(
            ui,
            Rect::from_center_size(center, Vec2::splat(15.)),
            icon,
            ink,
        );
        ui.painter().text(
            rect.left_center() + Vec2::new(24., 0.),
            Align2::LEFT_CENTER,
            caption,
            FontId::proportional(11.5),
            ink,
        );
    } else {
        glyph(ui, rect.shrink(6.), icon, ink);
    }
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn clipped_text(ui: &egui::Ui, rect: Rect, text: &str, size: f32, ink: Color32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), ink);
    job.wrap.max_width = rect.width().max(1.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let text = ui.painter().layout_job(job);
    let pos = Pos2::new(rect.left(), rect.center().y - text.size().y * 0.5);
    ui.painter().with_clip_rect(rect).galley(pos, text, ink);
}

pub(super) fn stat(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(8, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.;
            ui.label(RichText::new(label).size(11.).weak());
            ui.add(egui::Label::new(RichText::new(value).size(13.).strong()).wrap());
        });
}

pub(super) fn detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() * 0.35).clamp(49., 94.);
        ui.add_sized(
            [width, 15.],
            egui::Label::new(RichText::new(label).size(11.5).weak()).wrap(),
        );
        ui.add(
            egui::Label::new(RichText::new(value).size(12.))
                .wrap()
                .selectable(true),
        );
    });
}

pub(super) fn field(ui: &mut egui::Ui, label: &str, value: &mut String, multiline: bool) {
    let title = ui.label(RichText::new(label).size(11.5).strong());
    let edit = if multiline {
        egui::TextEdit::multiline(value).desired_rows(3)
    } else {
        egui::TextEdit::singleline(value)
    };
    ui.add(
        edit.desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(8, 6)),
    )
    .labelled_by(title.id);
    ui.add_space(3.);
}
