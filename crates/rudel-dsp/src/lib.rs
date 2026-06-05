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
    pub freq: f32,
    pub gain: f32,
    /// 0.0 = hard left, 1.0 = hard right.
    pub pan: f32,
    pub adsr: Adsr,
    /// Hold time in seconds (the note's `whole` duration), before release.
    pub duration: f32,
    /// Low-pass cutoff in Hz (`cutoff`/`lpf`). `None` leaves the voice open.
    pub cutoff: Option<f32>,
    /// Low-pass resonance / Q (`resonance`/`lpq`).
    pub resonance: f32,
    /// High-pass cutoff in Hz (`hcutoff`/`hpf`).
    pub hcutoff: Option<f32>,
    /// High-pass resonance / Q (`hresonance`/`hpq`).
    pub hresonance: f32,
    /// Band-pass center in Hz (`bandf`/`bpf`).
    pub bandf: Option<f32>,
    /// Band-pass Q (`bandq`/`bpq`).
    pub bandq: f32,
    /// Reverb send amount (`room`), 0..1.
    pub room: f32,
    /// Delay send amount (`delay`), 0..1.
    pub delay: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams {
            waveform: Waveform::Sine,
            freq: 440.0,
            gain: 1.0,
            pan: 0.5,
            adsr: Adsr::default(),
            duration: 0.25,
            cutoff: None,
            resonance: 0.707,
            hcutoff: None,
            hresonance: 0.707,
            bandf: None,
            bandq: 1.0,
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
        if let Some(name) = map.get("s").and_then(|v| v.as_str())
            && let Some(w) = Waveform::from_name(name)
        {
            p.waveform = w;
        }
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
        if let Some(c) = map.get("cutoff").and_then(|v| v.as_f64()) {
            p.cutoff = Some(c as f32);
        }
        if let Some(q) = map.get("resonance").and_then(|v| v.as_f64()) {
            p.resonance = (q as f32).max(0.1);
        }
        if let Some(c) = map.get("hcutoff").and_then(|v| v.as_f64()) {
            p.hcutoff = Some(c as f32);
        }
        if let Some(q) = map.get("hresonance").and_then(|v| v.as_f64()) {
            p.hresonance = (q as f32).max(0.1);
        }
        if let Some(c) = map.get("bandf").and_then(|v| v.as_f64()) {
            p.bandf = Some(c as f32);
        }
        if let Some(q) = map.get("bandq").and_then(|v| v.as_f64()) {
            p.bandq = (q as f32).max(0.1);
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

impl Biquad {
    fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        let cutoff = cutoff.clamp(20.0, sample_rate * 0.45);
        let w0 = 2.0 * PI * cutoff / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn highpass(sample_rate: f32, cutoff: f32, q: f32) -> Biquad {
        let cutoff = cutoff.clamp(20.0, sample_rate * 0.45);
        let w0 = 2.0 * PI * cutoff / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 + cos) / 2.0 / a0,
            b1: -(1.0 + cos) / a0,
            b2: (1.0 + cos) / 2.0 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn bandpass(sample_rate: f32, center: f32, q: f32) -> Biquad {
        let center = center.clamp(20.0, sample_rate * 0.45);
        let w0 = 2.0 * PI * center / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha;
        // constant 0 dB peak gain (b0 = alpha)
        Biquad {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
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
    filters: Vec<Biquad>,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * PI / 2.0).cos();
        let right_gain = (pan * PI / 2.0).sin();
        let hold_end = params.duration.max(params.adsr.attack);
        let mut filters = Vec::new();
        if let Some(c) = params.cutoff {
            filters.push(Biquad::lowpass(sample_rate, c, params.resonance));
        }
        if let Some(c) = params.hcutoff {
            filters.push(Biquad::highpass(sample_rate, c, params.hresonance));
        }
        if let Some(c) = params.bandf {
            filters.push(Biquad::bandpass(sample_rate, c, params.bandq));
        }
        Voice {
            params,
            sample_rate,
            phase: 0.0,
            t: 0.0,
            left_gain,
            right_gain,
            hold_end,
            filters,
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
        let Adsr {
            attack,
            decay,
            sustain,
            release,
        } = self.params.adsr;
        let t = self.t;
        if t < attack {
            t / attack.max(1e-9)
        } else if t < attack + decay {
            1.0 - (1.0 - sustain) * ((t - attack) / decay.max(1e-9))
        } else if t < self.hold_end {
            sustain
        } else if t < self.hold_end + release {
            sustain * (1.0 - (t - self.hold_end) / release.max(1e-9))
        } else {
            0.0
        }
    }

    /// Render the next stereo sample `(left, right)`.
    pub fn tick(&mut self) -> (f32, f32) {
        if self.done {
            return (0.0, 0.0);
        }
        let env = self.envelope();
        let mut osc = self.params.waveform.sample(self.phase);
        for f in &mut self.filters {
            osc = f.process(osc);
        }
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let s = osc * env * self.params.gain * 0.3;

        self.phase = (self.phase + self.params.freq / self.sample_rate).rem_euclid(1.0);
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
    Synth(VoiceParams),
    Sampler(SamplerParams),
    Drum(DrumParams),
}

impl VoiceSpec {
    pub fn into_voice(self, sample_rate: f32) -> Box<dyn VoiceLike> {
        match self {
            VoiceSpec::Synth(p) => Box::new(Voice::new(p, sample_rate)),
            VoiceSpec::Sampler(p) => Box::new(SamplerVoice::new(p, sample_rate)),
            VoiceSpec::Drum(p) => Box::new(DrumVoice::new(p, sample_rate)),
        }
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

    #[test]
    fn highpass_attenuates_low_frequencies() {
        // A low tone through a high cutoff should be much quieter than open.
        let mk = |hcutoff| {
            Voice::new(
                VoiceParams {
                    freq: 100.0,
                    duration: 1.0,
                    hcutoff,
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
                cutoff: Some(200.0),
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
