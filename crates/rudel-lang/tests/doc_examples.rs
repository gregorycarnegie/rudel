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
//! since started working — so the corpus cannot silently drift. Each example is
//! its own test, named by its id.
//!
//! Regenerate the corpus after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_examples_oracle.mjs

use libtest_mimic::{Arguments, Failed, Trial};
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

fn main() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/oracle/examples_golden.json"))
            .expect("parse examples_golden.json");
    let allow: BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tools/oracle/examples_allowlist.json"
    ))
    .expect("parse examples_allowlist.json");
    let cases = corpus["cases"].as_array().expect("cases array").clone();

    let ids: BTreeSet<String> = cases
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    let listed = allow.keys().filter(|id| ids.contains(*id)).count();
    let mut trials = vec![Trial::test(
        "the_corpus_is_complete_and_mostly_runs",
        move || {
            assert!(
                ids.len() > 400,
                "expected the full corpus, got {} cases — regenerate it",
                ids.len()
            );
            // Each example's test holds it to its allowlist entry either way, so
            // how many run follows from the allowlist's size. A floor, so a change
            // that quietly stops running most of the corpus while dutifully growing
            // the allowlist still fails.
            let ran = ids.len() - listed;
            let coverage = ran as f64 / ids.len() as f64;
            assert!(
                coverage > 0.80,
                "only {ran}/{} documented examples run ({:.0}%)",
                ids.len(),
                coverage * 100.0
            );
            Ok(())
        },
    )];
    trials.extend(cases.into_iter().map(|case| {
        let id = case["id"].as_str().unwrap().to_string();
        let code = case["code"].as_str().unwrap().to_string();
        let allowed = allow.get(&id).cloned();
        Trial::test(id, move || {
            the_example_runs_or_is_allowlisted(&code, allowed.as_deref())
        })
    }));
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

/// Whether an example runs has to agree with its allowlist entry, either way.
fn the_example_runs_or_is_allowlisted(code: &str, allowed: Option<&str>) -> Result<(), Failed> {
    match (run(code), allowed) {
        (Ok(_), None) | (Err(_), Some(_)) => Ok(()),
        (Err(why), None) => Err(format!(
            "no longer runs and is not allowlisted: {why}\n\
             Fix it, or add it to tools/oracle/examples_allowlist.json with a reason."
        )
        .into()),
        (Ok(_), Some(_)) => {
            Err("now runs — remove it from tools/oracle/examples_allowlist.json".into())
        }
    }
}
