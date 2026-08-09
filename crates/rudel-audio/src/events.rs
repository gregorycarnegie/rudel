// events.rs - turning a pattern + clock into timed note events.
// This is the pure, testable core of the scheduler (no audio device needed).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{clock::Clock, samples::SampleBank, soundfont};
use rudel_core::{Pattern, Value, ValueMap, query_controls};
use rudel_dsp::{
    BusParams, ByteBeatParams, DrumKind, DrumParams, Duck, ModContext, ModSpecs, OrbitSend, PostFx,
    Sample, SamplerParams, VoiceParams, VoiceSpec, ZzfxParams,
};
use std::sync::Arc;

// Re-exported for back-compat; the canonical version lives in rudel-core.
pub use rudel_core::to_control_map;

/// A note to be played at `onset_seconds` (in the audio clock's timeline).
pub struct NoteEvent {
    /// The onset time in seconds on the audio timeline.
    pub onset_seconds: f64,
    /// The voice specification describing how to play this note.
    pub spec: VoiceSpec,
    /// Per-voice post-effects (crush/shape/distort/coarse/postgain).
    pub fx: PostFx,
    /// Which orbit bus this voice feeds, how much it sends to that orbit's
    /// reverb/delay, and the settings the orbit itself should take on.
    pub send: OrbitSend,
    /// Orbits this voice sidechain-ducks when it starts (`duckorbit`). Empty
    /// for the common case of a voice that ducks nothing.
    pub duck: Vec<Duck>,
    /// `lfo`/`env` modulators bound to this voice's parameters, resolved but
    /// not yet instantiated (that needs the device sample rate).
    pub mods: ModSpecs,
    /// `cut` group: when a new voice in the same group starts, any still-playing
    /// voice in that group is choked (fast fade). `None` means no group.
    pub cut: Option<i32>,
    /// Widget tags from the source hap; the mixer adds the voice's output to
    /// the per-widget scope taps registered under these ids.
    pub tags: Vec<String>,
}

/// The requested MIDI note for a sampler, from `freq` or `note` (name or
/// number). `None` when neither is set. Mirrors superdough's `valueToMidi`.
fn requested_midi(map: &ValueMap) -> Option<f64> {
    if let Some(freq) = map.get("freq").and_then(|v| v.as_f64()) {
        // freqToMidi: 12*log2(freq/440) + 69
        return Some(12.0 * (freq / 440.0).log2() + 69.0);
    }
    match map.get("note") {
        Some(Value::Str(s)) => rudel_core::note_to_midi(s).map(|m| m as f64),
        other => other.and_then(|v| v.as_f64()),
    }
}

