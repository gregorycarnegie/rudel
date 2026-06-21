use crate::filter::Biquad;
use crate::voice::VoiceLike;
use rudel_core::ValueMap;
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumKind {
    Bd,   // bass drum
    Sd,   // snare drum
    Rim,  // rimshot
    Clap, // hand clap
    Hh,   // closed hi-hat
    Oh,   // open hi-hat
    Lt,   // low tom
    Mt,   // mid tom
    Ht,   // high tom
    Rd,   // ride cymbal
    Cr,   // crash cymbal
}

impl DrumKind {
    /// Map a sound name to a drum kind (`bd`, `sd`, `hh`, ... and common aliases).
    pub fn from_name(name: &str) -> Option<DrumKind> {
        Some(match name {
            "bd" | "bassdrum" | "kick" => DrumKind::Bd,
            "sd" | "snare" | "sn" => DrumKind::Sd,
            "rim" | "rs" | "rimshot" => DrumKind::Rim,
            "cp" | "clap" | "hc" => DrumKind::Clap,
            "hh" | "ch" | "hat" | "hihat" => DrumKind::Hh,
            "oh" | "oht" | "openhat" => DrumKind::Oh,
            "lt" | "lowtom" => DrumKind::Lt,
            "mt" | "midtom" => DrumKind::Mt,
            "ht" | "hightom" => DrumKind::Ht,
            "rd" | "ride" => DrumKind::Rd,
            "cr" | "crash" => DrumKind::Cr,
            _ => return None,
        })
    }

    /// Total ring-out time in seconds (after which the voice is silent).
    fn lifetime(self) -> f32 {
        match self {
            DrumKind::Bd => 0.4,
            DrumKind::Sd => 0.3,
            DrumKind::Rim => 0.06,
            DrumKind::Clap => 0.4,
            DrumKind::Hh => 0.12,
            DrumKind::Oh => 0.4,
            DrumKind::Lt | DrumKind::Mt | DrumKind::Ht => 0.4,
            DrumKind::Rd => 0.7,
            DrumKind::Cr => 1.2,
        }
    }
}

/// Parameters for a synthesized drum hit.
#[derive(Clone, Copy, Debug)]
pub struct DrumParams {
    pub kind: DrumKind,
    pub gain: f32,
    pub pan: f32,
    pub room: f32,
    pub delay: f32,
    pub dry: f32,
}

impl DrumParams {
    pub fn new(kind: DrumKind) -> DrumParams {
        DrumParams {
            kind,
            gain: 1.0,
            pan: 0.5,
            room: 0.0,
            delay: 0.0,
            dry: 1.0,
        }
    }

    pub fn apply_controls(&mut self, map: &ValueMap) {
        if let Some(g) = map.get("gain").and_then(|v| v.as_f64()) {
            self.gain = g as f32;
        }
        if let Some(p) = map.get("pan").and_then(|v| v.as_f64()) {
            self.pan = p as f32;
        }
        if let Some(r) = map.get("room").and_then(|v| v.as_f64()) {
            self.room = r as f32;
        }
        if let Some(d) = map.get("delay").and_then(|v| v.as_f64()) {
            self.delay = d as f32;
        }
        if let Some(dry) = map.get("dry").and_then(|v| v.as_f64()) {
            self.dry = dry as f32;
        }
    }
}

/// A sounding synthesized drum voice.
pub struct DrumVoice {
    kind: DrumKind,
    t: f32,
    dt: f32,
    phase: f32,
    rng: u32,
    filter: Option<Biquad>,
    gain: f32,
    left_gain: f32,
    right_gain: f32,
    room: f32,
    delay: f32,
    dry: f32,
    done_at: f32,
    done: bool,
}

