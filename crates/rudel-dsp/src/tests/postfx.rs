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
    let out: Vec<f32> = (0..4000).map(|_| v.tick().0).collect();
    assert_is_signal(&out, "white noise through the \"a\" formant");
}

#[test]
fn vowel_formant_impulse_response_matches_webaudio() {
    // Sample-for-sample golden against superdough's VowelNode rendered through a
    // real Web Audio graph (OfflineAudioContext; tools/oracle/gen_vowel_oracle.mjs):
    // input -> 5 parallel bandpass biquads -> per-formant gains -> x8 makeup.
    // A `PostFxVoice` with only `vowel` set applies exactly that bank, so feeding
    // a unit impulse (`ImpulseVoice`) yields the vowel filter's impulse response.
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tools/oracle/vowel_golden.json"))
            .expect("parse golden");
    let sr = golden["sampleRate"].as_f64().unwrap() as f32;
    let n = golden["length"].as_u64().unwrap() as usize;

    // High-Q (80..140) bandpass biquads ring for many samples; f32 vs WebAudio's
    // f64 stays within this bound over the 64-sample window.
    const EPS: f32 = 1e-3;

    let mut failures = Vec::new();
    for case in golden["cases"].as_array().unwrap() {
        let vowel = case["vowel"].as_str().unwrap();
        let want: Vec<f32> = case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let fx = PostFx {
            vowel: Vowel::from_name(vowel),
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ImpulseVoice(false)), fx, sr);
        for (i, w) in want.iter().enumerate().take(n) {
            let got = v.tick().0;
            let d = (got - w).abs();
            if d > EPS {
                failures.push(format!(
                    "vowel {vowel} sample[{i}] = {got} vs webaudio {w} (diff {d:.3e})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "vowel impulse-response mismatches vs WebAudio:\n{}",
        failures.join("\n")
    );
}

/// One-shot impulse: 1.0 on the first tick, silence after.
struct ImpulseVoice(bool);
impl VoiceLike for ImpulseVoice {
    fn tick(&mut self) -> (f32, f32) {
        let v = if self.0 { 0.0 } else { 1.0 };
        self.0 = true;
        (v, v)
    }
    fn is_done(&self) -> bool {
        false
    }
}

#[test]
fn phaser_swept_notch_impulse_response_matches_webaudio() {
    // Sample-for-sample golden against superdough's getPhaser rendered through a
    // real Web Audio graph (OfflineAudioContext; tools/oracle/gen_phaser_oracle.mjs):
    // a `notch` BiquadFilterNode at `phasercenter + 282` whose `detune` is swept
    // by superdough's triangle LFO (±sweep cents). A `PostFxVoice` with only the
    // phaser controls set applies exactly that swept notch, so feeding a unit
    // impulse yields its (time-varying) impulse response. This both verifies the
    // LFO-waveform correctness fix (triangle, not sine) and pins the swept-notch
    // rendering against WebAudio.
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tools/oracle/phaser_golden.json"))
            .expect("parse golden");
    let sr = golden["sampleRate"].as_f64().unwrap() as f32;
    let n = golden["length"].as_u64().unwrap() as usize;
    const EPS: f32 = 1e-3;

    let mut failures = Vec::new();
    for case in golden["cases"].as_array().unwrap() {
        let rate = case["rate"].as_f64().unwrap() as f32;
        let depth = case["depth"].as_f64().unwrap() as f32;
        let center = case["center"].as_f64().unwrap() as f32;
        let sweep = case["sweep"].as_f64().unwrap() as f32;
        let want: Vec<f32> = case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let fx = PostFx {
            phaser: Some(rate),
            phaserdepth: depth,
            phasercenter: center,
            phasersweep: sweep,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ImpulseVoice(false)), fx, sr);
        for (i, w) in want.iter().enumerate().take(n) {
            let got = v.tick().0;
            let d = (got - w).abs();
            if d > EPS {
                failures.push(format!(
                    "phaser rate={rate} center={center} sweep={sweep} sample[{i}] = {got} vs webaudio {w} (diff {d:.3e})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "phaser impulse-response mismatches vs WebAudio:\n{}",
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
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

#[test]
fn process_block_matches_tick_for_memoryless_fx() {
    // The vectorized `process_block` fast path must be sample-for-sample
    // equivalent to the per-sample `tick` chain it replaces. Render two
    // identical saw + memoryless-post-fx voices, one by blocks and one by
    // ticks, and confirm they agree. The block size is a non-multiple of the
    // SIMD width so both the 8-wide body and the scalar remainder are covered.
    let sr = 48_000.0;
    // `shape` runs at its extreme in one of the cases below: the block path
    // builds its coefficients separately from the per-sample path, and the two
    // copies of the mapping did not agree at the bound.
    let cases = [
        PostFx {
            crush: Some(8.0),
            shape: Some(0.4),
            distort: Some(0.5),
            tremolo: Some(5.0),
            postgain: 0.8,
            ..Default::default()
        },
        PostFx {
            shape: Some(1.0),
            postgain: 0.9,
            ..Default::default()
        },
    ];
    for fx in cases {
        let saw = || VoiceParams {
            duration: 1.0e9,
            waveform: Waveform::Saw,
            ..Default::default()
        };
        let mut by_block = PostFxVoice::new(Box::new(Voice::new(saw(), sr)), fx, sr);
        let mut by_tick = PostFxVoice::new(Box::new(Voice::new(saw(), sr)), fx, sr);

        let block = 100;
        let (mut bl, mut br) = (vec![0.0f32; block], vec![0.0f32; block]);
        let mut max_diff = 0.0f32;
        for _ in 0..20 {
            by_block.process_block(&mut bl, &mut br);
            for k in 0..block {
                let (tl, tr) = by_tick.tick();
                // Checked explicitly: `f32::max` drops NaN, so a chain that has
                // gone non-finite on *both* paths would leave `max_diff` at zero
                // and sail straight through the comparison below.
                assert!(
                    bl[k].is_finite() && br[k].is_finite() && tl.is_finite() && tr.is_finite(),
                    "non-finite sample at {k}: block ({}, {}), tick ({tl}, {tr})",
                    bl[k],
                    br[k]
                );
                max_diff = max_diff.max((bl[k] - tl).abs()).max((br[k] - tr).abs());
            }
        }
        assert!(
            max_diff < 1e-4,
            "process_block diverged from tick (max diff {max_diff:e})"
        );
    }
}

#[test]
fn stretch_shifts_a_voice_up_an_octave() {
    // `stretch(1)` is pitchFactor 2 in superdough's mapping; a 220Hz sine
    // through the post-fx chain should come out around 440Hz.
    let sr = 44100.0;
    let params = VoiceParams {
        duration: 1.0e9,
        freq: 220.0,
        waveform: Waveform::Sine,
        ..Default::default()
    };
    let fx = PostFx {
        stretch: Some(1.0),
        ..Default::default()
    };
    assert!(fx.is_active(), "stretch must engage the post-fx wrapper");

    let mut voice = PostFxVoice::new(Box::new(Voice::new(params, sr)), fx, sr);
    // Discard the vocoder's fill-up, then capture a settled window.
    for _ in 0..8192 {
        voice.tick();
    }
    let n = 4096;
    let mut buf = Vec::with_capacity(n);
    for _ in 0..n {
        buf.push(voice.tick().0);
    }

    // Goertzel energy at 220Hz vs 440Hz: the shifted partial should dominate.
    let energy = |hz: f32| -> f32 {
        let w = TAU * hz / sr;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in buf.iter().enumerate() {
            // Hann-window so the two bins do not leak into each other.
            let win = 0.5 * (1.0 - (TAU * i as f32 / n as f32).cos());
            re += s * win * (w * i as f32).cos();
            im += s * win * (w * i as f32).sin();
        }
        re * re + im * im
    };
    let (low, high) = (energy(220.0), energy(440.0));
    assert!(
        high > low * 4.0,
        "expected the octave-up partial to dominate: 220Hz={low:e}, 440Hz={high:e}"
    );
}

// --- the compressor's static curve ------------------------------------------
//
// `compressor_attenuates_loud_signal_but_not_quiet` above only checks the
// direction: loud goes down, quiet does not. That leaves the whole level curve
// — the ratio, the soft knee, and which branch a level lands in — free to be
// rewritten. With a constant input the smoothing settles to the target gain, so
// the steady-state output level *is* the curve's output, in dB.

/// A stereo constant, for checking the detector links the two channels.
struct StereoConstVoice(f32, f32);
impl VoiceLike for StereoConstVoice {
    fn tick(&mut self) -> (f32, f32) {
        (self.0, self.1)
    }
    fn is_done(&self) -> bool {
        false
    }
}

fn compressed_db(amp: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let fx = PostFx {
        compressor: Some(threshold),
        comp_ratio: ratio,
        comp_knee: knee,
        comp_attack: 0.0005,
        comp_release: 0.0005,
        ..Default::default()
    };
    let mut v = PostFxVoice::new(Box::new(ConstVoice(amp)), fx, 44100.0);
    let mut last = 0.0f32;
    for _ in 0..44100 {
        last = v.tick().0.abs();
    }
    20.0 * last.max(1e-9).log10()
}

#[test]
fn the_compressor_follows_its_level_curve_in_every_region() {
    // out = in below the knee; threshold + over/ratio above it; a quadratic
    // interpolation across the knee itself.
    let threshold = -20.0;
    let ratio = 4.0;

    // Hard knee, well below threshold: untouched.
    let quiet = compressed_db(0.01, threshold, ratio, 0.0); // -40 dB in
    assert!(
        (quiet - -40.0).abs() < 0.2,
        "below threshold should pass at unity, got {quiet:.2} dB"
    );

    // Hard knee, well above: `threshold + over / ratio`. 0 dB in is 20 dB over,
    // so out = -20 + 20/4 = -15 dB.
    let loud = compressed_db(1.0, threshold, ratio, 0.0);
    assert!(
        (loud - -15.0).abs() < 0.2,
        "above threshold should follow threshold + over/ratio = -15 dB, got {loud:.2}"
    );

    // The ratio is what sets the slope: at 2:1 the same input gives -10 dB.
    let gentler = compressed_db(1.0, threshold, 2.0, 0.0);
    assert!(
        (gentler - -10.0).abs() < 0.2,
        "2:1 on 20 dB over should give -10 dB, got {gentler:.2}"
    );
    // ...and ratio 1 is no compression at all.
    let unity = compressed_db(1.0, threshold, 1.0, 0.0);
    assert!(
        (unity - 0.0).abs() < 0.2,
        "ratio 1 should leave the level alone, got {unity:.2}"
    );

    // Exactly at the threshold with a hard knee: still unity (over = 0 lands in
    // the `over <= -knee/2` branch when knee is 0).
    let at_threshold = compressed_db(0.1, threshold, ratio, 0.0);
    assert!(
        (at_threshold - -20.0).abs() < 0.2,
        "at threshold with a hard knee should be unity, got {at_threshold:.2}"
    );
}

#[test]
fn the_compressor_knee_softens_the_corner() {
    // With a 12 dB knee the curve starts bending 6 dB *below* the threshold and
    // is fully into the ratio 6 dB above it. At the threshold itself the
    // quadratic has applied half the knee's worth of reduction, so a level that
    // a hard knee leaves untouched comes out lower.
    let threshold = -20.0;
    let ratio = 4.0;
    let knee = 12.0;

    let hard = compressed_db(0.1, threshold, ratio, 0.0);
    let soft = compressed_db(0.1, threshold, ratio, knee);
    assert!(
        soft < hard - 0.5,
        "a soft knee should already be reducing at the threshold: {soft:.2} vs {hard:.2} dB"
    );

    // Below the knee's lower edge (-26 dB in) both are untouched.
    let below = compressed_db(0.0501, threshold, ratio, knee); // ~-26 dB
    assert!(
        (below - -26.0).abs() < 0.4,
        "below the knee the soft curve is still unity, got {below:.2}"
    );

    // The knee is quadratic: at the threshold the reduction is exactly
    // `(1/ratio - 1) * (knee/2)^2 / (2*knee)` = (0.25-1)*36/24 = -1.125 dB.
    assert!(
        (soft - (-20.0 - 1.125)).abs() < 0.3,
        "the knee formula should give -21.125 dB at the threshold, got {soft:.2}"
    );
}

#[test]
fn the_compressor_detector_links_the_two_channels() {
    // The level is `max(|l|, |r|)`, so a loud left pulls the quiet right down
    // with it rather than each side compressing on its own.
    let fx = PostFx {
        compressor: Some(-20.0),
        comp_ratio: 8.0,
        comp_knee: 0.0,
        comp_attack: 0.0005,
        comp_release: 0.0005,
        ..Default::default()
    };
    let mut v = PostFxVoice::new(Box::new(StereoConstVoice(1.0, 0.05)), fx, 44100.0);
    let mut last = (0.0f32, 0.0f32);
    for _ in 0..44100 {
        last = v.tick();
    }
    // Both sides took the same gain, so their ratio is unchanged from 1.0:0.05.
    let ratio_out = last.1.abs() / last.0.abs();
    assert!(
        (ratio_out - 0.05).abs() < 1e-3,
        "both channels should take one linked gain, got ratio {ratio_out:.4}"
    );
    // ...and that gain is the one the *louder* channel asked for.
    let left_db = 20.0 * last.0.abs().log10();
    assert!(
        (left_db - -17.5).abs() < 0.3,
        "the loud side should land on the curve (-20 + 20/8), got {left_db:.2}"
    );
}

#[test]
fn the_compressor_attacks_faster_than_it_releases() {
    // Reduction deepening uses `comp_attack`, recovery uses `comp_release`, and
    // the smoothing is a one-pole with `exp(-1 / (time * sr))`. A slow attack
    // must still be on its way down after the same number of samples that a fast
    // attack has finished in.
    let after = |attack: f32, n: usize| {
        let fx = PostFx {
            compressor: Some(-20.0),
            comp_ratio: 8.0,
            comp_knee: 0.0,
            comp_attack: attack,
            comp_release: 0.5,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(1.0)), fx, 44100.0);
        let mut last = 0.0f32;
        for _ in 0..n {
            last = v.tick().0.abs();
        }
        last
    };
    let n = 441; // 10ms
    let fast = after(0.001, n);
    let slow = after(0.2, n);
    assert!(
        slow > fast * 2.0,
        "a slow attack should still be well above a fast one at 10ms: {slow:.4} vs {fast:.4}"
    );

    // Release: once the input drops below the threshold the gain climbs back.
    // Compare a fast and a slow release from the same compressed state.
    let recovered = |release: f32| {
        let fx = PostFx {
            compressor: Some(-20.0),
            comp_ratio: 8.0,
            comp_knee: 0.0,
            comp_attack: 0.0005,
            comp_release: release,
            ..Default::default()
        };
        // Loud for long enough to settle, then quiet.
        let mut v = PostFxVoice::new(Box::new(RampVoice::new(1.0, 0.001, 4410)), fx, 44100.0);
        let mut last = 0.0f32;
        for _ in 0..(4410 + 441) {
            last = v.tick().0.abs();
        }
        last / 0.001 // gain applied to the quiet part
    };
    let quick = recovered(0.005);
    let slow_release = recovered(1.0);
    assert!(
        quick > slow_release * 1.5,
        "a fast release should have recovered more gain: {quick:.3} vs {slow_release:.3}"
    );
}

/// Loud for `switch` samples, then quiet — for watching the release.
struct RampVoice {
    loud: f32,
    quiet: f32,
    switch: usize,
    n: usize,
}
impl RampVoice {
    fn new(loud: f32, quiet: f32, switch: usize) -> RampVoice {
        RampVoice {
            loud,
            quiet,
            switch,
            n: 0,
        }
    }
}
impl VoiceLike for RampVoice {
    fn tick(&mut self) -> (f32, f32) {
        let v = if self.n < self.switch {
            self.loud
        } else {
            self.quiet
        };
        self.n += 1;
        (v, v)
    }
    fn is_done(&self) -> bool {
        false
    }
}

// --- the shape waveshaper ---------------------------------------------------

#[test]
fn shape_follows_its_hyperbolic_curve() {
    // `k = 2s / (1 - s)`, then `y = (1 + k)x / (1 + k|x|)`, times the postgain.
    // Nothing tested this stage at all.
    let through = |shape: f32, shapevol: f32, x: f32| {
        let fx = PostFx {
            shape: Some(shape),
            shapevol,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(x)), fx, 44100.0);
        v.tick().0
    };

    // Shape 0 is the identity (k = 0 gives y = x).
    for x in [-0.8f32, -0.2, 0.0, 0.3, 0.9] {
        assert!(
            (through(0.0, 1.0, x) - x).abs() < 1e-6,
            "shape 0 should pass {x} through unchanged"
        );
    }

    // The curve is expansive on small signals and saturating on large ones: at
    // shape 0.5, k = 2, so y = 3x / (1 + 2|x|).
    let expect = |x: f32| 3.0 * x / (1.0 + 2.0 * x.abs());
    for x in [0.1f32, 0.5, 1.0] {
        let got = through(0.5, 1.0, x);
        assert!(
            (got - expect(x)).abs() < 1e-5,
            "shape 0.5 at {x} should be {:.5}, got {got:.5}",
            expect(x)
        );
    }

    // Odd symmetry, and the postgain scales the result.
    assert!((through(0.5, 1.0, -0.4) + through(0.5, 1.0, 0.4)).abs() < 1e-6);
    let full = through(0.5, 1.0, 0.6);
    let half = through(0.5, 0.5, 0.6);
    assert!(
        (half - full * 0.5).abs() < 1e-5,
        "shapevol should scale the output: {half:.5} vs {full:.5}"
    );

    // Shape is clamped just below 1 so the `1 - shape` divisor cannot blow up.
    // Upstream's bound is `1.0 - 4e-10`, which rounds back to exactly 1.0 in
    // f32 and left `.shape(1)` emitting NaN into the mix.
    for x in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        assert!(
            through(1.0, 1.0, x).is_finite(),
            "shape 1 at {x} must not produce a non-finite sample"
        );
    }
    // At the bound it is effectively a hard clipper: everything but silence
    // comes out near full scale, with the sign kept.
    assert!((through(1.0, 1.0, 0.1) - 1.0).abs() < 0.01);
    assert!((through(1.0, 1.0, -0.1) + 1.0).abs() < 0.01);
    assert_eq!(through(1.0, 1.0, 0.0), 0.0);
}

// --- postgain ---------------------------------------------------------------

#[test]
fn postgain_scales_the_whole_chain() {
    let through = |postgain: f32, x: f32| {
        let fx = PostFx {
            postgain,
            ..Default::default()
        };
        // `postgain` alone does not make the chain active, so pair it with a
        // stage that does; crush at 16 bits is effectively transparent here.
        let fx = PostFx {
            crush: Some(16.0),
            ..fx
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(x)), fx, 44100.0);
        v.tick().0
    };
    let unity = through(1.0, 0.5);
    assert!(
        (unity - 0.5).abs() < 1e-3,
        "unity postgain leaves the level"
    );
    assert!(
        (through(0.5, 0.5) - unity * 0.5).abs() < 1e-3,
        "postgain should scale linearly"
    );
    assert!(
        (through(2.0, 0.5) - unity * 2.0).abs() < 1e-3,
        "postgain above 1 should scale too"
    );
    assert_eq!(through(0.0, 0.5), 0.0, "zero postgain is silence");
}

// --- dispatch: which effects count, and which take the fast path ------------
//
// `is_active` decides whether a voice gets wrapped at all, `mod_base` supplies
// the value a relative modulation depth scales, and `memoryless_only` decides
// between the vectorized block path and the per-sample one. All three are pure
// bookkeeping over the same fields, and a wrong answer is silent: an effect
// that stops being "active" simply never runs.

#[test]
fn every_control_makes_the_chain_active() {
    assert!(!PostFx::default().is_active(), "a bare chain is inert");

    let each: [(&str, PostFx); 11] = [
        (
            "crush",
            PostFx {
                crush: Some(4.0),
                ..Default::default()
            },
        ),
        (
            "shape",
            PostFx {
                shape: Some(0.5),
                ..Default::default()
            },
        ),
        (
            "distort",
            PostFx {
                distort: Some(0.5),
                ..Default::default()
            },
        ),
        (
            "coarse",
            PostFx {
                coarse: Some(2.0),
                ..Default::default()
            },
        ),
        (
            "vowel",
            PostFx {
                vowel: Some(Vowel::A),
                ..Default::default()
            },
        ),
        (
            "postgain",
            PostFx {
                postgain: 0.5,
                ..Default::default()
            },
        ),
        (
            "tremolo",
            PostFx {
                tremolo: Some(4.0),
                ..Default::default()
            },
        ),
        (
            "phaser",
            PostFx {
                phaser: Some(1.0),
                ..Default::default()
            },
        ),
        (
            "compressor",
            PostFx {
                compressor: Some(-20.0),
                ..Default::default()
            },
        ),
        (
            "transient",
            PostFx {
                transient: Some(0.5),
                ..Default::default()
            },
        ),
        (
            "stretch",
            PostFx {
                stretch: Some(1.0),
                ..Default::default()
            },
        ),
    ];
    for (name, fx) in each {
        assert!(fx.is_active(), "{name} alone should activate the chain");
    }

    // `postgain` is the odd one: it is not an Option, so unity has to read as
    // inactive or every voice would be wrapped.
    assert!(
        !PostFx {
            postgain: 1.0,
            ..Default::default()
        }
        .is_active(),
        "unity postgain is not an effect"
    );
}

#[test]
fn mod_base_reports_each_targets_own_value() {
    // A relative modulation depth is scaled by the target control's current
    // value, so reading the wrong field silently mis-scales the modulation.
    let fx = PostFx {
        postgain: 0.7,
        shape: Some(0.25),
        shapevol: 0.6,
        distort: Some(1.5),
        distortvol: 0.4,
        crush: Some(6.0),
        coarse: Some(3.0),
        ..Default::default()
    };
    for (target, want) in [
        (ModTarget::Postgain, 0.7),
        (ModTarget::Shape, 0.25),
        (ModTarget::Shapevol, 0.6),
        (ModTarget::Distort, 1.5),
        (ModTarget::Distortvol, 0.4),
        (ModTarget::Crush, 6.0),
        (ModTarget::Coarse, 3.0),
    ] {
        assert_eq!(fx.mod_base(target), want, "{target:?} base");
    }
    // Targets this chain does not own read as zero.
    assert_eq!(fx.mod_base(ModTarget::Frequency), 0.0);
    assert_eq!(fx.mod_base(ModTarget::Cutoff), 0.0);

    // The optional ones fall back to zero rather than to their defaults.
    let bare = PostFx::default();
    for target in [
        ModTarget::Shape,
        ModTarget::Distort,
        ModTarget::Crush,
        ModTarget::Coarse,
    ] {
        assert_eq!(bare.mod_base(target), 0.0, "{target:?} with nothing set");
    }
}

#[test]
fn only_the_memoryless_chain_takes_the_block_path() {
    // The block path hoists its coefficients out of the loop, so it is only
    // valid when nothing is state-recursive and nothing is modulated. Checked
    // through the observable consequence: `process_block` has to agree with
    // `tick` for the ones that qualify, and the ones that do not are exactly
    // the stages that carry state between samples.
    let sr = 44100.0;
    let agrees = |fx: PostFx| {
        let src = || VoiceParams {
            duration: 1.0e9,
            waveform: Waveform::Saw,
            ..Default::default()
        };
        let mut by_block = PostFxVoice::new(Box::new(Voice::new(src(), sr)), fx, sr);
        let mut by_tick = PostFxVoice::new(Box::new(Voice::new(src(), sr)), fx, sr);
        let n = 64;
        let (mut bl, mut br) = (vec![0.0f32; n], vec![0.0f32; n]);
        by_block.process_block(&mut bl, &mut br);
        let mut worst = 0.0f32;
        for k in 0..n {
            let (tl, tr) = by_tick.tick();
            assert!(bl[k].is_finite() && tl.is_finite(), "non-finite at {k}");
            worst = worst.max((bl[k] - tl).abs()).max((br[k] - tr).abs());
        }
        worst
    };

    // Memoryless stages, alone and together.
    for (name, fx) in [
        (
            "crush",
            PostFx {
                crush: Some(6.0),
                ..Default::default()
            },
        ),
        (
            "shape",
            PostFx {
                shape: Some(0.4),
                ..Default::default()
            },
        ),
        (
            "tremolo",
            PostFx {
                tremolo: Some(6.0),
                ..Default::default()
            },
        ),
        (
            "postgain",
            PostFx {
                postgain: 0.6,
                ..Default::default()
            },
        ),
        (
            "default distort",
            PostFx {
                distort: Some(0.7),
                ..Default::default()
            },
        ),
    ] {
        assert!(
            agrees(fx) < 1e-4,
            "{name} should render identically on both paths"
        );
    }

    // State-recursive stages must fall back rather than be hoisted; they still
    // have to agree, because `process_block` defers to `tick` for them.
    for (name, fx) in [
        (
            "coarse",
            PostFx {
                coarse: Some(4.0),
                ..Default::default()
            },
        ),
        (
            "vowel",
            PostFx {
                vowel: Some(Vowel::A),
                ..Default::default()
            },
        ),
        (
            "compressor",
            PostFx {
                compressor: Some(-20.0),
                ..Default::default()
            },
        ),
        (
            "phaser",
            PostFx {
                phaser: Some(1.0),
                ..Default::default()
            },
        ),
    ] {
        assert!(
            agrees(fx) < 1e-4,
            "{name} should fall back to the per-sample path and still agree"
        );
    }

    // A non-default distortion curve is not hoistable either, and must still
    // match.
    let alt = PostFx {
        distort: Some(0.7),
        distort_alg: DistortAlgo::Fold,
        ..Default::default()
    };
    assert!(
        agrees(alt) < 1e-4,
        "a non-scurve distortion should still agree across paths"
    );
}

#[test]
fn distort_algo_names_all_resolve() {
    // Every name a user can type in `distorttype`.
    for (name, want) in [
        ("scurve", DistortAlgo::Scurve),
        ("soft", DistortAlgo::Soft),
        ("hard", DistortAlgo::Hard),
        ("cubic", DistortAlgo::Cubic),
        ("diode", DistortAlgo::Diode),
        ("asym", DistortAlgo::Asym),
        ("fold", DistortAlgo::Fold),
        ("sinefold", DistortAlgo::Sinefold),
        ("chebyshev", DistortAlgo::Chebyshev),
    ] {
        assert_eq!(
            DistortAlgo::from_value(&Value::Str(name.into())),
            want,
            "{name}"
        );
    }
    // An unknown name falls back to the default rather than erroring.
    assert_eq!(
        DistortAlgo::from_value(&Value::Str("nope".into())),
        DistortAlgo::Scurve
    );
}
