// modulator.rs - LFO modulation source. Ported from the `lfo-processor`
// AudioWorklet in strudel/packages/superdough/worklets.mjs (waveshapes + the
// per-sample process loop) and the `getLfo` defaults in helpers.mjs. This is the
// deterministic modulation-source core of superdough's modulator engine
// (modulate/lfo/env/bmod); the per-voice control-target routing that connects a
// source to a node's AudioParam is a Web Audio graph concern tracked separately.
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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub struct Lfo {
    phase: f64,
    dt: f64,
    shape: usize,
    skew: f64,
    depth: f64,
    dcoffset: f64,
    curve: f64,
    min: f64,
    max: f64,
}

impl Lfo {
    pub fn new(cfg: &LfoConfig, sample_rate: f64) -> Lfo {
        // `ffrac(time * frequency + phaseoffset)`; phase stays non-negative.
        let init = cfg.time * cfg.frequency + cfg.phaseoffset;
        Lfo {
            phase: init - init.trunc(),
            dt: cfg.frequency / sample_rate,
            shape: cfg.shape,
            skew: cfg.skew,
            depth: cfg.depth,
            dcoffset: cfg.dcoffset,
            curve: cfg.curve,
            min: cfg.min,
            max: cfg.max,
        }
    }

    /// The next modulation value.
    pub fn tick(&mut self) -> f64 {
        let mut modval =
            (waveshape(self.shape, self.phase, self.skew) + self.dcoffset) * self.depth;
        modval = modval.powf(self.curve);
        // JS `clamp` is min(max(v,min),max), which (unlike f64::clamp) does not
        // assume min <= max and never panics.
        let out = modval.max(self.min).min(self.max);
        self.phase += self.dt;
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Configuration for a [`ModEnv`], mirroring the `envelope-processor`
/// parameter descriptors.
#[derive(Clone, Debug)]
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
            cfg: cfg.clone(),
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
    fn segments(&self) -> [EnvSeg; 5] {
        let c = &self.cfg;
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
        let segs = self.segments();
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
        let out = (self.val * self.cfg.depth)
            .max(self.cfg.min)
            .min(self.cfg.max);
        self.t += self.dt;
        out
    }
}

// ---------------------------------------------------------------------------
// Routing: binding a modulation source to a voice parameter.
//
// superdough connects a modulator node to a target `AudioParam`, and Web Audio
// *sums* a connection into the param's intrinsic value. So a modulator is an
// additive offset on top of the control's own value, which is exactly how the
// bank below is consumed. `modulators.mjs`'s `CONTROL_TARGETS` names the param
// per control; the subset here is the one Rudel's scalar DSP can vary per
// sample.

/// A control a modulator can be routed to.
///
/// Strudel's table (`superdoughdata.mjs`) covers every node parameter in the
/// Web Audio graph. Rudel bakes most controls into a voice at construction, so
/// only the parameters it already recomputes per sample can be modulated; the
/// rest are listed in `docs/UNSUPPORTED.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModTarget {
    /// Oscillator frequency (`s`/`freq`/`note` -> `source.frequency`).
    Frequency,
    /// Voice gain (`gain`).
    Gain,
    /// Low-pass cutoff (`cutoff`) and resonance (`resonance`).
    Cutoff,
    Resonance,
    /// High-pass cutoff (`hcutoff`) and resonance (`hresonance`).
    Hcutoff,
    Hresonance,
    /// Band-pass centre (`bandf`) and Q (`bandq`).
    Bandf,
    Bandq,
    /// Post-fx amounts.
    Postgain,
    Shape,
    Shapevol,
    Distort,
    Distortvol,
    Crush,
    Coarse,
}

/// Which part of the signal chain applies a target, so each can own (and tick)
/// only the modulators it is able to consume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModOwner {
    /// The synth/sampler voice: oscillator frequency, gain and the filters.
    Voice,
    /// The post-effect chain.
    PostFx,
}

