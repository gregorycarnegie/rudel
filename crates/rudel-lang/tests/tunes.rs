//! Whole-tune corpus: real, complete live-coding scripts run end to end.
//!
//! `doc_examples.rs` covers the documented one-liners. This covers the other
//! end: the songs behind <https://strudel.cc/examples> (`website/src/repl/
//! tunes.mjs`) plus upstream's own community-song fixtures (`test/
//! testtunes.mjs`) — tens of lines each, with comments, `const` bindings,
//! arrow callbacks and multi-statement bodies, i.e. what someone actually
//! pastes into the editor.
//!
//! Two things are pinned here. Every tune must evaluate *and* produce events —
//! a tune that parses into silence is as broken as one that fails to parse —
//! and every website tune must produce the events Strudel produces, compared
//! against upstream's own committed snapshot of them
//! (`test/__snapshots__/tunes.test.mjs.snap`, carried into the corpus by
//! `gen_tunes_oracle.mjs`). Running is the weaker claim: it says a tune makes a
//! sound, not that it makes the right one.
//!
//! Exceptions are named with a reason in `tools/oracle/tunes_allowlist.json`
//! (does not run) and `tools/oracle/tunes_parity_allowlist.json` (runs, but
//! does not yet match). Both fail in *both* directions — a tune that regresses,
//! and an allowlist entry that has since started working.
//!
//! Regenerate the corpus after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_tunes_oracle.mjs

mod common;

use common::{expected_line, multiset_diff, rudel_line};
use rudel_core::Frac;
use rudel_lang::eval;
use std::collections::{BTreeMap, BTreeSet};

/// Cycles queried per tune. Tunes are slow — several of them stretch a phrase
/// over 8 or 16 cycles — so one cycle is not enough to prove a tune sounds.
const CYCLES: i64 = 4;

/// Tunes listed under <https://strudel.cc/examples> at the vendored revision —
/// the menu a user actually picks from, and the set they were checked against by
/// ear.
const EXAMPLES_ON_THE_SITE: usize = 31;

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
    // Every tune the examples menu offers, so a regenerate cannot quietly shrink
    // the corpus to the handful that pass. A floor, not an equality: upstream
    // adds tunes, and a new one arriving should show up as a failing tune, not
    // as a failing count.
    let examples = cases.iter().filter(|c| c["source"] == "examples").count();
    assert!(
        examples >= EXAMPLES_ON_THE_SITE,
        "corpus has {examples} of the {EXAMPLES_ON_THE_SITE} tunes on strudel.cc/examples"
    );

    // A floor, so a change that quietly stops running most of the corpus while
    // dutifully growing the allowlist still fails.
    let coverage = ran as f64 / cases.len() as f64;
    assert!(
        coverage > 0.85,
        "only {ran}/{} tunes run ({:.0}%)",
        cases.len(),
        coverage * 100.0
    );
}

// ---------------------------------------------------------------------------
// Hap parity against Strudel's own snapshot. The comparison itself lives in
// `common`, shared with the stepwise corpus.
// ---------------------------------------------------------------------------

#[test]
fn every_tune_matches_the_haps_strudel_produces() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/tunes_golden.json"))
            .expect("parse tunes_golden.json");
    let cannot_run: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../../tools/oracle/tunes_allowlist.json"))
            .expect("parse tunes_allowlist.json");
    let allow: BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tools/oracle/tunes_parity_allowlist.json"
    ))
    .expect("parse tunes_parity_allowlist.json");

    let cases: Vec<&serde_json::Value> = corpus["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .filter(|c| c.get("haps").is_some())
        .collect();
    assert!(
        cases.len() > 25,
        "expected the website tunes to carry expected haps, got {} — regenerate the corpus",
        cases.len()
    );

    let mut mismatched: BTreeMap<String, String> = BTreeMap::new();
    let mut matched = 0usize;
    for case in cases {
        let id = case["id"].as_str().expect("case id").to_string();
        if cannot_run.contains_key(&id) {
            continue; // already reported by the runs-at-all test
        }
        let cycles = case["cycles"].as_i64().unwrap_or(1);
        let Ok(pattern) = eval(case["code"].as_str().expect("case code")) else {
            continue;
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
            matched += 1;
            continue;
        }
        let diff = multiset_diff(&ours, &theirs);
        let extra: i64 = diff.values().filter(|n| **n > 0).sum();
        let missing: i64 = -diff.values().filter(|n| **n < 0).sum::<i64>();
        // Enough of the difference to recognise it, not the whole tune — and
        // from both sides, since one side alone rarely shows what changed.
        let side = |want_ours: bool| {
            diff.iter()
                .filter(move |(_, n)| (**n > 0) == want_ours)
                .take(2)
                .map(move |(l, _)| {
                    format!(
                        "      {} {l}",
                        if want_ours { "ours   " } else { "strudel" }
                    )
                })
        };
        let sample: Vec<String> = side(true).chain(side(false)).collect();
        mismatched.insert(
            id,
            format!(
                "{} hap(s) ours-only, {missing} strudel-only, over {cycles} cycle(s)\n{}",
                extra,
                sample.join("\n")
            ),
        );
    }

    let failed: BTreeSet<&String> = mismatched.keys().collect();
    let allowed: BTreeSet<&String> = allow.keys().collect();

    let unexpected: Vec<String> = failed
        .difference(&allowed)
        .map(|id| format!("  {id}: {}", mismatched[*id]))
        .collect();
    let stale: Vec<&&String> = allowed.difference(&failed).collect();

    assert!(
        unexpected.is_empty(),
        "{} tune(s) no longer match Strudel's haps and are not allowlisted:\n{}\n\
         Fix them, or add them to tools/oracle/tunes_parity_allowlist.json with a reason.",
        unexpected.len(),
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "{} allowlisted tune(s) now match — remove them from \
         tools/oracle/tunes_parity_allowlist.json:\n{:?}",
        stale.len(),
        stale
    );

    // A floor on the real claim, so parity cannot be conceded tune by tune.
    // Raise it as the allowlist shrinks; it is the count that held when the
    // comparison was first turned on.
    assert!(
        matched >= 28,
        "only {matched} tune(s) reproduce Strudel's haps exactly"
    );
}
