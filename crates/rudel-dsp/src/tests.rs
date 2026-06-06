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
    let mk = |fm| {
        Voice::new(
            VoiceParams {
                waveform: Waveform::Sine,
                freq: 220.0,
                duration: 1.0,
                fm,
                fmh: 2.0,
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
