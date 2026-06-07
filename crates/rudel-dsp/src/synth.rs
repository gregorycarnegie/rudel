use crate::envelope::{Adsr, adsr_value};
use crate::filter::{FilterKind, VoiceFilter};
use crate::fm::FM_OPS;
use crate::oscillator::{NoiseGen, NoiseKind, Waveform, sample_table};
use crate::params::VoiceParams;
use crate::voice::VoiceLike;
use std::f32::consts::PI;

/// superdough's dry/wet crossfade gain: full across one half of the range, then
/// a linear fade across the other. `wetfade(d<0.5)=1`, then ramps down to 0.
fn wetfade(d: f32) -> f32 {
    if d < 0.5 { 1.0 } else { 1.0 - (d - 0.5) / 0.5 }
}

/// Exponential (geometric) interpolation between `a` and `b` over progress `p`,
/// matching Web Audio's `exponentialRampToValueAtTime`. Zeros are nudged off the
/// axis; if the endpoints straddle zero (undefined for an exp ramp) it falls
/// back to linear.
fn geo(a: f32, b: f32, p: f32) -> f32 {
    let nz = |x: f32| if x == 0.0 { 0.001 } else { x };
    let (a, b) = (nz(a), nz(b));
    if a.signum() != b.signum() {
        a + (b - a) * p
    } else {
        a * (b / a).powf(p)
    }
}

/// The pitch-envelope value (in semitones) at time `t`. Linear by default;
/// `exp` switches to exponential ramp segments (`pcurve`).
fn pitch_env_value(adsr: &Adsr, t: f32, hold_end: f32, min: f32, max: f32, exp: bool) -> f32 {
    if !exp {
        return min + adsr_value(adsr, t, hold_end) * (max - min);
    }
    let Adsr {
        attack,
        decay,
        sustain,
        release,
    } = *adsr;
    let sustain_val = min + sustain * (max - min);
    if t < attack {
        geo(min, max, t / attack.max(1e-9))
    } else if t < attack + decay {
        geo(max, sustain_val, (t - attack) / decay.max(1e-9))
    } else if t < hold_end {
        sustain_val
    } else if t < hold_end + release {
        geo(sustain_val, min, (t - hold_end) / release.max(1e-9))
    } else {
        min
    }
}

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
    /// Per-operator FM phases (index `1..=FM_OPS`).
    fm_phases: [f32; FM_OPS + 1],
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
            fm_phases: [0.0; FM_OPS + 1],
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
            semis += pitch_env_value(
                &adsr,
                self.t,
                self.hold_end,
                min,
                max,
                self.params.pcurve_exp,
            );
        }
        if semis == 0.0 {
            1.0
        } else {
            2f32.powf(semis / 12.0)
        }
    }

    /// Advance the FM operators one sample and return the carrier's frequency
    /// deviation. Each operator `k` outputs `wave_k(phase) * env_k`, scaled into
    /// its targets by `amt[k][j] * freq_k` (classic FM: index × modulator freq =
    /// peak deviation). Operators are sampled before any phase advances, so
    /// cross-modulation uses a one-sample delay.
    fn fm_deviation(&mut self, carrier: f32) -> f32 {
        let n = self.params.fm.max_op;
        let (t, hold_end, sr) = (self.t, self.hold_end, self.sample_rate);
        let mut op_out = [0.0f32; FM_OPS + 1];
        let mut op_freq = [0.0f32; FM_OPS + 1];
        for k in 1..=n {
            let op = self.params.fm.ops[k];
            op_freq[k] = carrier * op.ratio;
            let osc = op.wave.sample(self.fm_phases[k]);
            let env = op.env.map_or(1.0, |e| adsr_value(&e, t, hold_end));
            op_out[k] = osc * env;
        }
        // Advance each operator's phase by its (modulated) instantaneous freq.
        for j in 1..=n {
            let mut dev = 0.0;
            for k in 1..=n {
                dev += self.params.fm.amt[k][j] * op_freq[k] * op_out[k];
            }
            let inst = op_freq[j] + dev;
            self.fm_phases[j] = (self.fm_phases[j] + inst / sr).rem_euclid(1.0);
        }
        // Carrier deviation (target 0).
        (1..=n)
            .map(|k| self.params.fm.amt[k][0] * op_freq[k] * op_out[k])
            .sum()
    }

    /// Produce the next source sample and advance the oscillator phase(s).
    fn next_source(&mut self) -> f32 {
        let sr = self.sample_rate;
        let pitch = self.pitch_mult();
        if self.params.supersaw {
            let voices = self.params.unison.max(1);
            // main detune (cents -> semitones)
            let base = self.params.freq * pitch * 2f32.powf((self.params.detune / 100.0) / 12.0);
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
        let mut s = if let Some(table) = &self.params.additive {
            sample_table(table, self.phase)
        } else {
            match self.params.waveform {
                Waveform::Pulse => Waveform::pulse(self.phase, self.params.pw),
                w => w.sample(self.phase),
            }
        };
        let inc = if self.params.fm.active() {
            (carrier + self.fm_deviation(carrier)) / sr
        } else {
            carrier / sr
        };
        self.phase = (self.phase + inc).rem_euclid(1.0);
        // `noise` blends pink noise into the oscillator (superdough's drywet
        // crossfade: dry/wet each held at full across one half of the range).
        if self.params.noise_mix > 0.0 {
            let w = self.params.noise_mix;
            let pink = self.noise.next(NoiseKind::Pink);
            s = s * wetfade(w) + pink * wetfade(1.0 - w);
        }
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
