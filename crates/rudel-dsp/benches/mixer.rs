//! Mixer inner-loop baseline benchmark (for the `process_block` refactor).
//!
//!   cargo bench -p rudel-dsp --bench mixer
//!
//! The audio callback (`rudel-audio` `Mixer::render_frame`) is per-sample: for
//! every frame it walks all active voices, calls the trait-object `tick()` plus
//! `dry`/`room`/`delay_send`, and accumulates the dry/reverb/delay buses. A
//! proposed refactor would add `process_block` to `VoiceLike` so that (a) the
//! per-voice virtual dispatch is amortized over a block, (b) the mix
//! accumulation vectorizes, and (c) the *memoryless* post-effects (crush /
//! shape / distort / tremolo / postgain) vectorize over the block. The scalar,
//! time-recursive synthesis (oscillator phase, biquad filters) cannot be
//! helped, so the refactor only pays off if those three parts are a meaningful
//! fraction of the per-frame cost.
//!
//! This baseline reproduces the mixer's inner loop over a set of voices and
//! reports ns/frame and ns/frame/voice for two configs: `synth` (saw voice +
//! low-pass filter, synthesis only) and `synth+postfx` (the same wrapped in the
//! memoryless post-fx chain). The delta is the cost the refactor could
//! vectorize; if it is small relative to `synth`, the refactor is not
//! worthwhile.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_dsp::{FilterParams, PostFx, PostFxVoice, Voice, VoiceLike, VoiceParams, Waveform};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK: usize = 512;
const VOICE_COUNTS: &[usize] = &[8, 16, 32, 64];

/// A sustained saw voice with one active low-pass biquad — a realistic
/// synthesis cost. `duration` is huge so the voice never goes `done`.
fn synth_params() -> VoiceParams {
    VoiceParams {
        duration: 1.0e9,
        waveform: Waveform::Saw,
        lp: FilterParams {
            freq: Some(2_000.0),
            ..FilterParams::default()
        },
        ..VoiceParams::default()
    }
}

/// The memoryless post-fx chain the refactor would vectorize over a block.
fn memoryless_fx() -> PostFx {
    PostFx {
        crush: Some(8.0),
        shape: Some(0.4),
        distort: Some(0.5),
        tremolo: Some(5.0),
        postgain: 0.8,
        ..Default::default()
    }
}

fn synth_voice() -> Box<dyn VoiceLike> {
    Box::new(Voice::new(synth_params(), SAMPLE_RATE))
}

fn postfx_voice() -> Box<dyn VoiceLike> {
    let inner = Box::new(Voice::new(synth_params(), SAMPLE_RATE));
    Box::new(PostFxVoice::new(inner, memoryless_fx(), SAMPLE_RATE))
}

/// Render `BLOCK` frames mixing all voices, mirroring `Mixer::render_frame`'s
/// inner `retain_mut` body (tick + per-voice send queries + bus accumulation).
fn mix_block(voices: &mut [Box<dyn VoiceLike>]) -> usize {
    let mut acc = 0.0f32;
    for _ in 0..BLOCK {
        let (mut dl, mut dr) = (0.0f32, 0.0f32);
        let (mut rl, mut rr) = (0.0f32, 0.0f32);
        let (mut el, mut er) = (0.0f32, 0.0f32);
        for v in voices.iter_mut() {
            let (a, b) = v.tick();
            let dry = v.dry();
            dl += a * dry;
            dr += b * dry;
            let room = v.room();
            if room > 0.0 {
                rl += a * room;
                rr += b * room;
            }
            let dsend = v.delay_send();
            if dsend > 0.0 {
                el += a * dsend;
                er += b * dsend;
            }
        }
        acc += dl + dr + rl + rr + el + er;
    }
    black_box(acc);
    BLOCK
}

/// Mix all voices by rendering each one a block at a time via `process_block`
/// (one virtual dispatch + one post-fx pass per block instead of per frame),
/// then accumulating the dry bus. This mirrors what a block-based `Mixer` would
/// do; `room`/`delay_send` are queried once per block (they are zero for these
/// voices, matching the per-sample loop's skipped sends).
fn mix_block_via_process_block(
    voices: &mut [Box<dyn VoiceLike>],
    sl: &mut [f32],
    sr: &mut [f32],
    dl: &mut [f32],
    dr: &mut [f32],
) -> usize {
    dl.iter_mut().for_each(|x| *x = 0.0);
    dr.iter_mut().for_each(|x| *x = 0.0);
    for v in voices.iter_mut() {
        v.process_block(sl, sr);
        let dry = v.dry();
        let _ = (v.room(), v.delay_send());
        for i in 0..BLOCK {
            dl[i] += sl[i] * dry;
            dr[i] += sr[i] * dry;
        }
    }
    let mut acc = 0.0f32;
    for i in 0..BLOCK {
        acc += dl[i] + dr[i];
    }
    black_box(acc);
    BLOCK
}

fn time(label: &str, voices: usize, iters: u32, mut f: impl FnMut() -> usize) {
    let mut sink = 0usize;
    for _ in 0..(iters / 10).max(1) {
        sink = sink.wrapping_add(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        sink = sink.wrapping_add(f());
    }
    let per_block = start.elapsed().as_secs_f64() / f64::from(iters);
    let per_frame_ns = per_block * 1e9 / BLOCK as f64;
    println!(
        "{label:<22} {:>9.2} ns/frame {:>8.3} ns/frame/voice   (work={sink})",
        per_frame_ns,
        per_frame_ns / voices as f64,
    );
}

fn main() {
    println!("# mixer inner loop: mix N voices per frame @ {SAMPLE_RATE} Hz");
    println!("## per-sample tick (baseline)");
    for &n in VOICE_COUNTS {
        let mut voices: Vec<Box<dyn VoiceLike>> = (0..n).map(|_| synth_voice()).collect();
        time(&format!("synth x{n}"), n, 3_000, || mix_block(&mut voices));
    }
    println!();
    for &n in VOICE_COUNTS {
        let mut voices: Vec<Box<dyn VoiceLike>> = (0..n).map(|_| postfx_voice()).collect();
        time(&format!("synth+postfx x{n}"), n, 3_000, || {
            mix_block(&mut voices)
        });
    }

    println!("\n## process_block (Phase 1 prototype)");
    let (mut sl, mut srr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut dl, mut dr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    // synth-only voices use the default `process_block` (a `tick` loop), so this
    // isolates the dispatch-amortization win on the synthesis path.
    for &n in VOICE_COUNTS {
        let mut voices: Vec<Box<dyn VoiceLike>> = (0..n).map(|_| synth_voice()).collect();
        time(&format!("synth x{n}"), n, 3_000, || {
            mix_block_via_process_block(&mut voices, &mut sl, &mut srr, &mut dl, &mut dr)
        });
    }
    println!();
    for &n in VOICE_COUNTS {
        let mut voices: Vec<Box<dyn VoiceLike>> = (0..n).map(|_| postfx_voice()).collect();
        time(&format!("synth+postfx x{n}"), n, 3_000, || {
            mix_block_via_process_block(&mut voices, &mut sl, &mut srr, &mut dl, &mut dr)
        });
    }
}
