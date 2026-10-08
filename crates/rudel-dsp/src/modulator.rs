// modulator.rs - LFO modulation source. Ported from the `lfo-processor`
// AudioWorklet in strudel/packages/superdough/worklets.mjs (waveshapes + the
// per-sample process loop) and the `getLfo` defaults in helpers.mjs. This is the
// deterministic modulation-source core of superdough's modulator engine
// (modulate/lfo/env/bmod), plus the control-target routing that
// `connectLFO`/`connectEnvelope`/`connectBusModulator` express as Web Audio
// connections into an AudioParam.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Value, ValueMap};
use std::f64::consts::TAU;

/// Smooth a saw discontinuity (PolyBLEP), used by the `sawblep` shape.
fn poly_blep(phase: f64, dt: f64) -> f64 {
    let invdt = 1.0 / dt;
    if phase < dt {
        let p = phase * invdt;
        2.0 * p - p * p - 1.0
    } else if phase > 1.0 - dt {
        let p = (phase - 1.0) * invdt;
        p * p + 2.0 * p + 1.0
    } else {
        0.0
    }
}

/// A unipolar (mostly 0..1) LFO waveshape by index, matching the order in
/// superdough's `waveshapes` table: 0 tri, 1 sine, 2 ramp, 3 saw, 4 square,
/// 5 custom, 6 sawblep. `skew` doubles as the `dt` argument for `sawblep`
/// (as the worklet passes it). `custom` (5) needs an array of break-points the
/// scalar worklet path can't supply, so it is treated as silence here.
pub fn waveshape(shape: usize, phase: f64, skew: f64) -> f64 {
    match shape {
        0 => {
            let x = 1.0 - skew;
            if phase >= skew {
                1.0 / x - phase / x
            } else {
                phase / skew
            }
        }
        1 => (TAU * phase).sin() * 0.5 + 0.5,
        2 => phase,
        3 => 1.0 - phase,
        4 => {
            if phase >= skew {
                0.0
            } else {
                1.0
            }
        }
        6 => {
            let v = 2.0 * phase - 1.0;
            v - poly_blep(phase, skew)
        }
        _ => 0.0,
    }
}

/// Configuration for an [`Lfo`], mirroring the `lfo-processor` parameters and
/// `getLfo`'s defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoConfig {
    pub shape: usize,
    pub frequency: f64,
    pub skew: f64,
    pub depth: f64,
    pub dcoffset: f64,
    pub phaseoffset: f64,
    pub curve: f64,
    pub time: f64,
    pub min: f64,
    pub max: f64,
}

impl Default for LfoConfig {
    fn default() -> LfoConfig {
        // getLfo defaults (helpers.mjs): the unwritten min/max default to
        // dcoffset*depth .. dcoffset*depth + depth.
        let depth = 1.0;
        let dcoffset = -0.5;
        LfoConfig {
            shape: 0,
            frequency: 1.0,
            skew: 0.5,
            depth,
            dcoffset,
            phaseoffset: 0.0,
            curve: 1.0,
            time: 0.0,
            min: dcoffset * depth,
            max: dcoffset * depth + depth,
        }
    }
}

/// A stateful per-sample LFO (one `lfo-processor` instance).
///
/// The worklet reads its params once per 128-frame render quantum
/// (`parameters[name][0]`), so params another modulator drives are latched at
/// the start of each quantum, counted from the LFO's own start.
#[derive(Clone, Debug)]
pub struct Lfo {
    phase: f64,
    sample_rate: f64,
    cfg: LfoConfig,
    /// The params in force for the current quantum.
    dt: f64,
    shape: usize,
    skew: f64,
    depth: f64,
    dcoffset: f64,
    curve: f64,
    /// Frames into the current quantum.
    frame: u32,
    /// A shape driven off the table's integer indices makes the worklet throw
    /// (`waveshapes[undefined]`), after which it outputs nothing.
    dead: bool,
}

/// Web Audio's render quantum.
const QUANTUM: u32 = 128;

impl Lfo {
    pub fn new(cfg: &LfoConfig, sample_rate: f64) -> Lfo {
        // `ffrac(time * frequency + phaseoffset)`; phase stays non-negative.
        let init = cfg.time * cfg.frequency + cfg.phaseoffset;
        Lfo {
            phase: init - init.trunc(),
            sample_rate,
            cfg: *cfg,
            dt: cfg.frequency / sample_rate,
            shape: cfg.shape,
            skew: cfg.skew,
            depth: cfg.depth,
            dcoffset: cfg.dcoffset,
            curve: cfg.curve,
            frame: 0,
            dead: false,
        }
    }

    /// The next modulation value.
    pub fn tick(&mut self) -> f64 {
        self.tick_with(&ParamOffsets::default())
    }

    /// The next value, with `inputs` added to its params by other modulators.
    pub(crate) fn tick_with(&mut self, inputs: &ParamOffsets) -> f64 {
        if self.frame == 0 {
            let c = &self.cfg;
            self.dt = (c.frequency + inputs.get(ModParam::Frequency)) / self.sample_rate;
            self.skew = c.skew + inputs.get(ModParam::Skew);
            self.depth = c.depth + inputs.get(ModParam::Depth);
            self.dcoffset = c.dcoffset + inputs.get(ModParam::Dcoffset);
            self.curve = c.curve + inputs.get(ModParam::Curve);
            let shape = c.shape as f64 + inputs.get(ModParam::Shape);
            if shape.fract() != 0.0 || !(0.0..7.0).contains(&shape) {
                self.dead = true;
            } else {
                self.shape = shape as usize;
            }
        }
        self.frame = (self.frame + 1) % QUANTUM;
        if self.dead {
            return 0.0;
        }
        let mut modval =
            (waveshape(self.shape, self.phase, self.skew) + self.dcoffset) * self.depth;
        modval = modval.powf(self.curve);
        // JS `clamp` is min(max(v,min),max), which (unlike f64::clamp) does not
        // assume min <= max and never panics.
        let out = modval.max(self.cfg.min).min(self.cfg.max);
        self.phase += self.dt;
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Configuration for a [`ModEnv`], mirroring the `envelope-processor`
/// parameter descriptors.
#[derive(Clone, Copy, Debug)]
pub struct EnvConfig {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
    /// Per-segment curvature in -1..1: positive is snappier, negative calmer.
    pub attack_curve: f64,
    pub decay_curve: f64,
    pub release_curve: f64,
    pub depth: f64,
    pub min: f64,
    pub max: f64,
    /// Seconds the envelope holds at sustain before releasing — superdough's
    /// `end - begin`, i.e. the note's length including its own release.
    pub sustain_time: f64,
}

impl Default for EnvConfig {
    fn default() -> EnvConfig {
        EnvConfig {
            attack: 0.005,
            decay: 0.14,
            sustain: 0.0,
            release: 0.1,
            attack_curve: 0.0,
            decay_curve: 0.0,
            release_curve: 0.0,
            depth: 1.0,
            min: -1e9,
            max: 1e9,
            sustain_time: 0.0,
        }
    }
}

/// One segment of the envelope state machine: how long from the trigger it
/// runs, where it starts and ends, and its curvature.
#[derive(Clone, Copy)]
struct EnvSeg {
    time: f64,
    start: f64,
    target: f64,
    curve: f64,
}

/// A stateful per-sample modulation envelope (one `envelope-processor`
/// instance), ported from superdough's worklet.
///
/// Each voice gets its own instance starting at its onset, so the worklet's
/// `begin`-change/retrigger bookkeeping collapses: time is voice-relative and
/// the envelope always starts in the attack segment.
#[derive(Clone, Debug)]
pub struct ModEnv {
    /// Voice-relative time in seconds.
    t: f64,
    dt: f64,
    /// Current envelope value before `depth` is applied.
    val: f64,
    /// Index into the segment table; 0 is idle.
    state: usize,
    cfg: EnvConfig,
}

impl ModEnv {
    pub fn new(cfg: &EnvConfig, sample_rate: f64) -> ModEnv {
        ModEnv {
            t: 0.0,
            dt: 1.0 / sample_rate,
            val: 0.0,
            // The worklet enters state 1 (attack) as soon as `begin` passes.
            state: 1,
            cfg: *cfg,
        }
    }

    /// superdough's `_warp`: bend a 0..1 phase by `curvature`.
    fn warp(phase: f64, curvature: f64) -> f64 {
        const STRENGTH: f64 = 8.0;
        if phase == 0.0 || phase == 1.0 {
            return phase; // fast exit
        }
        if curvature > 0.0 {
            // snappier
            let exp = 1.0 + STRENGTH * curvature;
            1.0 - (1.0 - phase).powf(exp)
        } else {
            // more calm
            let exp = 1.0 - STRENGTH * curvature;
            phase.powf(exp)
        }
    }

    /// The segment table, rebuilt per sample like the worklet does (its params
    /// are a-rate, so it reads them inside the loop).
    fn segments(c: &EnvConfig) -> [EnvSeg; 5] {
        let idle = EnvSeg {
            time: f64::INFINITY,
            start: 0.0,
            target: 0.0,
            curve: 0.0,
        };
        [
            idle,
            EnvSeg {
                time: c.attack,
                // The attack always starts from 0 here: a fresh per-voice
                // envelope has no held value to ramp away from.
                start: 0.0,
                target: 1.0,
                curve: c.attack_curve,
            },
            EnvSeg {
                time: c.attack + c.decay,
                start: 1.0,
                target: c.sustain,
                curve: c.decay_curve,
            },
            EnvSeg {
                time: c.sustain_time,
                start: c.sustain,
                target: c.sustain,
                curve: 0.0,
            },
            EnvSeg {
                time: c.sustain_time + c.release,
                start: c.sustain,
                target: 0.0,
                curve: c.release_curve,
            },
        ]
    }

    /// The next modulation value.
    pub fn tick(&mut self) -> f64 {
        self.tick_with(&ParamOffsets::default())
    }

    /// The next value, with `inputs` added to its params by other modulators;
    /// the worklet reads them every sample.
    fn tick_with(&mut self, inputs: &ParamOffsets) -> f64 {
        let mut c = self.cfg;
        c.attack += inputs.get(ModParam::Attack);
        c.decay += inputs.get(ModParam::Decay);
        c.sustain += inputs.get(ModParam::Sustain);
        c.release += inputs.get(ModParam::Release);
        c.depth += inputs.get(ModParam::Depth);
        let segs = Self::segments(&c);
        let seg = segs[self.state];
        // `_advance`: the phase runs from the *trigger*, not from the segment
        // start, so `time` is cumulative.
        if seg.time == 0.0 || seg.start == seg.target {
            self.val = seg.target;
        } else {
            let phase = (self.t / seg.time).min(1.0);
            self.val = seg.start + (seg.target - seg.start) * Self::warp(phase, seg.curve);
        }
        let mut time = seg.time;
        while self.t >= time {
            self.state = (self.state + 1) % segs.len();
            time = segs[self.state].time;
        }
        let out = (self.val * c.depth).max(c.min).min(c.max);
        self.t += self.dt;
        out
    }
}

// ---------------------------------------------------------------------------
// Routing: binding a modulation source to a parameter.
//
// superdough connects a modulator node to a target `AudioParam`, and Web Audio
// *sums* every connection into the param's intrinsic value. So a modulator is
// an additive offset on the control's own value, and two modulators on one
// param add. Which param a modulator lands on is decided in `routing.rs`, by
// superdough's own rules; this section names rudel's params and runs the
// sources.

/// A parameter of a modulator itself, which another modulator can drive
/// (`lfo({ c: 'lfo_0', sc: 'rate' })`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModParam {
    Frequency,
    Depth,
    Skew,
    Curve,
    Dcoffset,
    Shape,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl ModParam {
    const COUNT: usize = 10;

    fn index(self) -> usize {
        self as usize
    }

    /// The LFO worklet's AudioParam of this name (`lfo-processor`). `begin`,
    /// `time`, `end`, `phaseoffset`, `min` and `max` exist too but only take
    /// effect at setup, so nothing modulating them would be heard.
    pub(crate) fn lfo_param(name: &str) -> Option<ModParam> {
        Some(match name {
            "frequency" => ModParam::Frequency,
            "depth" => ModParam::Depth,
            "skew" => ModParam::Skew,
            "curve" => ModParam::Curve,
            "dcoffset" => ModParam::Dcoffset,
            "shape" => ModParam::Shape,
            _ => return None,
        })
    }

    /// The envelope worklet's AudioParam of this name (`envelope-processor`).
    pub(crate) fn env_param(name: &str) -> Option<ModParam> {
        Some(match name {
            "attack" => ModParam::Attack,
            "decay" => ModParam::Decay,
            "sustain" => ModParam::Sustain,
            "release" => ModParam::Release,
            "depth" => ModParam::Depth,
            _ => return None,
        })
    }
}

/// Offsets added to an LFO's or envelope's own params this sample, by other
/// modulators.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ParamOffsets([f64; ModParam::COUNT]);

impl ParamOffsets {
    fn get(&self, p: ModParam) -> f64 {
        self.0[p.index()]
    }
}

/// Which of a voice's three filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterSlot {
    Low,
    High,
    Band,
}

/// The dynamics compressor's params.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompParam {
    Threshold,
    Ratio,
    Knee,
    Attack,
    Release,
}

