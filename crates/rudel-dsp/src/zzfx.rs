// zzfx.rs - ZzFX synth voice. Ported from strudel/packages/superdough/
// {zzfx,zzfx_fork}.mjs (itself KilledByAPixel's ZzFX). `build_samples` is the
// deterministic synthesis core (a pure function of its 20 params + sample rate,
// modulo the `randomness`/`zrand` term); `ZzfxParams::from_controls` mirrors
// `getZZFX`'s control-to-param mapping, and `ZzfxVoice` plays the generated
// buffer back with gain/pan and the reverb/delay sends.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    filter::{FilterSet, VoiceFilters},
    modulator::{ModBank, ModSpec},
    voice::VoiceLike,
};
use rudel_core::{Value, ValueMap};
use std::{
    f32::consts::FRAC_PI_2,
    f64::consts::TAU,
    sync::atomic::{AtomicU32, Ordering},
};

/// The 20 ZzFX synthesis parameters (the `buildSamples` argument list).
#[derive(Clone, Debug)]
pub struct ZzfxSynth {
    pub volume: f64,
    pub randomness: f64,
    pub frequency: f64,
    pub attack: f64,
    pub sustain: f64,
    pub release: f64,
    pub shape: i32,
    pub shape_curve: f64,
    pub slide: f64,
    pub delta_slide: f64,
    pub pitch_jump: f64,
    pub pitch_jump_time: f64,
    pub repeat_time: f64,
    pub noise: f64,
    pub modulation: f64,
    pub bit_crush: f64,
    pub delay: f64,
    pub sustain_volume: f64,
    pub decay: f64,
    pub tremolo: f64,
}

impl Default for ZzfxSynth {
    fn default() -> ZzfxSynth {
        // buildSamples defaults.
        ZzfxSynth {
            volume: 1.0,
            randomness: 0.05,
            frequency: 220.0,
            attack: 0.0,
            sustain: 0.0,
            release: 0.1,
            shape: 0,
            shape_curve: 1.0,
            slide: 0.0,
            delta_slide: 0.0,
            pitch_jump: 0.0,
            pitch_jump_time: 0.0,
            repeat_time: 0.0,
            noise: 0.0,
            modulation: 0.0,
            bit_crush: 0.0,
            delay: 0.0,
            sustain_volume: 1.0,
            decay: 0.0,
            tremolo: 0.0,
        }
    }
}

fn sign(v: f64) -> f64 {
    if v > 0.0 { 1.0 } else { -1.0 }
}

/// JS-style positive modulo for the bit-crush / repeat counters.
fn jsmod_i(a: i64, b: i64) -> i64 {
    a % b
}

