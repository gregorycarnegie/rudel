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

/// Waveshaping algorithm selected by the `distorttype` control. The order
/// matches superdough's `distortionAlgorithms` key order, so a numeric
/// `distorttype` indexes the same algorithm (wrapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DistortAlgo {
    /// `scurve` — superdough's default `distort` curve (index 0).
    #[default]
    Scurve,
    Soft,
    Hard,
    Cubic,
    Diode,
    Asym,
    Fold,
    Sinefold,
    Chebyshev,
}

impl DistortAlgo {
    /// All algorithms in superdough's order; a numeric `distorttype` indexes
    /// this list (wrapping), a string names it.
    const ORDER: [DistortAlgo; 9] = [
        DistortAlgo::Scurve,
        DistortAlgo::Soft,
        DistortAlgo::Hard,
        DistortAlgo::Cubic,
        DistortAlgo::Diode,
        DistortAlgo::Asym,
        DistortAlgo::Fold,
        DistortAlgo::Sinefold,
        DistortAlgo::Chebyshev,
    ];

    fn from_name(name: &str) -> Option<DistortAlgo> {
        Some(match name {
            "scurve" => DistortAlgo::Scurve,
            "soft" => DistortAlgo::Soft,
            "hard" => DistortAlgo::Hard,
            "cubic" => DistortAlgo::Cubic,
            "diode" => DistortAlgo::Diode,
            "asym" => DistortAlgo::Asym,
            "fold" => DistortAlgo::Fold,
            "sinefold" => DistortAlgo::Sinefold,
            "chebyshev" => DistortAlgo::Chebyshev,
            _ => return None,
        })
    }

    /// Resolve from a control value: a string names the algorithm; a number
    /// indexes [`ORDER`](Self::ORDER) (wrapping, matching superdough's
    /// `getDistortionAlgorithm`). Unknown names fall back to the default.
    pub fn from_value(value: &Value) -> DistortAlgo {
        match value {
            Value::Str(s) => DistortAlgo::from_name(s).unwrap_or_default(),
            other => match other.as_f64() {
                Some(n) => {
                    let len = DistortAlgo::ORDER.len() as i64;
                    let idx = (n as i64).rem_euclid(len) as usize;
                    DistortAlgo::ORDER[idx]
                }
                None => DistortAlgo::default(),
            },
        }
    }

    /// Apply this waveshaper to a sample. `k = e^distort - 1` is the drive
    /// (`shape` in superdough's worklet). Ported sample-for-sample from
    /// `superdough/helpers.mjs`.
    pub fn shape(self, x: f32, k: f32) -> f32 {
        match self {
            DistortAlgo::Scurve => d_scurve(x, k),
            DistortAlgo::Soft => d_soft(x, k),
            DistortAlgo::Hard => d_hard(x, k),
            DistortAlgo::Cubic => d_cubic(x, k),
            DistortAlgo::Diode => d_diode(x, k, false),
            DistortAlgo::Asym => d_diode(x, k, true),
            DistortAlgo::Fold => d_fold(x, k),
            DistortAlgo::Sinefold => d_sinefold(x, k),
            DistortAlgo::Chebyshev => d_chebyshev(x, k),
        }
    }
}

/// `[0, inf) -> [0, 1)` squash used by the drive-dependent algorithms.
fn d_squash(x: f32) -> f32 {
    x / (1.0 + x)
}

fn d_scurve(x: f32, k: f32) -> f32 {
    ((1.0 + k) * x) / (1.0 + k * x.abs())
}

fn d_soft(x: f32, k: f32) -> f32 {
    (x * (1.0 + k)).tanh()
}

fn d_hard(x: f32, k: f32) -> f32 {
    ((1.0 + k) * x).clamp(-1.0, 1.0)
}

fn d_fold(x: f32, k: f32) -> f32 {
    let y = (1.0 + 0.5 * k) * x;
    // floored modulo, matching superdough's `_mod`.
    let window = (y + 1.0).rem_euclid(4.0);
    1.0 - (window - 2.0).abs()
}

