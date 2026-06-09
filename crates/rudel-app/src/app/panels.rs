use super::{Output, RudelApp};
use crate::editor::code_editor;
use crate::reference::{CONTROLS, DRUMS, FACTORIES, SIGNALS, WAVEFORMS};
use crate::visualizer::draw_visualizer;
use crate::volume::vlc_volume_slider;
use eframe::egui;

impl eframe::App for RudelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_sample_jobs(ui.ctx());

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

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("editor")
            .resizable(true)
            .default_size(440.0)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    code_editor(ui, &mut self.code);
                });
            });
    }
}
