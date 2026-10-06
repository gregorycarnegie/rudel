use super::{Output, RudelApp};
use crate::{
    editor::{
        CodeEditorInput, EditorAction, code_editor,
        decorations::FlashSpan,
        settings::{EditorFontFamily, EditorTheme},
    },
    reference::{DRUMS, FACTORIES, SIGNALS, WAVEFORMS},
    volume::vlc_volume_slider,
};
use eframe::egui;

/// Storage key for the persisted editor buffer.
pub(super) const SAVED_CODE_KEY: &str = "code";

/// Storage key for the file that buffer belongs to.
pub(super) const SAVED_PATH_KEY: &str = "code_path";

/// Storage key for the editor settings.
pub(super) const SAVED_SETTINGS_KEY: &str = "editor_settings";
pub(crate) use crate::editor::CANVAS_RECT;

/// How many `log`/`logValues` lines the console keeps.
const LOG_LINES_SHOWN: usize = 512;

impl eframe::App for RudelApp {
    /// eframe calls this on its autosave timer and on exit. It keeps the
    /// editor buffer, the file it came from, and the editor settings; restored
    /// by `RudelApp::restore`.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(SAVED_CODE_KEY, self.code.clone());
        if let Some(path) = &self.file_path {
            storage.set_string(SAVED_PATH_KEY, path.to_string_lossy().into_owned());
        }
        storage.set_string(SAVED_SETTINGS_KEY, self.editor_settings.to_saved());
        storage.set_string(
            super::midi_in::SAVED_MIDI_CC_KEY,
            super::midi_in::saved_cc_state(),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // The GPU widget painters keep their resources in the wgpu renderer, so
        // without that backend they would draw nothing at all; and the spiral's
        // needs a storage buffer that downlevel devices cannot give it.
        let gpu_available = frame
            .wgpu_render_state()
            .is_some_and(|rs| crate::editor::spiral_gpu_supported(&rs.device.limits()));
        // The window's close button, Alt+F4 and the like all arrive here as a
        // close request; take it back if the buffer has edits the user says
        // they do not want to lose.
        if ui.ctx().input(|i| i.viewport().close_requested())
            && !self.confirm_discard("Close rudel")
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        pump_input_bus(ui.ctx());
        self.log_lines.extend(crate::editor::fonts::install(
            ui.ctx(),
            self.editor_settings.font_family.web_font(),
        ));
        self.poll_font_requests();
        self.watch_midi_inputs(ui.ctx());
        crate::gamepad::start_if_requested();
        self.poll_sample_requests();
        self.poll_sample_jobs(ui.ctx());
        let midi_connecting =
            self.poll_midi_connect() | self.poll_midi_in_connect() | self.poll_script_midi_inputs();

        // Match Strudel's REPL transport keys: Ctrl/Alt+Enter evaluates,
        // Ctrl/Alt+. hushes, and Ctrl+Shift+. panics (reset/all-notes-off).
        let (eval_shortcut, secondary_eval_shortcut, hush_shortcut, panic_shortcut) =
            ui.ctx().input(|i| {
                let trigger = i.modifiers.command || i.modifiers.alt;
                (
                    trigger && !i.modifiers.shift && i.key_pressed(egui::Key::Enter),
                    i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Enter),
                    trigger && !i.modifiers.shift && i.key_pressed(egui::Key::Period),
                    i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Period),
                )
            });
        // File keys, read in the same pass. Ctrl+S with no file yet falls
        // through to Save As, so there is no way to "save" into nowhere.
        let (open_shortcut, save_shortcut, save_as_shortcut) = ui.ctx().input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::S),
            )
        });
        if open_shortcut {
            self.open_file();
        }
        if save_as_shortcut {
            self.save_file_as();
        } else if save_shortcut {
            self.save_file();
        }

        if eval_shortcut {
            self.primary_eval();
        }
        if secondary_eval_shortcut {
            self.secondary_eval();
        }
        if panic_shortcut {
            self.panic();
        } else if hush_shortcut {
            self.hush();
        }

        self.sync_window_title(ui.ctx());
        self.fire_trigger_hooks();
        let active_spans = self.active_editor_spans();
        self.transport_panel(ui);
        self.errors_panel(ui);
        self.console_panel(ui);
        self.reference_panel(ui);
        self.editor_panel(ui, &active_spans, gpu_available);

        // Clock-in: follow the incoming MIDI clock tempo (4 beats per cycle).
        if self.clock_sync {
            let cps = self.midi_in.as_ref().and_then(|i| i.cps(4.0));
            if let Some(cps) = cps
                && (cps - self.cps).abs() > 1e-4
            {
                self.set_cps(cps);
            }
        }

        // Keep the playhead moving while playing (and polling clock / CC input /
        // a pending MIDI connection).
        // hydra's scene animates on its own clock, playing or not.
        if self.playing
            || self.editor_decorations.backdrop().is_some()
            || self.canvas_driver.is_some()
            || !self.sample_jobs.is_empty()
            || self.clock_sync
            || self.midi_in.is_some()
            || midi_connecting
        {
            ui.ctx().request_repaint();
        }
    }
}

