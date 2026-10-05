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
    // ftype parses on params, indexing ['12db', 'ladder', '24db'] (wrapping).
    let model_of = |f: Value| {
        VoiceParams::from_controls(
            &ValueMap::from([
                ("cutoff".to_string(), Value::F64(500.0)),
                ("ftype".to_string(), f),
            ]),
            1.0,
        )
        .lp
        .model
    };
    assert_eq!(model_of(Value::F64(0.0)), FilterModel::Db12);
    assert_eq!(model_of(Value::F64(1.0)), FilterModel::Ladder);
    assert_eq!(model_of(Value::F64(2.0)), FilterModel::Db24);
    assert_eq!(model_of(Value::F64(4.0)), FilterModel::Ladder); // wraps
    assert_eq!(model_of(Value::Str("ladder".into())), FilterModel::Ladder);
    assert_eq!(model_of(Value::Str("24db".into())), FilterModel::Db24);
}

#[test]
fn ladder_ftype_rolls_off_more_than_the_default_biquad() {
    // The Moog ladder is a 4-pole lowpass, so at the same cutoff it should pass
    // less of a bright signal than the default single biquad.
    let peak = |ftype: Value| {
        let p = VoiceParams::from_controls(
            &ValueMap::from([
                ("s".to_string(), Value::Str("sawtooth".into())),
                ("freq".to_string(), Value::F64(2000.0)),
                ("cutoff".to_string(), Value::F64(300.0)),
                ("ftype".to_string(), ftype),
            ]),
            0.2,
        );
        let mut v = Voice::new(p, 44100.0);
        (0..8000)
            .map(|_| v.tick().0.abs())
            .skip(2000)
            .fold(0.0f32, f32::max)
    };
    let biquad = peak(Value::F64(0.0));
    let ladder = peak(Value::Str("ladder".into()));
    assert!(
        ladder < biquad,
        "ladder ({ladder}) should attenuate more than the 12dB biquad ({biquad})"
    );
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
    // lowpass/highpass read their Q in dB, so those cases go through the voice
    // filter, with the case's Q as the pattern's `lpq`/`hpq`.
    use crate::filter::{Biquad, FilterKind, FilterParams, VoiceFilter};

    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tools/oracle/biquad_golden.json"))
            .expect("parse golden");
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

        let voice = |kind| {
            let fp = FilterParams {
                freq: Some(freq),
                q,
                ..FilterParams::default()
            };
            VoiceFilter::new(kind, &fp, sr)
        };
        let mut filter: Box<dyn FnMut(f32) -> f32> = match kind {
            "bandpass" => {
                let mut b = Biquad::bandpass(sr, freq, q);
                Box::new(move |x| b.process(x))
            }
            "notch" => {
                let mut b = Biquad::notch(sr, freq, q);
                Box::new(move |x| b.process(x))
            }
            "lowpass" | "highpass" => {
                let mut f = voice(if kind == "lowpass" {
                    FilterKind::Low
                } else {
                    FilterKind::High
                });
                Box::new(move |x| f.process(x, 0.0, 1.0, sr, 0.0, 0.0))
            }
            other => panic!("unexpected filter type in golden: {other}"),
        };
        for (i, &expected) in want.iter().enumerate().take(n) {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let got = filter(x);
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

#[test]
fn a_filter_lfo_follows_create_filter() {
    // helpers.mjs `createFilter`: any of depth/depthfrequency/skew/shape/rate
    // makes an LFO on the cutoff, `depth` (default 1) times the cutoff unless
    // `depthfrequency` says, at one cycle unless `rate`/`sync` says.
    use crate::filter::FilterSet;
    use rudel_core::{Value, ValueMap};
    let set = |pairs: &[(&str, f64)]| {
        let mut m = ValueMap::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), Value::F64(*v));
        }
        let mut set = FilterSet::from_controls(&m);
        set.set_lfos(&m, 0.5, 3.0);
        set
    };
    let lp = set(&[("cutoff", 800.0), ("lpdepth", 0.5)])
        .lp
        .lfo
        .expect("lpdepth starts one");
    assert_eq!((lp.depth, lp.frequency, lp.dcoffset), (400.0, 0.5, -0.5));
    assert_eq!((lp.min, lp.max), (-770.0, 19200.0));
    assert_eq!(lp.time, 6.0, "phase follows cycle time");
    let hp = set(&[
        ("hcutoff", 300.0),
        ("hpsync", 2.0),
        ("hpdepthfrequency", 50.0),
    ])
    .hp
    .lfo
    .expect("hpsync starts one");
    assert_eq!((hp.frequency, hp.depth), (1.0, 50.0));
    let bp = set(&[
        ("bandf", 1000.0),
        ("bprate", 3.0),
        ("bpdc", 0.0),
        ("bpskew", 0.2),
    ])
    .bp
    .lfo
    .expect("bprate starts one");
    assert_eq!(
        (bp.frequency, bp.dcoffset, bp.skew, bp.depth),
        (3.0, 0.0, 0.2, 1000.0)
    );
    assert!(
        set(&[("cutoff", 800.0)]).lp.lfo.is_none(),
        "no LFO controls, no LFO"
    );
    assert!(
        set(&[("lpdepth", 1.0)]).lp.lfo.is_none(),
        "no filter, no LFO"
    );
    assert!(
        set(&[("cutoff", 800.0), ("lpdepth", 0.0)]).lp.lfo.is_none(),
        "depth 0"
    );
}

#[test]
fn the_filter_lfo_rides_on_the_cutoff_in_hz() {
    // getParamLfo connects the LFO to the frequency param: its output is
    // summed onto the cutoff, exactly as a modulator's offset is.
    use crate::filter::{FilterKind, FilterParams, VoiceFilter};
    use crate::modulator::{Lfo, LfoConfig};
    let cfg = LfoConfig {
        frequency: 5.0,
        depth: 600.0,
        min: -770.0,
        max: 19200.0,
        ..LfoConfig::default()
    };
    let fp = FilterParams {
        freq: Some(800.0),
        lfo: Some(cfg),
        ..FilterParams::default()
    };
    let plain_fp = FilterParams { lfo: None, ..fp };
    let mut with_lfo = VoiceFilter::new(FilterKind::Low, &fp, 44100.0);
    let mut fed = VoiceFilter::new(FilterKind::Low, &plain_fp, 44100.0);
    let mut lfo = Lfo::new(&cfg, 44100.0);
    for i in 0..2000 {
        let x = ((i * 7919) % 200) as f32 / 100.0 - 1.0;
        let a = with_lfo.process(x, 0.0, 1.0, 44100.0, 0.0, 0.0);
        let b = fed.process(x, 0.0, 1.0, 44100.0, lfo.tick() as f32, 0.0);
        assert_eq!(a, b, "sample {i}");
    }
}
