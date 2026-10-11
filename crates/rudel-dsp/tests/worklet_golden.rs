// worklet_golden.rs — audio parity for the superdough AudioWorklet DSP that
// rudel ports by hand: the Moog `ladder-processor`, the orbit-bus
// `djf-processor`, and the `transient-processor` shaper.
//
// `worklet_golden.json` (from tools/oracle/gen_worklet_oracle.mjs) holds one
// shared mono input buffer plus, per case, the exact output the real worklet
// loop produces at 44.1kHz. Here each is replayed through rudel's port, one
// test per case.
//
// Upstream runs in JS doubles; rudel's DSP is `f32` throughout (like every
// other filter in the engine), so these compare against a tolerance rather than
// bit-for-bit. The ladder is a nonlinear feedback structure, so its tolerance is
// the loosest of the three — it bounds accumulated single-precision drift, not
// a difference in the recurrence.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::{Djf, Ladder, TransientShaper};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

const SAMPLE_RATE: f32 = 44100.0;

fn floats(v: &Value) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect()
}

// nextest starts this binary once per test, and every start runs `main`, so
// it reads only what names the tests; each test parses the case it runs.
fn main() {
    let top: BTreeMap<String, &'static RawValue> =
        serde_json::from_str(include_str!("../../../tools/oracle/worklet_golden.json"))
            .expect("parse golden");
    let input = top["input"];
    let cases: BTreeMap<String, &'static RawValue> =
        serde_json::from_str(top["cases"].get()).expect("cases");
    let count = cases.len();
    let mut trials = vec![Trial::test("the_golden_has_cases", move || {
        if count > 0 {
            Ok(())
        } else {
            Err("golden has no cases".into())
        }
    })];
    trials.extend(cases.into_iter().map(|(label, case)| {
        Trial::test(label, move || {
            let input = floats(&serde_json::from_str(input.get()).expect("input"));
            let case = serde_json::from_str(case.get()).expect("case");
            worklet_port_matches_superdough(&case, &input)
        })
    }));
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn worklet_port_matches_superdough(case: &Value, input: &[f32]) -> Result<(), Failed> {
    let want = floats(&case["samples"]);
    let f = |k: &str| case[k].as_f64().unwrap() as f32;
    let (got, tol): (Vec<f32>, f32) = match case["kind"].as_str().unwrap() {
        "ladder" => {
            let mut l = Ladder::new(SAMPLE_RATE, f("frequency"), f("q"), f("drive"));
            (input.iter().map(|&x| l.process(x)).collect(), 2e-4)
        }
        "djf" => {
            let mut d = Djf::new(SAMPLE_RATE, f("value"));
            (input.iter().map(|&x| d.process(x)).collect(), 5e-5)
        }
        "transient" => {
            let mut t = TransientShaper::new(SAMPLE_RATE, f("attack"), f("sustain"));
            (input.iter().map(|&x| t.process(x)).collect(), 5e-5)
        }
        other => return Err(format!("unknown case kind {other}").into()),
    };
    if got.len() != want.len() {
        return Err(format!("length {} != {}", got.len(), want.len()).into());
    }
    let (i, worst) = got
        .iter()
        .zip(&want)
        .map(|(a, b)| (a - b).abs())
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    // `total_cmp` puts NaN last, so a NaN sample is the worst and fails here.
    if worst.is_nan() || worst > tol {
        return Err(format!(
            "worst deviation {worst:e} at sample {i} (got {}, want {}) exceeds {tol:e}",
            got[i], want[i]
        )
        .into());
    }
    Ok(())
}