/// A parameter a modulator can be routed to: one entry per AudioParam that
/// superdough's table reaches and that exists, named for rudel's DSP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModTarget {
    /// The source's frequency in Hz (`s`/`freq`/`note` on an oscillator or a
    /// synth worklet).
    Frequency,
    /// A buffer source's `detune` in cents: what `s`/`freq`/`note` reach on a
    /// sample, a soundfont or a noise source, which have no `frequency`.
    Detune,
    /// Supersaw and wavetable unison: frequency spread and stereo spread.
    Freqspread,
    Panspread,
    /// Wavetable position and warp, and the LFOs on them.
    WtPosition,
    WtWarp,
    WtLfo(ModParam),
    WarpLfo(ModParam),
    /// The pulse width and its LFO.
    Pulsewidth,
    PwLfo(ModParam),
    /// FM operator `n` (1..=8): its modulation index, and its frequency in Hz.
    FmGain(u8),
    FmFreq(u8),
    /// Vibrato rate in Hz, and depth in cents.
    VibFreq,
    VibGain,
    /// The voice's gain stage, and its pan (bipolar, `2·pan − 1`).
    Gain,
    Pan,
    /// The filters, and the cutoff LFOs on them.
    Cutoff,
    Resonance,
    Hcutoff,
    Hresonance,
    Bandf,
    Bandq,
    FilterLfo(FilterSlot, ModParam),
    /// Post-effects.
    Vowel,
    Coarse,
    Crush,
    Shape,
    Shapevol,
    Distort,
    Distortvol,
    Postgain,
    Stretch,
    Tremolo(ModParam),
    TremoloGain,
    Compressor(CompParam),
    PhaserLfo(ModParam),
    PhaserCenter,
    PhaserQ,
    /// The orbit: this voice's sends, and the shared delay and DJ filter.
    DelaySend,
    DelayTime,
    DelayFeedback,
    RoomSend,
    Djf,
    /// Another modulator in the same bank (by its position there), one of its
    /// own params.
    Modulator(usize, ModParam),
}

/// Which part of the signal chain applies a target, so each can own (and tick)
/// only the modulators it is able to consume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModOwner {
    /// The synth/sampler voice: the source, gain, pan and the filters.
    Voice,
    /// The post-effect chain.
    PostFx,
    /// The orbit's sends and shared effects, applied by the mixer.
    Orbit,
    /// One `FX(...)` stage's gain and filters, and its post-effect rack.
    StageVoice(usize),
    StagePost(usize),
    /// One `FX(...)` stage's own delay and reverb, which a stage runs inline
    /// where the main chain sends to the orbit.
    StageSends(usize),
}

impl ModTarget {
    /// Number of offset slots (every target but [`ModTarget::Modulator`]):
    /// 34 single params, 5 compressor params, five LFOs of [`ModParam::COUNT`]
    /// params each plus three filter LFOs, and 8 FM gains and 8 frequencies.
    pub const SLOTS: usize = 39 + 8 * ModParam::COUNT + 16;

    /// The offset slot of a parameter target.
    #[inline]
    pub fn index(self) -> usize {
        use ModTarget::*;
        const LFO: usize = 39;
        const N: usize = ModParam::COUNT;
        let lfo = |block: usize, m: ModParam| LFO + block * N + m.index();
        match self {
            Frequency => 0,
            Detune => 1,
            Freqspread => 2,
            Panspread => 3,
            WtPosition => 4,
            WtWarp => 5,
            Pulsewidth => 6,
            VibFreq => 7,
            VibGain => 8,
            Gain => 9,
            Pan => 10,
            Cutoff => 11,
            Resonance => 12,
            Hcutoff => 13,
            Hresonance => 14,
            Bandf => 15,
            Bandq => 16,
            Vowel => 17,
            Coarse => 18,
            Crush => 19,
            Shape => 20,
            Shapevol => 21,
            Distort => 22,
            Distortvol => 23,
            Postgain => 24,
            Stretch => 25,
            TremoloGain => 26,
            PhaserCenter => 27,
            PhaserQ => 28,
            DelaySend => 29,
            DelayTime => 30,
            DelayFeedback => 31,
            RoomSend => 32,
            Djf => 33,
            Compressor(c) => 34 + c as usize,
            WtLfo(m) => lfo(0, m),
            WarpLfo(m) => lfo(1, m),
            PwLfo(m) => lfo(2, m),
            Tremolo(m) => lfo(3, m),
            PhaserLfo(m) => lfo(4, m),
            FilterLfo(slot, m) => lfo(5 + slot as usize, m),
            FmGain(n) => LFO + 8 * N + (n as usize - 1),
            FmFreq(n) => LFO + 8 * N + 8 + (n as usize - 1),
            Modulator(..) => panic!("a modulator target has no offset slot"),
        }
    }

