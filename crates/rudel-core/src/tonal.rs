// tonal.rs - note names, scales, and pitch transforms. Ported from
// strudel/packages/tonal/{tonal,tonleiter}.mjs (which lean on @tonaljs/tonal);
// here the scale-interval and chord tables are inlined so rudel-core has no
// external music-theory dependency.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::hap::Hap;
use crate::pattern::{Pattern, pure, silence, stack};
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

/// Semitone offsets for the seven note letters from C.
pub(crate) fn letter_semitone(letter: char) -> Option<i32> {
    Some(match letter.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    })
}

/// Parse a note name like `c`, `c4`, `c#4`, `eb3`, `Gb2` to a MIDI number.
/// Follows the Strudel convention: `a4` = 69, and a missing octave defaults to 3.
pub fn note_to_midi(s: &str) -> Option<i32> {
    note_to_midi_with_octave(s, 3)
}

/// Like [`note_to_midi`] but with a caller-supplied default octave for names
/// that omit one (Strudel's `x2midi` uses octave 4 for voicing anchors).
pub fn note_to_midi_with_octave(s: &str, default_octave: i32) -> Option<i32> {
    let mut chars = s.chars().peekable();
    let mut semis = letter_semitone(chars.next()?)?;
    let mut octave: i32 = default_octave;
    let mut octave_str = String::new();
    let mut octave_seen = false;
    while let Some(&c) = chars.peek() {
        match c {
            's' | '#' => {
                semis += 1;
                chars.next();
            }
            'b' => {
                semis -= 1;
                chars.next();
            }
            '-' | '0'..='9' => {
                octave_str.push(c);
                octave_seen = true;
                chars.next();
            }
            _ => return None,
        }
    }
    if octave_seen {
        octave = octave_str.parse().ok()?;
    }
    Some((octave + 1) * 12 + semis)
}

/// True if `s` parses as a note name (used to disambiguate scale roots and
/// scale-degree values).
pub fn is_note_name(s: &str) -> bool {
    note_to_midi(s).is_some()
}

/// Parse an interval string (e.g. `"3M"`, `"5P"`, `"11A"`, `"-2M"`) to
/// semitones. Quality may precede or follow the number (`"M3"` == `"3M"`); a
/// leading `-` denotes a descending interval. Bare numbers pass through as a
/// semitone count. Compound intervals (9th, 11th, …) wrap by octaves.
pub fn interval_to_semitones(s: &str) -> Option<i32> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return Some(n);
    }
    let (sign, body) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s),
    };
    let digits: String = body.chars().filter(|c| c.is_ascii_digit()).collect();
    let quality: String = body
        .chars()
        .filter(|c| matches!(c, 'd' | 'm' | 'M' | 'P' | 'A'))
        .collect();
    let num: i32 = digits.parse().ok()?;
    if num < 1 {
        return None;
    }
    let step = (num - 1) % 7;
    let oct = (num - 1) / 7;
    let base = [0, 2, 4, 5, 7, 9, 11][step as usize] + 12 * oct;
    // 1, 4, 5 (and their octave-equivalents) are the perfect family.
    let perfect = matches!(step, 0 | 3 | 4);
    Some(sign * (base + interval_quality_alteration(&quality, perfect)?))
}

/// Semitone alteration for an interval quality (`P`/`M`/`m`/`A..`/`d..`).
fn interval_quality_alteration(q: &str, perfect: bool) -> Option<i32> {
    match q {
        "P" if perfect => Some(0),
        "M" if !perfect => Some(0),
        "m" if !perfect => Some(-1),
        _ if !q.is_empty() && q.chars().all(|c| c == 'A') => Some(q.len() as i32),
        _ if !q.is_empty() && q.chars().all(|c| c == 'd') => {
            let k = q.len() as i32;
            Some(if perfect { -k } else { -(k + 1) })
        }
        _ => None,
    }
}

