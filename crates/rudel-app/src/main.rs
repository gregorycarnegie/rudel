//! rudel-app - native live-coding editor for Rudel.
//! Type Koto in the editor, Ctrl+Enter to evaluate and hot-swap the pattern into
//! the running output (audio, MIDI or OSC). The right panel visualizes one cycle
//! per orbit with a live playhead; a reference pane lists sounds and controls.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui;
use rudel_audio::Engine;
use rudel_core::{Frac, Hap, Pattern, Value};
use rudel_midi::{MidiEngine, MidiIn, MidiOut};
use rudel_osc::{OscEngine, OscOut};
use std::collections::{BTreeMap, HashSet};
use std::thread::JoinHandle;
use std::time::Duration;

const DEFAULT_CODE: &str = r#"stack(
  s("bd ~ bd bd").gain(0.9),
  s("~ sd ~ sd"),
  s("hh*8").gain(0.5),
  note("c4 e4 g4 b4 a4 g4 e4 d4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").lpf("400 1600").gain(0.6).delay(0.3)
)"#;

const DEFAULT_VOLUME_PERCENT: f32 = 100.0;
const MAX_VOLUME_PERCENT: f32 = 200.0;
const CODE_EDITOR_ID: &str = "rudel_code_editor";
const CODE_INDENT: &str = "  ";

/// Built-in synth waveforms + noise sources (always available as `s(...)`).
const WAVEFORMS: &[&str] = &[
    "sine", "saw", "square", "triangle", "pulse", "user", "supersaw", "white", "pink", "brown",
];

/// Built-in synthesized drum sounds (always available as `s(...)`).
const DRUMS: &[&str] = &[
    "bd", "sd", "rim", "cp", "hh", "oh", "lt", "mt", "ht", "rd", "cr",
];

/// Continuous signals (used as values, e.g. `sine.range(0, 1)`).
const SIGNALS: &[&str] = &[
    "sine", "cosine", "saw", "isaw", "tri", "square", "sine2", "saw2", "rand", "rand2", "perlin",
    "time", "irand(n)", "run(n)",
];

/// Pattern factories (top-level constructors).
const FACTORIES: &[&str] = &[
    "stack",
    "cat",
    "seq",
    "fastcat",
    "slowcat",
    "randcat",
    "chooseCycles",
    "pure",
    "gap",
    "silence",
    "i",
    "freq",
    "getFreq",
    "Math",
];

/// Control names exposed by the engine, for the reference pane.
const CONTROLS: &[&str] = &[
    "note",
    "n",
    "i",
    "freq",
    "s",
    "tune",
    "xen",
    "withBase",
    "ftrans",
    "mpe",
    "bendRange",
    "gain",
    "pan",
    "speed",
    "cutoff",
    "resonance",
    "lpf",
    "lpq",
    "hcutoff",
    "hresonance",
    "hpf",
    "hpq",
    "bandf",
    "bandq",
    "bpf",
    "bpq",
    "lpenv",
    "lpattack",
    "lpdecay",
    "lpsustain",
    "lprelease",
    "fanchor",
    "room",
    "roomlp",
    "roomdim",
    "roomfade",
    "rlp",
    "rdim",
    "rfade",
    "size",
    "shape",
    "crush",
    "distort",
    "postgain",
    "delay",
    "delaytime",
    "delayfeedback",
    "attack",
    "decay",
    "sustain",
    "release",
    "adsr",
    "ad",
    "ar",
    "hold",
    "unison",
    "detune",
    "spread",
    "fm",
    "fmh",
    "fmwave",
    "fmattack",
    "fmdecay",
    "fmsustain",
    "fmrelease",
    "fmi2",
    "fmh2",
    "fmwave2",
    "partials",
    "phases",
    "pw",
    "noise",
    "pcurve",
    "vib",
    "vibmod",
    "penv",
    "pattack",
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
    "fmap",
    "piano",
    "pow",
];

const LANGUAGE_KEYWORDS: &[&str] = &[
    "const", "let", "fn", "if", "else", "for", "while", "in", "match", "return", "true", "false",
    "null",
];

