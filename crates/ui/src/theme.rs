use egui::{Color32, FontId, Frame, Margin, RichText, Stroke, Style, TextStyle, Ui, Visuals, vec2};

pub fn style(dark: bool) -> Style {
    let mut style = Style {
        visuals: if dark {
            Visuals::dark()
        } else {
            Visuals::light()
        },
        ..Style::default()
    };
    let (ink, surface, background, accent) = if dark {
        (rgb(0xe3ede8), rgb(0x202e29), rgb(0x16221e), rgb(0x8dd0b6))
    } else {
        (rgb(0x1d2e28), Color32::WHITE, rgb(0xf2f5f1), rgb(0x1f5c4b))
    };
    style.visuals.override_text_color = Some(ink);
    style.visuals.selection.bg_fill = if dark { rgb(0x305044) } else { rgb(0xe1eee7) };
    style.visuals.selection.stroke.color = accent;
    style.visuals.extreme_bg_color = background;
    style.visuals.panel_fill = surface;
    style.visuals.widgets.noninteractive.bg_stroke.color =
        if dark { rgb(0x43544c) } else { rgb(0xd8e2dc) };
    style.visuals.error_fg_color = if dark { rgb(0xffada5) } else { rgb(0x9b251c) };
    for text_style in [TextStyle::Body, TextStyle::Button] {
        style
            .text_styles
            .insert(text_style, FontId::proportional(14.0));
    }
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(12.0, 7.0);
    style
}

const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

pub fn parse_color(value: &str) -> Option<Color32> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(rgb)
}

pub fn color_string(value: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", value.r(), value.g(), value.b())
}

pub fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.stroke.color
}

pub fn primary(ui: &mut Ui, label: &str) -> egui::Response {
    let foreground = if ui.visuals().dark_mode {
        rgb(0x16221e)
    } else {
        Color32::WHITE
    };
    ui.add(
        egui::Button::new(RichText::new(label).strong().color(foreground))
            .fill(accent(ui))
            .corner_radius(8),
    )
}

pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(14)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui)
        })
        .inner
}
