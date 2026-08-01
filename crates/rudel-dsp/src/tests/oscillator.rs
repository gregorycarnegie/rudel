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
fn pulse_width_moves_the_duty_cycle() {
    // The duty is the fraction of the cycle spent high, so the switch point
    // tracks `pw` rather than sitting at the square wave's 0.5.
    assert_eq!(Waveform::pulse(0.24, 0.25), 1.0);
    assert_eq!(Waveform::pulse(0.26, 0.25), -1.0);
    assert_eq!(Waveform::pulse(0.74, 0.75), 1.0);
    assert_eq!(Waveform::pulse(0.76, 0.75), -1.0);

    // A duty of 0.5 is the square wave, and the extremes are stuck rails.
    for p in [0.0f32, 0.1, 0.49, 0.51, 0.9] {
        assert_eq!(Waveform::pulse(p, 0.5), Waveform::Square.sample(p));
    }
    assert!((0..10).all(|i| Waveform::pulse(i as f32 / 10.0, 0.0) == -1.0));
    assert!((0..10).all(|i| Waveform::pulse(i as f32 / 10.0, 1.0) == 1.0));

    // Out-of-range duty clamps rather than wrapping.
    assert_eq!(Waveform::pulse(0.5, 2.0), 1.0);
    assert_eq!(Waveform::pulse(0.5, -1.0), -1.0);
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
