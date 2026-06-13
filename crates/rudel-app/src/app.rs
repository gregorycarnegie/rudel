mod panels;
mod routing;
mod samples;

use crate::volume::DEFAULT_VOLUME_PERCENT;
use eframe::egui;
use rudel_audio::Engine;
use rudel_core::Pattern;
use rudel_midi::{MidiEngine, MidiIn};
use rudel_osc::OscEngine;
use std::collections::HashSet;
use std::thread::JoinHandle;

const DEFAULT_CODE: &str = r#"stack(
  s("bd ~ bd bd").gain(0.9),
  s("~ sd ~ sd"),
  s("hh*8").gain(0.5),
  note("c4 e4 g4 b4 a4 g4 e4 d4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").lpf("400 1600").gain(0.6).delay(0.3)
)"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Audio,
    Midi,
    Osc,
}

struct SampleJob {
    key: String,
    label: String,
    handle: JoinHandle<Result<usize, String>>,
}

pub(crate) struct RudelApp {
    engine: Option<Engine>,
    audio_error: Option<String>,
    code: String,
    eval_error: Option<String>,
    status: String,
    cps: f64,
    volume_percent: f32,
    /// Identifiers the editor highlights as keywords, generated once from the
    /// live runtime via `rudel_lang::reference()` so it can't drift.
    highlight_idents: HashSet<String>,
    playing: bool,
    /// When playback started, used as a wall-clock position source for
    /// active-event highlighting when there is no audio device to clock from.
    play_start: Option<std::time::Instant>,
    current: Option<Pattern>,

    // Sample loading.
    sample_dir: String,
    sample_names: Vec<String>,
    /// Sources already loaded via `samples(...)`, so re-evaluating doesn't
    /// re-fetch the same pack on every keystroke.
    loaded_sample_sources: HashSet<String>,
    sample_jobs: Vec<SampleJob>,

    // Output routing.
    output: Output,
    midi_port: String,
    osc_target: String,
    midi: Option<MidiEngine>,
    osc: Option<OscEngine>,
    io_error: Option<String>,
    // MIDI input (CC -> `ccin` bus, clock-in -> cps).
    midi_in: Option<MidiIn>,
    midi_in_port: String,
    clock_sync: bool,
}

impl RudelApp {
    fn new() -> RudelApp {
        rudel_mini::install();
        let (engine, audio_error) = match Engine::new() {
            Ok(e) => {
                e.set_cps(0.5);
                e.set_volume((DEFAULT_VOLUME_PERCENT / 100.0) as f64);
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
            volume_percent: DEFAULT_VOLUME_PERCENT,
            highlight_idents: RudelApp::build_highlight_idents(),
            playing: false,
            play_start: None,
            current: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            loaded_sample_sources: HashSet::new(),
            sample_jobs: Vec::new(),
            output: Output::Audio,
            midi_port: String::new(),
            osc_target: "127.0.0.1:57120".to_string(),
            midi: None,
            osc: None,
            io_error: None,
            midi_in: None,
            midi_in_port: String::new(),
            clock_sync: false,
        }
    }

    /// Build the editor's highlight identifier set from the live runtime
    /// reference: top-level functions, pattern methods, control names, plus the
    /// Koto language keywords.
    fn build_highlight_idents() -> HashSet<String> {
        let reference = rudel_lang::reference();
        let mut idents: HashSet<String> = HashSet::new();
        idents.extend(reference.functions);
        idents.extend(reference.methods);
        idents.extend(reference.controls);
        idents.extend(
            crate::reference::LANGUAGE_KEYWORDS
                .iter()
                .map(|s| s.to_string()),
        );
        idents
    }

    /// Evaluate the editor contents and route the result to the active output.
    fn evaluate(&mut self) {
        match rudel_lang::eval_with_samples(&self.code) {
            Ok((pat, effects)) => {
                self.apply_sample_effects(&effects);
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
}

pub(crate) fn run() -> eframe::Result {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::MAX_VOLUME_PERCENT;

    fn app_without_engine() -> RudelApp {
        RudelApp {
            engine: None,
            audio_error: None,
            code: String::new(),
            eval_error: None,
            status: String::new(),
            cps: 0.5,
            volume_percent: DEFAULT_VOLUME_PERCENT,
            highlight_idents: RudelApp::build_highlight_idents(),
            playing: false,
            play_start: None,
            current: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            loaded_sample_sources: HashSet::new(),
            sample_jobs: Vec::new(),
            output: Output::Audio,
            midi_port: String::new(),
            osc_target: "127.0.0.1:57120".to_string(),
            midi: None,
            osc: None,
            io_error: None,
            midi_in: None,
            midi_in_port: String::new(),
            clock_sync: false,
        }
    }

    #[test]
    fn sample_effects_apply_cps_to_app_state() {
        let mut app = app_without_engine();
        app.apply_sample_effects(&rudel_lang::SampleEffects {
            cps: Some(0.75),
            ..Default::default()
        });
        assert_eq!(app.cps, 0.75);
    }

    #[test]
    fn volume_percent_clamps_to_vlc_style_range() {
        let mut app = app_without_engine();
        app.set_volume_percent(250.0);
        assert_eq!(app.volume_percent, MAX_VOLUME_PERCENT);

        app.set_volume_percent(-10.0);
        assert_eq!(app.volume_percent, 0.0);
    }
}
