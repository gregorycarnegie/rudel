use super::*;
use rudel_core::Value;
use std::collections::BTreeMap;
use std::f32::consts::PI;
use std::sync::Arc;

#[test]
fn note_names() {
    assert_eq!(note_name_to_midi("a4"), Some(69));
    assert_eq!(note_name_to_midi("c4"), Some(60));
    assert_eq!(note_name_to_midi("c3"), Some(48));
    assert_eq!(note_name_to_midi("c#4"), Some(61));
    assert_eq!(note_name_to_midi("eb3"), Some(51));
    assert_eq!(note_name_to_midi("c"), Some(48)); // default octave 3
}

#[test]
fn mtof_a4() {
    assert!((mtof(69.0) - 440.0).abs() < 0.001);
}

#[test]
fn voice_produces_sound_then_finishes() {
    let p = VoiceParams {
        duration: 0.01,
        ..Default::default()
    };
    let mut v = Voice::new(p, 44100.0);
    let mut peak = 0.0f32;
    for _ in 0..44100 {
        let (l, _r) = v.tick();
        peak = peak.max(l.abs());
        if v.is_done() {
            break;
        }
    }
    assert!(peak > 0.0, "voice should produce non-silent output");
    assert!(v.is_done(), "voice should finish after its envelope");
}

#[test]
fn dry_control_parses_and_defaults_full() {
    // Default dry is full (1.0); `dry` parses into the param + voice accessor.
    assert_eq!(VoiceParams::default().dry, 1.0);
    let map = BTreeMap::from([("dry".to_string(), Value::F64(0.25))]);
    let p = VoiceParams::from_controls(&map, 0.1);
    assert_eq!(p.dry, 0.25);
    let v = Voice::new(p, 44100.0);
    assert_eq!(VoiceLike::dry(&v), 0.25);
    // A voice with no `dry` control reports full dry by default.
    let v = Voice::new(VoiceParams::default(), 44100.0);
    assert_eq!(VoiceLike::dry(&v), 1.0);
}

#[test]
fn ftype_24db_cascades_the_filter() {
    use rudel_core::Value;
    // A 24dB low-pass attenuates high frequencies more than the 12dB default.
    // Drive each filter with a steady-ish high-frequency input and compare the
    // residual energy.
    fn residual(ftype: f64) -> f32 {
        let map = BTreeMap::from([
            ("cutoff".to_string(), Value::F64(200.0)),
            ("ftype".to_string(), Value::F64(ftype)),
        ]);
        let mut p = VoiceParams::from_controls(&map, 1.0);
        // make a bright source (square) so there's high-frequency content
        p.waveform = Waveform::Square;
        let mut v = Voice::new(p, 44100.0);
        // settle, then measure peak over a window
        for _ in 0..2000 {
            v.tick();
        }
        let mut peak = 0.0f32;
        for _ in 0..4000 {
            let (l, _) = v.tick();
            peak = peak.max(l.abs());
        }
        peak
    }
    let twelve = residual(0.0);
    let twentyfour = residual(2.0);
    // The steeper 24dB slope should pass less of the bright signal than 12dB.
    assert!(
        twentyfour < twelve,
        "24dB ({twentyfour}) should attenuate more than 12dB ({twelve})"
    );
    // ftype parses on params: 0/1 -> single, 2 -> cascade.
    let cascade_of = |f: f64| {
        VoiceParams::from_controls(
            &BTreeMap::from([
                ("cutoff".to_string(), Value::F64(500.0)),
                ("ftype".to_string(), Value::F64(f)),
            ]),
            1.0,
        )
        .lp
        .cascade
    };
    assert!(!cascade_of(0.0));
    assert!(!cascade_of(1.0));
    assert!(cascade_of(2.0));
}

#[test]
fn drum_names_resolve() {
    assert_eq!(DrumKind::from_name("bd"), Some(DrumKind::Bd));
    assert_eq!(DrumKind::from_name("hh"), Some(DrumKind::Hh));
    assert_eq!(DrumKind::from_name("oh"), Some(DrumKind::Oh));
    assert_eq!(DrumKind::from_name("rim"), Some(DrumKind::Rim));
    assert_eq!(DrumKind::from_name("sawtooth"), None);
}

