// rudel-dsp - synthesis voices for Rudel.
// Phase-3 voices are hand-rolled (oscillator + ADSR + pan) so they're
// deterministic and testable offline; fundsp powers effects in a later phase.
// Param model mirrors strudel/packages/superdough/synth.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::Value;
use std::collections::BTreeMap;
use std::f32::consts::PI;
use std::sync::Arc;

/// Common interface for anything the mixer can play (synth or sampler).
pub trait VoiceLike: Send {
    /// Render the next stereo sample.
    fn tick(&mut self) -> (f32, f32);
    fn is_done(&self) -> bool;
    /// Reverb (`room`) send amount.
    fn room(&self) -> f32;
    /// Delay (`delay`) send amount.
    fn delay_send(&self) -> f32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Waveform {
    pub fn from_name(name: &str) -> Option<Waveform> {
        Some(match name {
            "sine" | "sin" => Waveform::Sine,
            "saw" | "sawtooth" => Waveform::Saw,
            "square" | "sqr" => Waveform::Square,
            "triangle" | "tri" => Waveform::Triangle,
            _ => return None,
        })
    }

    fn sample(self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        match self {
            Waveform::Sine => (2.0 * PI * p).sin(),
            Waveform::Saw => 2.0 * p - 1.0,
            Waveform::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 4.0 * (if p < 0.5 { p } else { 1.0 - p }) - 1.0,
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
struct NoiseGen {
    rng: u32,
    pink: [f32; 7],
    brown_last: f32,
}

impl NoiseGen {
    fn new() -> NoiseGen {
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

    fn next(&mut self, kind: NoiseKind) -> f32 {
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

/// Attack/decay/sustain/release envelope (seconds; sustain is a 0..1 level).
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Adsr {
    // Strudel synth defaults: getADSRValues(..., [0.001, 0.05, 0.6, 0.01])
    fn default() -> Self {
        Adsr {
            attack: 0.001,
            decay: 0.05,
            sustain: 0.6,
            release: 0.01,
        }
    }
}

/// Everything needed to render one note.
#[derive(Clone, Copy, Debug)]
pub struct VoiceParams {
    pub waveform: Waveform,
    /// When set, the source is noise rather than the oscillator.
    pub noise: Option<NoiseKind>,
    /// When true, the source is a detuned super-saw.
    pub supersaw: bool,
    /// Super-saw voice count (`unison`).
    pub unison: usize,
    /// Super-saw detune in cents (`detune`).
    pub detune: f32,
    /// Super-saw frequency spread in semitones (`spread`).
    pub spread: f32,
    /// FM modulation index (`fm`/`fmi`); `None` = no FM.
    pub fm: Option<f32>,
    /// FM harmonicity ratio (`fmh`), modulator freq / carrier freq.
    pub fmh: f32,
    /// Vibrato rate in Hz (`vib`); `None`/0 = off.
    pub vib: Option<f32>,
    /// Vibrato depth in semitones (`vibmod`).
    pub vibmod: f32,
    /// Pitch-envelope amount in semitones (`penv`).
    pub penv: Option<f32>,
    pub pattack: Option<f32>,
    pub pdecay: Option<f32>,
    pub psustain: Option<f32>,
    pub prelease: Option<f32>,
    /// Pitch-envelope anchor (`panchor`); defaults to the pitch sustain.
    pub panchor: Option<f32>,
    pub freq: f32,
    pub gain: f32,
    /// 0.0 = hard left, 1.0 = hard right.
    pub pan: f32,
    pub adsr: Adsr,
    /// Hold time in seconds (the note's `whole` duration), before release.
    pub duration: f32,
    /// Extra sustain hold beyond the note duration (`hold`), in seconds.
    pub hold: f32,
    /// Low-pass filter (`cutoff`/`lpf` + `lpenv`/`lpattack`/...).
    pub lp: FilterParams,
    /// High-pass filter (`hcutoff`/`hpf` + `hpenv`/...).
    pub hp: FilterParams,
    /// Band-pass filter (`bandf`/`bpf` + `bpenv`/...).
    pub bp: FilterParams,
    /// Reverb send amount (`room`), 0..1.
    pub room: f32,
    /// Delay send amount (`delay`), 0..1.
    pub delay: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            waveform: Waveform::Sine,
            noise: None,
            supersaw: false,
            unison: 5,
            detune: 0.0,
            spread: 0.2,
            fm: None,
            fmh: 1.0,
            vib: None,
            vibmod: 0.5,
            penv: None,
            pattack: None,
            pdecay: None,
            psustain: None,
            prelease: None,
            panchor: None,
            freq: 440.0,
            gain: 1.0,
            pan: 0.5,
            adsr: Adsr::default(),
            duration: 0.25,
            hold: 0.0,
            lp: FilterParams::default(),
            hp: FilterParams::default(),
            bp: FilterParams {
                q: 1.0,
                ..FilterParams::default()
            },
            room: 0.0,
            delay: 0.0,
        }
    }
}

impl VoiceParams {
    /// Build params from a control map and the note duration in seconds.
    pub fn from_controls(map: &BTreeMap<String, Value>, duration: f32) -> VoiceParams {
        let mut p = VoiceParams {
            duration,
            ..Default::default()
        };
        if let Some(name) = map.get("s").and_then(|v| v.as_str()) {
            if name == "supersaw" {
                p.supersaw = true;
            } else if let Some(w) = Waveform::from_name(name) {
                p.waveform = w;
            } else if let Some(nk) = NoiseKind::from_name(name) {
                p.noise = Some(nk);
            }
        }
        if let Some(u) = map.get("unison").and_then(|v| v.as_f64()) {
            p.unison = (u as usize).max(1);
        }
        if let Some(d) = map.get("detune").and_then(|v| v.as_f64()) {
            p.detune = d as f32;
        }
        if let Some(s) = map.get("spread").and_then(|v| v.as_f64()) {
            p.spread = s as f32;
        }
        // FM: `fm`/`fmi` modulation index, `fmh` harmonicity ratio.
        if let Some(i) = map
            .get("fm")
            .or_else(|| map.get("fmi"))
            .and_then(|v| v.as_f64())
        {
            p.fm = Some(i as f32);
        }
        if let Some(h) = map.get("fmh").and_then(|v| v.as_f64()) {
            p.fmh = h as f32;
        }
        // Vibrato (`vib` rate Hz, `vibmod` depth semitones).
        if let Some(r) = map.get("vib").and_then(|v| v.as_f64()) {
            p.vib = Some(r as f32);
        }
        if let Some(d) = map.get("vibmod").and_then(|v| v.as_f64()) {
            p.vibmod = d as f32;
        }
        // Pitch envelope (`penv` semitones + `p{attack,decay,sustain,release}`).
        p.penv = map.get("penv").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.pattack = map.get("pattack").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.pdecay = map.get("pdecay").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.psustain = map.get("psustain").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.prelease = map.get("prelease").and_then(|v| v.as_f64()).map(|x| x as f32);
        p.panchor = map.get("panchor").and_then(|v| v.as_f64()).map(|x| x as f32);
        if let Some(freq) = map.get("freq").and_then(|v| v.as_f64()) {
            p.freq = freq as f32;
        } else if let Some(n) = map.get("note") {
            p.freq = note_to_freq(n).unwrap_or(p.freq);
        } else if let Some(n) = map.get("n") {
            // bare numbers as note numbers when no note/freq given
            if let Some(f) = note_to_freq(n) {
                p.freq = f;
            }
        }
        if let Some(g) = map.get("gain").and_then(|v| v.as_f64()) {
            p.gain = g as f32;
        }
        if let Some(pan) = map.get("pan").and_then(|v| v.as_f64()) {
            p.pan = pan as f32;
        }
        if let Some(a) = map.get("attack").and_then(|v| v.as_f64()) {
            p.adsr.attack = a as f32;
        }
        if let Some(d) = map.get("decay").and_then(|v| v.as_f64()) {
            p.adsr.decay = d as f32;
        }
        if let Some(s) = map.get("sustain").and_then(|v| v.as_f64()) {
            p.adsr.sustain = s as f32;
        }
        if let Some(r) = map.get("release").and_then(|v| v.as_f64()) {
            p.adsr.release = r as f32;
        }
        // ADSR shortcut controls accept a `:`-list, e.g. `adsr("0.1:0.1:0.5:0.2")`.
        let list = |k: &str| -> Option<Vec<f32>> {
            map.get(k).map(|v| match v {
                Value::List(items) => items.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect(),
                other => other.as_f64().map(|f| f as f32).into_iter().collect(),
            })
        };
        if let Some(v) = list("adsr") {
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(d) = v.get(1) {
                p.adsr.decay = *d;
            }
            if let Some(s) = v.get(2) {
                p.adsr.sustain = *s;
            }
            if let Some(r) = v.get(3) {
                p.adsr.release = *r;
            }
        }
        if let Some(v) = list("ad") {
            // attack/decay with no sustain (percussive)
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(d) = v.get(1) {
                p.adsr.decay = *d;
            }
            p.adsr.sustain = 0.0;
        }
        if let Some(v) = list("ar") {
            if let Some(a) = v.first() {
                p.adsr.attack = *a;
            }
            if let Some(r) = v.get(1) {
                p.adsr.release = *r;
            }
        }
        if let Some(h) = map.get("hold").and_then(|v| v.as_f64()) {
            p.hold = h as f32;
        }
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        // Low-pass (cutoff/lpf) + its envelope.
        p.lp.freq = get("cutoff");
        if let Some(q) = get("resonance") {
            p.lp.q = q.max(0.1);
        }
        p.lp.env = get("lpenv");
        p.lp.attack = get("lpattack");
        p.lp.decay = get("lpdecay");
        p.lp.sustain = get("lpsustain");
        p.lp.release = get("lprelease");
        // High-pass (hcutoff/hpf) + its envelope.
        p.hp.freq = get("hcutoff");
        if let Some(q) = get("hresonance") {
            p.hp.q = q.max(0.1);
        }
        p.hp.env = get("hpenv");
        p.hp.attack = get("hpattack");
        p.hp.decay = get("hpdecay");
        p.hp.sustain = get("hpsustain");
        p.hp.release = get("hprelease");
        // Band-pass (bandf/bpf) + its envelope.
        p.bp.freq = get("bandf");
        if let Some(q) = get("bandq") {
            p.bp.q = q.max(0.1);
        }
        p.bp.env = get("bpenv");
        p.bp.attack = get("bpattack");
        p.bp.decay = get("bpdecay");
        p.bp.sustain = get("bpsustain");
        p.bp.release = get("bprelease");
        // Shared filter-envelope anchor (`fanchor`).
        if let Some(a) = get("fanchor") {
            p.lp.anchor = a;
            p.hp.anchor = a;
            p.bp.anchor = a;
        }
        if let Some(room) = map.get("room").and_then(|v| v.as_f64()) {
            p.room = room as f32;
        }
        if let Some(d) = map.get("delay").and_then(|v| v.as_f64()) {
            p.delay = d as f32;
        }
        p
    }
}

/// A transposed-direct-form-II biquad, used for the per-voice low-pass filter
/// (RBJ cookbook coefficients).
#[derive(Clone, Copy)]
struct Biquad {
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
enum FilterKind {
    Low,
    High,
    Band,
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

    fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::Low, sample_rate, cutoff, q)
    }
    fn highpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::High, sample_rate, cutoff, q)
    }
    fn bandpass(sample_rate: f32, center: f32, q: f32) -> Biquad {
        Biquad::new(FilterKind::Band, sample_rate, center, q)
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
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cos) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn process(&mut self, x: f32) -> f32 {
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
struct VoiceFilter {
    kind: FilterKind,
    q: f32,
    biquad: Biquad,
    /// `(adsr, min_hz, max_hz)` when a cutoff envelope is active.
    env: Option<(Adsr, f32, f32)>,
}

impl VoiceFilter {
    fn new(kind: FilterKind, fp: &FilterParams, sample_rate: f32) -> VoiceFilter {
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
            env,
        }
    }

    fn process(&mut self, x: f32, t: f32, hold_end: f32, sample_rate: f32) -> f32 {
        if let Some((adsr, min, max)) = self.env {
            let shape = adsr_value(&adsr, t, hold_end);
            let freq = min + shape * (max - min);
            self.biquad.update(self.kind, sample_rate, freq, self.q);
        }
        self.biquad.process(x)
    }
}

/// Linear ADSR shape in 0..1 at time `t` (seconds), with the note held until
/// `hold_end` then released. Shared by the amp envelope and filter envelopes.
fn adsr_value(adsr: &Adsr, t: f32, hold_end: f32) -> f32 {
    let Adsr {
        attack,
        decay,
        sustain,
        release,
    } = *adsr;
    if t < attack {
        t / attack.max(1e-9)
    } else if t < attack + decay {
        1.0 - (1.0 - sustain) * ((t - attack) / decay.max(1e-9))
    } else if t < hold_end {
        sustain
    } else if t < hold_end + release {
        sustain * (1.0 - (t - hold_end) / release.max(1e-9))
    } else {
        0.0
    }
}

/// MIDI note number to frequency in Hz.
pub fn mtof(note: f64) -> f32 {
    (440.0 * 2f64.powf((note - 69.0) / 12.0)) as f32
}

/// Convert a note value (number, or a name like `c4`/`eb3`/`f#5`) to a
/// frequency. Note names follow the convention a4 = 69 = 440 Hz.
pub fn note_to_freq(value: &Value) -> Option<f32> {
    if let Some(n) = value.as_f64() {
        // numeric: treat as a MIDI note number
        if matches!(value, Value::Int(_) | Value::F64(_)) {
            return Some(mtof(n));
        }
    }
    let s = value.as_str()?;
    note_name_to_midi(s).map(|m| mtof(m as f64))
}

/// Parse a note name like `c`, `cs4`, `c#4`, `eb3`, `Gb2` to a MIDI number.
/// Delegates to the canonical implementation in `rudel_core::tonal`.
pub fn note_name_to_midi(s: &str) -> Option<i32> {
    rudel_core::note_to_midi(s)
}

/// A single sounding note.
pub struct Voice {
    params: VoiceParams,
    sample_rate: f32,
    phase: f32,
    t: f32, // elapsed seconds
    left_gain: f32,
    right_gain: f32,
    hold_end: f32,
    /// Filter chain (low/high/band-pass), applied in order to the oscillator.
    filters: Vec<VoiceFilter>,
    noise: NoiseGen,
    /// FM modulator phase.
    mod_phase: f32,
    /// Per-voice phases for the super-saw source.
    super_phases: Vec<f32>,
    /// Pitch envelope as `(adsr, min_semitones, max_semitones)`.
    pitch_env: Option<(Adsr, f32, f32)>,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * PI / 2.0).cos();
        let right_gain = (pan * PI / 2.0).sin();
        let hold_end = (params.duration + params.hold).max(params.adsr.attack);
        let mut filters = Vec::new();
        if params.lp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::Low, &params.lp, sample_rate));
        }
        if params.hp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::High, &params.hp, sample_rate));
        }
        if params.bp.freq.is_some() {
            filters.push(VoiceFilter::new(FilterKind::Band, &params.bp, sample_rate));
        }
        // Spread the super-saw voices' initial phases for a fuller sound.
        let super_phases = if params.supersaw {
            (0..params.unison.max(1))
                .map(|n| n as f32 / params.unison.max(1) as f32)
                .collect()
        } else {
            Vec::new()
        };
        // Pitch envelope (superdough getPitchEnvelope): sweep detune in cents.
        let pitch_active = params.penv.is_some()
            || params.pattack.is_some()
            || params.pdecay.is_some()
            || params.psustain.is_some()
            || params.prelease.is_some();
        let pitch_env = if pitch_active {
            let adsr = Adsr {
                attack: params.pattack.unwrap_or(0.2),
                decay: params.pdecay.unwrap_or(0.001),
                sustain: params.psustain.unwrap_or(1.0),
                release: params.prelease.unwrap_or(0.001),
            };
            let penv = params.penv.unwrap_or(1.0); // semitones
            let anchor = params.panchor.unwrap_or(adsr.sustain);
            let min = -penv * anchor;
            let max = penv - penv * anchor;
            Some((adsr, min, max))
        } else {
            None
        };
        Voice {
            params,
            sample_rate,
            phase: 0.0,
            t: 0.0,
            left_gain,
            right_gain,
            hold_end,
            filters,
            noise: NoiseGen::new(),
            mod_phase: 0.0,
            super_phases,
            pitch_env,
            done: false,
        }
    }

    /// Reverb send amount for this voice (`room`).
    pub fn room(&self) -> f32 {
        self.params.room
    }
    /// Delay send amount for this voice (`delay`).
    pub fn delay_send(&self) -> f32 {
        self.params.delay
    }

    fn envelope(&self) -> f32 {
        adsr_value(&self.params.adsr, self.t, self.hold_end)
    }

    /// Pitch multiplier from vibrato + pitch envelope (applied to the carrier).
    fn pitch_mult(&self) -> f32 {
        let mut semis = 0.0;
        if let Some(rate) = self.params.vib
            && rate > 0.0
        {
            semis += self.params.vibmod * (2.0 * PI * rate * self.t).sin();
        }
        if let Some((adsr, min, max)) = self.pitch_env {
            let shape = adsr_value(&adsr, self.t, self.hold_end);
            semis += min + shape * (max - min);
        }
        if semis == 0.0 {
            1.0
        } else {
            2f32.powf(semis / 12.0)
        }
    }

    /// Produce the next source sample and advance the oscillator phase(s).
    fn next_source(&mut self) -> f32 {
        let sr = self.sample_rate;
        let pitch = self.pitch_mult();
        if self.params.supersaw {
            let voices = self.params.unison.max(1);
            // main detune (cents -> semitones)
            let base =
                self.params.freq * pitch * 2f32.powf((self.params.detune / 100.0) / 12.0);
            let scale = if voices > 1 {
                self.params.spread / (voices as f32 - 1.0)
            } else {
                0.0
            };
            let center = self.params.spread * 0.5;
            let mut sum = 0.0;
            for (n, ph) in self.super_phases.iter_mut().enumerate() {
                let d = n as f32 * scale - center; // semitone detune for this voice
                let f = base * 2f32.powf(d / 12.0);
                sum += 2.0 * *ph - 1.0; // naive saw
                *ph = (*ph + f / sr).rem_euclid(1.0);
            }
            return sum / (voices as f32).sqrt();
        }
        if let Some(kind) = self.params.noise {
            return self.noise.next(kind);
        }
        // Oscillator, optionally frequency-modulated.
        let carrier = self.params.freq * pitch;
        let s = self.params.waveform.sample(self.phase);
        let inc = if let Some(index) = self.params.fm {
            let modfreq = carrier * self.params.fmh;
            let modv = (2.0 * PI * self.mod_phase).sin();
            self.mod_phase = (self.mod_phase + modfreq / sr).rem_euclid(1.0);
            (carrier + index * modfreq * modv) / sr
        } else {
            carrier / sr
        };
        self.phase = (self.phase + inc).rem_euclid(1.0);
        s
    }

    /// Render the next stereo sample `(left, right)`.
    pub fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let env = self.envelope();
        let mut osc = self.next_source();
        let (t, hold_end, sr) = (self.t, self.hold_end, self.sample_rate);
        for f in &mut self.filters {
            osc = f.process(osc, t, hold_end, sr);
        }
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let s = osc * env * self.params.gain * 0.3;

        self.t += 1.0 / self.sample_rate;
        if self.t >= self.hold_end + self.params.adsr.release {
            self.done = true;
        }
        (s * self.left_gain, s * self.right_gain)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl VoiceLike for Voice {
    fn tick(&mut self) -> (f32, f32) {
        Voice::tick(self)
    }
    fn is_done(&self) -> bool {
        self.done
    }
    fn room(&self) -> f32 {
        self.params.room
    }
    fn delay_send(&self) -> f32 {
        self.params.delay
    }
}

