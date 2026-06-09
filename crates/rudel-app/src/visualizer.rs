use eframe::egui;
use rudel_core::{Frac, Hap, Pattern, Value};
use std::collections::BTreeMap;

/// Draw one cycle per orbit as colored blocks, with an optional playhead at
/// `playhead` (0..1 within the cycle).
pub(crate) fn draw_visualizer(ui: &mut egui::Ui, pat: &Pattern, playhead: Option<f32>) {
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.retain(|h| h.whole.is_some());
    haps.sort_by_key(|h| h.part.begin);

    // Group by orbit (sorted).
    let mut orbits: BTreeMap<i64, Vec<&Hap>> = BTreeMap::new();
    for h in &haps {
        orbits.entry(orbit_of(&h.value)).or_default().push(h);
    }
    let band_count = orbits.len().max(1);

    let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));

    let pad = 4.0;
    let w = (rect.width() - 2.0 * pad).max(1.0);
    let band_h = ((rect.height() - 2.0 * pad) / band_count as f32).max(8.0);

    for (band_i, (orbit, band_haps)) in orbits.iter().enumerate() {
        let band_top = rect.top() + pad + band_i as f32 * band_h;
        draw_band(&painter, rect.left() + pad, band_top, w, band_h, band_haps);
        painter.text(
            egui::pos2(rect.left() + pad + 2.0, band_top + 2.0),
            egui::Align2::LEFT_TOP,
            format!("orbit {orbit}"),
            egui::FontId::monospace(10.0),
            egui::Color32::from_gray(120),
        );
    }

    if let Some(x) = playhead {
        let px = rect.left() + pad + x * w;
        painter.line_segment(
            [
                egui::pos2(px, rect.top() + pad),
                egui::pos2(px, rect.bottom() - pad),
            ],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(240, 240, 120)),
        );
    }
}

/// The `orbit` of a hap value (default 0), used to split the display into bands.
fn orbit_of(value: &Value) -> i64 {
    match value {
        Value::Map(m) => m.get("orbit").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64,
        _ => 0,
    }
}

/// Lane-pack and draw one orbit's haps within a horizontal band.
fn draw_band(painter: &egui::Painter, left: f32, top: f32, w: f32, band_h: f32, haps: &[&Hap]) {
    let mut lane_ends: Vec<f64> = Vec::new();
    let mut lanes: Vec<usize> = Vec::with_capacity(haps.len());
    for h in haps {
        let begin = h.part.begin.to_f64();
        let end = h.part.end.to_f64();
        let lane = match lane_ends.iter().position(|&e| e <= begin + 1e-9) {
            Some(l) => {
                lane_ends[l] = end;
                l
            }
            None => {
                lane_ends.push(end);
                lane_ends.len() - 1
            }
        };
        lanes.push(lane);
    }
    let lane_count = lane_ends.len().max(1);
    let lane_h = ((band_h - 2.0) / lane_count as f32).max(2.0);

    for (h, &lane) in haps.iter().zip(&lanes) {
        let begin = h.part.begin.to_f64() as f32;
        let end = h.part.end.to_f64() as f32;
        let x0 = left + begin * w;
        let x1 = left + end * w;
        let y0 = top + 1.0 + lane as f32 * lane_h;
        let block = egui::Rect::from_min_max(
            egui::pos2(x0 + 1.0, y0),
            egui::pos2((x1 - 1.0).max(x0 + 1.0), y0 + lane_h - 1.0),
        );
        let label = hap_label(&h.value);
        painter.rect_filled(block, 2.0, color_for(&label));
        if block.width() > 18.0 {
            painter.text(
                block.left_center() + egui::vec2(3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                truncate(&label, 16),
                egui::FontId::monospace(11.0),
                egui::Color32::from_gray(10),
            );
        }
    }
}

/// A concise label for a hap value (prefer the sound/note, else debug).
fn hap_label(value: &Value) -> String {
    match value {
        Value::Map(m) => {
            for k in ["s", "note", "n"] {
                if let Some(v) = m.get(k) {
                    return format!("{k}:{}", value_short(v));
                }
            }
            m.keys().next().cloned().unwrap_or_default()
        }
        other => value_short(other),
    }
}

fn value_short(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => format!("{x:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string(),
        other => format!("{other:?}"),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// Deterministic pastel color from a label.
fn color_for(label: &str) -> egui::Color32 {
    let mut h: u32 = 2166136261;
    for b in label.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32 / 360.0;
    let (r, g, b) = hsv_to_rgb(hue, 0.55, 0.92);
    egui::Color32::from_rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_short_formats_common_values() {
        assert_eq!(value_short(&Value::Str("bd".to_string())), "bd");
        assert_eq!(value_short(&Value::Int(42)), "42");
        assert_eq!(value_short(&Value::F64(1.2300)), "1.23");
        assert_eq!(value_short(&Value::F64(2.0)), "2");
    }

    #[test]
    fn hap_label_prefers_named_controls() {
        let with_sound = Value::Map(BTreeMap::from([
            ("note".to_string(), Value::Int(60)),
            ("s".to_string(), Value::Str("bd".to_string())),
        ]));
        let with_note = Value::Map(BTreeMap::from([("note".to_string(), Value::Int(64))]));

        assert_eq!(hap_label(&with_sound), "s:bd");
        assert_eq!(hap_label(&with_note), "note:64");
        assert_eq!(hap_label(&Value::Map(BTreeMap::new())), "");
    }

    #[test]
    fn orbit_defaults_to_zero_and_reads_map_control() {
        assert_eq!(orbit_of(&Value::Str("bd".to_string())), 0);
        assert_eq!(
            orbit_of(&Value::Map(BTreeMap::from([(
                "orbit".to_string(),
                Value::F64(2.9)
            )]))),
            2
        );
    }

    #[test]
    fn truncate_respects_character_boundaries() {
        assert_eq!(truncate("abcd", 4), "abcd");

        let shortened = truncate("abcdef", 4);
        assert_eq!(shortened.chars().count(), 5);
        assert!(shortened.ends_with('\u{2026}'));

        let unicode = truncate("\u{03b1}\u{03b2}\u{03b3}", 2);
        assert_eq!(unicode.chars().count(), 3);
        assert!(unicode.ends_with('\u{2026}'));
    }

    #[test]
    fn color_helpers_are_deterministic() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(color_for("bd"), color_for("bd"));
        assert_ne!(color_for("bd"), color_for("sd"));
    }
}