impl ModTarget {
    /// Every target, in declaration order; also the offset-table layout.
    const ALL: [ModTarget; 15] = [
        ModTarget::Frequency,
        ModTarget::Gain,
        ModTarget::Cutoff,
        ModTarget::Resonance,
        ModTarget::Hcutoff,
        ModTarget::Hresonance,
        ModTarget::Bandf,
        ModTarget::Bandq,
        ModTarget::Postgain,
        ModTarget::Shape,
        ModTarget::Shapevol,
        ModTarget::Distort,
        ModTarget::Distortvol,
        ModTarget::Crush,
        ModTarget::Coarse,
    ];

    /// Resolve a canonical control name (the descriptor's `control` value is
    /// already run through `getControlName`). `None` for controls Rudel cannot
    /// modulate.
    pub fn from_control(name: &str) -> Option<ModTarget> {
        Some(match name {
            "s" | "freq" | "note" => ModTarget::Frequency,
            "gain" => ModTarget::Gain,
            "cutoff" => ModTarget::Cutoff,
            "resonance" => ModTarget::Resonance,
            "hcutoff" => ModTarget::Hcutoff,
            "hresonance" => ModTarget::Hresonance,
            "bandf" => ModTarget::Bandf,
            "bandq" => ModTarget::Bandq,
            "postgain" => ModTarget::Postgain,
            "shape" => ModTarget::Shape,
            "shapevol" => ModTarget::Shapevol,
            "distort" => ModTarget::Distort,
            "distortvol" => ModTarget::Distortvol,
            "crush" => ModTarget::Crush,
            "coarse" => ModTarget::Coarse,
            _ => return None,
        })
    }

    fn owner(self) -> ModOwner {
        match self {
            ModTarget::Frequency
            | ModTarget::Gain
            | ModTarget::Cutoff
            | ModTarget::Resonance
            | ModTarget::Hcutoff
            | ModTarget::Hresonance
            | ModTarget::Bandf
            | ModTarget::Bandq => ModOwner::Voice,
            _ => ModOwner::PostFx,
        }
    }

    /// True when the underlying Web Audio param is a `frequency`, which is what
    /// `getRangeForParam` keys its 20Hz..24kHz clamp off.
    fn is_frequency(self) -> bool {
        matches!(
            self,
            ModTarget::Frequency | ModTarget::Cutoff | ModTarget::Hcutoff | ModTarget::Bandf
        )
    }

    fn index(self) -> usize {
        ModTarget::ALL.iter().position(|&t| t == self).unwrap()
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

/// A resolved but not-yet-running modulation source.
#[derive(Clone, Debug)]
enum SourceConfig {
    Lfo(LfoConfig),
    Env(EnvConfig),
}

/// One resolved modulator: the control it offsets plus its source config.
///
/// Sample-rate free, so the scheduler can resolve it from the hap while the
/// mixer instantiates it at the device rate ([`ModBank::new`]).
#[derive(Clone, Debug)]
pub struct ModSpec {
    target: ModTarget,
    source: SourceConfig,
}

/// The modulators a hap carries, split by which part of the chain can consume
/// them so each side ticks only its own.
#[derive(Clone, Debug, Default)]
pub struct ModSpecs {
    pub voice: Vec<ModSpec>,
    pub post: Vec<ModSpec>,
}

impl ModSpecs {
    pub fn is_empty(&self) -> bool {
        self.voice.is_empty() && self.post.is_empty()
    }

    /// The specs for one owner.
    pub fn for_owner(&self, owner: ModOwner) -> &[ModSpec] {
        match owner {
            ModOwner::Voice => &self.voice,
            ModOwner::PostFx => &self.post,
        }
    }
}

/// A live modulation source bound to a target control.
#[derive(Clone, Debug)]
enum ModSource {
    Lfo(Lfo),
    Env(ModEnv),
}

/// One running modulator.
#[derive(Clone, Debug)]
struct Modulation {
    target: ModTarget,
    source: ModSource,
}

impl Modulation {
    fn tick(&mut self) -> f64 {
        match &mut self.source {
            ModSource::Lfo(l) => l.tick(),
            ModSource::Env(e) => e.tick(),
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
#[derive(Clone, Debug, Default)]
pub struct ModBank {
    mods: Vec<Modulation>,
    offsets: [f32; ModTarget::ALL.len()],
}

impl ModBank {
    /// True when nothing is modulated, so the whole stage can be skipped.
    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
    }

    /// Advance every source by one sample.
    pub fn tick(&mut self) {
        for m in &mut self.mods {
            self.offsets[m.target.index()] = m.tick() as f32;
        }
    }

    /// The current additive offset for `target` (0.0 when unmodulated).
    pub fn get(&self, target: ModTarget) -> f32 {
        self.offsets[target.index()]
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
                    },
                })
                .collect(),
            offsets: [0.0; ModTarget::ALL.len()],
        }
    }
}