fn is_highlighted_ident(ident: &str) -> bool {
    LANGUAGE_KEYWORDS.contains(&ident)
        || FACTORIES.contains(&ident)
        || CONTROLS.contains(&ident)
        || SIGNALS
            .iter()
            .any(|s| s.strip_suffix("(n)").unwrap_or(s) == ident)
}

fn highlighted_editor_job(code: &str, ui: &egui::Ui, wrap_width: f32) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let normal = egui::TextFormat::simple(font_id.clone(), ui.visuals().text_color());
    let keyword = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(106, 153, 205));
    let method = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(220, 220, 170));
    let string = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(206, 145, 120));
    let number = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(181, 206, 168));
    let comment = egui::TextFormat::simple(font_id, egui::Color32::from_rgb(106, 153, 85));

    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i] as char;

        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            job.append(&code[start..i], 0.0, comment.clone());
        } else if c == '"' || c == '\'' {
            let quote = bytes[i];
            i += 1;
            let mut escaped = false;
            while i < bytes.len() {
                let b = bytes[i];
                i += 1;
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == quote {
                    break;
                }
            }
            job.append(&code[start..i], 0.0, string.clone());
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '.' | '_' | '/') {
                    i += 1;
                } else {
                    break;
                }
            }
            job.append(&code[start..i], 0.0, number.clone());
        } else if c.is_ascii_alphabetic() || matches!(c, '_' | '$') {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '_' | '$') {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &code[start..i];
            let format = if start > 0 && bytes[start - 1] == b'.' {
                method.clone()
            } else if is_highlighted_ident(ident) {
                keyword.clone()
            } else {
                normal.clone()
            };
            job.append(ident, 0.0, format);
        } else {
            i += 1;
            job.append(&code[start..i], 0.0, normal.clone());
        }
    }

    job
}

#[derive(Clone, Copy, Default)]
struct EditorShortcuts {
    comment_toggle: bool,
    indent: bool,
    outdent: bool,
}

fn capture_editor_shortcuts(ui: &mut egui::Ui, editor_id: egui::Id) -> EditorShortcuts {
    if !ui.memory(|m| m.has_focus(editor_id)) {
        return EditorShortcuts::default();
    }
    ui.input_mut(|i| EditorShortcuts {
        comment_toggle: i.consume_key(egui::Modifiers::CTRL, egui::Key::Slash),
        indent: i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
        outdent: i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
    })
}

fn editor_typed_text(ui: &egui::Ui) -> Option<String> {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Text(text) if text.chars().count() == 1 => Some(text.clone()),
                _ => None,
            })
            .last()
    })
}

fn editor_enter_pressed(ui: &egui::Ui) -> bool {
    ui.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command
            )
        })
    })
}

fn apply_editor_text_edits(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    shortcuts: EditorShortcuts,
    typed_text: Option<&str>,
    enter_pressed: bool,
) -> Option<egui::text::CCursorRange> {
    if shortcuts.comment_toggle {
        return Some(toggle_line_comments(text, cursor_range));
    }
    if shortcuts.outdent {
        return Some(indent_lines(text, cursor_range, false));
    }
    if shortcuts.indent {
        return Some(indent_lines(text, cursor_range, true));
    }
    if enter_pressed && let Some(range) = auto_indent_after_enter(text, cursor_range) {
        return Some(range);
    }
    typed_text.and_then(|typed| apply_auto_pair(text, cursor_range, typed))
}

#[derive(Clone, Debug)]
struct CharChange {
    pos: usize,
    delete_len: usize,
    insert: String,
}

fn apply_char_changes(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    changes: Vec<CharChange>,
) -> egui::text::CCursorRange {
    if changes.is_empty() {
        return cursor_range;
    }

    let primary = map_index_after_changes(cursor_range.primary.index, &changes, true);
    let secondary = map_index_after_changes(cursor_range.secondary.index, &changes, true);

    for change in changes.iter().rev() {
        replace_char_range(
            text,
            change.pos..change.pos + change.delete_len,
            &change.insert,
        );
    }

    egui::text::CCursorRange {
        primary: egui::text::CCursor::new(primary),
        secondary: egui::text::CCursor::new(secondary),
        h_pos: None,
    }
}

