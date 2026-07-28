use crate::{
    envelope::{Adsr, adsr_value},
    modulator::{ModBank, ModTarget},
};
use rudel_core::{Value, ValueMap};
use std::f32::consts::TAU;

#[derive(Clone, Copy)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

/// Which RBJ biquad to compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilterKind {
    Low,
    High,
    Band,
    Notch,
}

impl Biquad {
    fn new(kind: FilterKind, sample_rate: f32, freq: f32, q: f32) -> Biquad {
        let mut b = Biquad {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        };
        b.update(kind, sample_rate, freq, q);
        b
    }

    pub(crate) fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::Low, sample_rate, cutoff, q)
    }
    pub(crate) fn highpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::High, sample_rate, cutoff, q)
    }
    pub(crate) fn bandpass(sample_rate: f32, center: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::Band, sample_rate, center, q)
    }
    pub(crate) fn notch(sample_rate: f32, center: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::Notch, sample_rate, center, q)
    }

    /// Recompute notch coefficients in place (used to sweep the phaser).
    pub(crate) fn set_notch(&mut self, sample_rate: f32, freq: f32, q: f32) {
        self.update(FilterKind::Notch, sample_rate, freq, q);
    }

    /// Recompute lowpass coefficients in place (used to sweep the reverb IR's
    /// `roomlp` -> `roomdim` filter).
    pub(crate) fn set_lowpass(&mut self, sample_rate: f32, freq: f32, q: f32) {
        self.update(FilterKind::Low, sample_rate, freq, q);
    }

    /// Recompute the RBJ coefficients in place, preserving the filter state
    /// (`z1`/`z2`) so the cutoff can be modulated per sample.
    fn update(&mut self, kind: FilterKind, sample_rate: f32, freq: f32, q: f32) {
        let freq = freq.clamp(20.0, sample_rate * 0.45);
        let w0 = TAU * freq / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match kind {
            FilterKind::Low => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            FilterKind::High => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
            // constant 0 dB peak gain (b0 = alpha)
            FilterKind::Band => (alpha, 0.0, -alpha),
            FilterKind::Notch => (1.0, -2.0 * cos, 1.0),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cos) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    /// The transposed-direct-form-II coefficients `(b0, b1, b2, a1, a2)`, used to
    /// pack several independent biquads into SIMD lanes (see the vowel formant
    /// bank in `postfx.rs`).
    pub(crate) fn coeffs(&self) -> (f32, f32, f32, f32, f32) {
        (self.b0, self.b1, self.b2, self.a1, self.a2)
    }
}

/// Which filter model `ftype` selects. Superdough's list is
/// `['12db', 'ladder', '24db']`, so a numeric `ftype` indexes this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterModel {
    /// A single RBJ biquad (superdough's `'12db'`, the default).
    #[default]
    Db12,
    /// The Moog-style nonlinear ladder (superdough's `ladder-processor`).
    /// Always a lowpass, whichever slot it replaces — as upstream.
    Ladder,
    /// The biquad cascaded twice for a steeper slope (superdough's `'24db'`).
    Db24,
}

impl FilterModel {
    /// Resolve from an `ftype` control value: a string names the model, a
    /// number indexes `['12db', 'ladder', '24db']` (wrapping), matching
    /// superdough's `getFilterType`.
    pub fn from_value(v: &Value) -> FilterModel {
        match v {
            Value::Str(s) => match s.as_str() {
                "ladder" => FilterModel::Ladder,
                "24db" => FilterModel::Db24,
                _ => FilterModel::Db12,
            },
            other => match other.as_f64().map(|f| f.rem_euclid(3.0).floor() as i32) {
                Some(1) => FilterModel::Ladder,
                Some(2) => FilterModel::Db24,
                _ => FilterModel::Db12,
            },
        }
    }
}

/// superdough's `fast_tanh` rational approximation (`worklets.mjs`). The ladder
/// is ported against this, not `f32::tanh`, so the saturation curve matches.
fn fast_tanh(x: f32) -> f32 {
    let x2 = x * x;
    (x * (27.0 + x2)) / (27.0 + 9.0 * x2)
}

