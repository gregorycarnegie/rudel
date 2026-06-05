// events.rs - turning a pattern + clock into timed note events.
// This is the pure, testable core of the scheduler (no audio device needed).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::samples::SampleBank;
use rudel_core::{Frac, Pattern, State, TimeSpan, Value};
use rudel_dsp::{SamplerParams, VoiceParams, VoiceSpec};
use std::collections::BTreeMap;

/// A note to be played at `onset_seconds` (in the audio clock's timeline).
pub struct NoteEvent {
    pub onset_seconds: f64,
    pub spec: VoiceSpec,
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

/// Resolve a control map into either a sampler or synth voice spec.
fn spec_for(map: &BTreeMap<String, Value>, duration: f32, bank: &SampleBank) -> VoiceSpec {
    if let Some(name) = map.get("s").and_then(|v| v.as_str())
        && bank.contains(name)
    {
        let index = map.get("n").and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
        if let Some(sample) = bank.get(name, index) {
            let mut params = SamplerParams::new(sample);
            params.apply_controls(map);
            return VoiceSpec::Sampler(params);
        }
    }
    VoiceSpec::Synth(VoiceParams::from_controls(map, duration))
}

/// Query `pattern` over the cycle window `[begin_cycle, end_cycle)` and return
/// note events for every onset, with times converted to seconds via `cps`
/// (cycles per second). Sample-backed sounds are resolved against `bank`.
pub fn collect_events(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
    bank: &SampleBank,
) -> Vec<NoteEvent> {
    if cps <= 0.0 || end_cycle <= begin_cycle {
        return Vec::new();
    }
    let begin = Frac::from_f64(begin_cycle);
    let end = Frac::from_f64(end_cycle);
    // Expose cps to cps-dependent transforms (loopAt/fit/splice) via `_cps`,
    // mirroring Strudel's `state.controls._cps`.
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
        let onset_seconds = onset_cycle / cps;
        let duration = (hap.duration().to_f64() / cps) as f32;
        let map = to_control_map(&hap.value);
        out.push(NoteEvent {
            onset_seconds,
            spec: spec_for(&map, duration, bank),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Value, pure, sequence, silence};
    use rudel_dsp::{Sample, VoiceSpec};
    use std::sync::Arc;

    fn seq3() -> Pattern {
        sequence(&[
            pure(Value::Str("bd".into())),
            silence(),
            pure(Value::Str("sd".into())),
        ])
    }

    #[test]
    fn events_have_correct_onsets() {
        let bank = SampleBank::new();
        let events = collect_events(&seq3(), 1.0, 0.0, 1.0, &bank);
        assert_eq!(events.len(), 2);
        assert!((events[0].onset_seconds - 0.0).abs() < 1e-9);
        assert!((events[1].onset_seconds - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn cps_scales_time() {
        let bank = SampleBank::new();
        let events = collect_events(&seq3(), 2.0, 0.0, 1.0, &bank);
        assert!((events[1].onset_seconds - (2.0 / 3.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn windows_do_not_duplicate_or_drop() {
        let bank = SampleBank::new();
        let a = collect_events(&seq3(), 1.0, 0.0, 0.5, &bank);
        let b = collect_events(&seq3(), 1.0, 0.5, 1.0, &bank);
        assert_eq!(a.len() + b.len(), 2);
    }

    #[test]
    fn known_sample_resolves_to_sampler() {
        let mut bank = SampleBank::new();
        bank.register(
            "bd",
            Arc::new(Sample {
                data: vec![0.5; 100],
                sample_rate: 44100.0,
            }),
        );
        let events = collect_events(&pure(Value::Str("bd".into())), 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Sampler(_)));
    }

    #[test]
    fn unknown_sound_falls_back_to_synth() {
        let bank = SampleBank::new();
        let events = collect_events(&pure(Value::Str("sine".into())), 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Synth(_)));
    }
}
