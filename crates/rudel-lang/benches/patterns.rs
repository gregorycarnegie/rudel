//! Dependency-free performance benchmarks over representative Strudel patterns.
//!
//!   cargo bench -p rudel-lang
//!
//! Measures the two hot paths of the live-coding loop separately:
//!   * **eval** — preprocess + Koto compile/run + mini-notation parse to build a
//!     `Pattern` (what runs on every keystroke-eval), and
//!   * **query** — `query_arc` over a window of cycles (what the scheduler runs
//!     ~100ms ahead on every audio window).
//!
//! Uses a `harness = false` main with `std::time::Instant` so it pulls in no
//! benchmarking dependency; it prints µs/iter and iters/s per case. The patterns
//! mirror Strudel's own `packages/core/bench` (a 64-step sequence, an 8-way
//! stack, a random signal sampled at high resolution) plus a few realistic
//! live-coding patterns (drums with Euclid, a melodic line with higher-order
//! transforms, a tonal scale run).

use rudel_core::{Frac, Pattern};
use rudel_lang::eval;
use std::time::Instant;

/// Representative patterns: (label, source). Each must evaluate.
const PATTERNS: &[(&str, &str)] = &[
    ("seq64", r#"n("0 .. 63")"#),
    ("seq64.iter.fast", r#"n("0 .. 63").iter(64).fast(64)"#),
    (
        "stack8",
        r#"stack(s("bd*4"), s("hh*8"), s("~ cp"), note("c e g").slow(2), n("0 2 4 7"), s("rim*3"), s("oh*2"), s("mt lt ht"))"#,
    ),
    ("rand.segment128", r#"rand.segment(128)"#),
    ("drums.euclid", r#"s("bd(3,8) sd(5,8,2) hh*8").gain("0.8 0.6")"#),
    (
        "melody.hof",
        r#"note("c e g b").fast(2).every(3, |x| x.rev()).add(note("<0 12>")).lpf(800)"#,
    ),
    ("scale.run", r#"n("0 .. 7").scale("c:major").s("piano")"#),
];

/// Cycles to query in the query benchmark (a generous scheduler window).
const QUERY_CYCLES: i64 = 16;

fn time<F: FnMut() -> usize>(label: &str, iters: u32, mut f: F) {
    // Warm up (and sanity-check the closure produces work).
    let mut sink = 0usize;
    for _ in 0..(iters / 10).max(1) {
        sink = sink.wrapping_add(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        sink = sink.wrapping_add(f());
    }
    let elapsed = start.elapsed();
    let per = elapsed.as_secs_f64() / f64::from(iters);
    println!(
        "{label:<28} {:>10.2} µs/iter {:>12.0} iter/s   (work={sink})",
        per * 1e6,
        1.0 / per,
    );
}

fn main() {
    println!("# eval (preprocess + Koto + mini -> Pattern)");
    for (label, src) in PATTERNS {
        // Confirm it evaluates before timing, with a clear message if not.
        eval(src).unwrap_or_else(|e| panic!("bench pattern {label:?} failed to eval: {e}"));
        time(label, 2_000, || eval(src).map(|_| 1).unwrap_or(0));
    }

    println!("\n# query_arc over {QUERY_CYCLES} cycles");
    for (label, src) in PATTERNS {
        let pat: Pattern = eval(src).unwrap();
        let end = Frac::new(QUERY_CYCLES, 1);
        time(label, 5_000, || pat.query_arc(Frac::zero(), end).len());
    }
}