fn apply_line_changes(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    line_starts: &[usize],
    changes: Vec<CharChange>,
) -> egui::text::CCursorRange {
    if cursor_range.is_empty() || line_starts.is_empty() {
        return apply_char_changes(text, cursor_range, changes);
    }

    let first_line = line_starts[0];
    let last_line = *line_starts.last().unwrap();
    let last_line_end = line_end_at(text, last_line);
    let selection_start = map_index_after_changes(first_line, &changes, false);
    let selection_end = map_index_after_changes(last_line_end, &changes, true);

    for change in changes.iter().rev() {
        replace_char_range(
            text,
            change.pos..change.pos + change.delete_len,
            &change.insert,
        );
    }

    egui::text::CCursorRange::two(
        egui::text::CCursor::new(selection_start),
        egui::text::CCursor::new(selection_end),
    )
}

fn map_index_after_changes(
    index: usize,
    changes: &[CharChange],
    include_insert_at_index: bool,
) -> usize {
    let mut delta = 0isize;
    for change in changes {
        let insert_len = change.insert.chars().count();
        let deleted_end = change.pos + change.delete_len;
        if index < change.pos
            || (!include_insert_at_index && index == change.pos && change.delete_len == 0)
        {
            break;
        }
        if index <= deleted_end {
            return (change.pos as isize + delta + insert_len as isize).max(0) as usize;
        }
        delta += insert_len as isize - change.delete_len as isize;
    }
    (index as isize + delta).max(0) as usize
}

fn apply_auto_pair(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    typed: &str,
) -> Option<egui::text::CCursorRange> {
    let cursor = cursor_range.single()?;
    let typed = typed.chars().next()?;
    let idx = cursor.index;
    if idx == 0 || char_at(text, idx - 1) != Some(typed) {
        return None;
    }

    if is_pair_closer(typed) || is_quote_pair(typed) {
        if char_at(text, idx) == Some(typed) {
            replace_char_range(text, idx - 1..idx, "");
            return Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)));
        }
    }

    let close = match typed {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' | '\'' | '`' => typed,
        _ => return None,
    };
    insert_text_at_char(text, idx, &close.to_string());
    Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx)))
}

fn auto_indent_after_enter(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
) -> Option<egui::text::CCursorRange> {
    let cursor = cursor_range.single()?;
    let idx = cursor.index;
    if idx == 0 || char_at(text, idx - 1) != Some('\n') {
        return None;
    }

    let newline_idx = idx - 1;
    let prev_start = line_start_at(text, newline_idx);
    let prev_line = char_slice(text, prev_start..newline_idx);
    let base_indent: String = prev_line
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect();
    let prev_trimmed = prev_line.trim_end();
    let extra_indent = if matches!(prev_trimmed.chars().last(), Some('(' | '[' | '{')) {
        CODE_INDENT
    } else {
        ""
    };

    let insert = if matching_close_after_cursor(prev_trimmed, char_at(text, idx)) {
        format!("{base_indent}{extra_indent}\n{base_indent}")
    } else {
        format!("{base_indent}{extra_indent}")
    };
    let cursor_idx = idx + base_indent.chars().count() + extra_indent.chars().count();
    insert_text_at_char(text, idx, &insert);
    Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
        cursor_idx,
    )))
}

fn matching_close_after_cursor(prev_trimmed: &str, next: Option<char>) -> bool {
    matches!(
        (prev_trimmed.chars().last(), next),
        (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}'))
    )
}

