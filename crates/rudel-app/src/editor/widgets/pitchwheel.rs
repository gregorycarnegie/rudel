use super::{
    options::VisualWidgetOptions,
    style::{WidgetDrawColors, color_with_alpha, event_alpha, event_color},
    values::value_to_midi,
};
use eframe::egui;
use rudel_core::{Hap, Value};

pub(super) fn paint_pitchwheel(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[Hap],
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    let painter = ui.painter();
    let size = rect.width().min(rect.height());
    let center = rect.center();
    let thickness = options.thickness;
    let hap_radius = options.hap_radius;
    let margin = options.margin;
    let radius = (size / 2.0 - thickness / 2.0 - hap_radius - margin).max(4.0);
    let root = rudel_core::midi_to_freq(36.0);
    let edo = options.edo;

    if options.circle {
        painter.circle_stroke(center, radius, egui::Stroke::new(thickness, colors.active));
    }

    if edo > 0 {
        for i in 0..edo {
            let freq = root * 2f64.powf(i as f64 / edo as f64);
            let pos = pitchwheel_pos(center, radius, freq_to_angle(freq, root));
            painter.circle_filled(
                pos,
                hap_radius * 0.45,
                color_with_alpha(colors.inactive, 0.45),
            );
        }
    }

    if edo > 0 {
        painter.text(
            rect.right_bottom() - egui::vec2(8.0, 8.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{edo} EDO"),
            egui::FontId::monospace(11.0),
            colors.muted,
        );
    }

    let mut shape = Vec::new();
    for hap in haps {
        let Some(freq) = hap_frequency(hap) else {
            continue;
        };
        let angle = freq_to_angle(freq, root);
        let pos = pitchwheel_pos(center, radius, angle);
        let color = color_with_alpha(event_color(hap, colors.active), event_alpha(hap));
        shape.push((pos, angle, color));
        if !options.polygon {
            painter.line_segment([center, pos], egui::Stroke::new(1.0, color));
        }
        if options.hapcircles {
            painter.circle_filled(pos, hap_radius, color);
        }
    }

    if options.polygon && shape.len() > 1 {
        shape.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let points = shape.iter().map(|(pos, _, _)| *pos).collect::<Vec<_>>();
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(hap_radius, colors.active),
        ));
    }
}

pub(super) fn hap_frequency(hap: &Hap) -> Option<f64> {
    let mut controls = rudel_core::to_control_map(&hap.value);
    rudel_core::tonal::apply_transpose_controls(&mut controls, hap.context.scale.as_deref());
    if let Some(freq) = controls.get("freq").and_then(Value::as_f64) {
        return Some(freq);
    }
    controls
        .get("note")
        .or_else(|| controls.get("n"))
        .and_then(value_to_midi)
        .map(rudel_core::midi_to_freq)
}

pub(super) fn freq_to_angle(freq: f64, root: f64) -> f32 {
    let octaves = (freq / root).log2();
    let js_remainder = octaves - octaves.trunc();
    (0.5 - js_remainder) as f32
}

fn pitchwheel_pos(center: egui::Pos2, radius: f32, angle: f32) -> egui::Pos2 {
    let radians = angle * std::f32::consts::TAU;
    egui::pos2(
        center.x + radians.sin() * radius,
        center.y + radians.cos() * radius,
    )
}