/// The Moog-style nonlinear ladder lowpass, ported sample-for-sample from
/// superdough's `ladder-processor` worklet (itself adapted from
/// <https://github.com/TheBouteillacBear/webaudioworklet-wasm>). Four
/// `tanh`-saturated one-pole stages with resonant feedback, read through a
/// fixed 4-tap smoothing FIR.
#[derive(Clone)]
pub struct Ladder {
    /// The four cascaded one-pole stage states (`p0`..`p3`).
    stages: [f32; 4],
    /// The previous three `p3` values (`p32`/`p33`/`p34`), feeding the output FIR.
    history: [f32; 3],
    /// Resonance feedback coefficient (`min(8, q * 0.13)`).
    k: f32,
    /// Input drive (`clamp(exp(drive), 0.1, 2000)`).
    drive: f32,
    /// Output gain compensating for drive and resonance loss.
    makeup: f32,
    /// Cutoff as a normalised one-pole coefficient (`min(1, 2πf/sr)`).
    cutoff: f32,
}

impl Ladder {
    pub fn new(sample_rate: f32, freq: f32, q: f32, drive: f32) -> Ladder {
        let k = (q * 0.13).min(8.0);
        let drive = drive.exp().clamp(0.1, 2000.0);
        let mut l = Ladder {
            stages: [0.0; 4],
            history: [0.0; 3],
            k,
            drive,
            // drive makeup * resonance volume-loss makeup
            makeup: (1.0 / drive) * (1.0 + k).min(1.75),
            cutoff: 0.0,
        };
        l.set_cutoff(sample_rate, freq);
        l
    }

    /// Retune the cutoff in place, preserving the stage states so the envelope
    /// can sweep it per sample.
    pub fn set_cutoff(&mut self, sample_rate: f32, freq: f32) {
        self.cutoff = (freq * TAU / sample_rate).min(1.0);
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let [p0, p1, p2, p3] = self.stages;
        let [p32, p33, p34] = self.history;
        let out = p3 * 0.360891 + p32 * 0.41729 + p33 * 0.177896 + p34 * 0.0439725;
        self.history = [p3, p32, p33];

        let c = self.cutoff;
        let p0 = p0 + (fast_tanh(x * self.drive - self.k * out) - fast_tanh(p0)) * c;
        let p1 = p1 + (fast_tanh(p0) - fast_tanh(p1)) * c;
        let p2 = p2 + (fast_tanh(p1) - fast_tanh(p2)) * c;
        let p3 = p3 + (fast_tanh(p2) - fast_tanh(p3)) * c;
        self.stages = [p0, p1, p2, p3];

        out * self.makeup
    }
}

/// Per-filter parameters (low/high/band) including an optional cutoff envelope.
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    /// Cutoff / center frequency in Hz; `None` disables this filter.
    pub freq: Option<f32>,
    pub q: f32,
    /// Envelope amount in octaves (`lpenv`/`hpenv`/`bpenv`).
    pub env: Option<f32>,
    pub attack: Option<f32>,
    pub decay: Option<f32>,
    pub sustain: Option<f32>,
    pub release: Option<f32>,
    /// `fanchor`: where the base cutoff sits within the sweep (0 = bottom).
    pub anchor: f32,
    /// `ftype`: which filter model to run.
    pub model: FilterModel,
    /// `drive`: ladder input drive (superdough's default 0.69). Unused by the
    /// biquad models.
    pub drive: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams {
            freq: None,
            q: 0.707,
            env: None,
            attack: None,
            decay: None,
            sustain: None,
            release: None,
            anchor: 0.0,
            model: FilterModel::Db12,
            drive: 0.69,
        }
    }
}

impl FilterParams {
    fn has_env(&self) -> bool {
        self.env.is_some()
            || self.attack.is_some()
            || self.decay.is_some()
            || self.sustain.is_some()
            || self.release.is_some()
    }
}

/// The three filter slots a voice runs, as superdough configures them from one
/// control map: `cutoff`/`resonance`, `hcutoff`/`hresonance`, `bandf`/`bandq`,
/// each with its own envelope, plus the shared `fanchor`/`ftype`/`drive`.
#[derive(Clone, Copy, Debug)]
pub struct FilterSet {
    pub lp: FilterParams,
    pub hp: FilterParams,
    pub bp: FilterParams,
}

impl Default for FilterSet {
    fn default() -> FilterSet {
        FilterSet {
            lp: FilterParams::default(),
            hp: FilterParams::default(),
            // superdough's band-pass defaults to Q 1 rather than 0.707.
            bp: FilterParams {
                q: 1.0,
                ..FilterParams::default()
            },
        }
    }
}

