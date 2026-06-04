// rudel-app - native live-coding editor for Rudel.
// Type Koto in the editor, Ctrl+Enter to evaluate and hot-swap the pattern
// into the running audio engine. The right panel visualizes one cycle.
// SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui;
use rudel_audio::Engine;
use rudel_core::{Frac, Pattern};

const DEFAULT_CODE: &str = r#"stack(
  note("c4 e4 g4 b4 a4 g4 e4 d4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").cutoff("400 1600").gain(0.6).delay(0.3)
)"#;

struct RudelApp {
    engine: Option<Engine>,
    audio_error: Option<String>,
    code: String,
    eval_error: Option<String>,
    status: String,
    cps: f64,
    playing: bool,
    current: Option<Pattern>,
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
        }
    }

    /// Evaluate the editor contents and, if playing, swap it into the engine.
    fn evaluate(&mut self) {
        match rudel_lang::eval(&self.code) {
            Ok(pat) => {
                self.current = Some(pat.clone());
                self.eval_error = None;
                self.status = "evaluated".to_string();
                if self.playing
                    && let Some(engine) = &self.engine
                {
                    engine.set_pattern(pat);
                }
            }
            Err(e) => {
                self.eval_error = Some(e);
                self.status = "error".to_string();
            }
        }
    }

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        if let Some(engine) = &self.engine {
            match (playing, &self.current) {
                (true, Some(pat)) => engine.set_pattern(pat.clone()),
                _ => engine.set_pattern(rudel_core::silence()),
            }
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
                if ui
                    .add(egui::Slider::new(&mut self.cps, 0.1..=2.0).fixed_decimals(2))
                    .changed()
                    && let Some(engine) = &self.engine
                {
                    engine.set_cps(self.cps);
                }
                ui.separator();
                ui.label(format!("status: {}", self.status));
                if self.audio_error.is_some() {
                    ui.colored_label(egui::Color32::YELLOW, "(no audio)");
                }
            });
        });

        egui::Panel::bottom("errors").show_inside(ui, |ui| {
            if let Some(e) = &self.audio_error {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 60), format!("audio: {e}"));
            }
            if let Some(e) = &self.eval_error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
            } else {
                ui.label("Ctrl+Enter to evaluate");
            }
        });

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

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label("pattern (one cycle)");
            match &self.current {
                Some(pat) => draw_pattern(ui, pat),
                None => {
                    ui.weak("evaluate a pattern to see it here");
                }
            }
        });
    }
}

/// Draw one cycle of a pattern as colored blocks, packed into lanes.
fn draw_pattern(ui: &mut egui::Ui, pat: &Pattern) {
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.retain(|h| h.whole.is_some());
    haps.sort_by_key(|h| h.part.begin);

    // greedy lane assignment
    let mut lane_ends: Vec<f64> = Vec::new();
    let mut lanes: Vec<usize> = Vec::with_capacity(haps.len());
    for h in &haps {
        let begin = h.part.begin.to_f64();
        let end = h.part.end.to_f64();
        let lane = lane_ends.iter().position(|&e| e <= begin + 1e-9);
        let lane = match lane {
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

    let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));

    let pad = 4.0;
    let w = (rect.width() - 2.0 * pad).max(1.0);
    let lane_h = ((rect.height() - 2.0 * pad) / lane_count as f32).max(2.0);

    for (h, &lane) in haps.iter().zip(&lanes) {
        let begin = h.part.begin.to_f64() as f32;
        let end = h.part.end.to_f64() as f32;
        let x0 = rect.left() + pad + begin * w;
        let x1 = rect.left() + pad + end * w;
        let y0 = rect.top() + pad + lane as f32 * lane_h;
        let block = egui::Rect::from_min_max(
            egui::pos2(x0 + 1.0, y0 + 1.0),
            egui::pos2((x1 - 1.0).max(x0 + 1.0), y0 + lane_h - 1.0),
        );
        let label = format!("{:?}", h.value);
        let color = color_for(&label);
        painter.rect_filled(block, 2.0, color);
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
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rudel",
        native_options,
        Box::new(|_cc| Ok(Box::new(RudelApp::new()))),
    )
}