/// Interpret a value as a transpose amount in semitones: numbers pass through;
/// strings parse as a number first, then as an interval name (`"3M"`).
fn value_to_semitones(v: &Value) -> f64 {
    match v {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| interval_to_semitones(s).map(|i| i as f64))
            .unwrap_or(0.0),
        other => other.as_f64().unwrap_or(0.0),
    }
}

/// Coerce a value to a MIDI note number: numbers pass through, note-name strings
/// are parsed.
fn value_to_midi(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::F64(n) => Some(*n),
        Value::Frac(f) => Some(f.to_f64()),
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| note_to_midi(s).map(|m| m as f64)),
        _ => None,
    }
}

/// Scale-type name (lowercased, spaces normalised) → semitone intervals.
fn scale_intervals(name: &str) -> Option<&'static [i32]> {
    let n = name.trim().to_lowercase();
    Some(match n.as_str() {
        "major" | "ionian" => &[0, 2, 4, 5, 7, 9, 11],
        "minor" | "aeolian" => &[0, 2, 3, 5, 7, 8, 10],
        "dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "phrygian" => &[0, 1, 3, 5, 7, 8, 10],
        "lydian" => &[0, 2, 4, 6, 7, 9, 11],
        "mixolydian" => &[0, 2, 4, 5, 7, 9, 10],
        "locrian" => &[0, 1, 3, 5, 6, 8, 10],
        "harmonic minor" => &[0, 2, 3, 5, 7, 8, 11],
        "melodic minor" => &[0, 2, 3, 5, 7, 9, 11],
        "major pentatonic" | "pentatonic" => &[0, 2, 4, 7, 9],
        "minor pentatonic" => &[0, 3, 5, 7, 10],
        "ritusen" => &[0, 2, 5, 7, 9],
        "egyptian" => &[0, 2, 5, 7, 10],
        "whole tone" | "whole" => &[0, 2, 4, 6, 8, 10],
        "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "blues" | "minor blues" => &[0, 3, 5, 6, 7, 10],
        "major blues" => &[0, 2, 3, 4, 7, 9],
        "bebop major" => &[0, 2, 4, 5, 7, 8, 9, 11],
        "bebop" | "bebop dominant" => &[0, 2, 4, 5, 7, 9, 10, 11],
        "diminished" | "whole half diminished" => &[0, 2, 3, 5, 6, 8, 9, 11],
        "half whole diminished" => &[0, 1, 3, 4, 6, 7, 9, 10],
        "augmented" => &[0, 3, 4, 7, 8, 11],
        "hirajoshi" => &[0, 2, 3, 7, 8],
        "in" | "iwato" => &[0, 1, 5, 6, 10],
        _ => return None,
    })
}

/// Split a scale spec like `"C:major"`, `"c4:harmonic:minor"`, or `"major"`
/// into `(root_midi, intervals)`. The root defaults to `C` (octave 3).
fn parse_scale(scale: &str) -> Option<(i32, &'static [i32])> {
    let parts: Vec<&str> = scale.split(':').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    // If the first token is a note name, it is the root; the rest is the type.
    let (root, type_parts) = if is_note_name(parts[0]) && parts.len() > 1 {
        (note_to_midi(parts[0])?, &parts[1..])
    } else if is_note_name(parts[0]) && scale_intervals(parts[0]).is_none() {
        // a bare root with no type -> default major
        (note_to_midi(parts[0])?, &parts[..0])
    } else {
        (note_to_midi("c")?, &parts[..])
    };
    let type_name = if type_parts.is_empty() {
        "major".to_string()
    } else {
        type_parts.join(" ")
    };
    Some((root, scale_intervals(&type_name)?))
}

fn floor_div(a: i32, b: i32) -> i32 {
    (a as f64 / b as f64).floor() as i32
}

fn modulo(a: i32, b: i32) -> i32 {
    a.rem_euclid(b)
}