    /// Every parameter target (all but [`ModTarget::Modulator`]).
    #[cfg(test)]
    pub(crate) fn all() -> Vec<ModTarget> {
        use ModParam::{Dcoffset, Depth, Frequency as Rate, Shape as Wave, Skew};
        use ModTarget::*;
        let mut all = vec![
            Frequency,
            Detune,
            Freqspread,
            Panspread,
            WtPosition,
            WtWarp,
            Pulsewidth,
            VibFreq,
            VibGain,
            Gain,
            Pan,
            Cutoff,
            Resonance,
            Hcutoff,
            Hresonance,
            Bandf,
            Bandq,
            Vowel,
            Coarse,
            Crush,
            Shape,
            Shapevol,
            Distort,
            Distortvol,
            Postgain,
            Stretch,
            TremoloGain,
            PhaserCenter,
            PhaserQ,
            DelaySend,
            DelayTime,
            DelayFeedback,
            RoomSend,
            Djf,
        ];
        for c in [
            CompParam::Threshold,
            CompParam::Ratio,
            CompParam::Knee,
            CompParam::Attack,
            CompParam::Release,
        ] {
            all.push(Compressor(c));
        }
        // The LFO params superdough's table reaches on each LFO node.
        for m in [Rate, Depth, Skew] {
            all.extend([WtLfo(m), WarpLfo(m)]);
        }
        all.extend([PwLfo(Rate), PwLfo(Depth), PhaserLfo(Rate), PhaserLfo(Depth)]);
        all.extend([Tremolo(Rate), Tremolo(Skew), Tremolo(Wave)]);
        for slot in [FilterSlot::Low, FilterSlot::High, FilterSlot::Band] {
            for m in [Depth, Wave, Dcoffset, Skew] {
                all.push(FilterLfo(slot, m));
            }
        }
        for n in 1..=8 {
            all.extend([FmGain(n), FmFreq(n)]);
        }
        all
    }

    /// Who applies it, for a modulator connected through `fxi`.
    pub fn owner(self, fxi: crate::routing::Fxi) -> ModOwner {
        use ModTarget::*;
        let post = match self {
            Vowel | Coarse | Crush | Shape | Shapevol | Distort | Distortvol | Postgain
            | Stretch | Tremolo(_) | TremoloGain | Compressor(_) | PhaserLfo(_) | PhaserCenter
            | PhaserQ => true,
            // A stage's delay and reverb are its own; the main chain's are the
            // orbit's.
            DelaySend | DelayTime | DelayFeedback | RoomSend => {
                return match fxi {
                    crate::routing::Fxi::Stage(i) => ModOwner::StageSends(i),
                    _ => ModOwner::Orbit,
                };
            }
            Djf => return ModOwner::Orbit,
            _ => false,
        };
        match (fxi, post) {
            (crate::routing::Fxi::Stage(i), true) => ModOwner::StagePost(i),
            (crate::routing::Fxi::Stage(i), false) => ModOwner::StageVoice(i),
            (_, true) => ModOwner::PostFx,
            (_, false) => ModOwner::Voice,
        }
    }

    /// `getRangeForParam` keys its 20 Hz..24 kHz clamp off a param *named*
    /// `frequency`; a buffer source's `detune` counts, since the lookup asked
    /// for `frequency` before falling back to it.
    fn is_frequency(self) -> bool {
        use ModTarget::*;
        matches!(
            self,
            Frequency
                | Detune
                | Cutoff
                | Hcutoff
                | Bandf
                | Vowel
                | PhaserCenter
                | VibFreq
                | FmFreq(_)
                | WtLfo(ModParam::Frequency)
                | WarpLfo(ModParam::Frequency)
                | PwLfo(ModParam::Frequency)
                | Tremolo(ModParam::Frequency)
                | PhaserLfo(ModParam::Frequency)
                | Modulator(_, ModParam::Frequency)
        )
    }

    /// The main-chain target of a control when every node is present: what
    /// the routing finds for a plain synth voice with every effect on. Used by
    /// tests and by [`ModSpecs::from_controls`].
    pub fn from_control(name: &str) -> Option<ModTarget> {
        let (node, param) = crate::routing::control_data(name, None)?;
        ModTarget::of_node(node, param)
    }

