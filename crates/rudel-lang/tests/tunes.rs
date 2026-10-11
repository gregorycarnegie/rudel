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
//! and an allowlist entry that has since started working. Each tune is its own
//! test (`runs/<id>`, `matches/<id>`), so a failure names it.
//!
//! Regenerate the corpus after bumping the vendored Strudel:
//!   cd tools/oracle && node gen_tunes_oracle.mjs

mod common;

use common::{expected_line, multiset_diff, rudel_line};
use libtest_mimic::{Arguments, Failed, Trial};
use rudel_core::Frac;
use rudel_lang::eval;
use serde_json::{Value, value::RawValue};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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

/// An allowlist: tune id to the reason it is on it.
type Allow = Arc<BTreeMap<String, String>>;

fn allowlist(json: &str) -> Allow {
    Arc::new(serde_json::from_str(json).expect("parse allowlist"))
}

/// A golden case with its fields left as unparsed text: nextest starts this
/// binary once per test, so the start only reads what names and lists the
/// tests, and a test parses the fields it uses.
type Raw = BTreeMap<String, &'static RawValue>;

fn text(case: &Raw, key: &str) -> String {
    serde_json::from_str(case[key].get()).unwrap_or_else(|e| panic!("case {key}: {e}"))
}

fn main() {
    let corpus: Raw = serde_json::from_str(include_str!("../../../tools/oracle/tunes_golden.json"))
        .expect("parse tunes_golden.json");
    let cases: Vec<Raw> = serde_json::from_str(corpus["cases"].get()).expect("cases array");
    let cannot_run = allowlist(include_str!("../../../tools/oracle/tunes_allowlist.json"));
    let mismatch = allowlist(include_str!(
        "../../../tools/oracle/tunes_parity_allowlist.json"
    ));

    let summary: Vec<Tune> = cases
        .iter()
        .map(|c| Tune {
            id: text(c, "id"),
            source: text(c, "source"),
            has_haps: c.contains_key("haps"),
        })
        .collect();
    let mut trials = vec![{
        let (cannot_run, mismatch) = (cannot_run.clone(), mismatch.clone());
        Trial::test("the_corpus_is_complete_and_mostly_working", move || {
            the_corpus_is_complete_and_mostly_working(&summary, &cannot_run, &mismatch)
        })
    }];
    for case in cases {
        let id = text(&case, "id");
        let code = case["code"];
        let allowed = cannot_run.get(&id).cloned();
        trials.push(Trial::test(format!("runs/{id}"), move || {
            the_tune_runs_or_is_allowlisted(&text_of(code), allowed.as_deref())
        }));
        // Only the website tunes carry Strudel's haps, and a tune that cannot
        // run is reported by its `runs/` test alone.
        if let Some(&haps) = case.get("haps")
            && !cannot_run.contains_key(&id)
        {
            let allowed = mismatch.get(&id).cloned();
            let cycles = case.get("cycles").copied();
            trials.push(Trial::test(format!("matches/{id}"), move || {
                let cycles = cycles.map_or(1, |c| serde_json::from_str(c.get()).expect("cycles"));
                let haps: Value = serde_json::from_str(haps.get()).expect("haps");
                the_tune_matches_the_haps_strudel_produces(
                    &text_of(code),
                    cycles,
                    &haps,
                    allowed.as_deref(),
                )
            }));
        }
    }
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn text_of(raw: &RawValue) -> String {
    serde_json::from_str(raw.get()).expect("a string")
}

/// What the corpus check needs of a tune.
struct Tune {
    id: String,
    source: String,
    has_haps: bool,
}

/// Whether a tune runs has to agree with its allowlist entry, either way.
fn the_tune_runs_or_is_allowlisted(code: &str, allowed: Option<&str>) -> Result<(), Failed> {
    match (run(code), allowed) {
        (Ok(_), None) | (Err(_), Some(_)) => Ok(()),
        (Err(why), None) => Err(format!(
            "no longer runs and is not allowlisted: {why}\n\
             Fix it, or add it to tools/oracle/tunes_allowlist.json with a reason."
        )
        .into()),
        (Ok(_), Some(_)) => {
            Err("now runs — remove it from tools/oracle/tunes_allowlist.json".into())
        }
    }
}

/// The haps a website tune makes against Strudel's committed snapshot of them,
/// agreeing with its parity allowlist entry either way.
fn the_tune_matches_the_haps_strudel_produces(
    code: &str,
    cycles: i64,
    haps: &Value,
    allowed: Option<&str>,
) -> Result<(), Failed> {
    let pattern = eval(code)?;
    let mut ours: Vec<String> = pattern
        .query_arc(Frac::new(0, 1), Frac::new(cycles, 1))
        .iter()
        .map(rudel_line)
        .collect();
    let mut theirs: Vec<String> = haps
        .as_array()
        .expect("haps array")
        .iter()
        .map(expected_line)
        .collect();
    ours.sort();
    theirs.sort();

    match (ours == theirs, allowed) {
        (true, None) | (false, Some(_)) => return Ok(()),
        (true, Some(_)) => {
            return Err(
                "now matches — remove it from tools/oracle/tunes_parity_allowlist.json".into(),
            );
        }
        (false, None) => {}
    }
    let diff = multiset_diff(&ours, &theirs);
    let extra: i64 = diff.values().filter(|n| **n > 0).sum();
    let missing: i64 = -diff.values().filter(|n| **n < 0).sum::<i64>();
    // Enough of the difference to recognise it, not the whole tune — and from
    // both sides, since one side alone rarely shows what changed.
    let side = |want_ours: bool| {
        diff.iter()
            .filter(move |(_, n)| (**n > 0) == want_ours)
            .take(2)
            .map(move |(l, _)| format!("  {} {l}", if want_ours { "ours   " } else { "strudel" }))
    };
    let sample: Vec<String> = side(true).chain(side(false)).collect();
    Err(format!(
        "{extra} hap(s) ours-only, {missing} strudel-only, over {cycles} cycle(s)\n{}\n\
         Fix it, or add it to tools/oracle/tunes_parity_allowlist.json with a reason.",
        sample.join("\n")
    )
    .into())
}

/// The corpus as a whole. Every tune's test above holds it to its allowlist
/// entry, so how many run and how many match follow from the allowlists' sizes —
/// and the floors here stop a change from conceding the corpus tune by tune.
fn the_corpus_is_complete_and_mostly_working(
    cases: &[Tune],
    cannot_run: &BTreeMap<String, String>,
    mismatch: &BTreeMap<String, String>,
) -> Result<(), Failed> {
    assert!(
        cases.len() > 40,
        "expected the full tune corpus, got {} cases — regenerate it",
        cases.len()
    );
    // Both corpora have to be present, so a regenerate that silently drops one
    // file does not pass as coverage.
    let sources: BTreeSet<&str> = cases.iter().map(|c| c.source.as_str()).collect();
    assert!(
        sources.contains("examples") && sources.contains("testtunes"),
        "tune corpus is missing a source: {sources:?}"
    );
    // Every tune the examples menu offers, so a regenerate cannot quietly
    // shrink the corpus to the handful that pass. A floor, not an equality:
    // upstream adds tunes, and a new one arriving should show up as a failing
    // tune, not as a failing count.
    let examples = cases.iter().filter(|c| c.source == "examples").count();
    assert!(
        examples >= EXAMPLES_ON_THE_SITE,
        "corpus has {examples} of the {EXAMPLES_ON_THE_SITE} tunes on strudel.cc/examples"
    );
    let ids: BTreeSet<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    let listed = |allow: &BTreeMap<String, String>| {
        allow.keys().filter(|id| ids.contains(id.as_str())).count()
    };
    let ran = cases.len() - listed(cannot_run);
    let coverage = ran as f64 / cases.len() as f64;
    assert!(
        coverage > 0.85,
        "only {ran}/{} tunes run ({:.0}%)",
        cases.len(),
        coverage * 100.0
    );
    let with_haps: BTreeSet<&str> = cases
        .iter()
        .filter(|c| c.has_haps)
        .map(|c| c.id.as_str())
        .collect();
    assert!(
        with_haps.len() > 25,
        "expected the website tunes to carry expected haps, got {} — regenerate the corpus",
        with_haps.len()
    );
    // A floor on the real claim, so parity cannot be conceded tune by tune.
    // Raise it as the allowlist shrinks; it is the count that held when the
    // comparison was first turned on.
    let matched = with_haps
        .iter()
        .filter(|id| !cannot_run.contains_key(**id) && !mismatch.contains_key(**id))
        .count();
    assert!(
        matched >= 29,
        "only {matched} tune(s) reproduce Strudel's haps exactly"
    );
    Ok(())
}
