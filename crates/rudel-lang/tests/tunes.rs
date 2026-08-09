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

use rudel_core::{Frac, Hap, Value};
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

// ---------------------------------------------------------------------------
// Hap parity against Strudel's own snapshot.
// ---------------------------------------------------------------------------

/// Decimals kept when comparing a control value. Both engines compute in `f64`
/// and Strudel's snapshot prints full precision, so the two agree far past this
/// when they agree at all; anything finer is inaudible and is only `f64` noise
/// arriving by a different route. Upstream's own tune test rounds to 12 for the
/// same reason.
const VALUE_PRECISION: usize = 9;

/// Render a number the same way from either side, so a rudel `Int(1)` and a
/// JSON `1` compare equal without `1.0` vs `1` mattering.
fn number(x: f64) -> String {
    let s = format!("{x:.VALUE_PRECISION$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// One control value as a comparable string. Deliberately matches on the
/// variant rather than going through `Value::as_f64`, which parses strings as
/// numbers — a note that arrived as `"A4"` where Strudel emitted `55` is a real
/// difference and has to stay visible.
fn control_value(v: &Value) -> String {
    match v {
        Value::Int(n) => number(*n as f64),
        Value::F64(n) => number(*n),
        Value::Frac(f) => number(f.to_f64()),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn json_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => number(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Resolve a note *name* to its MIDI number, for the `note` control only.
///
/// Strudel carries `note` as whatever the pattern produced and converts it in
/// superdough at playback (`valueToMidi`); Rudel resolves it when the control is
/// set. So `note:A4` and `note:69` are the same event written two ways, and
/// comparing them literally would bury every real difference under a spelling
/// one. Only names that parse are rewritten — anything else stays as it is, so
/// a genuinely wrong value still shows up.
fn normalize_note(key: &str, value: String) -> String {
    if key != "note" {
        return value;
    }
    match rudel_core::note_to_midi(&value) {
        Some(midi) => number(midi as f64),
        None => value,
    }
}

/// A hap as one canonical line: the whole and part spans, then the controls in
/// key order. Both sides render through this, so the comparison is a multiset
/// difference over strings and reports insertions and deletions rather than
/// stopping at the first index that disagrees.
fn line(wb: &str, b: &str, e: &str, we: &str, controls: &BTreeMap<String, String>) -> String {
    let body: Vec<String> = controls.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    format!("[{wb} <= {b} -> {e} => {we} | {}]", body.join(" "))
}

fn rudel_line(hap: &Hap) -> String {
    let whole = hap.whole_or_part();
    let controls = match &hap.value {
        Value::Map(m) => m
            .iter()
            .map(|(k, v)| (k.to_string(), normalize_note(k, control_value(v))))
            .collect(),
        // A bare value has no control name upstream either; `show` prints it
        // alone, and the oracle records that as an unparseable hap, so a tune
        // reaching here will report as a plain difference.
        other => BTreeMap::from([(String::new(), control_value(other))]),
    };
    line(
        &whole.begin.to_string(),
        &hap.part.begin.to_string(),
        &hap.part.end.to_string(),
        &whole.end.to_string(),
        &controls,
    )
}

fn expected_line(hap: &serde_json::Value) -> String {
    let s = |k: &str| hap[k].as_str().unwrap_or_default().to_string();
    let controls = hap["v"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), normalize_note(k, json_value(v))))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    line(&s("wb"), &s("b"), &s("e"), &s("we"), &controls)
}

/// `+n` for haps only Rudel produced, `-n` for haps only Strudel produced.
fn multiset_diff(ours: &[String], theirs: &[String]) -> BTreeMap<String, i64> {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for l in ours {
        *counts.entry(l.clone()).or_default() += 1;
    }
    for l in theirs {
        *counts.entry(l.clone()).or_default() -= 1;
    }
    counts.retain(|_, n| *n != 0);
    counts
}

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
        matched >= 11,
        "only {matched} tune(s) reproduce Strudel's haps exactly"
    );
}
