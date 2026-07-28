// vocoder.rs - the `stretch` pitch shifter. Ports superdough's
// `phase-vocoder-processor` worklet (strudel/packages/superdough/worklets.mjs)
// and the overlap-add framework it derives from (`ola-processor.js`), both
// originally from https://github.com/olvb/phaze.
//
// The algorithm is a peak-shifting phase vocoder: each 2048-sample analysis
// frame is Hann-windowed and FFT'd, spectral peaks are located, and each peak's
// region of influence is moved to `peakIndex * pitchFactor` with a phase
// correction, then inverse-FFT'd and overlap-added at a 128-sample hop.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fft::Fft;
use std::f32::consts::TAU;

/// `BUFFERED_BLOCK_SIZE` in worklets.mjs — the analysis frame length.
const BLOCK_SIZE: usize = 2048;
/// `WEBAUDIO_BLOCK_SIZE` in ola-processor.js — the hop between frames. Upstream
/// only supports a hop of one Web Audio render quantum.
const HOP_SIZE: usize = 128;
/// `nbOverlaps = blockSize / hopSize`, the overlap-add normalization divisor.
const OVERLAPS: f32 = (BLOCK_SIZE / HOP_SIZE) as f32;

/// `fround` from worklets.mjs: `(x + 0.5) | 0`, i.e. round-half-up *truncated*
/// to an int32 (so it truncates toward zero for negatives).
fn fround(x: f32) -> i32 {
    (x + 0.5) as i32
}

/// `fceil` from worklets.mjs: `(x + 1) | 0`.
fn fceil(x: f32) -> i32 {
    (x + 1.0) as i32
}

// ---------------------------------------------------------------------------
// Phase vocoder

/// One channel of the phase vocoder: the OLA buffers plus the analysis state.
///
// ponytail: two 2048-point FFTs per 128 input samples per channel is the
// upstream cost too (it is the same algorithm), but it makes `stretch` by far
// the most expensive per-voice effect. If many simultaneous stretched voices
// ever matter, the fix is a shared worker rather than a cheaper transform.
struct Channel {
    /// `inputBuffers`: `blockSize + hopSize` long, new samples land at the tail.
    input: Vec<f32>,
    /// `outputBuffers`: the overlap-add accumulator.
    output: Vec<f32>,
    /// Scratch for the framed analysis signal and the complex spectra.
    frame: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    shifted_re: Vec<f32>,
    shifted_im: Vec<f32>,
    /// Squared magnitudes of bins `0..=blockSize/2`.
    magnitudes: Vec<f32>,
    peaks: Vec<usize>,
}

impl Channel {
    fn new() -> Channel {
        Channel {
            input: vec![0.0; BLOCK_SIZE + HOP_SIZE],
            output: vec![0.0; BLOCK_SIZE],
            frame: vec![0.0; BLOCK_SIZE],
            re: vec![0.0; BLOCK_SIZE],
            im: vec![0.0; BLOCK_SIZE],
            shifted_re: vec![0.0; BLOCK_SIZE],
            shifted_im: vec![0.0; BLOCK_SIZE],
            magnitudes: vec![0.0; BLOCK_SIZE / 2 + 1],
            peaks: Vec::with_capacity(BLOCK_SIZE / 4),
        }
    }
}

/// The `stretch` pitch shifter. Feed it `HOP_SIZE`-sample stereo blocks; it
/// returns the overlap-added output for the same block (delayed by the
/// algorithm's inherent latency, which upstream compensates by starting the
/// voice 0.04s early).
pub struct PhaseVocoder {
    fft: Fft,
    hann: Vec<f32>,
    left: Channel,
    right: Channel,
    /// `timeCursor`, advanced by the hop each frame; drives the phase
    /// correction applied to a shifted peak.
    time_cursor: f32,
    pitch_factor: f32,
}