#[test]
fn drum_produces_sound_then_finishes() {
    for kind in [
        DrumKind::Bd,
        DrumKind::Sd,
        DrumKind::Hh,
        DrumKind::Oh,
        DrumKind::Rim,
        DrumKind::Clap,
        DrumKind::Lt,
        DrumKind::Mt,
        DrumKind::Ht,
        DrumKind::Rd,
        DrumKind::Cr,
    ] {
        let mut v = DrumVoice::new(DrumParams::new(kind), 44100.0);
        let mut peak = 0.0f32;
        let mut ticks = 0;
        for _ in 0..(44100 * 2) {
            let (l, _r) = v.tick();
            peak = peak.max(l.abs());
            ticks += 1;
            if v.is_done() {
                break;
            }
        }
        assert!(peak > 0.0, "{kind:?} should produce sound");
        assert!(v.is_done(), "{kind:?} should finish");
        assert!(ticks < 44100 * 2, "{kind:?} should finish within 2s");
    }
}

/// A test voice emitting a fixed stereo value, never done.
struct ConstVoice(f32);
impl VoiceLike for ConstVoice {
    fn tick(&mut self) -> (f32, f32) {
        (self.0, self.0)
    }
    fn is_done(&self) -> bool {
        false
    }
    fn room(&self) -> f32 {
        0.0
    }
    fn delay_send(&self) -> f32 {
        0.0
    }
}

