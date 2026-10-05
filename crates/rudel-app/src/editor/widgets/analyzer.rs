//! `_scope` (= `tscope`), `_fscope` and `_spectrum` widgets: draw a widget's
//! analyzer tap ([`rudel_audio::ScopeTap`]) the way Strudel's `scope.mjs` /
//! `spectrum.mjs` draw a Web Audio `AnalyserNode` — a triggered oscilloscope
//! (with smear), frequency-domain bars, and a scrolling spectrogram.

use super::options::VisualWidgetOptions;
use eframe::egui;
use rudel_audio::{Fft, ScopeTap};
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, Mutex},
};

/// The display transform. One fixed size, so the bit-reversal table and
/// twiddles are built once instead of per repaint.
static FFT: LazyLock<Fft> = LazyLock::new(|| Fft::new(FFT_SIZE));

/// Displayed samples / frequency bins — the `frequencyBinCount` of Strudel's
/// default analyser (`fftSize` 1024).
const BUFFER_SIZE: usize = 512;
const FFT_SIZE: usize = 1024;
/// `AnalyserNode.smoothingTimeConstant` in superdough's `getAnalyserById`.
const SMOOTHING: f32 = 0.5;
/// Most faded smear trace still drawn.
const MAX_SMEAR_TRACES: usize = 12;

/// Oscilloscope (Strudel `drawTimeScope` with the inline `_scope` defaults
/// `pos: 0.5, scale: 1`): falling-edge trigger alignment, y = (pos - scale*s)·h.
pub(super) fn paint_scope(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget_id: &str,
    tap: Option<&ScopeTap>,
    options: VisualWidgetOptions,
    color: egui::Color32,
) {
    let mut buf = vec![0.0f32; BUFFER_SIZE];
    if let Some(tap) = tap {
        tap.latest(&mut buf);
    }

    // Smear: keep recent traces and draw them fading out (Strudel overpaints
    // the old canvas with alpha 1-smear; trace age a gets alpha smear^a).
    let traces: Arc<Mutex<VecDeque<Vec<f32>>>> = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<VecDeque<Vec<f32>>>>>(egui::Id::new((
            "rudel-scope-smear",
            widget_id,
        )))
        .clone()
    });
    let mut traces = traces.lock().unwrap();
    if options.smear > 0.0 {
        traces.push_front(buf.clone());
        traces.truncate(MAX_SMEAR_TRACES);
    } else if !traces.is_empty() {
        traces.clear();
    }

    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let draw = |samples: &[f32], color: egui::Color32| {
        painter.add(egui::Shape::line(
            scope_points(samples, rect, &options),
            egui::Stroke::new(options.thickness, color),
        ));
    };

    for (age, trace) in traces.iter().enumerate().skip(1).rev() {
        let alpha = options.smear.powi(age as i32);
        draw(trace, super::style::color_with_alpha(color, alpha));
    }
    draw(&buf, color);
}

/// The oscilloscope trace (`drawTimeScope`): from the first falling crossing
/// of `-trigger` when aligned (else the start), one point per sample across
/// the width, at `(pos - scale * sample) * height`.
fn scope_points(
    samples: &[f32],
    rect: egui::Rect,
    options: &VisualWidgetOptions,
) -> Vec<egui::Pos2> {
    let start = if options.align {
        samples
            .windows(2)
            .position(|w| w[0] > -options.trigger && w[1] <= -options.trigger)
            .map_or(0, |i| i + 1)
    } else {
        0
    };
    let pos = options.pos.unwrap_or(0.5);
    let scale = options.scale.unwrap_or(1.0);
    let slice = rect.width() / samples.len().max(1) as f32;
    samples[start..]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            egui::pos2(
                rect.left() + i as f32 * slice,
                rect.top() + (pos - scale * s) * rect.height(),
            )
        })
        .collect()
}

/// Frequency-domain bars (Strudel `drawFrequencyScope`): one bar per bin on a
/// linear axis, height/anchor from `scale`/`pos`/`lean`, dB range `min..max`.
pub(super) fn paint_fscope(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget_id: &str,
    tap: Option<&ScopeTap>,
    options: VisualWidgetOptions,
    color: egui::Color32,
) {
    let db = frequency_data(ui, widget_id, tap);
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    for bar in fscope_bars(&db, rect, &options) {
        painter.rect_filled(bar, 0.0, color);
    }
}