/// Map a zero-indexed scale degree to a MIDI note (`scaleStep`). Degrees beyond
/// the scale length wrap into higher/lower octaves.
pub fn scale_step(step: i32, scale: &str) -> Option<i32> {
    let (root, intervals) = parse_scale(scale)?;
    let len = intervals.len() as i32;
    let octave_offset = floor_div(step, len);
    let idx = modulo(step, len) as usize;
    Some(root + intervals[idx] + 12 * octave_offset)
}

/// Transpose a note (already in the scale) by `offset` scale steps
/// (`scaleOffset`/`scaleTranspose`).
pub fn scale_offset(scale: &str, offset: i32, note_midi: i32) -> Option<i32> {
    let (root, intervals) = parse_scale(scale)?;
    let len = intervals.len() as i32;
    let rel = note_midi - root;
    let base_oct = floor_div(rel, 12);
    let chroma = modulo(rel, 12);
    // Find the degree whose interval matches (or is nearest to) this chroma.
    let idx = intervals
        .iter()
        .position(|&i| i == chroma)
        .unwrap_or_else(|| {
            intervals
                .iter()
                .enumerate()
                .min_by_key(|&(_, &i)| (i - chroma).abs())
                .map(|(j, _)| j)
                .unwrap_or(0)
        }) as i32;
    let new_index = idx + offset;
    let octave_offset = base_oct + floor_div(new_index, len);
    let i = modulo(new_index, len) as usize;
    Some(root + intervals[i] + 12 * octave_offset)
}

/// Fold the `mtranspose` (modal / scale-step) and `ctranspose` (chromatic /
/// semitone) controls into the `note` value, matching SuperDirt's external-synth
/// pitch handling: the note is shifted `mtranspose` steps within `scale` (the
/// scale tagged on the hap, defaulting to `C:major`), then `ctranspose`
/// semitones on top. The two controls are consumed (removed) once applied.
///
/// Only folds when a `note` is present; otherwise the controls are left in place
/// so an external synth can still interpret them.
pub fn apply_transpose_controls(controls: &mut BTreeMap<String, Value>, scale: Option<&str>) {
    if !controls.contains_key("mtranspose") && !controls.contains_key("ctranspose") {
        return;
    }
    let Some(mut midi) = controls.get("note").and_then(value_to_midi) else {
        return;
    };
    if let Some(steps) = controls.get("mtranspose").and_then(|v| v.as_f64())
        && let Some(new) = scale_offset(
            scale.unwrap_or("C:major"),
            steps.round() as i32,
            midi.round() as i32,
        )
    {
        midi = new as f64;
    }
    if let Some(semis) = controls.get("ctranspose").and_then(|v| v.as_f64()) {
        midi += semis;
    }
    controls.insert("note".to_string(), Value::F64(midi));
    controls.remove("mtranspose");
    controls.remove("ctranspose");
}

/// Chord-symbol suffix → semitone intervals from the root.
fn chord_intervals(symbol: &str) -> Option<&'static [i32]> {
    Some(match symbol {
        "" | "M" | "maj" | "major" => &[0, 4, 7],
        "m" | "min" | "minor" | "-" => &[0, 3, 7],
        "dim" | "o" => &[0, 3, 6],
        "aug" | "+" => &[0, 4, 8],
        "6" | "maj6" => &[0, 4, 7, 9],
        "m6" | "min6" => &[0, 3, 7, 9],
        "7" | "dom7" => &[0, 4, 7, 10],
        "maj7" | "M7" | "^7" => &[0, 4, 7, 11],
        "m7" | "min7" | "-7" => &[0, 3, 7, 10],
        "m7b5" | "halfdim" | "ø" => &[0, 3, 6, 10],
        "dim7" | "o7" => &[0, 3, 6, 9],
        "sus2" => &[0, 2, 7],
        "sus4" | "sus" => &[0, 5, 7],
        "add9" => &[0, 4, 7, 14],
        "9" => &[0, 4, 7, 10, 14],
        "maj9" | "M9" => &[0, 4, 7, 11, 14],
        "m9" | "min9" => &[0, 3, 7, 10, 14],
        _ => return None,
    })
}

