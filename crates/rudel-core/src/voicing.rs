// voicing.rs - chord-symbol voicings. Ported from strudel/packages/tonal/
// {voicings,tonleiter,ireal}.mjs. The recommended `voicing` path
// (`renderVoicing`) lives entirely in tonleiter.mjs with no external dependency,
// so it ports cleanly here; the curated dictionaries from voicings.mjs
// (lefthand / triads / guidetones / legacy) plus the default iReal dictionaries
// (`ireal` = `simple`, `ireal-ext` = `complex`) are inlined. The deprecated
// `voicings()` voice-leading (external `chord-voicings` package) is the one
// intentional gap: rudel's `voicings(dict)` instead aliases `voicing` with a
// named dictionary (no smoothest-voice-leading state).
// SPDX-License-Identifier: AGPL-3.0-or-later

mod dictionaries;
#[cfg(test)]
mod tests;

use crate::{
    pattern::{Pattern, pure, silence, stack},
    tonal::{chord_symbol, interval_to_semitones, letter_semitone, note_to_midi_with_octave},
    value::{Value, ValueMap},
};
use dictionaries::dictionary;
use std::sync::{LazyLock, RwLock};

/// How a voicing is aligned to the anchor note.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Top note <= anchor.
    Below,
    /// Top note <= anchor, with notes equal to the anchor dropped.
    Duck,
    /// Bottom note >= anchor.
    Above,
    /// Bottom note used as the target; always picks the first voicing.
    Root,
}

impl Mode {
    fn from_str(s: &str) -> Mode {
        match s {
            "above" => Mode::Above,
            "duck" => Mode::Duck,
            "root" => Mode::Root,
            _ => Mode::Below,
        }
    }

    /// The note in a voicing the anchor is compared against.
    fn target(self, voicing: &[i32]) -> i32 {
        match self {
            Mode::Above | Mode::Root => voicing[0],
            Mode::Below | Mode::Duck => *voicing.last().unwrap(),
        }
    }
}

/// Pitch class (letter + accidentals) to a 0..11 chroma.
fn pc_to_chroma(pc: &str) -> Option<i32> {
    let mut chars = pc.chars();
    let mut chroma = letter_semitone(chars.next()?)?;
    for c in chars {
        match c {
            '#' | 's' => chroma += 1,
            'b' | 'f' => chroma -= 1,
            _ => return None,
        }
    }
    Some(chroma.rem_euclid(12))
}

/// Split a chord symbol like `"C^7"`, `"Am7"`, `"G7/B"` into `(root, symbol)`,
/// dropping any slash-bass.
fn tokenize_chord(chord: &str) -> Option<(String, String)> {
    let chord = chord.split('/').next().unwrap_or(chord);
    let mut chars = chord.chars().peekable();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || !"abcdefg".contains(first.to_ascii_lowercase()) {
        return None;
    }
    let mut root = String::new();
    root.push(first);
    while let Some(&c) = chars.peek() {
        if c == '#' || c == 'b' {
            root.push(c);
            chars.next();
        } else {
            break;
        }
    }
    Some((root, chars.collect()))
}

/// Normalise alternate chord-symbol spellings to the dictionary's canonical
/// keys. The canonical keys use `^` (major 7th) which mini-notation can't spell,
/// so accept the `maj`/`min`/`dim`/`+` spellings too.
fn normalize_symbol(s: &str) -> &str {
    match s {
        "maj7" | "M7" => "^7",
        "maj9" | "M9" => "^9",
        "min7" | "-7" => "m7",
        "min9" | "-9" => "m9",
        "minor" | "min" | "-" => "m",
        "major" | "maj" => "",
        "dim" => "o",
        "dim7" => "o7",
        "min7b5" | "m7-5" | "hdim" => "m7b5",
        "+" => "aug",
        "minMaj7" | "mmaj7" => "mM7",
        other => other,
    }
}

/// `scaleStep`: index into `notes` like a scale, octaving overshoots.
fn scale_step_in(notes: &[i32], offset: i32, octaves: i32) -> i32 {
    let len = notes.len() as i32;
    let oct_offset = offset.div_euclid(len) * octaves * 12;
    notes[offset.rem_euclid(len) as usize] + oct_offset
}

/// Options for [`render_voicing`].
struct VoicingOpts {
    dict: String,
    offset: i32,
    n: Option<i32>,
    mode: Option<Mode>,
    anchor: Option<i32>,
    octaves: i32,
}

/// The dictionary `.voicing()` uses when the hap names none, as
/// `setDefaultVoicings(name)` last left it (tonal/voicings.mjs `defaultDict`).
/// Process-global because upstream's is too — a script sets it once at the top
/// and every later `.voicing()` reads it.
static DEFAULT_DICT: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new("ireal".to_string()));

