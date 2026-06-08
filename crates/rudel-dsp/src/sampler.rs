use crate::filter::Biquad;
use crate::voice::VoiceLike;
use rudel_core::Value;
use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

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
    /// `ftype` 24dB: cascade the lowpass twice for a steeper slope.
    pub cascade: bool,
    pub room: f32,
    pub delay: f32,
    /// Dry (direct) signal level (`dry`), 0..1. Defaults to full.
    pub dry: f32,
    /// Hold time in seconds (0 = play to the sample's natural end).
    pub duration: f32,
    /// Start/end positions as fractions of the sample (0..1).
    pub begin: f32,
    pub end: f32,
    /// When true (`unit: 'c'`), `speed` is interpreted in cycles: the effective
    /// playback rate is multiplied by the sample's duration in seconds. Used by
    /// `loopAt`/`fit`/`splice` to time-stretch a sample.
    pub unit_cycles: bool,
    /// `loop`: when true, the sample loops between `loop_begin`/`loop_end` for
    /// the duration of the hap instead of playing once to its natural end.
    pub loop_on: bool,
    /// Loop region start/end as fractions of the sample (0..1).
    pub loop_begin: f32,
    pub loop_end: f32,
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
            cascade: false,
            room: 0.0,
            delay: 0.0,
            dry: 1.0,
            duration: 0.0,
            begin: 0.0,
            end: 1.0,
            unit_cycles: false,
            loop_on: false,
            loop_begin: 0.0,
            loop_end: 1.0,
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
        // `ftype` 24dB cascades the lowpass twice (see params.rs); 'ladder' is
        // not ported and falls back to the default 12dB single biquad.
        self.cascade = match map.get("ftype") {
            Some(Value::Str(s)) => s == "24db",
            Some(v) => v
                .as_f64()
                .map(|f| f.rem_euclid(3.0).floor() as i32 == 2)
                .unwrap_or(false),
            None => self.cascade,
        };
        if let Some(room) = map.get("room").and_then(|v| v.as_f64()) {
            self.room = room as f32;
        }
        if let Some(d) = map.get("delay").and_then(|v| v.as_f64()) {
            self.delay = d as f32;
        }
        if let Some(dry) = map.get("dry").and_then(|v| v.as_f64()) {
            self.dry = dry as f32;
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
        if let Some(l) = map.get("loop").and_then(|v| v.as_f64()) {
            self.loop_on = l != 0.0;
        }
        if let Some(b) = map.get("loopBegin").and_then(|v| v.as_f64()) {
            self.loop_begin = (b as f32).clamp(0.0, 1.0);
        }
        if let Some(e) = map.get("loopEnd").and_then(|v| v.as_f64()) {
            self.loop_end = (e as f32).clamp(0.0, 1.0);
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
    dry: f32,
    filter: Option<Biquad>,
    /// Second cascaded lowpass for `ftype` 24dB.
    filter2: Option<Biquad>,
    done: bool,
    /// Looping: when active, `pos` wraps within `[loop_start, loop_end)` (in
    /// sample frames) and the voice plays until `hold_end` rather than the slice
    /// end. Only forward playback (`step > 0`) loops.
    loop_on: bool,
    loop_start: f64,
    loop_end: f64,
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
        // Loop region in sample frames. Keep at least one frame of headroom below
        // the buffer end so interpolation (`data[i+1]`) stays in bounds.
        let loop_start = (params.loop_begin as f64 * len as f64).clamp(0.0, len as f64);
        let loop_end = (params.loop_end as f64 * len as f64).clamp(0.0, (len.max(1) - 1) as f64);
        let loop_on = params.loop_on && step > 0.0 && loop_end > loop_start;
        let hold_end = if loop_on {
            // Looping plays for the hap's duration (no natural-length cap).
            params.duration.max(0.0)
        } else if params.duration > 0.0 {
            params.duration.min(natural as f32)
        } else {
            natural as f32
        };
        let filter = params
            .cutoff
            .map(|c| Biquad::lowpass(sample_rate, c, params.resonance));
        let filter2 = (params.cascade)
            .then(|| {
                params
                    .cutoff
                    .map(|c| Biquad::lowpass(sample_rate, c, params.resonance))
            })
            .flatten();
        SamplerVoice {
            sample: params.sample.clone(),
            pos: begin,
            step,
            end_pos: end,
            gain: params.gain,
            left_gain: (pan * FRAC_PI_2).cos(),
            right_gain: (pan * FRAC_PI_2).sin(),
            attack: params.attack,
            release: params.release,
            t: 0.0,
            hold_end,
            sample_rate,
            room: params.room,
            delay: params.delay,
            dry: params.dry,
            filter,
            filter2,
            done: false,
            loop_on,
            loop_start,
            loop_end,
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
        // Looping wraps the read position back to the loop start.
        if self.loop_on {
            while self.pos >= self.loop_end {
                self.pos -= self.loop_end - self.loop_start;
            }
        }
        let i = self.pos.floor() as usize;
        // A looping voice never ends on position; it stops via the hold timer.
        if (!self.loop_on && self.pos >= self.end_pos) || i + 1 >= self.sample.data.len() {
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
        if let Some(f) = &mut self.filter2 {
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
    fn dry(&self) -> f32 {
        self.dry
    }
}

// ---------------------------------------------------------------------------
// Synthesized drums (TR-style). Strudel ships these as downloaded samples; for
// an offline native engine we synthesize the General-MIDI-ish drum kit so
// `s("bd sd hh")` works with no sample packs.
