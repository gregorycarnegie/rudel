// wavetable.rs - the wavetable oscillator, ported from superdough's
// `WavetableOscillatorProcessor` (`worklets.mjs`) and the parameter wiring in
// `wavetable.mjs`'s `onTriggerSynth`.
//
// A wavetable sound is a `.wav` file sliced into equal-length single-cycle
// frames (`tables(url, frameLen)`). `wt` picks a position through the frame
// stack (interpolating between neighbours), `warp`/`warpmode` distort the read
// phase before sampling, and `unison`/`detune`/`spread` stack detuned voices the
// same way the super-saw does.
//
// Everything here is deterministic and sample-rate-free apart from `dphase`, so
// the warp table can be golden-tested against the worklet.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    envelope::Adsr,
    modulator::{Lfo, LfoConfig},
};
use rudel_core::{Value, ValueMap};
use std::{f32::consts::TAU, sync::Arc};

/// The phase-distortion modes `warpmode` selects, in the worklet's numbering
/// (`WarpMode` in `worklets.mjs` / `Warpmode` in `wavetable.mjs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WarpMode {
    None = 0,
    Asym = 1,
    Mirror = 2,
    BendP = 3,
    BendM = 4,
    BendMP = 5,
    Sync = 6,
    Quant = 7,
    Fold = 8,
    Pwm = 9,
    Orbit = 10,
    Spin = 11,
    Chaos = 12,
    Primes = 13,
    Binary = 14,
    Brownian = 15,
    Reciprocal = 16,
    Wormhole = 17,
    Logistic = 18,
    Sigmoid = 19,
    Fractal = 20,
    Flip = 21,
}

