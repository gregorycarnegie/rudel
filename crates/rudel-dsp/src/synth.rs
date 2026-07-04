use crate::{
    envelope::{Adsr, adsr_value},
    filter::{FilterKind, VoiceFilter},
    fm::FM_OPS,
    oscillator::{NoiseGen, NoiseKind, Waveform, sample_table},
    params::VoiceParams,
    voice::VoiceLike,
};
use std::f32::consts::{FRAC_PI_2, TAU};
use wide::f32x8;

/// SIMD lane count used to render the super-saw unison voices in parallel.
const SUPER_LANES: usize = 8;

/// A uniform random phase in [0, 1) for super-saw voices, matching the
/// worklet's `Math.random()` initial phases. A tiny counter-hash avoids an rng
/// dependency; quality only needs to be "voices start decorrelated".
fn rand_phase() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0x9E37_79B9);
    let mut x = SEED.fetch_add(0x6D2B_79F5, Ordering::Relaxed);
    x ^= x >> 16;
    x = x.wrapping_mul(0x21F0_AAAD);
    x ^= x >> 15;
    x = x.wrapping_mul(0x735A_2D97);
    x ^= x >> 15;
    (x >> 8) as f32 / (1 << 24) as f32
}

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
    /// Per-voice frequency multipliers for the super-saw source: the constant
    /// `2^(detune/12)` for each unison voice, hoisted out of the per-sample loop
    /// so the render loop only multiplies by the (possibly pitch-modulated) base
    /// increment each sample instead of recomputing a `powf` per voice.
    super_incr_ratio: Vec<f32>,
    /// Per-voice left/right gains for the super-saw stereo spread (superdough
    /// alternates an L-weighted and R-weighted equal-power pair per voice).
    super_gain_l: Vec<f32>,
    super_gain_r: Vec<f32>,
    /// Second filter bank for the super-saw's right channel (the filters are
    /// stateful and mono, so the stereo pair needs independent state).
    filters_r: Vec<VoiceFilter>,
    /// Pitch envelope as `(adsr, min_semitones, max_semitones)`.
    pitch_env: Option<(Adsr, f32, f32)>,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * FRAC_PI_2).cos();
        let right_gain = (pan * FRAC_PI_2).sin();
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
        // Super-saw voices: random initial phases (superdough's worklet uses
        // `Math.random()` per voice), each voice's constant detune ratio
        // `2^(d/12)`, and alternating L/R equal-power gains for the stereo
        // spread. All arrays are padded up to a multiple of the SIMD lane count
        // so the render loop can sum them eight at a time with no scalar
        // remainder. Padding lanes hold phase 0.5 (saw value `2·0.5 − 1 = 0`),
        // ratio 0 (never advance, and the polyBLEP masks never fire) and gain 0,
        // so they contribute nothing to the mix.
        let (super_phases, super_incr_ratio, super_gain_l, super_gain_r) = if params.supersaw {
            let voices = params.unison.max(1);
            let padded = voices.next_multiple_of(SUPER_LANES);
            let scale = if voices > 1 {
                params.freqspread / (voices as f32 - 1.0)
            } else {
                0.0
            };
            let center = params.freqspread * 0.5;
            // superdough: panspread is forced to 0 for a single voice, then
            // remapped to [0.5, 1] before the sqrt gain pair.
            let panspread = if voices > 1 { params.panspread } else { 0.0 } * 0.5 + 0.5;
            let (gain_l, gain_r) = ((1.0 - panspread).sqrt(), panspread.sqrt());
            let mut phases = vec![0.5f32; padded];
            let mut ratios = vec![0.0f32; padded];
            let mut gains_l = vec![0.0f32; padded];
            let mut gains_r = vec![0.0f32; padded];
            for n in 0..voices {
                phases[n] = rand_phase();
                let d = n as f32 * scale - center; // semitone detune for this voice
                ratios[n] = 2f32.powf(d / 12.0);
                // invert the left and right gain each voice, like the worklet
                let (l, r) = if n % 2 == 0 {
                    (gain_l, gain_r)
                } else {
                    (gain_r, gain_l)
                };
                gains_l[n] = l;
                gains_r[n] = r;
            }
            (phases, ratios, gains_l, gains_r)
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };
        let filters_r = if params.supersaw {
            filters.clone()
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
            super_incr_ratio,
            super_gain_l,
            super_gain_r,
            filters_r,
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
            semis += self.params.vibmod * (TAU * rate * self.t).sin();
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

    /// Render one stereo super-saw sample and advance the voice phases.
    /// Mirrors superdough's supersaw worklet: per-voice polyBLEP saws
    /// (`sawblep`), alternating L/R equal-power gains, summed and normalized
    /// by `1/sqrt(voices)`.
    fn next_supersaw(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        // Main detune arrives via the pitch envelope / vibrato (`pitch_mult`),
        // like the worklet's `detune` AudioParam; the per-voice spread ratios
        // are precomputed in `super_incr_ratio`.
        let base = self.params.freq * self.pitch_mult();
        let base_over_sr = f32x8::splat(base / sr);
        let zero = f32x8::splat(0.0);
        let one = f32x8::splat(1.0);
        let two = f32x8::splat(2.0);
        let mut acc_l = zero;
        let mut acc_r = zero;
        for (((pchunk, rchunk), glc), grc) in self
            .super_phases
            .chunks_exact_mut(SUPER_LANES)
            .zip(self.super_incr_ratio.chunks_exact(SUPER_LANES))
            .zip(self.super_gain_l.chunks_exact(SUPER_LANES))
            .zip(self.super_gain_r.chunks_exact(SUPER_LANES))
        {
            let p = f32x8::from(<[f32; SUPER_LANES]>::try_from(&*pchunk).unwrap());
            let r = f32x8::from(<[f32; SUPER_LANES]>::try_from(rchunk).unwrap());
            let gl = f32x8::from(<[f32; SUPER_LANES]>::try_from(glc).unwrap());
            let gr = f32x8::from(<[f32; SUPER_LANES]>::try_from(grc).unwrap());
            let dt = base_over_sr * r;
            // polyBLEP: smooth the saw's wrap discontinuity inside the dt-wide
            // windows at both cycle edges (the worklet's `sawblep`). Padded
            // lanes have dt = 0, so neither mask fires there (the garbage the
            // unselected arms compute is discarded by the bitwise blend) and
            // their naive-saw value stays 0.
            let dtw = dt.min(one - dt);
            let inv = one / dtw;
            let t0 = p * inv;
            let start = two * t0 - t0 * t0 - one;
            let t1 = (p - one) * inv;
            let end = t1 * t1 + two * t1 + one;
            let blep = p.simd_lt(dtw).blend(start, zero) + p.simd_gt(one - dtw).blend(end, zero);
            let v = two * p - one - blep;
            acc_l += v * gl;
            acc_r += v * gr;
            // Advance each lane's phase, wrapping to [0, 1) with
            // `phase − floor(phase)` (the increment is non-negative, so this
            // matches `rem_euclid(1.0)`).
            let np = p + dt;
            pchunk.copy_from_slice(&(np - np.floor()).to_array());
        }
        let norm = 1.0 / (self.params.unison.max(1) as f32).sqrt();
        (acc_l.reduce_add() * norm, acc_r.reduce_add() * norm)
    }

    /// Produce the next source sample and advance the oscillator phase(s).
    fn next_source(&mut self) -> f32 {
        let sr = self.sample_rate;
        let pitch = self.pitch_mult();
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
        let (t, hold_end, sr) = (self.t, self.hold_end, self.sample_rate);
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let out = if self.params.supersaw {
            let (mut l, mut r) = self.next_supersaw();
            for f in &mut self.filters {
                l = f.process(l, t, hold_end, sr);
            }
            for f in &mut self.filters_r {
                r = f.process(r, t, hold_end, sr);
            }
            let s = env * self.params.gain * 0.3;
            // The pair is already stereo-spread; apply the voice pan as a
            // balance (identity at center, like a StereoPannerNode driven with
            // a stereo input) instead of the mono equal-power gains.
            let p = 2.0 * self.params.pan.clamp(0.0, 1.0) - 1.0;
            let (bl, br) = if p >= 0.0 {
                (1.0 - p, 1.0)
            } else {
                (1.0, 1.0 + p)
            };
            (l * s * bl, r * s * br)
        } else {
            let mut osc = self.next_source();
            for f in &mut self.filters {
                osc = f.process(osc, t, hold_end, sr);
            }
            let s = osc * env * self.params.gain * 0.3;
            (s * self.left_gain, s * self.right_gain)
        };

        self.t += 1.0 / self.sample_rate;
        if self.t >= self.hold_end + self.params.adsr.release {
            self.done = true;
        }
        out
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
    fn dry(&self) -> f32 {
        self.params.dry
    }
}