/// One bar per bin (`drawFrequencyScope`): `min..max` dB normalized to 0..1
/// and scaled by `scale`, standing `lean` of its height above `pos`.
fn fscope_bars(db: &[f32], rect: egui::Rect, options: &VisualWidgetOptions) -> Vec<egui::Rect> {
    let (min, max) = (options.min_db.unwrap_or(-150.0), options.max_db);
    let scale = options.scale.unwrap_or(0.25);
    let pos = options.pos.unwrap_or(0.75);
    let slice = rect.width() / db.len().max(1) as f32;
    db.iter()
        .enumerate()
        .map(|(i, &db)| {
            let v = ((db - min) / (max - min)).clamp(0.0, 1.0) * scale;
            egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + i as f32 * slice,
                    rect.top() + (pos - v * options.lean) * rect.height(),
                ),
                egui::vec2(slice.max(1.0), v * rect.height()),
            )
        })
        .collect()
}

/// Per-widget scrolling-spectrogram state (Strudel keeps the previous canvas
/// frame per analyser id; we keep an image we shift left each frame).
#[derive(Default)]
pub(super) struct SpectrumState {
    image: Option<egui::ColorImage>,
    tex: Option<egui::TextureHandle>,
    /// Color of the last frame that had an active hap (Strudel's
    /// `latestColor[id]`), so trails keep their color between events.
    last_color: Option<egui::Color32>,
}

/// Scrolling spectrogram (Strudel `drawSpectrum`): the image scrolls left by
/// `speed` px per frame; the new right-hand column plots each bin at a
/// log-frequency height with alpha = normalized dB.
pub(super) fn paint_spectrum(
    ui: &egui::Ui,
    rect: egui::Rect,
    widget_id: &str,
    tap: Option<&ScopeTap>,
    options: VisualWidgetOptions,
    hap_color: Option<egui::Color32>,
    theme_color: egui::Color32,
) {
    let db = frequency_data(ui, widget_id, tap);
    let state: Arc<Mutex<SpectrumState>> = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<SpectrumState>>>(egui::Id::new((
            "rudel-spectrum",
            widget_id,
        )))
        .clone()
    });
    let mut state = state.lock().unwrap();
    if let Some(color) = hap_color {
        state.last_color = Some(color);
    }
    let color = hap_color.or(state.last_color).unwrap_or(theme_color);

    let (w, h) = (
        (rect.width().round() as usize).clamp(1, 2048),
        (rect.height().round() as usize).clamp(1, 2048),
    );
    if state.image.as_ref().map(|i| i.size) != Some([w, h]) {
        state.image = Some(egui::ColorImage::filled([w, h], egui::Color32::TRANSPARENT));
        state.tex = None;
    }
    let image = state.image.as_mut().unwrap();

    // Scroll left by `speed` and clear the incoming columns.
    let speed = (options.speed.round() as usize).clamp(1, w);
    for row in image.pixels.chunks_mut(w) {
        row.copy_within(speed.., 0);
        row[w - speed..].fill(egui::Color32::TRANSPARENT);
    }

    let (min, max) = (options.min_db.unwrap_or(-80.0), options.max_db);
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    for (i, &db) in db.iter().enumerate() {
        let Some((y, alpha)) = spectrum_cell(i, db, h, min, max) else {
            continue;
        };
        for row in y..(y + 2).min(h) {
            let px = &mut image.pixels[row * w + w - speed..row * w + w];
            for p in px {
                if p.a() < alpha {
                    *p = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);
                }
            }
        }
    }

    let image_clone = image.clone();
    let tex = match &mut state.tex {
        Some(tex) => {
            tex.set(image_clone, egui::TextureOptions::NEAREST);
            tex.clone()
        }
        none => {
            let tex = ui.ctx().load_texture(
                format!("rudel-spectrum-{widget_id}"),
                image_clone,
                egui::TextureOptions::NEAREST,
            );
            *none = Some(tex.clone());
            tex
        }
    };
    ui.painter_at(rect.intersect(ui.clip_rect())).image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

/// Where bin `i` lights the spectrogram's new column (`drawSpectrum`): the top
/// row of its 2-pixel cell, at log-frequency height `ln(i+1) / ln(bins)` of
/// `h` from the bottom, and its alpha from `min..max` dB; `None` when silent.
fn spectrum_cell(i: usize, db: f32, h: usize, min: f32, max: f32) -> Option<(usize, u8)> {
    let normalized = ((db - min) / (max - min)).clamp(0.0, 1.0);
    if normalized <= 0.0 {
        return None;
    }
    let from_bottom = ((i + 1) as f32).ln() / (BUFFER_SIZE as f32).ln() * h as f32;
    Some((
        (h as f32 - from_bottom).max(0.0) as usize,
        (normalized * 255.0) as u8,
    ))
}

