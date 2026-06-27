//! Vowel formant-bank benchmark.
//!
//!   cargo bench -p rudel-dsp --bench vowel
//!
//! The `vowel` post-effect (postfx.rs `Formant`) runs five parallel band-pass
//! biquads over every sample, per channel, for the life of the voice. Unlike a
//! time-recursive filter the five formants are *independent* and share the same
//! input, so they vectorize across filters (not time): one 8-lane SIMD biquad
//! advances all five at once. This measures a voice whose only active post-effect
//! is `vowel`, so the formant bank dominates the per-sample cost.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_dsp::{PostFx, PostFxVoice, VoiceLike, Vowel};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK: usize = 512;

/// A trivial always-on stereo source, so the benchmark measures the formant
/// bank rather than a real synth voice.
struct ConstVoice;
impl VoiceLike for ConstVoice {
    fn tick(&mut self) -> (f32, f32) {
        (0.25, 0.25)
    }
    fn is_done(&self) -> bool {
        false
    }
    fn room(&self) -> f32 {
        0.0
    }
    fn delay_send(&self) -> f32 {
        0.0
    }
}

fn make_voice() -> PostFxVoice {
    let fx = PostFx {
        vowel: Some(Vowel::A),
        ..Default::default()
    };
    PostFxVoice::new(Box::new(ConstVoice), fx, SAMPLE_RATE)
}

fn render_block(voice: &mut PostFxVoice) -> usize {
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
    println!(
        "{label:<20} {:>10.2} µs/block {:>9.2} ns/frame   (work={sink})",
        per_block * 1e6,
        per_block * 1e9 / BLOCK as f64,
    );
}

fn main() {
    println!("# vowel formant bank: render {BLOCK}-frame blocks @ {SAMPLE_RATE} Hz");
    let mut voice = make_voice();
    time("vowel=a (stereo)", 8_000, || render_block(&mut voice));
}