fn d_sinefold(x: f32, k: f32) -> f32 {
    (std::f32::consts::FRAC_PI_2 * d_fold(x, k)).sin()
}

fn d_cubic(x: f32, k: f32) -> f32 {
    let t = d_squash(k.ln_1p());
    let cubic = (x - (t / 3.0) * x * x * x) / (1.0 - t / 3.0);
    d_soft(cubic, k)
}

fn d_diode(x: f32, k: f32, asym: bool) -> f32 {
    let g = 1.0 + 2.0 * k;
    let t = d_squash(k.ln_1p());
    let bias = 0.07 * t;
    let pos = d_soft(x + bias, 2.0 * k);
    let neg = d_soft(if asym { bias } else { -x + bias }, 2.0 * k);
    let y = pos - neg;
    let sech = 1.0 / (g * bias).cosh();
    let sech2 = sech * sech;
    let denom = (if asym { 1.0 } else { 2.0 } * g * sech2).max(1e-8);
    d_soft(y / denom, k)
}

fn d_chebyshev(x: f32, k: f32) -> f32 {
    let kl = 10.0 * k.ln_1p();
    let mut tnm1 = 1.0f32;
    let mut tnm2 = x;
    let mut y = 0.0f32;
    for i in 1..64 {
        if i < 2 {
            y += tnm2; // i == 1 (i == 0 is never reached)
            continue;
        }
        let tn = 2.0 * x * tnm1 - tnm2;
        tnm2 = tnm1;
        tnm1 = tn;
        if i % 2 == 0 {
            y += (1.3 * kl / i as f32).min(2.0) * tn;
        }
    }
    d_soft(y, kl / 20.0)
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
    /// Waveshaping algorithm (`distorttype`); set by `soft`/`hard`/`cubic`/…
    pub distort_alg: DistortAlgo,
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
    /// Dynamics-compressor threshold in dB (`compressor`). `None` = off.
    pub compressor: Option<f32>,
    /// Compression ratio (`compressorRatio`), default 10.
    pub comp_ratio: f32,
    /// Soft-knee width in dB (`compressorKnee`), default 10.
    pub comp_knee: f32,
    /// Attack time in seconds (`compressorAttack`), default 0.005.
    pub comp_attack: f32,
    /// Release time in seconds (`compressorRelease`), default 0.05.
    pub comp_release: f32,
}

impl Default for PostFx {
    fn default() -> Self {
        PostFx {
            crush: None,
            shape: None,
            shapevol: 1.0,
            distort: None,
            distortvol: 1.0,
            distort_alg: DistortAlgo::Scurve,
            coarse: None,
            postgain: 1.0,
            vowel: None,
            tremolo: None,
            tremolodepth: 0.5,
            phaser: None,
            phaserdepth: 0.75,
            phasercenter: 1000.0,
            phasersweep: 2000.0,
            compressor: None,
            comp_ratio: 10.0,
            comp_knee: 10.0,
            comp_attack: 0.005,
            comp_release: 0.05,
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
            distort_alg: map
                .get("distorttype")
                .map(DistortAlgo::from_value)
                .unwrap_or_default(),
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
            // superdough's getDefaultValue('phaserdepth') is 0.75.
            phaserdepth: get("phaserdepth").unwrap_or(0.75),
            phasercenter: get("phasercenter").unwrap_or(1000.0),
            phasersweep: get("phasersweep").unwrap_or(2000.0),
            // superdough's getCompressor defaults (only applied when the
            // `compressor` threshold key is present).
            compressor: get("compressor"),
            comp_ratio: get("compressorRatio").unwrap_or(10.0),
            comp_knee: get("compressorKnee").unwrap_or(10.0),
            comp_attack: get("compressorAttack").unwrap_or(0.005),
            comp_release: get("compressorRelease").unwrap_or(0.05),
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
            || self.compressor.is_some()
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
    /// Smoothed compressor gain (1.0 = no reduction), driven by attack/release.
    comp_gain: f32,
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
            comp_gain: 1.0,
        }
    }

