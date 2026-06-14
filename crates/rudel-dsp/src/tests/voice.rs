use super::common::*;

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