/// Resolve a control map into either a sampler or synth voice spec.
fn spec_for(map: &ValueMap, duration: f32, bank: &SampleBank, cps: f64, cycle: f64) -> VoiceSpec {
    if let Some(name) = map.get("s").and_then(|v| v.as_str()) {
        // The `bank` control prepends `<bank>_` to the sound name, matching
        // Strudel: `s("bd").bank("RolandTR909")` resolves `RolandTR909_bd`. We
        // prefer the banked sample, then fall back to the bare name so the
        // built-in drum synth still works when no banked pack is loaded.
        let banked = map
            .get("bank")
            .and_then(|v| v.as_str())
            .map(|b| format!("{}_{name}", bank.canonical_bank(b)));

        // Loaded samples win over the built-in drum synth, which wins over the
        // plain oscillator synth.
        // `n` is rounded to the nearest integer (superdough's `getSoundIndex`
        // does `Math.round(n)`); the euclidean wrap into range happens in
        // `bank.resolve`, so a fractional or negative `n` matches upstream.
        let index = map
            .get("n")
            .and_then(|v| v.as_f64())
            .map(|n| n.round() as i64)
            .unwrap_or(0);
        let midi = requested_midi(map);
        for candidate in banked.as_deref().into_iter().chain(std::iter::once(name)) {
            if let Some((sample, transpose)) = bank.resolve(candidate, index, midi) {
                let mut params = SamplerParams::new(sample);
                params.apply_controls(map);
                // Repitch the sample onto the requested note (note-keyed maps) or
                // relative to C3 (flat maps with `note`): rate *= 2^(semis/12).
                if transpose != 0.0 {
                    params.speed *= 2f32.powf(transpose as f32 / 12.0);
                }
                // How long the sample sounds, following `sampler.mjs`:
                //
                //   if (clip == null && loop == null && value.release == null)
                //     duration = sliceDuration;
                //
                // — a sample plays its whole slice *unless* the pattern asked
                // for it to be cut, and `clip`, `loop` or `release` all ask.
                // Playing the natural length regardless is fine for a drum hit
                // but not for a sustained instrument: a VCSL ocarina runs for
                // seconds, so every note of `s('ocarina_vib').clip(1)` rang far
                // past its hap and the layers piled up into noise.
                if params.loop_on
                    || ["clip", "loop", "release"]
                        .iter()
                        .any(|k| map.contains_key(*k))
                {
                    params.duration = duration;
                }
                return VoiceSpec::Sampler(params);
            }
        }
        // Soundfont sounds (`gm_*`). A preset picks its recording by MIDI key
        // range and detunes in cents, so it resolves separately from the
        // nearest-tuning sample groups above. A loaded pack of the same name
        // still wins, matching the loaded-samples-first order.
        if let Some(voice) = bank.resolve_font(name, index, midi.unwrap_or(60.0)) {
            let mut params = SamplerParams::new(voice.sample);
            params.apply_controls(map);
            params.speed *= voice.rate;
            if let Some((begin, end)) = voice.loop_region {
                // A looping zone sustains for the hap rather than playing to
                // its natural end.
                params.loop_on = true;
                params.loop_begin = begin;
                params.loop_end = end;
                params.duration = duration;
            }
            return VoiceSpec::Sampler(params);
        }
        if soundfont::gm_variants(name) > 0 {
            // Known but not loaded: ask for it and stay silent this time, the
            // same first-note miss upstream's async loader has.
            soundfont::request_font(name, index);
            return VoiceSpec::Sampler(SamplerParams::new(Arc::new(Sample {
                data: Vec::new(),
                sample_rate: 44100.0,
            })));
        }
        if let Some(kind) = DrumKind::from_name(name) {
            let mut params = DrumParams::new(kind);
            params.apply_controls(map);
            params.duration = duration;
            return VoiceSpec::Drum(params);
        }
        // ZzFX synths: `zzfx` and the `z_<wave>` family (superdough's
        // registerZZFXSounds). Resolved here so a loaded sample of the same name
        // still wins above.
        if name == "zzfx" || name.starts_with("z_") {
            return VoiceSpec::Zzfx(Box::new(ZzfxParams::from_controls(name, map, duration)));
        }
        // The `bytebeat` synth: an integer expression sampled per audio frame.
        if name == "bytebeat" {
            return VoiceSpec::ByteBeat(Box::new(ByteBeatParams::from_controls(map, duration)));
        }
        // `s("bus")` plays back whatever another pattern sent to `.bus(n)`.
        if name == "bus" {
            return VoiceSpec::Bus(BusParams::from_controls(map, duration));
        }
    }
    let mut params = VoiceParams::from_controls_at(map, duration, cps, cycle);
    // A wavetable collection loaded by `tables(...)` turns `s("name")` into the
    // wavetable oscillator; it is checked after loaded samples (which win, like
    // upstream's registerSound order) and before the built-in synths.
    if let Some(name) = map.get("s").and_then(|v| v.as_str()) {
        let index = map
            .get("n")
            .and_then(|v| v.as_f64())
            .map(|n| n.round() as i64)
            .unwrap_or(0);
        params.wavetable = bank.resolve_table(name, index);
    }
    VoiceSpec::Synth(Box::new(params))
}

/// Query `pattern` over the cycle window `[begin_cycle, end_cycle)` and return
/// note events for every onset, with times converted to seconds via `cps`
/// (cycles per second). Sample-backed sounds are resolved against `bank`.
///
/// Convenience wrapper over [`collect_events_at`] for a clock anchored at the
/// origin (the common constant-cps case, `onset_seconds = onset_cycle / cps`).
pub fn collect_events(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
    bank: &SampleBank,
) -> Vec<NoteEvent> {
    collect_events_at(pattern, &Clock::new(cps), begin_cycle, end_cycle, bank).0
}