impl FilterSet {
    pub fn from_controls(map: &ValueMap) -> FilterSet {
        let mut s = FilterSet::default();
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        // Low-pass (cutoff/lpf) + its envelope.
        s.lp.freq = get("cutoff");
        if let Some(q) = get("resonance") {
            s.lp.q = q.max(0.1);
        }
        s.lp.env = get("lpenv");
        s.lp.attack = get("lpattack");
        s.lp.decay = get("lpdecay");
        s.lp.sustain = get("lpsustain");
        s.lp.release = get("lprelease");
        // High-pass (hcutoff/hpf) + its envelope.
        s.hp.freq = get("hcutoff");
        if let Some(q) = get("hresonance") {
            s.hp.q = q.max(0.1);
        }
        s.hp.env = get("hpenv");
        s.hp.attack = get("hpattack");
        s.hp.decay = get("hpdecay");
        s.hp.sustain = get("hpsustain");
        s.hp.release = get("hprelease");
        // Band-pass (bandf/bpf) + its envelope.
        s.bp.freq = get("bandf");
        if let Some(q) = get("bandq") {
            s.bp.q = q.max(0.1);
        }
        s.bp.env = get("bpenv");
        s.bp.attack = get("bpattack");
        s.bp.decay = get("bpdecay");
        s.bp.sustain = get("bpsustain");
        s.bp.release = get("bprelease");
        // Shared filter-envelope anchor (`fanchor`).
        if let Some(a) = get("fanchor") {
            s.lp.anchor = a;
            s.hp.anchor = a;
            s.bp.anchor = a;
        }
        // `ftype` selects the filter model for every slot (superdough passes the
        // same `model` to each `createFilter` call); `drive` feeds the ladder.
        let model = map
            .get("ftype")
            .map(FilterModel::from_value)
            .unwrap_or_default();
        let drive = get("drive").unwrap_or(0.69);
        for f in [&mut s.lp, &mut s.hp, &mut s.bp] {
            f.model = model;
            f.drive = drive;
        }
        s
    }
}

/// A voice's running filter chain: the enabled slots in low → high → band
/// order, plus a second bank for the right channel when the source is stereo
/// (a biquad carries state, so the channels cannot share one).
///
/// This is the stage every voice needs and only the oscillator and sampler used
/// to have; keeping it here rather than inside `Voice` is what lets the drum,
/// ZZFX, bytebeat and bus voices run the same filters from the same controls.
#[derive(Clone, Default)]
pub struct VoiceFilters {
    left: Vec<VoiceFilter>,
    right: Vec<VoiceFilter>,
}

impl VoiceFilters {
    /// Build the enabled slots. `stereo` allocates the second bank; without it
    /// [`process_stereo`](Self::process_stereo) leaves the right channel dry.
    pub fn new(set: &FilterSet, sample_rate: f32, stereo: bool) -> VoiceFilters {
        let mut left = Vec::new();
        for (kind, fp) in [
            (FilterKind::Low, &set.lp),
            (FilterKind::High, &set.hp),
            (FilterKind::Band, &set.bp),
        ] {
            if fp.freq.is_some() {
                left.push(VoiceFilter::new(kind, fp, sample_rate));
            }
        }
        let right = if stereo { left.clone() } else { Vec::new() };
        VoiceFilters { left, right }
    }

    /// Filter one mono sample through every enabled slot.
    pub fn process(
        &mut self,
        x: f32,
        t: f32,
        hold_end: f32,
        sample_rate: f32,
        mods: &ModBank,
    ) -> f32 {
        run(&mut self.left, x, t, hold_end, sample_rate, mods)
    }

    /// Filter a stereo pair, each channel through its own bank.
    pub fn process_stereo(
        &mut self,
        l: f32,
        r: f32,
        t: f32,
        hold_end: f32,
        sample_rate: f32,
        mods: &ModBank,
    ) -> (f32, f32) {
        (
            run(&mut self.left, l, t, hold_end, sample_rate, mods),
            run(&mut self.right, r, t, hold_end, sample_rate, mods),
        )
    }
}

fn run(
    bank: &mut [VoiceFilter],
    mut x: f32,
    t: f32,
    hold_end: f32,
    sample_rate: f32,
    mods: &ModBank,
) -> f32 {
    for f in bank.iter_mut() {
        let (ft, qt) = f.mod_targets();
        x = f.process(x, t, hold_end, sample_rate, mods.get(ft), mods.get(qt));
    }
    x
}