impl WarpMode {
    /// Resolve a `warpmode` control value: a number is the mode index, a string
    /// is the mode name (`Warpmode[warpmode.toUpperCase()] ?? NONE` upstream).
    pub fn from_index(index: u8) -> WarpMode {
        use WarpMode::*;
        match index {
            1 => Asym,
            2 => Mirror,
            3 => BendP,
            4 => BendM,
            5 => BendMP,
            6 => Sync,
            7 => Quant,
            8 => Fold,
            9 => Pwm,
            10 => Orbit,
            11 => Spin,
            12 => Chaos,
            13 => Primes,
            14 => Binary,
            15 => Brownian,
            16 => Reciprocal,
            17 => Wormhole,
            18 => Logistic,
            19 => Sigmoid,
            20 => Fractal,
            21 => Flip,
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<WarpMode> {
        use WarpMode::*;
        Some(match name.to_ascii_lowercase().as_str() {
            "none" => None,
            "asym" => Asym,
            "mirror" => Mirror,
            "bendp" => BendP,
            "bendm" => BendM,
            "bendmp" => BendMP,
            "sync" => Sync,
            "quant" => Quant,
            "fold" => Fold,
            "pwm" => Pwm,
            "orbit" => Orbit,
            "spin" => Spin,
            "chaos" => Chaos,
            "primes" => Primes,
            "binary" => Binary,
            "brownian" => Brownian,
            "reciprocal" => Reciprocal,
            "wormhole" => Wormhole,
            "logistic" => Logistic,
            "sigmoid" => Sigmoid,
            "fractal" => Fractal,
            "flip" => Flip,
            _ => return Option::None,
        })
    }
}

// The worklet's integer helpers. `ffloor`/`fround` are JS's "fast integer ops
// for non-negative values" (`x | 0` truncates toward zero), so they are only
// equivalent to `floor` for non-negative input — which is all these see.
fn ffloor(x: f32) -> f32 {
    (x as i32) as f32
}
fn fround(x: f32) -> f32 {
    ffloor(x + 0.5)
}
fn ffrac(x: f32) -> f32 {
    x - ffloor(x)
}
fn frac(x: f32) -> f32 {
    x - x.floor()
}
fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// `hash32` + `hash01`: the worklet's integer hash, used by the brownian warp.
/// The JS arithmetic is on 32-bit signed values with unsigned shifts, so this
/// mirrors it with wrapping `i32`/`u32` ops rather than float math.
fn hash32(u: u32) -> u32 {
    let mut u = u;
    u = u.wrapping_add(0x7ed5_5d16).wrapping_add(u << 12);
    u = u ^ 0xc761_c23c ^ (u >> 19);
    u = u.wrapping_add(0x1656_67b1).wrapping_add(u << 5);
    u = u.wrapping_add(0xd3a2_646c) ^ (u << 9);
    u = u.wrapping_add(0xfd70_46c5).wrapping_add(u << 3);
    u ^ 0xb55a_4f09 ^ (u >> 16)
}

fn hash01(i: i32) -> f32 {
    (hash32(i as u32) >> 8) as f32 / 0x0100_0000 as f32
}

fn noise(x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let a = hash01(i as i32);
    let b = hash01(i as i32 + 1);
    a + (b - a) * f
}

fn brownian(x: f32, octaves: u32) -> f32 {
    let (mut amp, mut sum, mut norm, mut freq) = (0.5f32, 0.0f32, 0.0f32, 1.0f32);
    for _ in 0..octaves {
        sum += amp * noise(x * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    (sum / norm) * 2.0 - 1.0
}

fn bit_reverse(i: u32, n: u32) -> u32 {
    let mut i = i;
    let mut r = 0u32;
    for _ in 0..n {
        r = (r << 1) | (i & 1);
        i >>= 1;
    }
    r
}

fn mirror(x: f32) -> f32 {
    1.0 - (2.0 * x - 1.0).abs()
}

/// `_toBits`: map a 0..1 amount onto a bit count and its power of two.
fn to_bits(amt: f32, min: f32, max: f32) -> (f32, f32) {
    let b = max + (min - max) * amt;
    (b, fround(2f32.powf(b)))
}

fn is_prime(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    if n.is_multiple_of(2) {
        return n == 2;
    }
    let mut d = 3u32;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}

/// `_warpPhase`: distort the read phase before sampling the frame.
pub fn warp_phase(phase: f32, amt: f32, mode: WarpMode) -> f32 {
    use WarpMode::*;
    match mode {
        None | Flip => phase,
        Asym => {
            let a = 0.01 + 0.99 * amt;
            if phase < a {
                0.5 * phase / a
            } else {
                0.5 + 0.5 * (phase - a) / (1.0 - a)
            }
        }
        Mirror => mirror(warp_phase(phase, amt, Asym)),
        BendP => phase.powf(1.0 + 3.0 * amt),
        BendM => phase.powf(1.0 / (1.0 + 3.0 * amt)),
        // Upstream passes the raw mode numbers 3/2 here, i.e. BendP and Mirror.
        BendMP => {
            if amt < 0.5 {
                warp_phase(phase, 1.0 - 2.0 * amt, BendP)
            } else {
                warp_phase(phase, 2.0 * amt - 1.0, Mirror)
            }
        }
        Sync => {
            let sync_ratio = 16f32.powf(amt * amt);
            (phase * sync_ratio) % 1.0
        }
        Quant => {
            let (_, n) = to_bits(amt, 2.0, 12.0);
            ffloor(phase * n) / n
        }
        Fold => {
            const K: f32 = 7.0;
            let k = 1.0 + fround(K * amt).max(1.0);
            (ffrac(k * phase) - 0.5).abs() * 2.0
        }
        Pwm => {
            let w = clamp01(0.5 + 0.49 * (2.0 * amt - 1.0));
            if phase < w {
                (phase / w) * 0.5
            } else {
                0.5 + ((phase - w) / (1.0 - w)) * 0.5
            }
        }
        Orbit => {
            let depth = 0.5 * amt;
            frac(phase + depth * (TAU * 3.0 * phase).sin())
        }
        Spin => {
            let depth = 0.5 * amt;
            let (_, n) = to_bits(amt, 1.0, 6.0);
            frac(phase + depth * (TAU * n * phase).sin())
        }
        Chaos => {
            let r = 3.7 + 0.3 * amt;
            let logistic = r * phase * (1.0 - phase);
            clamp01((1.0 - amt) * phase + amt * logistic)
        }
        Primes => {
            let (_, n) = to_bits(amt, 3.0, 12.0);
            let mut n = n as u32;
            while !is_prime(n) {
                n += 1;
            }
            let n = n as f32;
            ffloor(phase * n) / n
        }
        Binary => {
            let (b, _) = to_bits(amt, 3.0, 12.0);
            let b = fround(b) as u32;
            let n = (1u32 << b) as f32;
            let idx = ffloor(phase * n) as u32;
            bit_reverse(idx, b) as f32 / n
        }
        Brownian => {
            let disp = 0.25 * amt * brownian(64.0 * phase, 4);
            frac(phase + disp)
        }
        Reciprocal => {
            let g = 2.0 + 4.0 * amt;
            let num = phase * g;
            let den = phase + (1.0 - phase) * g;
            let y = if den > 1e-12 { num / den } else { 0.0 };
            clamp01(y)
        }
        Wormhole => {
            let gap = clamp01(0.8 * amt);
            let a = 0.5 * (1.0 - gap);
            let b = 0.5 * (1.0 + gap);
            if phase < a {
                (phase / a) * 0.5
            } else if phase > b {
                0.5 * (1.0 + (phase - b) / (1.0 - b))
            } else {
                0.5
            }
        }
        Logistic => {
            let r = 3.6 + 0.4 * amt;
            let iters = 1 + fround(2.0 * amt) as u32;
            let mut x = phase;
            for _ in 0..iters {
                x = r * x * (1.0 - x);
            }
            clamp01(x)
        }
        Sigmoid => {
            let k = 1.0 + 10.0 * amt;
            let x = phase - 0.5;
            let y = 1.0 / (1.0 + (-k * x).exp());
            let y0 = 1.0 / (1.0 + (0.5 * k).exp());
            let y1 = 1.0 / (1.0 + (-0.5 * k).exp());
            (y - y0) / (y1 - y0)
        }
        Fractal => {
            let d = 0.5 * (TAU * phase).sin() * amt;
            frac(phase + d)
        }
    }
}

/// `_sampleFrame`: linear interpolation within one frame, wrapping at the end.
fn sample_frame(frame: &[f32], phase: f32) -> f32 {
    let len = frame.len();
    if len == 0 {
        return 0.0;
    }
    let pos = phase * len as f32;
    let mut i = pos as usize;
    if i >= len {
        i = 0; // the worklet's fast wrap
    }
    let frac = pos - i as f32;
    let a = frame[i];
    let b = frame[if i + 1 >= len { 0 } else { i + 1 }];
    a + (b - a) * frac
}

/// A loaded wavetable: the single-cycle frames a `.wav` was sliced into.
/// Shared between voices (one table, many notes), so it is behind an `Arc`.
#[derive(Clone, Debug)]
pub struct WaveTable {
    pub frames: Arc<Vec<Vec<f32>>>,
}

impl WaveTable {
    /// Slice `samples` into frames of `frame_len`, as `getPayload` does. A
    /// buffer shorter than one frame becomes a single (zero-padded) frame, so a
    /// short file still plays rather than producing silence.
    pub fn from_samples(samples: &[f32], frame_len: usize) -> WaveTable {
        let frame_len = frame_len.max(1);
        let num_frames = (samples.len() / frame_len).max(1);
        let frames: Vec<Vec<f32>> = (0..num_frames)
            .map(|i| {
                let start = i * frame_len;
                let end = (start + frame_len).min(samples.len());
                let mut frame = samples[start.min(samples.len())..end].to_vec();
                frame.resize(frame_len, 0.0);
                frame
            })
            .collect();
        WaveTable {
            frames: Arc::new(frames),
        }
    }
}

/// One wavetable voice's per-sample state: the unison phases, plus the constants
/// the worklet hoists out of its inner loop.
pub struct WavetableOsc {
    table: WaveTable,
    /// Per-unison-voice phase.
    phase: Vec<f32>,
    /// Per-unison-voice frequency ratio from `getDetuner`'s semitone spread.
    ratio: Vec<f32>,
    gain_l: Vec<f32>,
    gain_r: Vec<f32>,
    normalizer: f32,
    inv_sr: f32,
}

impl WavetableOsc {
    /// `voices` is `unison` (min 1), `freqspread` the `detune` semitone spread,
    /// `panspread` the stereo width, and `phaserand` how much of a random
    /// initial phase each voice gets.
    pub fn new(
        table: WaveTable,
        voices: usize,
        freqspread: f32,
        panspread: f32,
        phaserand: f32,
        sample_rate: f32,
        mut rand: impl FnMut() -> f32,
    ) -> WavetableOsc {
        let voices = voices.max(1);
        // `getDetuner(unison, detune)`.
        let scale = if voices > 1 {
            freqspread / (voices as f32 - 1.0)
        } else {
            0.0
        };
        let center = freqspread * 0.5;
        // The worklet forces panspread to 0 for a single voice, then takes the
        // sqrt pair around 0.5.
        let panspread = if voices > 1 { clamp01(panspread) } else { 0.0 };
        let gain1 = (0.5 - 0.5 * panspread).sqrt();
        let gain2 = (0.5 + 0.5 * panspread).sqrt();
        let mut phase = Vec::with_capacity(voices);
        let mut ratio = Vec::with_capacity(voices);
        let mut gain_l = Vec::with_capacity(voices);
        let mut gain_r = Vec::with_capacity(voices);
        for n in 0..voices {
            phase.push(rand() * clamp01(phaserand));
            let detune = if voices > 1 {
                n as f32 * scale - center
            } else {
                0.0
            };
            ratio.push(2f32.powf(detune / 12.0));
            // invert the left and right gain each voice
            let (l, r) = if n % 2 == 0 {
                (gain1, gain2)
            } else {
                (gain2, gain1)
            };
            gain_l.push(l);
            gain_r.push(r);
        }
        WavetableOsc {
            table,
            phase,
            ratio,
            gain_l,
            gain_r,
            normalizer: 1.0 / (voices as f32).sqrt(),
            inv_sr: 1.0 / sample_rate,
        }
    }

    /// Render one stereo sample at `freq` Hz, reading table position `position`
    /// (0..1) with `warp` amount `amt` in mode `mode`, and advance the phases.
    pub fn tick(&mut self, freq: f32, position: f32, amt: f32, mode: WarpMode) -> (f32, f32) {
        let frames = &*self.table.frames;
        if frames.is_empty() {
            return (0.0, 0.0);
        }
        let idx = clamp01(position) * (frames.len() - 1) as f32;
        let f_idx = idx as usize;
        let interp = idx - f_idx as f32;
        let next = (f_idx + 1).min(frames.len() - 1);
        let amt = clamp01(amt);
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for n in 0..self.phase.len() {
            let ph = warp_phase(self.phase[n], amt, mode);
            let s0 = sample_frame(&frames[f_idx], ph);
            let s1 = sample_frame(&frames[next], ph);
            let mut s = s0 + (s1 - s0) * interp;
            if mode == WarpMode::Flip && self.phase[n] < amt {
                s = -s;
            }
            out_l += s * self.gain_l[n] * self.normalizer;
            out_r += s * self.gain_r[n] * self.normalizer;
            let dphase = freq * self.ratio[n] * self.inv_sr;
            self.phase[n] = frac(self.phase[n] + dphase);
        }
        (out_l, out_r)
    }
}

/// The modulation superdough's `applyParameterModulators` puts on the `wt` and
/// `warp` params: a linear ADSR sweeping `offset .. offset + amount`, plus an
/// LFO summed on top. Both halves default to "off" unless one of their own
/// controls is set, exactly as `getParamADSR`/`getParamLfo` decide.
#[derive(Clone, Debug)]
pub struct ParamMod {
    /// The param's own (static) value, and the envelope's floor.
    pub offset: f32,
    /// Envelope sweep amount; 0 disables the envelope.
    pub amount: f32,
    pub adsr: Adsr,
    /// `None` when no LFO control was set (or its depth is 0).
    pub lfo: Option<LfoConfig>,
}

impl Default for ParamMod {
    /// A static parameter: no envelope, no LFO, value 0.
    fn default() -> ParamMod {
        ParamMod {
            offset: 0.0,
            amount: 0.0,
            adsr: Adsr {
                attack: 0.0,
                decay: 0.5,
                sustain: 0.0,
                release: 0.1,
            },
            lfo: Option::None,
        }
    }
}

impl ParamMod {
    /// Read the `{prefix}`-family controls (`wt`/`wtenv`/`wt{adsr}`/`wtrate`/…)
    /// out of an event's control map. `cps` scales `{prefix}sync`, and `time`
    /// is the event's cycle-locked start used for the LFO's initial phase.
    pub fn from_controls(map: &ValueMap, prefix: &str, cps: f64, time: f64) -> ParamMod {
        let get = |suffix: &str| -> Option<f64> {
            map.get(&format!("{prefix}{suffix}"))
                .and_then(Value::as_f64)
        };
        let offset = map.get(prefix).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let adsr_params = [get("attack"), get("decay"), get("sustain"), get("release")];
        // `amount == null` -> the default only when some ADSR value is set.
        let amount = match get("env") {
            Some(a) => a as f32,
            None if adsr_params.iter().any(Option::is_some) => 0.5,
            None => 0.0,
        };
        let adsr = adsr_from(&adsr_params, [0.0, 0.5, 0.0, 0.1]);
        // `{prefix}sync` is in cycles, `{prefix}rate` in Hz.
        let rate = match get("sync") {
            Some(sync) => Some(sync * cps),
            None => get("rate"),
        };
        let shape_ctl = map.get(&format!("{prefix}shape"));
        let skew = get("skew");
        // `wtdc`/`warpdc` default to 0 here rather than `getLfo`'s -0.5, as
        // `onTriggerSynth` passes `value.wtdc ?? 0`.
        let dcoffset = get("dc").unwrap_or(0.0);
        // `getParamLfo`: with no explicit depth, the LFO only runs when some
        // other LFO control was set, and then at `defaultDepth` (0.5 here).
        let depth = match get("depth") {
            Some(d) => d,
            None if rate.is_some() || skew.is_some() || shape_ctl.is_some() => 0.5,
            None => 0.0,
        };
        let lfo = (depth != 0.0).then(|| LfoConfig {
            shape: crate::modulator::shape_index(shape_ctl),
            frequency: rate.unwrap_or(1.0),
            skew: skew.unwrap_or(0.5),
            depth,
            dcoffset,
            phaseoffset: 0.0,
            curve: 1.0,
            // `getLfo`'s unwritten min/max.
            min: dcoffset * depth,
            max: dcoffset * depth + depth,
            time,
        });
        ParamMod {
            offset,
            amount,
            adsr,
            lfo,
        }
    }

    /// True when neither half does anything, so the caller can skip it and use
    /// the static value.
    pub fn is_static(&self) -> bool {
        self.amount == 0.0 && self.lfo.is_none()
    }
}

/// Superdough's `getADSRValues(values, 'linear', defaults)`: all-unset falls
/// back to `defaults`, otherwise each slot is floored/capped and `sustain`
/// depends on which of attack/decay were given.
fn adsr_from(values: &[Option<f64>; 4], defaults: [f32; 4]) -> Adsr {
    const ENV_MIN: f32 = 0.001;
    const RELEASE_MIN: f32 = 0.01;
    const ENV_MAX: f32 = 1.0;
    let [a, d, s, r] = *values;
    if a.is_none() && d.is_none() && s.is_none() && r.is_none() {
        let [attack, decay, sustain, release] = defaults;
        return Adsr {
            attack,
            decay,
            sustain,
            release,
        };
    }
    let sustain = match s {
        Some(s) => s as f32,
        None if d.is_none() => ENV_MAX,
        None => ENV_MIN,
    };
    Adsr {
        attack: (a.unwrap_or(0.0) as f32).max(ENV_MIN),
        decay: (d.unwrap_or(0.0) as f32).max(ENV_MIN),
        sustain: sustain.min(ENV_MAX),
        release: (r.unwrap_or(0.0) as f32).max(RELEASE_MIN),
    }
}

/// The per-sample runtime of a [`ParamMod`].
pub struct ParamModRunner {
    min: f32,
    max: f32,
    active_env: bool,
    adsr: Adsr,
    lfo: Option<Lfo>,
}

impl ParamModRunner {
    pub fn new(spec: &ParamMod, sample_rate: f64) -> ParamModRunner {
        ParamModRunner {
            min: spec.offset,
            max: spec.offset + spec.amount,
            active_env: spec.amount != 0.0,
            adsr: spec.adsr,
            lfo: spec.lfo.as_ref().map(|c| Lfo::new(c, sample_rate)),
        }
    }

    /// The parameter's value at elapsed time `t`, advancing the LFO one sample.
    pub fn tick(&mut self, t: f32, hold_end: f32) -> f32 {
        let base = if self.active_env {
            self.min + crate::envelope::adsr_value(&self.adsr, t, hold_end) * (self.max - self.min)
        } else {
            self.min
        };
        match &mut self.lfo {
            Some(lfo) => base + lfo.tick() as f32,
            None => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_modes_stay_in_range_and_are_identity_at_zero() {
        // Every mode maps 0..1 into 0..1 (the frame sampler assumes it), and
        // the modes that are pure identity stay so.
        for i in 0..=21u8 {
            let mode = WarpMode::from_index(i);
            for step in 0..64 {
                let phase = step as f32 / 64.0;
                for amt in [0.0, 0.25, 0.5, 0.9, 1.0] {
                    let out = warp_phase(phase, amt, mode);
                    assert!(
                        (0.0..=1.0).contains(&out),
                        "{mode:?} phase {phase} amt {amt} -> {out}"
                    );
                }
            }
            // NONE and FLIP leave the phase alone (FLIP inverts the sample, not
            // the phase).
            if matches!(mode, WarpMode::None | WarpMode::Flip) {
                assert_eq!(warp_phase(0.3, 0.7, mode), 0.3);
            }
        }
        assert_eq!(WarpMode::from_name("wormhole"), Some(WarpMode::Wormhole));
        assert_eq!(WarpMode::from_name("nope"), None);
    }

    #[test]
    fn frames_slice_and_a_single_frame_plays_its_wave() {
        // 4 frames of 8 samples.
        let samples: Vec<f32> = (0..32).map(|i| i as f32).collect();
        let table = WaveTable::from_samples(&samples, 8);
        assert_eq!(table.frames.len(), 4);
        assert_eq!(table.frames[1][0], 8.0);
        // A buffer shorter than one frame still yields one (padded) frame.
        let short = WaveTable::from_samples(&[1.0, 2.0], 8);
        assert_eq!(short.frames.len(), 1);
        assert_eq!(short.frames[0].len(), 8);

        // One unison voice at 1Hz over a 4-sample-rate reads the frame in
        // order. A single voice forces panspread to 0, so both channels carry
        // the equal-power sqrt(0.5) gain — as the worklet does.
        let table = WaveTable::from_samples(&[0.0, 1.0, 2.0, 3.0], 4);
        let mut osc = WavetableOsc::new(table, 1, 0.0, 1.0, 0.0, 4.0, || 0.0);
        let g = 0.5f32.sqrt();
        let got: Vec<f32> = (0..4)
            .map(|_| osc.tick(1.0, 0.0, 0.0, WarpMode::None).0)
            .collect();
        assert_eq!(got, vec![0.0 * g, 1.0 * g, 2.0 * g, 3.0 * g]);
    }
}