/// A decoded, mono audio sample.
pub struct Sample {
    pub data: Vec<f32>,
    pub sample_rate: f32,
}

/// Parameters for playing back a [`Sample`].
#[derive(Clone)]
pub struct SamplerParams {
    pub sample: Arc<Sample>,
    pub gain: f32,
    pub pan: f32,
    /// Playback-rate multiplier (`speed`); also driven by `note` for pitched
    /// samples.
    pub speed: f32,
    pub attack: f32,
    pub release: f32,
    pub cutoff: Option<f32>,
    pub resonance: f32,
    pub room: f32,
    pub delay: f32,
    /// Hold time in seconds (0 = play to the sample's natural end).
    pub duration: f32,
    /// Start/end positions as fractions of the sample (0..1).
    pub begin: f32,
    pub end: f32,
    /// When true (`unit: 'c'`), `speed` is interpreted in cycles: the effective
    /// playback rate is multiplied by the sample's duration in seconds. Used by
    /// `loopAt`/`fit`/`splice` to time-stretch a sample.
    pub unit_cycles: bool,
}

impl SamplerParams {
    pub fn new(sample: Arc<Sample>) -> SamplerParams {
        SamplerParams {
            sample,
            gain: 1.0,
            pan: 0.5,
            speed: 1.0,
            attack: 0.001,
            release: 0.05,
            cutoff: None,
            resonance: 0.707,
            room: 0.0,
            delay: 0.0,
            duration: 0.0,
            begin: 0.0,
            end: 1.0,
            unit_cycles: false,
        }
    }

