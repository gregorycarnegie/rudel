use std::f32::consts::PI;

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
            Waveform::Sine => (2.0 * PI * p).sin(),
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