#[test]
fn noise_names_and_sound() {
    assert_eq!(NoiseKind::from_name("white"), Some(NoiseKind::White));
    assert_eq!(NoiseKind::from_name("pink"), Some(NoiseKind::Pink));
    assert_eq!(NoiseKind::from_name("brown"), Some(NoiseKind::Brown));
    assert_eq!(NoiseKind::from_name("sine"), None);
    for kind in [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown] {
        let p = VoiceParams {
            noise: Some(kind),
            duration: 0.1,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        let mut peak = 0.0f32;
        for _ in 0..2000 {
            peak = peak.max(v.tick().0.abs());
        }
        assert!(peak > 0.0, "{kind:?} noise should produce sound");
    }
}

#[test]
fn supersaw_produces_sound() {
    let p = VoiceParams {
        supersaw: true,
        unison: 5,
        spread: 0.4,
        freq: 220.0,
        duration: 0.2,
        ..Default::default()
    };
    let mut v = Voice::new(p, 44100.0);
    let mut peak = 0.0f32;
    for _ in 0..4000 {
        peak = peak.max(v.tick().0.abs());
    }
    assert!(peak > 0.0, "supersaw should produce sound");
}

#[test]
fn fm_changes_the_signal() {
    let mk = |fm: Option<f32>| {
        Voice::new(
            VoiceParams {
                waveform: Waveform::Sine,
                freq: 220.0,
                duration: 1.0,
                fm: fm.map_or(FmSpec::default(), |i| {
                    FmSpec::single(i, 2.0, Waveform::Sine, None)
                }),
                ..Default::default()
            },
            44100.0,
        )
    };
    let (mut plain, mut modulated) = (mk(None), mk(Some(4.0)));
    let mut diff = 0.0f32;
    for _ in 0..2000 {
        diff += (plain.tick().0 - modulated.tick().0).abs();
    }
    assert!(diff > 0.0, "FM should change the carrier signal");
}

#[test]
fn fmwave_changes_the_modulator() {
    let mk = |w| {
        Voice::new(
            VoiceParams {
                waveform: Waveform::Sine,
                freq: 220.0,
                duration: 1.0,
                fm: FmSpec::single(6.0, 2.0, w, None),
                ..Default::default()
            },
            44100.0,
        )
    };
    let (mut sine_mod, mut square_mod) = (mk(Waveform::Sine), mk(Waveform::Square));
    let mut diff = 0.0f32;
    for _ in 0..2000 {
        diff += (sine_mod.tick().0 - square_mod.tick().0).abs();
    }
    assert!(
        diff > 0.0,
        "the FM modulator waveform should change the signal"
    );
}

#[test]
fn fm_envelope_ramps_in_the_modulation() {
    let base = || VoiceParams {
        waveform: Waveform::Sine,
        freq: 220.0,
        duration: 1.0,
        ..Default::default()
    };
    let mut plain = Voice::new(base(), 44100.0);
    let mut const_fm = Voice::new(
        VoiceParams {
            fm: FmSpec::single(8.0, 2.0, Waveform::Sine, None),
            ..base()
        },
        44100.0,
    );
    let mut env_fm = Voice::new(
        VoiceParams {
            // long attack: the index ramps in slowly from ~0
            fm: FmSpec::single(
                8.0,
                2.0,
                Waveform::Sine,
                Some(Adsr {
                    attack: 0.5,
                    decay: 0.001,
                    sustain: 1.0,
                    release: 0.01,
                }),
            ),
            ..base()
        },
        44100.0,
    );
    // Early in the 0.5s attack the enveloped index is near 0, so the enveloped
    // voice tracks the un-modulated carrier far more closely than constant FM.
    let (mut d_env, mut d_const) = (0.0f32, 0.0f32);
    for _ in 0..400 {
        let p = plain.tick().0;
        d_env += (p - env_fm.tick().0).abs();
        d_const += (p - const_fm.tick().0).abs();
    }
    assert!(
        d_env < d_const,
        "early FM-env modulation ({d_env}) should be weaker than constant FM ({d_const})"
    );
}

#[test]
fn two_operator_fm_chain_changes_the_signal() {
    // op2 -> op1 -> carrier (fmi2 + fmi). The second operator modulating the
    // first should change the timbre vs. single-operator FM alone.
    let map = |extra: &[(&str, f64)]| {
        let mut m = BTreeMap::new();
        m.insert("s".to_string(), Value::Str("sine".into()));
        m.insert("note".to_string(), Value::Str("c3".into()));
        m.insert("fm".to_string(), Value::F64(4.0)); // op1 -> carrier
        m.insert("fmh".to_string(), Value::F64(2.0));
        for (k, v) in extra {
            m.insert(k.to_string(), Value::F64(*v));
        }
        m
    };
    // single-op vs. op2 added on top
    let one = VoiceParams::from_controls(&map(&[]), 1.0);
    let two = VoiceParams::from_controls(&map(&[("fmi2", 5.0), ("fmh2", 3.0)]), 1.0);
    assert_eq!(one.fm.max_op, 1);
    assert_eq!(two.fm.max_op, 2);
    assert_eq!(two.fm.amt[2][1], 5.0);
    assert_eq!(two.fm.ops[2].ratio, 3.0);

    let mut v1 = Voice::new(one, 44100.0);
    let mut v2 = Voice::new(two, 44100.0);
    let mut diff = 0.0f32;
    for _ in 0..4000 {
        diff += (v1.tick().0 - v2.tick().0).abs();
    }
    assert!(diff > 0.0, "a second FM operator should change the timbre");
}

#[test]
fn additive_partials_build_a_custom_waveform() {
    let map = |partials: Vec<Value>| {
        let mut m = BTreeMap::new();
        m.insert("s".to_string(), Value::Str("sawtooth".into()));
        m.insert("note".to_string(), Value::Str("c3".into()));
        m.insert("partials".to_string(), Value::List(partials));
        m
    };
    // A single partial is just the fundamental sine; many partials add harmonics.
    let one = VoiceParams::from_controls(&map(vec![Value::F64(1.0)]), 1.0);
    let many = VoiceParams::from_controls(
        &map(vec![
            Value::F64(1.0),
            Value::F64(1.0),
            Value::F64(1.0),
            Value::F64(1.0),
        ]),
        1.0,
    );
    assert!(one.additive.is_some(), "partials should build a wavetable");
    let table = one.additive.as_ref().unwrap();
    assert!(
        table.iter().all(|x| x.abs() <= 1.0001),
        "table is normalized"
    );
    assert!(
        table.iter().any(|x| x.abs() > 0.5),
        "table should be non-silent and normalized to peak 1"
    );

    let mut v1 = Voice::new(one, 44100.0);
    let mut v4 = Voice::new(many, 44100.0);
    let mut diff = 0.0f32;
    for _ in 0..4000 {
        diff += (v1.tick().0 - v4.tick().0).abs();
    }
    assert!(diff > 0.0, "more partials should change the timbre");
}

#[test]
fn partials_count_expands_to_equal_harmonics() {
    let mut m = BTreeMap::new();
    m.insert("s".to_string(), Value::Str("user".into()));
    m.insert("partials".to_string(), Value::Int(6));
    let p = VoiceParams::from_controls(&m, 1.0);
    assert!(
        p.additive.is_some(),
        "a partials count should build a user wavetable"
    );
}

#[test]
fn pulse_width_sets_the_duty_cycle() {
    // pw fraction of the cycle is high (+1), the rest low (-1).
    assert_eq!(Waveform::pulse(0.1, 0.25), 1.0);
    assert_eq!(Waveform::pulse(0.3, 0.25), -1.0);
    assert_eq!(Waveform::pulse(0.6, 0.75), 1.0);
    // pw 0.5 matches the square wave.
    for &p in &[0.1, 0.4, 0.6, 0.9] {
        assert_eq!(Waveform::pulse(p, 0.5), Waveform::Square.sample(p));
    }
}

#[test]
fn pulse_resolves_from_s_and_pw_changes_output() {
    let map = |pw: f32| {
        let mut m = BTreeMap::new();
        m.insert("s".to_string(), Value::Str("pulse".into()));
        m.insert("pw".to_string(), Value::F64(pw as f64));
        m
    };
    let p = VoiceParams::from_controls(&map(0.2), 1.0);
    assert_eq!(p.waveform, Waveform::Pulse);
    assert!((p.pw - 0.2).abs() < 1e-6);

    let mut narrow = Voice::new(VoiceParams::from_controls(&map(0.1), 1.0), 44100.0);
    let mut wide = Voice::new(VoiceParams::from_controls(&map(0.9), 1.0), 44100.0);
    let mut diff = 0.0f32;
    for _ in 0..2000 {
        diff += (narrow.tick().0 - wide.tick().0).abs();
    }
    assert!(diff > 0.0, "different pulse widths should sound different");
}

#[test]
fn noise_mix_blends_in_noise() {
    let base = || VoiceParams {
        waveform: Waveform::Sine,
        freq: 220.0,
        duration: 1.0,
        ..Default::default()
    };
    let mut clean = Voice::new(base(), 44100.0);
    let mut noisy = Voice::new(
        VoiceParams {
            noise_mix: 0.5,
            ..base()
        },
        44100.0,
    );
    let mut diff = 0.0f32;
    for _ in 0..2000 {
        diff += (clean.tick().0 - noisy.tick().0).abs();
    }
    assert!(
        diff > 0.0,
        "a noise mix should change the oscillator output"
    );
}

#[test]
fn pcurve_changes_the_pitch_envelope_shape() {
    let base = || VoiceParams {
        waveform: Waveform::Sine,
        freq: 220.0,
        duration: 1.0,
        penv: Some(12.0),
        pattack: Some(0.3),
        ..Default::default()
    };
    let mut lin = Voice::new(base(), 44100.0);
    let mut expo = Voice::new(
        VoiceParams {
            pcurve_exp: true,
            ..base()
        },
        44100.0,
    );
    // During the attack the exponential ramp differs from the linear one.
    let mut diff = 0.0f32;
    for _ in 0..4000 {
        diff += (lin.tick().0 - expo.tick().0).abs();
    }
    assert!(diff > 0.0, "pcurve should change the pitch-envelope shape");
}

#[test]
fn vibrato_and_pitch_env_change_pitch() {
    let base = || VoiceParams {
        waveform: Waveform::Sine,
        freq: 220.0,
        duration: 1.0,
        ..Default::default()
    };
    // vibrato vs none
    let mut plain = Voice::new(base(), 44100.0);
    let mut vibd = Voice::new(
        VoiceParams {
            vib: Some(6.0),
            vibmod: 1.0,
            ..base()
        },
        44100.0,
    );
    let mut diff = 0.0f32;
    for _ in 0..4000 {
        diff += (plain.tick().0 - vibd.tick().0).abs();
    }
    assert!(diff > 0.0, "vibrato should change the pitch over time");

    // pitch envelope vs none
    let mut penvd = Voice::new(
        VoiceParams {
            penv: Some(12.0),
            pattack: Some(0.2),
            ..base()
        },
        44100.0,
    );
    let mut plain2 = Voice::new(base(), 44100.0);
    let mut diff2 = 0.0f32;
    for _ in 0..4000 {
        diff2 += (plain2.tick().0 - penvd.tick().0).abs();
    }
    assert!(diff2 > 0.0, "pitch envelope should bend the pitch");
}

#[test]
fn adsr_shortcut_parses_list() {
    let map = BTreeMap::from([(
        "adsr".to_string(),
        Value::List(vec![
            Value::F64(0.1),
            Value::F64(0.2),
            Value::F64(0.3),
            Value::F64(0.4),
        ]),
    )]);
    let p = VoiceParams::from_controls(&map, 0.5);
    assert_eq!(p.adsr.attack, 0.1);
    assert_eq!(p.adsr.decay, 0.2);
    assert_eq!(p.adsr.sustain, 0.3);
    assert_eq!(p.adsr.release, 0.4);
}

#[test]
fn vowel_formant_shapes_noise() {
    assert_eq!(Vowel::from_name("a"), Some(Vowel::A));
    assert_eq!(Vowel::from_name("z"), None);
    // white noise through the "a" formant should still produce output.
    let p = VoiceParams {
        noise: Some(NoiseKind::White),
        duration: 1.0,
        ..Default::default()
    };
    let voice = Box::new(Voice::new(p, 44100.0));
    let fx = PostFx {
        vowel: Some(Vowel::A),
        ..Default::default()
    };
    assert!(fx.is_active());
    let mut v = PostFxVoice::new(voice, fx, 44100.0);
    let mut peak = 0.0f32;
    for _ in 0..4000 {
        peak = peak.max(v.tick().0.abs());
    }
    assert!(peak > 0.0, "vowel formant should pass some signal");
}

#[test]
fn postfx_active_flag() {
    assert!(!PostFx::default().is_active());
    assert!(
        PostFx {
            crush: Some(4.0),
            ..Default::default()
        }
        .is_active()
    );
}

#[test]
fn crush_quantizes_to_levels() {
    // crush=2 bits -> step = 2^(2-1) = 2, so values snap to multiples of 0.5
    let fx = PostFx {
        crush: Some(2.0),
        postgain: 1.0,
        shapevol: 1.0,
        distortvol: 1.0,
        ..Default::default()
    };
    let mut v = PostFxVoice::new(Box::new(ConstVoice(0.3)), fx, 44100.0);
    let (l, _) = v.tick();
    assert_eq!(l, 0.5); // round(0.3*2)/2 = round(0.6)/2 = 1/2
}

#[test]
fn coarse_holds_samples() {
    // coarse=3: a ramping source is held for 3-sample windows
    struct Ramp(f32);
    impl VoiceLike for Ramp {
        fn tick(&mut self) -> (f32, f32) {
            self.0 += 1.0;
            (self.0, self.0)
        }
        fn is_done(&self) -> bool {
            false
        }
        fn room(&self) -> f32 {
            0.0
        }
        fn delay_send(&self) -> f32 {
            0.0
        }
    }
    let fx = PostFx {
        coarse: Some(3.0),
        postgain: 1.0,
        shapevol: 1.0,
        distortvol: 1.0,
        ..Default::default()
    };
    let mut v = PostFxVoice::new(Box::new(Ramp(0.0)), fx, 44100.0);
    let out: Vec<f32> = (0..6).map(|_| v.tick().0).collect();
    // first sample of each window held across the window
    assert_eq!(out, vec![1.0, 1.0, 1.0, 4.0, 4.0, 4.0]);
}

#[test]
fn distort_boosts_small_signal() {
    let fx = PostFx {
        distort: Some(2.0),
        postgain: 1.0,
        shapevol: 1.0,
        distortvol: 1.0,
        ..Default::default()
    };
    let mut v = PostFxVoice::new(Box::new(ConstVoice(0.1)), fx, 44100.0);
    let (l, _) = v.tick();
    assert!(l > 0.1, "distortion should boost a small input, got {l}");
}

#[test]
fn highpass_attenuates_low_frequencies() {
    // A low tone through a high cutoff should be much quieter than open.
    let mk = |hcutoff| {
        Voice::new(
            VoiceParams {
                freq: 100.0,
                duration: 1.0,
                hp: FilterParams {
                    freq: hcutoff,
                    ..Default::default()
                },
                ..Default::default()
            },
            44100.0,
        )
    };
    let (mut open, mut filtered) = (mk(None), mk(Some(4000.0)));
    let (mut e_open, mut e_filt) = (0.0f32, 0.0f32);
    for _ in 0..8000 {
        e_open += open.tick().0.abs();
        e_filt += filtered.tick().0.abs();
    }
    assert!(e_filt < e_open * 0.5, "highpass should cut the low tone");
}

#[test]
fn filter_envelope_opens_cutoff() {
    // A 4kHz tone is killed by a static lp at 200Hz; with lpenv the cutoff
    // sweeps up during the attack and lets much more through.
    let mk = |env: Option<f32>, attack: Option<f32>| {
        Voice::new(
            VoiceParams {
                freq: 4000.0,
                duration: 1.0,
                lp: FilterParams {
                    freq: Some(200.0),
                    env,
                    attack,
                    ..Default::default()
                },
                ..Default::default()
            },
            44100.0,
        )
    };
    let mut stat = mk(None, None);
    let mut swept = mk(Some(6.0), Some(0.2)); // opens ~6 octaves over 0.2s
    let (mut e_stat, mut e_swept) = (0.0f32, 0.0f32);
    for _ in 0..4410 {
        e_stat += stat.tick().0.abs();
        e_swept += swept.tick().0.abs();
    }
    assert!(
        e_swept > e_stat * 2.0,
        "filter env should open the cutoff (swept {e_swept} vs static {e_stat})"
    );
}

#[test]
fn lowpass_attenuates_high_frequencies() {
    // A high tone through a low cutoff should be much quieter than open.
    let mut open = Voice::new(
        VoiceParams {
            freq: 6000.0,
            duration: 1.0,
            ..Default::default()
        },
        44100.0,
    );
    let mut filtered = Voice::new(
        VoiceParams {
            freq: 6000.0,
            duration: 1.0,
            lp: FilterParams {
                freq: Some(200.0),
                ..Default::default()
            },
            ..Default::default()
        },
        44100.0,
    );
    let (mut e_open, mut e_filt) = (0.0f32, 0.0f32);
    for _ in 0..8000 {
        e_open += open.tick().0.abs();
        e_filt += filtered.tick().0.abs();
    }
    assert!(
        e_filt < e_open * 0.5,
        "filtered energy {e_filt} should be well below open {e_open}"
    );
}

#[test]
fn sampler_plays_a_buffer_then_finishes() {
    // a 0.1s buffer of a 200 Hz sine
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (2.0 * PI * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut v = SamplerVoice::new(SamplerParams::new(sample), sr);
    let mut peak = 0.0f32;
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        peak = peak.max(v.tick().0.abs());
        frames += 1;
    }
    assert!(peak > 0.0, "sampler should produce output");
    assert!(v.is_done(), "sampler should finish at the buffer end");
    assert!(frames < 44100, "sampler should not run forever");
}

#[test]
fn sampler_speed_changes_duration() {
    let sr = 44100.0;
    let data = vec![0.5f32; 4410]; // 0.1s of DC
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut fast = SamplerParams::new(sample.clone());
    fast.speed = 2.0;
    let mut v = SamplerVoice::new(fast, sr);
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        v.tick();
        frames += 1;
    }
    // at 2x speed the 0.1s buffer should take ~0.05s (~2205 frames)
    assert!(frames < 3000, "2x speed should play back in ~half the time");
}

