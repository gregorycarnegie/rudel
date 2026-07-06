use super::{
    analyzer::{paint_fscope, paint_scope, paint_spectrum},
    claviature::paint_claviature,
    options::{DrawWindow, VisualWidgetOptions},
    pianoroll::paint_pianoroll,
    pitchwheel::paint_pitchwheel,
    query::{hap_is_active, widget_haps},
    spiral::paint_spiral,
    style::{WidgetDrawColors, event_color},
};
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use rudel_audio::ScopeTaps;
use rudel_core::Pattern;

pub(super) fn paint_pattern_widget(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget: &WidgetDecoration,
    pattern: &Pattern,
    time_cycles: Option<f64>,
    colors: WidgetDrawColors,
    taps: Option<&ScopeTaps>,
) -> bool {
    let time = time_cycles.unwrap_or(0.0);
    let options = VisualWidgetOptions::from_widget(widget);
    // The audio ring feeding an analyzer widget: the tap registered under this
    // widget's id, filled by the voices whose haps carry the widget tag.
    let widget_tap = || taps.map(|taps| taps.get_or_create(&widget.id));
    // Strudel's tscope/spectrum color the trace by the active hap's `color`
    // control, falling back to the theme foreground.
    let hap_color = |fallback: Option<egui::Color32>| {
        widget_haps(pattern, widget, DrawWindow::around(time))
            .iter()
            .find(|hap| hap_is_active(hap, time))
            .map(|hap| event_color(hap, colors.active))
            .or(fallback)
    };
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
        "_claviature" => {
            let haps = widget_haps(pattern, widget, DrawWindow::around(time))
                .into_iter()
                .filter(|hap| hap_is_active(hap, time))
                .collect::<Vec<_>>();
            paint_claviature(ui, rect, &haps, colors, options);
            true
        }
        "_scope" => {
            let color = options
                .active_color
                .or_else(|| hap_color(None))
                .unwrap_or(colors.active);
            paint_scope(ui, rect, &widget.id, widget_tap().as_deref(), options, color);
            true
        }
        "_fscope" => {
            let color = options.active_color.unwrap_or(colors.active);
            paint_fscope(ui, rect, &widget.id, widget_tap().as_deref(), options, color);
            true
        }
        "_spectrum" => {
            let hap_color = options.active_color.or_else(|| hap_color(None));
            paint_spectrum(
                ui,
                rect,
                &widget.id,
                widget_tap().as_deref(),
                options,
                hap_color,
                colors.active,
            );
            true
        }
        _ => false,
    }
}