impl PhaseVocoder {
    /// `stretch` is the raw control value; upstream maps it to a pitch factor as
    /// `max(0, (v < 0 ? v * 0.25 : v) + 1)`.
    pub fn new(stretch: f32) -> PhaseVocoder {
        let pitch_factor = if stretch < 0.0 {
            stretch * 0.25
        } else {
            stretch
        };
        let hann = (0..BLOCK_SIZE)
            .map(|i| 0.5 * (1.0 - (TAU * i as f32 / BLOCK_SIZE as f32).cos()))
            .collect();
        PhaseVocoder {
            fft: Fft::new(BLOCK_SIZE),
            hann,
            left: Channel::new(),
            right: Channel::new(),
            time_cursor: 0.0,
            pitch_factor: (pitch_factor + 1.0).max(0.0),
        }
    }

    /// The number of samples this processes at a time.
    pub const BLOCK: usize = HOP_SIZE;

    /// Process one hop of stereo audio in place.
    pub fn process(&mut self, l: &mut [f32; HOP_SIZE], r: &mut [f32; HOP_SIZE]) {
        // `ola-processor.js::process`, in order, for both channels.
        for (ch, block) in [(&mut self.left, &mut *l), (&mut self.right, &mut *r)] {
            // readInputs: new block lands past the analysis window.
            ch.input[BLOCK_SIZE..].copy_from_slice(block);
            // shiftInputBuffers: copyWithin(0, hopSize).
            ch.input.copy_within(HOP_SIZE.., 0);
            // prepareInputBuffersToSend.
            ch.frame.copy_from_slice(&ch.input[..BLOCK_SIZE]);

            process_ola(
                &self.fft,
                &self.hann,
                ch,
                self.pitch_factor,
                self.time_cursor,
            );

            // handleOutputBuffersToRetrieve: accumulate the new frame.
            for (o, s) in ch.output.iter_mut().zip(ch.frame.iter()) {
                *o += *s / OVERLAPS;
            }
            // writeOutputs.
            block.copy_from_slice(&ch.output[..HOP_SIZE]);
            // shiftOutputBuffers.
            ch.output.copy_within(HOP_SIZE.., 0);
            ch.output[BLOCK_SIZE - HOP_SIZE..].fill(0.0);
        }
        self.time_cursor += HOP_SIZE as f32;
    }
}

/// `PhaseVocoderProcessor::processOLA` for one channel: analyse `ch.frame` and
/// leave the synthesised frame back in `ch.frame`.
fn process_ola(fft: &Fft, hann: &[f32], ch: &mut Channel, pitch_factor: f32, time_cursor: f32) {
    // applyHannWindow (the 1.62 factor is upstream's).
    for (v, w) in ch.frame.iter_mut().zip(hann) {
        *v *= w * 1.62;
    }

    // realTransform.
    ch.re.copy_from_slice(&ch.frame);
    ch.im.fill(0.0);
    fft.forward(&mut ch.re, &mut ch.im);

    // computeMagnitudes: squared, since only peak ordering matters.
    for (i, m) in ch.magnitudes.iter_mut().enumerate() {
        *m = ch.re[i] * ch.re[i] + ch.im[i] * ch.im[i];
    }

    // findPeaks: a bin strictly greater than its two neighbours either side.
    ch.peaks.clear();
    let end = ch.magnitudes.len() - 2;
    let mut i = 2;
    while i < end {
        let mag = ch.magnitudes[i];
        if ch.magnitudes[i - 1] >= mag
            || ch.magnitudes[i - 2] >= mag
            || ch.magnitudes[i + 1] >= mag
            || ch.magnitudes[i + 2] >= mag
        {
            i += 1;
            continue;
        }
        ch.peaks.push(i);
        i += 2;
    }

    shift_peaks(ch, pitch_factor, time_cursor);

    // completeSpectrum: mirror bins 1..N/2 as conjugates.
    for k in 1..BLOCK_SIZE / 2 {
        ch.shifted_re[BLOCK_SIZE - k] = ch.shifted_re[k];
        ch.shifted_im[BLOCK_SIZE - k] = -ch.shifted_im[k];
    }

    // inverseTransform + fromComplexArray (take the real part).
    ch.re.copy_from_slice(&ch.shifted_re);
    ch.im.copy_from_slice(&ch.shifted_im);
    fft.inverse(&mut ch.re, &mut ch.im);
    ch.frame.copy_from_slice(&ch.re);

    // applyHannWindow again (synthesis window).
    for (v, w) in ch.frame.iter_mut().zip(hann) {
        *v *= w * 1.62;
    }
}