impl RudelApp {
    /// Right-aligned status text with a colored state light: green playing,
    /// red on an error, grey otherwise.
    fn status_light(&self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if self.audio_error.is_some() {
                ui.colored_label(crate::theme::ACCENT, "no audio");
            }
            let light = if self.eval_error.is_some() || self.io_error.is_some() {
                crate::theme::STOP
            } else if self.playing {
                crate::theme::GO
            } else {
                egui::Color32::from_gray(90)
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, light);
            ui.weak(&self.status);
        });
    }

    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("transport").show(ui, |ui| {
            ui.add_space(3.0);
            // Wrapped, not plain horizontal: at a narrow window (or with the
            // MIDI/OSC port field showing) these controls are wider than the
            // row, and an overflowing horizontal draws its tail on top of
            // whatever is right-aligned in the same row.
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("rudel")
                        .monospace()
                        .size(18.0)
                        .strong()
                        .color(crate::theme::ACCENT),
                );
                ui.separator();
                if ui.button("Open").on_hover_text("Ctrl+O").clicked() {
                    self.open_file();
                }
                if ui
                    .button("Save")
                    .on_hover_text("Ctrl+S · Ctrl+Shift+S for Save As")
                    .clicked()
                {
                    self.save_file();
                }
                // Recording is a file action, so it sits with Open and Save
                // rather than with the transport.
                let recording = self.is_recording();
                let record_button = if recording {
                    egui::Button::new(
                        egui::RichText::new("⏺ Stop")
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(crate::theme::STOP)
                } else {
                    egui::Button::new("⏺ Rec")
                };
                if ui
                    .add(record_button)
                    .on_hover_text(if recording {
                        "Finish the recording"
                    } else {
                        "Record the audio output — WAV, FLAC, MP3, Ogg Vorbis or Opus, \
                         chosen by the file extension"
                    })
                    .clicked()
                {
                    self.toggle_recording();
                }
                ui.separator();
                // Play is the one action a live coder reaches for blind: filled
                // accent while stopped, red while playing.
                let play_button = if self.playing {
                    egui::Button::new(
                        egui::RichText::new("⏹ Stop")
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(crate::theme::STOP)
                } else {
                    egui::Button::new(
                        egui::RichText::new("▶ Play")
                            .strong()
                            .color(egui::Color32::BLACK),
                    )
                    .fill(crate::theme::ACCENT)
                };
                if ui
                    .add(play_button.min_size(egui::vec2(72.0, 26.0)))
                    .clicked()
                {
                    let now = !self.playing;
                    if now && self.current.is_none() {
                        self.evaluate();
                    }
                    self.set_playing(now);
                }
                let ((primary_label, primary_tip), (secondary_label, secondary_tip)) =
                    eval_button_labels(self.editor_settings.block_based_eval);
                if ui
                    .button(primary_label)
                    .on_hover_text(primary_tip)
                    .clicked()
                {
                    self.primary_eval();
                }
                if ui
                    .button(secondary_label)
                    .on_hover_text(secondary_tip)
                    .clicked()
                {
                    self.secondary_eval();
                }
                if ui.button("Hush").on_hover_text("Ctrl+.").clicked() {
                    self.hush();
                }
                if ui.button("Panic").on_hover_text("Ctrl+Shift+.").clicked() {
                    self.panic();
                }
                ui.separator();
                ui.label("cps");
                let mut cps = self.cps;
                let response = ui.add(egui::Slider::new(&mut cps, 0.1..=2.0).fixed_decimals(2));
                if response.changed()
                    | crate::scroll::scroll_adjust(ui, &response, &mut cps, 0.1..=2.0, 0.01)
                {
                    self.set_cps(cps);
                }
                ui.separator();
                let mut volume_percent = self.volume_percent;
                if vlc_volume_slider(ui, &mut volume_percent).changed() {
                    self.set_volume_percent(volume_percent);
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
            });

            // Occasional setup lives out of the way: one collapsed row instead
            // of two always-visible ones. The status light rides on the same
            // row, right-aligned — the controls row above is full, and this one
            // holds a single short label.
            let io_summary = io_summary(
                self.sample_names.len(),
                self.midi_in.is_some(),
                self.midi_in_pending.is_some(),
            );
            // `CollapsingState` rather than `CollapsingHeader` so the status
            // light can share the header row; the cost is that only the arrow
            // toggles, so the label is made clickable to match.
            let mut toggle = false;
            let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id("io_section"),
                false,
            )
            .show_header(ui, |ui| {
                toggle = ui
                    .add(egui::Label::new(io_summary).sense(egui::Sense::click()))
                    .clicked();
                self.status_light(ui);
            });
            if toggle {
                header.toggle();
            }
            header.body(|ui| {
                ui.horizontal(|ui| {
                    ui.label("samples");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.sample_dir)
                            .hint_text("folder, strudel.json, URL, or github:user/repo")
                            .desired_width(360.0),
                    );
                    if ui.button("Load samples").clicked() {
                        self.load_samples();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("midi in");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.midi_in_port)
                            .hint_text("first")
                            .desired_width(90.0),
                    );
                    let connected = self.midi_in.is_some();
                    let connecting = self.midi_in_pending.is_some();
                    let label = if connecting {
                        "Connecting…"
                    } else if connected {
                        "Reconnect"
                    } else {
                        "Connect"
                    };
                    if ui
                        .add_enabled(!connecting, egui::Button::new(label))
                        .clicked()
                    {
                        self.connect_input();
                    }
                    if connected && ui.button("Disconnect").clicked() {
                        self.midi_in = None;
                    }
                    ui.checkbox(&mut self.clock_sync, "clock→cps");
                    if let Some(bpm) = self.midi_in.as_ref().and_then(|i| i.bpm()) {
                        ui.weak(format!("{bpm:.0} bpm"));
                    }
                    ui.weak("→ ccin(n)");
                });
            });
            ui.add_space(2.0);
        });
    }

    /// Fire the `onTriggerTime` callbacks — and speak the `.speak(...)` haps —
    /// of every event whose onset the playhead has passed since the last frame.
    /// Upstream schedules these with `window.setTimeout`, so frame-rate accuracy
    /// matches its own caveat that the hook is "innacurate for audio tasks".
    ///
    /// Both run here because this is a thread that may block: a hook waits on
    /// the script engine's own thread, and speech is an OS call. Neither is
    /// something to make the audio callback wait on.
    fn fire_trigger_hooks(&mut self) {
        if self.trigger_hooks.is_empty() && !self.speaks && !self.serial_haps && !self.mqtt_haps {
            return;
        }
        let Some(pos) = self.playback_position_cycles() else {
            self.trigger_fired_upto = None;
            return;
        };
        let Some(pattern) = self.current.clone() else {
            return;
        };
        // The first frame after evaluating (or after a seek backwards) only
        // establishes the mark, so a whole cycle of past events doesn't all
        // fire at once.
        let Some(from) = self.trigger_fired_upto.filter(|&from| pos > from) else {
            self.trigger_fired_upto = Some(pos);
            return;
        };
        self.trigger_fired_upto = Some(pos);
        let haps = pattern.query_arc(
            rudel_core::Frac::from_f64(from),
            rudel_core::Frac::from_f64(pos),
        );
        for hap in haps {
            let onset = match &hap.whole {
                Some(w) => w.begin.to_f64(),
                None => continue, // continuous haps have no trigger
            };
            if onset < from || onset >= pos {
                continue;
            }
            if let Some(write) = rudel_core::serial::request(&hap.value) {
                self.serial.send(write);
            }
            if let Some(msg) =
                rudel_core::mqtt::publish(&hap.value, hap.duration().to_f64(), self.cps)
            {
                self.mqtt.send(msg);
            }
            if let Some(e) = self.trigger_hooks.fire(&hap) {
                self.eval_error = Some(format!("onTriggerTime: {e}"));
            }
            if let Some((words, lang, pick)) = crate::speech::request(&hap.value)
                && let Some(e) = self.speech.say(&words, lang.as_deref(), pick.as_ref())
            {
                self.eval_error = Some(e);
            }
        }
        self.apply_pattern_settings();
    }

    /// Editor settings a pattern changed as it played (`.theme("nord")`,
    /// `.fontSize(24)`), as the Strudel website applies them.
    fn apply_pattern_settings(&mut self) {
        for (key, value) in rudel_lang::triggers::take_settings() {
            match key.as_str() {
                "theme" => match EditorTheme::named(value.trim()) {
                    Some(theme) => self.editor_settings.theme = theme,
                    None => rudel_core::log_line(format!("theme: no theme named {value:?}")),
                },
                "fontSize" => {
                    if let Ok(size) = value.trim().parse::<f32>()
                        && size.is_finite()
                    {
                        self.editor_settings.font_size = size.clamp(6.0, 96.0);
                    }
                }
                "fontFamily" => {
                    if let Some(family) = EditorFontFamily::named(&value) {
                        self.editor_settings.font_family = family;
                    }
                }
                _ => {}
            }
        }
    }

    /// The `log`/`logValues` console — Strudel writes these to the REPL's side
    /// menu; Rudel collects them off the scheduler and shows them here. Hidden
    /// entirely until a pattern logs something, so it costs no screen space.
    fn console_panel(&mut self, ui: &mut egui::Ui) {
        self.log_lines.extend(rudel_core::drain_log());
        // Keep the tail; the ring in rudel-core is bounded the same way.
        let overflow = self.log_lines.len().saturating_sub(LOG_LINES_SHOWN);
        self.log_lines.drain(..overflow);
        if self.log_lines.is_empty() {
            return;
        }
        egui::Panel::bottom("console")
            .resizable(true)
            .default_size(90.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("console");
                    if ui.button("clear").clicked() {
                        self.log_lines.clear();
                    }
                });
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log_lines {
                            ui.label(egui::RichText::new(line).monospace().size(12.0));
                        }
                    });
            });
    }

    fn errors_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("errors").show(ui, |ui| {
            if let Some(e) = &self.audio_error {
                ui.colored_label(crate::theme::ACCENT, format!("audio: {e}"));
            }
            if let Some(e) = &self.io_error {
                ui.colored_label(crate::theme::ACCENT, e);
            }
            for e in &self.theme_errors {
                ui.colored_label(crate::theme::ACCENT, e);
            }
            if let Some(e) = &self.eval_error {
                ui.colored_label(crate::theme::STOP, e);
            } else {
                ui.weak(
                    "Ctrl+Enter eval · Ctrl+Shift+Enter block · Ctrl+. hush · Ctrl+Shift+. panic · Ctrl+/ comment · Ctrl+O open · Ctrl+S save",
                );
            }
        });
    }

    fn reference_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("reference")
            .resizable(true)
            .default_size(170.0)
            .show(ui, |ui| {
                ui.heading("reference");
                ui.add(
                    egui::TextEdit::singleline(&mut self.reference_filter)
                        .hint_text("filter…")
                        .desired_width(f32::INFINITY),
                );
                let query = self.reference_filter.trim();
                let filtering = !query.is_empty();
                // Force sections open while filtering so matches are visible.
                let force_open = filtering.then_some(true);

                let synths = fuzzy_filter(WAVEFORMS.iter().copied(), query);
                let drums = fuzzy_filter(DRUMS.iter().copied(), query);
                let samples = fuzzy_filter(self.sample_names.iter().map(String::as_str), query);
                let sound_groups = [("synths", synths), ("drums", drums), ("samples", samples)];
                // Controls come straight from the live runtime surface, so the
                // pane can't advertise a control the engine no longer has.
                let controls =
                    fuzzy_filter(self.reference.controls.iter().map(String::as_str), query);
                let signals = fuzzy_filter(SIGNALS.iter().copied(), query);
                let factories = fuzzy_filter(FACTORIES.iter().copied(), query);

                // Collected locally to avoid borrowing `self` inside the
                // closures (sound_groups borrows sample_names immutably).
                let mut insert: Option<String> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if sound_groups.iter().any(|(_, items)| !items.is_empty()) {
                        egui::CollapsingHeader::new("sounds")
                            .default_open(true)
                            .open(force_open)
                            .show(ui, |ui| {
                                let mut first = true;
                                for (label, items) in &sound_groups {
                                    if items.is_empty() {
                                        continue;
                                    }
                                    if !first {
                                        ui.separator();
                                    }
                                    first = false;
                                    ui.weak(*label);
                                    if let Some(text) = reference_items(ui, items) {
                                        insert = Some(text);
                                    }
                                }
                            });
                    }
                    for (title, items, default_open) in [
                        ("controls", &controls, true),
                        ("signals", &signals, false),
                        ("factories", &factories, false),
                    ] {
                        if items.is_empty() {
                            continue;
                        }
                        egui::CollapsingHeader::new(title)
                            .default_open(default_open)
                            .open(force_open)
                            .show(ui, |ui| {
                                if let Some(text) = reference_items(ui, items) {
                                    insert = Some(text);
                                }
                            });
                    }
                });
                if insert.is_some() {
                    self.pending_insert = insert;
                }
            });
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui, active_spans: &[FlashSpan], gpu_available: bool) {
        // Theme the whole editor region to its own theme (not the host/system
        // theme) so the background, text and TextEdit all share one color and the
        // editor fills its panel seamlessly — no contrasting box with light
        // margins around it.
        let draw = self.editor_settings.draw_theme();
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(draw.background))
            .show(ui, |ui| {
                *ui.visuals_mut() = if draw.light {
                    egui::Visuals::light()
                } else {
                    // Keep the app theme (rounded widgets, accent) inside the
                    // dark editor instead of stock egui dark.
                    ui.ctx().style_of(egui::Theme::Dark).visuals.clone()
                };
                ui.visuals_mut().override_text_color = Some(draw.foreground);
                ui.add_space(4.0);
                self.editor_settings_panel(ui);
                let menu_action = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let sliders = self.editor_decorations.sliders().to_vec();
                        let widgets = self.editor_decorations.widgets().to_vec();
                        let backdrop = self.editor_decorations.backdrop().cloned();
                        let hydra_time = self.hydra_epoch.elapsed().as_secs_f64();
                        // What hydra's `a` hears this frame, for any function
                        // argument that reads `a.fft`.
                        if !self.hydra_params.is_empty()
                            && let Some(engine) = &self.engine
                        {
                            rudel_lang::hydra::set_loudness(&super::hydra_audio::tap_loudness(
                                engine.scope_taps().master(),
                                engine.sample_rate(),
                            ));
                        }
                        let cycle = self.playback_position_cycles().unwrap_or(0.0);
                        // Strudel's draw canvas: one frame of the script's
                        // painters, at the size the editor showed last frame.
                        let rect = ui
                            .ctx()
                            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(CANVAS_RECT)))
                            .unwrap_or(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1280.0, 720.0),
                            ));
                        let (w, h) = (rect.width().round() as u32, rect.height().round() as u32);
                        rudel_lang::canvas::set_size(w, h);
                        if let Some(driver) = self.canvas_driver {
                            let ms = hydra_time * 1000.0;
                            let events = self.canvas.events(ui.ctx(), rect);
                            match driver.frame(cycle, ms, w, h, &events) {
                                Some(ops) => self.canvas.draw(ui.ctx(), &ops, w, h),
                                None => self.canvas_driver = None,
                            }
                        }
                        let canvas = self.canvas_driver.and(self.canvas.texture());
                        let hydra_values: Vec<Option<f64>> = self
                            .hydra_params
                            .iter()
                            .map(|param| param.value(hydra_time, cycle))
                            .collect();
                        let current_pattern = self.current.clone();
                        let playback_position_cycles = self.playback_position_cycles();
                        let insert_text = self.pending_insert.take();
                        let editor_output = code_editor(
                            ui,
                            &mut self.code,
                            CodeEditorInput {
                                active: active_spans,
                                idents: &self.highlight_idents,
                                reference: &self.reference,
                                sample_names: &self.sample_names,
                                current_pattern: current_pattern.as_ref(),
                                pattern_generation: self.pattern_generation,
                                playback_position_cycles,
                                scope_taps: self.engine.as_ref().map(|e| e.scope_taps()),
                                gpu_available,
                                sliders: &sliders,
                                widgets: &widgets,
                                backdrop: backdrop.as_ref(),
                                hydra_time,
                                hydra_values: &hydra_values,
                                draw_canvas: canvas,
                                widget_host: &mut self.widget_host,
                                settings: &self.editor_settings,
                                insert_text,
                            },
                        );
                        if let Some(change) = editor_output.text_change {
                            self.editor_decorations.map_change(change);
                            self.blocks.shift(change.from, change.to, change.insert_len);
                        }
                        if let Some(update) = editor_output.slider_update {
                            self.editor_decorations
                                .set_slider_literal(&update.id, update.insert);
                        }
                        if let Some(cursor) = editor_output.cursor_byte {
                            self.editor_cursor_byte = cursor;
                        }
                        editor_output.action
                    });
                // Outside the ScrollArea closure: these borrow all of `self`.
                if let Some(action) = menu_action.inner {
                    match action {
                        EditorAction::Evaluate => self.evaluate(),
                        EditorAction::EvaluateBlock => self.evaluate_current_block(),
                        EditorAction::PrimaryEval => self.primary_eval(),
                        EditorAction::Hush => self.hush(),
                        EditorAction::Panic => self.panic(),
                    }
                }
            });
    }

    fn editor_settings_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("editor settings")
            .id_salt("editor_settings")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.editor_settings.line_wrapping, "wrap");
                    ui.checkbox(&mut self.editor_settings.bracket_matching, "match");
                    ui.checkbox(&mut self.editor_settings.bracket_closing, "close");
                    ui.checkbox(&mut self.editor_settings.line_numbers, "lines");
                    ui.checkbox(&mut self.editor_settings.active_line, "active line");
                    ui.checkbox(&mut self.editor_settings.autocomplete, "complete");
                    ui.checkbox(&mut self.editor_settings.pattern_highlighting, "highlight");
                    ui.checkbox(&mut self.editor_settings.flash, "flash");
                    ui.checkbox(&mut self.editor_settings.tooltips, "tooltips");
                    ui.checkbox(&mut self.editor_settings.tab_indentation, "tab indent");
                    ui.checkbox(&mut self.editor_settings.block_based_eval, "block eval");
                });

                ui.horizontal(|ui| {
                    ui.label("theme");
                    egui::ComboBox::from_id_salt("editor_theme")
                        .selected_text(self.editor_settings.theme.label())
                        .show_ui(ui, |ui| {
                            for theme in EditorTheme::all() {
                                ui.selectable_value(
                                    &mut self.editor_settings.theme,
                                    theme,
                                    theme.label(),
                                );
                            }
                        });
                    if let Some(dir) = crate::editor::themes::user_dir() {
                        let button = ui.small_button("folder").on_hover_text(format!(
                            "Your own themes: a .toml file each, read at launch\n{}",
                            dir.display()
                        ));
                        if button.clicked() {
                            open_folder(&dir);
                        }
                    }

                    ui.label("keys");
                    egui::ComboBox::from_id_salt("editor_keymap")
                        .selected_text(self.editor_settings.keymap.label())
                        .show_ui(ui, |ui| {
                            for keymap in crate::editor::keymap::Keymap::ALL {
                                ui.selectable_value(
                                    &mut self.editor_settings.keymap,
                                    keymap,
                                    keymap.label(),
                                );
                            }
                        });

                    ui.label("font");
                    egui::ComboBox::from_id_salt("editor_font_family")
                        .selected_text(self.editor_settings.font_family.label())
                        .show_ui(ui, |ui| {
                            for family in EditorFontFamily::all() {
                                ui.selectable_value(
                                    &mut self.editor_settings.font_family,
                                    family,
                                    family.label(),
                                );
                            }
                        });

                    let mut size = self.editor_settings.font_size as f64;
                    let response = ui.add(
                        egui::Slider::new(&mut self.editor_settings.font_size, 11.0..=32.0)
                            .text("size")
                            .step_by(1.0),
                    );
                    if crate::scroll::scroll_adjust(ui, &response, &mut size, 11.0..=32.0, 1.0) {
                        self.editor_settings.font_size = size as f32;
                    }
                });
            });
    }

    /// Current playback position in (fractional) cycles, or `None` when
    /// stopped. Uses the audio clock when an audio device is present, and
    /// otherwise falls back to a wall clock from when Play was pressed so that
    /// MIDI/OSC-only playback still drives the playhead and highlighting.
    fn playback_position_cycles(&self) -> Option<f64> {
        if !self.playing {
            return None;
        }
        if let Some(engine) = &self.engine {
            return Some(engine.position_cycles());
        }
        self.play_start
            .map(|start| start.elapsed().as_secs_f64() * self.cps)
    }

    /// Source byte ranges of the haps sounding at the current playback
    /// position, for active-event highlighting in the editor. Like Strudel,
    /// only discrete events (haps with a `whole`) flash — continuous signals
    /// are skipped — and an event flashes for the span of its `whole`.
    fn active_source_spans(&mut self) -> Vec<FlashSpan> {
        let (Some(pat), Some(pos)) = (&self.current, self.playback_position_cycles()) else {
            return Vec::new();
        };
        // Only the whole-cycle query is expensive, and it changes once per
        // cycle rather than once per repaint — the same reason the inline
        // widgets cache their haps.
        let cycle = pos.floor();
        let key = (self.pattern_generation, cycle.to_bits());
        match &self.flash_cache {
            Some((cached, _)) if *cached == key => {}
            _ => self.flash_cache = Some((key, cycle_flashes(pat, cycle))),
        }
        let (_, flashes) = self.flash_cache.as_ref().expect("just populated");
        spans_at(flashes, pos)
    }

    fn active_editor_spans(&mut self) -> Vec<FlashSpan> {
        if !self.editor_settings.flash {
            self.editor_decorations.set_flash_ranges_from_eval(&[]);
            self.block_flash = None;
            return Vec::new();
        }

        let eval_spans = self.active_source_spans();
        self.editor_decorations
            .set_flash_ranges_from_eval(&eval_spans);
        let mut spans = self.editor_decorations.flash_ranges();
        if let Some((range, started)) = self.block_flash {
            if started.elapsed() <= std::time::Duration::from_millis(200) {
                spans.push((range.from, range.to, None));
            } else {
                self.block_flash = None;
            }
        }
        spans
    }

    fn primary_eval(&mut self) {
        if self.editor_settings.block_based_eval {
            self.evaluate_current_block();
        } else {
            self.evaluate();
        }
    }

    fn secondary_eval(&mut self) {
        if self.editor_settings.block_based_eval {
            self.evaluate();
        } else {
            self.evaluate_current_block();
        }
    }
}

