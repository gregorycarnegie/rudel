// bandlimited.rs - Web Audio's built-in oscillator waveforms, band-limited.
//
// superdough plays `square`/`sawtooth`/`triangle` through an `OscillatorNode`,
// which the spec defines as a PeriodicWave of the waveform's Fourier series,
// normalized to peak 1, with the partials that would alias left out. This
// ports Chrome's construction of it (Blink's periodic_wave_handler.cc and
// oscillator_handler.cc), where most Strudel is played:
//
// - one table of `size` samples (4096 at 44.1/48 kHz) per pitch range, three
//   ranges to the octave, range `r` keeping `size/2 · 2^(-r/3)` partials;
// - every table scaled by the peak of the first (all partials);
// - a sample blends the two tables around the oscillator's pitch, read with
//   linear interpolation (3- or 5-point Lagrange at LFO rates).
//
// The tables depend only on the shape and size, so they are built once per
// process; the sample rate only decides which ones a frequency reads.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fft::Fft;
use crate::oscillator::Waveform;
use std::sync::OnceLock;

/// Ranges per octave (`kNumberOfOctaveBands`).
const BANDS_PER_OCTAVE: f32 = 3.0;
/// Below these table-sample increments per output sample, Chrome reads with
/// 3- and then 5-point Lagrange interpolation (`kInterpolate2Point`/`3Point`).
const INTERPOLATE_2_POINT: f32 = 0.3;
const INTERPOLATE_3_POINT: f32 = 0.16;

/// One shape's band-limited tables at one size.
pub(crate) struct WaveTables {
    size: usize,
    ranges: Vec<Box<[f32]>>,
}

/// `PeriodicWaveSize`: shorter tables at low rates, longer ones above 88.2 kHz.
fn table_size(sample_rate: f32) -> usize {
    if sample_rate <= 24000.0 {
        2048
    } else if sample_rate <= 88200.0 {
        4096
    } else {
        16384
    }
}

/// The waveform's sine-series coefficients `b[n]` for `n` in `1..size/2`
/// (`GenerateBasicWaveform`; every shape is odd, so the cosine terms are 0).
fn coefficient(shape: Waveform, n: usize) -> f32 {
    let pi_factor = 2.0 / (n as f32 * std::f32::consts::PI);
    let odd = n & 1 == 1;
    match shape {
        Waveform::Square | Waveform::Pulse => {
            if odd {
                2.0 * pi_factor
            } else {
                0.0
            }
        }
        Waveform::Saw => pi_factor * if odd { 1.0 } else { -1.0 },
        Waveform::Triangle => {
            if odd {
                2.0 * pi_factor * pi_factor * if (n - 1) >> 1 & 1 == 1 { -1.0 } else { 1.0 }
            } else {
                0.0
            }
        }
        Waveform::Sine => {
            if n == 1 {
                1.0
            } else {
                0.0
            }
        }
    }
}

impl WaveTables {
    fn build(shape: Waveform, size: usize) -> WaveTables {
        let half = size / 2;
        let fft = Fft::new(size);
        let number_of_ranges = (0.5 + BANDS_PER_OCTAVE * (size as f32).log2()) as usize;
        let mut scale = 1.0;
        let mut ranges = Vec::with_capacity(number_of_ranges);
        for range in 0..number_of_ranges {
            // `NumberOfPartialsForRange`: cull a third of an octave per range.
            let culling = 2f64.powf(-(range as f64) / BANDS_PER_OCTAVE as f64);
            let partials = (culling as f32 * half as f32) as usize;
            // x[n] = Σ b[k]·sin(2πkn/N) as the real part of a forward FFT of
            // Z[k] = i·b/2, Z[N-k] = -i·b/2.
            let mut re = vec![0.0f32; size];
            let mut im = vec![0.0f32; size];
            for k in 1..half.min(partials + 1) {
                let b = coefficient(shape, k);
                im[k] = 0.5 * b;
                im[size - k] = -0.5 * b;
            }
            fft.forward(&mut re, &mut im);
            if range == 0 {
                let peak = re.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
                if peak > 0.0 {
                    scale = 1.0 / peak;
                }
            }
            for x in &mut re {
                *x *= scale;
            }
            ranges.push(re.into_boxed_slice());
        }
        WaveTables { size, ranges }
    }

