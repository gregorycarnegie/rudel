//! Real-mixer render benchmark (`OfflineMixer`).
//!
//!   cargo bench -p rudel-audio --bench render
//!
//! Unlike rudel-dsp's `mixer` bench (which times the voice-mix inner loop in
//! isolation), this drives the *full* `Mixer` — including the global fundsp
//! reverb and the stereo delay line, whose per-frame cost is fixed regardless
//! of voice count and is unaffected by the `process_block` refactor. It is the
//! end-to-end baseline: how many ns/frame the audio callback actually spends
//! mixing N active synth+post-fx voices plus the global effects.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_audio::{NoteEvent, OfflineMixer};
use rudel_dsp::{FilterParams, PostFx, VoiceParams, VoiceSpec, Waveform};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK: usize = 512;
const VOICE_COUNTS: &[usize] = &[8, 16, 32, 64];

/// A sustained saw + low-pass voice spec wrapped in the memoryless post-fx
/// chain. `duration` is huge so the voices stay active for the whole render.
fn note_event() -> NoteEvent {
    let params = VoiceParams {
        duration: 1.0e9,
        waveform: Waveform::Saw,
        lp: FilterParams {
            freq: Some(2_000.0),
            ..FilterParams::default()
        },
        ..VoiceParams::default()
    };
    NoteEvent {
        onset_seconds: 0.0,
        spec: VoiceSpec::Synth(Box::new(params)),
        fx: PostFx {
            crush: Some(8.0),
            shape: Some(0.4),
            distort: Some(0.5),
            tremolo: Some(5.0),
            postgain: 0.8,
            ..Default::default()
        },
        cut: None,
        tags: Vec::new(),
    }
}

/// An offline mixer with `n` active voices (scheduled at onset 0 and started by
/// one warm-up frame).
fn loaded_mixer(n: usize) -> OfflineMixer {
    let mut m = OfflineMixer::new(SAMPLE_RATE);
    for _ in 0..n {
        m.schedule(note_event());
    }
    m.render_frame(); // drain the queue and start the voices
    assert_eq!(m.active_len(), n, "all voices should be active");
    m
}

fn render_block(mixer: &mut OfflineMixer, out: &mut [(f32, f32)]) -> usize {
    mixer.render_block(out);
    black_box(out[0].0 + out[BLOCK - 1].1);
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
    println!(
        "# full mixer (voices + reverb + delay): render {BLOCK}-frame blocks @ {SAMPLE_RATE} Hz"
    );
    let mut out = vec![(0.0f32, 0.0f32); BLOCK];
    for &n in VOICE_COUNTS {
        let mut mixer = loaded_mixer(n);
        time(&format!("synth+postfx x{n}"), n, 2_000, || {
            render_block(&mut mixer, &mut out)
        });
    }
}
