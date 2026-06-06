use crate::filter::Biquad;
use crate::voice::VoiceLike;
use rudel_core::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vowel {
    A,
    E,
    I,
    O,
    U,
}

impl Vowel {
    pub fn from_name(name: &str) -> Option<Vowel> {
        Some(match name {
            "a" => Vowel::A,
            "e" => Vowel::E,
            "i" => Vowel::I,
            "o" => Vowel::O,
            "u" => Vowel::U,
            _ => return None,
        })
    }

    /// Five formants as `(frequency, gain, Q)` (webdirt/superdough table).
    fn formants(self) -> [(f32, f32, f32); 5] {
        match self {
            Vowel::A => [
                (660.0, 1.0, 80.0),
                (1120.0, 0.5012, 90.0),
                (2750.0, 0.0708, 120.0),
                (3000.0, 0.0631, 130.0),
                (3350.0, 0.0126, 140.0),
            ],
            Vowel::E => [
                (440.0, 1.0, 70.0),
                (1800.0, 0.1995, 80.0),
                (2700.0, 0.1259, 100.0),
                (3000.0, 0.1, 120.0),
                (3300.0, 0.1, 120.0),
            ],
            Vowel::I => [
                (270.0, 1.0, 40.0),
                (1850.0, 0.0631, 90.0),
                (2900.0, 0.0631, 100.0),
                (3350.0, 0.0158, 120.0),
                (3590.0, 0.0158, 120.0),
            ],
            Vowel::O => [
                (430.0, 1.0, 40.0),
                (820.0, 0.3162, 80.0),
                (2700.0, 0.0501, 100.0),
                (3000.0, 0.0794, 120.0),
                (3300.0, 0.01995, 120.0),
            ],
            Vowel::U => [
                (370.0, 1.0, 40.0),
                (630.0, 0.1, 60.0),
                (2750.0, 0.0708, 100.0),
                (3000.0, 0.0316, 120.0),
                (3400.0, 0.01995, 120.0),
            ],
        }
    }
}

/// A mono bank of five parallel band-pass formant filters.
#[derive(Clone)]
struct Formant {
    filters: [Biquad; 5],
    gains: [f32; 5],
}

impl Formant {
    fn new(vowel: Vowel, sample_rate: f32) -> Formant {
        let f = vowel.formants();
        Formant {
            filters: std::array::from_fn(|i| Biquad::bandpass(sample_rate, f[i].0, f[i].2)),
            gains: std::array::from_fn(|i| f[i].1),
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let mut sum = 0.0;
        for i in 0..5 {
            sum += self.filters[i].process(x) * self.gains[i];
        }
        sum * 8.0 // makeup gain (matches superdough's VowelNode)
    }
}

/// Per-voice post-effects (`crush`, `shape`, `distort`, `coarse`, `postgain`,
/// `vowel`).
#[derive(Clone, Copy, Debug)]
pub struct PostFx {
    /// Bit-crush depth in bits (>= 1). `None` = off.
    pub crush: Option<f32>,
    /// Waveshaper amount 0..1 (`shape`). `None` = off.
    pub shape: Option<f32>,
    /// Post-gain for `shape` (`shapevol`), 0.001..1.
    pub shapevol: f32,
    /// Distortion amount (`distort`); `k = e^distort - 1`. `None` = off.
    pub distort: Option<f32>,
    /// Post-gain for `distort` (`distortvol`), 0.001..1.
    pub distortvol: f32,
    /// Sample-rate reduction factor (`coarse`, >= 1). `None` = off.
    pub coarse: Option<f32>,
    /// Overall post-gain (`postgain`).
    pub postgain: f32,
    /// Formant filter vowel (`vowel`).
    pub vowel: Option<Vowel>,
    /// Tremolo (amplitude LFO) rate in Hz (`tremolo`). `None` = off.
    pub tremolo: Option<f32>,
    /// Tremolo depth 0..1 (`tremolodepth`).
    pub tremolodepth: f32,
    /// Phaser (swept notch) LFO rate in Hz (`phaser`/`phaserrate`). `None` = off.
    pub phaser: Option<f32>,
    /// Phaser depth 0..1 (`phaserdepth`), controls notch Q.
    pub phaserdepth: f32,
    /// Phaser notch center frequency in Hz (`phasercenter`).
    pub phasercenter: f32,
    /// Phaser sweep range in cents (`phasersweep`).
    pub phasersweep: f32,
}

impl Default for PostFx {
    fn default() -> Self {
        PostFx {
            crush: None,
            shape: None,
            shapevol: 1.0,
            distort: None,
            distortvol: 1.0,
            coarse: None,
            postgain: 1.0,
            vowel: None,
            tremolo: None,
            tremolodepth: 0.5,
            phaser: None,
            phaserdepth: 0.5,
            phasercenter: 1000.0,
            phasersweep: 2000.0,
        }
    }
}

impl PostFx {
    pub fn from_controls(map: &BTreeMap<String, Value>) -> PostFx {
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        PostFx {
            crush: get("crush"),
            shape: get("shape"),
            shapevol: get("shapevol").unwrap_or(1.0),
            distort: get("distort"),
            distortvol: get("distortvol").unwrap_or(1.0),
            coarse: get("coarse"),
            postgain: get("postgain").unwrap_or(1.0),
            vowel: map
                .get("vowel")
                .and_then(|v| v.as_str())
                .and_then(Vowel::from_name),
            tremolo: get("tremolo"),
            tremolodepth: get("tremolodepth").unwrap_or(0.5),
            // `phaser` and `phaserrate` are aliases for the LFO rate.
            phaser: get("phaser").or_else(|| get("phaserrate")),
            phaserdepth: get("phaserdepth").unwrap_or(0.5),
            phasercenter: get("phasercenter").unwrap_or(1000.0),
            phasersweep: get("phasersweep").unwrap_or(2000.0),
        }
    }

