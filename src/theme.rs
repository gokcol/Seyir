//! Cool chrome around a neutral image stage.
use eframe::egui::{self, Color32, Stroke};

pub(crate) const CANVAS: Color32 = Color32::from_rgb(25, 27, 32);
pub(crate) const CANVAS_INK: Color32 = Color32::from_rgb(219, 224, 236);

pub(crate) fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let visual = &mut style.visuals;
        let (panel, surface, muted, border, accent, selection, ink) = if visual.dark_mode {
            (
                rgb(0x181e28),
                rgb(0x232d3b),
                rgb(0x9faec3),
                rgb(0x38475c),
                rgb(0x63dbd3),
                rgb(0x24454e),
                rgb(0xf1f6ff),
            )
        } else {
            (
                rgb(0xf3f6fa),
                rgb(0xffffff),
                rgb(0x53637a),
                rgb(0xd4dce8),
                rgb(0x127f86),
                rgb(0xd9efef),
                rgb(0x1e2d43),
            )
        };
        visual.panel_fill = panel;
        visual.window_fill = surface;
        visual.faint_bg_color = surface;
        visual.extreme_bg_color = surface;
        visual.weak_text_color = Some(muted);
        visual.override_text_color = Some(ink);
        visual.window_stroke = Stroke::new(1.0_f32, border);
        visual.selection.bg_fill = selection;
        visual.selection.stroke = Stroke::new(1.0_f32, accent);
        visual.hyperlink_color = accent;
        for widget in [
            &mut visual.widgets.inactive,
            &mut visual.widgets.hovered,
            &mut visual.widgets.active,
            &mut visual.widgets.open,
        ] {
            widget.bg_stroke = Stroke::new(1.0_f32, border);
            widget.fg_stroke.color = ink;
            widget.bg_fill = surface;
            widget.weak_bg_fill = surface;
        }
        visual.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        visual.widgets.hovered.bg_fill = selection;
        visual.widgets.hovered.weak_bg_fill = selection;
        visual.widgets.active.bg_fill = selection;
        visual.widgets.active.weak_bg_fill = selection;
        visual.widgets.open.bg_fill = selection;
        visual.widgets.open.weak_bg_fill = selection;
        visual.widgets.hovered.bg_stroke.color = accent;
        visual.widgets.active.bg_stroke.color = accent;
    });
}

pub(crate) fn bar(ctx: &egui::Context, margin: i8) -> egui::Frame {
    let visual = ctx.style();
    egui::Frame::new()
        .fill(visual.visuals.panel_fill)
        .stroke(visual.visuals.window_stroke)
        .inner_margin(egui::Margin::symmetric(12, margin))
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
