//! Per-sample oscillator voice cost.
//!
//!   cargo bench -p rudel-dsp --bench voice
//!
//! `Voice::tick` is the innermost thing the audio callback does — every active
//! synth voice pays it 48000 times a second — so a nanosecond here is worth
//! more than anywhere else in the engine. The `mixer` bench measures the same
//! path through the mix loop; this one isolates it, and breaks out the two
//! pieces that dominate: the phase wrap and the filter.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_dsp::{FilterParams, Voice, VoiceLike, VoiceParams, Waveform};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
const N: usize = 2_000_000;

fn params(waveform: Waveform, filtered: bool) -> VoiceParams {
    VoiceParams {
        duration: 1.0e9, // never goes `done`
        waveform,
        lp: FilterParams {
            freq: filtered.then_some(2_000.0),
            ..FilterParams::default()
        },
        ..VoiceParams::default()
    }
}

fn time(label: &str, mut f: impl FnMut()) {
    for _ in 0..N / 10 {
        f();
    }
    let start = Instant::now();
    for _ in 0..N {
        f();
    }
    println!(
        "{label:<28} {:>8.2} ns/sample",
        start.elapsed().as_nanos() as f64 / N as f64
    );
}

fn main() {
    println!("# Voice::tick, one stereo sample @ {SAMPLE_RATE} Hz");
    for (label, waveform, filtered) in [
        ("saw", Waveform::Saw, false),
        ("saw + lowpass", Waveform::Saw, true),
        ("sine + lowpass", Waveform::Sine, true),
        ("square + lowpass", Waveform::Square, true),
    ] {
        let mut v = Voice::new(params(waveform, filtered), SAMPLE_RATE);
        time(label, || {
            black_box(v.tick());
        });
    }

    // Through the trait object, as the mixer actually calls it.
    let mut v: Box<dyn VoiceLike> = Box::new(Voice::new(params(Waveform::Saw, true), SAMPLE_RATE));
    time("saw + lowpass (dyn)", || {
        black_box(v.tick());
    });

    // The phase wrap, isolated: `rem_euclid(1.0)` is a division plus a branch
    // where `x - x.floor()` is one instruction, and a voice wraps twice per
    // sample (advance + waveform lookup).
    let mut phase = 0.0f32;
    time("wrap pair: rem_euclid", || {
        black_box(2.0 * phase.rem_euclid(1.0) - 1.0);
        phase = (phase + 0.01).rem_euclid(1.0);
    });
    let mut phase = 0.0f32;
    time("wrap pair: x - x.floor()", || {
        let p = phase - phase.floor();
        black_box(2.0 * p - 1.0);
        phase += 0.01;
        phase -= phase.floor();
    });
}
