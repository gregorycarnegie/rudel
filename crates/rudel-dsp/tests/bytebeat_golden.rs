// bytebeat_golden.rs — parity for the bytebeat expression evaluator against V8.
//
// `bytebeat_golden.json` (from tools/oracle/gen_bytebeat_oracle.mjs) holds, per
// expression, the values real JavaScript produces for a spread of `t` — the
// exact thing superdough's `byte-beat-processor` gets from its `new Function`
// compiled beat. Rudel evaluates the same expressions with its own parser, so
// this is what keeps the two languages agreeing. One test per expression.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_dsp::ByteBeatExpr;
use serde_json::Value;
use std::sync::Arc;

/// f64 arithmetic is identical either side, so the raw values should agree to
/// rounding; the byte the worklet plays must agree exactly.
const EPS: f64 = 1e-9;

fn main() {
    let golden: Value =
        serde_json::from_str(include_str!("../../../tools/oracle/bytebeat_golden.json"))
            .expect("parse golden");
    let ts: Arc<Vec<f64>> = Arc::new(
        golden["ts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect(),
    );
    let cases = golden["cases"].as_object().expect("cases object");
    let count = cases.len();
    let mut trials = vec![Trial::test(
        "the_golden_holds_the_full_case_set",
        move || {
            if count > 30 {
                Ok(())
            } else {
                Err(format!("expected the full case set, got {count}").into())
            }
        },
    )];
    trials.extend(cases.iter().map(|(label, entry)| {
        let (entry, ts) = (entry.clone(), ts.clone());
        Trial::test(label, move || expression_matches_javascript(&entry, &ts))
    }));
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn expression_matches_javascript(entry: &Value, ts: &[f64]) -> Result<(), Failed> {
    let src = entry["src"].as_str().unwrap();
    let expr = ByteBeatExpr::parse(src);
    let values = entry["values"].as_array().unwrap();
    let bytes = entry["bytes"].as_array().unwrap();

    let mut failures = Vec::new();
    for (i, &t) in ts.iter().enumerate() {
        let got = expr.eval(t);

        // The byte is what actually becomes audio; it must match exactly.
        let want_byte = bytes[i].as_i64().unwrap();
        let got_byte = i64::from(to_int32(got) & 255);
        if got_byte != want_byte {
            failures.push(format!("{src} at t={t}: byte {got_byte} != {want_byte}"));
            continue;
        }

        // `null` marks a NaN/Infinity result, which the byte check above
        // already pinned (both coerce to 0).
        let Some(want) = values[i].as_f64() else {
            continue;
        };
        let tol = EPS * want.abs().max(1.0);
        if got.is_nan() || (got - want).abs() > tol {
            failures.push(format!("{src} at t={t}: value {got} != {want}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

/// JS `ToInt32`, mirroring the private helper in `bytebeat.rs`.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4294967296.0) as u32) as i32
}
