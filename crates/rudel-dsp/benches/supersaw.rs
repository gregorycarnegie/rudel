//! Super-saw voice render benchmark.
//!
//!   cargo bench -p rudel-dsp --bench supersaw
//!
//! The super-saw source ([`Voice::next_source`] via `tick`) sums `unison`
//! independent detuned saws every output sample — the one steady-state hot path
//! in the synth where the per-sample work scales with a tunable count, which
//! makes it the prime candidate for SIMD. This measures the cost of rendering a
//! block of frames from a single super-saw voice across a range of unison
//! counts, so the per-voice scaling (and any speed-up from vectorizing the
//! unison loop) is directly visible.
//!
//! Like `rudel-lang`'s `patterns` bench it pulls in no benchmarking crate: a
//! `harness = false` main times with `std::time::Instant` and prints
//! µs/block and ns/frame per case.

use rudel_dsp::{Voice, VoiceParams};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
/// Frames rendered per timed iteration (one ~10ms audio block).
const BLOCK: usize = 512;
/// Unison counts to sweep. Deliberately includes non-multiples of the SIMD
/// width (8) so the scalar-remainder handling is exercised, plus large counts
/// where the unison loop dominates the fixed per-sample overhead.
const UNISONS: &[usize] = &[1, 2, 3, 5, 7, 8, 12, 16, 32, 64];

/// Build a sustained super-saw voice with the given unison count. `duration` is
/// set huge so the voice never enters its release/`done` state during the
/// benchmark (a `done` voice short-circuits `tick` and would skip the work).
fn make_voice(unison: usize) -> Voice {
    let p = VoiceParams {
        supersaw: true,
        unison,
        spread: 0.8,
        detune: 12.0,
        duration: 1.0e9,
        ..VoiceParams::default()
    };
    Voice::new(p, SAMPLE_RATE)
}

/// Render `BLOCK` frames, folding the output into a sink so the work can't be
/// optimized away. Returns the block size (used as the `time` work counter).
fn render_block(voice: &mut Voice) -> usize {
    let mut acc = 0.0f32;
    for _ in 0..BLOCK {
        let (l, r) = voice.tick();
        acc += l + r;
    }
    black_box(acc);
    BLOCK
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
    let per_block = elapsed.as_secs_f64() / f64::from(iters);
    let per_frame_ns = per_block * 1e9 / BLOCK as f64;
    println!(
        "{label:<24} {:>10.2} µs/block {:>9.2} ns/frame   (work={sink})",
        per_block * 1e6,
        per_frame_ns,
    );
}

fn main() {
    println!("# super-saw voice: render {BLOCK}-frame blocks @ {SAMPLE_RATE} Hz");
    for &u in UNISONS {
        let mut voice = make_voice(u);
        time(&format!("unison={u}"), 8_000, || render_block(&mut voice));
    }
}
