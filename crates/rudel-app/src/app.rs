mod files;
mod hydra_audio;
mod panels;
mod routing;
mod samples;
#[cfg(test)]
mod ui_tests;

use crate::{
    editor::{
        blocks::block_at_byte,
        decorations::{EditorDecorationState, SourceRange},
        settings::EditorSettings,
        widgets::WidgetHostState,
    },
    volume::DEFAULT_VOLUME_PERCENT,
};
use eframe::egui;
use rudel_audio::Engine;
use rudel_core::Pattern;
use rudel_midi::{MidiEngine, MidiIn, MidiOut};
use rudel_osc::OscEngine;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    thread::JoinHandle,
    time::Instant,
};

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
    /// Report a failure to the console rather than the error bar. Set for the
    /// startup sample banks: they are not something the user asked for, and
    /// working offline should not open with seven red messages.
    quiet: bool,
}

pub(crate) struct RudelApp {
    engine: Option<Engine>,
    audio_error: Option<String>,
    code: String,
    /// The buffer as it last was on disk (or as it started, with no file yet).
    /// Anything else in `code` is an unsaved edit; see `RudelApp::is_dirty`.
    saved_code: String,
    /// The Open/Save/confirm dialogs; a test swaps in its own answers.
    dialogs: files::Dialogs,
    /// The file the buffer came from, when Open or Save named one.
    /// `None` is an unsaved scratch buffer (still autosaved by eframe).
    file_path: Option<PathBuf>,
    /// Last title pushed to the window, so it is only pushed on a change.
    window_title: String,
    eval_error: Option<String>,
    status: String,
    cps: f64,
    volume_percent: f32,
    /// Identifiers the editor highlights as keywords, generated once from the
    /// live runtime via `rudel_lang::reference()` so it can't drift.
    reference: rudel_lang::Reference,
    highlight_idents: HashSet<String>,
    /// Case-insensitive substring filter for the reference side panel.
    reference_filter: String,
    /// Reference name double-clicked this frame, inserted into the editor at
    /// the cursor when the editor panel renders (same frame).
    pending_insert: Option<String>,
    playing: bool,
    /// When playback started, used as a wall-clock position source for
    /// active-event highlighting when there is no audio device to clock from.
    play_start: Option<std::time::Instant>,
    current: Option<Pattern>,
    /// Bumped whenever `current` is replaced, so the widgets' hap cache can tell
    /// a re-eval from a redraw without comparing patterns (which are closures).
    pattern_generation: u64,
    /// The current cycle's flash spans, keyed on `(pattern_generation, cycle)`
    /// — the cycle as raw bits so the key is `Eq`.
    flash_cache: Option<((u64, u64), panels::CycleFlashes)>,
    editor_decorations: EditorDecorationState,
    editor_settings: EditorSettings,
    widget_host: WidgetHostState,
    /// The per-frame hydra arguments of the last evaluation that had any.
    hydra_params: Vec<rudel_lang::hydra::HydraParam>,
    /// When hydra's clock started: it runs in seconds from launch, as
    /// upstream's does from `initHydra`, transport or no.
    hydra_epoch: Instant,
    /// What the script draws on Strudel's canvas each frame, and the canvas.
    canvas_driver: Option<rudel_lang::canvas::CanvasDriver>,
    canvas: crate::canvas::Canvas,
    editor_cursor_byte: usize,
    block_flash: Option<(SourceRange, Instant)>,

    // Sample loading.
    sample_dir: String,
    sample_names: Vec<String>,
    /// Sources already loaded via `samples(...)`, so re-evaluating doesn't
    /// re-fetch the same pack on every keystroke.
    loaded_sample_sources: HashSet<String>,
    /// Soundfont presets already queued or loaded, so a pattern asking for one
    /// every cycle only ever triggers a single fetch.
    requested_fonts: HashSet<(String, i64)>,
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
    /// Extra MIDI input ports opened because a script called
    /// `midin(name)`/`midikeys(name)`, keyed by the name the script used (which
    /// is also how the input bus tags what arrives on them). Held so the
    /// connections stay alive; opened once per name per session.
    script_midi_ins: HashMap<String, MidiIn>,
    /// `midin`/`midikeys` port opens still in flight, as `(name, handle)`.
    script_midi_in_pending: Vec<(String, JoinHandle<Result<MidiIn, String>>)>,
    /// Lines written by `log`/`logValues`-tagged events, drained off the
    /// scheduler each frame and shown in the console panel.
    pub(super) log_lines: Vec<String>,
    /// `onTriggerTime` callbacks from the last evaluation, plus the cycle
    /// position they have already been fired up to.
    pub(super) trigger_hooks: rudel_lang::triggers::TriggerHooks,
    pub(super) trigger_fired_upto: Option<f64>,
    /// The platform speech synthesiser behind `.speak(...)`, and whether the
    /// active pattern has anything for it — checked when the pattern is routed
    /// so the per-frame trigger sweep can be skipped when it has not.
    pub(super) speech: crate::speech::Speech,
    pub(super) speaks: bool,
}

