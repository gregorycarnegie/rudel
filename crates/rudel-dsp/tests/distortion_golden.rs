// distortion_golden.rs — audio parity for the waveshaping distortion algorithms.
//
// `distortion_golden.json` (from tools/oracle/gen_distortion_oracle.mjs) holds,
// per algorithm, the exact `shape(x, k)` outputs of superdough's verbatim
// `distortionAlgorithms` over an (x, k) grid (k = e^distort - 1). Here each is
// rebuilt with rudel's `DistortAlgo::shape` and compared sample-for-sample, one
// test per algorithm. This upgrades the distortion coverage from
// reference-formula spot checks to a full node->Rust golden across all nine
// algorithms.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::DistortAlgo;
use serde_json::Value;
use std::sync::Arc;

// f32 waveshapers vs the f64 oracle. Across the realistic drive range
// (distort <= 2) every algorithm agrees to < 1e-5. The tolerance floor is set by
// `diode`/`asym` at extreme drive (distort = 4, k ~= 53.6), where both saturated
// `tanh` terms approach 1 and the `pos - neg` subtraction loses ~3e-4 to f32
// catastrophic cancellation — a genuine property of the f32 port, not an
// algorithmic difference.
const EPS: f64 = 5e-4;

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn main() {
    let golden: Value =
        serde_json::from_str(include_str!("../../../tools/oracle/distortion_golden.json"))
            .expect("parse golden");
    let grid = Arc::new((floats(&golden["xs"]), floats(&golden["ks"])));
    let trials = golden["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            let name = case["name"].as_str().unwrap().to_string();
            let (want, grid) = (floats(&case["samples"]), grid.clone());
            let label = name.clone();
            Trial::test(label, move || {
                algorithm_matches_superdough(&name, &want, &grid)
            })
        })
        .collect();
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn algorithm_matches_superdough(
    name: &str,
    want: &[f64],
    (xs, ks): &(Vec<f64>, Vec<f64>),
) -> Result<(), Failed> {
    let algo = DistortAlgo::from_value(&rudel_core::Value::Str(name.into()));
    // samples are flattened in (k outer, x inner) order, matching the oracle.
    let mut failures = Vec::new();
    let mut idx = 0usize;
    for &k in ks {
        for &x in xs {
            let got = algo.shape(x as f32, k as f32) as f64;
            let w = want[idx];
            let d = (got - w).abs();
            if d.is_nan() || d > EPS {
                failures.push(format!(
                    "shape(x={x}, k={k:.4}) = {got} vs strudel {w} (diff {d:.3e})"
                ));
            }
            idx += 1;
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}