    /// Apply common controls from a map.
    pub fn apply_controls(&mut self, map: &BTreeMap<String, Value>) {
        if let Some(g) = map.get("gain").and_then(|v| v.as_f64()) {
            self.gain = g as f32;
        }
        if let Some(p) = map.get("pan").and_then(|v| v.as_f64()) {
            self.pan = p as f32;
        }
        if let Some(s) = map.get("speed").and_then(|v| v.as_f64()) {
            self.speed = s as f32;
        }
        if let Some(c) = map.get("cutoff").and_then(|v| v.as_f64()) {
            self.cutoff = Some(c as f32);
        }
        if let Some(q) = map.get("resonance").and_then(|v| v.as_f64()) {
            self.resonance = (q as f32).max(0.1);
        }
        if let Some(room) = map.get("room").and_then(|v| v.as_f64()) {
            self.room = room as f32;
        }
        if let Some(d) = map.get("delay").and_then(|v| v.as_f64()) {
            self.delay = d as f32;
        }
        if let Some(b) = map.get("begin").and_then(|v| v.as_f64()) {
            self.begin = (b as f32).clamp(0.0, 1.0);
        }
        if let Some(e) = map.get("end").and_then(|v| v.as_f64()) {
            self.end = (e as f32).clamp(0.0, 1.0);
        }
        if let Some(u) = map.get("unit").and_then(|v| v.as_str()) {
            self.unit_cycles = u == "c";
        }
        if let Some(a) = map.get("attack").and_then(|v| v.as_f64()) {
            self.attack = a as f32;
        }
        if let Some(r) = map.get("release").and_then(|v| v.as_f64()) {
            self.release = r as f32;
        }
    }
}

