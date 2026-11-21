use egui::{
    Color32, Context, FontId, Frame, Margin, RichText, Stroke, Style, TextStyle, Theme,
    ThemePreference, Ui, Visuals, vec2,
};

pub fn install(ctx: &Context, saved_theme: &str) {
    ctx.set_style_of(Theme::Light, style(false));
    ctx.set_style_of(Theme::Dark, style(true));
    ctx.options_mut(|options| options.fallback_theme = Theme::Light);
    ctx.set_theme(preference(saved_theme));
}

pub fn preference(saved_theme: &str) -> ThemePreference {
    match saved_theme {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

fn style(dark: bool) -> Style {
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

#[cfg(test)]
mod tests {
    use egui::{RawInput, epaint::FontColorTransferFunction};

    use super::*;

    #[test]
    fn font_atlas_uses_the_same_theme_as_the_visible_interface() {
        let ctx = Context::default();
        install(&ctx, "light");

        for (saved_theme, system_theme, dark) in [
            ("light", Some(Theme::Dark), false),
            ("dark", Some(Theme::Light), true),
            ("system", None, false),
            ("system", Some(Theme::Dark), true),
            ("system", Some(Theme::Light), false),
        ] {
            ctx.set_theme(preference(saved_theme));
            let input = RawInput {
                system_theme,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.label("Сегодня");
                assert_eq!(ui.visuals().dark_mode, dark);
                let expected = if dark {
                    FontColorTransferFunction::DARK_MODE_DEFAULT
                } else {
                    FontColorTransferFunction::LIGHT_MODE_DEFAULT
                };
                ctx.fonts(|fonts| {
                    assert_eq!(fonts.options().color_transfer_function, expected);
                    assert!(fonts.options().subpixel_binning);
                });
            });
            output.textures_delta.clear();
        }
    }
}
