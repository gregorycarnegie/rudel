//! Whole-tune corpus: real, complete live-coding scripts run end to end.
//!
//! `doc_examples.rs` covers the documented one-liners. This covers the other
//! end: the songs behind <https://strudel.cc/examples> (`website/src/repl/
//! tunes.mjs`) plus upstream's own community-song fixtures (`test/
//! testtunes.mjs`) — tens of lines each, with comments, `const` bindings,
//! arrow callbacks and multi-statement bodies, i.e. what someone actually
//! pastes into the editor.
//!
//! Each tune must evaluate *and* produce events; a tune that parses into
//! silence is as broken as one that fails to parse. Anything Rudel cannot run
//! is named with a reason in `tools/oracle/tunes_allowlist.json`, and the test
//! fails both ways — a tune that regresses, and an allowlist entry that has
//! since started working.
//!
//! Regenerate the corpus after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_tunes_oracle.mjs

use rudel_core::Frac;
use rudel_lang::eval;
use std::collections::{BTreeMap, BTreeSet};

/// Cycles queried per tune. Tunes are slow — several of them stretch a phrase
/// over 8 or 16 cycles — so one cycle is not enough to prove a tune sounds.
const CYCLES: i64 = 4;

/// Evaluate a tune and query `CYCLES` cycles, returning the hap count.
fn run(code: &str) -> Result<usize, String> {
    let pattern = eval(code).map_err(|e| format!("eval: {e}"))?;
    // A panic in the query path is a genuine defect, so it is deliberately not
    // caught here — it fails the test loudly with a backtrace.
    let haps = pattern.query_arc(Frac::new(0, 1), Frac::new(CYCLES, 1));
    if haps.is_empty() {
        return Err(format!(
            "evaluated but produced no haps over {CYCLES} cycles"
        ));
    }
    Ok(haps.len())
}

#[test]
fn every_tune_runs_or_is_allowlisted() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/tunes_golden.json"))
            .expect("parse tunes_golden.json");
    let allow: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../../tools/oracle/tunes_allowlist.json"))
            .expect("parse tunes_allowlist.json");

    let cases = corpus["cases"].as_array().expect("cases array");
    assert!(
        cases.len() > 40,
        "expected the full tune corpus, got {} cases — regenerate it",
        cases.len()
    );

    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    let mut sources = BTreeSet::new();
    let mut ran = 0usize;
    for case in cases {
        let id = case["id"].as_str().expect("case id").to_string();
        let code = case["code"].as_str().expect("case code");
        sources.insert(case["source"].as_str().expect("case source").to_string());
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
        "{} tune(s) no longer run and are not allowlisted:\n{}\n\
         Fix them, or add them to tools/oracle/tunes_allowlist.json with a reason.",
        unexpected.len(),
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "{} allowlisted tune(s) now run — remove them from \
         tools/oracle/tunes_allowlist.json:\n{:?}",
        stale.len(),
        stale
    );

    // Both corpora have to be present, so a regenerate that silently drops one
    // file does not pass as coverage.
    assert!(
        sources.contains("examples") && sources.contains("testtunes"),
        "tune corpus is missing a source: {sources:?}"
    );

    // A floor, so a change that quietly stops running most of the corpus while
    // dutifully growing the allowlist still fails.
    let coverage = ran as f64 / cases.len() as f64;
    assert!(
        coverage > 0.75,
        "only {ran}/{} tunes run ({:.0}%)",
        cases.len(),
        coverage * 100.0
    );
}
