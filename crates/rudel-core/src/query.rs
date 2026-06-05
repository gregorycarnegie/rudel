// query.rs - scheduler-agnostic extraction of timed control events from a
// pattern. Shared by the audio, MIDI and OSC back-ends so they all see the same
// onsets, timing and control maps.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::Pattern;
use crate::state::State;
use crate::timespan::TimeSpan;
use crate::value::Value;
use std::collections::BTreeMap;

/// A discrete onset with its control map and timing, in seconds on the caller's
/// clock (derived from `cps`).
#[derive(Clone, Debug)]
pub struct ControlEvent {
    /// Onset time in seconds (`onset_cycle / cps`).
    pub onset_seconds: f64,
    /// Event duration in seconds (`whole_duration / cps`).
    pub duration_seconds: f64,
    /// Onset time in cycles.
    pub onset_cycle: f64,
    /// The resolved control map (`note`, `s`, `gain`, ...).
    pub controls: BTreeMap<String, Value>,
}

/// Normalize a hap value into a control map: maps pass through; bare strings
/// become `{s}`; `name:index` lists become `{s, n}`; bare numbers become
/// `{note}`.
pub fn to_control_map(value: &Value) -> BTreeMap<String, Value> {
    match value {
        Value::Map(m) => m.clone(),
        Value::Str(_) => BTreeMap::from([("s".to_string(), value.clone())]),
        Value::List(items) if !items.is_empty() => {
            let mut m = BTreeMap::from([("s".to_string(), items[0].clone())]);
            if let Some(n) = items.get(1) {
                m.insert("n".to_string(), n.clone());
            }
            m
        }
        other => BTreeMap::from([("note".to_string(), other.clone())]),
    }
}

/// Query `pattern` over the cycle window `[begin_cycle, end_cycle)` and return a
/// [`ControlEvent`] for every onset, with times converted to seconds via `cps`.
/// Exposes `_cps` to cps-dependent transforms (`loopAt`/`fit`/`splice`).
pub fn query_controls(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
) -> Vec<ControlEvent> {
    if cps <= 0.0 || end_cycle <= begin_cycle {
        return Vec::new();
    }
    let begin = Frac::from_f64(begin_cycle);
    let end = Frac::from_f64(end_cycle);
    let controls = BTreeMap::from([("_cps".to_string(), Value::F64(cps))]);
    let state = State::with_controls(TimeSpan::new(begin, end), controls);

    let mut out = Vec::new();
    for hap in pattern.query(&state) {
        let Some(whole) = hap.whole else {
            continue; // skip continuous haps
        };
        if !hap.has_onset() {
            continue;
        }
        let onset_cycle = whole.begin.to_f64();
        if onset_cycle < begin_cycle || onset_cycle >= end_cycle {
            continue; // avoid duplicates across adjacent windows
        }
        out.push(ControlEvent {
            onset_seconds: onset_cycle / cps,
            duration_seconds: hap.duration().to_f64() / cps,
            onset_cycle,
            controls: to_control_map(&hap.value),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{note, pure, sequence, silence};

    #[test]
    fn onsets_and_timing() {
        // "bd ~ sd" at cps=1: onsets at 0 and 2/3
        let pat = sequence(&[
            pure(Value::Str("bd".into())),
            silence(),
            pure(Value::Str("sd".into())),
        ]);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert_eq!(evs.len(), 2);
        assert!((evs[0].onset_seconds - 0.0).abs() < 1e-9);
        assert!((evs[1].onset_seconds - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(evs[0].controls.get("s"), Some(&Value::Str("bd".into())));
    }

    #[test]
    fn note_value_becomes_control() {
        let evs = query_controls(&note(pure(Value::Int(60))), 2.0, 0.0, 1.0);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].controls.get("note"), Some(&Value::Int(60)));
        // duration in seconds = 1 cycle / cps
        assert!((evs[0].duration_seconds - 0.5).abs() < 1e-9);
    }

    #[test]
    fn windows_do_not_duplicate() {
        let pat = sequence(&[pure(Value::Int(0)), pure(Value::Int(1))]);
        let a = query_controls(&pat, 1.0, 0.0, 0.5);
        let b = query_controls(&pat, 1.0, 0.5, 1.0);
        assert_eq!(a.len() + b.len(), 2);
    }
}
