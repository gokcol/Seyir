//! Matched light and dark palettes for the five viewing layouts.
use crate::layout::Layout;
use eframe::egui::{self, Color32, Stroke};

pub(crate) const CANVAS_INK: Color32 = Color32::from_rgb(219, 224, 236);

fn palette(layout: Layout, dark: bool) -> [Color32; 8] {
    let colors = match (layout, dark) {
        // Panel, card, secondary text, border, accent, selection, text, canvas.
        (Layout::StudioDesk, false) => [
            0xf6f6f2, 0xfcfcf9, 0x56675c, 0xdadfd6, 0x2f6957, 0xe0eee6, 0x26362d, 0x344038,
        ],
        (Layout::StudioDesk, true) => [
            0x232724, 0x2b332d, 0xa8b7ab, 0x424c43, 0x9bd7be, 0x354d40, 0xf0f5ee, 0x171e1a,
        ],
        (Layout::Filmstrip, false) => [
            0xf3f6fa, 0xffffff, 0x53637a, 0xd4dce8, 0x127f86, 0xd9efef, 0x1e2d43, 0x191b20,
        ],
        (Layout::Filmstrip, true) => [
            0x181e28, 0x232d3b, 0x9faec3, 0x38475c, 0x63dbd3, 0x24454e, 0xf1f6ff, 0x191b20,
        ],
        (Layout::ContactSheet, false) => [
            0xf5efe8, 0xfbf8f3, 0x795b43, 0xe2d5c5, 0x96501d, 0xf8e9d8, 0x402e23, 0x393129,
        ],
        (Layout::ContactSheet, true) => [
            0x2b251f, 0x322b23, 0xc2aa96, 0x554639, 0xf4ba88, 0x4e3c30, 0xfaf0e7, 0x1d1914,
        ],
        (Layout::FocusMode, false) => [
            0xeef2fb, 0xf8faff, 0x526782, 0xd1ddef, 0x265eae, 0xe4edfc, 0x1c2c44, 0x111720,
        ],
        (Layout::FocusMode, true) => [
            0x202a3b, 0x273348, 0xa5b5ce, 0x3f526f, 0xadcaff, 0x2c4062, 0xf0f5ff, 0x111720,
        ],
        (Layout::ToolRail, false) => [
            0xf1eef6, 0xfaf8fc, 0x67587b, 0xded7e9, 0x603dc2, 0xebe4fb, 0x332643, 0x292636,
        ],
        (Layout::ToolRail, true) => [
            0x251f2e, 0x2c2638, 0xb9a9cc, 0x4e435b, 0xd1baff, 0x45355e, 0xf6efff, 0x18151f,
        ],
    };
    colors.map(rgb)
}

pub(crate) fn apply(ctx: &egui::Context, layout: Layout) {
    ctx.all_styles_mut(|style| {
        let visual = &mut style.visuals;
        let [panel, surface, muted, border, accent, selection, ink, _] =
            palette(layout, visual.dark_mode);
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

pub(crate) fn canvas(ctx: &egui::Context, layout: Layout) -> Color32 {
    palette(layout, ctx.style().visuals.dark_mode)[7]
}

pub(crate) fn bar(ctx: &egui::Context, margin: i8) -> egui::Frame {
    let visual = ctx.style();
    egui::Frame::new()
        .fill(visual.visuals.panel_fill)
        .stroke(visual.visuals.window_stroke)
        .inner_margin(egui::Margin::symmetric(12, margin))
}

pub(crate) fn floating(ctx: &egui::Context) -> egui::Frame {
    let style = ctx.style();
    egui::Frame::popup(&style)
        .fill(style.visuals.panel_fill)
        .corner_radius(12)
        .inner_margin(egui::Margin::same(8))
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
