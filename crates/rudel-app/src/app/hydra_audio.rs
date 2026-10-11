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
    use rstest::rstest;

    /// Meyda's own `loudness.specific`, from `tools/oracle/gen_loudness_oracle.mjs`.
    #[rstest]
    fn loudness_matches_meyda(
        #[values("tone_200", "tone_1000", "tone_4000", "tone_12000", "noise", "ramp")] signal: &str,
        #[values(44100, 48000)] rate: u32,
    ) {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tools/oracle/loudness_golden.json"
        ))
        .expect("parse loudness_golden.json");
        let case = &golden["cases"][format!("{signal}_{rate}")];
        let floats = |v: &serde_json::Value| -> Vec<f32> {
            v.as_array()
                .expect("an array")
                .iter()
                .map(|x| x.as_f64().expect("a number") as f32)
                .collect()
        };
        let input: [f32; BUFFER_SIZE] = floats(&case["input"]).try_into().expect("512 samples");
        let want = floats(&case["specific"]);
        let got = loudness(&input, rate as f32);
        // Compared as the summed amplitudes the bands are `^0.23` of: rudel's
        // FFT is f32 and Meyda's f64, which in a near-silent band is a tiny
        // error in the sum that `^0.23` blows up to a few percent.
        let sum = |b: f32| b.powf(1.0 / 0.23);
        let loudest = want.iter().copied().map(sum).fold(0.0, f32::max);
        for (band, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (sum(*g) - sum(*w)).abs() <= 1e-5 * loudest,
                "band {band}: rudel {g}, meyda {w}\nrudel {got:?}\nmeyda {want:?}"
            );
        }
    }

    #[test]
    fn the_tap_is_measured_as_it_sounds() {
        let mut app = crate::app::RudelApp::headless();
        let (engine, output) = rudel_audio::Engine::with_fake_output(48_000.0);
        app.engine = Some(engine);
        app.current = Some(rudel_lang::eval(r#"note("c4").s("sine")"#).unwrap());
        app.playing = true;
        app.route();
        let engine = app.engine.as_ref().unwrap();
        let tap = engine.scope_taps().master();
        let mut samples = [0.0f32; BUFFER_SIZE];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while samples.iter().all(|&s| s.abs() < 1e-3) {
            assert!(
                std::time::Instant::now() < deadline,
                "the sine never sounded"
            );
            output.pull(BUFFER_SIZE);
            tap.latest(&mut samples);
        }
        // The device moves only when pulled, so the tap holds still from here.
        let heard = tap_loudness(tap, engine.sample_rate());
        assert_eq!(heard, loudness(&samples, engine.sample_rate()));
        assert!(heard.iter().any(|&b| b > 1.0), "{heard:?}");
    }

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
