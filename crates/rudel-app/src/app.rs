mod panels;
mod routing;
mod samples;

use crate::editor::blocks::block_at_byte;
use crate::editor::decorations::{EditorDecorationState, SourceRange};
use crate::editor::settings::EditorSettings;
use crate::editor::widgets::WidgetHostState;
use crate::volume::DEFAULT_VOLUME_PERCENT;
use eframe::egui;
use rudel_audio::Engine;
use rudel_core::Pattern;
use rudel_midi::{MidiEngine, MidiIn, MidiOut};
use rudel_osc::OscEngine;
use std::collections::HashSet;
use std::thread::JoinHandle;
use std::time::Instant;

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
    reference: rudel_lang::Reference,
    highlight_idents: HashSet<String>,
    playing: bool,
    /// When playback started, used as a wall-clock position source for
    /// active-event highlighting when there is no audio device to clock from.
    play_start: Option<std::time::Instant>,
    current: Option<Pattern>,
    eval_meta: rudel_lang::EvalMeta,
    editor_decorations: EditorDecorationState,
    editor_settings: EditorSettings,
    widget_host: WidgetHostState,
    editor_cursor_byte: usize,
    block_flash: Option<(SourceRange, Instant)>,

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
    /// In-flight MIDI output connection. The first device open can block for a
    /// long time while the OS MIDI subsystem initializes, so it runs on a
    /// background thread and the engine is adopted once it finishes (see
    /// `poll_midi_connect`) instead of freezing the UI.
    midi_pending: Option<JoinHandle<Result<MidiOut, String>>>,
    osc: Option<OscEngine>,
    io_error: Option<String>,
    // MIDI input (CC -> `ccin` bus, clock-in -> cps).
    midi_in: Option<MidiIn>,
    /// In-flight MIDI input connection, connected on a background thread for the
    /// same reason as [`midi_pending`] and adopted by `poll_midi_in_connect`.
    ///
    /// [`midi_pending`]: RudelApp::midi_pending
    midi_in_pending: Option<JoinHandle<Result<MidiIn, String>>>,
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
        let reference = rudel_lang::reference();
        let highlight_idents = RudelApp::build_highlight_idents(&reference);
        RudelApp {
            engine,
            audio_error,
            code: DEFAULT_CODE.to_string(),
            eval_error: None,
            status: "ready".to_string(),
            cps: 0.5,
            volume_percent: DEFAULT_VOLUME_PERCENT,
            reference,
            highlight_idents,
            playing: false,
            play_start: None,
            current: None,
            eval_meta: rudel_lang::EvalMeta::default(),
            editor_decorations: EditorDecorationState::default(),
            editor_settings: EditorSettings::default(),
            widget_host: WidgetHostState::default(),
            editor_cursor_byte: 0,
            block_flash: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            loaded_sample_sources: HashSet::new(),
            sample_jobs: Vec::new(),
            output: Output::Audio,
            midi_port: String::new(),
            osc_target: "127.0.0.1:57120".to_string(),
            midi: None,
            midi_pending: None,
            osc: None,
            io_error: None,
            midi_in: None,
            midi_in_pending: None,
            midi_in_port: String::new(),
            clock_sync: false,
        }
    }

    /// Build the editor's highlight identifier set from the live runtime
    /// reference: top-level functions, pattern methods, control names, plus the
    /// Koto language keywords.
    fn build_highlight_idents(reference: &rudel_lang::Reference) -> HashSet<String> {
        let mut idents: HashSet<String> = HashSet::new();
        idents.extend(reference.functions.iter().cloned());
        idents.extend(reference.methods.iter().cloned());
        idents.extend(reference.controls.iter().cloned());
        idents.extend(
            crate::reference::LANGUAGE_KEYWORDS
                .iter()
                .map(|s| s.to_string()),
        );
        idents
    }

    /// Evaluate the editor contents and route the result to the active output.
    fn evaluate(&mut self) {
        match rudel_lang::eval_result(&self.code) {
            Ok(result) => {
                self.apply_sample_effects(&result.sample_effects);
                self.current = Some(result.pattern);
                self.editor_decorations.replace_all(&result.meta);
                self.widget_host.sync(self.editor_decorations.widgets());
                self.eval_meta = result.meta;
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

    fn evaluate_current_block(&mut self) {
        let Some(range) = block_at_byte(&self.code, self.editor_cursor_byte) else {
            self.evaluate();
            return;
        };
        if range.is_empty_in(&self.code) {
            self.status = "empty block".to_string();
            return;
        }

        let block = self.code[range.from..range.to].to_string();
        match rudel_lang::eval_result_with_source_range(&block, (range.from, range.to)) {
            Ok(result) => {
                self.apply_sample_effects(&result.sample_effects);
                self.current = Some(result.pattern);
                let source_range = SourceRange::new(range.from, range.to);
                self.editor_decorations
                    .replace_range(&result.meta, source_range);
                self.widget_host.sync(self.editor_decorations.widgets());
                self.eval_meta = result.meta;
                self.eval_error = None;
                self.block_flash = Some((source_range, Instant::now()));
                self.status = "block evaluated".to_string();
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
        let reference = rudel_lang::reference();
        let highlight_idents = RudelApp::build_highlight_idents(&reference);
        RudelApp {
            engine: None,
            audio_error: None,
            code: String::new(),
            eval_error: None,
            status: String::new(),
            cps: 0.5,
            volume_percent: DEFAULT_VOLUME_PERCENT,
            reference,
            highlight_idents,
            playing: false,
            play_start: None,
            current: None,
            eval_meta: rudel_lang::EvalMeta::default(),
            editor_decorations: EditorDecorationState::default(),
            editor_settings: EditorSettings::default(),
            widget_host: WidgetHostState::default(),
            editor_cursor_byte: 0,
            block_flash: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            loaded_sample_sources: HashSet::new(),
            sample_jobs: Vec::new(),
            output: Output::Audio,
            midi_port: String::new(),
            osc_target: "127.0.0.1:57120".to_string(),
            midi: None,
            midi_pending: None,
            osc: None,
            io_error: None,
            midi_in: None,
            midi_in_pending: None,
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
    fn midi_connect_is_polled_off_the_ui_thread() {
        // A background MIDI connection is adopted by poll_midi_connect rather
        // than blocking the UI; here it resolves to an error (no device), which
        // must be surfaced without leaving a dangling pending handle or engine.
        let mut app = app_without_engine();
        app.midi_pending = Some(std::thread::spawn(|| Err("no MIDI ports".to_string())));
        // Poll returns true while the connection is in flight, false once adopted.
        while app.poll_midi_connect() {}
        assert!(app.midi_pending.is_none());
        assert!(app.midi.is_none());
        assert!(
            app.io_error
                .as_ref()
                .is_some_and(|e| e.contains("no MIDI ports")),
            "connect error should be surfaced, got {:?}",
            app.io_error
        );
    }

    #[test]
    fn midi_input_connect_is_polled_off_the_ui_thread() {
        // MIDI input connects on a background thread too; poll_midi_in_connect
        // adopts the result and surfaces failures without a dangling handle.
        let mut app = app_without_engine();
        app.midi_in_pending = Some(std::thread::spawn(|| Err("no MIDI in ports".to_string())));
        while app.poll_midi_in_connect() {}
        assert!(app.midi_in_pending.is_none());
        assert!(app.midi_in.is_none());
        assert!(
            app.io_error
                .as_ref()
                .is_some_and(|e| e.contains("no MIDI in ports")),
            "connect error should be surfaced, got {:?}",
            app.io_error
        );
    }

    #[test]
    fn panic_stops_playback_and_resets_backends() {
        let mut app = app_without_engine();
        app.playing = true;
        app.panic();
        assert!(!app.playing);
        assert_eq!(app.status, "panic");
        assert!(app.midi.is_none());
        assert!(app.osc.is_none());
    }

    #[test]
    fn volume_percent_clamps_to_vlc_style_range() {
        let mut app = app_without_engine();
        app.set_volume_percent(250.0);
        assert_eq!(app.volume_percent, MAX_VOLUME_PERCENT);

        app.set_volume_percent(-10.0);
        assert_eq!(app.volume_percent, 0.0);
    }

    #[test]
    fn block_eval_uses_absolute_metadata_and_preserves_outside_widgets() {
        let mut app = app_without_engine();
        app.code = r#"note("c")._spiral()

slider(0.5, 0, 1)"#
            .to_string();

        app.editor_cursor_byte = 0;
        app.evaluate_current_block();
        assert_eq!(app.editor_decorations.widgets().len(), 1);
        assert_eq!(app.editor_decorations.widgets()[0].widget_type, "_spiral");

        app.editor_cursor_byte = app.code.find("slider").unwrap();
        app.evaluate_current_block();

        assert_eq!(app.editor_decorations.widgets().len(), 1);
        assert_eq!(app.editor_decorations.widgets()[0].widget_type, "_spiral");
        assert_eq!(app.editor_decorations.sliders().len(), 1);
        assert_eq!(
            app.editor_decorations.sliders()[0].range,
            SourceRange::new(
                app.code.find("0.5").unwrap(),
                app.code.find("0.5").unwrap() + 3
            )
        );
    }
}
