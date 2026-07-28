//! The Strudel documentation example corpus, run against Rudel.
//!
//! Strudel's `test/examples.test.mjs` walks every jsdoc `@example` in
//! `packages/` and snapshots its haps. This is the Rudel counterpart: the same
//! corpus (extracted by `tools/oracle/gen_examples_oracle.mjs`, since `doc.json`
//! is a build artefact) is evaluated and queried here.
//!
//! Rudel is not JavaScript, so a hap-for-hap snapshot is the wrong bar — the
//! `*_parity.rs` oracles already pin behaviour where it can be compared
//! directly. What this pins is *reach*: every documented example either
//! evaluates and queries, or is named in
//! `tools/oracle/examples_allowlist.json` with the reason it cannot. The test
//! fails both ways — an example that regresses, and an allowlist entry that has
//! since started working — so the corpus cannot silently drift.
//!
//! Regenerate the corpus after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_examples_oracle.mjs

use rudel_core::Frac;
use rudel_lang::eval;
use std::collections::{BTreeMap, BTreeSet};

/// Evaluate a snippet and query one cycle, reporting the first failure.
fn run(code: &str) -> Result<usize, String> {
    let pattern = eval(code).map_err(|e| format!("eval: {e}"))?;
    // A panic in the query path is a genuine defect, so it is deliberately not
    // caught here — it fails the test loudly with a backtrace.
    let haps = pattern.query_arc(Frac::new(0, 1), Frac::new(1, 1));
    Ok(haps.len())
}

#[test]
fn every_documented_example_runs_or_is_allowlisted() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/examples_golden.json"))
            .expect("parse examples_golden.json");
    let allow: BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tools/oracle/examples_allowlist.json"
    ))
    .expect("parse examples_allowlist.json");

    let cases = corpus["cases"].as_array().expect("cases array");
    assert!(
        cases.len() > 400,
        "expected the full corpus, got {} cases — regenerate it",
        cases.len()
    );

    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    let mut ran = 0usize;
    for case in cases {
        let id = case["id"].as_str().unwrap().to_string();
        let code = case["code"].as_str().unwrap();
        match run(code) {
            Ok(_) => ran += 1,
            Err(why) => {
                failed.insert(id, why);
            }
        }
    }

    let failed_ids: BTreeSet<&String> = failed.keys().collect();
    let allowed_ids: BTreeSet<&String> = allow.keys().collect();

    let unexpected: Vec<String> = failed_ids
        .difference(&allowed_ids)
        .map(|id| format!("  {id}: {}", failed[*id]))
        .collect();
    let stale: Vec<&&String> = allowed_ids.difference(&failed_ids).collect();

    assert!(
        unexpected.is_empty(),
        "{} documented example(s) no longer run and are not allowlisted:\n{}\n\
         Fix them, or add them to tools/oracle/examples_allowlist.json with a reason.",
        unexpected.len(),
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "{} allowlisted example(s) now run — remove them from \
         tools/oracle/examples_allowlist.json:\n{:?}",
        stale.len(),
        stale
    );

    // A floor, so a change that quietly stops running most of the corpus while
    // dutifully growing the allowlist still fails.
    let coverage = ran as f64 / cases.len() as f64;
    assert!(
        coverage > 0.80,
        "only {ran}/{} documented examples run ({:.0}%)",
        cases.len(),
        coverage * 100.0
    );
}