impl RudelApp {
    fn new() -> RudelApp {
        let (engine, audio_error) = match Engine::new() {
            Ok(e) => {
                e.set_cps(0.5);
                e.set_volume((DEFAULT_VOLUME_PERCENT / 100.0) as f64);
                (Some(e), None)
            }
            Err(e) => (None, Some(e)),
        };
        let mut app = RudelApp {
            engine,
            audio_error,
            ..RudelApp::headless()
        };
        // The sample banks the Strudel REPL preloads. Queued in the background,
        // cached on disk, and quiet on failure, so a first run picks them up
        // and later ones (offline included) start instantly.
        app.prebake_default_samples();
        app
    }

    /// The full app state with no audio device attached. [`RudelApp::new`]
    /// layers a real [`Engine`] on top; tests build this directly so they never
    /// need (or block on) audio hardware.
    fn headless() -> RudelApp {
        rudel_lang::install_mini();
        let reference = rudel_lang::reference();
        let highlight_idents = RudelApp::build_highlight_idents(&reference);
        RudelApp {
            engine: None,
            audio_error: None,
            code: DEFAULT_CODE.to_string(),
            saved_code: DEFAULT_CODE.to_string(),
            dialogs: files::Dialogs::default(),
            file_path: None,
            window_title: String::new(),
            eval_error: None,
            status: "ready".to_string(),
            cps: 0.5,
            volume_percent: DEFAULT_VOLUME_PERCENT,
            reference,
            highlight_idents,
            reference_filter: String::new(),
            pending_insert: None,
            playing: false,
            play_start: None,
            current: None,
            pattern_generation: 0,
            flash_cache: None,
            editor_decorations: EditorDecorationState::default(),
            editor_settings: EditorSettings::default(),
            widget_host: WidgetHostState::default(),
            hydra_params: Vec::new(),
            hydra_epoch: Instant::now(),
            canvas_driver: None,
            canvas: crate::canvas::Canvas::default(),
            editor_cursor_byte: 0,
            block_flash: None,
            sample_dir: String::new(),
            sample_names: Vec::new(),
            loaded_sample_sources: HashSet::new(),
            requested_fonts: HashSet::new(),
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
            script_midi_ins: HashMap::new(),
            script_midi_in_pending: Vec::new(),
            log_lines: Vec::new(),
            trigger_hooks: rudel_lang::triggers::TriggerHooks::default(),
            trigger_fired_upto: None,
            speech: crate::speech::Speech::default(),
            speaks: false,
        }
    }

