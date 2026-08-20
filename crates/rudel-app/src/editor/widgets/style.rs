use crate::editor::settings::DrawTheme;
use eframe::egui;
use rudel_core::{Hap, Value, ValueMap};
use std::borrow::Cow;

/// The three theme colors an inline widget draws with: its panel, whatever is
/// sounding or labelled, and whatever is idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WidgetDrawColors {
    pub(super) background: egui::Color32,
    pub(super) foreground: egui::Color32,
    pub(super) muted: egui::Color32,
}

pub(super) fn widget_draw_colors(draw_theme: DrawTheme) -> WidgetDrawColors {
    WidgetDrawColors {
        background: draw_theme.line_background,
        foreground: draw_theme.foreground,
        muted: draw_theme.gutter_foreground,
    }
}

/// One control off a hap, read without copying its map.
///
/// `to_control_map` hands back a map by value, so it clones the hap's whole
/// `IndexMap` — and the painters call it per hap per frame just to look one key
/// up. Only `Value::Map` haps carry controls at all: the maps `to_control_map`
/// synthesizes for the other value shapes hold nothing but `s`, `n` and `note`.
pub(super) fn control<'a>(hap: &'a Hap, key: &str) -> Option<&'a Value> {
    debug_assert!(
        !matches!(key, "s" | "n" | "note"),
        "{key} can come from a non-map hap — use controls() for it"
    );
    match &hap.value {
        Value::Map(map) => map.get(key),
        _ => None,
    }
}

/// A hap's full control map with its transpose controls applied, borrowed
/// unless something actually has to change. For the keys `control` refuses —
/// `s`, `n`, `note` — which a bare string, list or number hap also produces.
///
/// `apply_transpose_controls` no-ops without `mtranspose` or `ctranspose`, so
/// gating on those is what keeps the common case a borrow.
pub(super) fn controls(hap: &Hap) -> Cow<'_, ValueMap> {
    let mut controls = match &hap.value {
        Value::Map(map) => Cow::Borrowed(map),
        other => Cow::Owned(rudel_core::to_control_map(other)),
    };
    if controls.contains_key("mtranspose") || controls.contains_key("ctranspose") {
        rudel_core::tonal::apply_transpose_controls(controls.to_mut(), hap.context.scale.as_deref());
    }
    controls
}

pub(super) fn event_color(hap: &Hap, fallback: egui::Color32) -> egui::Color32 {
    control(hap, "color")
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
    let from_css = control(hap, "markcss")
        .and_then(Value::as_str)
        .and_then(css_color);
    from_css.or_else(|| {
        control(hap, "color")
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
    let velocity = control(hap, "velocity")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let gain = control(hap, "gain").and_then(Value::as_f64).unwrap_or(1.0);
    (velocity * gain).clamp(0.0, 1.0) as f32
}

/// Resolve a pattern `color` control to a color: a `#rrggbb`/`#rrggbbaa` hex, or
/// a CSS named color via `draw/color.mjs`'s table (which resolves to `#rrggbb`).
pub(crate) fn resolve_color(color: &str) -> Option<egui::Color32> {
    let hex = if color.starts_with('#') {
        color
    } else {
        rudel_core::css_color_hex(color)?
    };
    egui::Color32::from_hex(hex).ok()
}

pub(super) fn color_with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let alpha = (a as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}
