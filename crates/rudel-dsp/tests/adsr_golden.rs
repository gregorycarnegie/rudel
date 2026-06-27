// adsr_golden.rs — audio parity for the linear ADSR gain envelope.
//
// `adsr_golden.json` (from tools/oracle/gen_adsr_oracle.mjs) holds, per case, the
// (attack, decay, sustain, release, duration) and the exact envelope curve the
// real superdough `getParamADSR(param, a, d, s, r, 0, 1, 0, duration, 'linear')`
// schedule produces, sampled at 44.1kHz. Here each curve is rebuilt with rudel's
// `adsr_value` and compared sample-for-sample. The cases cover the common ADSR
// plus the two edge cases superdough handles by cutting the envelope at the note
// duration: attack longer than the note, and attack+decay overrunning the note.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::{Adsr, adsr_value};

const SAMPLE_RATE: f64 = 44100.0;
// rudel evaluates the envelope in f32 (the synth's time/params are all f32),
// while the oracle curve is f64. Most samples agree to <2e-6; the tolerance is
// set by the steep `tiny_release` case (a 1ms release), where the `(t -
// duration)/release` term amplifies f32 rounding ~1000x. An actual algorithmic
// divergence is orders of magnitude larger (e.g. a wrong t=0 value is ~1.0).
const EPS: f64 = 5e-5;

#[test]
fn adsr_value_matches_superdough() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/adsr_golden.json"))
            .expect("parse golden");
    let cases = golden["cases"].as_array().expect("cases array");

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let adsr = Adsr {
            attack: case["attack"].as_f64().unwrap() as f32,
            decay: case["decay"].as_f64().unwrap() as f32,
            sustain: case["sustain"].as_f64().unwrap() as f32,
            release: case["release"].as_f64().unwrap() as f32,
        };
        let duration = case["duration"].as_f64().unwrap() as f32;
        let want: Vec<f64> = case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();

        let mut worst = 0.0_f64;
        let mut at = 0usize;
        for (k, w) in want.iter().enumerate() {
            let t = k as f64 / SAMPLE_RATE;
            let got = adsr_value(&adsr, t as f32, duration) as f64;
            let d = (got - w).abs();
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > EPS {
            let t = at as f64 / SAMPLE_RATE;
            let got = adsr_value(&adsr, t as f32, duration) as f64;
            failures.push(format!(
                "{name}: max diff {worst:.3e} at sample {at} (t={t:.5}s, rudel {got} vs strudel {})",
                want[at]
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "adsr_value mismatches vs superdough getParamADSR:\n{}",
        failures.join("\n")
    );
}
