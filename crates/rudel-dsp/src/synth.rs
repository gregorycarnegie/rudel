use crate::{
    envelope::adsr_value,
    filter::{FilterSet, VoiceFilters},
    fm::FM_OPS,
    modulator::{ModBank, ModSpec, ModTarget},
    oscillator::{NoiseGen, NoiseKind, Waveform, sample_table, wrap01},
    params::VoiceParams,
    pitch::PitchMod,
    voice::VoiceLike,
    wavetable::{ParamModRunner, WavetableOsc},
};
use std::f32::consts::FRAC_PI_2;
use wide::f32x8;

/// SIMD lane count used to render the super-saw unison voices in parallel.
const SUPER_LANES: usize = 8;

/// A uniform random phase in [0, 1) for super-saw voices, matching the
/// worklet's `Math.random()` initial phases. A tiny counter-hash avoids an rng
/// dependency; quality only needs to be "voices start decorrelated".
pub(crate) fn rand_phase() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0x9E37_79B9);
    phase_hash(SEED.fetch_add(0x6D2B_79F5, Ordering::Relaxed))
}

/// The hash behind [`rand_phase`], split out so it can be pinned by value:
/// `rand_phase` reads a process-wide counter, so what it returns depends on
/// how many voices ran before it.
pub(crate) fn phase_hash(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x21F0_AAAD);
    x ^= x >> 15;
    x = x.wrapping_mul(0x735A_2D97);
    x ^= x >> 15;
    (x >> 8) as f32 / (1 << 24) as f32
}

/// superdough's dry/wet crossfade gain: full across one half of the range, then
/// a linear fade across the other. `wetfade(d<0.5)=1`, then ramps down to 0.
pub(crate) fn wetfade(d: f32) -> f32 {
    if d < 0.5 { 1.0 } else { 1.0 - (d - 0.5) / 0.5 }
}

pub struct Voice {
    params: VoiceParams,
    sample_rate: f32,
    /// `pub(crate)` for `tests::synth`, which reads it to check the oscillator
    /// advances at `carrier / sample_rate`.
    pub(crate) phase: f32,
    left_gain: f32,
    right_gain: f32,
    hold_end: f32,
    /// Filter chain (low/high/band-pass), applied in order to the oscillator.
    filters: VoiceFilters,
    noise: NoiseGen,
    /// Per-operator FM phases (index `1..=FM_OPS`).
    fm_phases: [f32; FM_OPS + 1],
    /// Per-voice phases for the super-saw source.
    /// `pub(crate)` so `tests::supersaw` can plant the oracle's initial phases;
    /// upstream seeds them from `Math.random()`, so a parity golden has to pin
    /// them rather than take whatever `rand_phase` drew.
    pub(crate) super_phases: Vec<f32>,
    /// Also `pub(crate)` for that test: the source `next_*` methods read `t` but
    /// only `tick` advances it, so a golden that drives a source directly has to
    /// set the sample time itself to exercise anything time-varying.
    pub(crate) t: f32,
    /// Per-voice frequency multipliers for the super-saw source: the constant
    /// `2^(detune/12)` for each unison voice, hoisted out of the per-sample loop
    /// so the render loop only multiplies by the (possibly pitch-modulated) base
    /// increment each sample instead of recomputing a `powf` per voice.
    super_incr_ratio: Vec<f32>,
    /// Per-voice left/right gains for the super-saw stereo spread (superdough
    /// alternates an L-weighted and R-weighted equal-power pair per voice).
    super_gain_l: Vec<f32>,
    super_gain_r: Vec<f32>,
    /// Pitch envelope as `(adsr, min_semitones, max_semitones)`.
    pitch: PitchMod,
    /// Wavetable source with its `wt` position and `warp` amount modulators.
    wavetable: Option<(WavetableOsc, ParamModRunner, ParamModRunner)>,
    /// Modulators targeting this voice (frequency, gain, the filters). Empty
    /// for the common case, and then every offset reads as zero.
    mods: ModBank,
    done: bool,
}

impl Voice {
    pub fn new(params: VoiceParams, sample_rate: f32) -> Voice {
        Voice::with_mods(params, sample_rate, &[])
    }

