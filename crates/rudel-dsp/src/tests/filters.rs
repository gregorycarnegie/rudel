use super::common::*;

#[test]
fn ftype_24db_cascades_the_filter() {
    // A 24dB low-pass attenuates high frequencies more than the 12dB default.
    // Drive each filter with a steady-ish high-frequency input and compare the
    // residual energy.
    fn residual(ftype: f64) -> f32 {
        let map = ValueMap::from([
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
            &ValueMap::from([
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
fn biquad_impulse_response_matches_webaudio() {
    // Sample-for-sample golden against a *real Web Audio graph*: an impulse
    // rendered through a BiquadFilterNode in an OfflineAudioContext (node, via
    // node-web-audio-api; see tools/oracle/gen_biquad_oracle.mjs). Only the
    // bandpass/notch types are golden-tested, because their Q is linear in both
    // WebAudio and the RBJ cookbook so they match Rudel's `Biquad` exactly;
    // lowpass/highpass use WebAudio's dB-Q convention and stay on smoke tests.
    use crate::filter::Biquad;

    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/biquad_golden.json")).expect("parse golden");
    let sr = golden["sampleRate"].as_f64().unwrap() as f32;
    let n = golden["length"].as_u64().unwrap() as usize;

    // f32 transposed-direct-form-II vs WebAudio's f64 biquad: stable bandpass /
    // notch impulse responses agree to well within this bound over 64 samples.
    const EPS: f32 = 2e-4;

    let mut failures = Vec::new();
    for case in golden["cases"].as_array().unwrap() {
        let kind = case["type"].as_str().unwrap();
        let freq = case["frequency"].as_f64().unwrap() as f32;
        let q = case["q"].as_f64().unwrap() as f32;
        let want: Vec<f32> = case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();

        let mut filter = match kind {
            "bandpass" => Biquad::bandpass(sr, freq, q),
            "notch" => Biquad::notch(sr, freq, q),
            other => panic!("unexpected filter type in golden: {other}"),
        };
        for (i, &expected) in want.iter().enumerate().take(n) {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let got = filter.process(x);
            let d = (got - expected).abs();
            if d > EPS {
                failures.push(format!(
                    "{kind} f={freq} q={q} sample[{i}] = {got} vs webaudio {expected} (diff {d:.3e})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "biquad impulse-response mismatches vs WebAudio:\n{}",
        failures.join("\n")
    );
}