fn toggle_line_comments(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
) -> egui::text::CCursorRange {
    let line_starts = selected_line_starts(text, cursor_range);
    let code_lines: Vec<usize> = line_starts
        .iter()
        .copied()
        .filter(|&line| !line_is_blank(text, line))
        .collect();
    let uncomment = !code_lines.is_empty()
        && code_lines
            .iter()
            .all(|&line| line_comment_pos(text, line).is_some());

    let mut changes = Vec::new();
    for &line in &line_starts {
        let indent = leading_whitespace_len(text, line);
        let pos = line + indent;
        if uncomment {
            if let Some(comment_pos) = line_comment_pos(text, line) {
                let delete_len = if char_at(text, comment_pos + 2) == Some(' ') {
                    3
                } else {
                    2
                };
                changes.push(CharChange {
                    pos: comment_pos,
                    delete_len,
                    insert: String::new(),
                });
            }
        } else {
            changes.push(CharChange {
                pos,
                delete_len: 0,
                insert: "// ".to_string(),
            });
        }
    }

    apply_line_changes(text, cursor_range, &line_starts, changes)
}

fn indent_lines(
    text: &mut String,
    cursor_range: egui::text::CCursorRange,
    indent: bool,
) -> egui::text::CCursorRange {
    let mut changes = Vec::new();
    let line_starts = selected_line_starts(text, cursor_range);
    for &line in &line_starts {
        if indent {
            changes.push(CharChange {
                pos: line,
                delete_len: 0,
                insert: CODE_INDENT.to_string(),
            });
        } else if char_at(text, line) == Some('\t') {
            changes.push(CharChange {
                pos: line,
                delete_len: 1,
                insert: String::new(),
            });
        } else {
            let spaces = (0..CODE_INDENT.chars().count())
                .take_while(|i| char_at(text, line + i) == Some(' '))
                .count();
            if spaces > 0 {
                changes.push(CharChange {
                    pos: line,
                    delete_len: spaces,
                    insert: String::new(),
                });
            }
        }
    }

    apply_line_changes(text, cursor_range, &line_starts, changes)
}

fn selected_line_starts(text: &str, cursor_range: egui::text::CCursorRange) -> Vec<usize> {
    let [min, max] = cursor_range.sorted_cursors();
    let start = min.index;
    let mut end = max.index;
    if end > start && char_at(text, end - 1) == Some('\n') {
        end -= 1;
    }

    let mut line = line_start_at(text, start);
    let mut lines = vec![line];
    while let Some(next) = next_line_start_after(text, line) {
        if next > end {
            break;
        }
        line = next;
        lines.push(line);
    }
    lines
}

fn line_comment_pos(text: &str, line_start: usize) -> Option<usize> {
    let pos = line_start + leading_whitespace_len(text, line_start);
    (char_at(text, pos) == Some('/') && char_at(text, pos + 1) == Some('/')).then_some(pos)
}

fn line_is_blank(text: &str, line_start: usize) -> bool {
    char_slice(text, line_start..line_end_at(text, line_start))
        .trim()
        .is_empty()
}

fn leading_whitespace_len(text: &str, line_start: usize) -> usize {
    text.chars()
        .skip(line_start)
        .take_while(|c| matches!(c, ' ' | '\t'))
        .count()
}

fn line_start_at(text: &str, char_idx: usize) -> usize {
    let mut start = 0;
    for (idx, ch) in text.chars().enumerate().take(char_idx) {
        if ch == '\n' {
            start = idx + 1;
        }
    }
    start
}

fn line_end_at(text: &str, line_start: usize) -> usize {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return line_start + offset;
        }
    }
    text.chars().count()
}

fn next_line_start_after(text: &str, line_start: usize) -> Option<usize> {
    for (offset, ch) in text.chars().skip(line_start).enumerate() {
        if ch == '\n' {
            return Some(line_start + offset + 1);
        }
    }
    None
}

fn char_at(text: &str, char_idx: usize) -> Option<char> {
    text.chars().nth(char_idx)
}

fn char_slice(text: &str, range: std::ops::Range<usize>) -> &str {
    &text[byte_index_at_char(text, range.start)..byte_index_at_char(text, range.end)]
}

fn insert_text_at_char(text: &mut String, char_idx: usize, insert: &str) {
    text.insert_str(byte_index_at_char(text, char_idx), insert);
}

fn replace_char_range(text: &mut String, range: std::ops::Range<usize>, insert: &str) {
    let start = byte_index_at_char(text, range.start);
    let end = byte_index_at_char(text, range.end);
    text.replace_range(start..end, insert);
}

