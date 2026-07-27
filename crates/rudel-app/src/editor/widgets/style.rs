use crate::editor::settings::DrawTheme;
use eframe::egui;
use rudel_core::{Hap, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WidgetDrawColors {
    pub(super) background: egui::Color32,
    pub(super) active: egui::Color32,
    pub(super) inactive: egui::Color32,
    pub(super) text: egui::Color32,
    pub(super) muted: egui::Color32,
}

pub(super) fn widget_draw_colors(draw_theme: DrawTheme) -> WidgetDrawColors {
    WidgetDrawColors {
        background: draw_theme.line_background,
        active: draw_theme.foreground,
        inactive: draw_theme.gutter_foreground,
        text: draw_theme.foreground,
        muted: draw_theme.gutter_foreground,
    }
}

pub(super) fn event_color(hap: &Hap, fallback: egui::Color32) -> egui::Color32 {
    let controls = rudel_core::to_control_map(&hap.value);
    controls
        .get("color")
        .and_then(Value::as_str)
        .and_then(resolve_color)
        .unwrap_or(fallback)
}

/// The colour the editor should flash this event's source span with, or `None`
/// to use the theme's default flash. Ports `highlight.mjs`'s
/// `hap.value?.markcss || 'outline: solid 2px ${color}'` rule as far as a
/// native text editor can: Rudel paints a background rather than applying CSS,
/// so a `markcss` declaration is scanned for a colour and everything else in it
/// (borders, text-decoration, fonts) is ignored. With no `markcss`, the `color`
/// control is used, as upstream's default outline does.
pub(crate) fn mark_color(hap: &Hap) -> Option<egui::Color32> {
    let controls = rudel_core::to_control_map(&hap.value);
    let from_css = controls
        .get("markcss")
        .and_then(Value::as_str)
        .and_then(css_color);
    from_css.or_else(|| {
        controls
            .get("color")
            .and_then(Value::as_str)
            .and_then(resolve_color)
    })
}

/// The first colour-valued token in a CSS declaration list, e.g. `#ff0000` in
/// `outline: solid 2px #ff0000` or `red` in `background-color: red`.
fn css_color(css: &str) -> Option<egui::Color32> {
    css.split(';')
        .flat_map(|decl| decl.split(':').skip(1))
        .flat_map(str::split_whitespace)
        .find_map(resolve_color)
}

pub(super) fn event_alpha(hap: &Hap) -> f32 {
    let controls = rudel_core::to_control_map(&hap.value);
    let velocity = controls
        .get("velocity")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let gain = controls.get("gain").and_then(Value::as_f64).unwrap_or(1.0);
    (velocity * gain).clamp(0.0, 1.0) as f32
}

/// Resolve a pattern `color` control to a color: a `#rrggbb`/`#rrggbbaa` hex, or
/// a CSS named color via `draw/color.mjs`'s table (which resolves to `#rrggbb`).
pub(crate) fn resolve_color(color: &str) -> Option<egui::Color32> {
    if color.starts_with('#') {
        return parse_hex_color(color);
    }
    parse_hex_color(rudel_core::css_color_hex(color)?)
}

pub(super) fn parse_hex_color(color: &str) -> Option<egui::Color32> {
    let hex = color.strip_prefix('#')?;
    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    match hex.len() {
        6 => Some(egui::Color32::from_rgb(
            parse(0..2)?,
            parse(2..4)?,
            parse(4..6)?,
        )),
        8 => Some(egui::Color32::from_rgba_unmultiplied(
            parse(0..2)?,
            parse(2..4)?,
            parse(4..6)?,
            parse(6..8)?,
        )),
        _ => None,
    }
}

pub(super) fn color_with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let alpha = (a as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}