#[test]
fn loop_plays_past_the_buffers_natural_length() {
    // A 0.1s buffer asked to loop for 0.5s should still be audible well past
    // its own length, then stop near the hold time (not run forever).
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (2.0 * PI * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut p = SamplerParams::new(sample);
    p.loop_on = true;
    p.duration = 0.5; // hold far longer than the 0.1s buffer
    let mut v = SamplerVoice::new(p, sr);

    let mut peak_late = 0.0f32;
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        let s = v.tick().0.abs();
        if frames > (sr * 0.2) as usize {
            peak_late = peak_late.max(s); // sampled past the natural end
        }
        frames += 1;
    }
    assert!(
        peak_late > 0.0,
        "a looping sample should still sound past its natural length"
    );
    assert!(
        frames >= (sr * 0.4) as usize,
        "should play roughly the hold duration"
    );
    assert!(
        frames < (sr * 0.7) as usize,
        "should stop after the hold + release, not loop forever"
    );
}

#[test]
fn pan_hard_left_silences_right() {
    let p = VoiceParams {
        pan: 0.0,
        ..Default::default()
    };
    let mut v = Voice::new(p, 44100.0);
    // skip the very start so the envelope has opened
    for _ in 0..100 {
        v.tick();
    }
    let (l, r) = v.tick();
    assert!(l.abs() > 0.0);
    assert!(r.abs() < 1e-6);
}