/// Collapsed-header summary for the I/O section, so what's loaded/connected
/// shows at a glance without opening it.
/// Show `dir` in the system's file manager, making it first if need be.
fn open_folder(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}

fn io_summary(sample_count: usize, midi_in_connected: bool, midi_in_connecting: bool) -> String {
    let midi = if midi_in_connecting {
        "midi in connecting…"
    } else if midi_in_connected {
        "midi in connected"
    } else {
        "midi in off"
    };
    format!("i/o — {sample_count} samples · {midi}")
}

/// `(label, shortcut-tooltip)` for the primary (Ctrl+Enter) and secondary
/// (Ctrl+Shift+Enter) eval buttons. Which action each triggers swaps with the
/// block-based-eval setting; the shortcut binding stays fixed.
type ButtonLabel = (&'static str, &'static str);
fn eval_button_labels(block_based_eval: bool) -> (ButtonLabel, ButtonLabel) {
    if block_based_eval {
        (("Block", "Ctrl+Enter"), ("Eval", "Ctrl+Shift+Enter"))
    } else {
        (("Eval", "Ctrl+Enter"), ("Block", "Ctrl+Shift+Enter"))
    }
}

/// Byte indices in `name` of `query`'s chars matched in order
/// (case-insensitive subsequence), or `None` when `query` doesn't match.
/// An empty query never matches; callers treat that as "not filtering".
fn fuzzy_match(name: &str, query: &str) -> Option<Vec<usize>> {
    let mut hits = Vec::new();
    // ponytail: ASCII case folding only — reference names are ASCII.
    let mut wanted = query.chars().map(|c| c.to_ascii_lowercase());
    let mut want = wanted.next()?;
    for (i, ch) in name.char_indices() {
        if ch.to_ascii_lowercase() == want {
            hits.push(i);
            match wanted.next() {
                Some(next) => want = next,
                None => return Some(hits),
            }
        }
    }
    None
}