    pub fn is_active(&self) -> bool {
        self.crush.is_some()
            || self.shape.is_some()
            || self.distort.is_some()
            || self.coarse.is_some()
            || self.vowel.is_some()
            || self.postgain != 1.0
            || self.tremolo.is_some()
            || self.phaser.is_some()
    }
}

/// Wraps a voice and applies [`PostFx`] to its stereo output.
pub struct PostFxVoice {
    inner: Box<dyn VoiceLike>,
    fx: PostFx,
    sample_rate: f32,
    /// Elapsed time in seconds, driving the tremolo / phaser LFOs.
    time: f32,
    coarse_hold: (f32, f32),
    coarse_count: u32,
    /// Per-channel formant banks when `vowel` is set.
    vowel: Option<(Formant, Formant)>,
    /// Per-channel swept notch filters when `phaser` is set.
    phaser: Option<(Biquad, Biquad)>,
}

impl PostFxVoice {
    pub fn new(inner: Box<dyn VoiceLike>, fx: PostFx, sample_rate: f32) -> PostFxVoice {
        let vowel = fx
            .vowel
            .map(|v| (Formant::new(v, sample_rate), Formant::new(v, sample_rate)));
        let phaser = fx.phaser.map(|_| {
            let center = fx.phasercenter + 282.0;
            let q = 2.0 - (fx.phaserdepth * 2.0).clamp(0.0, 1.9);
            (
                Biquad::notch(sample_rate, center, q),
                Biquad::notch(sample_rate, center, q),
            )
        });
        PostFxVoice {
            inner,
            fx,
            sample_rate,
            time: 0.0,
            coarse_hold: (0.0, 0.0),
            coarse_count: 0,
            vowel,
            phaser,
        }
    }

    fn shape_sample(x: f32, shape: f32, postgain: f32) -> f32 {
        let shape = if shape < 1.0 { shape } else { 1.0 - 4e-10 };
        let shape = (2.0 * shape) / (1.0 - shape);
        ((1.0 + shape) * x) / (1.0 + shape * x.abs()) * postgain
    }

    // s-curve waveshaper (superdough's default `distort` algorithm).
    fn distort_sample(x: f32, k: f32, postgain: f32) -> f32 {
        postgain * ((1.0 + k) * x) / (1.0 + k * x.abs())
    }
}

impl VoiceLike for PostFxVoice {
    fn tick(&mut self) -> (f32, f32) {
        let (mut l, mut r) = self.inner.tick();

        // vowel: parallel formant band-pass bank.
        if let Some((fl, fr)) = &mut self.vowel {
            l = fl.process(l);
            r = fr.process(r);
        }
        // phaser: notch filter whose center sweeps ±phasersweep cents at the
        // LFO rate (matches superdough's notch-detune phaser).
        if let (Some(rate), Some((nl, nr))) = (self.fx.phaser, &mut self.phaser) {
            let lfo = (std::f32::consts::TAU * rate * self.time).sin();
            let center = self.fx.phasercenter + 282.0;
            let freq = center * 2f32.powf(self.fx.phasersweep * lfo / 1200.0);
            let q = 2.0 - (self.fx.phaserdepth * 2.0).clamp(0.0, 1.9);
            nl.set_notch(self.sample_rate, freq, q);
            nr.set_notch(self.sample_rate, freq, q);
            l = nl.process(l);
            r = nr.process(r);
        }
        // coarse: sample-and-hold every `coarse` output samples.
        if let Some(c) = self.fx.coarse {
            let c = c.max(1.0) as u32;
            if self.coarse_count.is_multiple_of(c) {
                self.coarse_hold = (l, r);
            } else {
                (l, r) = self.coarse_hold;
            }
            self.coarse_count = self.coarse_count.wrapping_add(1);
        }
        // crush: quantize to `crush` bits.
        if let Some(bits) = self.fx.crush {
            let x = 2f32.powf(bits.max(1.0) - 1.0);
            l = (l * x).round() / x;
            r = (r * x).round() / x;
        }
        // shape: hyperbolic waveshaper.
        if let Some(s) = self.fx.shape {
            let pg = self.fx.shapevol.clamp(0.001, 1.0);
            l = Self::shape_sample(l, s, pg);
            r = Self::shape_sample(r, s, pg);
        }
        // distort: s-curve with exponential drive.
        if let Some(d) = self.fx.distort {
            let k = d.exp_m1();
            let pg = self.fx.distortvol.clamp(0.001, 1.0);
            l = Self::distort_sample(l, k, pg);
            r = Self::distort_sample(r, k, pg);
        }
        // tremolo: amplitude LFO. gain = (1-depth) + depth * unipolar-sine.
        if let Some(rate) = self.fx.tremolo {
            let depth = self.fx.tremolodepth.clamp(0.0, 1.0);
            let unipolar = 0.5 * (1.0 - (std::f32::consts::TAU * rate * self.time).cos());
            let gain = (1.0 - depth) + depth * unipolar;
            l *= gain;
            r *= gain;
        }
        if self.fx.postgain != 1.0 {
            l *= self.fx.postgain;
            r *= self.fx.postgain;
        }
        self.time += 1.0 / self.sample_rate;
        (l, r)
    }
    fn is_done(&self) -> bool {
        self.inner.is_done()
    }
    fn room(&self) -> f32 {
        self.inner.room()
    }
    fn delay_send(&self) -> f32 {
        self.inner.delay_send()
    }
}