/// Generate the ZzFX sample buffer (port of `zzfx_fork.mjs::buildSamples`).
/// `rand01` supplies the single `Math.random()` draw used by the `randomness`
/// term — irrelevant when `randomness == 0` (the deterministic, golden-tested
/// path).
// `min().max()` for the tan shape is kept verbatim from JS (`Math.max(Math.min
// (Math.tan(t), 1), -1)`) rather than `clamp`, which differs on NaN inputs.
#[allow(clippy::manual_clamp)]
pub fn build_samples(p: &ZzfxSynth, sample_rate: f64, rand01: f64) -> Vec<f64> {
    let pi2 = TAU;
    let sr = sample_rate;

    let mut slide = p.slide * (500.0 * pi2) / sr / sr;
    let start_slide = slide;
    let mut frequency =
        p.frequency * ((1.0 + p.randomness * 2.0 * rand01 - p.randomness) * pi2) / sr;
    let mut start_frequency = frequency;

    let attack = p.attack * sr + 9.0; // minimum attack to prevent pop
    let decay = p.decay * sr;
    let sustain = p.sustain * sr;
    let release = p.release * sr;
    let delay = p.delay * sr;
    let delta_slide = p.delta_slide * (500.0 * pi2) / sr.powi(3);
    let modulation = p.modulation * pi2 / sr;
    let pitch_jump = p.pitch_jump * pi2 / sr;
    let pitch_jump_time = p.pitch_jump_time * sr;
    let repeat_time = (p.repeat_time * sr) as i64; // `| 0`
    let bit_crush_mod = (p.bit_crush * 100.0) as i64; // `| 0`

    let length = (attack + decay + sustain + release + delay) as i64; // `| 0`
    let mut b: Vec<f64> = Vec::with_capacity(length.max(0) as usize);

    let mut t = 0.0_f64;
    let mut tm = 0.0_f64;
    let mut j: i64 = 1;
    let mut r: i64 = 0;
    let mut c: i64 = 0;
    let mut s = 0.0_f64;

    let mut i: i64 = 0;
    while i < length {
        let fi = i as f64;
        c += 1;
        // `!(++c % ((bitCrush*100)|0))`: with a zero modulus JS yields `!NaN` (true).
        if bit_crush_mod == 0 || jsmod_i(c, bit_crush_mod) == 0 {
            // wave shape
            s = match p.shape {
                0 => t.sin(),
                1 => 1.0 - 4.0 * ((t / pi2).round() - t / pi2).abs(), // triangle
                2 => 1.0 - (((2.0 * t / pi2) % 2.0 + 2.0) % 2.0),     // saw
                3 => t.tan().min(1.0).max(-1.0),                      // tan
                _ => ((t % pi2).powi(3)).sin(),                       // 4+ noise
            };

            let tremolo_gain = if repeat_time != 0 {
                1.0 - p.tremolo + p.tremolo * ((pi2 * fi) / repeat_time as f64).sin()
            } else {
                1.0
            };
            let env = if fi < attack {
                fi / attack
            } else if fi < attack + decay {
                1.0 - ((fi - attack) / decay) * (1.0 - p.sustain_volume)
            } else if fi < attack + decay + sustain {
                p.sustain_volume
            } else if fi < length as f64 - delay {
                ((length as f64 - fi - delay) / release) * p.sustain_volume
            } else {
                0.0
            };
            s = tremolo_gain * sign(s) * s.abs().powf(p.shape_curve) * p.volume * env;

            // sample delay (feedback into the buffer being built)
            if delay != 0.0 {
                let delayed = if delay > fi {
                    0.0
                } else {
                    let ramp = if fi < length as f64 - delay {
                        1.0
                    } else {
                        (length as f64 - fi) / delay
                    };
                    let idx = (fi - delay) as i64;
                    ramp * b[idx as usize] / 2.0
                };
                s = s / 2.0 + delayed;
            }
        }

        // frequency / modulation / noise (always, even on bit-crush holds)
        slide += delta_slide;
        frequency += slide;
        let f = frequency * (modulation * tm).cos();
        tm += 1.0;
        t += f - f * p.noise * (1.0 - (((fi.sin() + 1.0) * 1e9) % 2.0));

        // pitch jump (once, after pitchJumpTime)
        if j != 0 {
            j += 1;
            if j as f64 > pitch_jump_time {
                frequency += pitch_jump;
                start_frequency += pitch_jump;
                j = 0;
            }
        }

        // repeat: reset frequency/slide and re-arm the pitch jump
        if repeat_time != 0 {
            r += 1;
            if jsmod_i(r, repeat_time) == 0 {
                frequency = start_frequency;
                slide = start_slide;
                if j == 0 {
                    j = 1;
                }
            }
        }

        b.push(s);
        i += 1;
    }

    b
}