fn byte_index_at_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn is_pair_closer(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

fn is_quote_pair(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

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

struct RudelApp {
    engine: Option<Engine>,
    audio_error: Option<String>,
    code: String,
    eval_error: Option<String>,
    status: String,
    cps: f64,
    volume_percent: f32,
    playing: bool,
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
            playing: false,
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

    /// Connect (or reconnect) a MIDI input device: incoming CCs feed `ccin`, and
    /// MIDI clock can drive `cps` when `clock_sync` is on.
    fn connect_input(&mut self) {
        let port = {
            let p = self.midi_in_port.trim();
            if p.is_empty() { None } else { Some(p) }
        };
        match MidiIn::connect(port) {
            Ok(input) => {
                self.midi_in = Some(input);
                self.io_error = None;
                self.status = "MIDI input connected".to_string();
            }
            Err(e) => self.io_error = Some(format!("MIDI in: {e}")),
        }
    }

    fn poll_sample_jobs(&mut self, ctx: &egui::Context) {
        let mut finished = 0;
        let mut loaded = 0;
        let mut failed = false;
        let mut i = 0;
        while i < self.sample_jobs.len() {
            if !self.sample_jobs[i].handle.is_finished() {
                i += 1;
                continue;
            }
            let job = self.sample_jobs.swap_remove(i);
            match job.handle.join() {
                Ok(Ok(n)) => {
                    loaded += n;
                    finished += 1;
                }
                Ok(Err(e)) => {
                    self.loaded_sample_sources.remove(&job.key);
                    self.io_error = Some(format!("{}: {e}", job.label));
                    failed = true;
                    finished += 1;
                }
                Err(_) => {
                    self.loaded_sample_sources.remove(&job.key);
                    self.io_error = Some(format!("{}: loader thread panicked", job.label));
                    failed = true;
                    finished += 1;
                }
            }
        }

        if finished > 0 {
            if let Some(engine) = &self.engine {
                self.sample_names = engine.sample_names();
            }
            if loaded > 0 || !failed {
                self.status = format!(
                    "loaded {loaded} samples ({} sounds)",
                    self.sample_names.len()
                );
                if !failed {
                    self.io_error = None;
                }
            } else {
                self.status = "sample load failed".to_string();
            }
        }

        if !self.sample_jobs.is_empty() {
            self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn queue_sample_source(&mut self, source: String) {
        if self.engine.is_none() {
            self.io_error = Some("no audio engine to load samples into".to_string());
            return;
        }
        if !self.loaded_sample_sources.insert(source.clone()) {
            return;
        }
        let handle = self.engine.as_ref().unwrap().spawn_samples(source.clone());
        self.sample_jobs.push(SampleJob {
            key: source.clone(),
            label: format!("samples({source:?})"),
            handle,
        });
        self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
    }

    fn queue_sample_map(&mut self, json: String, base: String) {
        if self.engine.is_none() {
            self.io_error = Some("no audio engine to load samples into".to_string());
            return;
        }
        let key = format!("map:{base}\n{json}");
        if !self.loaded_sample_sources.insert(key.clone()) {
            return;
        }
        let handle = self
            .engine
            .as_ref()
            .unwrap()
            .spawn_load_sample_map(json, base);
        self.sample_jobs.push(SampleJob {
            key,
            label: "samples(map)".to_string(),
            handle,
        });
        self.status = format!("loading samples ({} job(s))", self.sample_jobs.len());
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

    /// Apply `samples(...)` / `aliasBank(...)` requests from the script. Sample
    /// sources already loaded are skipped, so re-evaluation doesn't re-fetch.
    fn apply_sample_effects(&mut self, effects: &rudel_lang::SampleEffects) {
        if let Some(cps) = effects.cps {
            self.set_cps(cps);
        }
        if let Some(engine) = &self.engine {
            for (canonical, alias) in &effects.bank_aliases {
                engine.alias_bank(canonical, alias);
            }
        }
        for source in &effects.sources {
            self.queue_sample_source(source.clone());
        }
        for (json, base) in &effects.maps {
            self.queue_sample_map(json.clone(), base.clone());
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

    fn set_volume_percent(&mut self, volume_percent: f32) {
        self.volume_percent = volume_percent.max(0.0).min(MAX_VOLUME_PERCENT);
        if let Some(e) = &self.engine {
            e.set_volume((self.volume_percent / 100.0) as f64);
        }
    }

    /// Split the current pattern across the audio / MIDI / OSC back-ends.
    ///
    /// Per-pattern `.midi()` / `.osc()` tags always route to their back-end;
    /// untagged events go to the selected default `output`. MIDI/OSC back-ends
    /// are started lazily when the default selects them or a tag routes to them.
    fn route(&mut self) {
        let active = if self.playing {
            self.current.clone().unwrap_or_else(rudel_core::silence)
        } else {
            rudel_core::silence()
        };
        let (tag_midi, tag_osc) = if self.playing {
            rudel_lang::output_targets(&active)
        } else {
            (false, false)
        };
        if self.playing && (self.output == Output::Midi || tag_midi) {
            self.ensure_midi();
        }
        if self.playing && (self.output == Output::Osc || tag_osc) {
            self.ensure_osc();
        }
        if let Some(e) = &self.engine {
            e.set_pattern(rudel_lang::filter_output(
                &active,
                "audio",
                self.output == Output::Audio,
            ));
        }
        if let Some(m) = &self.midi {
            m.set_pattern(rudel_lang::filter_output(
                &active,
                "midi",
                self.output == Output::Midi,
            ));
        }
        if let Some(o) = &self.osc {
            o.set_pattern(rudel_lang::filter_output(
                &active,
                "osc",
                self.output == Output::Osc,
            ));
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
            }
        }
    }

    fn load_samples(&mut self) {
        let source = self.sample_dir.trim().to_string();
        if source.is_empty() {
            self.io_error =
                Some("samples: enter a folder, strudel.json, URL, or github:user/repo".to_string());
            return;
        }
        // `samples()` accepts a local folder, a local strudel.json, an http(s)
        // URL, or a `github:`/`bubo:` pseudo-URL.
        self.queue_sample_source(source);
    }
}

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
                    let editor_id = ui.make_persistent_id(egui::Id::new(CODE_EDITOR_ID));
                    let shortcuts = capture_editor_shortcuts(ui, editor_id);
                    let typed_text = editor_typed_text(ui);
                    let enter_pressed = editor_enter_pressed(ui);
                    let mut layouter =
                        |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                            let job = highlighted_editor_job(text.as_str(), ui, wrap_width);
                            ui.fonts_mut(|fonts| fonts.layout_job(job))
                        };
                    let mut output = egui::TextEdit::multiline(&mut self.code)
                        .id_salt(CODE_EDITOR_ID)
                        .code_editor()
                        .layouter(&mut layouter)
                        .desired_rows(28)
                        .desired_width(f32::INFINITY)
                        .show(ui);
                    if output.response.has_focus()
                        && let Some(cursor_range) = output.cursor_range
                        && let Some(new_range) = apply_editor_text_edits(
                            &mut self.code,
                            cursor_range,
                            shortcuts,
                            typed_text.as_deref(),
                            enter_pressed,
                        )
                    {
                        output.state.cursor.set_char_range(Some(new_range));
                        output.state.store(ui.ctx(), output.response.id);
                    }
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

fn vlc_volume_slider(ui: &mut egui::Ui, volume_percent: &mut f32) -> egui::Response {
    let desired = egui::vec2(136.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 54.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0),
    );

    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *volume_percent = volume_percent_from_track_x(pos.x, track);
        response.mark_changed();
    }

    let painter = ui.painter_at(rect);
    draw_speaker_icon(
        &painter,
        egui::Rect::from_min_size(rect.min, egui::vec2(18.0, rect.height())),
        ui.visuals().text_color(),
        egui::Color32::from_rgb(230, 145, 45),
    );
    painter.text(
        egui::pos2(rect.left() + 19.0, rect.top() + 1.0),
        egui::Align2::LEFT_TOP,
        format!("{:.0}%", volume_percent.clamp(0.0, MAX_VOLUME_PERCENT)),
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    draw_volume_wedge(
        &painter,
        track,
        (*volume_percent / MAX_VOLUME_PERCENT).clamp(0.0, 1.0),
    );

    response
}

fn volume_percent_from_track_x(x: f32, track: egui::Rect) -> f32 {
    let t = ((x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
    t * MAX_VOLUME_PERCENT
}

fn draw_speaker_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    accent: egui::Color32,
) {
    let cy = rect.center().y + 2.0;
    let left = rect.left() + 2.0;
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, cy - 3.0), egui::pos2(left + 4.0, cy + 3.0)),
        1.0,
        color,
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(left + 4.0, cy - 4.5),
            egui::pos2(left + 10.0, cy - 8.0),
            egui::pos2(left + 10.0, cy + 8.0),
            egui::pos2(left + 4.0, cy + 4.5),
        ],
        color,
        egui::Stroke::NONE,
    ));

    let center = egui::pos2(left + 8.0, cy);
    for radius in [5.0, 8.0] {
        painter.add(egui::Shape::line(
            arc_points(center, radius, -0.75, 0.75),
            egui::Stroke::new(1.2, accent),
        ));
    }
}

