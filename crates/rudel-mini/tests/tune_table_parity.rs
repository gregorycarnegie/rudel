// tune_table_parity.rs — exhaustive parity of rudel's generated tune.js scale
// table against the real tune.js runtime.
//
// `tune_table_golden.json` (from tools/oracle/gen_tune_table_oracle.mjs) holds,
// for every scale in tune.js's archive, the per-degree ratios `tune.note(0..N)`
// with tonic 1 (degrees 0..length, the last being the octave). Here `tune(name)`
// is rebuilt with rudel-core for every scale and compared within tolerance,
// verifying both the generated `tune_table.rs` data and rudel's ratio derivation
// against tune.js for the whole archive, one test per scale.
// SPDX-License-Identifier: AGPL-3.0-or-later

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_core::{Frac, Pattern, Value, i, pure, sequence};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

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

fn main() {
    // Every test is its own process under nextest, and each one starts here.
    // Borrowing each scale's ratios as unparsed text keeps that to one scan of
    // the file; only the scale a process runs is parsed into numbers.
    // Names are owned: one (`scalamakesrc2\semisixths_8`) has an escape in it.
    let golden: BTreeMap<String, &'static RawValue> =
        serde_json::from_str(include_str!("../../../tools/oracle/tune_table_golden.json"))
            .expect("parse golden json");
    let count = golden.len();
    // The archive is large, so a typo in enumeration can't pass vacuously.
    let mut trials = vec![Trial::test("the_archive_is_complete", move || {
        assert!(count > 3000, "only {count} scales in the golden");
        Ok(())
    })];
    trials.extend(golden.into_iter().map(|(name, ratios)| {
        Trial::test(name.clone(), move || scale_matches_tunejs(&name, ratios))
    }));
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn scale_matches_tunejs(name: &str, ratios: &RawValue) -> Result<(), Failed> {
    let want: Vec<f64> = serde_json::from_str(ratios.get()).expect("ratio array");
    // golden has degrees 0..=length; the scale length is want.len() - 1.
    let got = rudel_ratios(name, want.len().saturating_sub(1));
    let same = got.len() == want.len() && got.iter().zip(&want).all(|(g, w)| (g - w).abs() <= EPS);
    if same {
        return Ok(());
    }
    Err(format!(
        "tunejs: {want:?}
rudel:  {got:?}"
    )
    .into())
}
