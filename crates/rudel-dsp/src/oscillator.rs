use std::f32::consts::TAU;
use wide::f32x8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
    /// Variable-duty pulse (`s("pulse")` + `pw`). Sampled via [`Waveform::pulse`].
    Pulse,
}

impl Waveform {
    pub fn from_name(name: &str) -> Option<Waveform> {
        Some(match name {
            "sine" | "sin" => Waveform::Sine,
            "saw" | "sawtooth" => Waveform::Saw,
            "square" | "sqr" => Waveform::Square,
            "triangle" | "tri" => Waveform::Triangle,
            "pulse" => Waveform::Pulse,
            _ => return None,
        })
    }

    pub(crate) fn sample(self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        match self {
            Waveform::Sine => (TAU * p).sin(),
            Waveform::Saw => 2.0 * p - 1.0,
            Waveform::Square | Waveform::Pulse => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 4.0 * (if p < 0.5 { p } else { 1.0 - p }) - 1.0,
        }
    }

    /// A pulse wave with the given duty cycle (`pw`, 0..1). 0.5 == square.
    pub(crate) fn pulse(phase: f32, pw: f32) -> f32 {
        if phase.rem_euclid(1.0) < pw.clamp(0.0, 1.0) {
            1.0
        } else {
            -1.0
        }
    }
}

/// One cycle of a precomputed additive wavetable, in samples.
pub(crate) const ADDITIVE_SIZE: usize = 2048;

/// The base harmonic series an additive (`partials`) waveform is built from.
/// Mirrors superdough's `waveformN` term table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdditiveType {
    Saw,
    Square,
    Triangle,
    /// `user`: harmonics come entirely from the `partials` magnitudes.
    User,
}

impl AdditiveType {
    pub fn from_name(name: &str) -> Option<AdditiveType> {
        Some(match name {
            "sawtooth" | "saw" => AdditiveType::Saw,
            "square" | "sqr" => AdditiveType::Square,
            "triangle" | "tri" => AdditiveType::Triangle,
            "user" => AdditiveType::User,
            _ => return None,
        })
    }

    /// `(real, imag)` Fourier coefficient for harmonic `n` (1-indexed).
    fn term(self, n: usize) -> (f32, f32) {
        let nf = n as f32;
        let odd = n % 2 == 1;
        match self {
            AdditiveType::Saw => (0.0, -1.0 / nf),
            AdditiveType::Square => (0.0, if odd { 1.0 / nf } else { 0.0 }),
            AdditiveType::Triangle => (if odd { 1.0 / (nf * nf) } else { 0.0 }, 0.0),
            AdditiveType::User => (0.0, 1.0),
        }
    }
}

/// Build a one-cycle, peak-normalized additive wavetable from harmonic
/// magnitudes (`partials`) over a base series, with optional per-harmonic
/// `phases` (in turns). Ports `waveformN` + Web Audio's default normalization.
pub(crate) fn build_additive(
    partials: &[f32],
    phases: Option<&[f32]>,
    base: AdditiveType,
) -> Vec<f32> {
    // Per-harmonic (real, imag) coefficients, scaled by magnitude and rotated.
    let coeffs: Vec<(f32, f32)> = partials
        .iter()
        .enumerate()
        .map(|(k, &mag)| {
            let (r0, i0) = base.term(k + 1);
            let (mut r, mut i) = (r0 * mag, i0 * mag);
            if let Some(ph) = phases.and_then(|p| p.get(k)).copied()
                && ph != 0.0
            {
                let (c, s) = ((TAU * ph).cos(), (TAU * ph).sin());
                (r, i) = (c * r - s * i, s * r + c * i);
            }
            (r, i)
        })
        .collect();

    // Fill the table eight slots at a time: each lane is a distinct phase `t`,
    // and for every harmonic we evaluate `r·cos(ang) + i·sin(ang)` across all
    // eight lanes with one vectorized `sin_cos`. `ADDITIVE_SIZE` is a multiple
    // of the lane count, so `chunks_exact_mut` leaves no scalar remainder.
    let mut table = vec![0.0f32; ADDITIVE_SIZE];
    let inv_size = 1.0 / ADDITIVE_SIZE as f32;
    let tau = f32x8::splat(TAU);
    for (chunk, slots) in table.chunks_exact_mut(8).enumerate() {
        let base = chunk * 8;
        let t = f32x8::from(std::array::from_fn::<f32, 8, _>(|l| (base + l) as f32 * inv_size));
        let mut acc = f32x8::splat(0.0);
        for (k, &(r, i)) in coeffs.iter().enumerate() {
            let ang = tau * f32x8::splat((k + 1) as f32) * t;
            let (sin, cos) = ang.sin_cos();
            acc += f32x8::splat(r) * cos + f32x8::splat(i) * sin;
        }
        slots.copy_from_slice(&acc.to_array());
    }
    // Normalize to peak 1 (Web Audio normalizes PeriodicWave by default).
    let peak = table.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    if peak > 1e-9 {
        for x in &mut table {
            *x /= peak;
        }
    }
    table
}

/// Sample a one-cycle wavetable at `phase` (0..1) with linear interpolation.
pub(crate) fn sample_table(table: &[f32], phase: f32) -> f32 {
    let len = table.len();
    let p = phase.rem_euclid(1.0) * len as f32;
    let i = p.floor() as usize;
    let frac = p - i as f32;
    let a = table[i % len];
    let b = table[(i + 1) % len];
    a + (b - a) * frac
}

/// A noise source (`s("white"/"pink"/"brown")`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseKind {
    White,
    Pink,
    Brown,
}

impl NoiseKind {
    pub fn from_name(name: &str) -> Option<NoiseKind> {
        Some(match name {
            "white" | "noise" => NoiseKind::White,
            "pink" => NoiseKind::Pink,
            "brown" => NoiseKind::Brown,
            _ => return None,
        })
    }
}

/// Stateful noise generator (white/pink/brown), ported from superdough's
/// `getNoiseBuffer`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NoiseGen {
    rng: u32,
    pink: [f32; 7],
    brown_last: f32,
}

impl NoiseGen {
    pub(crate) fn new() -> NoiseGen {
        NoiseGen {
            rng: 0x1234_5678,
            pink: [0.0; 7],
            brown_last: 0.0,
        }
    }

    fn white(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    pub(crate) fn next(&mut self, kind: NoiseKind) -> f32 {
        let white = self.white();
        match kind {
            NoiseKind::White => white,
            NoiseKind::Brown => {
                let out = (self.brown_last + 0.02 * white) / 1.02;
                self.brown_last = out;
                out
            }
            NoiseKind::Pink => {
                // Paul Kellet's refined pink-noise filter.
                let b = &mut self.pink;
                b[0] = 0.99886 * b[0] + white * 0.0555179;
                b[1] = 0.99332 * b[1] + white * 0.0750759;
                b[2] = 0.969 * b[2] + white * 0.153852;
                b[3] = 0.8665 * b[3] + white * 0.3104856;
                b[4] = 0.55 * b[4] + white * 0.5329522;
                b[5] = -0.7616 * b[5] - white * 0.016898;
                let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + white * 0.5362;
                b[6] = white * 0.115926;
                out * 0.11
            }
        }
    }
}
