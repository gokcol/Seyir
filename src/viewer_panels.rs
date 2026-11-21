//! Compact drawing controls shared by the two viewer sidebars.
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Folder,
    Image,
    Info,
    Up,
    Refresh,
    Copy,
    Reveal,
    Check,
    Chevron,
}

pub(super) fn glyph(ui: &egui::Ui, rect: Rect, kind: Glyph, ink: Color32) {
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
    }
}

pub(super) fn title(ui: &mut egui::Ui, icon: Glyph, title: &str, subtitle: Option<(&str, &str)>) {
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

pub(super) fn action(
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

pub(super) fn clipped_text(ui: &egui::Ui, rect: Rect, text: &str, size: f32, ink: Color32) {
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
