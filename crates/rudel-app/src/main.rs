// rudel-app - native live-coding editor for Rudel.
// Type Koto in the editor, Ctrl+Enter to evaluate and hot-swap the pattern into
// the running output (audio, MIDI or OSC). The right panel visualizes one cycle
// per orbit with a live playhead; a reference pane lists sounds and controls.
// SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui;
use rudel_audio::Engine;
use rudel_core::{Frac, Hap, Pattern, Value};
use rudel_midi::{MidiEngine, MidiOut};
use rudel_osc::{OscEngine, OscOut};
use std::collections::BTreeMap;

const DEFAULT_CODE: &str = r#"stack(
  note("c4 e4 g4 b4 a4 g4 e4 d4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").cutoff("400 1600").gain(0.6).delay(0.3)
)"#;

/// Built-in synth waveforms (always available as `s(...)`).
const WAVEFORMS: &[&str] = &["sine", "saw", "square", "triangle"];

/// Control names exposed by the engine, for the reference pane.
const CONTROLS: &[&str] = &[
    "note",
    "n",
    "s",
    "gain",
    "pan",
    "speed",
    "cutoff",
    "resonance",
    "room",
    "size",
    "shape",
    "crush",
    "delay",
    "delaytime",
    "delayfeedback",
    "attack",
    "decay",
    "sustain",
    "release",
    "vowel",
    "accelerate",
    "coarse",
    "orbit",
    "velocity",
    "begin",
    "end",
    "legato",
    "clip",
    "unit",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Audio,
    Midi,
    Osc,
}

struct RudelApp {
    engine: Option<Engine>,
    audio_error: Option<String>,
    code: String,
    eval_error: Option<String>,
    status: String,
    cps: f64,
    playing: bool,
    current: Option<Pattern>,

    // Sample loading.
    sample_dir: String,
    sample_names: Vec<String>,

    // Output routing.
    output: Output,
    midi_port: String,
    osc_target: String,
    midi: Option<MidiEngine>,
    osc: Option<OscEngine>,
    io_error: Option<String>,
}

impl RudelApp {
    fn new() -> RudelApp {
        rudel_mini::install();
        let (engine, audio_error) = match Engine::new() {
            Ok(e) => {
                e.set_cps(0.5);
                (Some(e), None)
            }
            Err(e) => (None, Some(e)),
        };
        RudelApp {
            engine,
            audio_error,
            code: DEFAULT_CODE.to_string(),
            eval_error: None,
            status: "ready".to_string(),
            cps: 0.5,
            playing: false,
            current: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            output: Output::Audio,
            midi_port: String::new(),
            osc_target: "127.0.0.1:57120".to_string(),
            midi: None,
            osc: None,
            io_error: None,
        }
    }

    /// Evaluate the editor contents and route the result to the active output.
    fn evaluate(&mut self) {
        match rudel_lang::eval(&self.code) {
            Ok(pat) => {
                self.current = Some(pat);
                self.eval_error = None;
                self.status = "evaluated".to_string();
                self.route();
            }
            Err(e) => {
                self.eval_error = Some(e);
                self.status = "error".to_string();
            }
        }
    }

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.route();
    }

    fn set_cps(&mut self, cps: f64) {
        self.cps = cps;
        if let Some(e) = &self.engine {
            e.set_cps(cps);
        }
        if let Some(m) = &self.midi {
            m.set_cps(cps);
        }
        if let Some(o) = &self.osc {
            o.set_cps(cps);
        }
    }

    /// Push the current pattern (or silence) to the active output, and silence
    /// to the others, lazily connecting MIDI/OSC as needed.
    fn route(&mut self) {
        let active = if self.playing {
            self.current.clone().unwrap_or_else(rudel_core::silence)
        } else {
            rudel_core::silence()
        };
        if self.output == Output::Midi && self.playing {
            self.ensure_midi();
        }
        if self.output == Output::Osc && self.playing {
            self.ensure_osc();
        }
        let silence = rudel_core::silence();
        if let Some(e) = &self.engine {
            e.set_pattern(if self.output == Output::Audio {
                active.clone()
            } else {
                silence.clone()
            });
        }
        if let Some(m) = &self.midi {
            m.set_pattern(if self.output == Output::Midi {
                active.clone()
            } else {
                silence.clone()
            });
        }
        if let Some(o) = &self.osc {
            o.set_pattern(if self.output == Output::Osc {
                active.clone()
            } else {
                silence.clone()
            });
        }
    }

    fn ensure_midi(&mut self) {
        if self.midi.is_some() {
            return;
        }
        let port = {
            let p = self.midi_port.trim();
            if p.is_empty() { None } else { Some(p) }
        };
        match MidiOut::connect(port) {
            Ok(out) => {
                let pat = self.current.clone().unwrap_or_else(rudel_core::silence);
                self.midi = Some(MidiEngine::start(out, pat, self.cps));
                self.io_error = None;
            }
            Err(e) => {
                self.io_error = Some(format!("MIDI: {e}"));
                self.output = Output::Audio;
            }
        }
    }

    fn ensure_osc(&mut self) {
        if self.osc.is_some() {
            return;
        }
        match OscOut::connect(self.osc_target.trim()) {
            Ok(out) => {
                let pat = self.current.clone().unwrap_or_else(rudel_core::silence);
                self.osc = Some(OscEngine::start(out, pat, self.cps));
                self.io_error = None;
            }
            Err(e) => {
                self.io_error = Some(format!("OSC: {e}"));
                self.output = Output::Audio;
            }
        }
    }

    fn load_samples(&mut self) {
        let Some(engine) = &self.engine else {
            self.io_error = Some("no audio engine to load samples into".to_string());
            return;
        };
        match engine.load_samples(self.sample_dir.trim()) {
            Ok(n) => {
                self.sample_names = engine.sample_names();
                self.status = format!("loaded {n} samples ({} sounds)", self.sample_names.len());
                self.io_error = None;
            }
            Err(e) => self.io_error = Some(format!("samples: {e}")),
        }
    }
}

