// convolver.rs - convolution reverb. Ports superdough's `createReverb`
// (strudel/packages/superdough/reverb.mjs), the impulse-response generator it
// calls (`reverbGen.mjs`, Alan deLespinasse's, Apache-2.0), and the
// `adjustLength` resampler that fits a user impulse response (`ir`/`iresponse`)
// to the requested room size.
//
// Upstream hands the resulting buffer to a Web Audio `ConvolverNode`; here the
// convolution itself is a uniform-partitioned overlap-save FFT convolver, since
// a 3-second impulse response is far too long to convolve directly.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{fft::Fft, filter::Biquad};
use wide::f32x8;

/// Partition length. Bigger is cheaper (cost per sample is ~`6·irLen/PARTITION`
/// flops) but adds latency, since a uniform-partitioned convolver cannot emit a
/// block before it has one.
///
// ponytail: uniform partitioning, so the reverb return is one partition late
// (~23ms at 44.1kHz — within the range of a normal reverb pre-delay, and
// upstream's `ConvolverNode` has none). Non-uniform partitioning (a few short
// blocks up front, long ones behind) removes it, at a lot more bookkeeping.
const PARTITION: usize = 1024;
/// FFT length: twice the partition, so a linear convolution of two partitions
/// fits without wrapping.
const FFT_SIZE: usize = PARTITION * 2;
/// SIMD lane count for the per-bin partition sum, which is the reverb's whole
/// inner loop.
const LANES: usize = 8;

/// The Nyquist bin. A real signal's spectrum is conjugate-symmetric about it,
/// so the partition sum only has to run this far. A multiple of [`LANES`], so
/// the vectorized loop still has no scalar remainder.
const HALF_SPECTRUM: usize = FFT_SIZE / 2;

