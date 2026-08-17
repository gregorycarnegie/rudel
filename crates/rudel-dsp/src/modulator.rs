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

/// A live modulation source bound to a target control.
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
}

impl Modulation {
    fn tick(&mut self) -> f64 {
        match &mut self.source {
            ModSource::Lfo(l) => l.tick(),
            ModSource::Env(e) => e.tick(),
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
                        SourceConfig::Bus(c) => ModSource::Bus(BusMod {
                            cfg: *c,
                            input: Vec::new(),
                            pos: 0,
                        }),
                    },
                })
                .collect(),
            offsets: [0.0; ModTarget::ALL.len()],
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

impl ModSpecs {
    /// Resolve a hap's `lfo`/`env`/`bmod` descriptors.
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
        for kind in ["lfo", "env", "bmod"] {
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
                let source = if kind == "lfo" {
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
                } else if kind == "bmod" {
                    // A `bmod` with no bus reads `getBus(undefined)` upstream,
                    // which nothing ever sends to; skipping is the same silence.
                    let Some(bus) = get("bus") else { continue };
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
        Value::F64(n) if n.fract() == 0.0 => (*n as i64).to_string(),
        other => other.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the name and descriptor tables ------------------------------------
    //
    // The 2026-08 mutation run left 80 of modulator.rs's 234 mutants alive, and
    // the two biggest clusters were lookup tables: `ModTarget::from_control`
    // (13) and `ModSpecs::from_controls` (13). Both sit between a pattern's
    // controls and the DSP, so a wrong arm does not error — it modulates
    // something else, or nothing.

    /// A one-entry modulator descriptor in the nested-map shape Koto hands over.
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

    #[test]
    fn every_modulatable_control_names_its_own_target() {
        for (name, want) in [
            ("gain", ModTarget::Gain),
            ("cutoff", ModTarget::Cutoff),
            ("resonance", ModTarget::Resonance),
            ("hcutoff", ModTarget::Hcutoff),
            ("hresonance", ModTarget::Hresonance),
            ("bandf", ModTarget::Bandf),
            ("bandq", ModTarget::Bandq),
            ("postgain", ModTarget::Postgain),
            ("shape", ModTarget::Shape),
            ("shapevol", ModTarget::Shapevol),
            ("distort", ModTarget::Distort),
            ("distortvol", ModTarget::Distortvol),
            ("crush", ModTarget::Crush),
            ("coarse", ModTarget::Coarse),
        ] {
            assert_eq!(
                ModTarget::from_control(name),
                Some(want),
                "control {name:?} should modulate {want:?}"
            );
        }

        // Pitch has three spellings, all the same target.
        for name in ["s", "freq", "note"] {
            assert_eq!(ModTarget::from_control(name), Some(ModTarget::Frequency));
        }

        // Names are exact: no case folding, no prefixes.
        for name in ["", "Gain", "gains", "gai", "cut", "lpf", "nonesuch"] {
            assert_eq!(
                ModTarget::from_control(name),
                None,
                "{name:?} is not a modulation target"
            );
        }
    }

    #[test]
    fn no_two_controls_share_a_target_and_none_is_unreachable() {
        // Guards the table against a swapped pair, which leaves every target
        // reachable and so slips past the list above if one is ever edited to
        // match the code rather than the intent.
        let names = [
            "s",
            "freq",
            "note",
            "gain",
            "cutoff",
            "resonance",
            "hcutoff",
            "hresonance",
            "bandf",
            "bandq",
            "postgain",
            "shape",
            "shapevol",
            "distort",
            "distortvol",
            "crush",
            "coarse",
        ];
        let mut seen: Vec<ModTarget> = Vec::new();
        for name in names {
            let t = ModTarget::from_control(name).expect("a target");
            seen.push(t);
        }
        for target in ModTarget::ALL {
            assert!(
                seen.contains(&target),
                "{target:?} cannot be reached by any control name"
            );
        }
        // Every target appears, and only pitch appears more than once.
        let mut unique = seen.clone();
        unique.sort_by_key(|t| format!("{t:?}"));
        unique.dedup();
        assert_eq!(unique.len(), ModTarget::ALL.len());
        assert_eq!(
            seen.iter().filter(|t| **t == ModTarget::Frequency).count(),
            3,
            "only the pitch control has aliases"
        );
    }

    #[test]
    fn voice_and_post_fx_modulators_are_kept_apart() {
        // A modulator has to run in the stage that owns its parameter; landing
        // in the wrong bank means it is ticked at the wrong point in the chain.
        for (name, voice_side) in [
            ("freq", true),
            ("gain", true),
            ("cutoff", true),
            ("resonance", true),
            ("hcutoff", true),
            ("hresonance", true),
            ("bandf", true),
            ("bandq", true),
            ("postgain", false),
            ("shape", false),
            ("distort", false),
            ("crush", false),
            ("coarse", false),
        ] {
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

        // A control that cannot be modulated yields nothing rather than
        // defaulting onto some other parameter.
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
        // ...and the frequency-like ones are exactly the four.
        let clamped: Vec<_> = ModTarget::ALL
            .into_iter()
            .filter(|t| range_for(*t, 440.0).is_some())
            .collect();
        assert_eq!(
            clamped,
            vec![
                ModTarget::Frequency,
                ModTarget::Cutoff,
                ModTarget::Hcutoff,
                ModTarget::Bandf
            ]
        );
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
        for (i, &target) in ModTarget::ALL.iter().enumerate() {
            assert_eq!(target.index(), i, "{target:?}");
        }
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
}