/// Render a value as a chord symbol. Strings pass through; `:`-list tails like
/// `["C", "maj7"]` (how mini-notation spells `c:maj7`) are joined into
/// `"Cmaj7"`. Other value types yield `None`.
pub(crate) fn chord_symbol(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::List(items) if !items.is_empty() => Some(items.iter().map(chord_token).collect()),
        _ => None,
    }
}

/// Render a single chord-symbol list element as a token (e.g. the `7` in
/// `["g", 7]` -> `"7"`), dropping a redundant `.0` on integral floats.
fn chord_token(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) if x.fract() == 0.0 => (*x as i64).to_string(),
        Value::F64(x) => x.to_string(),
        Value::Frac(f) => chord_token(&Value::F64(f.to_f64())),
        _ => String::new(),
    }
}

/// Parse a chord name like `"C"`, `"Am"`, `"F#maj7"`, `"Bb7"` into its MIDI
/// notes.
pub fn chord_notes(name: &str) -> Option<Vec<i32>> {
    // Root: a letter, optional accidentals, optional octave digits.
    let mut split = 1;
    let bytes: Vec<char> = name.chars().collect();
    if bytes.is_empty() {
        return None;
    }
    while split < bytes.len() && matches!(bytes[split], '#' | 'b' | 's') {
        split += 1;
    }
    while split < bytes.len() && (bytes[split].is_ascii_digit() || bytes[split] == '-') {
        // octave digits belong to the root
        let rest: String = bytes[split..].iter().collect();
        // only consume as octave if what's left is still a valid suffix
        if chord_intervals(&rest).is_some() {
            break;
        }
        split += 1;
    }
    let root: String = bytes[..split].iter().collect();
    let symbol: String = bytes[split..].iter().collect();
    let root_midi = note_to_midi(&root)?;
    let intervals = chord_intervals(&symbol)?;
    Some(intervals.iter().map(|i| root_midi + i).collect())
}

/// Add `semis` semitones to a single value (number, note string, or a map
/// carrying a `note` key).
fn transpose_value(v: Value, semis: f64) -> Value {
    match v {
        Value::Map(mut m) => {
            if let Some(note) = m.get("note")
                && let Some(midi) = value_to_midi(note)
            {
                m.insert("note".to_string(), Value::F64(midi + semis));
            }
            Value::Map(m)
        }
        other => match value_to_midi(&other) {
            Some(midi) => Value::F64(midi + semis),
            None => other,
        },
    }
}

impl Pattern {
    /// Map scale-degree numbers to notes in `scale` (and quantise note names to
    /// it). Tags each hap with the scale for [`scale_transpose`](Self::scale_transpose).
    /// `scale` may be a string or a pattern of scale names.
    pub fn scale(&self, scale: impl IntoPattern) -> Pattern {
        let arg = scale.into_pattern();
        let pat = self.clone();
        if let Some(v) = &arg.pure_value {
            let name = v.as_str().unwrap_or("C:major").to_string();
            return pat.apply_scale(name);
        }
        arg.fmap(move |v| {
            let name = v.as_str().unwrap_or("C:major").to_string();
            Value::Pat(Box::new(pat.apply_scale(name)))
        })
        .inner_join()
    }

    /// Apply a fixed scale name to every hap.
    fn apply_scale(&self, scale: String) -> Pattern {
        let steps = self.steps;
        self.with_haps(move |haps, _| {
            haps.into_iter()
                .filter_map(|hap| apply_scale_to_hap(hap, &scale))
                .collect()
        })
        .set_steps(steps)
    }

