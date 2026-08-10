//! The stepwise surface, hap for hap against the real Strudel engine.
//!
//! <https://strudel.cc/learn/stepwise/> documents functions that work in
//! *steps* rather than cycles, and a step count is metadata: getting it wrong
//! is silent. The page says as much — `expand(2)` and `expand(4)` "sound
//! exactly the same" on their own — so an example can evaluate, query, and be
//! wrong, until a `stepcat` or a `pace` reads the count back out and the
//! pattern collapses to nothing.
//!
//! That is why `doc_examples.rs` does not cover this. It pins *reach*: every
//! documented example evaluates and queries. All 25 stepwise examples passed
//! that bar while producing zero haps. What this pins is the events themselves,
//! against `tools/oracle/stepwise_golden.json` — the page's own examples plus
//! every `@example` of the functions it documents, each evaluated by Strudel and
//! by Rudel from *the same source string*.
//!
//! Regenerate after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_stepwise_oracle.mjs

mod common;

use common::{expected_line, multiset_diff, rudel_line};
use rudel_core::Frac;
use rudel_lang::eval;
use std::collections::BTreeMap;

#[test]
fn every_stepwise_example_matches_the_haps_strudel_produces() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/stepwise_golden.json"))
            .expect("parse stepwise_golden.json");

    let cycles = corpus["cycles"].as_i64().expect("cycles");
    let cases = corpus["cases"].as_array().expect("cases array");
    assert!(
        cases.len() >= 40,
        "expected the full stepwise corpus, got {} cases — regenerate it",
        cases.len()
    );

    let mut mismatched: BTreeMap<String, String> = BTreeMap::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id").to_string();
        let code = case["code"].as_str().expect("case code");

        let pattern = match eval(code) {
            Ok(pattern) => pattern,
            Err(why) => {
                mismatched.insert(id, format!("eval: {why}"));
                continue;
            }
        };

        let mut ours: Vec<String> = pattern
            .query_arc(Frac::new(0, 1), Frac::new(cycles, 1))
            .iter()
            .map(rudel_line)
            .collect();
        let mut theirs: Vec<String> = case["haps"]
            .as_array()
            .expect("haps array")
            .iter()
            .map(expected_line)
            .collect();
        ours.sort();
        theirs.sort();
        if ours == theirs {
            continue;
        }

        let diff = multiset_diff(&ours, &theirs);
        let detail: Vec<String> = diff
            .iter()
            .take(6)
            .map(|(line, n)| format!("      {:+} {line}", n))
            .collect();
        mismatched.insert(
            id,
            format!(
                "{} hap(s) differ (ours {}, theirs {})\n    {code}\n{}",
                diff.values().map(|n| n.unsigned_abs()).sum::<u64>(),
                ours.len(),
                theirs.len(),
                detail.join("\n")
            ),
        );
    }

    assert!(
        mismatched.is_empty(),
        "{} stepwise example(s) do not match Strudel:\n{}",
        mismatched.len(),
        mismatched
            .iter()
            .map(|(id, why)| format!("  {id}: {why}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
