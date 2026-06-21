//! Additive-wavetable build benchmark (`partials`).
//!
//!   cargo bench -p rudel-dsp --bench additive
//!
//! `build_additive` (oscillator.rs) fills a 2048-sample wavetable by summing
//! `N` harmonics at every sample — an `O(2048 · N)` double loop of `sin`/`cos`
//! that runs once per voice **onset** when a `partials` list is given. It is not
//! a steady-state cost, but a heavy onset spike can starve the audio callback,
//! so it is worth vectorizing. The harmonic sum at each sample is independent
//! across the 2048 table slots, so it maps cleanly onto `f32x8`.
//!
//! `build_additive` is crate-private, so this drives it through its real entry
//! point, `VoiceParams::from_controls` with a `partials` list. The fixed map /
//! param-parse overhead is constant across partial counts, so the trig loop's
//! scaling (and any SIMD speed-up) is still clearly visible.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_core::Value;
use rudel_dsp::VoiceParams;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

/// Partial counts to sweep (non-multiples of 8 included to exercise the loop's
/// tail handling, large counts where the trig sum dominates).
const PARTIALS: &[usize] = &[4, 7, 16, 32, 64, 100, 256];

/// A control map selecting a sawtooth additive base with `n` unit partials.
fn partials_map(n: usize) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    m.insert("s".to_string(), Value::Str("sawtooth".into()));
    m.insert("note".to_string(), Value::Str("c3".into()));
    m.insert(
        "partials".to_string(),
        Value::List(vec![Value::F64(1.0); n]),
    );
    m
}

fn time(label: &str, iters: u32, mut f: impl FnMut() -> usize) {
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
        "{label:<16} {:>10.2} µs/build   (work={sink})",
        per * 1e6,
    );
}

fn main() {
    println!("# additive wavetable build (2048 samples) by partial count");
    for &n in PARTIALS {
        let map = partials_map(n);
        time(&format!("partials={n}"), 4_000, || {
            let p = VoiceParams::from_controls(&map, 1.0);
            // Touch the built table so the work can't be elided.
            black_box(p.additive.as_ref().map(|t| t.len()).unwrap_or(0))
        });
    }
}