    /// Shift each note by a number of semitones, or by a named interval string
    /// (`"3M"`, `"5P"`, `"-2M"`) — or a pattern of either.
    pub fn transpose(&self, semis: impl IntoPattern) -> Pattern {
        let arg = semis.into_pattern();
        let pat = self.clone();
        if let Some(v) = &arg.pure_value {
            let s = value_to_semitones(v);
            return pat.with_value(move |val| transpose_value(val, s));
        }
        arg.fmap(move |v| {
            let s = value_to_semitones(&v);
            Value::Pat(Box::new(pat.with_value(move |val| transpose_value(val, s))))
        })
        .inner_join()
    }

    /// Alias for [`transpose`](Self::transpose) (`trans`).
    pub fn trans(&self, semis: impl IntoPattern) -> Pattern {
        self.transpose(semis)
    }

    /// Alias for [`scale_transpose`](Self::scale_transpose) (`scaleTrans`/`strans`).
    pub fn strans(&self, offset: impl IntoPattern) -> Pattern {
        self.scale_transpose(offset)
    }

    /// Transpose each note by `offset` steps *within* the scale tagged by a
    /// previous `.scale(...)` (`scaleTranspose`).
    pub fn scale_transpose(&self, offset: impl IntoPattern) -> Pattern {
        let arg = offset.into_pattern();
        let pat = self.clone();
        let apply = move |off: i32| pat.with_hap(move |hap| scale_transpose_hap(hap, off));
        if let Some(v) = &arg.pure_value {
            return apply(v.as_f64().unwrap_or(0.0) as i32);
        }
        arg.fmap(move |v| Value::Pat(Box::new(apply(v.as_f64().unwrap_or(0.0) as i32))))
            .inner_join()
    }

    /// Turn a pattern of chord names into stacks of simultaneous notes
    /// (`chord`). Unknown names produce silence.
    pub fn chord(&self) -> Pattern {
        self.bind(
            |v| match chord_symbol(&v).as_deref().and_then(chord_notes) {
                Some(notes) => {
                    let pats: Vec<Pattern> = notes
                        .into_iter()
                        .map(|m| pure(Value::Int(m as i64)))
                        .collect();
                    stack(&pats)
                }
                None => silence(),
            },
        )
    }
}

/// Convert one hap's value under a fixed scale (the per-hap body of `scale`).
fn apply_scale_to_hap(hap: Hap, scale: &str) -> Option<Hap> {
    let mut context = hap.context.clone();
    context.scale = Some(scale.to_string());
    let new_value = match &hap.value {
        Value::Map(m) => {
            // Prefer note, then n, then value as the degree/note source.
            let source = m
                .get("note")
                .or_else(|| m.get("n"))
                .or_else(|| m.get("value"))
                .cloned();
            let Some(source) = source else {
                return Some(hap.set_context(context));
            };
            let note = scale_resolve(&source, scale)?;
            let mut out = m.clone();
            out.remove("n");
            out.remove("value");
            out.insert("note".to_string(), Value::F64(note));
            Value::Map(out)
        }
        other => Value::F64(scale_resolve(other, scale)?),
    };
    Some(Hap::new(hap.whole, hap.part, new_value).with_context(context))
}

/// Resolve a single value against a scale: note names are quantised to the
/// nearest scale note; numbers are treated as scale degrees.
fn scale_resolve(v: &Value, scale: &str) -> Option<f64> {
    match v {
        Value::Str(s) if is_note_name(s) && s.parse::<f64>().is_err() => {
            let midi = note_to_midi(s)?;
            Some(nearest_scale_note(scale, midi)? as f64)
        }
        _ => {
            // numeric scale degree (supports trailing sharps/flats on strings)
            let (step, offset) = step_number_and_offset(v)?;
            let note = scale_step(step, scale)?;
            Some((note + offset) as f64)
        }
    }
}