/// Filter `items` by [`fuzzy_match`] against `query`, keeping list order. An
/// empty query keeps everything with no hit indices (rendered unhighlighted).
fn fuzzy_filter<'a>(
    items: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Vec<(&'a str, Vec<usize>)> {
    let items = items.into_iter();
    if query.is_empty() {
        return items.map(|item| (item, Vec::new())).collect();
    }
    items
        .filter_map(|item| fuzzy_match(item, query).map(|hits| (item, hits)))
        .collect()
}

/// One reference section's entries, culled to the scroll viewport: rows above
/// and below it become a single blank space each.
///
/// Every entry is a `dnd_drag_source` around a `Label`, and egui charges per
/// widget per frame (id-clash check, widget_info, accesskit registration)
/// whether or not the widget is visible. With the sample banks loaded that is
/// thousands of widgets a frame for ~20 visible rows — profiling put this whole
/// panel at 45% of the process's CPU. Rows are uniform monospace, so the
/// visible range is arithmetic.
fn reference_items(ui: &mut egui::Ui, items: &[(&str, Vec<usize>)]) -> Option<String> {
    let row = ui.text_style_height(&egui::TextStyle::Monospace) + ui.spacing().item_spacing.y;
    let view = ui.clip_rect();
    let (first, last) = visible_rows(
        view.top() - ui.cursor().top(),
        view.height(),
        row,
        items.len(),
    );

    let mut insert = None;
    ui.add_space(first as f32 * row);
    for (item, hits) in &items[first..last] {
        if let Some(text) = reference_item(ui, item, hits) {
            insert = Some(text);
        }
    }
    ui.add_space((items.len() - last) as f32 * row);
    insert
}

