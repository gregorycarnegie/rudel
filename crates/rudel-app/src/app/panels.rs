use super::{Output, RudelApp};
use crate::editor::settings::{EditorFontFamily, EditorTheme};
use crate::editor::{CodeEditorInput, code_editor};
use crate::reference::{CONTROLS, DRUMS, FACTORIES, SIGNALS, WAVEFORMS};
use crate::volume::vlc_volume_slider;
use eframe::egui;

impl eframe::App for RudelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_sample_jobs(ui.ctx());

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

        let active_spans = self.active_editor_spans();
        self.transport_panel(ui);
        self.errors_panel(ui);
        self.reference_panel(ui);
        self.editor_panel(ui, &active_spans);

        // Clock-in: follow the incoming MIDI clock tempo (4 beats per cycle).
        if self.clock_sync {
            let cps = self.midi_in.as_ref().and_then(|i| i.cps(4.0));
            if let Some(cps) = cps
                && (cps - self.cps).abs() > 1e-4
            {
                self.set_cps(cps);
            }
        }

        // Keep the playhead moving while playing (and polling clock / CC input).
        if self.playing || !self.sample_jobs.is_empty() || self.clock_sync || self.midi_in.is_some()
        {
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
                let (primary_eval_label, secondary_eval_label) =
                    eval_button_labels(self.editor_settings.block_based_eval);
                if ui.button(primary_eval_label).clicked() {
                    self.primary_eval();
                }
                if ui.button(secondary_eval_label).clicked() {
                    self.secondary_eval();
                }
                if ui.button("Hush (Ctrl+.)").clicked() {
                    self.hush();
                }
                if ui.button("Panic (Ctrl+Shift+.)").clicked() {
                    self.panic();
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
                if ui
                    .button(if connected { "Reconnect" } else { "Connect" })
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
                ui.weak(
                    "Ctrl+Enter eval · Ctrl+Shift+Enter block · Ctrl+. hush · Ctrl+Shift+. panic · Ctrl+/ comment",
                );
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
                            ui.separator();
                            ui.weak("drums");
                            for d in DRUMS {
                                ui.monospace(*d);
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
                    egui::CollapsingHeader::new("signals")
                        .default_open(false)
                        .show(ui, |ui| {
                            for s in SIGNALS {
                                ui.monospace(*s);
                            }
                        });
                    egui::CollapsingHeader::new("factories")
                        .default_open(false)
                        .show(ui, |ui| {
                            for f in FACTORIES {
                                ui.monospace(*f);
                            }
                        });
                });
            });
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui, active_spans: &[(usize, usize)]) {
        // Theme the whole editor region to its own theme (not the host/system
        // theme) so the background, text and TextEdit all share one color and the
        // editor fills its panel seamlessly — no contrasting box with light
        // margins around it.
        let draw = self.editor_settings.draw_theme();
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(draw.background))
            .show_inside(ui, |ui| {
                *ui.visuals_mut() = if draw.light {
                    egui::Visuals::light()
                } else {
                    egui::Visuals::dark()
                };
                ui.visuals_mut().override_text_color = Some(draw.foreground);
                ui.add_space(4.0);
                self.editor_settings_panel(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let sliders = self.editor_decorations.sliders().to_vec();
                        let widgets = self.editor_decorations.widgets().to_vec();
                        let current_pattern = self.current.clone();
                        let playback_position_cycles = self.playback_position_cycles();
                        let editor_output = code_editor(
                            ui,
                            &mut self.code,
                            CodeEditorInput {
                                active: active_spans,
                                idents: &self.highlight_idents,
                                reference: &self.reference,
                                sample_names: &self.sample_names,
                                current_pattern: current_pattern.as_ref(),
                                playback_position_cycles,
                                sliders: &sliders,
                                widgets: &widgets,
                                widget_host: &mut self.widget_host,
                                settings: &self.editor_settings,
                            },
                        );
                        if let Some(change) = editor_output.text_change {
                            self.editor_decorations.map_change(change);
                        }
                        if let Some(update) = editor_output.slider_update {
                            self.editor_decorations
                                .set_slider_literal(&update.id, update.insert);
                        }
                        if let Some(cursor) = editor_output.cursor_byte {
                            self.editor_cursor_byte = cursor;
                        }
                    });
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
                    ui.add_enabled(
                        false,
                        egui::Checkbox::new(&mut self.editor_settings.multi_cursor, "multi-cursor"),
                    )
                    .on_hover_text("pending: egui TextEdit has one native selection");
                });

                ui.horizontal(|ui| {
                    ui.label("theme");
                    egui::ComboBox::from_id_salt("editor_theme")
                        .selected_text(self.editor_settings.theme.label())
                        .show_ui(ui, |ui| {
                            for theme in EditorTheme::ALL {
                                ui.selectable_value(
                                    &mut self.editor_settings.theme,
                                    theme,
                                    theme.label(),
                                );
                            }
                        });

                    ui.label("font");
                    egui::ComboBox::from_id_salt("editor_font_family")
                        .selected_text(self.editor_settings.font_family.label())
                        .show_ui(ui, |ui| {
                            for family in EditorFontFamily::ALL {
                                ui.selectable_value(
                                    &mut self.editor_settings.font_family,
                                    family,
                                    family.label(),
                                );
                            }
                        });

                    ui.add(
                        egui::Slider::new(&mut self.editor_settings.font_size, 11.0..=32.0)
                            .text("size")
                            .step_by(1.0),
                    );
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
    fn active_source_spans(&self) -> Vec<(usize, usize)> {
        match (&self.current, self.playback_position_cycles()) {
            (Some(pat), Some(pos)) => active_source_spans_at(pat, pos),
            _ => Vec::new(),
        }
    }

    fn active_editor_spans(&mut self) -> Vec<(usize, usize)> {
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
                spans.push((range.from, range.to));
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

fn eval_button_labels(block_based_eval: bool) -> (&'static str, &'static str) {
    if block_based_eval {
        ("Block (Ctrl+Enter)", "Eval (Ctrl+Shift+Enter)")
    } else {
        ("Eval (Ctrl+Enter)", "Block (Ctrl+Shift+Enter)")
    }
}

/// The deduped source byte ranges of the discrete events sounding at cycle
/// position `pos`. Factored out of [`RudelApp::active_source_spans`] so it can
/// be tested without a running engine.
fn active_source_spans_at(pat: &rudel_core::Pattern, pos: f64) -> Vec<(usize, usize)> {
    let pos_f = rudel_core::Frac::from_f64(pos);
    let cycle = pos.floor();
    let mut spans: Vec<(usize, usize)> = pat
        .query_arc(
            rudel_core::Frac::from_f64(cycle),
            rudel_core::Frac::from_f64(cycle + 1.0),
        )
        .into_iter()
        .filter(|h| {
            h.whole
                .as_ref()
                .is_some_and(|w| w.begin <= pos_f && pos_f < w.end)
        })
        .flat_map(|h| h.context.locations.clone())
        .collect();
    spans.sort_unstable();
    spans.dedup();
    spans
}

#[cfg(test)]
mod tests {
    use super::{active_source_spans_at, eval_button_labels};

    #[test]
    fn active_spans_flash_discrete_events_at_position() {
        // s("bd sd"): `bd` (bytes 3..5) sounds in [0,0.5), `sd` (6..8) in [0.5,1).
        let pat = rudel_lang::eval(r#"s("bd sd")"#).expect("eval");
        assert_eq!(active_source_spans_at(&pat, 0.25), vec![(3, 5)]);
        assert_eq!(active_source_spans_at(&pat, 0.75), vec![(6, 8)]);
        // the same structure repeats every cycle, so cycle 2 maps identically
        assert_eq!(active_source_spans_at(&pat, 2.25), vec![(3, 5)]);
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
            ("Eval (Ctrl+Enter)", "Block (Ctrl+Shift+Enter)")
        );
        assert_eq!(
            eval_button_labels(true),
            ("Block (Ctrl+Enter)", "Eval (Ctrl+Shift+Enter)")
        );
    }
}