    /// Build the editor's highlight identifier set from the live runtime
    /// reference: top-level functions, pattern methods, control names, plus the
    /// JavaScript keywords.
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
        self.set_picture_dir();
        match rudel_lang::eval_result(&self.code) {
            Ok(result) => {
                self.apply_sample_effects(&result.sample_effects);
                self.current = Some(result.pattern);
                self.pattern_generation = self.pattern_generation.wrapping_add(1);
                self.trigger_hooks = result.trigger_hooks;
                self.trigger_fired_upto = None;
                self.editor_decorations.replace_all(&result.meta);
                self.hydra_params = result.meta.hydra_params;
                // A new evaluation starts on a clean canvas.
                self.canvas_driver = result.meta.canvas;
                self.canvas.clear();
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

    /// A picture a script names by relative path (`s0.initImage('a.png')`)
    /// is read from beside the open file.
    fn set_picture_dir(&self) {
        crate::editor::widgets::set_picture_dir(
            self.file_path
                .as_deref()
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf),
        );
    }

    fn evaluate_current_block(&mut self) {
        self.set_picture_dir();
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
                self.pattern_generation = self.pattern_generation.wrapping_add(1);
                self.trigger_hooks = result.trigger_hooks;
                self.trigger_fired_upto = None;
                let source_range = SourceRange::new(range.from, range.to);
                self.editor_decorations
                    .replace_range(&result.meta, source_range);
                // ponytail: slots are numbered per evaluation, so a block's
                // replace the last ones wholesale and a chain from another
                // block reads this block's values. Number them per block if
                // two hydra blocks ever have to animate side by side.
                if !result.meta.hydra_params.is_empty() {
                    self.hydra_params = result.meta.hydra_params;
                }
                // A block that draws takes the canvas over; one that does not
                // leaves the drawing running.
                if result.meta.canvas.is_some() {
                    self.canvas_driver = result.meta.canvas;
                }
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

/// The window/taskbar icon: a two-turn spiral (a nod to Strudel's pretzel
/// swirl) on a dark rounded square, purple at the core fading to green at the
/// tail. Baked to `icon.png` so no rasterizer runs at startup; `icon.ico` is
/// the same artwork, embedded into the exe by `build.rs` for Explorer.
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../icon.png")).expect("icon.png is valid")
}

/// Give the GPU widgets their pipeline stores. Shader widgets compile against
/// the window's format, and only the creation context knows it; without a wgpu
/// render state (the glow backend, or a test with no renderer) the widgets fall
/// back to drawing on the CPU.
pub(crate) fn install_gpu_stores(cc: &eframe::CreationContext<'_>) {
    if let Some(render_state) = &cc.wgpu_render_state {
        let format = render_state.target_format;
        let mut renderer = render_state.renderer.write();
        renderer
            .callback_resources
            .insert(crate::editor::ShaderStore::new(format));
        renderer
            .callback_resources
            .insert(crate::editor::SpiralStore::new(format));
        renderer
            .callback_resources
            .insert(crate::editor::HydraStore::new(format));
    }
}

pub(crate) fn run() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 640.0])
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "rudel",
        native_options,
        Box::new(|cc| {
            crate::theme::apply(&cc.egui_ctx);
            install_gpu_stores(cc);
            let mut app = RudelApp::new();
            // Restore the last autosaved buffer, so closing the window (or
            // losing it) does not lose what was typed, plus the file it came
            // from so Ctrl+S still knows where to write.
            if let Some(storage) = cc.storage {
                if let Some(code) = storage.get_string(panels::SAVED_CODE_KEY) {
                    app.code = code;
                }
                app.file_path = storage
                    .get_string(panels::SAVED_PATH_KEY)
                    .as_deref()
                    .and_then(files::restore_path);
                // A restored buffer only counts as saved if it still matches
                // the file it came from: edits that were never written stay
                // unsaved across a restart, and still warn before being lost.
                app.saved_code = app
                    .file_path
                    .as_deref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_else(|| app.code.clone());
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::MAX_VOLUME_PERCENT;

    #[test]
    fn the_baked_window_icon_decodes_at_full_size() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        // Center sits inside the rounded square; the corner is outside it.
        assert_eq!(icon.rgba[((256 / 2) * 256 + 256 / 2) * 4 + 3], 255);
        assert_eq!(icon.rgba[3], 0);
    }

    fn app_without_engine() -> RudelApp {
        RudelApp {
            code: String::new(),
            status: String::new(),
            ..RudelApp::headless()
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

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStorage(HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn saving_keeps_the_buffer_and_the_file_it_came_from() {
        let mut app = RudelApp::headless();
        app.code = "s(\"bd\")".to_string();
        app.file_path = Some(std::path::PathBuf::from("song.js"));
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        assert_eq!(
            storage.0.get(panels::SAVED_CODE_KEY).map(String::as_str),
            Some("s(\"bd\")")
        );
        assert_eq!(
            storage.0.get(panels::SAVED_PATH_KEY).map(String::as_str),
            Some("song.js")
        );
    }

    #[test]
    fn highlight_idents_are_the_reference_plus_the_keywords() {
        let reference = rudel_lang::Reference {
            functions: vec!["fnname".into()],
            methods: vec!["methname".into()],
            controls: vec!["ctlname".into()],
        };
        let idents = RudelApp::build_highlight_idents(&reference);
        for name in ["fnname", "methname", "ctlname"] {
            assert!(idents.contains(name), "{name}");
        }
    }
}