/// `PhaseVocoderProcessor::shiftPeaks`.
fn shift_peaks(ch: &mut Channel, pitch_factor: f32, time_cursor: f32) {
    ch.shifted_re.fill(0.0);
    ch.shifted_im.fill(0.0);
    let n_bins = ch.magnitudes.len();

    for idx in 0..ch.peaks.len() {
        let peak = ch.peaks[idx] as i32;
        let peak_shifted = fround(peak as f32 * pitch_factor);
        if peak_shifted as usize > n_bins {
            break;
        }
        // Region of influence: halfway to each neighbouring peak.
        let start = if idx > 0 {
            peak - fround((peak - ch.peaks[idx - 1] as i32) as f32 / 2.0)
        } else {
            0
        };
        let end = if idx + 1 < ch.peaks.len() {
            peak + fceil((ch.peaks[idx + 1] as i32 - peak) as f32 / 2.0)
        } else {
            BLOCK_SIZE as i32
        };

        let omega_delta = TAU / BLOCK_SIZE as f32 * (peak_shifted - peak) as f32;
        let phase_re = (omega_delta * time_cursor).cos();
        let phase_im = (omega_delta * time_cursor).sin();

        for j in (start - peak)..(end - peak) {
            let bin = peak + j;
            let bin_shifted = peak_shifted + j;
            if bin_shifted as usize >= n_bins {
                break;
            }
            // Upstream reads a Float32Array, so a negative index yields
            // `undefined` and the arithmetic goes NaN; in practice `startIndex`
            // never goes below 0 for the first peak (it is pinned to 0) and the
            // halfway rule keeps later ones in range. Guard anyway.
            if bin < 0 || bin_shifted < 0 {
                continue;
            }
            let (bin, bin_shifted) = (bin as usize, bin_shifted as usize);
            let vr = ch.re[bin];
            let vi = ch.im[bin];
            ch.shifted_re[bin_shifted] += vr * phase_re - vi * phase_im;
            ch.shifted_im[bin_shifted] += vr * phase_im + vi * phase_re;
        }
    }
}

/// Sample-at-a-time adapter around [`PhaseVocoder`], for the per-sample voice
/// chain. Buffers a hop of input, processes it, then drains it — so the stage
/// adds `HOP_SIZE` samples of latency on top of the vocoder's own (superdough
/// compensates for the total by starting a stretched voice 0.04s early; Rudel's
/// scheduler has no per-effect pre-roll, so a stretched voice is that fraction
/// of a beat late).
pub struct StretchStage {
    vocoder: PhaseVocoder,
    in_l: [f32; HOP_SIZE],
    in_r: [f32; HOP_SIZE],
    out_l: [f32; HOP_SIZE],
    out_r: [f32; HOP_SIZE],
    /// Write cursor into the input hop; also the read cursor into the output.
    pos: usize,
}

impl StretchStage {
    pub fn new(stretch: f32) -> StretchStage {
        StretchStage {
            vocoder: PhaseVocoder::new(stretch),
            in_l: [0.0; HOP_SIZE],
            in_r: [0.0; HOP_SIZE],
            out_l: [0.0; HOP_SIZE],
            out_r: [0.0; HOP_SIZE],
            pos: 0,
        }
    }

