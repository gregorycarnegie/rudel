// edo.rs - Equal Division of the Octave (EDO) scales via MOS large/small-step
// notation. Ported from strudel/packages/edo/{edo,edoscale,intervals,pitches,
// ratios}.mjs (themselves ports of robmckinnon's `pitfalls` Lua library).
//
// An EDO scale definition is `root:sequence:large:small`, e.g. `C:LLsLLLs:2:1`
// (C major, 12-EDO): the `L`/`s` step sequence with large step 2 and small step
// 1 sums to 12 divisions. `edoScale` maps a numeric scale-degree pattern to
// notes/freqs in that tuning. The interactive `change*`/`set*` mutators from the
// upstream class are UI-only and not ported; only the construction + read path
// `edoScale` actually uses is here.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::pattern::{Pattern, silence};
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

const TUNING: f64 = 440.0;

// Whole-number ratios for pitch intervals (ratios.mjs), in the upstream insertion
// order that `nearestInterval` scans. Each entry is (numerator, denominator, key
// label). Generated from `@strudel/edo/ratios.mjs`.
const RATIO_INTERVALS: &[(i64, i64, &str)] = &[
    (1, 1, "P1"),
    (16, 15, "m2"),
    (15, 14, "A1"),
    (13, 12, "t2"),
    (12, 11, "N2"),
    (11, 10, "n2"),
    (10, 9, "T2"),
    (9, 8, "M2"),
    (8, 7, "S2"),
    (7, 6, "s3"),
    (19, 16, "o3"),
    (6, 5, "m3"),
    (17, 14, "t3"),
    (11, 9, "n3"),
    (5, 4, "M3"),
    (9, 7, "S3"),
    (13, 10, "d4"),
    (4, 3, "P4"),
    (19, 14, "N4"),
    (11, 8, "n4"),
    (25, 18, "a4"),
    (7, 5, "sT"),
    (45, 32, "A4"),
    (17, 12, "d5"),
    (10, 7, "ST"),
    (13, 9, "t5"),
    (3, 2, "P5"),
    (14, 9, "s6"),
    (25, 16, "a5"),
    (11, 7, "A5"),
    (8, 5, "m6"),
    (13, 8, "N6"),
    (18, 11, "n6"),
    (5, 3, "M6"),
    (128, 75, "d7"),
    (17, 10, "T6"),
    (12, 7, "S6"),
    (7, 4, "s7"),
    (16, 9, "m7"),
    (9, 5, "g7"),
    (11, 6, "n7"),
    (13, 7, "N7"),
    (15, 8, "M7"),
    (17, 9, "T7"),
    (19, 10, "d8"),
    (2, 1, "P8"),
];

/// The interval label nearest ratio `v` within 1% (`nearestInterval` + `key`),
/// or `""` if none is close enough.
fn nearest_interval_key(v: f64) -> &'static str {
    let mut min = 1.0_f64;
    let mut key = "";
    for &(num, den, k) in RATIO_INTERVALS {
        let ratio = num as f64 / den as f64;
        let diff = ((ratio - v) / ratio).abs();
        if diff < min {
            min = diff;
            key = k;
        }
    }
    if min < 0.01 { key } else { "" }
}

/// Round `x` to `dp` decimal places (mirrors JS `parseFloat(x.toFixed(dp))`).
fn round_dp(x: f64, dp: i32) -> f64 {
    let m = 10f64.powi(dp);
    (x * m).round() / m
}

fn ratio_pow(division: i64, edivisions: i64) -> f64 {
    if division == 0 {
        1.0
    } else {
        2f64.powf(division as f64 / edivisions as f64)
    }
}

fn midi_to_hz(n: f64) -> f64 {
    TUNING * 2f64.powf((n - 69.0) / 12.0)
}

fn hz_to_midi(freq: f64) -> f64 {
    12.0 * (freq / TUNING).log2() + 69.0
}

/// An EDO scale built from `(large, small, sequence)`: step types, per-step
/// sizes, cumulative divisions, and total divisions of the octave.
struct EdoScale {
    divisions: Vec<i64>,
    edivisions: i64,
    length: usize,
    tonic: i64,
}