    fn shape_sample(x: f32, shape: f32, postgain: f32) -> f32 {
        let shape = if shape < 1.0 { shape } else { 1.0 - 4e-10 };
        let shape = (2.0 * shape) / (1.0 - shape);
        ((1.0 + shape) * x) / (1.0 + shape * x.abs()) * postgain
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
        // phaser: notch filter whose detune sweeps ±phasersweep cents at the LFO
        // rate. Matches superdough's `getPhaser`: a single `notch` BiquadFilter
        // at `phasercenter + 282`, its `detune` driven by `getLfo` with the
        // default **triangle** shape (`waveshapes.tri`, shape 0), `dcoffset -0.5`
        // and `depth = sweep*2`, so detune = 2·sweep·(tri − 0.5) ∈ [−sweep, +sweep].
        // (The LFO is a triangle, not a sine, and its phase here starts at the
        // voice onset — superdough phase-locks it to the global clock via
        // `frac(begin·rate)`, which coincides for onsets at cycle 0.)
        if let (Some(rate), Some((nl, nr))) = (self.fx.phaser, &mut self.phaser) {
            let phase = (rate * self.time).rem_euclid(1.0);
            let tri = if phase < 0.5 {
                2.0 * phase
            } else {
                2.0 - 2.0 * phase
            };
            let detune = 2.0 * self.fx.phasersweep * (tri - 0.5); // cents, ±sweep
            let center = self.fx.phasercenter + 282.0;
            let freq = center * 2f32.powf(detune / 1200.0);
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
        // distort: waveshaper (selected by `distorttype`) with exponential
        // drive `k = e^distort - 1`, then postgain (superdough's DistortProcessor).
        if let Some(d) = self.fx.distort {
            let k = d.exp_m1();
            let pg = self.fx.distortvol.clamp(0.001, 1.0);
            let alg = self.fx.distort_alg;
            l = pg * alg.shape(l, k);
            r = pg * alg.shape(r, k);
        }
        // tremolo: amplitude LFO. gain = (1-depth) + depth * unipolar-sine.
        if let Some(rate) = self.fx.tremolo {
            let depth = self.fx.tremolodepth.clamp(0.0, 1.0);
            let unipolar = 0.5 * (1.0 - (std::f32::consts::TAU * rate * self.time).cos());
            let gain = (1.0 - depth) + depth * unipolar;
            l *= gain;
            r *= gain;
        }
        // compressor: feedforward soft-knee dynamics compressor, matching the
        // per-voice DynamicsCompressorNode superdough inserts in the fx chain
        // (`chain.connect(compressorNode)`). Stereo-linked (peak of |l|,|r|),
        // no makeup gain (WebAudio's node has none).
        if let Some(threshold) = self.fx.compressor {
            let level = l.abs().max(r.abs()).max(1e-9);
            let level_db = 20.0 * level.log10();
            let knee = self.fx.comp_knee.max(0.0);
            let ratio = self.fx.comp_ratio.max(1.0);
            let over = level_db - threshold;
            // static input→output level curve (dB), with a quadratic soft knee.
            let out_db = if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
                level_db + (1.0 / ratio - 1.0) * (over + knee / 2.0).powi(2) / (2.0 * knee)
            } else if over <= -knee / 2.0 {
                level_db
            } else {
                threshold + over / ratio
            };
            let target_gain = 10f32.powf((out_db - level_db) / 20.0); // <= 1.0
            // attack when reduction deepens (target below current), else release.
            let time = if target_gain < self.comp_gain {
                self.fx.comp_attack
            } else {
                self.fx.comp_release
            };
            let coeff = (-1.0 / (time.max(1e-4) * self.sample_rate)).exp();
            self.comp_gain = coeff * self.comp_gain + (1.0 - coeff) * target_gain;
            l *= self.comp_gain;
            r *= self.comp_gain;
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
    fn dry(&self) -> f32 {
        self.inner.dry()
    }
}