/// Like [`collect_events`], but maps each onset cycle to a trigger time through
/// `clock`, so an event's seconds honor the clock's current cps anchor. The
/// scheduler uses this so onsets stay correct after a live cps change.
///
/// Also reports the tempo an event in this window asks the transport to take
/// on — cyclist reads `hap.value.cps` off the query stream the same way, so
/// `.cps("<0.5 1>")` retunes the transport mid-pattern. The last onset carrying
/// a usable `cps` wins (upstream applies each in turn, so the last one stands),
/// and it is the *scheduler's* to act on, not the mixer's: the events here were
/// already timed against the current clock.
pub fn collect_events_at(
    pattern: &Pattern,
    clock: &Clock,
    begin_cycle: f64,
    end_cycle: f64,
    bank: &SampleBank,
) -> (Vec<NoteEvent>, Option<f64>) {
    let control_events = query_controls(pattern, clock.cps(), begin_cycle, end_cycle);
    let cps_change = control_events
        .iter()
        .filter_map(|ev| ev.controls.get("cps").and_then(|v| v.as_f64()))
        .rfind(|cps| cps.is_finite() && *cps > 0.0);
    let events = control_events
        .into_iter()
        .map(|ev| {
            let spec = spec_for(
                &ev.controls,
                ev.duration_seconds as f32,
                bank,
                clock.cps(),
                ev.onset_cycle,
            );
            let fx = PostFx::from_controls(&ev.controls);
            // A modulator's relative `depth` scales the target control's own
            // value, so the sources are resolved against the built voice.
            let ctx = ModContext {
                cps: clock.cps(),
                cycle: ev.onset_cycle,
                note_seconds: ev.duration_seconds,
            };
            let mods = ModSpecs::from_controls(&ev.controls, &ctx, |t| spec.mod_base(t, &fx));
            let mut send = OrbitSend::from_controls(&ev.controls, clock.cps());
            // `ir`/`iresponse` names a loaded sample to use as the reverb's
            // impulse response. `OrbitSend` cannot resolve it (rudel-dsp has no
            // bank), so it is filled in here.
            send.reverb.ir = ev
                .controls
                .get("ir")
                .or_else(|| ev.controls.get("iresponse"))
                .and_then(|v| v.as_str())
                .and_then(|name| bank.resolve(name, 0, None))
                .map(|(sample, _transpose)| sample);
            NoteEvent {
                onset_seconds: clock.seconds_at(ev.onset_cycle),
                spec,
                fx,
                mods,
                send,
                duck: Duck::from_controls(&ev.controls),
                cut: ev
                    .controls
                    .get("cut")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as i32),
                tags: ev.tags,
            }
        })
        .collect();
    (events, cps_change)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soundfont as rudel_audio_preset;
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
    fn a_cps_control_is_reported_as_a_tempo_change() {
        let bank = SampleBank::new();
        let clock = Clock::new(1.0);
        let collect = |pat: &Pattern, b, e| collect_events_at(pat, &clock, b, e, &bank).1;

        // No `cps` anywhere -> nothing for the scheduler to do.
        assert_eq!(collect(&seq3(), 0.0, 1.0), None);

        // A hap carrying `cps` reports it. `.cps("<0.5 2>")` alternates, so each
        // cycle reports its own value rather than the value at eval time.
        let pat = s(pure(Value::Str("bd".into()))).ctrl("cps", Value::F64(0.25));
        assert_eq!(collect(&pat, 0.0, 1.0), Some(0.25));

        // Last onset in the window wins, matching cyclist applying each in turn.
        let alternating = sequence(&[
            s(pure(Value::Str("bd".into()))).ctrl("cps", Value::F64(0.25)),
            s(pure(Value::Str("sd".into()))).ctrl("cps", Value::F64(2.0)),
        ]);
        assert_eq!(collect(&alternating, 0.0, 1.0), Some(2.0));

        // A rate that cannot drive a clock is ignored rather than stopping time.
        let zero = s(pure(Value::Str("bd".into()))).ctrl("cps", Value::F64(0.0));
        assert_eq!(collect(&zero, 0.0, 1.0), None);
    }

    #[test]
    fn a_loaded_wavetable_turns_a_sound_into_the_wavetable_oscillator() {
        use rudel_dsp::{WarpMode, WaveTable};

        let mut bank = SampleBank::new();
        bank.register_table(
            "wt_test",
            WaveTable::from_samples(&[0.0, 1.0, 0.0, -1.0], 4),
        );
        // `s("wt_test")` resolves to a synth voice carrying the table, and the
        // `warp`/`warpmode`/`wt` controls come through with it.
        let pat = s(pure(Value::Str("wt_test".into())))
            .warpmode(pure(Value::Str("wormhole".into())))
            .warp(pure(Value::F64(0.5)));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        assert_eq!(events.len(), 1);
        match &events[0].spec {
            VoiceSpec::Synth(params) => {
                let table = params.wavetable.as_ref().expect("wavetable attached");
                assert_eq!(table.frames.len(), 1);
                assert_eq!(params.warpmode, WarpMode::Wormhole);
                assert_eq!(params.warp.offset, 0.5);
            }
            _ => panic!("expected a synth voice"),
        }
        // An unloaded name is still the plain oscillator.
        let plain = collect_events(&s(pure(Value::Str("sine".into()))), 1.0, 0.0, 1.0, &bank);
        match &plain[0].spec {
            VoiceSpec::Synth(params) => assert!(params.wavetable.is_none()),
            _ => panic!("expected a synth voice"),
        }
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
    fn a_sample_is_cut_to_the_hap_only_when_asked() {
        // `sampler.mjs`: a sample plays its whole slice unless `clip`, `loop`
        // or `release` is set, in which case it holds for the hap and releases.
        // Always playing the natural length let a sustained instrument ring far
        // past its note — the layers of a tune then pile up into noise.
        let mut bank = SampleBank::new();
        bank.register(
            "ocarina",
            Arc::new(Sample {
                data: vec![0.5; 44100 * 4], // four seconds of sustain
                sample_rate: 44100.0,
            }),
        );
        let hold = |src: &str| {
            let pattern = rudel_lang::eval(src).expect("eval");
            match &collect_events(&pattern, 1.0, 0.0, 1.0, &bank)[0].spec {
                VoiceSpec::Sampler(p) => p.duration,
                _ => panic!("expected a sampler voice"),
            }
        };
        // Bare: 0 means "play to the sample's natural end".
        assert_eq!(hold(r#"s("ocarina")"#), 0.0);
        // Any of the three cuts it to the hap.
        for src in [
            r#"s("ocarina").clip(1)"#,
            r#"s("ocarina").release(0.1)"#,
            r#"s("ocarina").loop(1)"#,
        ] {
            assert!(hold(src) > 0.0, "should hold for the hap: {src}");
        }
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
    fn zzfx_names_resolve_to_zzfx_voice() {
        // `zzfx` and the `z_<wave>` family route to the ZzFX synth.
        let bank = SampleBank::new();
        for name in ["zzfx", "z_sine", "z_sawtooth", "z_square", "z_noise"] {
            let events = collect_events(&pure(Value::Str(name.into())), 1.0, 0.0, 1.0, &bank);
            assert!(
                matches!(events[0].spec, VoiceSpec::Zzfx(_)),
                "{name} should resolve to a ZzFX voice"
            );
        }
        // A non-z synth name still falls back to the oscillator synth.
        let events = collect_events(&pure(Value::Str("zara".into())), 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Synth(_)));
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
    fn bank_alias_resolves_to_the_canonical_pack() {
        // aliasBank("RolandTR909", "tr909") -> s("bd").bank("tr909") finds
        // the pack registered as "RolandTR909_bd".
        let mut bank = SampleBank::new();
        bank.register(
            "RolandTR909_bd",
            Arc::new(Sample {
                data: vec![0.5; 100],
                sample_rate: 44100.0,
            }),
        );
        bank.alias_bank("RolandTR909", "tr909");
        let pat = s(Value::Str("bd".into())).bank(Value::Str("tr909".into()));
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
    fn pitched_map_repitches_to_the_requested_note() {
        // A note-keyed "piano" tuned at c4 (MIDI 60), played at e4 (64), should
        // pick the c4 sample and set speed = 2^((64-60)/12).
        let mut bank = SampleBank::new();
        bank.register_note(
            "piano",
            60,
            Arc::new(Sample {
                data: vec![0.5; 100],
                sample_rate: 44100.0,
            }),
        );
        let pat = s(Value::Str("piano".into())).note(Value::Str("e4".into()));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Sampler(p) => {
                let expected = 2f32.powf(4.0 / 12.0);
                assert!(
                    (p.speed - expected).abs() < 1e-4,
                    "speed {} should be ~{expected}",
                    p.speed
                );
            }
            _ => panic!("expected a sampler voice"),
        }
    }

    #[test]
    fn n_is_rounded_to_the_nearest_sample_index() {
        // Three "x" samples with distinct first values; n=1.6 rounds to index 2
        // (superdough's getSoundIndex does Math.round(n)), not truncates to 1.
        let mut bank = SampleBank::new();
        for v in [0.1f32, 0.2, 0.3] {
            bank.register(
                "x",
                Arc::new(Sample {
                    data: vec![v; 8],
                    sample_rate: 44100.0,
                }),
            );
        }
        let pat = s(Value::Str("x".into())).n(Value::F64(1.6));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Sampler(p) => assert!(
                (p.sample.data[0] - 0.3).abs() < 1e-6,
                "n=1.6 should round to index 2 (value 0.3), got {}",
                p.sample.data[0]
            ),
            _ => panic!("expected a sampler voice"),
        }
    }

    /// A one-zone preset covering every key, tuned to MIDI 60.
    fn test_preset(loops: bool) -> rudel_audio_preset::Preset {
        use rudel_audio_preset::{Preset, Zone};
        Preset {
            zones: vec![Zone {
                sample: Arc::new(Sample {
                    data: vec![0.5; 1000],
                    sample_rate: 44100.0,
                }),
                key_low: 0,
                key_high: 127,
                original_pitch: 6000.0,
                coarse_tune: 0.0,
                fine_tune: 0.0,
                sample_rate: 44100.0,
                loop_start: if loops { 100.0 } else { 0.0 },
                loop_end: if loops { 900.0 } else { 0.0 },
            }],
        }
    }

    #[test]
    fn a_loaded_soundfont_plays_at_the_requested_pitch() {
        let mut bank = SampleBank::new();
        bank.register_font("gm_piano", 0, test_preset(false));
        let pat = s(Value::Str("gm_piano".into())).note(Value::Str("c5".into()));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Sampler(p) => {
                // The zone is tuned to MIDI 60; c5 is 72, an octave up.
                assert!((p.speed - 2.0).abs() < 1e-5, "speed {}", p.speed);
                assert!(!p.loop_on);
            }
            _ => panic!("expected a sampler voice"),
        }
    }

    #[test]
    fn a_looping_soundfont_zone_sustains_for_the_hap() {
        let mut bank = SampleBank::new();
        bank.register_font("gm_pad", 0, test_preset(true));
        let pat = s(Value::Str("gm_pad".into())).note(Value::Int(60));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Sampler(p) => {
                assert!(p.loop_on);
                assert!(p.loop_begin > 0.0 && p.loop_end > p.loop_begin);
                assert!(p.duration > 0.0, "a looping zone plays for the hap");
            }
            _ => panic!("expected a sampler voice"),
        }
    }

    #[test]
    fn an_unloaded_soundfont_is_requested_and_stays_silent() {
        // Upstream loads fonts asynchronously, so the first note of a fresh
        // instrument is missed; here the miss is recorded for the app to fetch.
        let _ = crate::soundfont::take_font_requests(); // drain other tests
        let bank = SampleBank::new();
        let pat = s(Value::Str("gm_piano".into())).note(Value::Int(60));
        let events = collect_events(&pat, 1.0, 0.0, 1.0, &bank);
        match &events[0].spec {
            VoiceSpec::Sampler(p) => assert!(p.sample.data.is_empty(), "silent placeholder"),
            _ => panic!("expected a silent sampler"),
        }
        let requests = crate::soundfont::take_font_requests();
        assert!(
            requests.contains(&("gm_piano".to_string(), 0)),
            "{requests:?}"
        );
    }

    #[test]
    fn an_unknown_sound_is_not_mistaken_for_a_soundfont() {
        let bank = SampleBank::new();
        let events = collect_events(&pure(Value::Str("sine".into())), 1.0, 0.0, 1.0, &bank);
        assert!(matches!(events[0].spec, VoiceSpec::Synth(_)));
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