/// The rows of a uniform list that land in a viewport `from` below the list's
/// top and `height` tall, as `first..last`. One extra row is kept at the
/// bottom so a row scrolling in is drawn before it shows.
fn visible_rows(from: f32, height: f32, row: f32, len: usize) -> (usize, usize) {
    let first = ((from / row).floor().max(0.0) as usize).min(len);
    let last = ((((from + height) / row).ceil().max(0.0)) as usize + 1).min(len);
    (first, last)
}

/// A reference list entry: draggable into the editor, double-click to insert
/// at the cursor. Returns the name when it was double-clicked this frame.
fn reference_item(ui: &mut egui::Ui, name: &str, hits: &[usize]) -> Option<String> {
    let id = ui.id().with(name);
    let response = ui.dnd_drag_source(id, name.to_string(), |ui| fuzzy_label(ui, name, hits));
    response.inner.double_clicked().then(|| name.to_string())
}

/// A monospace label with the fuzzy-matched chars tinted like a hyperlink.
fn fuzzy_label(ui: &mut egui::Ui, name: &str, hits: &[usize]) -> egui::Response {
    let text: egui::WidgetText = if hits.is_empty() {
        egui::RichText::new(name).monospace().into()
    } else {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        fuzzy_job(
            name,
            hits,
            egui::text::TextFormat::simple(font.clone(), ui.visuals().text_color()),
            egui::text::TextFormat::simple(font, ui.visuals().hyperlink_color),
        )
        .into()
    };
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        .on_hover_text("double-click to insert · drag into the editor")
}

/// Split `name` into alternating normal and highlighted runs, `hits` being the
/// byte offsets of the matched characters. Separate from [`fuzzy_label`] so the
/// segmentation can be checked without a `Ui` to paint into.
fn fuzzy_job(
    name: &str,
    hits: &[usize],
    normal: egui::text::TextFormat,
    hit: egui::text::TextFormat,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut last = 0;
    for &start in hits {
        if start > last {
            job.append(&name[last..start], 0.0, normal.clone());
        }
        let end = start + name[start..].chars().next().map_or(1, char::len_utf8);
        job.append(&name[start..end], 0.0, hit.clone());
        last = end;
    }
    if last < name.len() {
        job.append(&name[last..], 0.0, normal);
    }
    job
}

/// One cycle's discrete events, reduced to what the flash needs: each event's
/// `whole` span and the source spans it lights up. Querying a cycle is the
/// expensive half and only changes once per cycle, so it is cached
/// ([`RudelApp::flash_cache`]) while `pos` sweeps across it.
pub(crate) type CycleFlashes = Vec<(rudel_core::Frac, rudel_core::Frac, Vec<FlashSpan>)>;

fn cycle_flashes(pat: &rudel_core::Pattern, cycle: f64) -> CycleFlashes {
    pat.query_arc(
        rudel_core::Frac::from_f64(cycle),
        rudel_core::Frac::from_f64(cycle + 1.0),
    )
    .into_iter()
    .filter_map(|h| {
        let whole = h.whole?;
        let color = crate::editor::mark_color(&h).map(crate::editor::pack_color);
        let spans = h
            .context
            .locations
            .iter()
            .map(|&(from, to)| (from, to, color))
            .collect();
        Some((whole.begin, whole.end, spans))
    })
    .collect()
}

/// The deduped source byte ranges sounding at `pos`, out of a cycle already
/// reduced by [`cycle_flashes`].
fn spans_at(flashes: &CycleFlashes, pos: f64) -> Vec<FlashSpan> {
    let pos_f = rudel_core::Frac::from_f64(pos);
    let mut spans: Vec<FlashSpan> = flashes
        .iter()
        .filter(|(begin, end, _)| *begin <= pos_f && pos_f < *end)
        .flat_map(|(_, _, spans)| spans.iter().copied())
        .collect();
    spans.sort_unstable();
    spans.dedup();
    spans
}

/// The deduped source byte ranges of the discrete events sounding at cycle
/// position `pos`. Factored out of [`RudelApp::active_source_spans`] so it can
/// be tested without a running engine; the app itself goes through the cache,
/// and this stays as the uncached reference that cache is checked against.
#[cfg(test)]
fn active_source_spans_at(pat: &rudel_core::Pattern, pos: f64) -> Vec<FlashSpan> {
    spans_at(&cycle_flashes(pat, pos.floor()), pos)
}