impl EdoScale {
    fn new(large: i64, small: i64, sequence: &str) -> EdoScale {
        let medium = large; // upstream defaults medium = large
        // step types: 'L' -> large, 'M' -> medium, anything else -> small.
        let step_values: Vec<i64> = sequence
            .chars()
            .map(|c| match c {
                'L' => large,
                'M' => medium,
                _ => small,
            })
            .collect();
        let length = step_values.len();
        // divisions[i] is the running sum *before* step i; edivisions is the total.
        let mut divisions = Vec::with_capacity(length);
        let mut sum = 0;
        for &sv in &step_values {
            divisions.push(sum);
            sum += sv;
        }
        EdoScale {
            divisions,
            edivisions: sum,
            length,
            tonic: 1,
        }
    }

    /// Per-step sizes recovered from the cumulative `divisions` (+ total).
    fn step_value(&self, i: usize) -> i64 {
        let next = if i + 1 < self.length {
            self.divisions[i + 1]
        } else {
            self.edivisions
        };
        next - self.divisions[i]
    }
}

/// Interval ratios per degree plus their nearest-named labels.
struct Intervals {
    ratios: Vec<f64>,
    int_labels: Vec<Option<String>>,
}

impl Intervals {
    fn new(scale: &EdoScale) -> Intervals {
        let mut ratios = vec![1.0];
        // int_labels[0] is never assigned upstream (stays undefined -> null).
        let mut int_labels: Vec<Option<String>> = vec![None];
        let mut division = 0i64;
        for i in 0..scale.length {
            division += scale.step_value(i);
            let r = ratio_pow(division, scale.edivisions);
            ratios.push(r);
            let key = nearest_interval_key(r);
            int_labels.push(Some(key.to_string()));
        }
        Intervals { ratios, int_labels }
    }
}

/// Frequencies/MIDI per (octave, degree) for an EDO scale.
struct Pitches {
    edivisions: i64,
    length: usize,
    tonic: i64,
    root_octave: i64,
    base_freq: f64,
    base_freq_str: String,
    divisions: Vec<i64>,
    ratios: Vec<f64>,
    int_labels: Vec<Option<String>>,
}

impl Pitches {
    fn new(scale: EdoScale, intervals: Intervals, midi_start: f64, root_octave: i64) -> Pitches {
        let base_freq = midi_to_hz(midi_start);
        Pitches {
            edivisions: scale.edivisions,
            length: scale.length,
            tonic: scale.tonic,
            root_octave,
            base_freq,
            base_freq_str: format!("{base_freq:.4}"),
            divisions: scale.divisions,
            ratios: intervals.ratios,
            int_labels: intervals.int_labels,
        }
    }

    /// Frequency for the tonic in octave `oct` (`get_freq` with `index = tonic`).
    fn octave_base(&self, oct: i64) -> f64 {
        let f = self.base_freq * ratio_pow(self.tonic - 1, self.edivisions);
        if oct < self.root_octave {
            f / 2f64.powi((self.root_octave - oct) as i32)
        } else if oct > self.root_octave {
            f * 2f64.powi((oct - self.root_octave) as i32)
        } else {
            f
        }
    }

    /// Map a 1-indexed degree to `(octave, degree-in-scale)`, wrapping octaves.
    fn octdeg(&self, deg: i64) -> (i64, i64) {
        let len = self.length as i64;
        let higher = deg > len;
        let octave = self.root_octave + if higher { (deg - 1).div_euclid(len) } else { 0 };
        let degree = if higher {
            let m = deg.rem_euclid(len);
            if m == 0 { len } else { m }
        } else {
            deg
        };
        (octave, degree)
    }

    /// Frequency for a 1-indexed degree in octave `oct` (or `None` out of range).
    fn octdegfreq(&self, oct: i64, deg: i64) -> Option<f64> {
        if !(0..=8).contains(&oct) || deg < 1 || deg as usize > self.length {
            return None;
        }
        let f = self.octave_base(oct);
        Some(round_dp(f * self.ratios[(deg - 1) as usize], 3))
    }

    /// MIDI number for a 1-indexed degree in octave `oct` (or `None`).
    fn octdegmidi(&self, oct: i64, deg: i64) -> Option<f64> {
        if !(0..=8).contains(&oct) || deg < 1 || deg as usize > self.length {
            return None;
        }
        let f = self.octave_base(oct);
        Some(round_dp(hz_to_midi(f * self.ratios[(deg - 1) as usize]), 4))
    }

