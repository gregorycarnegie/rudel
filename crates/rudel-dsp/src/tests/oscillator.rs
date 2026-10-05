//! Oscillator coverage: parity for the two parts with a real upstream (the
//! additive wavetable builder and the pink/brown noise filters), and the plain
//! waveform/table maths checked against its own definition.
//!
//! `oscillator_golden.json` comes from tools/oracle/gen_oscillator_oracle.mjs —
//! see that file for what is copied from superdough and what is written from the
//! Web Audio spec.

use super::common::*;
use crate::oscillator::{ADDITIVE_SIZE, AdditiveType, NoiseGen, build_additive, sample_table};

fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../tools/oracle/oscillator_golden.json"
    ))
    .expect("parse golden")
}

fn floats(v: &serde_json::Value) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect()
}

/// Worst deviation of `got` from `want`, with the index it happened at.
///
/// A non-finite sample counts as an infinite deviation rather than being
/// subtracted. Every comparison against a NaN is false, so `(a - b).abs() > eps`
/// silently *passes* for NaN output — and `f32::max` drops NaN as well, so a
/// table that has gone non-finite would otherwise sail through both the peak
/// normalisation and this check.
fn worst(got: &[f32], want: &[f32]) -> (f32, usize) {
    assert_eq!(got.len(), want.len(), "length");
    got.iter()
        .zip(want)
        .map(|(a, b)| {
            if a.is_finite() {
                (a - b).abs()
            } else {
                f32::INFINITY
            }
        })
        .enumerate()
        .fold(
            (0.0, 0),
            |(m, at), (i, d)| if d > m { (d, i) } else { (m, at) },
        )
}

/// rudel sums the harmonics eight lanes at a time in `f32`; the oracle sums them
/// scalar in `f64`. Both then divide by the peak, so this bounds the rounding
/// difference between those two summations. A wrong coefficient — a dropped
/// `1/n`, an even/odd test inverted, a rotation applied backwards — moves the
/// normalised table by O(0.1).
const ADDITIVE_EPS: f32 = 1e-4;

