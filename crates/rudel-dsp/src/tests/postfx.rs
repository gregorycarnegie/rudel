use super::common::*;
use proptest::prelude::*;

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
fn compressor_attenuates_loud_signal_but_not_quiet() {
    // A constant signal well above the threshold gets pulled down toward it
    // over the attack; a signal below the threshold passes essentially intact.
    let settled = |amp| {
        // threshold -20 dB (~0.1 linear), ratio 10, hard knee.
        let fx = PostFx {
            compressor: Some(-20.0),
            comp_ratio: 10.0,
            comp_knee: 0.0,
            comp_attack: 0.001,
            comp_release: 0.05,
            ..Default::default()
        };
        assert!(fx.is_active());
        let mut v = PostFxVoice::new(Box::new(ConstVoice(amp)), fx, 44100.0);
        let mut last = 0.0f32;
        for _ in 0..4410 {
            last = v.tick().0.abs();
        }
        last
    };

    // Loud (0 dB = 1.0): far above -20 dB threshold -> heavily reduced.
    let loud = settled(1.0);
    assert!(loud < 0.5, "loud signal should be compressed, got {loud}");
    // Quiet (-40 dB ~ 0.01): below threshold -> passes ~unchanged.
    let quiet = settled(0.01);
    assert!(
        (quiet - 0.01).abs() < 5e-4,
        "quiet signal should pass intact, got {quiet}"
    );
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

proptest! {
    #[test]
    fn crush_quantizes_to_the_expected_grid(input in -1.0f32..1.0f32, bits in 1.0f32..8.0f32) {
        let fx = PostFx {
            crush: Some(bits),
            postgain: 1.0,
            shapevol: 1.0,
            distortvol: 1.0,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(input)), fx, 44100.0);
        let (l, r) = v.tick();
        let grid = 2f32.powf(bits.max(1.0) - 1.0);
        let expected = (input * grid).round() / grid;

        prop_assert_eq!(l, expected);
        prop_assert_eq!(r, expected);
    }
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

proptest! {
    #[test]
    fn coarse_holds_the_first_sample_of_each_window(hold in 1u32..16) {
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
            coarse: Some(hold as f32),
            postgain: 1.0,
            shapevol: 1.0,
            distortvol: 1.0,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(Ramp(0.0)), fx, 44100.0);
        let hold = hold as usize;
        let out: Vec<f32> = (0..(hold * 3)).map(|_| v.tick().0).collect();

        for (idx, sample) in out.into_iter().enumerate() {
            let expected = ((idx / hold) * hold + 1) as f32;
            prop_assert_eq!(sample, expected);
        }
    }
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
fn distort_algo_resolves_from_name_and_index() {
    // String names map to the algorithm; numbers index superdough's order,
    // wrapping; unknown names fall back to the default (scurve).
    assert_eq!(
        DistortAlgo::from_value(&Value::Str("soft".into())),
        DistortAlgo::Soft
    );
    assert_eq!(
        DistortAlgo::from_value(&Value::Str("diode".into())),
        DistortAlgo::Diode
    );
    assert_eq!(DistortAlgo::from_value(&Value::Int(0)), DistortAlgo::Scurve);
    assert_eq!(DistortAlgo::from_value(&Value::Int(2)), DistortAlgo::Hard);
    assert_eq!(DistortAlgo::from_value(&Value::Int(9)), DistortAlgo::Scurve); // wraps
    assert_eq!(
        DistortAlgo::from_value(&Value::Str("nope".into())),
        DistortAlgo::Scurve
    );
}

#[test]
fn distort_algorithms_match_reference_formulas() {
    // At drive k=0 each algorithm reduces to its documented base curve
    // (ported sample-for-sample from superdough/helpers.mjs).
    let x = 0.4f32;
    // scurve(x, 0) = x (identity at zero drive).
    assert!((DistortAlgo::Scurve.shape(x, 0.0) - x).abs() < 1e-6);
    // soft(x, 0) = tanh(x).
    assert!((DistortAlgo::Soft.shape(x, 0.0) - x.tanh()).abs() < 1e-6);
    // hard clamps the boosted signal to [-1, 1].
    assert_eq!(DistortAlgo::Hard.shape(2.0, 1.0), 1.0);
    assert_eq!(DistortAlgo::Hard.shape(-2.0, 1.0), -1.0);
    // fold(x, 0) is the identity on [0, 1] and stays within [-1, 1] everywhere.
    assert!((DistortAlgo::Fold.shape(x, 0.0) - x).abs() < 1e-6);
    for xi in [-5.0, -1.7, 0.0, 0.9, 3.3, 7.5] {
        let y = DistortAlgo::Fold.shape(xi, 3.0);
        assert!((-1.0..=1.0).contains(&y), "fold out of range: {y}");
    }
    // Every algorithm maps silence to silence and stays finite.
    for alg in [
        DistortAlgo::Scurve,
        DistortAlgo::Soft,
        DistortAlgo::Hard,
        DistortAlgo::Cubic,
        DistortAlgo::Diode,
        DistortAlgo::Asym,
        DistortAlgo::Fold,
        DistortAlgo::Sinefold,
        DistortAlgo::Chebyshev,
    ] {
        assert!(
            alg.shape(0.0, 2.0).abs() < 1e-6,
            "{alg:?} should map 0 -> 0"
        );
        assert!(
            alg.shape(0.6, 5.0).is_finite(),
            "{alg:?} produced a non-finite sample"
        );
    }
}

#[test]
fn distorttype_selects_the_algorithm_in_the_voice() {
    // The PostFx voice applies the algorithm chosen by `distorttype`: a hard
    // clipper on a boosted const input saturates to exactly 1.0, while the
    // default s-curve does not reach 1.0.
    let mk = |alg| PostFx {
        distort: Some(2.0),
        distort_alg: alg,
        ..Default::default()
    };
    let mut hard = PostFxVoice::new(Box::new(ConstVoice(0.9)), mk(DistortAlgo::Hard), 44100.0);
    assert_eq!(hard.tick().0, 1.0);
    let mut scurve = PostFxVoice::new(Box::new(ConstVoice(0.9)), mk(DistortAlgo::Scurve), 44100.0);
    assert!(scurve.tick().0 < 1.0);
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