/// The ZzFX shape index for a sound name (`['sine','triangle','sawtooth','tan',
/// 'noise']`), or `-1` for anything else (`square`/`zzfx`), matching `getZZFX`.
fn shape_index(name: &str) -> i32 {
    ["sine", "triangle", "sawtooth", "tan", "noise"]
        .iter()
        .position(|&w| w == name)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// A ZzFX voice: the synthesis params plus the playback controls.
#[derive(Clone, Debug)]
pub struct ZzfxParams {
    pub synth: ZzfxSynth,
    pub gain: f32,
    pub pan: f32,
    /// The hap's length in seconds, which the filter envelopes hold for.
    pub duration: f32,
    pub filters: FilterSet,
}

impl ZzfxParams {
    /// Map a control map to ZzFX params (port of `getZZFX`). `duration` is the
    /// hap's length in seconds.
    pub fn from_controls(name: &str, map: &ValueMap, duration: f32) -> ZzfxParams {
        let num = |k: &str, d: f64| map.get(k).and_then(|v| v.as_f64()).unwrap_or(d);

        let attack = num("attack", 0.0);
        let decay = num("decay", 0.0);
        let sustain_vol = num("sustain", 0.8);
        let release = num("release", 0.1);
        let mut curve = num("curve", 1.0);
        let dur = num("duration", duration.max(0.0) as f64);
        let sustain_time = (dur - attack - decay).max(0.0);

        // frequency: explicit `freq`, else from `note` (default 36).
        let freq = map.get("freq").and_then(|v| v.as_f64()).unwrap_or_else(|| {
            let note = match map.get("note") {
                Some(Value::Str(s)) => crate::pitch::note_name_to_midi(s)
                    .map(|m| m as f64)
                    .unwrap_or(36.0),
                Some(v) => v.as_f64().unwrap_or(36.0),
                None => 36.0,
            };
            crate::pitch::mtof(note) as f64
        });

        // sound name -> shape; `square` forces curve 0 (square via |s|^0).
        let bare = name.strip_prefix("z_").unwrap_or(name);
        let shape = shape_index(bare);
        if bare == "square" {
            curve = 0.0;
        }

        // An explicit `zzfx` control array overrides the derived params.
        let synth = match map.get("zzfx") {
            Some(Value::List(items)) if items.len() >= 20 => {
                let g = |i: usize| items[i].as_f64().unwrap_or(0.0);
                ZzfxSynth {
                    volume: g(0),
                    randomness: g(1),
                    frequency: g(2),
                    attack: g(3),
                    sustain: g(4),
                    release: g(5),
                    shape: g(6) as i32,
                    shape_curve: g(7),
                    slide: g(8),
                    delta_slide: g(9),
                    pitch_jump: g(10),
                    pitch_jump_time: g(11),
                    repeat_time: g(12),
                    noise: g(13),
                    modulation: g(14),
                    bit_crush: g(15),
                    delay: g(16),
                    sustain_volume: g(17),
                    decay: g(18),
                    tremolo: g(19),
                }
            }
            _ => ZzfxSynth {
                volume: 0.25,
                randomness: num("zrand", 0.0),
                frequency: freq,
                attack,
                sustain: sustain_time,
                release,
                shape,
                shape_curve: curve,
                slide: num("slide", 0.0),
                delta_slide: num("deltaSlide", 0.0),
                pitch_jump: num("pitchJump", 0.0),
                pitch_jump_time: num("pitchJumpTime", 0.0),
                repeat_time: num("lfo", 0.0),
                noise: num("znoise", 0.0),
                modulation: num("zmod", 0.0),
                bit_crush: num("zcrush", 0.0),
                delay: num("zdelay", 0.0),
                sustain_volume: sustain_vol,
                decay,
                tremolo: num("tremolo", 0.0),
            },
        };

        ZzfxParams {
            synth,
            gain: num("gain", 1.0) as f32,
            pan: num("pan", 0.5) as f32,
            duration: dur as f32,
            filters: FilterSet::from_controls(map),
        }
    }
}

static ZZFX_RNG: AtomicU32 = AtomicU32::new(0x2545_f491);

/// A per-voice pseudo-random draw in `[0, 1)` for the `randomness` term. ZzFX
/// uses an unseeded `Math.random()`, so this is deliberately non-reproducible
/// across voices but does not affect the `randomness == 0` default.
fn next_rand01() -> f64 {
    // xorshift32 over a shared counter. Advanced with one atomic
    // read-modify-write rather than a load and a store: two voices starting in
    // the same audio callback would otherwise read the same state and draw the
    // same "random" detune.
    let step = |x: u32| {
        let mut x = x.wrapping_add(0x9e37_79b9);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let previous = ZZFX_RNG
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some(step(x)))
        .unwrap_or(0);
    (step(previous) as f64) / (u32::MAX as f64 + 1.0)
}

/// A ZzFX voice playing its pre-rendered buffer.
pub struct ZzfxVoice {
    buffer: Vec<f64>,
    pos: usize,
    filters: VoiceFilters,
    mods: ModBank,
    sample_rate: f32,
    hold_end: f32,
    gain: f32,
    left_gain: f32,
    right_gain: f32,
}

impl ZzfxVoice {
    pub fn new(params: ZzfxParams, sample_rate: f32) -> ZzfxVoice {
        ZzfxVoice::with_mods(params, sample_rate, &[])
    }

    pub fn with_mods(params: ZzfxParams, sample_rate: f32, mods: &[ModSpec]) -> ZzfxVoice {
        let buffer = build_samples(&params.synth, sample_rate as f64, next_rand01());
        let pan = params.pan.clamp(0.0, 1.0);
        ZzfxVoice {
            buffer,
            pos: 0,
            filters: VoiceFilters::new(&params.filters, sample_rate, false),
            mods: ModBank::new(mods, sample_rate as f64),
            sample_rate,
            hold_end: params.duration,
            gain: params.gain,
            left_gain: (pan * FRAC_PI_2).cos(),
            right_gain: (pan * FRAC_PI_2).sin(),
        }
    }
}

impl VoiceLike for ZzfxVoice {
    fn tick(&mut self) -> (f32, f32) {
        if self.pos >= self.buffer.len() {
            return (0.0, 0.0);
        }
        let raw = self.buffer[self.pos] as f32;
        let t = self.pos as f32 / self.sample_rate;
        self.mods.tick();
        let s = self
            .filters
            .process(raw, t, self.hold_end, self.sample_rate, &self.mods)
            * self.gain;
        self.pos += 1;
        (s * self.left_gain, s * self.right_gain)
    }
    fn is_done(&self) -> bool {
        self.pos >= self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the parts the golden cannot reach ---------------------------------

    #[test]
    fn the_random_source_is_uniform_over_the_unit_interval() {
        // Every golden case runs at `randomness: 0` so the draw is a no-op
        // there, which left all 16 of `next_rand01`'s mutants alive. It stands
        // in for `Math.random()`, so there is no oracle for it either — what
        // can be pinned is that it behaves like one.
        //
        // Note the limit: the xorshift triple (13, 17, 5) is a *quality*
        // parameter. Another triple is still a working generator, so no
        // behavioural test distinguishes them; what these catch is the
        // structural damage — a shift that collapses the state, an `^` turned
        // into `|` or `&`, or a scale that leaves the unit interval.
        const N: usize = 20_000;
        let draws: Vec<f64> = (0..N).map(|_| next_rand01()).collect();

        for (i, &v) in draws.iter().enumerate() {
            assert!(
                (0.0..1.0).contains(&v),
                "draw {i} left the unit interval: {v}"
            );
        }

        // A uniform source fills every bucket to roughly its share. A generator
        // that has collapsed to a constant, or to a handful of values, leaves
        // buckets empty.
        const BUCKETS: usize = 16;
        let mut counts = [0usize; BUCKETS];
        for &v in &draws {
            counts[(v * BUCKETS as f64) as usize] += 1;
        }
        let expected = N / BUCKETS;
        for (i, &c) in counts.iter().enumerate() {
            assert!(
                c > expected / 2 && c < expected * 2,
                "bucket {i} holds {c} of {N}, expected about {expected}: {counts:?}"
            );
        }

        // Mean and variance of a uniform draw on 0..1.
        let mean = draws.iter().sum::<f64>() / N as f64;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean} should be near 0.5");
        let var = draws.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / N as f64;
        assert!(
            (var - 1.0 / 12.0).abs() < 0.01,
            "variance {var} should be near 1/12"
        );

        // Consecutive draws are not the same value, so the state really moves.
        let repeats = draws.windows(2).filter(|w| w[0] == w[1]).count();
        assert_eq!(repeats, 0, "the generator repeated a value immediately");
    }

    #[test]
    fn the_random_draw_detunes_the_start_frequency() {
        // The reason the draw exists. `build_samples` takes it as an argument,
        // so its effect is deterministic and can be pinned exactly: the start
        // frequency is scaled by `1 + randomness * 2 * rand - randomness`, so
        // a draw of 0.5 is the midpoint and lands back on no detune at all.
        let synth = |randomness: f64| ZzfxSynth {
            volume: 0.25,
            randomness,
            frequency: 440.0,
            attack: 0.001,
            sustain: 0.02,
            release: 0.003,
            ..ZzfxSynth::default()
        };
        let render =
            |randomness: f64, rand01: f64| build_samples(&synth(randomness), 44100.0, rand01);

        // A draw at the midpoint cancels out, so it matches no randomness.
        let none = render(0.0, 0.5);
        assert_eq!(
            render(0.5, 0.5),
            none,
            "a mid draw should leave the frequency alone"
        );
        // Draws either side of it detune in opposite directions.
        let low = render(0.5, 0.0);
        let high = render(0.5, 1.0);
        assert!(
            low != none && high != none && low != high,
            "the draw should move the start frequency"
        );
        // With no randomness the draw is ignored entirely, which is what makes
        // the golden comparison possible.
        assert_eq!(render(0.0, 0.0), render(0.0, 1.0));
    }

    #[test]
    fn sign_treats_zero_as_negative_the_way_the_fork_does() {
        // `sign` here is the fork's two-way test, not a three-way signum: zero
        // goes to -1. The waveshaper leans on that at the start of a cycle.
        assert_eq!(sign(1.5), 1.0);
        assert_eq!(sign(1e-12), 1.0);
        assert_eq!(sign(0.0), -1.0);
        assert_eq!(sign(-0.0), -1.0);
        assert_eq!(sign(-1e-12), -1.0);
        assert_eq!(sign(-3.0), -1.0);
    }

    #[test]
    fn the_shape_names_index_the_fork_s_waveform_table() {
        for (name, want) in [
            ("sine", 0),
            ("triangle", 1),
            ("sawtooth", 2),
            ("tan", 3),
            ("noise", 4),
        ] {
            assert_eq!(shape_index(name), want, "shape {name}");
        }
        // Anything else is -1 rather than silently becoming a sine.
        for name in ["", "saw", "square", "Sine", "noisey"] {
            assert_eq!(shape_index(name), -1, "shape {name}");
        }
    }

    fn map(pairs: &[(&str, Value)]) -> ValueMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn control_mapping_mirrors_getzzfx() {
        // z_sawtooth -> shape 2; note 36 default; duration drives sustainTime.
        let p = ZzfxParams::from_controls("z_sawtooth", &map(&[]), 0.2);
        assert_eq!(p.synth.shape, 2);
        assert_eq!(p.synth.volume, 0.25);
        // note default 36 -> its frequency.
        assert!((p.synth.frequency - crate::pitch::mtof(36.0) as f64).abs() < 1e-6);
        // sustainTime = duration - attack - decay = 0.2 (within f32->f64 slop).
        assert!((p.synth.sustain - 0.2).abs() < 1e-6);

        // square forces curve (shapeCurve) to 0 so |s|^0 = 1 gives a square wave.
        let sq = ZzfxParams::from_controls("z_square", &map(&[]), 0.2);
        assert_eq!(sq.synth.shape_curve, 0.0);

        // the z-prefixed controls map onto the right synth params.
        let q = ZzfxParams::from_controls(
            "zzfx",
            &map(&[
                ("freq", Value::F64(440.0)),
                ("zcrush", Value::F64(0.5)),
                ("zmod", Value::F64(20.0)),
                ("lfo", Value::F64(0.01)),
            ]),
            0.2,
        );
        assert_eq!(q.synth.frequency, 440.0);
        assert_eq!(q.synth.bit_crush, 0.5);
        assert_eq!(q.synth.modulation, 20.0);
        assert_eq!(q.synth.repeat_time, 0.01);
    }

    #[test]
    fn voice_plays_then_finishes() {
        // attack 0 + sustain 0.01s + release 0.1s ~= 4860 samples at 44.1kHz.
        let p = ZzfxParams::from_controls("z_sine", &map(&[("freq", Value::F64(220.0))]), 0.01);
        let mut v = ZzfxVoice::new(p, 44100.0);
        let mut peak = 0.0_f32;
        for _ in 0..6000 {
            let (l, _r) = v.tick();
            peak = peak.max(l.abs());
        }
        assert!(peak > 0.0, "voice should produce sound");
        assert!(v.is_done(), "short voice should finish within the window");
    }
}
