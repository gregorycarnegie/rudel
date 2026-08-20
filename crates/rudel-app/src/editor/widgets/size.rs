use super::options::option_f32;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;

pub(super) fn default_surface_size(widget_type: &str) -> egui::Vec2 {
    match widget_type {
        "_spiral" => egui::vec2(275.0, 275.0),
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
        assert_eq!(default_surface_size("_spiral"), egui::vec2(275.0, 275.0));
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
