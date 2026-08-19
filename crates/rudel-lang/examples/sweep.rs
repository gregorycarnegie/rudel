//! Run a directory of patterns through Rudel, and cross-tabulate the result
//! against the same directory run through real Strudel.
//!
//! The Strudel side is `tools/oracle/strudel_diff.test.mjs`, which writes the
//! same `<OK|EMPTY|ERR|PANIC>\t<id>\t<haps|message>` shape. Pairing the two is
//! the only way to read a pass rate over patterns people actually wrote: a
//! large share of them do not work in Strudel either, and without the other
//! side you cannot tell those from a Rudel gap.
//!
//! ```text
//! cargo run -q --release -p rudel-lang --example sweep -- CORPUS [options]
//!
//!   --cycles N        query length, default 8
//!   --out FILE        write this side's results (default: sweep.tsv)
//!   --strudel FILE    the other side's results; prints the square and the gaps
//!   --gaps FILE       write the gap ids, one per line
//!   --errors          print each gap's full error, for triage
//! ```
//! SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::Frac;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};

/// What happened to one pattern.
struct Outcome {
    id: String,
    /// `OK`, `EMPTY`, `ERR` or `PANIC`.
    verdict: &'static str,
    /// Hap count, or the error's first line.
    detail: String,
    /// The whole error, which the buckets are too short to triage from.
    full: String,
    /// Of the file's contents, so patterns duplicated across the corpus (the
    /// same tune re-shared a dozen times) count once as a source.
    source: u64,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut corpus = PathBuf::new();
    let (mut cycles, mut out) = (8i64, PathBuf::from("sweep.tsv"));
    let (mut strudel, mut gaps_out) = (None, None);
    let mut errors = false;
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("missing value");
        match arg.as_str() {
            "--cycles" => cycles = value().parse().expect("--cycles wants a number"),
            "--out" => out = value().into(),
            "--strudel" => strudel = Some(PathBuf::from(value())),
            "--gaps" => gaps_out = Some(PathBuf::from(value())),
            "--errors" => errors = true,
            _ => corpus = arg.into(),
        }
    }
    assert!(corpus.is_dir(), "usage: sweep CORPUS [options]");

    let outcomes = run(&corpus, cycles);
    let lines: Vec<String> = outcomes
        .iter()
        .map(|o| format!("{}\t{}\t{}", o.verdict, o.id, o.detail))
        .collect();
    std::fs::write(&out, lines.join("\n") + "\n").expect("write results");
    let count = |verdict| outcomes.iter().filter(|o| o.verdict == verdict).count();
    println!(
        "{} files: {} ok, {} empty, {} error, {} PANIC  -> {}",
        outcomes.len(),
        count("OK"),
        count("EMPTY"),
        count("ERR"),
        count("PANIC"),
        out.display()
    );

    let Some(strudel) = strudel else { return };
    let gaps = square(&outcomes, &strudel);
    if let Some(path) = gaps_out {
        let ids: Vec<&str> = gaps.iter().map(|o| o.id.as_str()).collect();
        std::fs::write(&path, ids.join("\n") + "\n").expect("write gaps");
        println!("\nwrote {} gap ids to {}", gaps.len(), path.display());
    }
    if errors {
        println!();
        for gap in &gaps {
            println!("=== {}\n{}", gap.id, gap.full);
        }
    }
}

/// Evaluate and query every `.js` file in `corpus`, catching panics — those are
/// the ones worth knowing about, so they must not stop the run.
fn run(corpus: &std::path::Path, cycles: i64) -> Vec<Outcome> {
    rudel_lang::install_mini();
    std::panic::set_hook(Box::new(|_| {})); // panics are the data, not noise
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus)
        .expect("read corpus")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "js").then_some(path)
        })
        .collect();
    files.sort();

    files
        .iter()
        .map(|path| {
            let id = path.file_stem().unwrap_or_default().to_string_lossy();
            let src = std::fs::read_to_string(path).unwrap_or_default();
            let mut hasher = DefaultHasher::new();
            src.hash(&mut hasher);
            let (verdict, detail, full) = match catch_unwind(AssertUnwindSafe(|| {
                let result = rudel_lang::eval_result(&src)?;
                Ok::<usize, String>(
                    result
                        .pattern
                        .query_arc(Frac::zero(), Frac::int(cycles))
                        .len(),
                )
            })) {
                Ok(Ok(0)) => ("EMPTY", String::new(), String::new()),
                Ok(Ok(haps)) => ("OK", haps.to_string(), String::new()),
                Ok(Err(e)) => ("ERR", first_line(&e), e),
                Err(panic) => {
                    let message = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "<non-string panic>".into());
                    ("PANIC", first_line(&message), message)
                }
            };
            Outcome {
                id: id.to_string(),
                verdict,
                detail,
                full,
                source: hasher.finish(),
            }
        })
        .collect()
}

fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or("").trim().to_string()
}

/// Print the 2×2 and the ranked causes, and return the patterns Strudel runs
/// and Rudel does not — the only quadrant that is ours to fix.
fn square<'a>(outcomes: &'a [Outcome], strudel: &std::path::Path) -> Vec<&'a Outcome> {
    let other = std::fs::read_to_string(strudel).expect("read the strudel side");
    let theirs: HashMap<&str, bool> = other
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let verdict = fields.next()?;
            Some((fields.next()?, matches!(verdict, "OK" | "EMPTY")))
        })
        .collect();

    let mut table = [[0usize; 2]; 2];
    let mut gaps = Vec::new();
    for outcome in outcomes {
        let Some(&works_there) = theirs.get(outcome.id.as_str()) else {
            continue; // not in the other run: a shard, or a file added since
        };
        let works_here = matches!(outcome.verdict, "OK" | "EMPTY");
        table[usize::from(!works_there)][usize::from(!works_here)] += 1;
        if works_there && !works_here {
            gaps.push(outcome);
        }
    }
    println!("\n{:18}{:>13}{:>13}", "", "rudel works", "rudel fails");
    for (row, name) in ["works", "fails"].iter().enumerate() {
        println!(
            "strudel {name:10}{:>13}{:>13}",
            table[row][0], table[row][1]
        );
    }

    // Weighted by distinct sources as well as by pattern: one tune re-shared
    // fifty times is one thing to fix, not fifty.
    let mut causes: HashMap<&str, (usize, Vec<u64>)> = HashMap::new();
    for gap in &gaps {
        let entry = causes.entry(gap.detail.as_str()).or_default();
        entry.0 += 1;
        entry.1.push(gap.source);
    }
    let mut ranked: Vec<_> = causes
        .into_iter()
        .map(|(cause, (count, mut sources))| {
            sources.sort_unstable();
            sources.dedup();
            (count, sources.len(), cause)
        })
        .collect();
    ranked.sort_unstable_by(|a, b| b.cmp(a));
    println!("\nrudel-only failures: {}", gaps.len());
    println!("{:>5} {:>5}  cause", "pats", "srcs");
    for (count, sources, cause) in ranked.iter().take(20) {
        println!("{count:>5} {sources:>5}  {}", &cause[..cause.len().min(72)]);
    }
    gaps
}
