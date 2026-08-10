//! Comparing a Rudel hap against the one Strudel produced.
//!
//! Shared by the corpora that carry expected haps — `tunes.rs` (upstream's own
//! committed snapshot of the website tunes) and `stepwise_parity.rs` (haps
//! dumped from the real engine by `gen_stepwise_oracle.mjs`) — so the two
//! cannot drift on what "the same event" means. Both goldens record a hap as
//! `{wb, b, e, we, v}`.
//!
//! Not every corpus uses every helper, and a test binary only compiles what it
//! calls.
#![allow(dead_code)]

use rudel_core::{Hap, Value};
use std::collections::BTreeMap;

/// Decimals kept when comparing a control value. Both engines compute in `f64`
/// and Strudel's snapshot prints full precision, so the two agree far past this
/// when they agree at all; anything finer is inaudible and is only `f64` noise
/// arriving by a different route. Upstream's own tune test rounds to 12 for the
/// same reason.
pub const VALUE_PRECISION: usize = 9;

/// Render a number the same way from either side, so a rudel `Int(1)` and a
/// JSON `1` compare equal without `1.0` vs `1` mattering.
pub fn number(x: f64) -> String {
    let s = format!("{x:.VALUE_PRECISION$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// One control value as a comparable string. Deliberately matches on the
/// variant rather than going through `Value::as_f64`, which parses strings as
/// numbers — a note that arrived as `"A4"` where Strudel emitted `55` is a real
/// difference and has to stay visible.
pub fn control_value(v: &Value) -> String {
    match v {
        Value::Int(n) => number(*n as f64),
        Value::F64(n) => number(*n),
        Value::Frac(f) => number(f.to_f64()),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

pub fn json_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => number(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Resolve a note *name* to its MIDI number, for the `note` control only.
///
/// Strudel carries `note` as whatever the pattern produced and converts it in
/// superdough at playback (`valueToMidi`); Rudel resolves it when the control is
/// set. So `note:A4` and `note:69` are the same event written two ways, and
/// comparing them literally would bury every real difference under a spelling
/// one. Only names that parse are rewritten — anything else stays as it is, so
/// a genuinely wrong value still shows up.
pub fn normalize_note(key: &str, value: String) -> String {
    if key != "note" {
        return value;
    }
    match rudel_core::note_to_midi(&value) {
        Some(midi) => number(midi as f64),
        None => value,
    }
}

/// A hap as one canonical line: the whole and part spans, then the controls in
/// key order. Both sides render through this, so the comparison is a multiset
/// difference over strings and reports insertions and deletions rather than
/// stopping at the first index that disagrees.
pub fn line(wb: &str, b: &str, e: &str, we: &str, controls: &BTreeMap<String, String>) -> String {
    let body: Vec<String> = controls.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    format!("[{wb} <= {b} -> {e} => {we} | {}]", body.join(" "))
}

pub fn rudel_line(hap: &Hap) -> String {
    let whole = hap.whole_or_part();
    let controls = match &hap.value {
        Value::Map(m) => m
            .iter()
            .map(|(k, v)| (k.to_string(), normalize_note(k, control_value(v))))
            .collect(),
        // A bare value has no control name upstream either; `show` prints it
        // alone, and the oracle records that as an unparseable hap, so a tune
        // reaching here will report as a plain difference.
        other => BTreeMap::from([(String::new(), control_value(other))]),
    };
    line(
        &whole.begin.to_string(),
        &hap.part.begin.to_string(),
        &hap.part.end.to_string(),
        &whole.end.to_string(),
        &controls,
    )
}

pub fn expected_line(hap: &serde_json::Value) -> String {
    let s = |k: &str| hap[k].as_str().unwrap_or_default().to_string();
    let controls = hap["v"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), normalize_note(k, json_value(v))))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    line(&s("wb"), &s("b"), &s("e"), &s("we"), &controls)
}

/// `+n` for haps only Rudel produced, `-n` for haps only Strudel produced.
pub fn multiset_diff(ours: &[String], theirs: &[String]) -> BTreeMap<String, i64> {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for l in ours {
        *counts.entry(l.clone()).or_default() += 1;
    }
    for l in theirs {
        *counts.entry(l.clone()).or_default() -= 1;
    }
    counts.retain(|_, n| *n != 0);
    counts
}