/// `setDefaultVoicings(name)`.
pub fn set_default_voicings(dict: impl Into<String>) {
    *DEFAULT_DICT.write().unwrap() = dict.into();
}

fn default_dict() -> String {
    DEFAULT_DICT.read().unwrap().clone()
}

/// `addVoicings(name, dictionary)` (tonal/voicings.mjs): register a chord
/// dictionary under `name`, for a later `.voicing(name)` or
/// `setDefaultVoicings(name)` to reach. Each entry maps a chord symbol to the
/// voicings to choose between, written as interval lists (`"3M 7m 9M"`).
///
/// Upstream's third argument, `range`, is not taken: it reaches only the
/// deprecated `.voicings(dict)` voice-leading path, which Rudel aliases to
/// `voicing` — the same gap `setVoicingRange` already documents.
pub fn add_voicings(name: &str, dictionary: impl IntoIterator<Item = (String, Vec<String>)>) {
    dictionaries::register(name, dictionary.into_iter().collect());
}

impl Default for VoicingOpts {
    fn default() -> Self {
        VoicingOpts {
            dict: default_dict(),
            offset: 0,
            n: None,
            mode: None,
            anchor: None,
            octaves: 1,
        }
    }
}

/// Render a chord symbol into a list of MIDI notes (port of `renderVoicing`).
fn render_voicing(chord: &str, opts: &VoicingOpts) -> Option<Vec<i32>> {
    let dict = dictionary(&opts.dict);
    let mode = opts.mode.unwrap_or(dict.mode);
    let anchor = opts
        .anchor
        .or_else(|| note_to_midi_with_octave(dict.anchor, 4))?;

    let (root, symbol) = tokenize_chord(chord)?;
    let root_chroma = pc_to_chroma(&root)?;
    let anchor_chroma = anchor.rem_euclid(12);

    let normalized = normalize_symbol(&symbol);
    let voicing_defs = dict
        .voicings(symbol.as_str())
        .or_else(|| dict.voicings(normalized))?;
    let voicings: Vec<Vec<i32>> = voicing_defs
        .iter()
        .map(|v| {
            v.split_whitespace()
                .filter_map(interval_to_semitones)
                .collect()
        })
        .collect();
    if voicings.iter().any(|v| v.is_empty()) {
        return None;
    }

    // Pick the voicing whose top/bottom note sits closest below the anchor.
    let mut min_distance: Option<i32> = None;
    let mut best_index = 0;
    let chroma_diffs: Vec<i32> = voicings
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let diff = (anchor_chroma - mode.target(v) - root_chroma).rem_euclid(12);
            if min_distance.is_none_or(|m| diff < m) {
                min_distance = Some(diff);
                best_index = i;
            }
            diff
        })
        .collect();
    if mode == Mode::Root {
        best_index = 0;
    }

    let len = voicings.len() as i32;
    let oct_diff = (opts.offset as f64 / len as f64).ceil() as i32 * 12;
    let index = (best_index as i32 + opts.offset).rem_euclid(len) as usize;
    let voicing = &voicings[index];
    let target_step = mode.target(voicing);
    let anchor_midi = anchor - chroma_diffs[index] + oct_diff;
    let voicing_midi: Vec<i32> = voicing
        .iter()
        .map(|v| anchor_midi - target_step + v)
        .collect();

    let notes: Vec<i32> = if mode == Mode::Duck {
        voicing_midi.into_iter().filter(|&m| m != anchor).collect()
    } else {
        voicing_midi
    };

    match opts.n {
        Some(n) => Some(vec![scale_step_in(&notes, n, opts.octaves)]),
        None => Some(notes),
    }
}

/// The MIDI note an `anchor` control refers to, which is upstream's
/// `x2midi(anchor?.note || anchor, 4)`.
///
/// The anchor is often a whole event rather than a scalar — `.anchor(melody)`
/// stores the melody's control map under the key, and the note to voice against
/// is the `note` inside it. A bare name or number is used as it stands, and an
/// octave-less name defaults to octave 4.
fn anchor_midi(value: &Value) -> Option<i32> {
    let target = match value {
        Value::Map(m) => m.get("note").unwrap_or(value),
        other => other,
    };
    match target {
        Value::Str(s) => note_to_midi_with_octave(s, 4),
        other => other.as_f64().map(|f| f.round() as i32),
    }
}

