use super::options::option_f32;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;

pub(super) fn default_surface_size(widget_type: &str) -> egui::Vec2 {
    match widget_type {
        // Wider than Strudel's 275, and the one default here that deliberately
        // is. Upstream's canvas is 275 with `size` = 275/5 = 55, which puts
        // `inset: 3`'s "now" arc at radius 165 — outside the 137.5 a 275 canvas
        // inscribes — so the current position, the thing a live-coding
        // visualiser most needs to show, is clipped into the corners.
        //
        // The fix is the surface, not the geometry: `spiral_size` still
        // defaults to 55 (see `options::VisualWidgetOptions::from_widget`), so
        // a pattern draws exactly what it draws upstream and `inset` keeps its
        // documented value. Only the canvas it is drawn on is bigger. A `size`
        // option still sets both, the way `_spiral`'s widget registration does
        // upstream.
        "_spiral" => egui::vec2(400.0, 400.0),
        "_pitchwheel" | "_spectrum" | "_shader" => egui::vec2(200.0, 200.0),
        "_wordfall" => egui::vec2(500.0, 120.0),
        "_claviature" => egui::vec2(500.0, 100.0),
        _ => egui::vec2(500.0, 60.0),
    }
}

pub(super) fn surface_size(widget: &WidgetDecoration) -> egui::Vec2 {
    let default = default_surface_size(&widget.widget_type);
    let size = option_f32(&widget.options, "size");
    let width = option_f32(&widget.options, "width")
        .or(size)
        .unwrap_or(default.x)
        .max(20.0);
    let height = option_f32(&widget.options, "height")
        .or(size)
        .unwrap_or(default.y)
        .max(20.0);
    egui::vec2(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_widget_type_has_its_own_default_surface() {
        // Wider than upstream on purpose; `default_sizes_follow_strudel_canvas_defaults`
        // and `the_default_spiral_surface_fits_the_now_arc` in `tests.rs` carry the why.
        assert_eq!(default_surface_size("_spiral"), egui::vec2(400.0, 400.0));
        assert_eq!(
            default_surface_size("_pitchwheel"),
            egui::vec2(200.0, 200.0)
        );
        assert_eq!(default_surface_size("_spectrum"), egui::vec2(200.0, 200.0));
        // The two wide ones differ only in height, and only from the fallback.
        assert_eq!(default_surface_size("_wordfall"), egui::vec2(500.0, 120.0));
        assert_eq!(
            default_surface_size("_claviature"),
            egui::vec2(500.0, 100.0)
        );
        assert_eq!(default_surface_size("_pianoroll"), egui::vec2(500.0, 60.0));
    }
}
