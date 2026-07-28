// bytebeat_golden.rs — parity for the bytebeat expression evaluator against V8.
//
// `bytebeat_golden.json` (from tools/oracle/gen_bytebeat_oracle.mjs) holds, per
// expression, the values real JavaScript produces for a spread of `t` — the
// exact thing superdough's `byte-beat-processor` gets from its `new Function`
// compiled beat. Rudel evaluates the same expressions with its own parser, so
// this is what keeps the two languages agreeing.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::ByteBeatExpr;

/// f64 arithmetic is identical either side, so the raw values should agree to
/// rounding; the byte the worklet plays must agree exactly.
const EPS: f64 = 1e-9;

#[test]
fn bytebeat_expressions_match_javascript() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/bytebeat_golden.json"))
            .expect("parse golden");

    let ts: Vec<f64> = golden["ts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();

    let cases = golden["cases"].as_object().expect("cases object");
    assert!(
        cases.len() > 30,
        "expected the full case set, got {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for (label, entry) in cases {
        let src = entry["src"].as_str().unwrap();
        let expr = ByteBeatExpr::parse(src);
        let values = entry["values"].as_array().unwrap();
        let bytes = entry["bytes"].as_array().unwrap();

        for (i, &t) in ts.iter().enumerate() {
            let got = expr.eval(t);

            // The byte is what actually becomes audio; it must match exactly.
            let want_byte = bytes[i].as_i64().unwrap();
            let got_byte = i64::from(to_int32(got) & 255);
            if got_byte != want_byte {
                failures.push(format!(
                    "{label} ({src}) at t={t}: byte {got_byte} != {want_byte}"
                ));
                continue;
            }

            // `null` marks a NaN/Infinity result, which the byte check above
            // already pinned (both coerce to 0).
            let Some(want) = values[i].as_f64() else {
                continue;
            };
            let tol = EPS * want.abs().max(1.0);
            if (got - want).abs() > tol {
                failures.push(format!("{label} ({src}) at t={t}: value {got} != {want}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// JS `ToInt32`, mirroring the private helper in `bytebeat.rs`.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4294967296.0) as u32) as i32
}
