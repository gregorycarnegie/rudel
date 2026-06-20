use super::options::{DrawWindow, VisualWidgetOptions};
use super::pianoroll::paint_pianoroll;
use super::pitchwheel::paint_pitchwheel;
use super::query::{hap_is_active, widget_haps};
use super::spiral::paint_spiral;
use super::style::WidgetDrawColors;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use rudel_core::Pattern;

pub(super) fn paint_pattern_widget(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    pattern: &Pattern,
    time_cycles: Option<f64>,
    colors: WidgetDrawColors,
) -> bool {
    let time = time_cycles.unwrap_or(0.0);
    let options = VisualWidgetOptions::from_widget(widget);
    match widget.widget_type.as_str() {
        "_pianoroll" | "_punchcard" => {
            let haps = widget_haps(pattern, widget, options.window(time));
            paint_pianoroll(ui, rect, &haps, time, colors, options);
            true
        }
        "_wordfall" => {
            let haps = widget_haps(pattern, widget, options.window(time));
            paint_pianoroll(
                ui,
                rect,
                &haps,
                time,
                colors,
                options.with_wordfall_defaults(widget),
            );
            true
        }
        "_pitchwheel" => {
            let haps = widget_haps(pattern, widget, DrawWindow::around(time))
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_pitchwheel(ui, rect, &haps, colors, options);
            true
        }
        "_spiral" => {
            let haps = widget_haps(pattern, widget, DrawWindow::around(time));
            paint_spiral(ui, rect, &haps, time, colors, options);
            true
        }
        _ => false,
    }
}
