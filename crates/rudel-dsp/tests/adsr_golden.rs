// adsr_golden.rs — audio parity for the linear ADSR gain envelope.
//
// `adsr_golden.json` (from tools/oracle/gen_adsr_oracle.mjs) holds, per case, the
// (attack, decay, sustain, release, duration) and the exact envelope curve the
// real superdough `getParamADSR(param, a, d, s, r, 0, 1, 0, duration, 'linear')`
// schedule produces, sampled at 44.1kHz. Here each curve is rebuilt with rudel's
// `adsr_value` and compared sample-for-sample, one test per case. The cases cover
// the common ADSR plus the two edge cases superdough handles by cutting the
// envelope at the note duration: attack longer than the note, and attack+decay
// overrunning the note.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::{Adsr, adsr_value};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

const SAMPLE_RATE: f64 = 44100.0;
// rudel evaluates the envelope in f32 (the synth's time/params are all f32),
// while the oracle curve is f64. Most samples agree to <2e-6; the tolerance is
// set by the steep `tiny_release` case (a 1ms release), where the `(t -
// duration)/release` term amplifies f32 rounding ~1000x. An actual algorithmic
// divergence is orders of magnitude larger (e.g. a wrong t=0 value is ~1.0).
const EPS: f64 = 5e-5;

// nextest starts this binary once per test, and every start runs `main`, so
// it reads only what names the tests; each test parses the case it runs.
fn main() {
    let top: BTreeMap<String, &'static RawValue> =
        serde_json::from_str(include_str!("../../../tools/oracle/adsr_golden.json"))
            .expect("parse golden");
    let cases: Vec<BTreeMap<String, &'static RawValue>> =
        serde_json::from_str(top["cases"].get()).expect("cases array");
    let trials = cases
        .into_iter()
        .map(|case| {
            let name: String = serde_json::from_str(case["name"].get()).expect("name");
            Trial::test(name, move || adsr_value_matches_superdough(&parse(&case)))
        })
        .collect();
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

/// A case whose fields were left unparsed, parsed.
fn parse(case: &BTreeMap<String, &RawValue>) -> Value {
    Value::Object(
        case.iter()
            .map(|(k, v)| (k.clone(), serde_json::from_str(v.get()).expect("a field")))
            .collect(),
    )
}

fn adsr_value_matches_superdough(case: &Value) -> Result<(), Failed> {
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

    let got = |k: usize| adsr_value(&adsr, (k as f64 / SAMPLE_RATE) as f32, duration) as f64;
    let (at, worst) =
        (0..want.len())
            .map(|k| (k, (got(k) - want[k]).abs()))
            .fold(
                (0, 0.0),
                |m, d| if d.1.is_nan() || d.1 > m.1 { d } else { m },
            );
    // NaN compares false against any bound: it is a failure, not a pass.
    if worst.is_nan() || worst > EPS {
        return Err(format!(
            "max diff {worst:.3e} at sample {at} (t={:.5}s, rudel {} vs strudel {})",
            at as f64 / SAMPLE_RATE,
            got(at),
            want[at]
        )
        .into());
    }
    Ok(())
}