/// Extract a voicing's controls from a hap value (chord string, or a map with a
/// `chord` key plus optional `dict`/`anchor`/`mode`/`offset`/`octaves`/`n`).
/// Returns `(chord, opts, extra_controls)`.
fn opts_from_value(value: &Value) -> Option<(String, VoicingOpts, ValueMap)> {
    match value {
        Value::Map(m) => {
            let chord = chord_symbol(m.get("chord")?)?;
            let mut opts = VoicingOpts::default();
            if let Some(d) = m
                .get("dictionary")
                .or_else(|| m.get("dict"))
                .and_then(|v| v.as_str())
            {
                opts.dict = d.to_string();
            }
            if let Some(a) = m.get("anchor") {
                opts.anchor = anchor_midi(a);
            }
            if let Some(mode) = m.get("mode").and_then(|v| v.as_str()) {
                opts.mode = Some(Mode::from_str(mode));
            }
            if let Some(o) = m.get("offset").and_then(|v| v.as_f64()) {
                opts.offset = o.round() as i32;
            }
            if let Some(o) = m.get("octaves").and_then(|v| v.as_f64()) {
                opts.octaves = o.round() as i32;
            }
            if let Some(n) = m.get("n").and_then(|v| v.as_f64()) {
                opts.n = Some(n.round() as i32);
            }
            // Everything except the voicing controls is merged onto the output.
            let mut extra = m.clone();
            for k in [
                "chord",
                "dictionary",
                "dict",
                "anchor",
                "mode",
                "offset",
                "octaves",
                "n",
            ] {
                extra.shift_remove(k);
            }
            Some((chord, opts, extra))
        }
        other => Some((
            chord_symbol(other)?,
            VoicingOpts::default(),
            ValueMap::new(),
        )),
    }
}

/// Build a stacked note pattern for one chord, merging any extra controls.
fn voicing_pattern(chord: &str, opts: &VoicingOpts, extra: &ValueMap) -> Pattern {
    match render_voicing(chord, opts) {
        Some(notes) => {
            let pats: Vec<Pattern> = notes
                .into_iter()
                .map(|midi| {
                    // Always a `note` control, never a bare number: upstream
                    // ends with `stack(...notes).note().set(rest)`. A bare
                    // number survives on its own but composes wrongly — the
                    // `.add(note("0,.1"))` a tune uses to detune a voicing
                    // unions `{note: 0}` onto `{value: 58}` and leaves the
                    // voiced note sitting in `value` with the control unset.
                    let mut map = extra.clone();
                    map.insert("note".to_string(), Value::F64(midi as f64));
                    pure(Value::Map(map))
                })
                .collect();
            stack(&pats)
        }
        None => silence(),
    }
}

impl Pattern {
    /// Turn chord symbols into voicings (`voicing`). Values may be chord strings
    /// (e.g. `"C^7"`) or maps with a `chord` key plus optional
    /// `dict`/`anchor`/`mode`/`offset`/`octaves`/`n` controls. Uses the `ireal`
    /// dictionary by default (matching Strudel's `defaultDict`).
    pub fn voicing(&self) -> Pattern {
        self.outer_bind(|value| match opts_from_value(&value) {
            Some((chord, opts, extra)) => voicing_pattern(&chord, &opts, &extra),
            None => silence(),
        })
    }

    /// Like [`voicing`](Self::voicing) but with an explicit dictionary name
    /// (`ireal`, `ireal-ext`, `lefthand`, `triads`, `guidetones`, or `legacy`).
    pub fn voicings(&self, dict: impl Into<String>) -> Pattern {
        let dict = dict.into();
        self.outer_bind(move |value| match opts_from_value(&value) {
            Some((chord, mut opts, extra)) => {
                opts.dict = dict.clone();
                voicing_pattern(&chord, &opts, &extra)
            }
            None => silence(),
        })
    }

    /// Map chord symbols to their root note in the given octave (`rootNotes`).
    ///
    /// The root comes out as a *name* (`"C4"`), which is what upstream builds
    /// (`root + octave`) and what the rest of the chain then reads it as. A MIDI
    /// number here looks identical to a scale degree, so a following `.scale()`
    /// takes the note-quantising branch for one and the degree branch for the
    /// other: `rootNotes(4).scale('C minor')` read 60 as degree 60 and landed
    /// eight octaves up.
    pub fn root_notes(&self, octave: i64) -> Pattern {
        let octave = octave as i32;
        self.with_value(move |value| {
            let chord = match &value {
                Value::Map(m) => m.get("chord").and_then(chord_symbol),
                other => chord_symbol(other),
            };
            let Some(chord) = chord else { return value };
            let Some((root, _)) = tokenize_chord(&chord) else {
                return value;
            };
            let name = format!("{root}{octave}");
            // Upstream concatenates without checking; keep the guard so an
            // unparseable root passes the value through instead of inventing one.
            if note_to_midi_with_octave(&name, octave).is_none() {
                return value;
            }
            match value {
                Value::Map(mut m) => {
                    m.insert("note".to_string(), Value::Str(name));
                    Value::Map(m)
                }
                _ => Value::Str(name),
            }
        })
    }
}
