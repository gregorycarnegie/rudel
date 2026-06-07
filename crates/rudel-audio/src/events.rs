// events.rs - turning a pattern + clock into timed note events.
// This is the pure, testable core of the scheduler (no audio device needed).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::samples::SampleBank;
use rudel_core::{Pattern, Value, query_controls};
use rudel_dsp::{DrumKind, DrumParams, PostFx, SamplerParams, VoiceParams, VoiceSpec};
use std::collections::BTreeMap;

// Re-exported for back-compat; the canonical version lives in rudel-core.
pub use rudel_core::to_control_map;

/// A note to be played at `onset_seconds` (in the audio clock's timeline).
pub struct NoteEvent {
    pub onset_seconds: f64,
    pub spec: VoiceSpec,
    /// Per-voice post-effects (crush/shape/distort/coarse/postgain).
    pub fx: PostFx,
    /// `cut` group: when a new voice in the same group starts, any still-playing
    /// voice in that group is choked (fast fade). `None` means no group.
    pub cut: Option<i32>,
}

/// Resolve a control map into either a sampler or synth voice spec.
fn spec_for(map: &BTreeMap<String, Value>, duration: f32, bank: &SampleBank) -> VoiceSpec {
    if let Some(name) = map.get("s").and_then(|v| v.as_str()) {
        // The `bank` control prepends `<bank>_` to the sound name, matching
        // Strudel: `s("bd").bank("RolandTR909")` resolves `RolandTR909_bd`. We
        // prefer the banked sample, then fall back to the bare name so the
        // built-in drum synth still works when no banked pack is loaded.
        let banked = map
            .get("bank")
            .and_then(|v| v.as_str())
            .map(|b| format!("{b}_{name}"));

        // Loaded samples win over the built-in drum synth, which wins over the
        // plain oscillator synth.
        let index = map.get("n").and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
        for candidate in banked.as_deref().into_iter().chain(std::iter::once(name)) {
            if let Some(sample) = bank.get(candidate, index) {
                let mut params = SamplerParams::new(sample);
                params.apply_controls(map);
                return VoiceSpec::Sampler(params);
            }
        }
        if let Some(kind) = DrumKind::from_name(name) {
            let mut params = DrumParams::new(kind);
            params.apply_controls(map);
            return VoiceSpec::Drum(params);
        }
    }
    VoiceSpec::Synth(Box::new(VoiceParams::from_controls(map, duration)))
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
    query_controls(pattern, cps, begin_cycle, end_cycle)
        .into_iter()
        .map(|ev| NoteEvent {
            onset_seconds: ev.onset_seconds,
            spec: spec_for(&ev.controls, ev.duration_seconds as f32, bank),
            fx: PostFx::from_controls(&ev.controls),
            cut: ev
                .controls
                .get("cut")
                .and_then(|v| v.as_f64())
                .map(|v| v as i32),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Value, pure, s, sequence, silence};
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

    #[test]
    fn drum_name_resolves_to_drum_synth() {
        // With no sample loaded, "bd" uses the built-in drum synth, not a note.
        let bank = SampleBank::new();
        let events = collect_events(&pure(Value::Str("bd".into())), 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Drum(_)));
    }

    #[test]
    fn noise_name_resolves_to_synth_noise() {
        let bank = SampleBank::new();
        let events = collect_events(&pure(Value::Str("white".into())), 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Synth(p) => assert!(p.noise.is_some(), "expected a noise source"),
            _ => panic!("expected a synth noise voice"),
        }
    }

    #[test]
    fn bank_prefixes_the_sample_name() {
        // s("bd").bank("RolandTR909") resolves the banked sample "RolandTR909_bd".
        let mut bank = SampleBank::new();
        bank.register(
            "RolandTR909_bd",
            Arc::new(Sample {
                data: vec![0.5; 100],
                sample_rate: 44100.0,
            }),
        );
        let pat = s(Value::Str("bd".into())).bank(Value::Str("RolandTR909".into()));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Sampler(_)));
    }

    #[test]
    fn bank_falls_back_to_drum_synth_when_pack_missing() {
        // With no banked pack loaded, "bd" still hits the built-in drum synth.
        let bank = SampleBank::new();
        let pat = s(Value::Str("bd".into())).bank(Value::Str("Nonexistent".into()));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Drum(_)));
    }

    #[test]
    fn loaded_sample_overrides_drum_synth() {
        // A loaded "bd" sample takes priority over the built-in drum.
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
}
