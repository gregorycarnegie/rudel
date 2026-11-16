//! The Rust dough engine against the C it was ported from: the same events,
//! rendered in the browser's 128-frame blocks, in ns per frame.
//!
//!   cargo bench -p rudel-dough --features c-reference
//!   cargo bench -p rudel-dough --features c-reference -- phaser   (only those)
//!
//! Each workload runs on both engines back to back, round after round. The
//! ratio is the median of the rounds' own Rust/C ratios: a busy machine slows
//! both halves of a round alike, so a round's ratio holds up where absolute
//! times drift. The best time of each is shown for scale. The first block of
//! each is compared too: a port that is fast because it is wrong would show up
//! here before it reached the timing.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_dough::{Dough, c_reference::CDough};
use std::{hint::black_box, time::Instant};

const RATE: u32 = 48_000;
const BLOCK: usize = 128;
const SECONDS: usize = 4;
const ROUNDS: usize = 9;

/// `n` sustained voices, voice `i` built by `voice(i)`.
fn voices(n: usize, voice: impl Fn(usize) -> String) -> Vec<String> {
    (0..n).map(voice).collect()
}

fn workloads() -> Vec<(&'static str, Vec<String>)> {
    vec![
        (
            "8 sines",
            voices(8, |i| {
                format!("dough/play/s/sine/note/{}/duration/100", 60 + i)
            }),
        ),
        (
            "32 saws, 24 dB low-pass",
            voices(32, |i| {
                format!(
                    "dough/play/s/saw/note/{}/lpf/1200/lpq/0.4/ftype/24db/duration/100",
                    36 + i
                )
            }),
        ),
        (
            "16 FM + vibrato + envelopes",
            voices(16, |i| {
                format!(
                    "dough/play/s/sine/note/{}/fm/2/fmh/1.5/fme/2/fma/0.1/vib/5/vibmod/0.3\
                     /attack/0.05/decay/0.2/sustain/0.6/lpf/3000/lpe/1/lpd/0.3/duration/100",
                    48 + i
                )
            }),
        ),
        (
            "16 saws, phaser + chorus + flanger",
            voices(16, |i| {
                format!(
                    "dough/play/s/saw/note/{}/phaser/0.5/chorus/0.4/flanger/0.3/duration/100",
                    40 + i
                )
            }),
        ),
        (
            "16 saws, phaser",
            voices(16, |i| {
                format!("dough/play/s/saw/note/{}/phaser/0.5/duration/100", 40 + i)
            }),
        ),
        (
            "16 saws, chorus",
            voices(16, |i| {
                format!("dough/play/s/saw/note/{}/chorus/0.4/duration/100", 40 + i)
            }),
        ),
        (
            "16 saws, flanger",
            voices(16, |i| {
                format!("dough/play/s/saw/note/{}/flanger/0.3/duration/100", 40 + i)
            }),
        ),
        (
            "16 pulses into delay + reverb",
            voices(16, |i| {
                format!(
                    "dough/play/s/pulse/note/{}/pw/0.3/delay/0.5/delaytime/0.25\
                     /delayfeedback/0.6/room/0.4/duration/100",
                    44 + i
                )
            }),
        ),
    ]
}

/// Render `SECONDS` of audio block by block; ns per frame, and the first block.
fn time(mut render: impl FnMut(&mut [(f32, f32)])) -> (f64, Vec<(f32, f32)>) {
    let blocks = SECONDS * RATE as usize / BLOCK;
    let mut first = vec![(0.0, 0.0); BLOCK];
    render(&mut first);
    let mut block = vec![(0.0, 0.0); BLOCK];
    let start = Instant::now();
    for _ in 1..blocks {
        block.fill((0.0, 0.0));
        render(&mut block);
        black_box(&block);
    }
    let ns = start.elapsed().as_nanos() as f64 / ((blocks - 1) * BLOCK) as f64;
    (ns, first)
}

fn main() {
    println!("dough engine, {RATE} Hz, {BLOCK}-frame blocks, {ROUNDS} rounds of {SECONDS} s");
    println!(
        "{:38} {:>10} {:>10} {:>8}",
        "workload", "C ns/fr", "Rust ns/fr", "Rust/C"
    );
    let mut worst = 0.0_f64;
    // One engine of each kind, reset between rounds: the C engine keeps its
    // delay and reverb tails across a reset, so the Rust one has to as well
    // for the two to start each round alike.
    let mut rust = Dough::new(RATE);
    // cargo passes `--bench`; anything else names the workloads to run.
    let only: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    for (name, messages) in workloads() {
        if !only.is_empty() && !only.iter().any(|o| name.contains(o.as_str())) {
            continue;
        }
        let (mut best_c, mut best_rust) = (f64::MAX, f64::MAX);
        let mut ratios = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let mut c = CDough::acquire(RATE).expect("the C engine is free");
            for m in &messages {
                assert!(c.evaluate(m), "{m}");
            }
            let (ns_c, first_c) = time(|out| c.render_into(out));
            drop(c);

            assert!(rust.evaluate("dough/reset"));
            for m in &messages {
                assert!(rust.evaluate(m), "{m}");
            }
            let (ns_rust, first_rust) = time(|out| rust.render_into(out));

            assert!(
                first_c == first_rust,
                "{name}: the engines disagree on the first block"
            );
            best_c = best_c.min(ns_c);
            best_rust = best_rust.min(ns_rust);
            ratios.push(ns_rust / ns_c);
        }
        ratios.sort_by(f64::total_cmp);
        let ratio = ratios[ROUNDS / 2];
        worst = worst.max(ratio);
        println!("{name:38} {best_c:>10.1} {best_rust:>10.1} {ratio:>8.3}");
    }
    println!("slowest Rust/C ratio: {worst:.3} (below 1 is faster than the C)");
}