impl DrumVoice {
    pub fn new(params: DrumParams, sample_rate: f32) -> DrumVoice {
        let pan = params.pan.clamp(0.0, 1.0);
        let filter = match params.kind {
            DrumKind::Hh | DrumKind::Oh => Some(Biquad::highpass(sample_rate, 7000.0, 0.7)),
            DrumKind::Rd => Some(Biquad::highpass(sample_rate, 5000.0, 0.7)),
            DrumKind::Cr => Some(Biquad::highpass(sample_rate, 4000.0, 0.7)),
            DrumKind::Sd => Some(Biquad::bandpass(sample_rate, 1800.0, 0.6)),
            DrumKind::Rim => Some(Biquad::bandpass(sample_rate, 1700.0, 1.2)),
            _ => None,
        };
        DrumVoice {
            kind: params.kind,
            t: 0.0,
            dt: 1.0 / sample_rate,
            phase: 0.0,
            rng: 0x9E37_79B9,
            filter,
            gain: params.gain,
            left_gain: (pan * FRAC_PI_2).cos(),
            right_gain: (pan * FRAC_PI_2).sin(),
            room: params.room,
            delay: params.delay,
            dry: params.dry,
            done_at: params.kind.lifetime(),
            done: false,
        }
    }

    /// White noise in -1..1 (xorshift).
    fn noise(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Advance the tonal oscillator at `freq` Hz and return sin(phase).
    fn osc(&mut self, freq: f32) -> f32 {
        self.phase = (self.phase + freq * self.dt).rem_euclid(1.0);
        (TAU * self.phase).sin()
    }

    fn mono(&mut self) -> f32 {
        let t = self.t;
        let exp = |tau: f32| (-t / tau).exp();
        match self.kind {
            DrumKind::Bd => {
                let freq = 48.0 + 90.0 * exp(0.03);
                let body = self.osc(freq) * exp(0.16);
                let click = if t < 0.003 { 0.6 } else { 0.0 };
                body + click
            }
            DrumKind::Lt | DrumKind::Mt | DrumKind::Ht => {
                let base = match self.kind {
                    DrumKind::Lt => 90.0,
                    DrumKind::Mt => 150.0,
                    _ => 230.0,
                };
                let freq = base + base * 0.6 * exp(0.04);
                self.osc(freq) * exp(0.22)
            }
            DrumKind::Sd => {
                let tone = self.osc(185.0) * exp(0.1) * 0.5;
                let mut noise = self.noise() * exp(0.16);
                if let Some(f) = &mut self.filter {
                    noise = f.process(noise);
                }
                tone + noise
            }
            DrumKind::Rim => {
                let n = self.noise();
                let tone = self.osc(1700.0) * 0.5;
                let mut s = (n + tone) * exp(0.012);
                if let Some(f) = &mut self.filter {
                    s = f.process(s);
                }
                s
            }
            DrumKind::Clap => {
                // three quick bursts then a short tail
                let env = exp(0.012)
                    + (-(t - 0.01).max(0.0) / 0.012).exp()
                    + (-(t - 0.02).max(0.0) / 0.02).exp();
                let mut n = self.noise() * env * 0.4;
                // a touch of body
                n += self.noise() * exp(0.12) * 0.1;
                n
            }
            DrumKind::Hh => {
                let mut n = self.noise() * exp(0.03);
                if let Some(f) = &mut self.filter {
                    n = f.process(n);
                }
                n
            }
            DrumKind::Oh => {
                let mut n = self.noise() * exp(0.18);
                if let Some(f) = &mut self.filter {
                    n = f.process(n);
                }
                n
            }
            DrumKind::Rd => {
                // metallic partials + noise shimmer
                let metal = (self.osc(5200.0) + (TAU * 8400.0 * t).sin()) * 0.2;
                let mut s = (metal + self.noise() * 0.5) * exp(0.4);
                if let Some(f) = &mut self.filter {
                    s = f.process(s);
                }
                s
            }
            DrumKind::Cr => {
                let mut n = self.noise() * exp(0.5);
                if let Some(f) = &mut self.filter {
                    n = f.process(n);
                }
                n
            }
        }
    }
}

impl VoiceLike for DrumVoice {
    fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let s = self.mono() * self.gain * 0.7;
        self.t += self.dt;
        if self.t >= self.done_at {
            self.done = true;
        }
        (s * self.left_gain, s * self.right_gain)
    }
    fn is_done(&self) -> bool {
        self.done
    }
    fn room(&self) -> f32 {
        self.room
    }
    fn delay_send(&self) -> f32 {
        self.delay
    }
    fn dry(&self) -> f32 {
        self.dry
    }
}
