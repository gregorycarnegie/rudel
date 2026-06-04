// events.rs - turning a pattern + clock into timed note events.
// This is the pure, testable core of the scheduler (no audio device needed).
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Frac, Pattern, Value};
use rudel_dsp::VoiceParams;
use std::collections::BTreeMap;

/// A note to be played at `onset_seconds` (in the audio clock's timeline).
#[derive(Clone, Debug)]
pub struct NoteEvent {
    pub onset_seconds: f64,
    pub params: VoiceParams,
}

/// Normalize a hap value into a control map: maps pass through; bare strings
/// become `{s}`; bare numbers become `{note}`.
pub fn to_control_map(value: &Value) -> BTreeMap<String, Value> {
    match value {
        Value::Map(m) => m.clone(),
        Value::Str(_) => BTreeMap::from([("s".to_string(), value.clone())]),
        Value::List(items) if !items.is_empty() => {
            // e.g. a raw "bd:3" list -> {s, n}
            let mut m = BTreeMap::from([("s".to_string(), items[0].clone())]);
            if let Some(n) = items.get(1) {
                m.insert("n".to_string(), n.clone());
            }
            m
        }
        other => BTreeMap::from([("note".to_string(), other.clone())]),
    }
}

/// Query `pattern` over the cycle window `[begin_cycle, end_cycle)` and return
/// note events for every onset, with times converted to seconds via `cps`
/// (cycles per second).
pub fn collect_events(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
) -> Vec<NoteEvent> {
    if cps <= 0.0 || end_cycle <= begin_cycle {
        return Vec::new();
    }
    let begin = Frac::from_f64(begin_cycle);
    let end = Frac::from_f64(end_cycle);
    let mut out = Vec::new();
    for hap in pattern.query_arc(begin, end) {
        let Some(whole) = hap.whole else {
            continue; // skip continuous haps
        };
        if !hap.has_onset() {
            continue;
        }
        let onset_cycle = whole.begin.to_f64();
        // only events whose onset falls inside this window (avoids duplicates
        // across adjacent windows)
        if onset_cycle < begin_cycle || onset_cycle >= end_cycle {
            continue;
        }
        let onset_seconds = onset_cycle / cps;
        let duration_seconds = (hap.duration().to_f64() / cps) as f32;
        let map = to_control_map(&hap.value);
        let params = VoiceParams::from_controls(&map, duration_seconds);
        out.push(NoteEvent {
            onset_seconds,
            params,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Value, pure, sequence, silence};

    fn seq3() -> Pattern {
        // "bd ~ sd" via core builders (no mini dependency in unit tests)
        sequence(&[
            pure(Value::Str("bd".into())),
            silence(),
            pure(Value::Str("sd".into())),
        ])
    }

    #[test]
    fn events_have_correct_onsets() {
        // at cps = 1, cycle 0..1: bd at 0s, sd at 2/3s
        let events = collect_events(&seq3(), 1.0, 0.0, 1.0);
        assert_eq!(events.len(), 2);
        assert!((events[0].onset_seconds - 0.0).abs() < 1e-9);
        assert!((events[1].onset_seconds - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn cps_scales_time() {
        // at cps = 2, everything happens twice as fast
        let events = collect_events(&seq3(), 2.0, 0.0, 1.0);
        assert!((events[1].onset_seconds - (2.0 / 3.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn windows_do_not_duplicate_or_drop() {
        // two adjacent windows covering one cycle should yield each onset once
        let a = collect_events(&seq3(), 1.0, 0.0, 0.5);
        let b = collect_events(&seq3(), 1.0, 0.5, 1.0);
        assert_eq!(a.len() + b.len(), 2);
    }

    #[test]
    fn bare_number_becomes_note() {
        let map = to_control_map(&Value::Int(60));
        assert_eq!(map.get("note"), Some(&Value::Int(60)));
    }
}
