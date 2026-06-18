use super::style::parse_hex_color;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use std::collections::BTreeMap;

pub(super) const DRAW_LOOKBEHIND: f64 = -2.0;
const DRAW_LOOKAHEAD: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DrawWindow {
    pub(super) begin: f64,
    pub(super) end: f64,
}

impl DrawWindow {
    pub(super) fn around(time: f64) -> Self {
        Self {
            begin: time + DRAW_LOOKBEHIND,
            end: time + DRAW_LOOKAHEAD,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VisualWidgetOptions {
    pub(super) cycles: f64,
    pub(super) playhead: f64,
    pub(super) vertical: bool,
    pub(super) labels: bool,
    pub(super) flip_time: bool,
    pub(super) flip_values: bool,
    pub(super) fold: bool,
    pub(super) hide_inactive: bool,
    pub(super) hide_negative: bool,
    pub(super) fill: bool,
    pub(super) fill_active: bool,
    pub(super) stroke: Option<bool>,
    pub(super) stroke_active: bool,
    pub(super) colorize_inactive: bool,
    pub(super) min_midi: f64,
    pub(super) max_midi: f64,
    pub(super) autorange: bool,
    pub(super) circle: bool,
    pub(super) hapcircles: bool,
    pub(super) edo: i64,
    pub(super) thickness: f32,
    pub(super) hap_radius: f32,
    pub(super) margin: f32,
    pub(super) polygon: bool,
    pub(super) stretch: f32,
    pub(super) spiral_size: f32,
    pub(super) spiral_thickness: Option<f32>,
    pub(super) inset: f32,
    pub(super) playhead_length: f32,
    pub(super) playhead_thickness: Option<f32>,
    pub(super) padding: f32,
    pub(super) steady: f32,
    pub(super) colorize_spiral_inactive: bool,
    pub(super) fade: bool,
    pub(super) active_color: Option<egui::Color32>,
    pub(super) inactive_color: Option<egui::Color32>,
    pub(super) playhead_color: Option<egui::Color32>,
}

impl VisualWidgetOptions {
    pub(super) fn from_widget(widget: &WidgetDecoration) -> Self {
        let options = &widget.options;
        let spiral_size = if widget.widget_type == "_spiral" {
            option_f32(options, "size")
                .map(|size| size / 5.0)
                .unwrap_or(55.0)
        } else {
            option_f32(options, "size").unwrap_or(80.0)
        }
        .max(0.001);
        Self {
            cycles: option_f64(options, "cycles").unwrap_or(4.0).max(0.001),
            playhead: option_f64(options, "playhead")
                .unwrap_or(0.5)
                .clamp(0.0, 1.0),
            vertical: option_bool(options, "vertical").unwrap_or(false),
            labels: option_bool(options, "labels").unwrap_or(false),
            flip_time: option_bool(options, "flipTime").unwrap_or(false),
            flip_values: option_bool(options, "flipValues").unwrap_or(false),
            fold: option_bool(options, "fold").unwrap_or(true),
            hide_inactive: option_bool(options, "hideInactive").unwrap_or(false),
            hide_negative: option_bool(options, "hideNegative").unwrap_or(false),
            fill: option_bool(options, "fill").unwrap_or(true),
            fill_active: option_bool(options, "fillActive").unwrap_or(false),
            stroke: option_bool(options, "stroke"),
            stroke_active: option_bool(options, "strokeActive").unwrap_or(true),
            colorize_inactive: option_bool(options, "colorizeInactive").unwrap_or(true),
            min_midi: option_f64(options, "minMidi").unwrap_or(10.0),
            max_midi: option_f64(options, "maxMidi").unwrap_or(90.0),
            autorange: option_bool(options, "autorange").unwrap_or(false),
            circle: option_bool(options, "circle").unwrap_or(false),
            hapcircles: option_bool(options, "hapcircles").unwrap_or(true),
            edo: option_f64(options, "edo").unwrap_or(12.0).round().max(0.0) as i64,
            thickness: option_f32(options, "thickness").unwrap_or(3.0),
            hap_radius: option_f32(options, "hapRadius").unwrap_or(6.0),
            margin: option_f32(options, "margin").unwrap_or(10.0),
            polygon: option_str(options, "mode") == Some("polygon"),
            stretch: option_f32(options, "stretch").unwrap_or(1.0).max(0.001),
            spiral_size,
            spiral_thickness: option_f32(options, "thickness"),
            inset: option_f32(options, "inset").unwrap_or(3.0),
            playhead_length: option_f32(options, "playheadLength").unwrap_or(0.02),
            playhead_thickness: option_f32(options, "playheadThickness"),
            padding: option_f32(options, "padding").unwrap_or(0.0),
            steady: option_f32(options, "steady").unwrap_or(1.0),
            colorize_spiral_inactive: option_bool(options, "colorizeInactive").unwrap_or(false),
            fade: option_bool(options, "fade").unwrap_or(true),
            active_color: option_color(options, "active")
                .or_else(|| option_color(options, "activeColor")),
            inactive_color: option_color(options, "inactive")
                .or_else(|| option_color(options, "inactiveColor")),
            playhead_color: option_color(options, "playheadColor"),
        }
    }

    pub(super) fn with_wordfall_defaults(mut self, widget: &WidgetDecoration) -> Self {
        if !widget.options.contains_key("vertical") {
            self.vertical = true;
        }
        if !widget.options.contains_key("labels") {
            self.labels = true;
        }
        if !widget.options.contains_key("stroke") {
            self.stroke = Some(false);
        }
        if !widget.options.contains_key("fillActive") {
            self.fill_active = true;
        }
        self
    }

    pub(super) fn window(self, time: f64) -> DrawWindow {
        let from = -self.cycles * self.playhead;
        let to = self.cycles * (1.0 - self.playhead);
        DrawWindow {
            begin: time + from,
            end: time + to,
        }
    }
}

fn option_bool(options: &BTreeMap<String, rudel_lang::WidgetOption>, key: &str) -> Option<bool> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_bool)
}

fn option_f64(options: &BTreeMap<String, rudel_lang::WidgetOption>, key: &str) -> Option<f64> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_f64)
}

pub(super) fn option_f32(
    options: &BTreeMap<String, rudel_lang::WidgetOption>,
    key: &str,
) -> Option<f32> {
    option_f64(options, key).map(|value| value as f32)
}

fn option_str<'a>(
    options: &'a BTreeMap<String, rudel_lang::WidgetOption>,
    key: &str,
) -> Option<&'a str> {
    options.get(key).and_then(rudel_lang::WidgetOption::as_str)
}

fn option_color(
    options: &BTreeMap<String, rudel_lang::WidgetOption>,
    key: &str,
) -> Option<egui::Color32> {
    option_str(options, key).and_then(parse_hex_color)
}