    fn degree_indexes(&self) -> Value {
        Value::List(self.divisions.iter().map(|&d| Value::Int(d)).collect())
    }

    fn int_labels_value(&self) -> Value {
        Value::List(
            self.int_labels
                .iter()
                .map(|l| match l {
                    Some(s) => Value::Str(s.clone()),
                    None => Value::Null,
                })
                .collect(),
        )
    }
}

/// Extract the octave number from a note name (trailing signed integer),
/// defaulting to 3.
fn note_octave(note: &str) -> i64 {
    let digits: String = note
        .chars()
        .skip_while(|c| !(c.is_ascii_digit() || *c == '-'))
        .collect();
    digits.parse::<i64>().unwrap_or(3)
}

/// Parse a value as a scale degree (`parseInt`/round semantics).
fn value_to_degree(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::F64(f) => Some(f.round() as i64),
        Value::Frac(f) => Some(f.to_f64().round() as i64),
        Value::Str(s) => {
            // parseInt: leading optional sign + digits.
            let t = s.trim();
            let mut end = 0;
            for (i, c) in t.char_indices() {
                if (i == 0 && c == '-') || c.is_ascii_digit() {
                    end = i + c.len_utf8();
                } else {
                    break;
                }
            }
            t[..end].parse::<i64>().ok()
        }
        _ => None,
    }
}

/// Parse a scale definition (`C:LLsLLLs:2:1` as a string, or the mini colon-list
/// `["C","LLsLLLs",2,1]`) into a `Pitches`.
fn build_pitches(def: &Value) -> Option<Pitches> {
    // Flatten to a list of tokens.
    let tokens: Vec<Value> = match def {
        Value::List(items) => {
            let mut out = Vec::new();
            flatten_into(items, &mut out);
            out
        }
        Value::Str(s) => s.split(':').map(|p| Value::Str(p.to_string())).collect(),
        _ => return None,
    };
    if tokens.len() < 4 {
        return None;
    }
    let base_note = tokens[0].as_str()?.to_string();
    let sequence = tokens[1].as_str()?.to_string();
    let large = value_to_degree(&tokens[2])?;
    let small = value_to_degree(&tokens[3])?;
    let root_octave = note_octave(&base_note);
    let midi_start = crate::tonal::note_to_midi(&base_note)? as f64;
    let scale = EdoScale::new(large, small, &sequence);
    if scale.length == 0 || scale.edivisions == 0 {
        return None;
    }
    let intervals = Intervals::new(&scale);
    Some(Pitches::new(scale, intervals, midi_start, root_octave))
}

fn flatten_into(items: &[Value], out: &mut Vec<Value>) {
    for it in items {
        match it {
            Value::List(inner) => flatten_into(inner, out),
            other => out.push(other.clone()),
        }
    }
}

/// Map one hap value through the EDO scale (the body of `edoScale`'s fmap).
fn edo_map_value(value: Value, p: &Pitches) -> Value {
    let (is_object, n_val) = match &value {
        Value::Map(m) => (true, m.get("n").cloned()),
        other => (false, Some(other.clone())),
    };
    let Some(n_val) = n_val else { return value };

    // Legacy: a note-name value passes straight through.
    if let Value::Str(s) = &n_val
        && crate::tonal::is_note_name(s)
        && s.parse::<f64>().is_err()
    {
        return n_val;
    }

    let Some(n) = value_to_degree(&n_val) else {
        return value;
    };
    let deg = n + 1;
    let (oct, degree) = p.octdeg(deg);

    if is_object {
        let mut m = match value {
            Value::Map(m) => m,
            _ => BTreeMap::new(),
        };
        m.remove("n");
        m.insert("degree".to_string(), Value::Int(degree));
        m.insert("degreeIndexes".to_string(), p.degree_indexes());
        m.insert("intLabels".to_string(), p.int_labels_value());
        m.insert("root".to_string(), Value::Str(p.base_freq_str.clone()));
        m.insert(
            "freq".to_string(),
            match p.octdegfreq(oct, degree) {
                Some(f) => Value::F64(f),
                None => Value::Null,
            },
        );
        m.insert("edo".to_string(), Value::Int(p.edivisions));
        Value::Map(m)
    } else {
        match p.octdegmidi(oct, degree) {
            Some(note) => Value::F64(note),
            None => value,
        }
    }
}

