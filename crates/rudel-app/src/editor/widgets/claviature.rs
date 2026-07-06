//! `_claviature` widget: a piano keyboard that lights up the notes sounding
//! at the playhead (Strudel's claviature visualizer).

use super::{
    options::VisualWidgetOptions,
    pitchwheel::hap_frequency,
    style::{WidgetDrawColors, color_with_alpha, event_alpha, event_color},
};
use eframe::egui;
use rudel_core::Hap;

/// Default key range, C2..C6.
const LOW_MIDI: i32 = 36;
const HIGH_MIDI: i32 = 84;

pub(super) fn paint_claviature(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[Hap],
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    // `lowest`/`highest` options (midi number or note name), snapped outward
    // to white keys so the keyboard has clean edges.
    let mut low = options.lowest.map(|m| m as i32).unwrap_or(LOW_MIDI);
    let mut high = options.highest.map(|m| m as i32).unwrap_or(HIGH_MIDI);
    if high <= low {
        (low, high) = (LOW_MIDI, HIGH_MIDI);
    }
    while is_black(low) {
        low -= 1;
    }
    while is_black(high) {
        high += 1;
    }
    let painter = ui.painter();
    let active: Vec<(i32, egui::Color32)> = haps
        .iter()
        .filter_map(|hap| {
            let midi = hap_midi(hap)?;
            let color = color_with_alpha(event_color(hap, colors.active), event_alpha(hap));
            Some((midi, color))
        })
        .collect();
    let active_color = |midi: i32| {
        active
            .iter()
            .find(|(m, _)| *m == midi)
            .map(|(_, color)| *color)
    };

    let rect = rect.shrink(2.0);
    let white_count = (low..=high).filter(|m| !is_black(*m)).count();
    let key_w = rect.width() / white_count as f32;
    let stroke = egui::Stroke::new(1.0, colors.inactive);

    let mut white_index = 0.0f32;
    let mut white_x = std::collections::HashMap::new();
    for midi in low..=high {
        if is_black(midi) {
            continue;
        }
        let x = rect.left() + white_index * key_w;
        white_x.insert(midi, x);
        let key =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(key_w, rect.height()));
        let fill = active_color(midi).unwrap_or(egui::Color32::WHITE);
        painter.rect_filled(key, 1.0, fill);
        painter.rect_stroke(key, 1.0, stroke, egui::StrokeKind::Inside);
        white_index += 1.0;
    }

    for midi in low..=high {
        if !is_black(midi) {
            continue;
        }
        // A black key sits across the boundary after its lower white neighbor.
        let Some(&x) = white_x.get(&(midi - 1)) else {
            continue;
        };
        let key = egui::Rect::from_min_size(
            egui::pos2(x + key_w * 0.65, rect.top()),
            egui::vec2(key_w * 0.7, rect.height() * 0.62),
        );
        let fill = active_color(midi).unwrap_or(egui::Color32::BLACK);
        painter.rect_filled(key, 1.0, fill);
        painter.rect_stroke(key, 1.0, stroke, egui::StrokeKind::Inside);
    }
}

pub(super) fn is_black(midi: i32) -> bool {
    matches!(midi.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

pub(super) fn hap_midi(hap: &Hap) -> Option<i32> {
    let freq = hap_frequency(hap)?;
    Some((69.0 + 12.0 * (freq / 440.0).log2()).round() as i32)
}