impl ModSpecs {
    /// Resolve a hap's `lfo`/`env` descriptors.
    ///
    /// `base` supplies the target control's own value, which relative `depth`
    /// scales (superdough reads it off the target `AudioParam`). Entries naming
    /// a control Rudel cannot modulate are skipped — upstream logs "may not be
    /// modulatable" and carries on, and so does this.
    pub fn from_controls(
        map: &ValueMap,
        ctx: &ModContext,
        base: impl Fn(ModTarget) -> f32,
    ) -> ModSpecs {
        let mut out = ModSpecs::default();
        for (kind, is_lfo) in [("lfo", true), ("env", false)] {
            let Some(Value::Map(desc)) = map.get(kind) else {
                continue;
            };
            let Some(Value::List(ids)) = desc.get("__ids") else {
                continue;
            };
            for id in ids {
                let key = id_key(id);
                let Some(Value::Map(entry)) = desc.get(&key) else {
                    continue;
                };
                let Some(target) = entry
                    .get("control")
                    .and_then(|v| v.as_str())
                    .and_then(ModTarget::from_control)
                else {
                    continue;
                };
                let get = |k: &str| entry.get(k).and_then(|v| v.as_f64());
                // `currentValue === 0 ? 1 : currentValue`, then
                // `depthabs ?? depth * currentValue`.
                let current = match base(target) as f64 {
                    0.0 => 1.0,
                    v => v,
                };
                let depth = get("depthabs").unwrap_or(get("depth").unwrap_or(1.0) * current);
                let range = range_for(target, current);
                let source = if is_lfo {
                    let d = LfoConfig::default();
                    let dcoffset = get("dcoffset").unwrap_or(d.dcoffset);
                    let (min, max) = range.unwrap_or((dcoffset * depth, dcoffset * depth + depth));
                    let retrig = get("retrig").unwrap_or(0.0);
                    let cfg = LfoConfig {
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
                        // Unless it retriggers, the phase is locked to the
                        // global cycle clock rather than the note onset.
                        time: if retrig > 0.5 {
                            0.0
                        } else {
                            ctx.cycle / ctx.cps.max(1e-9)
                        },
                        min,
                        max,
                    };
                    SourceConfig::Lfo(cfg)
                } else {
                    let d = EnvConfig::default();
                    let (min, max) = range.unwrap_or((d.min, d.max));
                    let cfg = EnvConfig {
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
                    };
                    SourceConfig::Env(cfg)
                };
                let spec = ModSpec { target, source };
                match target.owner() {
                    ModOwner::Voice => out.voice.push(spec),
                    ModOwner::PostFx => out.post.push(spec),
                }
            }
        }
        out
    }
}

/// superdough's `getModulationShapeInput`: a number indexes the waveshape table
/// (mod 5), a name looks it up, anything else is the triangle.
fn shape_index(v: Option<&Value>) -> usize {
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
        Value::F64(n) if n.fract() == 0.0 => (*n as i64).to_string(),
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
