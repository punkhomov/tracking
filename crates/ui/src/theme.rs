use egui::{
    Color32, Context, CornerRadius, FontId, Stroke, Style, TextStyle, Theme, ThemePreference, Ui,
    Visuals, vec2,
};

const RADIUS: u8 = 10;
pub(crate) const CONTROL_RADIUS: u8 = RADIUS - 2;
pub(crate) const CARD_RADIUS: u8 = RADIUS + 4;
pub(crate) const CONTROL_HEIGHT: f32 = 32.0;

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
    let colors = palette(dark);
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.weak_text_color = Some(colors.muted);
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke.color = colors.accent;
    style.visuals.extreme_bg_color = colors.canvas;
    style.visuals.panel_fill = colors.surface;
    style.visuals.window_fill = colors.surface;
    style.visuals.window_stroke.color = colors.border;
    style.visuals.widgets.noninteractive.bg_stroke.color = colors.border;
    style.visuals.widgets.noninteractive.fg_stroke.color = colors.ink;
    style.visuals.error_fg_color = colors.error;
    let widgets = &mut style.visuals.widgets;
    for (widget, fill) in [
        (&mut widgets.inactive, colors.control),
        (&mut widgets.hovered, colors.hover),
        (&mut widgets.active, colors.selection),
        (&mut widgets.open, colors.selection),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.fg_stroke.color = colors.ink;
        widget.corner_radius = CornerRadius::same(CONTROL_RADIUS);
        widget.expansion = 0.0;
    }
    widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.hovered.bg_stroke = Stroke::new(1.0, colors.muted);
    widgets.active.bg_stroke = Stroke::new(1.0, colors.accent);
    widgets.open.bg_stroke = widgets.active.bg_stroke;
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(20.0));
    for text_style in [TextStyle::Body, TextStyle::Button] {
        style
            .text_styles
            .insert(text_style, FontId::proportional(14.0));
    }
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.interact_size.y = CONTROL_HEIGHT;
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

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    ink: Color32,
    muted: Color32,
    canvas: Color32,
    surface: Color32,
    border: Color32,
    control: Color32,
    hover: Color32,
    pub(crate) selection: Color32,
    accent: Color32,
    pub(crate) on_accent: Color32,
    error: Color32,
}

pub(crate) const fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            ink: rgb(0xe3ede8),
            muted: rgb(0xaabeb4),
            canvas: rgb(0x16221e),
            surface: rgb(0x202e29),
            border: rgb(0x43544c),
            control: rgb(0x293b33),
            hover: rgb(0x3b544a),
            selection: rgb(0x305044),
            accent: rgb(0x8dd0b6),
            on_accent: rgb(0x16221e),
            error: rgb(0xffada5),
        }
    } else {
        Palette {
            ink: rgb(0x1d2e28),
            muted: rgb(0x536760),
            canvas: rgb(0xf2f5f1),
            surface: Color32::WHITE,
            border: rgb(0xd8e2dc),
            control: rgb(0xecf1ed),
            hover: rgb(0xe1ebe5),
            selection: rgb(0xdcece3),
            accent: rgb(0x1f5c4b),
            on_accent: Color32::WHITE,
            error: rgb(0x9b251c),
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{RawInput, epaint::FontColorTransferFunction};

    use super::*;

    #[test]
    fn text_roles_have_sufficient_contrast_in_both_schemes() {
        fn luminance(color: Color32) -> f32 {
            let linear = |channel: u8| {
                let channel = f32::from(channel) / 255.0;
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
        }
        for dark in [false, true] {
            let colors = palette(dark);
            for (text, background) in [
                (colors.ink, colors.surface),
                (colors.ink, colors.selection),
                (colors.ink, colors.control),
                (colors.ink, colors.hover),
                (colors.muted, colors.canvas),
                (colors.muted, colors.surface),
                (colors.accent, colors.selection),
                (colors.on_accent, colors.accent),
                (colors.error, colors.canvas),
            ] {
                let a = luminance(text);
                let b = luminance(background);
                assert!(
                    (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                    "{text:?} on {background:?}"
                );
            }
        }
    }

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