    /// The target for a node key and param name, when the node is present.
    /// `None` when that node has no such param (the throwing case) or the node
    /// is not one a voice builds.
    pub(crate) fn of_node(node: &str, param: &str) -> Option<ModTarget> {
        use ModTarget::*;
        let lfo = ModParam::lfo_param;
        Some(match (node, param) {
            ("source", "frequency") => Frequency,
            ("source", "freqspread") => Freqspread,
            ("source", "panspread") => Panspread,
            ("source", "position") => WtPosition,
            ("source", "warp") => WtWarp,
            ("source", "pulsewidth") => Pulsewidth,
            ("wt_lfo", p) => WtLfo(lfo(p)?),
            ("warp_lfo", p) => WarpLfo(lfo(p)?),
            ("pw_lfo", p) => PwLfo(lfo(p)?),
            ("vib", "frequency") => VibFreq,
            ("vib_gain", "gain") => VibGain,
            ("gain", "gain") => Gain,
            ("pan", "pan") => Pan,
            ("lpf", "frequency") => Cutoff,
            ("lpf", "Q") => Resonance,
            ("hpf", "frequency") => Hcutoff,
            ("hpf", "Q") => Hresonance,
            ("bpf", "frequency") => Bandf,
            ("bpf", "Q") => Bandq,
            ("lpf_lfo", p) => FilterLfo(FilterSlot::Low, lfo(p)?),
            ("hpf_lfo", p) => FilterLfo(FilterSlot::High, lfo(p)?),
            ("bpf_lfo", p) => FilterLfo(FilterSlot::Band, lfo(p)?),
            ("vowel", "frequency") => Vowel,
            ("coarse", "coarse") => Coarse,
            ("crush", "crush") => Crush,
            ("shape", "shape") => Shape,
            ("shape", "postgain") => Shapevol,
            ("distort", "distort") => Distort,
            ("distort", "postgain") => Distortvol,
            ("post", "gain") => Postgain,
            ("stretch", "pitchFactor") => Stretch,
            ("tremolo", p) => Tremolo(lfo(p)?),
            ("tremolo_gain", "gain") => TremoloGain,
            ("compressor", "threshold") => Compressor(CompParam::Threshold),
            ("compressor", "ratio") => Compressor(CompParam::Ratio),
            ("compressor", "knee") => Compressor(CompParam::Knee),
            ("compressor", "attack") => Compressor(CompParam::Attack),
            ("compressor", "release") => Compressor(CompParam::Release),
            ("phaser_lfo", p) => PhaserLfo(lfo(p)?),
            ("phaser", "frequency") => PhaserCenter,
            ("phaser", "Q") => PhaserQ,
            ("delay_mix", "gain") => DelaySend,
            ("delay", "delayTime") => DelayTime,
            ("delay", "feedback") => DelayFeedback,
            ("room_mix", "gain") => RoomSend,
            ("djf", "value") => Djf,
            (fm, p) if fm.starts_with("fm_") => {
                let rest = &fm[3..];
                let (n, gain) = match rest.strip_suffix("_gain") {
                    Some(n) => (n, true),
                    None => (rest, false),
                };
                let n: u8 = n.parse().ok().filter(|n| (1..=8).contains(n))?;
                match (gain, p) {
                    (true, "gain") => FmGain(n),
                    (false, "frequency") => FmFreq(n),
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

/// What resolving a modulator descriptor needs beyond the control map: the
/// pattern clock (an LFO's phase is locked to cycle time unless it retriggers)
/// and the note's length (an envelope's sustain span).
#[derive(Clone, Copy, Debug, Default)]
pub struct ModContext {
    pub cps: f64,
    /// The hap's onset in cycles.
    pub cycle: f64,
    /// The note's length in seconds, including its release.
    pub note_seconds: f64,
}

/// Configuration for a [`BusMod`], mirroring `connectBusModulator`'s graph:
/// a `ConstantSourceNode(dc)` summed with the bus signal, through a gain, then
/// (for frequency params) a clamping waveshaper.
#[derive(Clone, Copy, Debug)]
pub struct BusConfig {
    /// Which numbered bus to read (`bmod({ b: 1 })`).
    pub bus: i32,
    /// DC offset added to the bus signal before scaling.
    pub dc: f64,
    /// The depth gain. Upstream builds `sign(d) * abs(d) / 0.3`, i.e. `d / 0.3`
    /// — the 0.3 assumes a bus carrying a signal of roughly that amplitude.
    pub gain: f64,
    pub min: f64,
    pub max: f64,
}

/// A resolved but not-yet-running modulation source.
#[derive(Clone, Debug)]
enum SourceConfig {
    Lfo(LfoConfig),
    Env(EnvConfig),
    Bus(BusConfig),
}

/// One resolved modulator: the param it offsets plus its source config.
///
/// Sample-rate free, so the scheduler can resolve it from the hap while the
/// mixer instantiates it at the device rate ([`ModBank::new`]).
#[derive(Clone, Debug)]
pub struct ModSpec {
    target: ModTarget,
    source: SourceConfig,
}

/// The modulators a hap carries, split by which part of the chain applies
/// them so each side ticks only its own. A modulator that modulates another
/// lives with the one it modulates.
#[derive(Clone, Debug, Default)]
pub struct ModSpecs {
    pub voice: Vec<ModSpec>,
    pub post: Vec<ModSpec>,
    pub orbit: Vec<ModSpec>,
    /// Per `FX(...)` stage, by index: its voice side, its post-effects and
    /// its delay and reverb.
    pub stages: Vec<(Vec<ModSpec>, Vec<ModSpec>, Vec<ModSpec>)>,
}

impl ModSpec {
    /// The param this modulator lands on.
    pub fn target(&self) -> ModTarget {
        self.target
    }
}

impl ModSpecs {
    pub fn is_empty(&self) -> bool {
        self.voice.is_empty()
            && self.post.is_empty()
            && self.orbit.is_empty()
            && self
                .stages
                .iter()
                .all(|(v, p, s)| v.is_empty() && p.is_empty() && s.is_empty())
    }

    /// The specs for one owner.
    pub fn for_owner(&self, owner: ModOwner) -> &[ModSpec] {
        match owner {
            ModOwner::Voice => &self.voice,
            ModOwner::PostFx => &self.post,
            ModOwner::Orbit => &self.orbit,
            ModOwner::StageVoice(i) => self.stages.get(i).map_or(&[], |s| s.0.as_slice()),
            ModOwner::StagePost(i) => self.stages.get(i).map_or(&[], |s| s.1.as_slice()),
            ModOwner::StageSends(i) => self.stages.get(i).map_or(&[], |s| s.2.as_slice()),
        }
    }

    fn list_mut(&mut self, owner: ModOwner) -> &mut Vec<ModSpec> {
        match owner {
            ModOwner::Voice => &mut self.voice,
            ModOwner::PostFx => &mut self.post,
            ModOwner::Orbit => &mut self.orbit,
            ModOwner::StageVoice(i) | ModOwner::StagePost(i) | ModOwner::StageSends(i) => {
                if self.stages.len() <= i {
                    self.stages
                        .resize(i + 1, (Vec::new(), Vec::new(), Vec::new()));
                }
                match owner {
                    ModOwner::StageVoice(_) => &mut self.stages[i].0,
                    ModOwner::StagePost(_) => &mut self.stages[i].1,
                    _ => &mut self.stages[i].2,
                }
            }
        }
    }
}

/// A running bus modulator: it has no oscillator of its own, it just reads the
/// signal another pattern sent to a bus with `.bus(n)`.
///
/// The mixer refills `input` with that bus's samples for the block about to be
/// rendered ([`ModBank::set_bus_input`]), so a bus modulator is sample-accurate
/// within a block as long as the sending voices render first.
#[derive(Clone, Debug)]
struct BusMod {
    cfg: BusConfig,
    input: Vec<f32>,
    pos: usize,
}

impl BusMod {
    fn tick(&mut self) -> f64 {
        let x = self.input.get(self.pos).copied().unwrap_or(0.0) as f64;
        self.pos += 1;
        ((x + self.cfg.dc) * self.cfg.gain)
            .max(self.cfg.min)
            .min(self.cfg.max)
    }
}

/// A live modulation source bound to a target.
#[derive(Clone, Debug)]
enum ModSource {
    Lfo(Lfo),
    Env(ModEnv),
    Bus(BusMod),
}

/// One running modulator.
#[derive(Clone, Debug)]
struct Modulation {
    target: ModTarget,
    source: ModSource,
    /// What later modulators add to this one's params, this sample.
    inputs: ParamOffsets,
}

impl Modulation {
    fn tick(&mut self) -> f64 {
        let inputs = std::mem::take(&mut self.inputs);
        match &mut self.source {
            ModSource::Lfo(l) => l.tick_with(&inputs),
            ModSource::Env(e) => e.tick_with(&inputs),
            ModSource::Bus(b) => b.tick(),
        }
    }
}

/// `getRangeForParam`: a frequency param is clamped so the *modulated* value
/// stays inside 20Hz..24kHz. A low current value indicates the param is itself
/// an LFO rate, which is left alone. Anything else is unclamped.
fn range_for(target: ModTarget, current: f64) -> Option<(f64, f64)> {
    (target.is_frequency() && current >= 30.0).then_some((20.0 - current, 24000.0 - current))
}

/// A bank of modulators owned by one part of the chain, ticked once per sample
/// into a small offset table the consumer reads by target.
#[derive(Clone, Debug)]
pub struct ModBank {
    mods: Vec<Modulation>,
    offsets: Box<[f32; ModTarget::SLOTS]>,
}

impl Default for ModBank {
    fn default() -> ModBank {
        ModBank {
            mods: Vec::new(),
            offsets: Box::new([0.0; ModTarget::SLOTS]),
        }
    }
}

impl ModBank {
    /// True when nothing is modulated, so the whole stage can be skipped.
    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
    }

    /// Advance every source by one sample. Later modulators run first, because
    /// a modulator can only drive one created before it: so each one has heard
    /// this sample's input from those modulating it before it runs.
    pub fn tick(&mut self) {
        if self.mods.is_empty() {
            return;
        }
        self.offsets.fill(0.0);
        for i in (0..self.mods.len()).rev() {
            let value = self.mods[i].tick();
            match self.mods[i].target {
                ModTarget::Modulator(j, param) => {
                    if let Some(m) = self.mods.get_mut(j) {
                        m.inputs.0[param.index()] += value;
                    }
                }
                target => self.offsets[target.index()] += value as f32,
            }
        }
    }

    /// The current additive offset for `target` (0.0 when unmodulated).
    #[inline]
    pub fn get(&self, target: ModTarget) -> f32 {
        // Most voices have no modulators: skip the slot lookup altogether.
        if self.mods.is_empty() {
            return 0.0;
        }
        self.offsets[target.index()]
    }

    /// The offsets on one of the voice's own LFOs (a filter's, the pulse
    /// width's, a wavetable param's, the tremolo's), as that LFO's inputs.
    pub(crate) fn lfo_inputs(&self, target: impl Fn(ModParam) -> ModTarget) -> ParamOffsets {
        let mut inputs = ParamOffsets::default();
        if self.mods.is_empty() {
            return inputs;
        }
        for p in [
            ModParam::Frequency,
            ModParam::Depth,
            ModParam::Skew,
            ModParam::Curve,
            ModParam::Dcoffset,
            ModParam::Shape,
        ] {
            inputs.0[p.index()] = self.offsets[target(p).index()] as f64;
        }
        inputs
    }

    /// Instantiate the specs for one owner at `sample_rate`.
    pub fn new(specs: &[ModSpec], sample_rate: f64) -> ModBank {
        ModBank {
            mods: specs
                .iter()
                .map(|s| Modulation {
                    target: s.target,
                    source: match &s.source {
                        SourceConfig::Lfo(c) => ModSource::Lfo(Lfo::new(c, sample_rate)),
                        SourceConfig::Env(c) => ModSource::Env(ModEnv::new(c, sample_rate)),
                        SourceConfig::Bus(c) => ModSource::Bus(BusMod {
                            cfg: *c,
                            input: Vec::new(),
                            pos: 0,
                        }),
                    },
                    inputs: ParamOffsets::default(),
                })
                .collect(),
            offsets: Box::new([0.0; ModTarget::SLOTS]),
        }
    }

    /// Hand bus `bus`'s signal for the block about to be rendered to every
    /// `bmod` modulator reading that bus, and rewind them to its start. Summed
    /// to mono, as Web Audio does on the way into an `AudioParam`.
    pub fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        for m in &mut self.mods {
            if let ModSource::Bus(b) = &mut m.source
                && b.cfg.bus == bus
            {
                b.input.clear();
                b.input
                    .extend(left.iter().zip(right).map(|(l, r)| (l + r) * 0.5));
                b.pos = 0;
            }
        }
    }
}

/// A graph in which every node a voice can build is present and every param
/// reads `base(target)`: the shape [`ModSpecs::from_controls`] resolves against.
struct EveryNode<F>(F);

impl<F: Fn(ModTarget) -> f32> crate::routing::ModGraph for EveryNode<F> {
    fn lookup(&self, _: crate::routing::Fxi, node: &str, param: &str) -> crate::routing::Lookup {
        use crate::routing::Lookup;
        let known = [
            "source",
            "gain",
            "post",
            "pan",
            "stretch",
            "tremolo",
            "tremolo_gain",
            "lpf",
            "hpf",
            "bpf",
            "lpf_lfo",
            "hpf_lfo",
            "bpf_lfo",
            "vowel",
            "coarse",
            "crush",
            "shape",
            "distort",
            "compressor",
            "phaser",
            "phaser_lfo",
            "delay",
            "delay_mix",
            "room_mix",
            "djf",
            "vib",
            "vib_gain",
            "wt_lfo",
            "warp_lfo",
            "pw_lfo",
        ];
        if !known.contains(&node) && !node.starts_with("fm_") {
            return Lookup::Absent;
        }
        match ModTarget::of_node(node, param) {
            Some(t) => Lookup::Found(t, (self.0)(t) as f64),
            None => Lookup::NoParam,
        }
    }
}

/// A modulator created so far: where it lives and what it is, so a later one
/// can reach it by name.
struct Created {
    name: String,
    owner: ModOwner,
    /// Its position in its owner's list.
    index: usize,
    lfo: bool,
    /// Its params' current values, for a relative depth.
    config: SourceConfig,
}

impl ModSpecs {
    /// Resolve a hap's `lfo`/`env`/`bmod` descriptors against a graph where
    /// every node a voice can build is present and each param reads
    /// `base(target)` — the main controls only.
    pub fn from_controls(
        map: &ValueMap,
        ctx: &ModContext,
        base: impl Fn(ModTarget) -> f32,
    ) -> ModSpecs {
        ModSpecs::resolve(map, &[], ctx, &EveryNode(base))
    }

    /// Resolve a hap's `lfo`/`env`/`bmod` descriptors as superdough's final
    /// loop does: each `FX` stage's modulators and then the main controls',
    /// `lfo` before `env` before `bmod`, each in `__ids` order, against the
    /// hap's own `graph`. A modulator whose target throws drops itself and
    /// every one after it.
    pub fn resolve(
        map: &ValueMap,
        stages: &[ValueMap],
        ctx: &ModContext,
        graph: &dyn crate::routing::ModGraph,
    ) -> ModSpecs {
        use crate::routing::{Fxi, Outcome, resolve};
        let mut out = ModSpecs::default();
        let mut created: Vec<Created> = Vec::new();
        let maps = stages.iter().enumerate().map(|(i, m)| (Fxi::Stage(i), m));
        for (key, fx) in maps.chain(std::iter::once((Fxi::Main, map))) {
            for kind in ["lfo", "env", "bmod"] {
                let Some(Value::Map(desc)) = fx.get(kind) else {
                    continue;
                };
                let Some(Value::List(ids)) = desc.get("__ids") else {
                    continue;
                };
                for id in ids {
                    let id = id_key(id);
                    let Some(Value::Map(entry)) = desc.get(&id) else {
                        continue;
                    };
                    let Some(control) = entry.get("control").and_then(Value::as_str) else {
                        continue;
                    };
                    let sub = entry.get("subControl").and_then(Value::as_str);
                    // `params.fxi ??= key`: a number is a stage, anything
                    // else the main chain.
                    let fxi = match entry.get("fxi").and_then(Value::as_f64) {
                        Some(i) if i >= 0.0 => Fxi::Stage(i as usize),
                        _ if entry.get("fxi").is_some() => Fxi::Main,
                        _ => key,
                    };
                    let outcome = match fxi {
                        // Modulators are registered in `nodes.main` only.
                        Fxi::Main => match resolve_modulator(&created, control, sub) {
                            Some(found) => found,
                            None => resolve(graph, fxi, control, sub),
                        },
                        Fxi::Stage(_) => resolve(graph, fxi, control, sub),
                    };
                    let (target, current, owner) = match outcome {
                        Outcome::Skip => continue,
                        Outcome::Throw => return out,
                        Outcome::Connect(ModTarget::Modulator(index, param), current) => {
                            let into = &created[index];
                            (ModTarget::Modulator(into.index, param), current, into.owner)
                        }
                        Outcome::Connect(target, current) => (target, current, target.owner(fxi)),
                    };
                    let Some(source) = source_config(kind, entry, ctx, target, current) else {
                        continue;
                    };
                    let list = out.list_mut(owner);
                    if kind != "bmod" {
                        // `nodeTracker.main[`lfo_${id}`] = [lfoNode]`: a later
                        // modulator can reach this one by name.
                        created.retain(|c| c.name != format!("{kind}_{id}"));
                        created.push(Created {
                            name: format!("{kind}_{id}"),
                            owner,
                            index: list.len(),
                            lfo: kind == "lfo",
                            config: source.clone(),
                        });
                    }
                    list.push(ModSpec { target, source });
                }
            }
        }
        out
    }
}

/// A modulator reached by name (`lfo_0`, `env_mod`): the same table lookup
/// as any other control, against the modulators created so far. `None` when
/// no such modulator exists, so the graph is asked instead.
fn resolve_modulator(
    created: &[Created],
    control: &str,
    sub: Option<&str>,
) -> Option<crate::routing::Outcome> {
    use crate::routing::{Outcome, control_data};
    let position = created.iter().rposition(|c| c.name == control)?;
    let found = &created[position];
    let (_, param) = control_data(control, sub)?;
    let param = if found.lfo {
        ModParam::lfo_param(param)
    } else {
        ModParam::env_param(param)
    };
    Some(match param {
        Some(p) => Outcome::Connect(
            ModTarget::Modulator(position, p),
            current_of(&found.config, p),
        ),
        None => Outcome::Throw,
    })
}

/// A modulator param's value at setup.
fn current_of(config: &SourceConfig, p: ModParam) -> f64 {
    match config {
        SourceConfig::Lfo(c) => match p {
            ModParam::Frequency => c.frequency,
            ModParam::Depth => c.depth,
            ModParam::Skew => c.skew,
            ModParam::Curve => c.curve,
            ModParam::Dcoffset => c.dcoffset,
            ModParam::Shape => c.shape as f64,
            _ => 0.0,
        },
        SourceConfig::Env(c) => match p {
            ModParam::Attack => c.attack,
            ModParam::Decay => c.decay,
            ModParam::Sustain => c.sustain,
            ModParam::Release => c.release,
            ModParam::Depth => c.depth,
            _ => 0.0,
        },
        SourceConfig::Bus(_) => 0.0,
    }
}

/// `connectLFO`/`connectEnvelope`/`connectBusModulator`: the source a
/// descriptor builds for a param whose value is `current`.
fn source_config(
    kind: &str,
    entry: &ValueMap,
    ctx: &ModContext,
    target: ModTarget,
    current: f64,
) -> Option<SourceConfig> {
    let get = |k: &str| entry.get(k).and_then(|v| v.as_f64());
    // `currentValue === 0 ? 1 : currentValue`, then
    // `depthabs ?? depth * currentValue`.
    let current = if current == 0.0 { 1.0 } else { current };
    let depth = get("depthabs").unwrap_or(get("depth").unwrap_or(1.0) * current);
    let range = range_for(target, current);
    Some(if kind == "lfo" {
        let d = LfoConfig::default();
        let dcoffset = get("dcoffset").unwrap_or(d.dcoffset);
        let (min, max) = range.unwrap_or((dcoffset * depth, dcoffset * depth + depth));
        let retrig = get("retrig").unwrap_or(0.0);
        SourceConfig::Lfo(LfoConfig {
            shape: shape_index(entry.get("shape")),
            // `sync` is in cycles, `rate` in Hz.
            frequency: match get("sync") {
                Some(s) => s * ctx.cps,
                None => get("rate").unwrap_or(1.0),
            },
            skew: get("skew").unwrap_or(d.skew),
            depth,
            dcoffset,
            phaseoffset: get("phaseoffset").unwrap_or(d.phaseoffset),
            curve: get("curve").unwrap_or(d.curve),
            // Unless it retriggers, the phase is locked to the global cycle
            // clock rather than the note onset.
            time: if retrig > 0.5 {
                0.0
            } else {
                ctx.cycle / ctx.cps.max(1e-9)
            },
            min,
            max,
        })
    } else if kind == "bmod" {
        // A `bmod` with no bus reads `getBus(undefined)` upstream, which
        // nothing ever sends to; skipping is the same silence.
        let bus = get("bus")?;
        let (min, max) = range.unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
        SourceConfig::Bus(BusConfig {
            bus: bus as i32,
            dc: get("dc").unwrap_or(0.0),
            gain: depth / 0.3,
            min,
            max,
        })
    } else {
        let d = EnvConfig::default();
        let (min, max) = range.unwrap_or((d.min, d.max));
        SourceConfig::Env(EnvConfig {
            attack: get("attack").unwrap_or(d.attack),
            decay: get("decay").unwrap_or(d.decay),
            sustain: get("sustain").unwrap_or(d.sustain),
            release: get("release").unwrap_or(d.release),
            attack_curve: get("acurve").unwrap_or(d.attack_curve),
            decay_curve: get("dcurve").unwrap_or(d.decay_curve),
            release_curve: get("rcurve").unwrap_or(d.release_curve),
            depth,
            min,
            max,
            sustain_time: ctx.note_seconds,
        })
    })
}

/// superdough's `getModulationShapeInput`: a number indexes the waveshape table
/// (mod 5), a name looks it up, anything else is the triangle.
pub(crate) fn shape_index(v: Option<&Value>) -> usize {
    match v {
        Some(Value::Str(s)) => match s.as_str() {
            "sine" => 1,
            "ramp" => 2,
            "saw" => 3,
            "square" => 4,
            _ => 0, // tri / triangle / unknown
        },
        Some(other) => other
            .as_f64()
            .map(|n| (n as i64).rem_euclid(5) as usize)
            .unwrap_or(0),
        None => 0,
    }
}

/// The string key an id value maps to, mirroring `modulate.rs`'s `id_key`
/// (JS object keys are strings; whole numbers render without a decimal point).
fn id_key(id: &Value) -> String {
    match id {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        // `{}` prints a whole f64 without a decimal point.
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // --- the name and descriptor tables ------------------------------------
    //
    // The 2026-08 mutation run left 80 of modulator.rs's 234 mutants alive, and
    // the two biggest clusters were lookup tables: `ModTarget::from_control`
    // (13) and `ModSpecs::from_controls` (13). Both sit between a pattern's
    // controls and the DSP, so a wrong arm does not error — it modulates
    // something else, or nothing.

    /// A one-entry modulator descriptor in the nested-map shape a script hands over.
    fn descriptor(kind: &str, entries: &[(&str, Value)]) -> ValueMap {
        let entry: ValueMap = entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let desc: ValueMap = [
            ("__ids".to_string(), Value::List(vec![Value::Int(0)])),
            ("0".to_string(), Value::Map(entry)),
        ]
        .into_iter()
        .collect();
        [(kind.to_string(), Value::Map(desc))].into_iter().collect()
    }

    fn ctx() -> ModContext {
        ModContext {
            cps: 0.5,
            cycle: 0.0,
            note_seconds: 1.0,
        }
    }

    #[rstest]
    #[case("gain", ModTarget::Gain)]
    #[case("cutoff", ModTarget::Cutoff)]
    #[case("resonance", ModTarget::Resonance)]
    #[case("hcutoff", ModTarget::Hcutoff)]
    #[case("hresonance", ModTarget::Hresonance)]
    #[case("bandf", ModTarget::Bandf)]
    #[case("bandq", ModTarget::Bandq)]
    #[case("postgain", ModTarget::Postgain)]
    #[case("shape", ModTarget::Shape)]
    #[case("shapevol", ModTarget::Shapevol)]
    #[case("distort", ModTarget::Distort)]
    #[case("distortvol", ModTarget::Distortvol)]
    #[case("crush", ModTarget::Crush)]
    #[case("coarse", ModTarget::Coarse)]
    // Pitch has three spellings, all the same target.
    #[case("s", ModTarget::Frequency)]
    #[case("freq", ModTarget::Frequency)]
    #[case("note", ModTarget::Frequency)]
    fn every_modulatable_control_names_its_own_target(#[case] name: &str, #[case] want: ModTarget) {
        assert_eq!(ModTarget::from_control(name), Some(want));
    }

    // Names are exact: no case folding, no prefixes.
    #[rstest]
    fn a_near_miss_is_not_a_modulation_target(
        #[values("", "Gain", "gains", "gai", "cut", "lpf", "nonesuch")] name: &str,
    ) {
        assert_eq!(ModTarget::from_control(name), None);
    }

    #[test]
    fn every_target_is_reachable_from_some_control() {
        // Guards `of_node` against a dropped or swapped arm: every parameter
        // target is what some row of superdough's table lands on (`Detune` is
        // the buffer-source fallback for `frequency`, found by the graph).
        let reached: Vec<ModTarget> = crate::routing::tests_support::all_rows()
            .filter_map(|(node, param)| ModTarget::of_node(node, param))
            .collect();
        for target in ModTarget::all() {
            if target == ModTarget::Detune {
                continue;
            }
            assert!(reached.contains(&target), "{target:?} is unreachable");
        }
        assert_eq!(
            reached
                .iter()
                .filter(|t| **t == ModTarget::Frequency)
                .count(),
            3,
            "s, freq and note all reach the source's frequency"
        );
    }

    // A modulator has to run in the stage that owns its parameter; landing in the
    // wrong bank means it is ticked at the wrong point in the chain.
    #[rstest]
    #[case::freq("freq", true)]
    #[case::gain("gain", true)]
    #[case::cutoff("cutoff", true)]
    #[case::resonance("resonance", true)]
    #[case::hcutoff("hcutoff", true)]
    #[case::hresonance("hresonance", true)]
    #[case::bandf("bandf", true)]
    #[case::bandq("bandq", true)]
    #[case::postgain("postgain", false)]
    #[case::shape("shape", false)]
    #[case::distort("distort", false)]
    #[case::crush("crush", false)]
    #[case::coarse("coarse", false)]
    fn voice_and_post_fx_modulators_are_kept_apart(#[case] name: &str, #[case] voice_side: bool) {
        let map = descriptor(
            "lfo",
            &[
                ("control", Value::from(name)),
                ("depthabs", Value::F64(0.5)),
                ("rate", Value::F64(2.0)),
            ],
        );
        let specs = ModSpecs::from_controls(&map, &ctx(), |_| 25.0);
        assert!(!specs.is_empty(), "{name} should resolve to a modulator");
        if voice_side {
            assert!(!specs.voice.is_empty(), "{name} belongs to the voice");
            assert!(specs.post.is_empty(), "{name} is not a post-fx modulator");
        } else {
            assert!(!specs.post.is_empty(), "{name} belongs to post-fx");
            assert!(specs.voice.is_empty(), "{name} is not a voice modulator");
        }
    }

    #[test]
    fn a_control_that_cannot_be_modulated_yields_no_modulator() {
        // Rather than defaulting onto some other parameter.
        let map = descriptor(
            "lfo",
            &[
                ("control", Value::from("nonesuch")),
                ("depthabs", Value::F64(0.5)),
            ],
        );
        assert!(ModSpecs::from_controls(&map, &ctx(), |_| 25.0).is_empty());
        // ...and so does an empty control map.
        assert!(ModSpecs::from_controls(&ValueMap::new(), &ctx(), |_| 25.0).is_empty());
    }

    #[test]
    fn the_lfo_shape_names_index_the_waveshape_table() {
        // `shape_index` picks the entry in the `waveshapes` table; the order is
        // upstream's and a wrong index silently substitutes another waveform.
        for (name, want) in [("sine", 1), ("ramp", 2), ("saw", 3), ("square", 4)] {
            assert_eq!(
                shape_index(Some(&Value::from(name))),
                want,
                "shape {name:?}"
            );
        }
        // Triangle is index 0, which is also what anything unrecognised gets.
        for name in ["tri", "triangle", "nonesuch", ""] {
            assert_eq!(shape_index(Some(&Value::from(name))), 0, "shape {name:?}");
        }
        // A number is the index itself, wrapped into range so it can never
        // point outside the table.
        for (n, want) in [(0.0, 0), (1.0, 1), (4.0, 4), (5.0, 0), (7.0, 2), (-1.0, 4)] {
            assert_eq!(shape_index(Some(&Value::F64(n))), want, "numeric shape {n}");
        }
        // Nothing at all is a triangle.
        assert_eq!(shape_index(None), 0);
    }

    #[test]
    fn a_frequency_target_is_clamped_only_when_it_is_audio_rate() {
        // `getRangeForParam` clamps a frequency parameter to 20Hz..24kHz, but
        // only when the current value is already audio rate — a low value means
        // the parameter is itself an LFO and clamping it would pin it.
        assert_eq!(
            range_for(ModTarget::Frequency, 440.0),
            Some((20.0 - 440.0, 24000.0 - 440.0))
        );
        assert_eq!(
            range_for(ModTarget::Cutoff, 1000.0),
            Some((-980.0, 23000.0))
        );
        // The boundary is inclusive at 30.
        assert!(range_for(ModTarget::Frequency, 30.0).is_some());
        assert!(range_for(ModTarget::Frequency, 29.9).is_none());
        // A non-frequency parameter is never clamped, however large.
        for target in [
            ModTarget::Gain,
            ModTarget::Resonance,
            ModTarget::Crush,
            ModTarget::Postgain,
        ] {
            assert_eq!(range_for(target, 1000.0), None, "{target:?}");
        }
        // ...and the clamped ones are exactly the params superdough names
        // `frequency` (a buffer source's `detune` stands in for one).
        let clamped: Vec<_> = ModTarget::all()
            .into_iter()
            .filter(|t| range_for(*t, 440.0).is_some())
            .collect();
        let named_frequency: Vec<_> = crate::routing::tests_support::all_rows()
            .filter(|(_, param)| *param == "frequency")
            .filter_map(|(node, param)| ModTarget::of_node(node, param))
            .chain([ModTarget::Detune])
            .collect();
        for t in &clamped {
            assert!(named_frequency.contains(t), "{t:?} is clamped");
        }
        for t in &named_frequency {
            assert!(clamped.contains(t), "{t:?} is not clamped");
        }
    }

    #[test]
    fn descriptor_ids_are_keyed_the_way_javascript_writes_them() {
        // The ids come back as object keys, and JS renders a whole number
        // without a decimal point. Getting this wrong means the entry is looked
        // up under a name that is not there and the modulator vanishes.
        assert_eq!(id_key(&Value::Str("a".into())), "a");
        assert_eq!(id_key(&Value::Int(2)), "2");
        assert_eq!(id_key(&Value::F64(2.0)), "2");
        assert_eq!(id_key(&Value::F64(-3.0)), "-3");
        assert_eq!(id_key(&Value::F64(2.5)), "2.5");
    }

    #[test]
    fn a_static_modulator_is_recognised_and_still_applied() {
        // An LFO with no movement is a constant offset; `from_controls` still
        // has to produce it, or `.lfo({rate: 0})` silently does nothing.
        let map = descriptor(
            "lfo",
            &[
                ("control", Value::from("gain")),
                ("depthabs", Value::F64(0.5)),
                ("rate", Value::F64(0.0)),
                ("dcoffset", Value::F64(0.0)),
            ],
        );
        let specs = ModSpecs::from_controls(&map, &ctx(), |_| 25.0);
        assert!(!specs.is_empty(), "a zero-rate LFO is still a modulator");

        let mut bank = ModBank::new(&specs.voice, 44100.0);
        let first = {
            bank.tick();
            bank.get(ModTarget::Gain)
        };
        for _ in 0..100 {
            bank.tick();
        }
        assert!(
            (bank.get(ModTarget::Gain) - first).abs() < 1e-6,
            "a zero-rate LFO should hold its value"
        );
    }

    #[test]
    fn sine_lfo_is_centered_and_bounded() {
        // a sine LFO (dcoffset -0.5, depth 1) oscillates in [-0.5, 0.5] around 0.
        let cfg = LfoConfig {
            shape: 1,
            frequency: 100.0,
            ..LfoConfig::default()
        };
        let mut lfo = Lfo::new(&cfg, 44100.0);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for _ in 0..1000 {
            let v = lfo.tick();
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo >= -0.5 - 1e-9, "min too low: {lo}");
        assert!(lo < -0.45, "min not reached: {lo}");
        assert!(hi <= 0.5 + 1e-9, "max too high: {hi}");
        assert!(hi > 0.45, "max not reached: {hi}");
    }

    #[test]
    fn an_lfo_that_does_not_retrigger_starts_at_the_cycle_clock() {
        // Cycle 2 at 0.5 cps is four seconds in.
        let map = descriptor(
            "lfo",
            &[
                ("control", Value::Str("gain".into())),
                ("rate", Value::F64(1.0)),
            ],
        );
        let ctx = ModContext {
            cps: 0.5,
            cycle: 2.0,
            note_seconds: 1.0,
        };
        let specs = ModSpecs::from_controls(&map, &ctx, |_| 1.0);
        let SourceConfig::Lfo(cfg) = &specs.voice[0].source else {
            panic!("expected an LFO");
        };
        assert_eq!(cfg.time, 4.0);
    }

    #[test]
    fn a_bus_modulator_multiplies_by_its_gain() {
        // `depthabs` 0.6 is a gain of 2, where `*` and `/` part ways.
        let map = descriptor(
            "bmod",
            &[
                ("control", Value::Str("gain".into())),
                ("bus", Value::Int(1)),
                ("depthabs", Value::F64(0.6)),
                ("dc", Value::F64(0.5)),
            ],
        );
        let specs = ModSpecs::from_controls(&map, &ModContext::default(), |_| 1.0);
        let mut bank = ModBank::new(&specs.voice, 44100.0);
        bank.set_bus_input(1, &[2.0], &[0.0]);
        bank.tick();
        // ((2 + 0) / 2 + 0.5) * 2: the bus sums to mono, then dc, then gain.
        assert!(
            (bank.get(ModTarget::Gain) - 3.0).abs() < 1e-6,
            "{}",
            bank.get(ModTarget::Gain)
        );
    }

    #[test]
    fn a_bus_modulator_offsets_scales_and_clamps_the_bus_signal() {
        // `connectBusModulator` builds (signal + dc) * depth/0.3 into the target
        // param. `depthabs` 0.3 makes that gain exactly 1, so the arithmetic is
        // readable.
        let entry: ValueMap = [
            ("control".to_string(), Value::Str("gain".into())),
            ("bus".to_string(), Value::Int(1)),
            ("depthabs".to_string(), Value::F64(0.3)),
            ("dc".to_string(), Value::F64(0.5)),
        ]
        .into_iter()
        .collect();
        let desc: ValueMap = [
            ("__ids".to_string(), Value::List(vec![Value::Int(0)])),
            ("0".to_string(), Value::Map(entry)),
        ]
        .into_iter()
        .collect();
        let map: ValueMap = [("bmod".to_string(), Value::Map(desc))]
            .into_iter()
            .collect();
        let specs = ModSpecs::from_controls(&map, &ModContext::default(), |_| 1.0);
        assert_eq!(specs.voice.len(), 1, "gain is a voice-side target");

        let mut bank = ModBank::new(&specs.voice, 44100.0);
        // The bus is stereo and sums to mono, so a hard-left signal reads half.
        bank.set_bus_input(1, &[0.0, 2.0, -2.0], &[0.0, 0.0, 0.0]);
        for expected in [0.5, 1.5, -0.5] {
            bank.tick();
            assert!((bank.get(ModTarget::Gain) - expected).abs() < 1e-6);
        }

        // Nothing writes bus 2, so a modulator pointed at it only ever sees the
        // dc offset — and reading past the supplied block is silence, not a
        // panic.
        let mut bank = ModBank::new(&specs.voice, 44100.0);
        bank.set_bus_input(2, &[9.0], &[9.0]);
        bank.tick();
        assert!((bank.get(ModTarget::Gain) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ramp_sweeps_up_then_resets() {
        // a ramp LFO with dcoffset 0 / depth 1 rises through 0..1 and resets.
        let cfg = LfoConfig {
            shape: 2,
            frequency: 4.0,
            dcoffset: 0.0,
            min: 0.0,
            max: 1.0,
            ..LfoConfig::default()
        };
        let mut lfo = Lfo::new(&cfg, 64.0); // 16 samples per cycle
        let vals: Vec<f64> = (0..20).map(|_| lfo.tick()).collect();
        assert!(vals[0].abs() < 1e-12, "starts at 0");
        assert!(
            vals.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "bounded 0..1"
        );
        // rises across the first cycle, then drops back near 0 after the wrap.
        assert!(vals[10] > vals[1], "rising within a cycle");
        assert!(vals[17] < vals[15], "resets after the period");
    }

    /// Run a resolved voice modulator for `n` samples and report the offsets it
    /// produced for `target`.
    fn offsets(
        map: &ValueMap,
        ctx: &ModContext,
        base: f32,
        target: ModTarget,
        n: usize,
    ) -> Vec<f32> {
        let specs = ModSpecs::from_controls(map, ctx, |_| base);
        let mut bank = ModBank::new(specs.for_owner(ModOwner::Voice), 1000.0);
        (0..n)
            .map(|_| {
                bank.tick();
                bank.get(target)
            })
            .collect()
    }

    fn span(values: &[f32]) -> (f32, f32) {
        values
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    }

    #[test]
    fn every_target_has_its_own_slot() {
        // `index` is the offset table's key: two targets sharing a slot would
        // have one modulator overwrite the other's value every sample.
        let mut slots: Vec<usize> = ModTarget::all().into_iter().map(ModTarget::index).collect();
        assert!(slots.iter().all(|&i| i < ModTarget::SLOTS));
        let n = slots.len();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), n, "two targets share a slot");
    }

    #[test]
    fn specs_are_split_by_who_consumes_them() {
        let empty = ModSpecs::default();
        assert!(empty.is_empty());
        assert!(empty.for_owner(ModOwner::Voice).is_empty());
        assert!(empty.for_owner(ModOwner::PostFx).is_empty());
        assert!(ModBank::new(&[], 1000.0).is_empty());

        // `cutoff` is the voice's, `crush` the post-fx chain's.
        let voice = descriptor("lfo", &[("control", Value::from("cutoff"))]);
        let post = descriptor("lfo", &[("control", Value::from("crush"))]);
        let voice = ModSpecs::from_controls(&voice, &ctx(), |_| 1000.0);
        let post = ModSpecs::from_controls(&post, &ctx(), |_| 1000.0);
        assert!(!voice.is_empty() && !post.is_empty());
        assert_eq!(voice.for_owner(ModOwner::Voice).len(), 1);
        assert!(voice.for_owner(ModOwner::PostFx).is_empty());
        assert_eq!(post.for_owner(ModOwner::PostFx).len(), 1);
        assert!(post.for_owner(ModOwner::Voice).is_empty());
        assert!(!ModBank::new(voice.for_owner(ModOwner::Voice), 1000.0).is_empty());
    }

    #[test]
    fn a_relative_depth_scales_against_the_control_it_targets() {
        // `depth * currentValue`, with `depthabs` overriding it outright. The
        // base is 100 and the depth 0.5, so the two are 50 and 0.5 apart —
        // adding or dividing them lands nowhere near either.
        let relative = descriptor(
            "lfo",
            &[
                ("control", Value::from("gain")),
                ("depth", Value::F64(0.5)),
                ("dcoffset", Value::F64(0.0)),
                ("rate", Value::F64(50.0)),
            ],
        );
        let (lo, hi) = span(&offsets(&relative, &ctx(), 100.0, ModTarget::Gain, 200));
        assert!((-0.01..5.0).contains(&lo), "low end was {lo}");
        assert!(
            (hi - 50.0).abs() < 2.0,
            "a depth of 0.5 * 100 should reach 50, got {hi}"
        );

        // An absolute depth ignores the base entirely.
        let absolute = descriptor(
            "lfo",
            &[
                ("control", Value::from("gain")),
                ("depthabs", Value::F64(4.0)),
                ("dcoffset", Value::F64(0.0)),
                ("rate", Value::F64(50.0)),
            ],
        );
        let (_, hi) = span(&offsets(&absolute, &ctx(), 100.0, ModTarget::Gain, 200));
        assert!((hi - 4.0).abs() < 0.2, "depthabs should win, got {hi}");
    }

    #[test]
    fn dcoffset_shifts_the_band_by_whole_depths() {
        // superdough: the band is `(dcoffset * depth, dcoffset * depth + depth)`,
        // so a dcoffset of 1 lifts a 0..4 swing to 4..8.
        let at = |dcoffset: f64| {
            let map = descriptor(
                "lfo",
                &[
                    ("control", Value::from("gain")),
                    ("depthabs", Value::F64(4.0)),
                    ("dcoffset", Value::F64(dcoffset)),
                    ("rate", Value::F64(50.0)),
                ],
            );
            span(&offsets(&map, &ctx(), 100.0, ModTarget::Gain, 200))
        };
        let (lo, hi) = at(0.0);
        assert!(
            lo.abs() < 0.2 && (hi - 4.0).abs() < 0.2,
            "0..4, got {lo}..{hi}"
        );
        let (lo, hi) = at(1.0);
        assert!(
            (lo - 4.0).abs() < 0.2 && (hi - 8.0).abs() < 0.2,
            "4..8, got {lo}..{hi}"
        );
    }

    #[test]
    fn sync_is_in_cycles_where_rate_is_in_hertz() {
        // `sync` multiplies by cps, so at cps 0.5 a sync of 2 is exactly the
        // same modulator as a rate of 1Hz.
        let ctx = ModContext {
            cps: 0.5,
            cycle: 0.0,
            note_seconds: 1.0,
        };
        let by = |key: &str, v: f64| {
            let map = descriptor(
                "lfo",
                &[
                    ("control", Value::from("gain")),
                    ("depthabs", Value::F64(1.0)),
                    (key, Value::F64(v)),
                ],
            );
            offsets(&map, &ctx, 1.0, ModTarget::Gain, 1000)
        };
        assert_eq!(by("sync", 2.0), by("rate", 1.0));
        assert_ne!(by("sync", 2.0), by("rate", 2.0));
    }

    #[test]
    fn an_lfo_locks_to_cycle_time_unless_it_retriggers() {
        // Phase comes from `cycle / cps` (seconds since the clock started), so
        // a 1Hz LFO half a cycle in at cps 0.5 is exactly one second in — back
        // at the phase it starts from.
        let map = |retrig: f64| {
            descriptor(
                "lfo",
                &[
                    ("control", Value::from("gain")),
                    ("depthabs", Value::F64(1.0)),
                    ("rate", Value::F64(1.0)),
                    ("retrig", Value::F64(retrig)),
                ],
            )
        };
        let at_cycle = |cycle: f64, retrig: f64| {
            let ctx = ModContext {
                cps: 0.5,
                cycle,
                note_seconds: 1.0,
            };
            offsets(&map(retrig), &ctx, 1.0, ModTarget::Gain, 8)
        };
        let restarted = at_cycle(0.5, 1.0);
        assert_eq!(
            at_cycle(0.0, 0.0),
            restarted,
            "cycle 0 is phase 0 either way"
        );
        assert_eq!(
            at_cycle(0.5, 0.0),
            restarted,
            "one second in is a whole period"
        );
        assert_ne!(at_cycle(0.25, 0.0), restarted, "a quarter cycle is not");
        // Retriggering ignores the clock entirely.
        assert_eq!(at_cycle(0.25, 1.0), restarted);
    }

    // --- the routing core ------------------------------------------------

    /// A descriptor map holding several modulators of one kind, keyed 0, 1, ...
    fn several(kind: &str, entries: &[&[(&str, Value)]]) -> ValueMap {
        let mut desc = ValueMap::new();
        desc.insert(
            "__ids".to_string(),
            Value::List((0..entries.len() as i64).map(Value::Int).collect()),
        );
        for (i, entry) in entries.iter().enumerate() {
            let e: ValueMap = entry
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            desc.insert(i.to_string(), Value::Map(e));
        }
        [(kind.to_string(), Value::Map(desc))].into_iter().collect()
    }

    fn run(specs: &[ModSpec], n: usize) -> Vec<f32> {
        let mut bank = ModBank::new(specs, 1000.0);
        (0..n)
            .map(|_| {
                bank.tick();
                bank.get(ModTarget::Cutoff)
            })
            .collect()
    }

    #[test]
    fn two_modulators_on_one_param_add() {
        // Web Audio sums every connection into an AudioParam.
        let lfo = |rate: f64| -> Vec<(&'static str, Value)> {
            vec![
                ("control", Value::from("cutoff")),
                ("rate", Value::F64(rate)),
                ("depthabs", Value::F64(10.0)),
            ]
        };
        let one = ModSpecs::from_controls(&several("lfo", &[&lfo(3.0)]), &ctx(), |_| 500.0);
        let other = ModSpecs::from_controls(&several("lfo", &[&lfo(7.0)]), &ctx(), |_| 500.0);
        let both =
            ModSpecs::from_controls(&several("lfo", &[&lfo(3.0), &lfo(7.0)]), &ctx(), |_| 500.0);
        let (a, b, ab) = (
            run(&one.voice, 300),
            run(&other.voice, 300),
            run(&both.voice, 300),
        );
        for i in 0..300 {
            assert!((ab[i] - (a[i] + b[i])).abs() < 1e-4, "sample {i}");
        }
    }

    #[test]
    fn a_modulator_can_drive_another_ones_rate() {
        // `s("saw").lfo().lpf(500).lfo({ s: 0.3 })` in Strudel's own docs: the
        // second LFO defaults to `lfo_0`, the first one's rate.
        let first = vec![
            ("control", Value::from("cutoff")),
            ("rate", Value::F64(2.0)),
        ];
        let second = vec![
            ("control", Value::from("lfo_0")),
            ("subControl", Value::from("rate")),
            ("rate", Value::F64(0.5)),
            ("depthabs", Value::F64(40.0)),
        ];
        let specs = ModSpecs::from_controls(&several("lfo", &[&first, &second]), &ctx(), |_| 500.0);
        assert_eq!(specs.voice.len(), 2, "both live with the cutoff");
        assert_eq!(
            specs.voice[1].target,
            ModTarget::Modulator(0, ModParam::Frequency)
        );
        let alone = ModSpecs::from_controls(&several("lfo", &[&first]), &ctx(), |_| 500.0);
        assert_ne!(run(&specs.voice, 600), run(&alone.voice, 600));
    }

    #[test]
    fn a_target_without_its_param_drops_every_modulator_after_it() {
        // `tremolo` has no `phase` param: `targetParams[0].value` throws, and
        // the rest of the hap's modulator setup never runs.
        let before = vec![("control", Value::from("cutoff"))];
        let broken = vec![("control", Value::from("tremolophase"))];
        let after = vec![("control", Value::from("gain"))];
        let specs =
            ModSpecs::from_controls(&several("lfo", &[&before, &broken, &after]), &ctx(), |_| {
                1.0
            });
        assert_eq!(specs.voice.len(), 1);
        assert_eq!(specs.voice[0].target, ModTarget::Cutoff);
        // An unknown control is only skipped.
        let unknown = vec![("control", Value::from("velocity"))];
        let specs = ModSpecs::from_controls(&several("lfo", &[&unknown, &after]), &ctx(), |_| 1.0);
        assert_eq!(specs.voice.len(), 1);
        assert_eq!(specs.voice[0].target, ModTarget::Gain);
    }

    #[test]
    fn an_fxi_sends_a_modulator_to_its_stage() {
        let to_stage = vec![("control", Value::from("cutoff")), ("fxi", Value::F64(1.0))];
        let specs = ModSpecs::from_controls(&several("lfo", &[&to_stage]), &ctx(), |_| 500.0);
        assert!(specs.voice.is_empty());
        assert_eq!(specs.for_owner(ModOwner::StageVoice(1)).len(), 1);
        let crush = vec![("control", Value::from("crush")), ("fxi", Value::F64(0.0))];
        let specs = ModSpecs::from_controls(&several("lfo", &[&crush]), &ctx(), |_| 4.0);
        assert_eq!(specs.for_owner(ModOwner::StagePost(0)).len(), 1);
    }

    #[test]
    fn lfo_params_latch_once_per_render_quantum() {
        // The worklet reads `parameters.frequency[0]`: a rate driven mid-block
        // only takes effect at the next 128-frame boundary.
        let cfg = LfoConfig {
            frequency: 1.0,
            ..LfoConfig::default()
        };
        let mut faster = ParamOffsets::default();
        faster.0[ModParam::Frequency.index()] = 50.0;
        // A mid-quantum change waits for the boundary.
        let mut lfo = Lfo::new(&cfg, 1000.0);
        let mut reference = Lfo::new(&cfg, 1000.0);
        for i in 0..128 {
            let inputs = if i >= 64 {
                faster
            } else {
                ParamOffsets::default()
            };
            assert_eq!(lfo.tick_with(&inputs), reference.tick(), "frame {i}");
        }
        // At the boundary the new rate is latched: the sample after it moves.
        assert_eq!(lfo.tick_with(&faster), reference.tick());
        assert_ne!(lfo.tick_with(&faster), reference.tick(), "the next quantum");
    }

    #[test]
    fn a_shape_off_the_table_silences_the_lfo() {
        // `waveShapeNames[2.5]` is undefined, and calling `waveshapes[undefined]`
        // throws inside the worklet, which outputs nothing from then on.
        let cfg = LfoConfig {
            depth: 1.0,
            dcoffset: 0.0,
            min: -1.0,
            max: 1.0,
            ..LfoConfig::default()
        };
        let mut lfo = Lfo::new(&cfg, 1000.0);
        let mut half = ParamOffsets::default();
        half.0[ModParam::Shape.index()] = 0.5;
        assert_eq!(lfo.tick_with(&half), 0.0);
        for _ in 0..500 {
            assert_eq!(lfo.tick(), 0.0, "it stays silent");
        }
        let mut whole = ParamOffsets::default();
        whole.0[ModParam::Shape.index()] = 1.0;
        let mut sine = Lfo::new(&cfg, 1000.0);
        let mut moved = false;
        for _ in 0..200 {
            moved |= sine.tick_with(&whole) != 0.0;
        }
        assert!(moved, "a whole step to another shape still plays");
    }
}