    /// Build a voice with modulators bound to its parameters.
    pub fn with_mods(params: VoiceParams, sample_rate: f32, mods: &[ModSpec]) -> Voice {
        let pan = params.pan.clamp(0.0, 1.0);
        // equal-power panning
        let left_gain = (pan * FRAC_PI_2).cos();
        let right_gain = (pan * FRAC_PI_2).sin();
        let hold_end = (params.duration + params.hold).max(params.adsr.attack);
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
            // superdough's `getDetuner` bails out to a flat 0 for a single
            // voice, so the centering offset has to go with the scale — keeping
            // `center` alive on its own detunes a one-voice super-saw by half
            // the spread (9 cents flat at the default 0.18).
            let (scale, center) = if voices > 1 {
                (
                    params.freqspread / (voices as f32 - 1.0),
                    params.freqspread * 0.5,
                )
            } else {
                (0.0, 0.0)
            };
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
        // The super-saw and wavetable sources are stereo, and a filter is
        // stateful and mono, so the right channel needs its own bank.
        let stereo = params.supersaw || params.wavetable.is_some();
        let filters = VoiceFilters::new(
            &FilterSet {
                lp: params.lp,
                hp: params.hp,
                bp: params.bp,
            },
            sample_rate,
            stereo,
        );
        // Vibrato + pitch envelope (superdough's getVibratoOscillator +
        // getPitchEnvelope), shared with the sampler.
        let pitch = PitchMod::new(
            params.vib,
            params.vibmod,
            params.penv,
            params.pattack,
            params.pdecay,
            params.psustain,
            params.prelease,
            params.panchor,
            params.pcurve_exp,
        );
        // Wavetable source: the unison stack mirrors the super-saw's, and the
        // `wt`/`warp` params get their own envelope+LFO runners.
        let wavetable = params.wavetable.clone().map(|table| {
            let voices = params.unison.max(1);
            // `wtphaserand ?? (unison > 1)` — a unison stack decorrelates by
            // default, a single voice starts at phase 0.
            let phaserand = params
                .wtphaserand
                .unwrap_or(if voices > 1 { 1.0 } else { 0.0 });
            (
                WavetableOsc::new(
                    table,
                    voices,
                    params.freqspread,
                    params.panspread,
                    phaserand,
                    sample_rate,
                    rand_phase,
                ),
                ParamModRunner::new(&params.wt, sample_rate as f64),
                ParamModRunner::new(&params.warp, sample_rate as f64),
            )
        });
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
            pitch,
            wavetable,
            mods: ModBank::new(mods, sample_rate as f64),
            done: false,
        }
    }

    pub(crate) fn envelope(&self) -> f32 {
        adsr_value(&self.params.adsr, self.t, self.hold_end)
    }

    /// Pitch multiplier from vibrato + pitch envelope (applied to the carrier).
    fn pitch_mult(&self) -> f32 {
        self.pitch.multiplier(self.t, self.hold_end)
    }

    /// Advance the FM operators one sample and return the carrier's frequency
    /// deviation. Each operator `k` outputs `wave_k(phase) * env_k`, scaled into
    /// its targets by `amt[k][j] * freq_k` (classic FM: index × modulator freq =
    /// peak deviation). Operators are sampled before any phase advances, so
    /// cross-modulation uses a one-sample delay.
    pub(crate) fn fm_deviation(&mut self, carrier: f32) -> f32 {
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
            self.fm_phases[j] = wrap01(self.fm_phases[j] + inst / sr);
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
    pub(crate) fn next_supersaw(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        // Main detune arrives via the pitch envelope / vibrato (`pitch_mult`),
        // like the worklet's `detune` AudioParam; the per-voice spread ratios
        // are precomputed in `super_incr_ratio`.
        let base = self.params.freq * self.pitch_mult() + self.mods.get(ModTarget::Frequency);
        let base_over_sr = f32x8::splat(base / sr);
        let zero = f32x8::splat(0.0);
        let one = f32x8::splat(1.0);
        let two = f32x8::splat(2.0);
        let mut acc_l = zero;
        let mut acc_r = zero;
        for (((pchunk, rchunk), glc), grc) in self
            .super_phases
            .as_chunks_mut::<SUPER_LANES>()
            .0
            .iter_mut()
            .zip(self.super_incr_ratio.as_chunks::<SUPER_LANES>().0)
            .zip(self.super_gain_l.as_chunks::<SUPER_LANES>().0)
            .zip(self.super_gain_r.as_chunks::<SUPER_LANES>().0)
        {
            let p = f32x8::from(*pchunk);
            let r = f32x8::from(*rchunk);
            let gl = f32x8::from(*glc);
            let gr = f32x8::from(*grc);
            let dt = base_over_sr * r;
            // polyBLEP: smooth the saw's wrap discontinuity inside the dt-wide
            // windows at both cycle edges (the worklet's `sawblep`). Padded
            // lanes have dt = 0, so `inv` is infinite there and both arms below
            // compute garbage — neither mask fires on those lanes, and `select`
            // discards the unselected arm wholesale, so their naive-saw value
            // stays 0.
            let dtw = dt.min(one - dt);
            let inv = one / dtw;
            let t0 = p * inv;
            let start = two * t0 - t0 * t0 - one;
            let t1 = (p - one) * inv;
            let end = t1 * t1 + two * t1 + one;
            let blep = p.simd_lt(dtw).select(start, zero) + p.simd_gt(one - dtw).select(end, zero);
            let v = two * p - one - blep;
            acc_l += v * gl;
            acc_r += v * gr;
            // Advance each lane's phase, wrapping to [0, 1) with
            // `phase − floor(phase)` (the increment is non-negative, so this
            // matches `rem_euclid(1.0)`).
            let np = p + dt;
            *pchunk = (np - np.floor()).to_array();
        }
        let norm = 1.0 / (self.params.unison.max(1) as f32).sqrt();
        (acc_l.reduce_add() * norm, acc_r.reduce_add() * norm)
    }

    /// Render one stereo wavetable sample and advance its phases. The `wt`
    /// position and `warp` amount are swept per sample by their own envelope +
    /// LFO, as superdough drives the worklet's AudioParams.
    fn next_wavetable(&mut self) -> (f32, f32) {
        let carrier = self.params.freq * self.pitch_mult() + self.mods.get(ModTarget::Frequency);
        // `onTriggerSynth` runs `applyFM` on the worklet's frequency param, so
        // the wavetable is FM-able like the plain oscillator.
        let freq = if self.params.fm.active() {
            carrier + self.fm_deviation(carrier)
        } else {
            carrier
        };
        let (t, hold_end) = (self.t, self.hold_end);
        let warpmode = self.params.warpmode;
        let Some((osc, wt, warp)) = &mut self.wavetable else {
            return (0.0, 0.0);
        };
        let position = wt.tick(t, hold_end);
        let amount = warp.tick(t, hold_end);
        osc.tick(freq, position, amount, warpmode)
    }

    /// Produce the next source sample and advance the oscillator phase(s).
    pub(crate) fn next_source(&mut self) -> f32 {
        let sr = self.sample_rate;
        let pitch = self.pitch_mult();
        if let Some(kind) = self.params.noise {
            return self.noise.next(kind);
        }
        // Oscillator, optionally frequency-modulated. A `freq`/`note` modulator
        // is an additive Hz offset on the source's frequency param.
        let carrier = self.params.freq * pitch + self.mods.get(ModTarget::Frequency);
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
        self.phase = wrap01(self.phase + inc);
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
        self.mods.tick();
        let env = self.envelope();
        let gain = self.params.gain + self.mods.get(ModTarget::Gain);
        let (t, hold_end, sr) = (self.t, self.hold_end, self.sample_rate);
        // 0.3 matches Strudel's synth turn-down (gainNode(0.3)).
        let out = if self.params.supersaw || self.wavetable.is_some() {
            let (mut l, mut r) = if self.wavetable.is_some() {
                self.next_wavetable()
            } else {
                self.next_supersaw()
            };
            (l, r) = self
                .filters
                .process_stereo(l, r, t, hold_end, sr, &self.mods);
            let s = env * gain * 0.3;
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
            let raw = self.next_source();
            let osc = self.filters.process(raw, t, hold_end, sr, &self.mods);
            let s = osc * env * gain * 0.3;
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
    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        self.mods.set_bus_input(bus, left, right);
    }
    fn is_done(&self) -> bool {
        self.done
    }
}