#[test]
fn additive_tables_match_superdough_waveform_n() {
    let golden = golden();
    assert_eq!(
        golden["additive_size"].as_u64().unwrap() as usize,
        ADDITIVE_SIZE
    );

    let mut failures = Vec::new();
    for case in golden["additive"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let base = AdditiveType::from_name(case["type"].as_str().unwrap()).expect("base type");
        let partials = floats(&case["partials"]);
        let phases = case["phases"].as_array().map(|_| floats(&case["phases"]));
        let want = floats(&case["table"]);

        let got = build_additive(&partials, phases.as_deref(), base);
        let (d, at) = worst(&got, &want);
        if d > ADDITIVE_EPS {
            failures.push(format!(
                "{name}: worst deviation {d:.3e} at slot {at}, want {:.6} got {:.6}",
                want[at], got[at]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "additive parity:\n{}",
        failures.join("\n")
    );
}

/// The pink filter's slowest pole sits at 0.99886, so a single-precision state
/// difference decays over ~870 samples rather than dying immediately; this
/// bounds that accumulation across the run. A changed filter coefficient moves
/// the output by orders of magnitude more, because these bands are summed
/// directly into the result.
const NOISE_EPS: f32 = 5e-4;

#[test]
fn noise_colouring_matches_superdough() {
    let golden = golden();
    let n = golden["noise_samples"].as_u64().unwrap() as usize;

    let mut failures = Vec::new();
    for case in golden["noise"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let kind = NoiseKind::from_name(name).expect("noise kind");
        let want = floats(&case["samples"]);
        assert_eq!(want.len(), n);

        let mut source = NoiseGen::new();
        let got: Vec<f32> = (0..n).map(|_| source.next(kind)).collect();
        let (d, at) = worst(&got, &want);
        if d > NOISE_EPS {
            failures.push(format!(
                "{name}: worst deviation {d:.3e} at sample {at}, want {:.6} got {:.6}",
                want[at], got[at]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "noise parity:\n{}",
        failures.join("\n")
    );
}

/// Every name here is what a user types in `s("…")`, so a dropped alias is a
/// sound that silently stops resolving.
#[test]
fn sound_names_resolve_to_their_kinds() {
    for (name, want) in [
        ("sine", Waveform::Sine),
        ("sin", Waveform::Sine),
        ("saw", Waveform::Saw),
        ("sawtooth", Waveform::Saw),
        ("square", Waveform::Square),
        ("sqr", Waveform::Square),
        ("triangle", Waveform::Triangle),
        ("tri", Waveform::Triangle),
        ("pulse", Waveform::Pulse),
    ] {
        assert_eq!(Waveform::from_name(name), Some(want), "waveform {name}");
    }
    assert_eq!(Waveform::from_name("bd"), None);

    for (name, want) in [
        ("sawtooth", AdditiveType::Saw),
        ("saw", AdditiveType::Saw),
        ("square", AdditiveType::Square),
        ("sqr", AdditiveType::Square),
        ("triangle", AdditiveType::Triangle),
        ("tri", AdditiveType::Triangle),
        ("user", AdditiveType::User),
    ] {
        assert_eq!(AdditiveType::from_name(name), Some(want), "additive {name}");
    }
    assert_eq!(AdditiveType::from_name("pulse"), None);

    for (name, want) in [
        ("white", NoiseKind::White),
        ("noise", NoiseKind::White),
        ("pink", NoiseKind::Pink),
        ("brown", NoiseKind::Brown),
    ] {
        assert_eq!(NoiseKind::from_name(name), Some(want), "noise {name}");
    }
    assert_eq!(NoiseKind::from_name("crackle"), None);
}

/// The plain waveforms have no upstream to compare against — Web Audio's
/// oscillators are band-limited, these are the naive shapes — so they are
/// checked against their own definitions at the points that distinguish them.
#[test]
fn waveform_sample_matches_its_definition() {
    let at = |w: Waveform, p: f32| w.sample(p);

    // Sine: zero at 0 and 0.5, +1 a quarter turn in, -1 three quarters in.
    for (p, want) in [(0.0, 0.0), (0.25, 1.0), (0.5, 0.0), (0.75, -1.0)] {
        assert!(
            (at(Waveform::Sine, p) - want).abs() < 1e-6,
            "sine at {p} should be {want}, got {}",
            at(Waveform::Sine, p)
        );
    }

    // Saw ramps -1 -> +1 across the cycle, crossing zero at the midpoint.
    assert_eq!(at(Waveform::Saw, 0.0), -1.0);
    assert_eq!(at(Waveform::Saw, 0.5), 0.0);
    assert!((at(Waveform::Saw, 0.75) - 0.5).abs() < 1e-6);

    // Square holds +1 over the first half and -1 over the second, flipping
    // exactly at 0.5 (which belongs to the low half).
    assert_eq!(at(Waveform::Square, 0.0), 1.0);
    assert_eq!(at(Waveform::Square, 0.499), 1.0);
    assert_eq!(at(Waveform::Square, 0.5), -1.0);
    assert_eq!(at(Waveform::Square, 0.999), -1.0);

    // Triangle peaks at the midpoint and bottoms out at the edges.
    assert_eq!(at(Waveform::Triangle, 0.0), -1.0);
    assert_eq!(at(Waveform::Triangle, 0.25), 0.0);
    assert_eq!(at(Waveform::Triangle, 0.5), 1.0);
    assert_eq!(at(Waveform::Triangle, 0.75), 0.0);

    // Phase is taken modulo one turn, and negative phase wraps forward rather
    // than reflecting.
    for w in [
        Waveform::Sine,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Triangle,
    ] {
        assert!(
            (at(w, 1.25) - at(w, 0.25)).abs() < 1e-6,
            "{w:?}: phase should wrap at 1.0"
        );
        assert!(
            (at(w, -0.75) - at(w, 0.25)).abs() < 1e-6,
            "{w:?}: negative phase should wrap forward"
        );
    }
}

#[test]
fn sample_table_interpolates_between_neighbours_and_wraps() {
    let table = [0.0f32, 1.0, 2.0, 3.0];

    // Exact slots read straight through.
    for (i, &want) in table.iter().enumerate() {
        assert_eq!(sample_table(&table, i as f32 / 4.0), want);
    }

    // Halfway between two slots is their midpoint...
    assert!((sample_table(&table, 0.125) - 0.5).abs() < 1e-6);
    assert!((sample_table(&table, 0.375) - 1.5).abs() < 1e-6);
    // ...including across the wrap from the last slot back to the first, which
    // interpolates 3 -> 0 rather than running off the end.
    assert!((sample_table(&table, 0.875) - 1.5).abs() < 1e-6);

    // Phase outside one turn wraps in both directions.
    assert_eq!(sample_table(&table, 1.25), sample_table(&table, 0.25));
    assert_eq!(sample_table(&table, -0.75), sample_table(&table, 0.25));
}

#[test]
fn band_limited_oscillators_match_chrome() {
    // tools/oracle/gen_chrome_oscillator_oracle.mjs: OscillatorNode rendered by
    // Chrome itself. Constant pitches across the range, a sweep crossing range
    // tables every sample, and LFO rates on the Lagrange interpolators.
    use crate::bandlimited::WaveTables;
    use crate::oscillator::Waveform;

    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/oracle/chrome_oscillator_golden.json"
    ))
    .expect("parse golden");
    let sr = golden["sampleRate"].as_f64().unwrap();
    let n = golden["length"].as_u64().unwrap() as usize;
    let mut failures = Vec::new();
    for case in golden["cases"].as_array().unwrap() {
        let kind = case["type"].as_str().unwrap();
        let shape = match kind {
            "sawtooth" => Waveform::Saw,
            "square" => Waveform::Square,
            "triangle" => Waveform::Triangle,
            other => panic!("unexpected type {other}"),
        };
        let f0 = case["frequency"].as_f64().unwrap();
        let f1 = case["to"].as_f64().unwrap_or(f0);
        let tables = WaveTables::get(shape, sr as f32);
        // Chrome keeps the read position in a double, advanced by a float.
        let mut phase = 0.0f64;
        let mut worst = (0.0f32, 0usize);
        for (i, want) in case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .take(n)
        {
            let freq = f0 + (f1 - f0) * i as f64 / n as f64;
            let cycles = (freq / sr) as f32;
            let got = tables.sample(phase as f32, cycles);
            let d = (got - want.as_f64().unwrap() as f32).abs();
            if d > worst.0 {
                worst = (d, i);
            }
            phase = (phase + cycles as f64).rem_euclid(1.0);
        }
        // rudel's phase is an f32 where Chrome reads from a double: ~1e-8 of a
        // cycle, worth about 1.5e-4 on a 55 Hz edge late in the render.
        if worst.0 > 2e-4 {
            failures.push(format!(
                "{kind} {f0}->{f1}: worst {:.2e} at [{}]",
                worst.0, worst.1
            ));
        }
    }
    assert!(failures.is_empty(), "vs Chrome:\n{}", failures.join("\n"));
}

#[test]
fn the_pulse_matches_superdoughs_worklet() {
    // tools/oracle/gen_pulse_oracle.mjs runs the `pulse-oscillator` processor
    // from superdough's own worklets.mjs.
    use crate::pulse::PulseOsc;

    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tools/oracle/pulse_golden.json"))
            .expect("parse golden");
    let sr = golden["sampleRate"].as_f64().unwrap();
    let mut failures = Vec::new();
    for case in golden["cases"].as_array().unwrap() {
        let freq = case["frequency"].as_f64().unwrap();
        let detune = case["detune"].as_f64().unwrap_or(0.0);
        let width = |i: usize| match &case["pulsewidth"] {
            serde_json::Value::String(_) => {
                0.5 + 0.4 * (std::f64::consts::TAU * i as f64 / 512.0).sin()
            }
            w => w.as_f64().unwrap(),
        };
        let mut osc = PulseOsc::default();
        let f = freq * 2f64.powf(detune / 100.0 / 12.0);
        for (i, want) in case["samples"].as_array().unwrap().iter().enumerate() {
            // The oracle's a-rate width is a Float32Array.
            let got = osc.next(f, width(i) as f32 as f64, sr);
            let want = want.as_f64().unwrap() as f32;
            if (got - want).abs() > 1e-5 {
                failures.push(format!(
                    "{case_f} pw={} [{i}]: {got} vs {want}",
                    case["pulsewidth"],
                    case_f = freq
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "vs superdough:\n{}",
        failures.join("\n")
    );
}
