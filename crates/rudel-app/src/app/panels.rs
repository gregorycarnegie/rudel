use super::{Output, RudelApp};
use crate::{
    editor::{
        CodeEditorInput, code_editor,
        settings::{EditorFontFamily, EditorTheme},
    },
    reference::{CONTROLS, DRUMS, FACTORIES, SIGNALS, WAVEFORMS},
    volume::vlc_volume_slider,
};
use eframe::egui;

impl eframe::App for RudelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_sample_jobs(ui.ctx());
        let midi_connecting = self.poll_midi_connect() | self.poll_midi_in_connect();

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

        // Keep the playhead moving while playing (and polling clock / CC input /
        // a pending MIDI connection).
        if self.playing
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
    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("transport").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("rudel")
                        .monospace()
                        .size(18.0)
                        .strong()
                        .color(crate::theme::ACCENT),
                );
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
                // Right edge: status text with a colored state light.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(&self.status);
                    let light = if self.eval_error.is_some() || self.io_error.is_some() {
                        crate::theme::STOP
                    } else if self.playing {
                        crate::theme::GO
                    } else {
                        egui::Color32::from_gray(90)
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, light);
                    if self.audio_error.is_some() {
                        ui.colored_label(crate::theme::ACCENT, "no audio");
                    }
                });
            });

            // Occasional setup lives out of the way: one collapsed row instead
            // of two always-visible ones.
            let io_summary = io_summary(
                self.sample_names.len(),
                self.midi_in.is_some(),
                self.midi_in_pending.is_some(),
            );
            egui::CollapsingHeader::new(io_summary)
                .id_salt("io_section")
                .default_open(false)
                .show(ui, |ui| {
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

    fn errors_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("errors").show(ui, |ui| {
            if let Some(e) = &self.audio_error {
                ui.colored_label(crate::theme::ACCENT, format!("audio: {e}"));
            }
            if let Some(e) = &self.io_error {
                ui.colored_label(crate::theme::ACCENT, e);
            }
            if let Some(e) = &self.eval_error {
                ui.colored_label(crate::theme::STOP, e);
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
                                    for (item, hits) in items {
                                        if let Some(text) = reference_item(ui, item, hits) {
                                            insert = Some(text);
                                        }
                                    }
                                }
                            });
                    }
                    for (title, all, default_open) in [
                        ("controls", CONTROLS, true),
                        ("signals", SIGNALS, false),
                        ("factories", FACTORIES, false),
                    ] {
                        let items = fuzzy_filter(all.iter().copied(), query);
                        if items.is_empty() {
                            continue;
                        }
                        egui::CollapsingHeader::new(title)
                            .default_open(default_open)
                            .open(force_open)
                            .show(ui, |ui| {
                                for (item, hits) in &items {
                                    if let Some(text) = reference_item(ui, item, hits) {
                                        insert = Some(text);
                                    }
                                }
                            });
                    }
                });
                if insert.is_some() {
                    self.pending_insert = insert;
                }
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
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let sliders = self.editor_decorations.sliders().to_vec();
                        let widgets = self.editor_decorations.widgets().to_vec();
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
                                playback_position_cycles,
                                sliders: &sliders,
                                widgets: &widgets,
                                widget_host: &mut self.widget_host,
                                settings: &self.editor_settings,
                                insert_text,
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

/// Collapsed-header summary for the I/O section, so what's loaded/connected
/// shows at a glance without opening it.
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
        let normal = egui::text::TextFormat::simple(font.clone(), ui.visuals().text_color());
        let hit = egui::text::TextFormat::simple(font, ui.visuals().hyperlink_color);
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
        job.into()
    };
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        .on_hover_text("double-click to insert · drag into the editor")
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
    use super::{
        active_source_spans_at, eval_button_labels, fuzzy_filter, fuzzy_match, io_summary,
    };

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
            (("Eval", "Ctrl+Enter"), ("Block", "Ctrl+Shift+Enter"))
        );
        assert_eq!(
            eval_button_labels(true),
            (("Block", "Ctrl+Enter"), ("Eval", "Ctrl+Shift+Enter"))
        );
    }
}