fn arc_points(center: egui::Pos2, radius: f32, start: f32, end: f32) -> Vec<egui::Pos2> {
    (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let a = start + (end - start) * t;
            egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

fn draw_volume_wedge(painter: &egui::Painter, rect: egui::Rect, amount: f32) {
    let amount = amount.clamp(0.0, 1.0);
    let bottom = rect.bottom();
    let top_at = |x: f32| {
        let t = ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        bottom - 2.0 - t * (rect.height() - 2.0)
    };
    let outline = vec![
        egui::pos2(rect.left(), bottom),
        egui::pos2(rect.right(), bottom),
        egui::pos2(rect.right(), top_at(rect.right())),
        egui::pos2(rect.left(), top_at(rect.left())),
    ];

    painter.add(egui::Shape::convex_polygon(
        outline.clone(),
        egui::Color32::from_gray(42),
        egui::Stroke::NONE,
    ));

    let segments = 48;
    for i in 0..segments {
        let t0 = i as f32 / segments as f32;
        if t0 >= amount {
            break;
        }
        let t1 = ((i + 1) as f32 / segments as f32).min(amount);
        let x0 = rect.left() + rect.width() * t0;
        let x1 = rect.left() + rect.width() * t1;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x0, bottom),
                egui::pos2(x1, bottom),
                egui::pos2(x1, top_at(x1)),
                egui::pos2(x0, top_at(x0)),
            ],
            volume_ramp_color(t1),
            egui::Stroke::NONE,
        ));
    }

    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(145));
    for line in outline.windows(2) {
        painter.line_segment([line[0], line[1]], stroke);
    }
    painter.line_segment([outline[3], outline[0]], stroke);
}