impl eframe::App for RudelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let eval_shortcut = ui
            .ctx()
            .input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
        if eval_shortcut {
            self.evaluate();
        }

        self.transport_panel(ui);
        self.errors_panel(ui);
        self.reference_panel(ui);
        self.editor_panel(ui);

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label("pattern (one cycle per orbit)");
            let playhead = if self.playing {
                self.engine
                    .as_ref()
                    .map(|e| e.position_cycles().rem_euclid(1.0) as f32)
            } else {
                None
            };
            match &self.current {
                Some(pat) => draw_visualizer(ui, pat, playhead),
                None => {
                    ui.weak("evaluate a pattern to see it here");
                }
            }
        });

        // Keep the playhead moving while playing.
        if self.playing {
            ui.ctx().request_repaint();
        }
    }
}

impl RudelApp {
    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("transport").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("rudel");
                ui.separator();
                let label = if self.playing { "⏹ Stop" } else { "▶ Play" };
                if ui.button(label).clicked() {
                    let now = !self.playing;
                    if now && self.current.is_none() {
                        self.evaluate();
                    }
                    self.set_playing(now);
                }
                if ui.button("Eval (Ctrl+Enter)").clicked() {
                    self.evaluate();
                }
                ui.separator();
                ui.label("cps");
                let mut cps = self.cps;
                if ui
                    .add(egui::Slider::new(&mut cps, 0.1..=2.0).fixed_decimals(2))
                    .changed()
                {
                    self.set_cps(cps);
                }
                ui.separator();
                ui.label("out");
                let prev = self.output;
                egui::ComboBox::from_id_salt("output")
                    .selected_text(match self.output {
                        Output::Audio => "Audio",
                        Output::Midi => "MIDI",
                        Output::Osc => "OSC",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.output, Output::Audio, "Audio");
                        ui.selectable_value(&mut self.output, Output::Midi, "MIDI");
                        ui.selectable_value(&mut self.output, Output::Osc, "OSC");
                    });
                if self.output != prev {
                    self.route();
                }
                match self.output {
                    Output::Midi => {
                        ui.label("port");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.midi_port)
                                .hint_text("first")
                                .desired_width(90.0),
                        );
                    }
                    Output::Osc => {
                        ui.label("target");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.osc_target).desired_width(140.0),
                        );
                    }
                    Output::Audio => {}
                }
                ui.separator();
                ui.label(format!("status: {}", self.status));
                if self.audio_error.is_some() {
                    ui.colored_label(egui::Color32::YELLOW, "(no audio)");
                }
            });

            ui.horizontal(|ui| {
                ui.label("samples");
                ui.add(
                    egui::TextEdit::singleline(&mut self.sample_dir)
                        .hint_text("path to a folder of sample subfolders")
                        .desired_width(360.0),
                );
                if ui.button("Load folder").clicked() {
                    self.load_samples();
                }
            });
        });
    }

    fn errors_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("errors").show_inside(ui, |ui| {
            if let Some(e) = &self.audio_error {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 60), format!("audio: {e}"));
            }
            if let Some(e) = &self.io_error {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 60), e);
            }
            if let Some(e) = &self.eval_error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
            } else {
                ui.label("Ctrl+Enter to evaluate");
            }
        });
    }

    fn reference_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("reference")
            .resizable(true)
            .default_size(170.0)
            .show_inside(ui, |ui| {
                ui.heading("reference");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::CollapsingHeader::new("sounds")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.weak("synths");
                            for w in WAVEFORMS {
                                ui.monospace(*w);
                            }
                            if !self.sample_names.is_empty() {
                                ui.separator();
                                ui.weak("samples");
                                for name in &self.sample_names {
                                    ui.monospace(name);
                                }
                            }
                        });
                    egui::CollapsingHeader::new("controls")
                        .default_open(true)
                        .show(ui, |ui| {
                            for c in CONTROLS {
                                ui.monospace(*c);
                            }
                        });
                });
            });
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("editor")
            .resizable(true)
            .default_size(440.0)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.code)
                            .code_editor()
                            .desired_rows(28)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
    }
}

/// The `orbit` of a hap value (default 0), used to split the display into bands.
fn orbit_of(value: &Value) -> i64 {
    match value {
        Value::Map(m) => m.get("orbit").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64,
        _ => 0,
    }
}

/// Draw one cycle per orbit as colored blocks, with an optional playhead at
/// `playhead` (0..1 within the cycle).
fn draw_visualizer(ui: &mut egui::Ui, pat: &Pattern, playhead: Option<f32>) {
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

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rudel",
        native_options,
        Box::new(|_cc| Ok(Box::new(RudelApp::new()))),
    )
}
