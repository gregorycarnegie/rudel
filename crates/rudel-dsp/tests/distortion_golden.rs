// distortion_golden.rs — audio parity for the waveshaping distortion algorithms.
//
// `distortion_golden.json` (from tools/oracle/gen_distortion_oracle.mjs) holds,
// per algorithm, the exact `shape(x, k)` outputs of superdough's verbatim
// `distortionAlgorithms` over an (x, k) grid (k = e^distort - 1). Here each is
// rebuilt with rudel's `DistortAlgo::shape` and compared sample-for-sample. This
// upgrades the distortion coverage from reference-formula spot checks to a full
// node->Rust golden across all nine algorithms.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::DistortAlgo;
use rudel_core::Value;

// f32 waveshapers vs the f64 oracle. Across the realistic drive range
// (distort <= 2) every algorithm agrees to < 1e-5. The tolerance floor is set by
// `diode`/`asym` at extreme drive (distort = 4, k ~= 53.6), where both saturated
// `tanh` terms approach 1 and the `pos - neg` subtraction loses ~3e-4 to f32
// catastrophic cancellation — a genuine property of the f32 port, not an
// algorithmic difference.
const EPS: f64 = 5e-4;

#[test]
fn distort_algorithms_match_superdough() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("distortion_golden.json")).expect("parse golden");
    let xs: Vec<f64> = golden["xs"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let ks: Vec<f64> = golden["ks"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let cases = golden["cases"].as_array().unwrap();

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let algo = DistortAlgo::from_value(&Value::Str(name.into()));
        let want: Vec<f64> = case["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();

        // samples are flattened in (k outer, x inner) order, matching the oracle.
        let mut idx = 0usize;
        for &k in &ks {
            for &x in &xs {
                let got = algo.shape(x as f32, k as f32) as f64;
                let w = want[idx];
                let d = (got - w).abs();
                if d > EPS {
                    failures.push(format!(
                        "{name}: shape(x={x}, k={k:.4}) = {got} vs strudel {w} (diff {d:.3e})"
                    ));
                }
                idx += 1;
            }
        }
    }

    assert!(
        failures.is_empty(),
        "distortion shape mismatches vs superdough:\n{}",
        failures.join("\n")
    );
}