/// A sounding sample playback voice with linear interpolation.
pub struct SamplerVoice {
    sample: Arc<Sample>,
    pos: f64,
    step: f64,
    end_pos: f64,
    gain: f32,
    left_gain: f32,
    right_gain: f32,
    attack: f32,
    release: f32,
    t: f32,
    hold_end: f32,
    sample_rate: f32,
    room: f32,
    delay: f32,
    filter: Option<Biquad>,
    done: bool,
}

impl SamplerVoice {
    pub fn new(params: SamplerParams, sample_rate: f32) -> SamplerVoice {
        let len = params.sample.data.len();
        let begin = (params.begin as f64 * len as f64).clamp(0.0, len as f64);
        let end = (params.end as f64 * len as f64).clamp(begin, len as f64);
        let pan = params.pan.clamp(0.0, 1.0);
        // With `unit: 'c'` the speed is in cycles, so scale by the sample's
        // duration in seconds (matches superdough: rate *= buffer.duration).
        let speed = if params.unit_cycles {
            let duration_secs = len as f64 / params.sample.sample_rate as f64;
            params.speed as f64 * duration_secs
        } else {
            params.speed as f64
        };
        // resample ratio: source rate vs engine rate, times speed
        let step = (params.sample.sample_rate as f64 / sample_rate as f64) * speed;
        let natural = if step != 0.0 {
            (end - begin).abs() / step.abs() / sample_rate as f64
        } else {
            0.0
        };
        let hold_end = if params.duration > 0.0 {
            params.duration.min(natural as f32)
        } else {
            natural as f32
        };
        let filter = params
            .cutoff
            .map(|c| Biquad::lowpass(sample_rate, c, params.resonance));
        SamplerVoice {
            sample: params.sample.clone(),
            pos: begin,
            step,
            end_pos: end,
            gain: params.gain,
            left_gain: (pan * PI / 2.0).cos(),
            right_gain: (pan * PI / 2.0).sin(),
            attack: params.attack,
            release: params.release,
            t: 0.0,
            hold_end,
            sample_rate,
            room: params.room,
            delay: params.delay,
            filter,
            done: false,
        }
    }

