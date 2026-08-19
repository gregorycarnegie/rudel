use super::{
    options::VisualWidgetOptions,
    style::{WidgetDrawColors, color_with_alpha, event_alpha, event_color},
};
use eframe::egui;
use rudel_core::{Hap, Value, value_to_midi};

pub(super) fn paint_pitchwheel(
    ui: &egui::Ui,
    rect: egui::Rect,
    haps: &[&Hap],
    colors: WidgetDrawColors,
    options: VisualWidgetOptions,
) {
    // Clip to the widget, the way a canvas clips: with `overscan` (and any
    // note that began before the window) a hap's geometry can extend past
    // the rect, and an unclipped painter would draw it over the editor.
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let size = rect.width().min(rect.height());
    let center = rect.center();
    let thickness = options.thickness;
    let hap_radius = options.hap_radius;
    let margin = options.margin;
    let radius = (size / 2.0 - thickness / 2.0 - hap_radius - margin).max(4.0);
    // `edoScale` tags each hap with the scale it came from, so the ring follows
    // the pattern instead of the widget default — upstream reads
    // `haps[0].value.{edo,root,degreeIndexes,intLabels}` the same way.
    let scale = haps
        .first()
        .map(|hap| rudel_core::to_control_map(&hap.value));
    let field = |key: &str| scale.as_ref().and_then(|m| m.get(key).cloned());
    let root = field("root")
        .and_then(|v| control_frequency(&v))
        .unwrap_or_else(|| rudel_core::midi_to_freq(36.0));
    let edo = field("edo")
        .and_then(|v| v.as_f64())
        .map(|edo| edo.round() as i64)
        .filter(|edo| *edo > 0)
        .unwrap_or(options.edo);
    let degree_indexes = field("degreeIndexes").and_then(|v| number_list(&v));
    let int_labels = field("intLabels").and_then(|v| string_list(&v));

    if options.circle {
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(thickness, colors.foreground),
        );
    }

    if edo > 0 {
        for i in 0..edo {
            let freq = root * 2f64.powf(i as f64 / edo as f64);
            let angle = freq_to_angle(freq, root);
            let pos = pitchwheel_pos(center, radius, angle);
            // Without a scale every degree is drawn alike; with one, degrees
            // outside it fade back to upstream's 0.15.
            let alpha = match &degree_indexes {
                Some(degrees) if degrees.contains(&i) => 1.0,
                Some(_) => 0.15,
                None => 0.45,
            };
            painter.circle_filled(
                pos,
                hap_radius * 0.45,
                color_with_alpha(colors.muted, alpha),
            );

            // Interval label for this degree, offset off the wheel the way
            // upstream's angle bands place it.
            let Some(label) = degree_label(&degree_indexes, &int_labels, i) else {
                continue;
            };
            let (offset, align) = if angle < 0.32 && angle > 0.125 {
                (egui::vec2(-10.0, 0.0), egui::Align2::RIGHT_CENTER)
            } else if angle < 0.1 && angle > -1.125 {
                (egui::vec2(0.0, -12.0), egui::Align2::CENTER_BOTTOM)
            } else {
                (egui::vec2(9.0, 0.0), egui::Align2::LEFT_CENTER)
            };
            painter.text(
                pos + offset,
                align,
                label,
                egui::FontId::monospace(11.0),
                colors.muted,
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
        let color = color_with_alpha(event_color(hap, colors.foreground), event_alpha(hap));
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
            egui::Stroke::new(hap_radius, colors.foreground),
        ));
    }
}

/// `edoScale` writes `root` as a formatted frequency string, so accept either
/// a number or a parseable string.
fn control_frequency(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|freq| *freq > 0.0)
}

fn number_list(value: &Value) -> Option<Vec<i64>> {
    match value {
        Value::List(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_f64().map(|n| n.round() as i64))
                .collect(),
        ),
        _ => None,
    }
}

fn string_list(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::List(items) => Some(
            items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.as_f64().map(|n| n.to_string()).unwrap_or_default())
                })
                .collect(),
        ),
        _ => None,
    }
}

/// The interval label for ring degree `i`, if the scale names one. Upstream
/// indexes `intLabels` by the degree's *position* in `degreeIndexes`.
pub(super) fn degree_label(
    degree_indexes: &Option<Vec<i64>>,
    int_labels: &Option<Vec<String>>,
    degree: i64,
) -> Option<String> {
    let position = degree_indexes.as_ref()?.iter().position(|d| *d == degree)?;
    int_labels
        .as_ref()?
        .get(position)
        .filter(|label| !label.is_empty())
        .cloned()
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