    /// The tables for `shape` at `sample_rate`, built on first use.
    pub(crate) fn get(shape: Waveform, sample_rate: f32) -> &'static WaveTables {
        static TABLES: [[OnceLock<WaveTables>; 3]; 4] =
            [const { [const { OnceLock::new() }; 3] }; 4];
        let size = table_size(sample_rate);
        let shape_index = match shape {
            Waveform::Saw => 0,
            Waveform::Square | Waveform::Pulse => 1,
            Waveform::Triangle => 2,
            Waveform::Sine => 3,
        };
        let size_index = match size {
            2048 => 0,
            4096 => 1,
            _ => 2,
        };
        TABLES[shape_index][size_index].get_or_init(|| WaveTables::build(shape, size))
    }

    /// Which two tables an oscillator `cycles_per_sample` (`freq / sr`) reads,
    /// and how much of each (`WaveDataForFundamentalFrequency`).
    pub(crate) fn pitch(&self, cycles_per_sample: f32) -> Pitch {
        // Table samples per output sample; also the frequency's ratio to the
        // lowest fundamental, `sr / size`.
        let incr = (cycles_per_sample * self.size as f32).abs();
        let ratio = if incr > 0.0 { incr } else { 0.5 };
        let last = self.ranges.len() - 1;
        let pitch_range = (1.0 + ratio.log2() * BANDS_PER_OCTAVE).clamp(0.0, last as f32);
        let higher = pitch_range as usize;
        Pitch {
            cycles: cycles_per_sample,
            incr,
            higher,
            lower: (higher + 1).min(last),
            factor: pitch_range - higher as f32,
        }
    }

    /// The waveform at `phase` (0..1) for an oscillator advancing
    /// `cycles_per_sample` a sample.
    #[cfg(test)]
    pub(crate) fn sample(&self, phase: f32, cycles_per_sample: f32) -> f32 {
        self.sample_at(phase, &self.pitch(cycles_per_sample))
    }

    /// [`sample`](Self::sample), reusing `cache` while the pitch holds: the
    /// table choice only moves when the frequency does.
    pub(crate) fn sample_cached(
        &self,
        phase: f32,
        cycles_per_sample: f32,
        cache: &mut Pitch,
    ) -> f32 {
        if cache.cycles != cycles_per_sample {
            *cache = self.pitch(cycles_per_sample);
        }
        self.sample_at(phase, cache)
    }

    /// `DoInterpolation`: read both tables at `phase`, then blend them.
    fn sample_at(&self, phase: f32, pitch: &Pitch) -> f32 {
        let (higher, lower) = (&self.ranges[pitch.higher], &self.ranges[pitch.lower]);
        let mask = self.size - 1;
        let phase = if (0.0..1.0).contains(&phase) {
            phase
        } else {
            phase.rem_euclid(1.0)
        };
        let factor = pitch.factor;
        if pitch.incr >= INTERPOLATE_2_POINT {
            let index = phase * self.size as f32;
            let i0 = index as usize;
            let t = index - i0 as f32;
            let (i0, i1) = (i0 & mask, (i0 + 1) & mask);
            let h = (1.0 - t) * higher[i0] + t * higher[i1];
            let l = (1.0 - t) * lower[i0] + t * lower[i1];
            return (1.0 - factor) * h + factor * l;
        }
        let index = phase as f64 * self.size as f64;
        let i0 = index as usize;
        let t = index - i0 as f64;
        let read = |table: &[f32]| -> f64 {
            let at = |k: isize| table[(i0 as isize + k) as usize & mask] as f64;
            if pitch.incr >= INTERPOLATE_3_POINT {
                0.5 * t * (t - 1.0) * at(-1) + (1.0 - t * t) * at(0) + 0.5 * t * (t + 1.0) * at(1)
            } else {
                let t2 = t * t;
                t * (t2 - 1.0) * (t - 2.0) / 24.0 * at(-2)
                    - t * (t - 1.0) * (t2 - 4.0) / 6.0 * at(-1)
                    + (t2 - 1.0) * (t2 - 4.0) / 4.0 * at(0)
                    - t * (t + 1.0) * (t2 - 4.0) / 6.0 * at(1)
                    + t * (t2 - 1.0) * (t + 2.0) / 24.0 * at(2)
            }
        };
        ((1.0 - factor) * read(higher) as f32) + factor * read(lower) as f32
    }
}

/// The two tables an oscillator reads at one pitch and their blend; kept per
/// oscillator so it is worked out only when the pitch moves.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pitch {
    cycles: f32,
    incr: f32,
    higher: usize,
    lower: usize,
    factor: f32,
}

impl Default for Pitch {
    /// Matches no pitch, so the first sample works one out.
    fn default() -> Pitch {
        Pitch {
            cycles: f32::NAN,
            incr: 0.0,
            higher: 0,
            lower: 0,
            factor: 0.0,
        }
    }
}

/// Build every shape's tables for `sample_rate` now, so the first note to need
/// them does not build them on the audio thread.
pub fn prepare_oscillators(sample_rate: f32) {
    for shape in [Waveform::Saw, Waveform::Square, Waveform::Triangle] {
        WaveTables::get(shape, sample_rate);
    }
}