    fn envelope(&self) -> f32 {
        if self.t < self.attack {
            self.t / self.attack.max(1e-9)
        } else if self.t > self.hold_end {
            (1.0 - (self.t - self.hold_end) / self.release.max(1e-9)).max(0.0)
        } else {
            1.0
        }
    }
}

impl VoiceLike for SamplerVoice {
    fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let i = self.pos.floor() as usize;
        if self.pos >= self.end_pos || i + 1 >= self.sample.data.len() {
            self.done = true;
            return (0.0, 0.0);
        }
        let frac = (self.pos - i as f64) as f32;
        let s0 = self.sample.data[i];
        let s1 = self.sample.data[i + 1];
        let mut s = s0 + (s1 - s0) * frac;
        if let Some(f) = &mut self.filter {
            s = f.process(s);
        }
        s *= self.envelope() * self.gain;

        self.pos += self.step;
        self.t += 1.0 / self.sample_rate;
        if self.t >= self.hold_end + self.release {
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
}

// ---------------------------------------------------------------------------
// Synthesized drums (TR-style). Strudel ships these as downloaded samples; for
// an offline native engine we synthesize the General-MIDI-ish drum kit so
// `s("bd sd hh")` works with no sample packs.

/// A synthesized drum-machine voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumKind {
    Bd, // bass drum
    Sd, // snare drum
    Rim, // rimshot
    Clap, // hand clap
    Hh, // closed hi-hat
    Oh, // open hi-hat
    Lt, // low tom
    Mt, // mid tom
    Ht, // high tom
    Rd, // ride cymbal
    Cr, // crash cymbal
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
}

impl DrumParams {
    pub fn new(kind: DrumKind) -> DrumParams {
        DrumParams {
            kind,
            gain: 1.0,
            pan: 0.5,
            room: 0.0,
            delay: 0.0,
        }
    }

