// tune_table_parity.rs — exhaustive parity of rudel's generated tune.js scale
// table against the real tune.js runtime.
//
// `tune_table_golden.json` (from tools/oracle/gen_tune_table_oracle.mjs) holds,
// for every scale in tune.js's archive, the per-degree ratios `tune.note(0..N)`
// with tonic 1 (degrees 0..length, the last being the octave). Here `tune(name)`
// is rebuilt with rudel-core for every scale and compared within tolerance,
// verifying both the generated `tune_table.rs` data and rudel's ratio derivation
// against tune.js for the whole archive.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Frac, Pattern, Value, i, pure, sequence};

const EPS: f64 = 1e-6;

/// `i("0 1 .. n").tune(name)` — the ratios rudel produces for a scale's degrees.
fn rudel_ratios(name: &str, len: usize) -> Vec<f64> {
    let degrees: Vec<Pattern> = (0..=len as i64).map(|d| pure(Value::Int(d))).collect();
    let pat = i(sequence(&degrees)).tune(Value::Str(name.to_string()));
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter()
        .map(|h| h.value.as_f64().unwrap_or(f64::NAN))
        .collect()
}

#[test]
fn tune_table_matches_tunejs_archive() {
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../../../tools/oracle/tune_table_golden.json"))
            .expect("parse golden json");

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for (name, ratios_json) in &golden {
        let want: Vec<f64> = ratios_json
            .as_array()
            .expect("ratio array")
            .iter()
            .map(|v| v.as_f64().expect("ratio number"))
            .collect();
        // golden has degrees 0..=length; the scale length is want.len() - 1.
        let len = want.len().saturating_sub(1);
        let got = rudel_ratios(name, len);
        let mut ok = got.len() == want.len();
        if ok {
            for (g, w) in got.iter().zip(&want) {
                if (g - w).abs() > EPS {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            checked += 1;
        } else if failures.len() < 20 {
            failures.push(format!(
                "scale {name:?}\n  tunejs: {want:?}\n  rudel:  {got:?}"
            ));
        } else {
            failures.push(format!("scale {name:?} (mismatch)"));
        }
    }

    assert!(
        failures.is_empty(),
        "{} / {} tune.js scales matched; {} mismatches:\n{}",
        checked,
        golden.len(),
        golden.len() - checked,
        failures.join("\n\n")
    );
    // Sanity: the archive is large, so a typo in enumeration can't pass vacuously.
    assert!(checked > 3000, "only {checked} scales checked");
}
