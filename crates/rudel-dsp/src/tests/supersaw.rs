//! Audio parity for the super-saw oscillator against superdough's
//! `supersaw-oscillator` worklet.
//!
//! `supersaw_golden.json` (from tools/oracle/gen_supersaw_oracle.mjs) holds, per
//! case, the voice/spread settings, the initial phase of every unison voice, and
//! the exact stereo output the real worklet loop produces at 44.1kHz — already
//! scaled by synth.mjs's `1/sqrt(voices)`, which rudel folds into
//! `next_supersaw`.
//!
//! This drives `next_supersaw` directly rather than `tick`, so the comparison is
//! against the oscillator alone with no envelope or filter in the path. It
//! replaces "the supersaw makes some noise" with "the supersaw makes *these
//! samples*": the previous coverage here left the whole polyBLEP window
//! calculation free to be rewired without a test noticing.

use super::common::*;

const SAMPLE_RATE: f32 = 44100.0;

/// rudel renders in `f32` (and eight lanes at a time) where the oracle is JS
/// `f64`, so this bounds accumulated single-precision drift. The polyBLEP
/// windows divide by `dt`, which is ~0.002 at 110Hz, so a rounding difference in
/// the phase is amplified ~500x inside them — that sets the floor here, not the
/// saw itself. A real algorithmic divergence is orders of magnitude larger: the
/// saw ramps across [-1, 1], so a wrong branch or operator moves a sample by
/// ~0.1 upwards.
const EPS: f32 = 2e-4;

#[test]
fn supersaw_matches_the_superdough_worklet() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/oracle/supersaw_golden.json"
    ))
    .expect("parse golden");
    assert_eq!(golden["sample_rate"].as_f64().unwrap() as f32, SAMPLE_RATE);

    let cases = golden["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "golden has no cases");

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let voices = case["voices"].as_u64().unwrap() as usize;
        let params = VoiceParams {
            supersaw: true,
            unison: voices,
            freqspread: case["freqspread"].as_f64().unwrap() as f32,
            panspread: case["panspread"].as_f64().unwrap() as f32,
            freq: case["frequency"].as_f64().unwrap() as f32,
            ..Default::default()
        };
        let want_l = floats(&case["left"]);
        let want_r = floats(&case["right"]);
        let trace: Vec<Vec<f32>> = case["phase_trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(floats)
            .collect();
        assert_eq!(trace.len(), want_l.len() + 1, "{name}: phase trace length");

        let mut v = Voice::new(params, SAMPLE_RATE);
        let (mut worst, mut at, mut side) = (0.0f32, 0usize, "L");
        for (i, (wl, wr)) in want_l.iter().zip(&want_r).enumerate() {
            // Plant this sample's phases over whatever the previous call (or
            // `rand_phase`) left. Only the live voices are overwritten; the SIMD
            // padding lanes keep the 0.5 that makes them contribute nothing.
            v.super_phases[..voices].copy_from_slice(&trace[i]);
            let (gl, gr) = v.next_supersaw();

            for (got, want, which) in [(gl, wl, "L"), (gr, wr, "R")] {
                // Non-finite counts as an infinite deviation: every comparison
                // against a NaN is false, so a plain `> worst` would let NaN
                // output pass as a match.
                let d = if got.is_finite() {
                    (got - want).abs()
                } else {
                    f32::INFINITY
                };
                if d > worst {
                    worst = d;
                    at = i;
                    side = which;
                }
            }
            // ...and the phases it advanced to must be the oracle's next row,
            // which is what pins the `phase + dt`, wrap and detune-ratio maths.
            for (voice, (got, want)) in v.super_phases[..voices]
                .iter()
                .zip(&trace[i + 1])
                .enumerate()
            {
                if (got - want).abs() > EPS {
                    failures.push(format!(
                        "{name}: phase drift {:.3e} on voice {voice} after sample {i}, \
                         want {want:.6} got {got:.6}",
                        (got - want).abs()
                    ));
                    break;
                }
            }
        }
        if worst > EPS {
            failures.push(format!(
                "{name}: worst deviation {worst:.3e} at sample {at} ({side}), \
                 want {:.6}, tolerance {EPS:.1e}",
                if side == "L" { want_l[at] } else { want_r[at] }
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "supersaw parity:\n{}",
        failures.join("\n")
    );
}

/// The stereo split is not incidental: superdough swaps the left/right gain pair
/// on every unison voice, so an even voice count lands the halves on opposite
/// sides. A single voice gets no spread at all.
#[test]
fn pan_spread_alternates_per_voice_and_collapses_at_one_voice() {
    let render = |unison, panspread| {
        let mut v = Voice::new(
            VoiceParams {
                supersaw: true,
                unison,
                panspread,
                freq: 220.0,
                ..Default::default()
            },
            SAMPLE_RATE,
        );
        (0..256).map(|_| v.next_supersaw()).collect::<Vec<_>>()
    };

    let widest = render(4, 1.0)
        .iter()
        .fold(0.0f32, |m, (l, r)| m.max((l - r).abs()));
    assert!(
        widest > 0.0,
        "alternating pan gains must separate the channels"
    );

    for (l, r) in render(1, 1.0) {
        assert_eq!(l, r, "a single voice is centred whatever the spread");
    }
}

fn floats(v: &serde_json::Value) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect()
}