#[cfg(test)]
mod tests {
    use super::{
        RudelApp, active_source_spans_at, eval_button_labels, fuzzy_filter, fuzzy_job, fuzzy_match,
        io_summary,
    };
    use crate::editor::decorations::SourceRange;
    use eframe::egui;
    use std::time::{Duration, Instant};

    /// A headless app with a pattern evaluated and the wall clock as its
    /// playhead (there is no audio device here).
    fn playing_app(started_ago: Duration, cps: f64) -> RudelApp {
        let mut app = RudelApp::headless();
        app.code = r#"s("bd sd")"#.to_string();
        app.evaluate();
        assert_eq!(app.eval_error, None, "the fixture pattern should evaluate");
        app.playing = true;
        app.play_start = Some(Instant::now() - started_ago);
        app.cps = cps;
        app
    }

    /// [`playing_app`] with `script`'s trigger hooks attached, the playhead at
    /// `pos` cycles and everything up to `fired_upto` already fired.
    fn triggering_app(fired_upto: f64, pos: f64) -> RudelApp {
        // The callback throws for the *first* event of each cycle only, so the
        // error afterwards says whether that particular onset fired — which
        // presence of a log line could not, since every event logs alike.
        let script = r#"n("0 1 2 3").onTriggerTime(h => { if (h.value.n == 0) throw 1 })"#;
        let result = rudel_lang::eval_result(script).expect("script");
        let mut app = playing_app(Duration::from_secs_f64(pos), 1.0);
        app.trigger_hooks = result.trigger_hooks;
        app.current = Some(result.pattern);
        app.trigger_fired_upto = Some(fired_upto);
        app.eval_error = None;
        app
    }

    #[test]
    fn a_trigger_fires_for_an_onset_at_the_mark_but_not_behind_it() {
        // The window is half-open from the mark, so an onset landing exactly
        // on it still fires.
        let mut app = triggering_app(1.0, 1.1);
        app.fire_trigger_hooks();
        assert!(app.eval_error.is_some(), "the onset at the mark fires");

        // Starting mid-event, the event already under way has its onset behind
        // the mark and must not fire again — that is what stops a seek, or one
        // slow frame, replaying a cycle of callbacks at once.
        let mut app = triggering_app(1.1, 1.3);
        app.fire_trigger_hooks();
        assert_eq!(app.eval_error, None, "the event under way already fired");
    }

    #[test]
    fn speech_alone_is_enough_to_follow_the_playhead() {
        // No hooks, but `.speak` haps: the mark still has to move, or the
        // first spoken frame would replay everything behind it.
        let mut app = triggering_app(0.0, 1.1);
        app.trigger_hooks = Default::default();
        app.trigger_fired_upto = None;
        app.fire_trigger_hooks();
        assert_eq!(
            app.trigger_fired_upto, None,
            "nothing to fire, nothing to track"
        );
        app.speaks = true;
        app.fire_trigger_hooks();
        assert!(app.trigger_fired_upto.is_some());
    }

    #[test]
    fn the_first_frame_after_evaluating_only_sets_the_mark() {
        let mut app = triggering_app(0.0, 1.1);
        app.trigger_fired_upto = None;
        app.fire_trigger_hooks();
        assert_eq!(app.eval_error, None, "nothing fires on the first frame");
        assert!(app.trigger_fired_upto.is_some(), "but the mark is set");

        // A backwards seek is the same case: the mark follows the playhead
        // back, and nothing between the two fires on the way.
        let mut app = triggering_app(5.0, 1.1);
        app.fire_trigger_hooks();
        assert_eq!(app.eval_error, None, "nothing fires backwards");
        assert_eq!(
            app.trigger_fired_upto.map(|p| p < 2.0),
            Some(true),
            "the mark follows the playhead back"
        );
    }

    #[test]
    fn the_playhead_advances_with_the_tempo_only_while_playing() {
        // With no audio device the position comes off a wall clock started when
        // Play was pressed, scaled by the tempo.
        let mut app = playing_app(Duration::from_secs(2), 0.5);
        let pos = app.playback_position_cycles().expect("a position");
        assert!(
            (pos - 1.0).abs() < 0.05,
            "two seconds at 0.5 cps is one cycle, got {pos}"
        );

        app.cps = 2.0;
        let faster = app.playback_position_cycles().expect("a position");
        assert!(
            (faster - 4.0).abs() < 0.2,
            "...and four cycles at 2 cps, got {faster}"
        );

        app.playing = false;
        assert_eq!(
            app.playback_position_cycles(),
            None,
            "stopped: there is no playhead"
        );

        app.playing = true;
        app.play_start = None;
        assert_eq!(
            app.playback_position_cycles(),
            None,
            "playing but never started: still none"
        );
    }

    #[test]
    fn nothing_flashes_without_both_a_pattern_and_a_playhead() {
        let mut app = playing_app(Duration::ZERO, 1.0);
        assert!(
            !app.active_source_spans().is_empty(),
            "the event under the playhead should flash"
        );

        app.playing = false;
        assert!(
            app.active_source_spans().is_empty(),
            "stopped: nothing flashes"
        );

        app.playing = true;
        app.current = None;
        assert!(
            app.active_source_spans().is_empty(),
            "no pattern: nothing flashes"
        );
    }

    /// The flash cache holds a whole cycle, so the danger is it freezing the
    /// highlight: within one cached cycle the spans must still follow the
    /// playhead, and a re-eval must throw the cycle away.
    #[test]
    fn the_flash_cache_still_tracks_the_playhead_and_drops_on_re_eval() {
        // `s("bd sd")` at 1 cps: bd sounds over [0, 0.5), sd over [0.5, 1) —
        // both inside cycle 0, so both reads hit the same cached cycle.
        let mut app = playing_app(Duration::from_millis(250), 1.0);
        let on_bd = app.active_source_spans();
        app.play_start = Some(Instant::now() - Duration::from_millis(750));
        let on_sd = app.active_source_spans();

        assert!(!on_bd.is_empty() && !on_sd.is_empty(), "both events flash");
        assert_ne!(
            on_bd, on_sd,
            "a cached cycle must not freeze the flash on one event"
        );
        let pat = app.current.clone().expect("a pattern");
        assert_eq!(
            on_sd,
            active_source_spans_at(&pat, 0.75),
            "matches uncached"
        );

        // Re-evaluating replaces the pattern: the cached cycle is from the old
        // one and has to go, or the editor keeps flashing bytes that moved.
        app.code = r#"s("~ sd")"#.to_string();
        app.evaluate();
        app.play_start = Some(Instant::now() - Duration::from_millis(250));
        assert!(
            app.active_source_spans().is_empty(),
            "the new pattern rests where the old one had bd"
        );
    }

    #[test]
    fn the_flash_setting_switches_the_highlight_off_entirely() {
        let mut app = playing_app(Duration::ZERO, 1.0);
        app.editor_settings.flash = true;
        assert!(
            !app.active_editor_spans().is_empty(),
            "flash on: the active event highlights"
        );

        app.editor_settings.flash = false;
        assert!(
            app.active_editor_spans().is_empty(),
            "flash off: nothing highlights"
        );
    }