/// Parse a scale-degree value, allowing string forms like `"3"`, `"-2"`, `"4#"`,
/// `"2b"`. Returns `(degree, semitone_offset)`.
fn step_number_and_offset(v: &Value) -> Option<(i32, i32)> {
    match v {
        Value::Int(n) => Some((*n as i32, 0)),
        Value::F64(n) => Some((n.round() as i32, 0)),
        Value::Frac(f) => Some((f.to_f64().round() as i32, 0)),
        Value::Str(s) => {
            let s = s.trim();
            let digits_end = s
                .char_indices()
                .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && *c == '-')))
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            let num: i32 = s[..digits_end].parse().ok()?;
            let offset = s[digits_end..]
                .chars()
                .map(|c| match c {
                    '#' | 's' => 1,
                    'b' | 'f' => -1,
                    _ => 0,
                })
                .sum();
            Some((num, offset))
        }
        _ => None,
    }
}

/// Quantise a MIDI note to the nearest note in the scale.
fn nearest_scale_note(scale: &str, note_midi: i32) -> Option<i32> {
    let (root, intervals) = parse_scale(scale)?;
    let rel = note_midi - root;
    let base_oct = floor_div(rel, 12);
    let chroma = modulo(rel, 12);
    let best = intervals
        .iter()
        .min_by_key(|&&i| (i - chroma).abs())
        .copied()
        .unwrap_or(0);
    Some(root + best + 12 * base_oct)
}