fn volume_ramp_color(t: f32) -> egui::Color32 {
    let green = egui::Color32::from_rgb(58, 205, 65);
    let yellow = egui::Color32::from_rgb(248, 218, 44);
    let red = egui::Color32::from_rgb(244, 80, 42);
    if t < 0.5 {
        lerp_color(green, yellow, t * 2.0)
    } else {
        lerp_color(yellow, red, (t - 0.5) * 2.0)
    }
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    egui::Color32::from_rgb(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn app_without_engine() -> RudelApp {
        RudelApp {
            engine: None,
            audio_error: None,
            code: String::new(),
            eval_error: None,
            status: String::new(),
            cps: 0.5,
            volume_percent: DEFAULT_VOLUME_PERCENT,
            playing: false,
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

    #[test]
    fn volume_track_maps_x_to_percent() {
        let track = egui::Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(110.0, 10.0));
        assert_eq!(volume_percent_from_track_x(10.0, track), 0.0);
        assert_eq!(volume_percent_from_track_x(60.0, track), 100.0);
        assert_eq!(volume_percent_from_track_x(110.0, track), 200.0);
        assert_eq!(volume_percent_from_track_x(140.0, track), 200.0);
    }

    fn cursor(index: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::one(egui::text::CCursor::new(index))
    }

    fn selection(start: usize, end: usize) -> egui::text::CCursorRange {
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(start),
            egui::text::CCursor::new(end),
        )
    }

    #[test]
    fn editor_auto_pairs_opening_brackets() {
        let mut text = "stack(".to_string();
        let range = apply_auto_pair(&mut text, cursor(6), "(").unwrap();
        assert_eq!(text, "stack()");
        assert_eq!(range.single().unwrap().index, 6);
    }

    #[test]
    fn editor_skips_existing_closing_brackets() {
        let mut text = "())".to_string();
        let range = apply_auto_pair(&mut text, cursor(2), ")").unwrap();
        assert_eq!(text, "()");
        assert_eq!(range.single().unwrap().index, 2);
    }

    #[test]
    fn editor_auto_indent_carries_indent_after_enter() {
        let mut text = "  note(\n".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(8)).unwrap();
        assert_eq!(text, "  note(\n    ");
        assert_eq!(range.single().unwrap().index, 12);
    }

    #[test]
    fn editor_auto_indent_splits_bracket_pairs() {
        let mut text = "(\n)".to_string();
        let range = auto_indent_after_enter(&mut text, cursor(2)).unwrap();
        assert_eq!(text, "(\n  \n)");
        assert_eq!(range.single().unwrap().index, 4);
    }

    #[test]
    fn editor_indents_and_outdents_selected_lines() {
        let mut text = "a\nb".to_string();
        let range = indent_lines(&mut text, selection(0, 3), true);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), 0..7);

        let range = indent_lines(&mut text, range, false);
        assert_eq!(text, "a\nb");
        assert_eq!(range.as_sorted_char_range(), 0..3);
    }

    #[test]
    fn editor_toggles_line_comments() {
        let mut text = "  a\n  b".to_string();
        let range = toggle_line_comments(&mut text, selection(0, 7));
        assert_eq!(text, "  // a\n  // b");

        let range = toggle_line_comments(&mut text, range);
        assert_eq!(text, "  a\n  b");
        assert_eq!(range.as_sorted_char_range(), 0..7);
    }

    #[test]
    fn value_short_formats_common_values() {
        assert_eq!(value_short(&Value::Str("bd".to_string())), "bd");
        assert_eq!(value_short(&Value::Int(42)), "42");
        assert_eq!(value_short(&Value::F64(1.2300)), "1.23");
        assert_eq!(value_short(&Value::F64(2.0)), "2");
    }

    #[test]
    fn hap_label_prefers_named_controls() {
        let with_sound = Value::Map(BTreeMap::from([
            ("note".to_string(), Value::Int(60)),
            ("s".to_string(), Value::Str("bd".to_string())),
        ]));
        let with_note = Value::Map(BTreeMap::from([("note".to_string(), Value::Int(64))]));

        assert_eq!(hap_label(&with_sound), "s:bd");
        assert_eq!(hap_label(&with_note), "note:64");
        assert_eq!(hap_label(&Value::Map(BTreeMap::new())), "");
    }

    #[test]
    fn orbit_defaults_to_zero_and_reads_map_control() {
        assert_eq!(orbit_of(&Value::Str("bd".to_string())), 0);
        assert_eq!(
            orbit_of(&Value::Map(BTreeMap::from([(
                "orbit".to_string(),
                Value::F64(2.9)
            )]))),
            2
        );
    }

    #[test]
    fn truncate_respects_character_boundaries() {
        assert_eq!(truncate("abcd", 4), "abcd");

        let shortened = truncate("abcdef", 4);
        assert_eq!(shortened.chars().count(), 5);
        assert!(shortened.ends_with('\u{2026}'));

        let unicode = truncate("\u{03b1}\u{03b2}\u{03b3}", 2);
        assert_eq!(unicode.chars().count(), 3);
        assert!(unicode.ends_with('\u{2026}'));
    }

    #[test]
    fn color_helpers_are_deterministic() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(color_for("bd"), color_for("bd"));
        assert_ne!(color_for("bd"), color_for("sd"));
    }
}