    /// Push one stereo sample in, get one (delayed) stereo sample out.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.in_l[self.pos] = l;
        self.in_r[self.pos] = r;
        let out = (self.out_l[self.pos], self.out_r[self.pos]);
        self.pos += 1;
        if self.pos == HOP_SIZE {
            self.pos = 0;
            self.out_l = self.in_l;
            self.out_r = self.in_r;
            self.vocoder.process(&mut self.out_l, &mut self.out_r);
        }
        out
    }

    /// Samples of latency this stage introduces before the first output.
    pub const LATENCY: usize = HOP_SIZE;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a sine through the vocoder and return the settled output.
    fn run(stretch: f32, freq: f32, samples: usize) -> Vec<f32> {
        let mut v = PhaseVocoder::new(stretch);
        let mut out = Vec::with_capacity(samples);
        let mut n = 0usize;
        while out.len() < samples {
            let mut l = [0.0f32; HOP_SIZE];
            let mut r = [0.0f32; HOP_SIZE];
            for i in 0..HOP_SIZE {
                let s = (TAU * freq * (n + i) as f32 / 44100.0).sin();
                l[i] = s;
                r[i] = s;
            }
            n += HOP_SIZE;
            v.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    /// Dominant frequency of `buf` by peak magnitude of its spectrum.
    fn dominant_hz(buf: &[f32]) -> f32 {
        let n = 4096.min(buf.len().next_power_of_two() / 2 * 2);
        let fft = Fft::new(n);
        let mut re: Vec<f32> = buf[buf.len() - n..].to_vec();
        let mut im = vec![0.0; n];
        // Hann-window so the peak is not smeared by the rectangular edges.
        for (i, v) in re.iter_mut().enumerate() {
            *v *= 0.5 * (1.0 - (TAU * i as f32 / n as f32).cos());
        }
        fft.forward(&mut re, &mut im);
        let (best, _) = (1..n / 2)
            .map(|k| (k, re[k] * re[k] + im[k] * im[k]))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();
        best as f32 * 44100.0 / n as f32
    }

    #[test]
    fn unity_stretch_passes_the_pitch_through() {
        // stretch 0 -> pitchFactor 1: the peaks land back where they started.
        let out = run(0.0, 440.0, BLOCK_SIZE * 4);
        assert!(
            (dominant_hz(&out) - 440.0).abs() < 20.0,
            "expected ~440Hz, got {}",
            dominant_hz(&out)
        );
    }

    #[test]
    fn positive_stretch_shifts_the_pitch_up() {
        // stretch 1 -> pitchFactor 2: an octave up.
        let out = run(1.0, 440.0, BLOCK_SIZE * 4);
        let hz = dominant_hz(&out);
        assert!((hz - 880.0).abs() < 40.0, "expected ~880Hz, got {hz}");
    }

    #[test]
    fn negative_stretch_shifts_the_pitch_down() {
        // stretch -1 -> pitchFactor max(0, -0.25 + 1) = 0.75.
        let out = run(-1.0, 880.0, BLOCK_SIZE * 4);
        let hz = dominant_hz(&out);
        assert!((hz - 660.0).abs() < 40.0, "expected ~660Hz, got {hz}");
    }

    #[test]
    fn the_per_sample_stage_matches_the_block_vocoder() {
        // Same input through both paths; the stage is the block output delayed
        // by one hop.
        let mut stage = StretchStage::new(1.0);
        let mut block = PhaseVocoder::new(1.0);
        let sig = |n: usize| (TAU * 330.0 * n as f32 / 44100.0).sin();

        let mut staged = Vec::new();
        for n in 0..HOP_SIZE * 6 {
            staged.push(stage.process(sig(n), sig(n)).0);
        }
        let mut blocked = Vec::new();
        for h in 0..6 {
            let mut l = [0.0f32; HOP_SIZE];
            let mut r = [0.0f32; HOP_SIZE];
            for i in 0..HOP_SIZE {
                l[i] = sig(h * HOP_SIZE + i);
                r[i] = l[i];
            }
            block.process(&mut l, &mut r);
            blocked.extend_from_slice(&l);
        }
        for i in 0..HOP_SIZE * 5 {
            let (a, b) = (staged[i + HOP_SIZE], blocked[i]);
            assert!((a - b).abs() < 1e-6, "sample {i}: {a} != {b}");
        }
    }

    #[test]
    fn output_stays_bounded() {
        let out = run(1.0, 440.0, BLOCK_SIZE * 4);
        let peak = out.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak.is_finite() && peak < 4.0, "runaway output: {peak}");
    }
}