/// A stereo impulse response, at the engine's sample rate.
#[derive(Clone, Debug, PartialEq)]
pub struct ImpulseResponse {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl ImpulseResponse {
    pub fn len(&self) -> usize {
        self.left.len().max(self.right.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A deterministic 32-bit RNG for the impulse-response noise.
///
/// `reverbGen.mjs` uses `Math.random()`, so there is no sample-exact target to
/// hit; a seeded generator is used instead so a given room setting always
/// produces the same tail (rebuilding the reverb mid-session does not change
/// its character, unlike upstream).
struct Rng(u32);

impl Rng {
    /// A random sample in `-1..1`, matching `randomSample()`.
    fn sample(&mut self) -> f32 {
        // xorshift32.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Generate a reverb impulse response — the port of
/// `reverbGen.generateReverb` plus `applyGradualLowpass`.
///
/// `decay_time` is the -60dB time (`size`/`roomsize`), `fade_in` the
/// `roomfade` ramp, and `lp_start`/`lp_end` the `roomlp`/`roomdim` sweep the
/// tail's lowpass makes over `decay_time` seconds.
pub fn generate_reverb_ir(
    sample_rate: f32,
    decay_time: f32,
    fade_in: f32,
    lp_start: f32,
    lp_end: f32,
) -> ImpulseResponse {
    let decay_time = decay_time.max(0.001);
    // "params.decayTime is the -60dB fade time. We let it go 50% longer to get
    // to -90dB."
    let total_time = decay_time * 1.5;
    let decay_frames = (decay_time * sample_rate).round().max(1.0);
    let n = (total_time * sample_rate).round() as usize;
    let fade_frames = (fade_in.max(0.0) * sample_rate).round() as usize;
    // 60dB is a factor of 1000 in amplitude.
    let decay_base = (1.0f32 / 1000.0).powf(1.0 / decay_frames);

    let mut rng = Rng(0x9e37_79b9);
    let channel = |rng: &mut Rng| -> Vec<f32> {
        let mut buf = Vec::with_capacity(n);
        let mut gain = 1.0f32;
        for _ in 0..n {
            buf.push(rng.sample() * gain);
            gain *= decay_base;
        }
        for (j, v) in buf.iter_mut().take(fade_frames).enumerate() {
            *v *= j as f32 / fade_frames as f32;
        }
        buf
    };
    let mut ir = ImpulseResponse {
        left: channel(&mut rng),
        right: channel(&mut rng),
    };

    apply_gradual_lowpass(&mut ir, sample_rate, lp_start, lp_end, decay_time);
    ir
}

/// `applyGradualLowpass`: a `lowpass` biquad with `Q = 0.0001` whose cutoff
/// ramps linearly from `lp_start` to `lp_end` over `ramp_seconds`, then holds.
fn apply_gradual_lowpass(
    ir: &mut ImpulseResponse,
    sample_rate: f32,
    lp_start: f32,
    lp_end: f32,
    ramp_seconds: f32,
) {
    // "if (lpFreqStart == 0) { callback(input); return; }"
    if lp_start == 0.0 {
        return;
    }
    let nyquist = sample_rate / 2.0;
    let lp_start = lp_start.min(nyquist);
    let lp_end = lp_end.min(nyquist);
    let ramp_frames = (ramp_seconds * sample_rate).max(1.0);

    // Web Audio reads a `lowpass`/`highpass` node's `Q` in *decibels*, so
    // upstream's `filter.Q.value = 0.0001` is a linear Q of 10^(0.0001/20) ≈ 1.
    // Rudel's `Biquad` takes a linear Q, hence the 1.0 here rather than 0.0001.
    const Q: f32 = 1.0;
    for buf in [&mut ir.left, &mut ir.right] {
        let mut filter = Biquad::lowpass(sample_rate, lp_start.max(1.0), Q);
        for (i, v) in buf.iter_mut().enumerate() {
            let t = (i as f32 / ramp_frames).min(1.0);
            let freq = lp_start + (lp_end - lp_start) * t;
            filter.set_lowpass(sample_rate, freq.max(1.0), Q);
            *v = filter.process(*v);
        }
    }
}

/// `BaseAudioContext.prototype.adjustLength`: fit a loaded impulse response
/// (`ir`/`iresponse`) to `duration` seconds, reading it at `speed` and starting
/// `offset` (0..1) of the way in, looping when it runs out.
pub fn adjust_length(
    src_l: &[f32],
    src_r: &[f32],
    sample_rate: f32,
    duration: f32,
    speed: f32,
    offset: f32,
) -> ImpulseResponse {
    let src_len = src_l.len().max(src_r.len());
    if src_len == 0 {
        return ImpulseResponse {
            left: Vec::new(),
            right: Vec::new(),
        };
    }
    let sample_offset = (offset.clamp(0.0, 1.0) * src_len as f32).floor() as i64;
    let new_len = (sample_rate * duration.max(0.0)).max(0.0) as usize;
    let speed_abs = speed.abs();

    let read = |src: &[f32], i: usize| -> f32 {
        if src.is_empty() {
            return 0.0;
        }
        // `position = (sampleOffset + i * abs(speed)) % oldData.length`, then
        // negated when `speed < 1`; `Float32Array.at()` counts a negative index
        // from the end, and an out-of-range one yields `undefined` -> 0.
        let mut position = (sample_offset + (i as f32 * speed_abs) as i64) % src_len as i64;
        if speed < 1.0 {
            position = -position;
        }
        let idx = if position < 0 {
            src_len as i64 + position
        } else {
            position
        };
        usize::try_from(idx)
            .ok()
            .and_then(|k| src.get(k).copied())
            .unwrap_or(0.0)
    };

    ImpulseResponse {
        left: (0..new_len).map(|i| read(src_l, i)).collect(),
        right: (0..new_len)
            .map(|i| read(if src_r.is_empty() { src_l } else { src_r }, i))
            .collect(),
    }
}

/// The gain a `ConvolverNode` applies to its impulse response when
/// `normalize` is `true` — which is its default, and superdough never turns it
/// off, so every Strudel reverb is scaled by this.
///
/// Ported verbatim from the Web Audio API spec's `calculateNormalizationScale`.
/// Without it a generated impulse response (whose first samples are full-scale
/// noise) makes the wet signal roughly two orders of magnitude too loud.
fn normalization_scale(ir: &ImpulseResponse, sample_rate: f32) -> f32 {
    const GAIN_CALIBRATION: f64 = 0.00125;
    const GAIN_CALIBRATION_SAMPLE_RATE: f64 = 44100.0;
    const MIN_POWER: f64 = 0.000125;

    let channels: [&Vec<f32>; 2] = [&ir.left, &ir.right];
    let length = ir.len();
    if length == 0 {
        return 1.0;
    }
    let sum: f64 = channels
        .iter()
        .flat_map(|c| c.iter())
        .map(|x| (*x as f64) * (*x as f64))
        .sum();
    let mut power = (sum / (channels.len() * length) as f64).sqrt();
    // "Protect against accidental overload."
    if !power.is_finite() || power < MIN_POWER {
        power = MIN_POWER;
    }
    // "Calibrate to make perceived volume same as unprocessed", then scale by
    // the sample rate.
    let scale =
        (1.0 / power) * GAIN_CALIBRATION * (GAIN_CALIBRATION_SAMPLE_RATE / sample_rate as f64);
    scale as f32
}

/// One channel of the partitioned convolver.
struct ConvChannel {
    /// Per-partition impulse-response spectra, `(re, im)` of length `FFT_SIZE`.
    ir_spectra: Vec<(Vec<f32>, Vec<f32>)>,
    /// Ring of recent input spectra, newest at `head`.
    in_spectra: Vec<(Vec<f32>, Vec<f32>)>,
    head: usize,
    /// The previous partition of input, prepended for overlap-save.
    prev: Vec<f32>,
    /// Input accumulator and the ready output block.
    input: Vec<f32>,
    output: Vec<f32>,
    /// Scratch buffers.
    re: Vec<f32>,
    im: Vec<f32>,
    acc_re: Vec<f32>,
    acc_im: Vec<f32>,
}

impl ConvChannel {
    fn new(fft: &Fft, ir: &[f32]) -> ConvChannel {
        let n_parts = ir.len().div_ceil(PARTITION).max(1);
        let mut ir_spectra = Vec::with_capacity(n_parts);
        for p in 0..n_parts {
            let mut re = vec![0.0; FFT_SIZE];
            let mut im = vec![0.0; FFT_SIZE];
            let start = p * PARTITION;
            let end = (start + PARTITION).min(ir.len());
            if start < ir.len() {
                re[..end - start].copy_from_slice(&ir[start..end]);
            }
            fft.forward(&mut re, &mut im);
            ir_spectra.push((re, im));
        }
        ConvChannel {
            in_spectra: (0..n_parts)
                .map(|_| (vec![0.0; FFT_SIZE], vec![0.0; FFT_SIZE]))
                .collect(),
            ir_spectra,
            head: 0,
            prev: vec![0.0; PARTITION],
            input: Vec::with_capacity(PARTITION),
            output: vec![0.0; PARTITION],
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            acc_re: vec![0.0; FFT_SIZE],
            acc_im: vec![0.0; FFT_SIZE],
        }
    }

    /// What [`run_block`](Self::run_block) leaves behind when the whole frame
    /// and the entire input ring are zero — which they are once the convolver
    /// has been fed silence for longer than the impulse response. Skipping the
    /// three FFTs and the partition sum is the difference between a few
    /// hundred nanoseconds a frame and none at all, and every pattern that
    /// never says `room` runs in exactly this state.
    fn skip_block(&mut self) {
        self.head = (self.head + 1) % self.in_spectra.len();
        self.output.fill(0.0);
        self.prev.fill(0.0);
        self.input.clear();
    }

    /// Transform the pending partition and overlap-save it against the IR.
    fn run_block(&mut self, fft: &Fft) {
        // Overlap-save frame: [previous partition, this partition].
        self.re[..PARTITION].copy_from_slice(&self.prev);
        self.re[PARTITION..].copy_from_slice(&self.input);
        self.im.fill(0.0);
        fft.forward(&mut self.re, &mut self.im);

        let n = self.in_spectra.len();
        self.head = (self.head + 1) % n;
        self.in_spectra[self.head].0.copy_from_slice(&self.re);
        self.in_spectra[self.head].1.copy_from_slice(&self.im);

        // Y = sum_p H[p] * X[head - p]. This is the whole cost of the reverb:
        // one complex multiply-accumulate per bin per partition, and a 3-second
        // room at 48kHz has ~140 partitions of 2048 bins. The bins are
        // independent, so it vectorizes exactly — `FFT_SIZE` is a multiple of
        // the lane count, leaving no scalar remainder.
        //
        // Only the lower half is summed: the input frame and the IR are both
        // real, so their spectra are conjugate-symmetric, and so is the
        // product. The upper half is mirrored in below instead of being
        // multiplied out again, which halves the loop above.
        self.acc_re.fill(0.0);
        self.acc_im.fill(0.0);
        for (p, (hr, hi)) in self.ir_spectra.iter().enumerate() {
            let (xr, xi) = &self.in_spectra[(self.head + n - p % n) % n];
            for k in (0..=HALF_SPECTRUM).step_by(LANES) {
                let hr8 = f32x8::from(&hr[k..k + LANES]);
                let hi8 = f32x8::from(&hi[k..k + LANES]);
                let xr8 = f32x8::from(&xr[k..k + LANES]);
                let xi8 = f32x8::from(&xi[k..k + LANES]);
                let re = f32x8::from(&self.acc_re[k..k + LANES]) + hr8 * xr8 - hi8 * xi8;
                let im = f32x8::from(&self.acc_im[k..k + LANES]) + hr8 * xi8 + hi8 * xr8;
                self.acc_re[k..k + LANES].copy_from_slice(&re.to_array());
                self.acc_im[k..k + LANES].copy_from_slice(&im.to_array());
            }
        }

        // Y[N-k] = conj(Y[k]); the loop above computed bins 0..=N/2 (plus a few
        // past it, which this overwrites with the same values).
        for k in 1..HALF_SPECTRUM {
            self.acc_re[FFT_SIZE - k] = self.acc_re[k];
            self.acc_im[FFT_SIZE - k] = -self.acc_im[k];
        }

        self.re.copy_from_slice(&self.acc_re);
        self.im.copy_from_slice(&self.acc_im);
        fft.inverse(&mut self.re, &mut self.im);
        // Overlap-save: the second half of the frame is the valid output.
        self.output.copy_from_slice(&self.re[PARTITION..]);

        self.prev.copy_from_slice(&self.input);
        self.input.clear();
    }
}

/// A stereo convolution reverb.
pub struct Convolver {
    fft: Fft,
    left: ConvChannel,
    right: ConvChannel,
    /// Read cursor into the ready output block; also the write cursor for the
    /// input accumulator, so the two stay in step.
    pos: usize,
    /// Consecutive all-zero input samples, and the run length past which the
    /// convolver is provably settled: the impulse response is finite, so once
    /// every sample it could still see is zero, so is its output. `room`
    /// defaults to 0, so an orbit that never uses the reverb sits here
    /// permanently and pays nothing.
    silent_run: usize,
    settled_after: usize,
}

impl Convolver {
    /// Build a convolver over `ir`, applying the same impulse-response
    /// normalization a `ConvolverNode` does (see [`normalization_scale`]).
    pub fn new(ir: &ImpulseResponse, sample_rate: f32) -> Convolver {
        let fft = Fft::new(FFT_SIZE);
        let scale = normalization_scale(ir, sample_rate);
        let scaled = |c: &[f32]| c.iter().map(|x| x * scale).collect::<Vec<_>>();
        let left = ConvChannel::new(&fft, &scaled(&ir.left));
        let right = ConvChannel::new(&fft, &scaled(&ir.right));
        // One block per ring slot zeroes every input spectrum, plus one more
        // to flush `prev` — after that the output cannot be anything but zero.
        let parts = left.in_spectra.len().max(right.in_spectra.len());
        Convolver {
            settled_after: (parts + 1) * PARTITION,
            left,
            right,
            fft,
            pos: 0,
            silent_run: 0,
        }
    }

    /// Push one stereo sample in, get one (partition-delayed) sample out.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.left.input.push(l);
        self.right.input.push(r);
        if l == 0.0 && r == 0.0 {
            self.silent_run += 1;
        } else {
            self.silent_run = 0;
        }
        let out = (self.left.output[self.pos], self.right.output[self.pos]);
        self.pos += 1;
        if self.pos == PARTITION {
            self.pos = 0;
            if self.silent_run >= self.settled_after {
                self.left.skip_block();
                self.right.skip_block();
            } else {
                self.left.run_block(&self.fft);
                self.right.run_block(&self.fft);
            }
        }
        out
    }

    /// Samples of latency before the first output appears.
    pub const LATENCY: usize = PARTITION;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolves_against_a_direct_sum() {
        // Longer than one PARTITION, so the convolver actually has to walk its
        // ring of input spectra — a single-partition IR makes every index in
        // `run_block` collapse to 0 and hides the ring arithmetic.
        let ir_len = PARTITION * 2 + 300;
        let ir = ImpulseResponse {
            // The decay is slow on purpose: at `exp(-i/100)` everything past
            // the first partition is below the tolerance, so the later
            // partitions could be dropped entirely and the sum would still
            // match.
            left: (0..ir_len)
                .map(|i| (i as f32 * 0.37).sin() * (-(i as f32) / 2000.0).exp())
                .collect(),
            right: (0..ir_len)
                .map(|i| if i == 0 { 1.0 } else { 0.0 })
                .collect(),
        };
        let input: Vec<f32> = (0..PARTITION * 4)
            .map(|i| (i as f32 * 0.11).sin())
            .collect();

        let mut c = Convolver::new(&ir, 44100.0);
        let got: Vec<f32> = input.iter().map(|&s| c.process(s, s).0).collect();
        // The convolver normalizes its impulse response like a `ConvolverNode`,
        // so the reference is the direct convolution times that scale.
        let scale = normalization_scale(&ir, 44100.0);

        for n in 0..PARTITION * 3 {
            // The convolver's output at `n + LATENCY` is the convolution at `n`.
            let want: f32 = (0..ir_len.min(n + 1))
                .map(|k| ir.left[k] * input[n - k])
                .sum::<f32>()
                * scale;
            let got = got[n + Convolver::LATENCY];
            assert!((got - want).abs() < 1e-5, "sample {n}: {got} != {want}");
        }
    }

    #[test]
    fn an_impulse_ir_passes_the_signal_through() {
        let ir = ImpulseResponse {
            left: vec![1.0],
            right: vec![1.0],
        };
        let mut c = Convolver::new(&ir, 44100.0);
        let scale = normalization_scale(&ir, 44100.0);
        let input: Vec<f32> = (0..PARTITION * 3).map(|i| (i as f32 * 0.3).cos()).collect();
        let got: Vec<f32> = input.iter().map(|&s| c.process(s, s).0).collect();
        for n in 0..PARTITION * 2 {
            let (a, b) = (got[n + Convolver::LATENCY], input[n] * scale);
            assert!((a - b).abs() < 1e-6, "sample {n}: {a} != {b}");
        }
    }

    #[test]
    fn generated_ir_decays_by_60db_over_the_decay_time() {
        let sr = 44100.0;
        let ir = generate_reverb_ir(sr, 1.0, 0.0, 0.0, 0.0);
        // totalTime = decayTime * 1.5.
        assert_eq!(ir.left.len(), (1.5 * sr) as usize);

        // RMS over a short window near the start vs. at the -60dB point.
        let rms = |from: usize| -> f32 {
            let w = &ir.left[from..from + 2000];
            (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt()
        };
        let ratio = rms(0) / rms(sr as usize - 2000);
        // -60dB is a factor of 1000 in amplitude; allow a wide band since the
        // windows straddle a continuously decaying envelope.
        assert!(
            (300.0..3000.0).contains(&ratio),
            "expected ~1000x decay over 1s, got {ratio}"
        );
    }

    #[test]
    fn roomfade_ramps_the_ir_in() {
        let sr = 44100.0;
        let fade = 0.25;
        let faded = generate_reverb_ir(sr, 1.0, fade, 0.0, 0.0);
        // The generator is seeded, so the unfaded IR is the same noise: the
        // ratio between them is exactly the `j / fadeInSampleFrames` ramp.
        let plain = generate_reverb_ir(sr, 1.0, 0.0, 0.0, 0.0);
        let fade_frames = (fade * sr).round() as usize;

        for j in [1usize, 100, 1000, fade_frames / 2, fade_frames - 1] {
            let want = plain.left[j] * j as f32 / fade_frames as f32;
            assert!(
                (faded.left[j] - want).abs() < 1e-6,
                "sample {j}: {} != {want}",
                faded.left[j]
            );
        }
        // Past the fade the two are identical.
        for j in [fade_frames, fade_frames + 1000, fade_frames * 2] {
            assert_eq!(faded.left[j], plain.left[j], "sample {j} past the fade");
        }
    }

    #[test]
    fn roomlp_darkens_the_tail() {
        let sr = 44100.0;
        // A closing lowpass (15k -> 500) should leave the tail with much less
        // high-frequency energy than an open one.
        let dark = generate_reverb_ir(sr, 1.0, 0.0, 15000.0, 500.0);
        let open = generate_reverb_ir(sr, 1.0, 0.0, 0.0, 0.0);
        // Sample-to-sample difference is a crude high-frequency measure.
        let hf = |b: &[f32]| -> f32 {
            let w = &b[(sr as usize)..(sr as usize + 4000)];
            w.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>() / w.len() as f32
        };
        let energy = |b: &[f32]| -> f32 {
            let w = &b[(sr as usize)..(sr as usize + 4000)];
            w.iter().map(|x| x.abs()).sum::<f32>() / w.len() as f32
        };
        // Normalize by overall level, since the filter also attenuates.
        let dark_ratio = hf(&dark.left) / energy(&dark.left).max(1e-12);
        let open_ratio = hf(&open.left) / energy(&open.left).max(1e-12);
        assert!(
            dark_ratio < open_ratio * 0.5,
            "roomdim should reduce HF content: {dark_ratio} vs {open_ratio}"
        );
    }

    #[test]
    fn adjust_length_loops_and_offsets_a_loaded_ir() {
        let src: Vec<f32> = (0..100).map(|i| i as f32).collect();
        // duration 1s at 10Hz -> 10 samples, speed 1, no offset: the head.
        let out = adjust_length(&src, &src, 10.0, 1.0, 1.0, 0.0);
        assert_eq!(out.left, (0..10).map(|i| i as f32).collect::<Vec<_>>());

        // A 0.5 offset starts halfway in.
        let off = adjust_length(&src, &src, 10.0, 1.0, 1.0, 0.5);
        assert_eq!(off.left[0], 50.0);

        // Speed 2 reads every other sample.
        let fast = adjust_length(&src, &src, 10.0, 1.0, 2.0, 0.0);
        assert_eq!(
            fast.left,
            (0..10).map(|i| (i * 2) as f32).collect::<Vec<_>>()
        );

        // Reading past the end wraps rather than going silent.
        let long = adjust_length(&src, &src, 10.0, 30.0, 1.0, 0.0);
        assert_eq!(long.left.len(), 300);
        assert_eq!(long.left[100], long.left[0]);

        // An empty source yields an empty IR rather than panicking.
        assert!(adjust_length(&[], &[], 10.0, 1.0, 1.0, 0.0).is_empty());
        // ...and a non-empty one does not, so `is_empty` has to look.
        assert!(!out.is_empty());
    }

    /// `speed < 1` negates the read position, so the IR is read backwards from
    /// the end. The 1-based source matters: with a 0 at index 0, a mis-signed
    /// index lands on the out-of-range 0.0 fallback and reads as correct.
    #[test]
    fn adjust_length_reads_backwards_below_unit_speed() {
        let src: Vec<f32> = (1..=100).map(|i| i as f32).collect();
        let out = adjust_length(&src, &src, 10.0, 1.0, 0.5, 0.0);
        // position = -(floor(i * 0.5) % 100), then counted from the end.
        let want: Vec<f32> = (0..10)
            .map(|i: usize| {
                let p = (i as f32 * 0.5) as i64;
                if p == 0 {
                    src[0]
                } else {
                    src[100 - p as usize]
                }
            })
            .collect();
        assert_eq!(out.left, want);
        // At exactly unit speed it reads forwards instead.
        assert_eq!(adjust_length(&src, &src, 10.0, 1.0, 1.0, 0.0).left[1], 2.0);
    }

    #[test]
    fn the_impulse_response_is_normalized_like_a_convolvernode() {
        // `ConvolverNode.normalize` defaults to true and superdough never turns
        // it off, so the wet signal is scaled by the spec's
        // `calculateNormalizationScale`. Skipping it made `room(...)` roughly
        // two orders of magnitude too loud.
        let sr = 44100.0;
        let ir = generate_reverb_ir(sr, 2.0, 0.1, 15000.0, 1000.0);
        let scale = normalization_scale(&ir, sr);
        assert!(
            (0.002..0.05).contains(&scale),
            "a default 2s room should be scaled well below unity, got {scale}"
        );

        // The spec formula, recomputed independently here.
        let n = ir.len();
        let sum: f64 = ir
            .left
            .iter()
            .chain(ir.right.iter())
            .map(|x| (*x as f64) * (*x as f64))
            .sum();
        let power = (sum / (2 * n) as f64).sqrt();
        let want = ((1.0 / power) * 0.00125) as f32;
        assert!((scale - want).abs() < 1e-6, "{scale} != {want}");

        // The calibration is quoted at 44.1kHz and scaled by 44100/sr, so the
        // same IR at 48kHz comes out proportionally quieter. (At 44.1k that
        // factor is exactly 1, which hides how it is applied.)
        let at_48k = normalization_scale(&ir, 48000.0);
        assert!(
            (at_48k - scale * 44100.0 / 48000.0).abs() < 1e-6,
            "{at_48k} is not {scale} scaled by 44100/48000"
        );

        // The point of normalizing by RMS power: the wet level stays roughly
        // constant as the room size changes, so `size` alters the tail's length
        // rather than its loudness.
        let long = normalization_scale(&generate_reverb_ir(sr, 8.0, 0.1, 15000.0, 1000.0), sr);
        let short = normalization_scale(&generate_reverb_ir(sr, 0.5, 0.1, 15000.0, 1000.0), sr);
        for other in [long, short] {
            let ratio = other / scale;
            assert!(
                (0.5..2.0).contains(&ratio),
                "room size should not swing the wet level much: {ratio}"
            );
        }

        // A silent impulse response falls back to `MIN_POWER` rather than
        // dividing by zero.
        let silent = ImpulseResponse {
            left: vec![0.0; 1000],
            right: vec![0.0; 1000],
        };
        assert!(normalization_scale(&silent, sr).is_finite());
        assert!(
            normalization_scale(
                &ImpulseResponse {
                    left: vec![],
                    right: vec![]
                },
                sr
            ) == 1.0
        );
    }

    #[test]
    fn a_normalized_reverb_send_stays_below_the_dry_signal() {
        // End-to-end: a full-scale impulse into a default room must not come
        // back louder than it went in.
        let sr = 44100.0;
        let ir = generate_reverb_ir(sr, 2.0, 0.1, 15000.0, 1000.0);
        let mut c = Convolver::new(&ir, sr);
        let mut peak = 0.0f32;
        for i in 0..sr as usize {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (l, r) = c.process(x, x);
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(
            peak < 1.0,
            "the wet return of a unit impulse should not exceed unity, got {peak}"
        );
        assert!(
            peak > 1e-4,
            "but it must not be inaudible either, got {peak}"
        );
    }

    #[test]
    fn the_generated_ir_is_reproducible() {
        let a = generate_reverb_ir(44100.0, 0.5, 0.0, 0.0, 0.0);
        let b = generate_reverb_ir(44100.0, 0.5, 0.0, 0.0, 0.0);
        assert_eq!(a, b);
        // The two channels are decorrelated, so the reverb is stereo.
        assert_ne!(a.left, a.right);
    }

    /// The rest of the suite only checks the IR's *statistics* — decay ratio,
    /// HF content, RMS scale — which any noise sequence satisfies. These
    /// goldens pin the actual samples, so the xorshift's shifts and the
    /// lowpass ramp's arithmetic cannot be rearranged unnoticed.
    #[test]
    fn the_generated_ir_matches_its_golden_samples() {
        // The window is short and the tolerance loose enough for libm drift;
        // any change to the noise or the ramp moves these by O(0.1).
        let check = |ir: &ImpulseResponse, want: &[(usize, f32, f32)], what: &str| {
            for &(i, l, r) in want {
                assert!(
                    (ir.left[i] - l).abs() < 1e-4 && (ir.right[i] - r).abs() < 1e-4,
                    "{what}[{i}]: ({}, {}) != ({l}, {r})",
                    ir.left[i],
                    ir.right[i]
                );
            }
        };
        // No lowpass (`lp_start == 0` returns early), so this is the seeded
        // noise times the decay envelope alone.
        check(
            &generate_reverb_ir(44100.0, 0.1, 0.0, 0.0, 0.0),
            &[
                (0, -0.366_812_94, 0.805_505_75),
                (1, 0.750_237_9, -0.230_076_73),
                (2, -0.033_295_233, -0.886_310_1),
                (3, -0.983_536_96, 0.114_570_86),
                (100, -0.726_035_83, -0.065_642_11),
                (1000, 0.060_700_76, 0.058_782_28),
            ],
            "plain",
        );
        // The same noise through the 8k -> 1k cutoff ramp.
        check(
            &generate_reverb_ir(44100.0, 0.1, 0.0, 8000.0, 1000.0),
            &[
                (0, -0.073_428_094, 0.161_245),
                (1, -0.038_910_106, 0.369_090_02),
                (2, 0.225_365_19, 0.043_515_28),
                (3, 0.084_338_2, -0.491_234_03),
                (100, 0.021_385_923, 0.231_807_29),
                (1000, 0.104_475_78, 0.130_161_69),
            ],
            "dark",
        );
    }

    #[test]
    fn the_lowpass_ramp_is_clamped_to_nyquist() {
        // A `roomlp` above Nyquist would make the biquad blow up, so both ends
        // of the ramp are clamped there — asking for 30k at 44.1k is the same
        // as asking for 22050.
        let sr = 44100.0;
        assert_eq!(
            generate_reverb_ir(sr, 0.1, 0.0, 30000.0, 40000.0),
            generate_reverb_ir(sr, 0.1, 0.0, sr / 2.0, sr / 2.0)
        );
    }

    /// The silence bypass has to be inaudible: a convolver that has idled
    /// through a long stretch of zeros must respond to the next burst exactly
    /// as a fresh one does. (The idle stretch is a whole number of partitions,
    /// so both are at the same point in the block cycle.)
    #[test]
    fn skipping_settled_silence_leaves_the_response_unchanged() {
        let sr = 44100.0;
        let ir = generate_reverb_ir(sr, 0.3, 0.0, 8000.0, 1000.0);
        let burst: Vec<f32> = (0..PARTITION * 4)
            .map(|i| if i < 64 { (i as f32 * 0.2).sin() } else { 0.0 })
            .collect();

        let mut idled = Convolver::new(&ir, sr);
        // Long enough to settle, and a whole number of partitions.
        let silence = idled.settled_after.next_multiple_of(PARTITION) + PARTITION * 2;
        for _ in 0..silence {
            assert_eq!(idled.process(0.0, 0.0), (0.0, 0.0));
        }
        assert!(
            idled.silent_run >= idled.settled_after,
            "should have settled"
        );

        let mut fresh = Convolver::new(&ir, sr);
        for &x in &burst {
            let (a, b) = idled.process(x, x);
            let (c, d) = fresh.process(x, x);
            assert_eq!((a, b), (c, d), "idled convolver diverged");
        }
        // And the burst really did produce something, so this is not vacuous.
        assert!(burst.iter().any(|&x| x != 0.0));
        assert!(idled.silent_run < idled.settled_after);
    }
}
