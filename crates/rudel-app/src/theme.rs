//! One cohesive dark "instrument panel" style for the whole app, tuned so the
//! chrome (panels, buttons, popups) and the Strudel-dark editor read as a
//! single surface: charcoal grays, rounded widgets, and Strudel's caret
//! yellow as the app-wide accent.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

/// Strudel's caret yellow — the single accent color for the whole app.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0xff, 0xcc, 0x00);
/// Status-light green while playing.
pub(crate) const GO: Color32 = Color32::from_rgb(0x3a, 0xcd, 0x41);
/// Stop button / error red.
pub(crate) const STOP: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);

pub(crate) fn apply(ctx: &egui::Context) {
    // The app is a dark instrument regardless of the OS theme; the editor has
    // its own theme setting on top.
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Small,
                FontId::new(10.5, FontFamily::Proportional),
            ),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.slider_width = 100.0;

        let v = &mut style.visuals;
        v.panel_fill = Color32::from_rgb(0x16, 0x16, 0x16);
        v.window_fill = Color32::from_rgb(0x1c, 0x1c, 0x1c);
        v.extreme_bg_color = Color32::from_rgb(0x0d, 0x0d, 0x0d);
        v.faint_bg_color = Color32::from_rgb(0x1f, 0x1f, 0x1f);
        v.hyperlink_color = ACCENT;
        v.slider_trailing_fill = true;
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(0xff, 0xcc, 0x00, 0x33);
        v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0xf2, 0xf2, 0xf2));

        let radius = CornerRadius::same(6);
        v.widgets.noninteractive.corner_radius = radius;
        v.widgets.inactive.corner_radius = radius;
        v.widgets.hovered.corner_radius = radius;
        v.widgets.active.corner_radius = radius;
        v.widgets.open.corner_radius = radius;

        // Separator / label colors.
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x2a));
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xc4, 0xc4, 0xc4));
        // Buttons and inputs: flat charcoal that brightens on hover/press.
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x24, 0x24, 0x24);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x24, 0x24, 0x24);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd8, 0xd8, 0xd8));
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x2e);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x2e);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4a, 0x4a));
        v.widgets.active.bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x38);
        v.widgets.open.weak_bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
    });
}