impl Pattern {
    /// Turn a numeric scale-degree pattern into notes/freqs in the given EDO
    /// scale (`edoScale`). The definition is `root:sequence:large:small`
    /// (e.g. `"C:LLsLLLs:2:1"`), as a colon string or a mini list. Bare values
    /// become MIDI notes; control maps gain `degree`/`degreeIndexes`/`intLabels`/
    /// `root`/`freq`/`edo` fields.
    pub fn edo_scale(&self, def: impl IntoPattern) -> Pattern {
        let arg = def.into_pattern();
        let pat = self.clone();
        if let Some(v) = &arg.pure_value {
            return pat.apply_edo_scale(v);
        }
        arg.fmap(move |v| Value::Pat(Box::new(pat.apply_edo_scale(&v))))
            .inner_join()
    }

    fn apply_edo_scale(&self, def: &Value) -> Pattern {
        let Some(pitches) = build_pitches(def) else {
            return silence();
        };
        let steps = self.steps;
        self.with_value(move |value| edo_map_value(value, &pitches))
            .set_steps(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frac, n, pure, sequence};

    fn pitches(pat: &Pattern) -> Vec<f64> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m.get("freq").and_then(Value::as_f64).unwrap(),
                other => other.as_f64().unwrap(),
            })
            .collect()
    }

    fn deg_pat(degrees: &[i64]) -> Pattern {
        sequence(&degrees.iter().map(|&d| pure(Value::Int(d))).collect::<Vec<_>>())
    }

    #[test]
    fn bare_12edo_c_major_maps_to_notes() {
        // C:LLsLLLs:2:1 is C major in 12-EDO, so degrees match the diatonic scale.
        let pat = deg_pat(&[0, 2, 4, 6]).edo_scale("C:LLsLLLs:2:1");
        assert_eq!(pitches(&pat), vec![48.0, 52.0, 55.0, 59.0]);
    }

    #[test]
    fn bare_16edo_gives_microtonal_midis() {
        // C:LLsLLL:3:1 is a 6-note scale in 16-EDO (3+3+1+3+3+3).
        let pat = deg_pat(&[0, 1, 2, 3, 4, 5, 6]).edo_scale("C:LLsLLL:3:1");
        assert_eq!(
            pitches(&pat),
            vec![48.0, 50.25, 52.5, 53.25, 55.5, 57.75, 60.0]
        );
    }

    #[test]
    fn degrees_wrap_into_octaves() {
        let pat = deg_pat(&[7, 8]).edo_scale("C:LLsLLLs:2:1");
        assert_eq!(pitches(&pat), vec![60.0, 62.0]);
    }

    #[test]
    fn object_input_carries_edo_metadata() {
        let pat = n(deg_pat(&[0, 2])).edo_scale("C:LLsLLLs:2:1");
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        let mut maps: Vec<&BTreeMap<String, Value>> = haps
            .iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => Some(m),
                _ => None,
            })
            .collect();
        maps.sort_by_key(|m| m.get("degree").and_then(Value::as_f64).unwrap() as i64);
        let first = maps[0];
        assert_eq!(first.get("degree"), Some(&Value::Int(1)));
        assert_eq!(first.get("edo"), Some(&Value::Int(12)));
        assert_eq!(first.get("root"), Some(&Value::Str("130.8128".into())));
        assert_eq!(first.get("freq").and_then(Value::as_f64), Some(130.813));
        assert_eq!(
            first.get("degreeIndexes"),
            Some(&Value::List(
                [0, 2, 4, 5, 7, 9, 11].iter().map(|&d| Value::Int(d)).collect()
            ))
        );
        // intLabels: [null, M2, M3, P4, P5, M6, T7, P8]
        let labels = match first.get("intLabels") {
            Some(Value::List(l)) => l.clone(),
            _ => panic!("intLabels missing"),
        };
        assert_eq!(labels[0], Value::Null);
        let label = |i: usize| labels[i].as_str().unwrap();
        assert_eq!(
            [label(1), label(2), label(3), label(4), label(5), label(6), label(7)],
            ["M2", "M3", "P4", "P5", "M6", "T7", "P8"]
        );
    }

    #[test]
    fn unknown_definition_is_silent() {
        let pat = deg_pat(&[0, 1]).edo_scale("not a scale");
        assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
    }
}
