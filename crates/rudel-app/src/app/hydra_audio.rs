//! What hydra's `a` hears: Meyda's `loudness` of rudel's own output.
//!
//! Upstream hydra-synth listens to the microphone through a Meyda analyser
//! (`lib/audio.js`) and reads `loudness.specific`, 24 Bark bands, every frame.
//! This is that feature extractor, ported (`meyda/extractors/loudness.js`,
//! `utilities.createBarkScale`, the Hann window and 512-sample buffer it
//! defaults to), fed from the master mix instead.

use rudel_audio::{Fft, ScopeTap};
use std::sync::LazyLock;

/// Meyda's default `bufferSize`.
const BUFFER_SIZE: usize = 512;
const BARK_BANDS: usize = 24;

static FFT: LazyLock<Fft> = LazyLock::new(|| Fft::new(BUFFER_SIZE));

/// The latest `loudness.specific` of `tap` at `sample_rate`.
pub(super) fn tap_loudness(tap: &ScopeTap, sample_rate: f32) -> [f32; BARK_BANDS] {
    let mut samples = [0.0f32; BUFFER_SIZE];
    tap.latest(&mut samples);
    loudness(&samples, sample_rate)
}

fn loudness(samples: &[f32; BUFFER_SIZE], sample_rate: f32) -> [f32; BARK_BANDS] {
    // Meyda's `hanning`, with its `size - 1` denominator.
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (BUFFER_SIZE - 1) as f32).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; BUFFER_SIZE];
    FFT.forward(&mut re, &mut im);
    let amp: Vec<f32> = (0..BUFFER_SIZE / 2)
        .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
        .collect();

    // `createBarkScale(bufferSize, sampleRate, bufferSize)`: only its first
    // `amp.len()` entries are ever read.
    let bark = |i: usize| {
        let f = i as f32 * sample_rate / BUFFER_SIZE as f32;
        13.0 * (f / 1315.8).atan() + 3.5 * (f / 7518.0).powi(2).atan()
    };
    let last = bark(amp.len() - 1);
    let mut limits = [0usize; BARK_BANDS + 1];
    let mut band = 1;
    let mut band_end = last / BARK_BANDS as f32;
    for i in 0..amp.len() {
        while bark(i) > band_end && band < BARK_BANDS {
            limits[band] = i;
            band += 1;
            band_end = band as f32 * last / BARK_BANDS as f32;
        }
    }
    limits[BARK_BANDS] = amp.len() - 1;

    let mut specific = [0.0f32; BARK_BANDS];
    for (i, out) in specific.iter_mut().enumerate() {
        let sum: f32 = amp[limits[i]..limits[i + 1]].iter().sum();
        *out = sum.powf(0.23);
    }
    specific
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_quiet_and_a_tone_is_loud_in_its_own_band() {
        assert_eq!(loudness(&[0.0; BUFFER_SIZE], 48_000.0), [0.0; BARK_BANDS]);

        let tone = |freq: f32| {
            let mut s = [0.0f32; BUFFER_SIZE];
            for (i, v) in s.iter_mut().enumerate() {
                *v = (std::f32::consts::TAU * freq * i as f32 / 48_000.0).sin();
            }
            loudness(&s, 48_000.0)
        };
        let loudest = |bands: [f32; BARK_BANDS]| {
            (0..BARK_BANDS)
                .max_by(|&a, &b| bands[a].total_cmp(&bands[b]))
                .unwrap()
        };
        // Bark bands rise with pitch: a low tone peaks lower than a high one.
        let (low, high) = (loudest(tone(200.0)), loudest(tone(4000.0)));
        assert!(low < high, "{low} vs {high}");
        assert!(
            tone(1000.0).iter().sum::<f32>() > 10.0,
            "a full-scale tone is loud"
        );
    }
}