/// The resonant core of a filter slot, selected by `ftype`.
#[derive(Clone)]
enum FilterCore {
    /// One biquad, or two cascaded for the 24dB slope.
    Biquad(Biquad, Option<Biquad>),
    /// The Moog ladder lowpass.
    Ladder(Ladder),
}

/// A voice filter slot: a resonant core plus an optional cutoff envelope sweep.
#[derive(Clone)]
pub(crate) struct VoiceFilter {
    kind: FilterKind,
    q: f32,
    /// The static cutoff, used as the base when a modulator offsets it.
    base_freq: f32,
    core: FilterCore,
    /// `(adsr, min_hz, max_hz)` when a cutoff envelope is active.
    env: Option<(Adsr, f32, f32)>,
}

impl VoiceFilter {
    pub(crate) fn new(kind: FilterKind, fp: &FilterParams, sample_rate: f32) -> VoiceFilter {
        let base = fp.freq.unwrap_or(1000.0);
        let q = fp.q.max(0.1);
        let env = if fp.has_env() {
            // superdough: min = 2^-offset * f, max = 2^(|env|-offset) * f
            let env_oct = fp.env.unwrap_or(1.0);
            let abs = env_oct.abs();
            let offset = abs * fp.anchor;
            let mut min = (2f32.powf(-offset) * base).clamp(0.0, 20000.0);
            let mut max = (2f32.powf(abs - offset) * base).clamp(0.0, 20000.0);
            if env_oct < 0.0 {
                std::mem::swap(&mut min, &mut max);
            }
            // filter ADSR defaults (superdough): [0.005, 0.14, 0, 0.1]
            let adsr = Adsr {
                attack: fp.attack.unwrap_or(0.005),
                decay: fp.decay.unwrap_or(0.14),
                sustain: fp.sustain.unwrap_or(0.0),
                release: fp.release.unwrap_or(0.1),
            };
            Some((adsr, min, max))
        } else {
            None
        };
        let core = match fp.model {
            FilterModel::Ladder => FilterCore::Ladder(Ladder::new(sample_rate, base, q, fp.drive)),
            FilterModel::Db24 => FilterCore::Biquad(
                Biquad::new(kind, sample_rate, base, q),
                Some(Biquad::new(kind, sample_rate, base, q)),
            ),
            FilterModel::Db12 => FilterCore::Biquad(Biquad::new(kind, sample_rate, base, q), None),
        };
        VoiceFilter {
            kind,
            q,
            base_freq: base,
            core,
            env,
        }
    }

    /// The `(cutoff, resonance)` modulation targets for this slot's kind, so a
    /// caller holding a mixed filter chain can look up the right offsets.
    pub(crate) fn mod_targets(&self) -> (ModTarget, ModTarget) {
        match self.kind {
            FilterKind::High => (ModTarget::Hcutoff, ModTarget::Hresonance),
            FilterKind::Band => (ModTarget::Bandf, ModTarget::Bandq),
            _ => (ModTarget::Cutoff, ModTarget::Resonance),
        }
    }

    /// Process one sample. `freq_mod`/`q_mod` are additive modulator offsets on
    /// the cutoff and resonance (Web Audio sums a connection into the param's
    /// own value, so modulation is additive); both zero means unmodulated.
    pub(crate) fn process(
        &mut self,
        x: f32,
        t: f32,
        hold_end: f32,
        sample_rate: f32,
        freq_mod: f32,
        q_mod: f32,
    ) -> f32 {
        let modulated = freq_mod != 0.0 || q_mod != 0.0;
        if self.env.is_some() || modulated {
            // The envelope sweep is the base when present, the static cutoff
            // otherwise; the modulator rides on top of either.
            let base = match self.env {
                Some((adsr, min, max)) => min + adsr_value(&adsr, t, hold_end) * (max - min),
                None => self.base_freq,
            };
            let freq = base + freq_mod;
            let q = (self.q + q_mod).max(0.1);
            match &mut self.core {
                FilterCore::Biquad(b1, b2) => {
                    b1.update(self.kind, sample_rate, freq, q);
                    if let Some(b2) = b2 {
                        b2.update(self.kind, sample_rate, freq, q);
                    }
                }
                FilterCore::Ladder(l) => l.set_cutoff(sample_rate, freq),
            }
        }
        match &mut self.core {
            FilterCore::Biquad(b1, b2) => {
                let y = b1.process(x);
                match b2 {
                    Some(b2) => b2.process(y),
                    None => y,
                }
            }
            FilterCore::Ladder(l) => l.process(x),
        }
    }
}
