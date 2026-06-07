use crate::envelope::{Adsr, adsr_value};
use std::f32::consts::PI;

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

    /// Recompute the RBJ coefficients in place, preserving the filter state
    /// (`z1`/`z2`) so the cutoff can be modulated per sample.
    fn update(&mut self, kind: FilterKind, sample_rate: f32, freq: f32, q: f32) {
        let freq = freq.clamp(20.0, sample_rate * 0.45);
        let w0 = 2.0 * PI * freq / sample_rate;
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
    /// `ftype` 24dB: cascade the biquad twice for a steeper slope.
    pub cascade: bool,
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
            cascade: false,
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

/// A voice filter slot: a biquad plus an optional cutoff envelope sweep.
pub(crate) struct VoiceFilter {
    kind: FilterKind,
    q: f32,
    biquad: Biquad,
    /// A second cascaded biquad for the `ftype` 24dB slope (`None` = 12dB).
    second: Option<Biquad>,
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
        VoiceFilter {
            kind,
            q,
            biquad: Biquad::new(kind, sample_rate, base, q),
            second: fp.cascade.then(|| Biquad::new(kind, sample_rate, base, q)),
            env,
        }
    }

    pub(crate) fn process(&mut self, x: f32, t: f32, hold_end: f32, sample_rate: f32) -> f32 {
        if let Some((adsr, min, max)) = self.env {
            let shape = adsr_value(&adsr, t, hold_end);
            let freq = min + shape * (max - min);
            self.biquad.update(self.kind, sample_rate, freq, self.q);
            if let Some(b2) = &mut self.second {
                b2.update(self.kind, sample_rate, freq, self.q);
            }
        }
        let y = self.biquad.process(x);
        match &mut self.second {
            Some(b2) => b2.process(y),
            None => y,
        }
    }
}