    #[test]
    fn a_block_flash_fades_after_its_two_hundred_milliseconds() {
        // Evaluating a block flashes it briefly. The window is checked against
        // the elapsed time, and expiring it also has to clear the state or the
        // check would run forever.
        let mut app = RudelApp::headless();
        app.editor_settings.flash = true;
        let range = SourceRange::new(0, 3);

        app.block_flash = Some((range, Instant::now()));
        assert_eq!(
            app.active_editor_spans(),
            vec![(0, 3, None)],
            "a fresh block flash is shown"
        );
        assert!(app.block_flash.is_some(), "and stays pending");

        app.block_flash = Some((range, Instant::now() - Duration::from_millis(500)));
        assert!(
            app.active_editor_spans().is_empty(),
            "a stale one is dropped"
        );
        assert!(app.block_flash.is_none(), "...and cleared, not re-checked");
    }

    /// `(text, is_highlighted)` for each run the fuzzy label would paint.
    fn runs(name: &str, hits: &[usize]) -> Vec<(String, bool)> {
        let normal = egui::text::TextFormat::simple(Default::default(), egui::Color32::WHITE);
        let hit = egui::text::TextFormat::simple(Default::default(), egui::Color32::RED);
        let job = fuzzy_job(name, hits, normal, hit);
        job.sections
            .iter()
            .map(|s| {
                (
                    job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(),
                    s.format.color == egui::Color32::RED,
                )
            })
            .collect()
    }

    #[test]
    fn a_fuzzy_label_highlights_exactly_the_matched_characters() {
        // The runs have to tile the name: every character appears once, and
        // only the matched ones are picked out.
        assert_eq!(
            runs("sound", &[0, 2]),
            vec![
                ("s".to_string(), true),
                ("o".to_string(), false),
                ("u".to_string(), true),
                ("nd".to_string(), false),
            ]
        );
        // Adjacent hits do not emit an empty run between them; the job merges
        // them into one highlighted stretch.
        assert_eq!(
            runs("sound", &[0, 1]),
            vec![("so".to_string(), true), ("und".to_string(), false)]
        );
        // A hit on the final character leaves no trailing run.
        assert_eq!(
            runs("ab", &[1]),
            vec![("a".to_string(), false), ("b".to_string(), true)]
        );
        // Whatever the hits, the runs reassemble into the original name.
        for hits in [vec![], vec![0], vec![1, 3], vec![0, 1, 2, 3, 4]] {
            let joined: String = runs("sound", &hits).into_iter().map(|(t, _)| t).collect();
            assert_eq!(joined, "sound", "hits {hits:?}");
        }
    }

    #[test]
    fn a_fuzzy_hit_takes_a_whole_character_not_a_byte() {
        // Byte offsets into a name with a multi-byte character: highlighting
        // one byte of it would slice mid-character and panic.
        assert_eq!(
            runs("éx", &[0]),
            vec![("é".to_string(), true), ("x".to_string(), false)]
        );
    }

    #[test]
    fn io_summary_reflects_connection_state() {
        assert_eq!(io_summary(0, false, false), "i/o — 0 samples · midi in off");
        assert_eq!(
            io_summary(12, true, false),
            "i/o — 12 samples · midi in connected"
        );
        assert_eq!(
            io_summary(3, false, true),
            "i/o — 3 samples · midi in connecting…"
        );
    }

    #[test]
    fn fuzzy_match_finds_case_insensitive_subsequences() {
        // contiguous and gapped subsequences, with byte indices of the hits
        assert_eq!(fuzzy_match("supersaw", "saw"), Some(vec![0, 6, 7]));
        assert_eq!(fuzzy_match("RolandTR909", "rtr9"), Some(vec![0, 6, 7, 8]));
        // chars must appear in order
        assert_eq!(fuzzy_match("saw", "was"), None);
        assert_eq!(fuzzy_match("bd", "bdx"), None);
        // empty query never matches (callers treat empty as "not filtering")
        assert_eq!(fuzzy_match("bd", ""), None);
    }