    pub fn apply_controls(&mut self, map: &BTreeMap<String, Value>) {
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
            left_gain: (pan * PI / 2.0).cos(),
            right_gain: (pan * PI / 2.0).sin(),
            room: params.room,
            delay: params.delay,
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
        (2.0 * PI * self.phase).sin()
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
                let metal = (self.osc(5200.0) + (2.0 * PI * 8400.0 * t).sin()) * 0.2;
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
}

/// What to play for a note: a synth voice, a sampler voice, or a drum voice.
pub enum VoiceSpec {
    Synth(Box<VoiceParams>),
    Sampler(SamplerParams),
    Drum(DrumParams),
}

impl VoiceSpec {
    pub fn into_voice(self, sample_rate: f32) -> Box<dyn VoiceLike> {
        match self {
            VoiceSpec::Synth(p) => Box::new(Voice::new(*p, sample_rate)),
            VoiceSpec::Sampler(p) => Box::new(SamplerVoice::new(p, sample_rate)),
            VoiceSpec::Drum(p) => Box::new(DrumVoice::new(p, sample_rate)),
        }
    }

    /// Build the voice and, if any post-effects are active, wrap it in a
    /// [`PostFxVoice`].
    pub fn into_voice_with_fx(self, sample_rate: f32, fx: PostFx) -> Box<dyn VoiceLike> {
        let voice = self.into_voice(sample_rate);
        if fx.is_active() {
            Box::new(PostFxVoice::new(voice, fx, sample_rate))
        } else {
            voice
        }
    }
}

// ---------------------------------------------------------------------------
// Waveshaping / bitcrush / decimation post-effects (superdough crush/shape/
// distort/coarse worklets). Applied per voice, after the voice renders.

/// A vowel for the formant filter (`vowel("a e i o u")`).
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
            vowel: map.get("vowel").and_then(|v| v.as_str()).and_then(Vowel::from_name),
        }
    }

    pub fn is_active(&self) -> bool {
        self.crush.is_some()
            || self.shape.is_some()
            || self.distort.is_some()
            || self.coarse.is_some()
            || self.vowel.is_some()
            || self.postgain != 1.0
    }
}

/// Wraps a voice and applies [`PostFx`] to its stereo output.
pub struct PostFxVoice {
    inner: Box<dyn VoiceLike>,
    fx: PostFx,
    coarse_hold: (f32, f32),
    coarse_count: u32,
    /// Per-channel formant banks when `vowel` is set.
    vowel: Option<(Formant, Formant)>,
}

