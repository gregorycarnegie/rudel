//! `_scope` (= `tscope`), `_fscope` and `_spectrum` widgets: draw a widget's
//! analyzer tap ([`rudel_audio::ScopeTap`]) the way Strudel's `scope.mjs` /
//! `spectrum.mjs` draw a Web Audio `AnalyserNode` — a triggered oscilloscope
//! (with smear), frequency-domain bars, and a scrolling spectrogram.

use super::options::VisualWidgetOptions;
use eframe::egui;
use rudel_audio::ScopeTap;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

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
    let thickness = options.thickness;
    let draw = |samples: &[f32], color: egui::Color32| {
        // Strudel: first falling crossing of -trigger, fallback to 0.
        let start = if options.align {
            samples
                .windows(2)
                .position(|w| w[0] > -options.trigger && w[1] <= -options.trigger)
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };
        let pos = options.pos.unwrap_or(0.5);
        let scale = options.scale.unwrap_or(1.0);
        let slice = rect.width() / BUFFER_SIZE as f32;
        let points = samples[start..]
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                egui::pos2(
                    rect.left() + i as f32 * slice,
                    rect.top() + (pos - scale * s) * rect.height(),
                )
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(thickness, color),
        ));
    };

    for (age, trace) in traces.iter().enumerate().skip(1).rev() {
        let alpha = options.smear.powi(age as i32);
        draw(trace, super::style::color_with_alpha(color, alpha));
    }
    draw(&buf, color);
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
    let (min, max) = (options.min_db.unwrap_or(-150.0), options.max_db);
    let scale = options.scale.unwrap_or(0.25);
    let pos = options.pos.unwrap_or(0.75);
    let slice = rect.width() / BUFFER_SIZE as f32;
    for (i, &db) in db.iter().enumerate() {
        let normalized = ((db - min) / (max - min)).clamp(0.0, 1.0);
        let v = normalized * scale;
        let x = rect.left() + i as f32 * slice;
        let y = rect.top() + (pos - v * options.lean) * rect.height();
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(slice.max(1.0), v * rect.height()),
            ),
            0.0,
            color,
        );
    }
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
    let log_span = (BUFFER_SIZE as f32).ln();
    for (i, &db) in db.iter().enumerate() {
        let normalized = ((db - min) / (max - min)).clamp(0.0, 1.0);
        if normalized <= 0.0 {
            continue;
        }
        let alpha = (normalized * 255.0) as u8;
        let from_bottom = ((i + 1) as f32).ln() / log_span * h as f32;
        let y = (h as f32 - from_bottom).max(0.0) as usize;
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
    fft_in_place(&mut re, &mut im);

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

/// In-place iterative radix-2 complex FFT (enough for a 1024-point display;
/// no external FFT dependency in the app crate).
pub(super) fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in start..start + len / 2 {
                let (ur, ui) = (re[k], im[k]);
                let (sr, si) = (re[k + len / 2], im[k + len / 2]);
                let (vr, vi) = (sr * cr - si * ci, sr * ci + si * cr);
                re[k] = ur + vr;
                im[k] = ui + vi;
                re[k + len / 2] = ur - vr;
                im[k + len / 2] = ui - vi;
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}