/// The analyser's frequency data in dB: Hann-windowed FFT magnitudes smoothed
/// across frames like `AnalyserNode.getFloatFrequencyData` (τ = 0.5), with
/// per-widget smoothing state.
fn frequency_data(ui: &egui::Ui, widget_id: &str, tap: Option<&ScopeTap>) -> Vec<f32> {
    let mut re = vec![0.0f32; FFT_SIZE];
    let mut im = vec![0.0f32; FFT_SIZE];
    if let Some(tap) = tap {
        tap.latest(&mut re);
    }
    for (i, s) in re.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos();
        *s *= w;
    }
    FFT.forward(&mut re, &mut im);

    let smoothed: Arc<Mutex<Vec<f32>>> = ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<Vec<f32>>>>(egui::Id::new((
            "rudel-analyzer-smooth",
            widget_id,
        )))
        .clone()
    });
    let mut smoothed = smoothed.lock().unwrap();
    smoothed.resize(BUFFER_SIZE, 0.0);
    (0..BUFFER_SIZE)
        .map(|k| {
            // Normalized so a full-scale sine peaks near 0 dB (Hann coherent
            // gain 0.5 → |X| = N/4).
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() / (FFT_SIZE as f32 / 4.0);
            let s = SMOOTHING * smoothed[k] + (1.0 - SMOOTHING) * mag;
            smoothed[k] = s;
            20.0 * s.max(1e-10).log10()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(set: impl FnOnce(&mut VisualWidgetOptions)) -> VisualWidgetOptions {
        use crate::editor::decorations::{SourceRange, WidgetDecoration};
        let mut options = VisualWidgetOptions::from_widget(&WidgetDecoration {
            widget_type: "_scope".to_string(),
            id: "scope".to_string(),
            range: SourceRange::new(0, 1),
            index: 0,
            options: Default::default(),
        });
        set(&mut options);
        options
    }

    const RECT: egui::Rect = egui::Rect {
        min: egui::pos2(10.0, 20.0),
        max: egui::pos2(110.0, 220.0),
    };

    #[test]
    fn the_scope_starts_at_the_falling_trigger_crossing() {
        let samples = [0.5, 0.2, -0.1, -0.3, 0.4];
        let aligned = scope_points(&samples, RECT, &options(|o| o.align = true));
        // The fall through 0 completes at index 2, so the trace starts there.
        assert_eq!(aligned.len(), 3);
        assert_eq!(aligned[0], egui::pos2(10.0, 20.0 + (0.5 + 0.1) * 200.0));
        assert_eq!(aligned[1].x, 10.0 + 100.0 / 5.0);

        let free = scope_points(&samples, RECT, &options(|o| o.align = false));
        assert_eq!(free.len(), 5);
        assert_eq!(free[0].y, 20.0);

        let no_crossing = scope_points(&[0.5, 0.6], RECT, &options(|o| o.align = true));
        assert_eq!(no_crossing.len(), 2, "falls back to the start");
        let scaled = scope_points(
            &[0.5],
            RECT,
            &options(|o| {
                o.pos = Some(0.75);
                o.scale = Some(0.5);
            }),
        );
        assert_eq!(scaled[0].y, 20.0 + (0.75 - 0.5 * 0.5) * 200.0);
    }

    #[test]
    fn frequency_bars_rise_from_pos_by_lean_of_their_height() {
        let o = options(|o| {
            o.min_db = Some(-100.0);
            o.max_db = 0.0;
            o.scale = Some(0.5);
            o.pos = Some(0.75);
            o.lean = 0.5;
        });
        let bars = fscope_bars(&[-200.0, -50.0, 10.0, 0.0], RECT, &o);
        assert_eq!(bars[0].height(), 0.0, "below min is silent");
        // -50 dB of -100..0 is 0.5, times scale 0.5: a quarter of the height.
        assert_eq!(bars[1].height(), 50.0);
        assert_eq!(bars[1].top(), 20.0 + (0.75 - 0.25 * 0.5) * 200.0);
        assert_eq!(bars[1].left(), 10.0 + 25.0);
        assert_eq!(
            (bars[2].top(), bars[2].height()),
            (bars[3].top(), bars[3].height()),
            "above max clamps to max"
        );
        assert_eq!(bars[3].height(), 100.0);
    }

    #[test]
    fn spectrogram_bins_sit_at_log_frequency_height() {
        let h = 100;
        assert_eq!(spectrum_cell(0, -10.0, h, -80.0, 0.0), Some((100, 223)));
        assert_eq!(
            spectrum_cell(BUFFER_SIZE - 1, 0.0, h, -80.0, 0.0),
            Some((0, 255))
        );
        let (row, _) = spectrum_cell(21, -40.0, h, -80.0, 0.0).unwrap();
        let expected = 100.0 - 22f32.ln() / (BUFFER_SIZE as f32).ln() * 100.0;
        assert_eq!(row, expected as usize);
        assert_eq!(spectrum_cell(5, -90.0, h, -80.0, 0.0), None, "silent");
    }
}