impl PostFxVoice {
    pub fn new(inner: Box<dyn VoiceLike>, fx: PostFx, sample_rate: f32) -> PostFxVoice {
        let vowel = fx
            .vowel
            .map(|v| (Formant::new(v, sample_rate), Formant::new(v, sample_rate)));
        PostFxVoice {
            inner,
            fx,
            coarse_hold: (0.0, 0.0),
            coarse_count: 0,
            vowel,
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
        if self.fx.postgain != 1.0 {
            l *= self.fx.postgain;
            r *= self.fx.postgain;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name_to_midi("a4"), Some(69));
        assert_eq!(note_name_to_midi("c4"), Some(60));
        assert_eq!(note_name_to_midi("c3"), Some(48));
        assert_eq!(note_name_to_midi("c#4"), Some(61));
        assert_eq!(note_name_to_midi("eb3"), Some(51));
        assert_eq!(note_name_to_midi("c"), Some(48)); // default octave 3
    }

    #[test]
    fn mtof_a4() {
        assert!((mtof(69.0) - 440.0).abs() < 0.001);
    }

    #[test]
    fn voice_produces_sound_then_finishes() {
        let p = VoiceParams {
            duration: 0.01,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        let mut peak = 0.0f32;
        for _ in 0..44100 {
            let (l, _r) = v.tick();
            peak = peak.max(l.abs());
            if v.is_done() {
                break;
            }
        }
        assert!(peak > 0.0, "voice should produce non-silent output");
        assert!(v.is_done(), "voice should finish after its envelope");
    }

    #[test]
    fn drum_names_resolve() {
        assert_eq!(DrumKind::from_name("bd"), Some(DrumKind::Bd));
        assert_eq!(DrumKind::from_name("hh"), Some(DrumKind::Hh));
        assert_eq!(DrumKind::from_name("oh"), Some(DrumKind::Oh));
        assert_eq!(DrumKind::from_name("rim"), Some(DrumKind::Rim));
        assert_eq!(DrumKind::from_name("sawtooth"), None);
    }

    #[test]
    fn drum_produces_sound_then_finishes() {
        for kind in [
            DrumKind::Bd,
            DrumKind::Sd,
            DrumKind::Hh,
            DrumKind::Oh,
            DrumKind::Rim,
            DrumKind::Clap,
            DrumKind::Lt,
            DrumKind::Mt,
            DrumKind::Ht,
            DrumKind::Rd,
            DrumKind::Cr,
        ] {
            let mut v = DrumVoice::new(DrumParams::new(kind), 44100.0);
            let mut peak = 0.0f32;
            let mut ticks = 0;
            for _ in 0..(44100 * 2) {
                let (l, _r) = v.tick();
                peak = peak.max(l.abs());
                ticks += 1;
                if v.is_done() {
                    break;
                }
            }
            assert!(peak > 0.0, "{kind:?} should produce sound");
            assert!(v.is_done(), "{kind:?} should finish");
            assert!(ticks < 44100 * 2, "{kind:?} should finish within 2s");
        }
    }

    /// A test voice emitting a fixed stereo value, never done.
    struct ConstVoice(f32);
    impl VoiceLike for ConstVoice {
        fn tick(&mut self) -> (f32, f32) {
            (self.0, self.0)
        }
        fn is_done(&self) -> bool {
            false
        }
        fn room(&self) -> f32 {
            0.0
        }
        fn delay_send(&self) -> f32 {
            0.0
        }
    }

    #[test]
    fn noise_names_and_sound() {
        assert_eq!(NoiseKind::from_name("white"), Some(NoiseKind::White));
        assert_eq!(NoiseKind::from_name("pink"), Some(NoiseKind::Pink));
        assert_eq!(NoiseKind::from_name("brown"), Some(NoiseKind::Brown));
        assert_eq!(NoiseKind::from_name("sine"), None);
        for kind in [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown] {
            let p = VoiceParams {
                noise: Some(kind),
                duration: 0.1,
                ..Default::default()
            };
            let mut v = Voice::new(p, 44100.0);
            let mut peak = 0.0f32;
            for _ in 0..2000 {
                peak = peak.max(v.tick().0.abs());
            }
            assert!(peak > 0.0, "{kind:?} noise should produce sound");
        }
    }

    #[test]
    fn supersaw_produces_sound() {
        let p = VoiceParams {
            supersaw: true,
            unison: 5,
            spread: 0.4,
            freq: 220.0,
            duration: 0.2,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        let mut peak = 0.0f32;
        for _ in 0..4000 {
            peak = peak.max(v.tick().0.abs());
        }
        assert!(peak > 0.0, "supersaw should produce sound");
    }

    #[test]
    fn fm_changes_the_signal() {
        let mk = |fm| {
            Voice::new(
                VoiceParams {
                    waveform: Waveform::Sine,
                    freq: 220.0,
                    duration: 1.0,
                    fm,
                    fmh: 2.0,
                    ..Default::default()
                },
                44100.0,
            )
        };
        let (mut plain, mut modulated) = (mk(None), mk(Some(4.0)));
        let mut diff = 0.0f32;
        for _ in 0..2000 {
            diff += (plain.tick().0 - modulated.tick().0).abs();
        }
        assert!(diff > 0.0, "FM should change the carrier signal");
    }

    #[test]
    fn vibrato_and_pitch_env_change_pitch() {
        let base = || VoiceParams {
            waveform: Waveform::Sine,
            freq: 220.0,
            duration: 1.0,
            ..Default::default()
        };
        // vibrato vs none
        let mut plain = Voice::new(base(), 44100.0);
        let mut vibd = Voice::new(
            VoiceParams {
                vib: Some(6.0),
                vibmod: 1.0,
                ..base()
            },
            44100.0,
        );
        let mut diff = 0.0f32;
        for _ in 0..4000 {
            diff += (plain.tick().0 - vibd.tick().0).abs();
        }
        assert!(diff > 0.0, "vibrato should change the pitch over time");

        // pitch envelope vs none
        let mut penvd = Voice::new(
            VoiceParams {
                penv: Some(12.0),
                pattack: Some(0.2),
                ..base()
            },
            44100.0,
        );
        let mut plain2 = Voice::new(base(), 44100.0);
        let mut diff2 = 0.0f32;
        for _ in 0..4000 {
            diff2 += (plain2.tick().0 - penvd.tick().0).abs();
        }
        assert!(diff2 > 0.0, "pitch envelope should bend the pitch");
    }

    #[test]
    fn adsr_shortcut_parses_list() {
        let map = BTreeMap::from([(
            "adsr".to_string(),
            Value::List(vec![
                Value::F64(0.1),
                Value::F64(0.2),
                Value::F64(0.3),
                Value::F64(0.4),
            ]),
        )]);
        let p = VoiceParams::from_controls(&map, 0.5);
        assert_eq!(p.adsr.attack, 0.1);
        assert_eq!(p.adsr.decay, 0.2);
        assert_eq!(p.adsr.sustain, 0.3);
        assert_eq!(p.adsr.release, 0.4);
    }

    #[test]
    fn vowel_formant_shapes_noise() {
        assert_eq!(Vowel::from_name("a"), Some(Vowel::A));
        assert_eq!(Vowel::from_name("z"), None);
        // white noise through the "a" formant should still produce output.
        let p = VoiceParams {
            noise: Some(NoiseKind::White),
            duration: 1.0,
            ..Default::default()
        };
        let voice = Box::new(Voice::new(p, 44100.0));
        let fx = PostFx {
            vowel: Some(Vowel::A),
            ..Default::default()
        };
        assert!(fx.is_active());
        let mut v = PostFxVoice::new(voice, fx, 44100.0);
        let mut peak = 0.0f32;
        for _ in 0..4000 {
            peak = peak.max(v.tick().0.abs());
        }
        assert!(peak > 0.0, "vowel formant should pass some signal");
    }

    #[test]
    fn postfx_active_flag() {
        assert!(!PostFx::default().is_active());
        assert!(
            PostFx {
                crush: Some(4.0),
                ..Default::default()
            }
            .is_active()
        );
    }

    #[test]
    fn crush_quantizes_to_levels() {
        // crush=2 bits -> step = 2^(2-1) = 2, so values snap to multiples of 0.5
        let fx = PostFx {
            crush: Some(2.0),
            postgain: 1.0,
            shapevol: 1.0,
            distortvol: 1.0,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(0.3)), fx, 44100.0);
        let (l, _) = v.tick();
        assert_eq!(l, 0.5); // round(0.3*2)/2 = round(0.6)/2 = 1/2
    }

    #[test]
    fn coarse_holds_samples() {
        // coarse=3: a ramping source is held for 3-sample windows
        struct Ramp(f32);
        impl VoiceLike for Ramp {
            fn tick(&mut self) -> (f32, f32) {
                self.0 += 1.0;
                (self.0, self.0)
            }
            fn is_done(&self) -> bool {
                false
            }
            fn room(&self) -> f32 {
                0.0
            }
            fn delay_send(&self) -> f32 {
                0.0
            }
        }
        let fx = PostFx {
            coarse: Some(3.0),
            postgain: 1.0,
            shapevol: 1.0,
            distortvol: 1.0,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(Ramp(0.0)), fx, 44100.0);
        let out: Vec<f32> = (0..6).map(|_| v.tick().0).collect();
        // first sample of each window held across the window
        assert_eq!(out, vec![1.0, 1.0, 1.0, 4.0, 4.0, 4.0]);
    }

    #[test]
    fn distort_boosts_small_signal() {
        let fx = PostFx {
            distort: Some(2.0),
            postgain: 1.0,
            shapevol: 1.0,
            distortvol: 1.0,
            ..Default::default()
        };
        let mut v = PostFxVoice::new(Box::new(ConstVoice(0.1)), fx, 44100.0);
        let (l, _) = v.tick();
        assert!(l > 0.1, "distortion should boost a small input, got {l}");
    }

    #[test]
    fn highpass_attenuates_low_frequencies() {
        // A low tone through a high cutoff should be much quieter than open.
        let mk = |hcutoff| {
            Voice::new(
                VoiceParams {
                    freq: 100.0,
                    duration: 1.0,
                    hp: FilterParams {
                        freq: hcutoff,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                44100.0,
            )
        };
        let (mut open, mut filtered) = (mk(None), mk(Some(4000.0)));
        let (mut e_open, mut e_filt) = (0.0f32, 0.0f32);
        for _ in 0..8000 {
            e_open += open.tick().0.abs();
            e_filt += filtered.tick().0.abs();
        }
        assert!(e_filt < e_open * 0.5, "highpass should cut the low tone");
    }

    #[test]
    fn filter_envelope_opens_cutoff() {
        // A 4kHz tone is killed by a static lp at 200Hz; with lpenv the cutoff
        // sweeps up during the attack and lets much more through.
        let mk = |env: Option<f32>, attack: Option<f32>| {
            Voice::new(
                VoiceParams {
                    freq: 4000.0,
                    duration: 1.0,
                    lp: FilterParams {
                        freq: Some(200.0),
                        env,
                        attack,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                44100.0,
            )
        };
        let mut stat = mk(None, None);
        let mut swept = mk(Some(6.0), Some(0.2)); // opens ~6 octaves over 0.2s
        let (mut e_stat, mut e_swept) = (0.0f32, 0.0f32);
        for _ in 0..4410 {
            e_stat += stat.tick().0.abs();
            e_swept += swept.tick().0.abs();
        }
        assert!(
            e_swept > e_stat * 2.0,
            "filter env should open the cutoff (swept {e_swept} vs static {e_stat})"
        );
    }

    #[test]
    fn lowpass_attenuates_high_frequencies() {
        // A high tone through a low cutoff should be much quieter than open.
        let mut open = Voice::new(
            VoiceParams {
                freq: 6000.0,
                duration: 1.0,
                ..Default::default()
            },
            44100.0,
        );
        let mut filtered = Voice::new(
            VoiceParams {
                freq: 6000.0,
                duration: 1.0,
                lp: FilterParams {
                    freq: Some(200.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            44100.0,
        );
        let (mut e_open, mut e_filt) = (0.0f32, 0.0f32);
        for _ in 0..8000 {
            e_open += open.tick().0.abs();
            e_filt += filtered.tick().0.abs();
        }
        assert!(
            e_filt < e_open * 0.5,
            "filtered energy {e_filt} should be well below open {e_open}"
        );
    }

    #[test]
    fn sampler_plays_a_buffer_then_finishes() {
        // a 0.1s buffer of a 200 Hz sine
        let sr = 44100.0;
        let n = (sr * 0.1) as usize;
        let data: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 200.0 * i as f32 / sr).sin())
            .collect();
        let sample = Arc::new(Sample {
            data,
            sample_rate: sr,
        });
        let mut v = SamplerVoice::new(SamplerParams::new(sample), sr);
        let mut peak = 0.0f32;
        let mut frames = 0;
        while !v.is_done() && frames < 44100 {
            peak = peak.max(v.tick().0.abs());
            frames += 1;
        }
        assert!(peak > 0.0, "sampler should produce output");
        assert!(v.is_done(), "sampler should finish at the buffer end");
        assert!(frames < 44100, "sampler should not run forever");
    }

    #[test]
    fn sampler_speed_changes_duration() {
        let sr = 44100.0;
        let data = vec![0.5f32; 4410]; // 0.1s of DC
        let sample = Arc::new(Sample {
            data,
            sample_rate: sr,
        });
        let mut fast = SamplerParams::new(sample.clone());
        fast.speed = 2.0;
        let mut v = SamplerVoice::new(fast, sr);
        let mut frames = 0;
        while !v.is_done() && frames < 44100 {
            v.tick();
            frames += 1;
        }
        // at 2x speed the 0.1s buffer should take ~0.05s (~2205 frames)
        assert!(frames < 3000, "2x speed should play back in ~half the time");
    }

    #[test]
    fn pan_hard_left_silences_right() {
        let p = VoiceParams {
            pan: 0.0,
            ..Default::default()
        };
        let mut v = Voice::new(p, 44100.0);
        // skip the very start so the envelope has opened
        for _ in 0..100 {
            v.tick();
        }
        let (l, r) = v.tick();
        assert!(l.abs() > 0.0);
        assert!(r.abs() < 1e-6);
    }
}