#[test]
fn tremolo_modulates_amplitude() {
    // depth=1, 100 Hz: gain swings across [0, 1] over one LFO period.
    let fx = PostFx {
        tremolo: Some(100.0),
        tremolodepth: 1.0,
        ..Default::default()
    };
    let sr = 44100.0;
    let mut v = PostFxVoice::new(Box::new(ConstVoice(1.0)), fx, sr);
    let period = (sr / 100.0) as usize; // 441 samples
    let out: Vec<f32> = (0..period).map(|_| v.tick().0).collect();
    let min = out.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = out.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(min < 0.05, "tremolo should dip near zero, got min {min}");
    assert!(max > 0.95, "tremolo should peak near unity, got max {max}");
    assert!(
        out.iter().all(|&g| (-0.0001..=1.0001).contains(&g)),
        "tremolo gain stays within [0, 1]"
    );
}

#[test]
fn phaser_attenuates_tone_at_notch() {
    // A sine sitting at the phaser's notch center should lose energy versus
    // the same sine with no phaser.
    struct SineSource {
        phase: f32,
        inc: f32,
    }
    impl VoiceLike for SineSource {
        fn tick(&mut self) -> (f32, f32) {
            let s = (self.phase * std::f32::consts::TAU).sin();
            self.phase = (self.phase + self.inc).fract();
            (s, s)
        }
        fn is_done(&self) -> bool {
            false
        }
        fn room(&self) -> f32 {
            0.0
        }
        fn delay_send(&self) -> f32 {
            0.0
        }
    }
    let sr = 44100.0;
    // notch center = phasercenter + 282 = 1282 Hz; sit the tone there.
    let mk = || SineSource {
        phase: 0.0,
        inc: 1282.0 / sr,
    };
    let fx = PostFx {
        phaser: Some(1.0),
        phaserdepth: 0.95, // low Q -> wide notch
        phasercenter: 1000.0,
        phasersweep: 200.0, // narrow sweep so the notch stays near the tone
        ..Default::default()
    };
    let mut plain = mk();
    let mut phased = PostFxVoice::new(Box::new(mk()), fx, sr);
    let (mut e_plain, mut e_phased) = (0.0f32, 0.0f32);
    for _ in 0..4410 {
        e_plain += plain.tick().0.abs();
        e_phased += phased.tick().0.abs();
    }
    assert!(
        e_phased < e_plain * 0.7,
        "phaser notch should attenuate the tone (phased {e_phased} vs plain {e_plain})"
    );
}