/// `scaleTranspose` body for a single hap.
fn scale_transpose_hap(hap: Hap, offset: i32) -> Hap {
    let Some(scale) = hap.context.scale.clone() else {
        return hap;
    };
    hap.with_value(|v| match v {
        Value::Map(mut m) => {
            if let Some(note) = m.get("note")
                && let Some(midi) = value_to_midi(note)
                && let Some(new) = scale_offset(&scale, offset, midi.round() as i32)
            {
                m.insert("note".to_string(), Value::F64(new as f64));
            }
            Value::Map(m)
        }
        other => match value_to_midi(&other)
            .and_then(|m| scale_offset(&scale, offset, m.round() as i32))
        {
            Some(new) => Value::F64(new as f64),
            None => other,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frac, fastcat, n, pure, sequence};

    fn notes(pat: &Pattern) -> Vec<f64> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m.get("note").and_then(|v| v.as_f64()).unwrap(),
                other => other.as_f64().unwrap(),
            })
            .collect()
    }

    #[test]
    fn note_names_to_midi() {
        assert_eq!(note_to_midi("a4"), Some(69));
        assert_eq!(note_to_midi("c4"), Some(60));
        assert_eq!(note_to_midi("c"), Some(48)); // default octave 3
        assert_eq!(note_to_midi("c#4"), Some(61));
        assert_eq!(note_to_midi("eb3"), Some(51));
        assert_eq!(note_to_midi("gb2"), Some(42));
        assert_eq!(note_to_midi("x"), None);
    }

    #[test]
    fn scale_degrees_map_to_notes() {
        // C major from C3 (=48): degrees 0..6 -> 48 50 52 53 55 57 59
        assert_eq!(scale_step(0, "C:major"), Some(48));
        assert_eq!(scale_step(1, "C:major"), Some(50));
        assert_eq!(scale_step(2, "C:major"), Some(52));
        // wrap to next octave
        assert_eq!(scale_step(7, "C:major"), Some(60));
        // negative wraps down
        assert_eq!(scale_step(-1, "C:major"), Some(47));
    }

    #[test]
    fn scale_with_root_octave() {
        // C4 major degree 0 = 60
        assert_eq!(scale_step(0, "C4:major"), Some(60));
    }

    #[test]
    fn scale_transform_on_n_pattern() {
        let pat = n(sequence(&[
            pure(Value::Int(0)),
            pure(Value::Int(2)),
            pure(Value::Int(4)),
        ]))
        .scale("C:major");
        assert_eq!(notes(&pat), vec![48.0, 52.0, 55.0]);
    }

    #[test]
    fn transpose_adds_semitones() {
        let pat = n(fastcat(&[pure(Value::Int(0)), pure(Value::Int(12))])).transpose(7);
        // n becomes note? no: transpose operates on raw values here -> n stays n.
        // Use note() instead for a clean check:
        let pat2 = crate::note(fastcat(&[pure(Value::Int(60))])).transpose(7);
        let _ = pat;
        assert_eq!(notes(&pat2), vec![67.0]);
    }

    #[test]
    fn scale_transpose_moves_within_scale() {
        // degree 0 of C major (=48), scaleTranspose +2 -> degree 2 (=52)
        let pat = n(pure(Value::Int(0))).scale("C:major").scale_transpose(2);
        assert_eq!(notes(&pat), vec![52.0]);
    }

    #[test]
    fn note_name_quantises_to_scale() {
        // c#3 (=49) quantised to C major -> nearest is C(48) or D(50); ties -> C
        let pat = crate::note(pure(Value::Str("c#3".into()))).scale("C:major");
        let got = notes(&pat)[0];
        assert!(got == 48.0 || got == 50.0);
    }

    #[test]
    fn chord_expands_to_notes() {
        let pat = pure(Value::Str("C".into())).chord();
        let mut vals: Vec<i32> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value.as_f64().unwrap() as i32)
            .collect();
        vals.sort();
        assert_eq!(vals, vec![48, 52, 55]); // C E G from C3
    }

    #[test]
    fn chord_reads_list_backed_symbols() {
        // mini spells `c:maj7` as the list ["c", "maj7"]; `.chord()` joins it.
        let pat = pure(Value::List(vec![
            Value::Str("c".into()),
            Value::Str("maj7".into()),
        ]))
        .chord();
        let mut vals: Vec<i32> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value.as_f64().unwrap() as i32)
            .collect();
        vals.sort();
        assert_eq!(vals, vec![48, 52, 55, 59]); // C E G B (Cmaj7 from C3)
        // numeric tails join too: ["g", 7] -> "g7".
        let pat = pure(Value::List(vec![Value::Str("g".into()), Value::Int(7)])).chord();
        let mut vals: Vec<i32> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value.as_f64().unwrap() as i32)
            .collect();
        vals.sort();
        assert_eq!(vals, chord_notes("g7").unwrap());
    }

    #[test]
    fn chord_parses_symbols() {
        assert_eq!(chord_notes("Am"), Some(vec![57, 60, 64]));
        assert_eq!(chord_notes("C7"), Some(vec![48, 52, 55, 58]));
        assert_eq!(chord_notes("F#maj7"), Some(vec![54, 58, 61, 65]));
    }

    #[test]
    fn interval_strings_to_semitones() {
        for (s, want) in [
            ("1P", 0),
            ("3m", 3),
            ("3M", 4),
            ("5P", 7),
            ("5d", 6),
            ("5A", 8),
            ("7m", 10),
            ("8P", 12),
            ("9M", 14),
            ("11A", 18),
            ("M3", 4),   // quality-first order
            ("-2M", -2), // descending
            ("-5P", -7),
            ("4", 4), // bare number = semitones
        ] {
            assert_eq!(interval_to_semitones(s), Some(want), "interval {s}");
        }
    }

    #[test]
    fn transpose_accepts_interval_strings() {
        // C4 (=60) up a major third -> E4 (=64)
        let pat = crate::note(pure(Value::Int(60))).transpose("3M");
        assert_eq!(notes(&pat), vec![64.0]);
        // a pattern of interval strings transposes each event
        let intervals = fastcat(&[
            pure(Value::Str("5P".into())),
            pure(Value::Str("-2M".into())),
        ]);
        let pat = crate::note(fastcat(&[pure(Value::Int(60)), pure(Value::Int(60))]))
            .transpose(intervals);
        assert_eq!(notes(&pat), vec![67.0, 58.0]);
    }
}
