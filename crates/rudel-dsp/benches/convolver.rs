//! Convolution reverb throughput.
//!
//!   cargo bench -p rudel-dsp --bench convolver
//!
//! `room` is the most expensive thing the audio thread does: profiling the app
//! with one `.room(0.5)` orbit put `ConvChannel::run_block` at half of all the
//! work on the callback thread, nearly all of it in the partitioned
//! multiply-accumulate. This measures that loop directly — a decaying IR of a
//! given length, fed continuous noise so the settled-silence fast path never
//! kicks in — and reports the fraction of a realtime core it costs.
//!
//! Dependency-free `harness = false` main, matching the other rudel benches.

use rudel_dsp::{Convolver, generate_reverb_ir};
use std::{hint::black_box, time::Instant};

const SAMPLE_RATE: f32 = 48_000.0;
/// Room sizes worth knowing: Strudel's `room` default decay, and a long tail.
const DECAYS: &[f32] = &[1.0, 2.0, 3.0];
const FRAMES: usize = 48_000 * 2;

fn main() {
    println!("convolution reverb, {SAMPLE_RATE} Hz\n");
    println!("{:>8}  {:>10}  {:>12}  {:>10}", "decay", "ns/frame", "partitions", "realtime");
    for &decay in DECAYS {
        let ir = generate_reverb_ir(SAMPLE_RATE, decay, 0.0, 0.0, 0.0);
        let partitions = ir.left.len().div_ceil(1024);
        let mut conv = Convolver::new(&ir, SAMPLE_RATE);

        // Continuous input: a cheap deterministic noise, so no frame takes the
        // all-zero shortcut and every block runs the full partition sum.
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / 8_388_608.0 - 1.0
        };
        let input: Vec<(f32, f32)> = (0..FRAMES).map(|_| (noise(), noise())).collect();

        // Warm up the input ring so the timed run is steady state.
        for &(l, r) in &input[..4096] {
            black_box(conv.process(l, r));
        }
        let start = Instant::now();
        for &(l, r) in &input {
            black_box(conv.process(l, r));
        }
        let elapsed = start.elapsed();

        let ns = elapsed.as_secs_f64() * 1e9 / FRAMES as f64;
        let realtime = ns * f64::from(SAMPLE_RATE) / 1e9;
        println!("{decay:>7.1}s  {ns:>10.1}  {partitions:>12}  {:>9.1}%", realtime * 100.0);
    }
}
