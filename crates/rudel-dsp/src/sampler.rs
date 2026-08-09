use crate::{
    filter::{FilterKind, FilterModel, FilterParams, VoiceFilter},
    modulator::{ModBank, ModSpec, ModTarget},
    pitch::PitchMod,
    voice::VoiceLike,
};
use rudel_core::ValueMap;
use std::{f32::consts::FRAC_PI_2, sync::Arc};

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
    /// `ftype`: which lowpass model to run (12dB biquad, Moog ladder, 24dB).
    pub model: FilterModel,
    /// `drive`: ladder input drive; unused by the biquad models.
    pub drive: f32,
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
    /// Vibrato + pitch envelope (`vib`/`vibmod`/`penv`/...), which superdough
    /// applies to a sampler's `detune` exactly as it does to a synth's.
    pub pitch: PitchMod,
}

impl SamplerParams {
    pub fn new(sample: Arc<Sample>) -> SamplerParams {
        SamplerParams {
            sample,
            gain: 1.0,
            pan: 0.5,
            speed: 1.0,
            attack: 0.001,
            // superdough's `getADSRValues` with nothing set: `[0.001, 0.001, 1,
            // 0.01]`. The release only shows when `clip`/`loop` cut a sample
            // short of its own end — and there it was five times too long, so a
            // clipped note bled into the next one.
            release: 0.01,
            cutoff: None,
            resonance: 0.707,
            model: FilterModel::Db12,
            drive: 0.69,
            duration: 0.0,
            begin: 0.0,
            end: 1.0,
            unit_cycles: false,
            loop_on: false,
            loop_begin: 0.0,
            loop_end: 1.0,
            pitch: PitchMod::default(),
        }
    }

    /// Apply common controls from a map.
    pub fn apply_controls(&mut self, map: &ValueMap) {
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
        if let Some(v) = map.get("ftype") {
            self.model = FilterModel::from_value(v);
        }
        if let Some(d) = map.get("drive").and_then(|v| v.as_f64()) {
            self.drive = d as f32;
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
        self.pitch = PitchMod::from_controls(map);
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
    filter: Option<VoiceFilter>,
    /// Modulators targeting this voice (gain and the lowpass).
    mods: ModBank,
    done: bool,
    /// Looping: when active, `pos` wraps within `[loop_start, loop_end)` (in
    /// sample frames) and the voice plays until `hold_end` rather than the slice
    /// end. Only forward playback (`step > 0`) loops.
    loop_on: bool,
    loop_start: f64,
    loop_end: f64,
    /// Vibrato / pitch envelope, scaling the read step per sample. `None` when
    /// neither is set, which is the overwhelmingly common case.
    pitch: Option<PitchMod>,
    /// `speed < 0`: read the buffer back-to-front. `pos` still walks forwards,
    /// so everything positional (`begin`/`end`, looping, the hold timer) works
    /// unchanged — only the frame lookup flips.
    rev: bool,
}

impl SamplerVoice {
    pub fn new(params: SamplerParams, sample_rate: f32) -> SamplerVoice {
        SamplerVoice::with_mods(params, sample_rate, &[])
    }

    /// Build a sampler voice with modulators bound to its parameters.
    pub fn with_mods(params: SamplerParams, sample_rate: f32, mods: &[ModSpec]) -> SamplerVoice {
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
        // resample ratio: source rate vs engine rate, times speed. A negative
        // speed plays the buffer *reversed* at |speed| — superdough swaps in a
        // reversed copy and uses `Math.abs(speed)` as the rate, so `begin`/`end`
        // and looping index the reversed buffer. Stepping backwards from `begin`
        // instead would run straight off the front of the sample.
        let rev = speed < 0.0;
        let step = (params.sample.sample_rate as f64 / sample_rate as f64) * speed.abs();
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
        // The sampler's lowpass has no envelope, so it reuses the voice filter
        // slot with `env` unset — that also gives it `ftype`/`drive` for free.
        let filter = params.cutoff.map(|c| {
            let fp = FilterParams {
                freq: Some(c),
                q: params.resonance,
                model: params.model,
                drive: params.drive,
                ..FilterParams::default()
            };
            VoiceFilter::new(FilterKind::Low, &fp, sample_rate)
        });
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
            filter,
            mods: ModBank::new(mods, sample_rate as f64),
            done: false,
            loop_on,
            loop_start,
            loop_end,
            pitch: (!params.pitch.is_idle()).then_some(params.pitch),
            rev,
        }
    }

    /// Read frame `i`, counting from the end of the buffer when the voice is
    /// reversed. `i + 1 < len` is checked by the caller, so both are in bounds.
    fn frame(&self, i: usize) -> f32 {
        let data = &self.sample.data;
        if self.rev {
            data[data.len() - 1 - i]
        } else {
            data[i]
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
        let s0 = self.frame(i);
        let s1 = self.frame(i + 1);
        let mut s = s0 + (s1 - s0) * frac;
        self.mods.tick();
        if let Some(f) = &mut self.filter {
            let (ft, qt) = f.mod_targets();
            s = f.process(
                s,
                self.t,
                self.hold_end,
                self.sample_rate,
                self.mods.get(ft),
                self.mods.get(qt),
            );
        }
        s *= self.envelope() * (self.gain + self.mods.get(ModTarget::Gain));

        // Vibrato / pitch envelope detune the playback rate, which is what
        // superdough's `getVibratoOscillator`/`getPitchEnvelope` do to a
        // sampler's `detune`.
        self.pos += match &self.pitch {
            Some(p) => self.step * p.multiplier(self.t, self.hold_end) as f64,
            None => self.step,
        };
        self.t += 1.0 / self.sample_rate;
        if self.t >= self.hold_end + self.release {
            self.done = true;
        }
        (s * self.left_gain, s * self.right_gain)
    }
    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        self.mods.set_bus_input(bus, left, right);
    }
    fn is_done(&self) -> bool {
        self.done
    }
}

// ---------------------------------------------------------------------------
// Synthesized drums (TR-style). Strudel ships these as downloaded samples; for
// an offline native engine we synthesize the General-MIDI-ish drum kit so
// `s("bd sd hh")` works with no sample packs.