    #[test]
    fn fuzzy_filter_keeps_order_and_passes_everything_on_empty_query() {
        let items = ["bd", "sd", "hh"];
        let all = fuzzy_filter(items, "");
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|(_, hits)| hits.is_empty()));

        let filtered = fuzzy_filter(items, "d");
        assert_eq!(filtered, vec![("bd", vec![1]), ("sd", vec![1])]);
    }

    #[test]
    fn active_spans_flash_discrete_events_at_position() {
        // s("bd sd"): `bd` (bytes 3..5) sounds in [0,0.5), `sd` (6..8) in [0.5,1).
        let pat = rudel_lang::eval(r#"s("bd sd")"#).expect("eval");
        assert_eq!(active_source_spans_at(&pat, 0.25), vec![(3, 5, None)]);
        assert_eq!(active_source_spans_at(&pat, 0.75), vec![(6, 8, None)]);
        // the same structure repeats every cycle, so cycle 2 maps identically
        assert_eq!(active_source_spans_at(&pat, 2.25), vec![(3, 5, None)]);
    }

    #[test]
    fn markcss_and_color_set_the_flash_colour() {
        // `markcss` wins, and a colour is picked out of the CSS declaration.
        let pat =
            rudel_lang::eval(r#"s("bd").color("blue").markcss('outline: solid 2px #ff0000')"#)
                .expect("eval");
        let red = crate::editor::pack_color(eframe::egui::Color32::from_rgb(0xff, 0, 0));
        assert!(
            active_source_spans_at(&pat, 0.25)
                .iter()
                .all(|&(_, _, c)| c == Some(red))
        );
        // With no `markcss`, the `color` control colours the flash instead.
        let pat = rudel_lang::eval(r#"s("bd").color("blue")"#).expect("eval");
        let blue = crate::editor::pack_color(eframe::egui::Color32::from_rgb(0, 0, 0xff));
        assert!(
            active_source_spans_at(&pat, 0.25)
                .iter()
                .all(|&(_, _, c)| c == Some(blue))
        );
        // CSS with no colour in it leaves the theme's flash alone.
        let pat =
            rudel_lang::eval(r#"s("bd").markcss('text-decoration: underline')"#).expect("eval");
        assert!(
            active_source_spans_at(&pat, 0.25)
                .iter()
                .all(|&(_, _, c)| c.is_none())
        );
    }

    #[test]
    fn continuous_signals_do_not_flash() {
        // a continuous signal produces haps with no `whole`, so the `whole`
        // filter keeps them from flashing even though they are always "active".
        let pat = rudel_lang::eval("note(sine)").expect("eval");
        let haps = pat.query_arc(rudel_core::Frac::zero(), rudel_core::Frac::one());
        assert!(
            haps.iter().all(|h| h.whole.is_none()),
            "expected analog haps"
        );
        assert!(active_source_spans_at(&pat, 0.3).is_empty());
    }

    #[test]
    fn eval_button_labels_follow_block_based_setting() {
        assert_eq!(
            eval_button_labels(false),
            (("Eval", "Ctrl+Enter"), ("Block", "Ctrl+Shift+Enter"))
        );
        assert_eq!(
            eval_button_labels(true),
            (("Block", "Ctrl+Enter"), ("Eval", "Ctrl+Shift+Enter"))
        );
    }
}

/// Publish this frame's pointer position and held keys to `rudel-core`'s input
/// bus, where the `mousex`/`mousey`/`keyDown` signals read them at query time.
/// Strudel gets these from `document` listeners; the egui window is the source
/// here.
fn pump_input_bus(ctx: &egui::Context) {
    ctx.input(|i| {
        let rect = i.viewport_rect();
        if let Some(p) = i.pointer.latest_pos() {
            rudel_core::set_pointer(
                ((p.x - rect.left()) / rect.width().max(1.0)) as f64,
                ((p.y - rect.top()) / rect.height().max(1.0)) as f64,
            );
        }
        // egui reports modifiers separately from `keys_down`; the browser
        // reports them as keys of their own, which is what patterns name.
        let mods = [
            (i.modifiers.ctrl, "Control"),
            (i.modifiers.shift, "Shift"),
            (i.modifiers.alt, "Alt"),
            (i.modifiers.mac_cmd, "Meta"),
        ];
        let held = i.keys_down.iter().map(|k| browser_key_name(*k)).chain(
            mods.into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, n)| n.to_string()),
        );
        rudel_core::set_keys_held(held);
    });
}

/// Name an egui key the way a browser's `KeyboardEvent.key` would, so patterns
/// name keys identically in Rudel and Strudel. egui's own names already agree
/// for most keys (`Enter`, `Escape`, `Tab`, the digits, the function keys);
/// only the arrows, space and the letters differ.
fn browser_key_name(key: egui::Key) -> String {
    use egui::Key;
    match key {
        Key::ArrowDown => "ArrowDown".to_string(),
        Key::ArrowUp => "ArrowUp".to_string(),
        Key::ArrowLeft => "ArrowLeft".to_string(),
        Key::ArrowRight => "ArrowRight".to_string(),
        Key::Space => " ".to_string(),
        other => {
            // Letters are "A".."Z" in egui; the browser reports the unshifted
            // character, with `Shift` held separately (as it is here).
            let name = other.name();
            // Lowercasing a one-character name that is not a letter leaves it.
            if name.len() == 1 {
                name.to_lowercase()
            } else {
                name.to_string()
            }
        }
    }
}

#[cfg(test)]
mod input_bus_tests {
    use super::{browser_key_name, pump_input_bus, spans_at};
    use eframe::egui::{self, Key};
    use rudel_core::Frac;

    #[test]
    fn the_pointer_is_normalised_against_the_window() {
        // `mousex`/`mousey` are 0..1 across the window, as Strudel's are
        // across the body — so the window's own origin has to come off the
        // pointer position before the division, and the division has to be by
        // the window's size.
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(200.0, 100.0));
        // `run_ui` hands back texture deltas that panic on drop if nobody
        // takes them; there is no painter here to apply them.
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![egui::Event::PointerMoved(egui::pos2(150.0, 100.0))],
                ..Default::default()
            },
            |_| {},
        );
        out.textures_delta.clear();
        pump_input_bus(&ctx);
        let (x, y) = rudel_core::get_pointer();
        assert!((x - 0.25).abs() < 1e-6, "x was {x}");
        assert!((y - 0.5).abs() < 1e-6, "y was {y}");
    }

    #[test]
    fn a_span_sounds_from_its_onset_up_to_its_end() {
        // Half-open, like every other span in the engine: the flash starts on
        // the onset and is gone by the end, or two events back to back both
        // light up on the frame they change over.
        let flashes = vec![
            (Frac::from_f64(0.0), Frac::from_f64(0.5), vec![(0, 3, None)]),
            (Frac::from_f64(0.5), Frac::from_f64(1.0), vec![(4, 7, None)]),
        ];
        assert_eq!(spans_at(&flashes, 0.25), vec![(0, 3, None)]);
        assert_eq!(spans_at(&flashes, 0.5), vec![(4, 7, None)]);
        assert_eq!(spans_at(&flashes, 1.0), vec![]);
    }

    #[test]
    fn keys_are_named_as_the_browser_does() {
        // The names patterns use are `KeyboardEvent.key` values, so a pattern
        // written for Strudel names the same keys in Rudel.
        assert_eq!(browser_key_name(Key::J), "j");
        assert_eq!(browser_key_name(Key::Num4), "4");
        assert_eq!(browser_key_name(Key::Space), " ");
        assert_eq!(browser_key_name(Key::ArrowDown), "ArrowDown");
        assert_eq!(browser_key_name(Key::Enter), "Enter");
        assert_eq!(browser_key_name(Key::Escape), "Escape");
    }
}

#[cfg(test)]
mod visible_rows_tests {
    use super::visible_rows;

    #[test]
    fn only_the_rows_in_view_and_one_past_it_are_drawn() {
        // 10px rows, a 35px view: from the top, rows 0..=3 show and 4 is kept.
        assert_eq!(visible_rows(0.0, 35.0, 10.0, 100), (0, 5));
        // Scrolled 25px down, rows 2..=5 show.
        assert_eq!(visible_rows(25.0, 35.0, 10.0, 100), (2, 7));
        // Above the list only the spare row is drawn; past its end, nothing.
        assert_eq!(visible_rows(-50.0, 35.0, 10.0, 100), (0, 1));
        assert_eq!(visible_rows(5000.0, 35.0, 10.0, 100), (100, 100));
        // A short list is drawn whole.
        assert_eq!(visible_rows(0.0, 35.0, 10.0, 2), (0, 2));
    }
}

#[cfg(test)]
mod reference_items_tests {
    use super::reference_items;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    /// A thousand entries in a scroll area scrolled to `offset`. The state is
    /// the height the list laid out and the row height it should be built of:
    /// a monospace line plus the item spacing.
    fn list(offset: f32) -> Harness<'static, (f32, f32)> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 200.0))
            .build_ui_state(
                move |ui, state: &mut (f32, f32)| {
                    state.1 = ui.text_style_height(&egui::TextStyle::Monospace)
                        + ui.spacing().item_spacing.y;
                    egui::ScrollArea::vertical()
                        .vertical_scroll_offset(offset)
                        .show(ui, |ui| {
                            let names: Vec<String> =
                                (0..1000).map(|i| format!("entry{i}")).collect();
                            let items: Vec<(&str, Vec<usize>)> =
                                names.iter().map(|n| (n.as_str(), Vec::new())).collect();
                            let top = ui.cursor().top();
                            reference_items(ui, &items);
                            state.0 = ui.cursor().top() - top;
                        });
                },
                (0.0, 0.0),
            );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn a_culled_list_keeps_its_full_height_and_draws_what_is_in_view() {
        let top = list(0.0);
        let (height, row) = *top.state();
        // The blank space stands in for every row not drawn.
        // Within a row: the spacing after the last item is not laid out.
        assert!(
            (height - row * 1000.0).abs() < row,
            "{height} for rows of {row}"
        );
        top.get_by_label("entry0");
        assert!(top.query_by_label("entry999").is_none());
        // Scrolled half way down, the rows there are the ones drawn.
        let middle = list(row * 500.0);
        middle.get_by_label("entry502");
        assert!(middle.query_by_label("entry0").is_none());
    }
}
