// hap_graph.rs - one hap's audio graph, as superdough's modulators see it.
//
// `getTargetParamsForControl` looks a node up in `nodes[fxi]`, which
// superdough.mjs fills while building the hap's chain: a node is there only
// when its effect is active (`lpf` once `cutoff` is set, `pan` once `pan` is,
// the delay on the main chain only with `delay`, `delaytime` and
// `delayfeedback` all positive, ...), and holds the params its Web Audio node
// or worklet declares. This answers the same question from rudel's resolved
// voice, with each param's value at setup for a relative `depth`.
// docs/MODULATION_TARGETS.md is the table this follows.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    bus::OrbitSend,
    filter::{FilterModel, FilterParams, FilterSet},
    modulator::{CompParam, FilterSlot, LfoConfig, ModParam, ModTarget},
    params::VoiceParams,
    postfx::PostFx,
    routing::{Fxi, Lookup, ModGraph},
    spec::{FxStage, VoiceSpec},
};
use rudel_core::ValueMap;

/// The parts of a hap that decide its graph.
pub struct HapGraph<'a> {
    pub spec: &'a VoiceSpec,
    pub fx: &'a PostFx,
    pub send: &'a OrbitSend,
    /// The hap's own controls (presence decides some nodes).
    pub controls: &'a ValueMap,
    /// The `FX(...)` stages and their control maps.
    pub stages: &'a [FxStage],
    pub stage_maps: &'a [ValueMap],
}

fn found(target: ModTarget, current: f64) -> Lookup {
    Lookup::Found(target, current)
}

/// A param of an LFO worklet node, or the throw when it has none (including
/// the `[undefined]` an absent LFO leaves under its key).
fn lfo_node(
    lfo: Option<&LfoConfig>,
    param: &str,
    target: impl Fn(ModParam) -> ModTarget,
) -> Lookup {
    let (Some(cfg), Some(p)) = (lfo, ModParam::lfo_param(param)) else {
        return Lookup::NoParam;
    };
    let current = match p {
        ModParam::Frequency => cfg.frequency,
        ModParam::Depth => cfg.depth,
        ModParam::Skew => cfg.skew,
        ModParam::Curve => cfg.curve,
        ModParam::Dcoffset => cfg.dcoffset,
        ModParam::Shape => cfg.shape as f64,
        _ => return Lookup::NoParam,
    };
    found(target(p), current)
}

/// A filter (`lpf`/`hpf`/`bpf`) or its LFO, present once its frequency is set.
fn filter_node(fp: &FilterParams, slot: FilterSlot, lfo: bool, param: &str) -> Lookup {
    let Some(freq) = fp.freq else {
        return Lookup::Absent;
    };
    if lfo {
        return lfo_node(fp.lfo.as_ref(), param, |p| ModTarget::FilterLfo(slot, p));
    }
    let (frequency, q) = match slot {
        FilterSlot::Low => (ModTarget::Cutoff, ModTarget::Resonance),
        FilterSlot::High => (ModTarget::Hcutoff, ModTarget::Hresonance),
        FilterSlot::Band => (ModTarget::Bandf, ModTarget::Bandq),
    };
    match (param, fp.model) {
        ("frequency", _) => found(frequency, freq as f64),
        // The ladder worklet's resonance param is `q`, not `Q`.
        ("Q", FilterModel::Ladder) => Lookup::NoParam,
        ("Q", _) => found(q, fp.q as f64),
        _ => Lookup::NoParam,
    }
}

/// The insert effects a chain builds from one control map: the filters and the
/// post-effect rack. Both the main chain and every `FX` stage have these.
fn insert_node(
    filters: &FilterSet,
    fx: &PostFx,
    map: &ValueMap,
    node: &str,
    param: &str,
) -> Option<Lookup> {
    let has = |k: &str| map.get(k).is_some();
    Some(match node {
        "lpf" => filter_node(&filters.lp, FilterSlot::Low, false, param),
        "hpf" => filter_node(&filters.hp, FilterSlot::High, false, param),
        "bpf" => filter_node(&filters.bp, FilterSlot::Band, false, param),
        "lpf_lfo" => filter_node(&filters.lp, FilterSlot::Low, true, param),
        "hpf_lfo" => filter_node(&filters.hp, FilterSlot::High, true, param),
        "bpf_lfo" => filter_node(&filters.bp, FilterSlot::Band, true, param),
        "vowel" => match fx.vowel {
            None => Lookup::Absent,
            Some(v) if param == "frequency" => found(ModTarget::Vowel, v.formants()[0].0 as f64),
            Some(_) => Lookup::NoParam,
        },
        "coarse" => match (fx.coarse, param) {
            (None, _) => Lookup::Absent,
            (Some(c), "coarse") => found(ModTarget::Coarse, c as f64),
            _ => Lookup::NoParam,
        },
        "crush" => match (fx.crush, param) {
            (None, _) => Lookup::Absent,
            (Some(c), "crush") => found(ModTarget::Crush, c as f64),
            _ => Lookup::NoParam,
        },
        "shape" => match (fx.shape, param) {
            (None, _) => Lookup::Absent,
            (Some(s), "shape") => found(ModTarget::Shape, s as f64),
            (Some(_), "postgain") => found(ModTarget::Shapevol, fx.shapevol as f64),
            _ => Lookup::NoParam,
        },
        "distort" => match (fx.distort, param) {
            (None, _) => Lookup::Absent,
            (Some(d), "distort") => found(ModTarget::Distort, d as f64),
            (Some(_), "postgain") => found(ModTarget::Distortvol, fx.distortvol as f64),
            _ => Lookup::NoParam,
        },
        "tremolo" => match &fx.tremolo_lfo() {
            None => Lookup::Absent,
            Some(cfg) => lfo_node(Some(cfg), param, ModTarget::Tremolo),
        },
        "tremolo_gain" => match (fx.tremolo_lfo(), param) {
            (None, _) => Lookup::Absent,
            (Some(_), "gain") => found(
                ModTarget::TremoloGain,
                (1.0 - fx.tremolodepth as f64).max(0.0),
            ),
            _ => Lookup::NoParam,
        },
        "compressor" => match fx.compressor {
            None => Lookup::Absent,
            Some(threshold) => {
                let (c, v) = match param {
                    "threshold" => (CompParam::Threshold, threshold),
                    "ratio" => (CompParam::Ratio, fx.comp_ratio),
                    "knee" => (CompParam::Knee, fx.comp_knee),
                    "attack" => (CompParam::Attack, fx.comp_attack),
                    "release" => (CompParam::Release, fx.comp_release),
                    _ => return Some(Lookup::NoParam),
                };
                found(ModTarget::Compressor(c), v as f64)
            }
        },
        "pan" if has("pan") => match param {
            "pan" => {
                let pan = map.get("pan").and_then(|v| v.as_f64()).unwrap_or(0.5);
                found(ModTarget::Pan, 2.0 * pan - 1.0)
            }
            _ => Lookup::NoParam,
        },
        "phaser" | "phaser_lfo" => match fx.phaser {
            // `fx.phaserrate !== undefined && phaserdepth > 0`
            Some(rate) if fx.phaserdepth > 0.0 => match (node, param) {
                ("phaser", "frequency") => {
                    found(ModTarget::PhaserCenter, fx.phasercenter as f64 + 282.0)
                }
                ("phaser", "Q") => found(
                    ModTarget::PhaserQ,
                    2.0 - (fx.phaserdepth as f64 * 2.0).clamp(0.0, 1.9),
                ),
                ("phaser_lfo", "frequency") => {
                    found(ModTarget::PhaserLfo(ModParam::Frequency), rate as f64)
                }
                ("phaser_lfo", "depth") => found(
                    ModTarget::PhaserLfo(ModParam::Depth),
                    fx.phasersweep as f64 * 2.0,
                ),
                ("phaser_lfo", p) if ModParam::lfo_param(p).is_some() => {
                    Lookup::Found(ModTarget::PhaserLfo(ModParam::lfo_param(p)?), 0.0)
                }
                _ => Lookup::NoParam,
            },
            _ => Lookup::Absent,
        },
        "stretch" => match (fx.stretch, param) {
            (None, _) => Lookup::Absent,
            (Some(s), "pitchFactor") => found(ModTarget::Stretch, s as f64),
            _ => Lookup::NoParam,
        },
        // Registered as a bare node rather than a list, so `forEach` throws.
        "transient" if fx.transient.is_some() => Lookup::NoParam,
        _ => return None,
    })
}

impl HapGraph<'_> {
    /// The voice's source node: what kind of node it is decides its params.
    fn source(&self, param: &str) -> Lookup {
        match self.spec {
            VoiceSpec::Synth(p) => synth_source(p, param),
            // A sample, soundfont, ZZFX buffer or synthesized drum is an
            // `AudioBufferSourceNode`: no `frequency`, so `getNodeParam` falls
            // back to its `detune`, in cents, which starts at 0.
            VoiceSpec::Sampler(_) | VoiceSpec::Drum(_) | VoiceSpec::Zzfx(_) => match param {
                "frequency" => found(ModTarget::Detune, 0.0),
                _ => Lookup::NoParam,
            },
            VoiceSpec::ByteBeat(p) => match param {
                "frequency" => found(ModTarget::Frequency, p.freq as f64),
                _ => Lookup::NoParam,
            },
            // `s("bus")` is a bus's GainNode: no `frequency` and no fallback.
            VoiceSpec::Bus(_) => Lookup::NoParam,
        }
    }

    /// The source's own sub-nodes: vibrato, FM operators, the pulse width and
    /// wavetable LFOs.
    fn source_part(&self, node: &str, param: &str) -> Option<Lookup> {
        let synth = match self.spec {
            VoiceSpec::Synth(p) => Some(p.as_ref()),
            _ => None,
        };
        let vib = match self.spec {
            VoiceSpec::Synth(p) => p.vib.map(|r| (r, p.vibmod)),
            VoiceSpec::Sampler(p) => p.pitch.vib.map(|r| (r, p.pitch.vibmod)),
            _ => None,
        };
        Some(match node {
            "vib" | "vib_gain" => match vib {
                // `if (vib > 0)`
                Some((rate, depth)) if rate > 0.0 => match (node, param) {
                    ("vib", "frequency") => found(ModTarget::VibFreq, rate as f64),
                    ("vib_gain", "gain") => found(ModTarget::VibGain, depth as f64 * 100.0),
                    _ => Lookup::NoParam,
                },
                _ => Lookup::Absent,
            },
            "pw_lfo" => match synth {
                Some(p) if is_pulse(p) => lfo_node(p.pw_lfo.as_ref(), param, ModTarget::PwLfo),
                _ => Lookup::Absent,
            },
            "wt_lfo" | "warp_lfo" => match synth {
                Some(p) if p.wavetable.is_some() => {
                    let (lfo, target): (_, fn(ModParam) -> ModTarget) = if node == "wt_lfo" {
                        (p.wt.lfo.as_ref(), ModTarget::WtLfo)
                    } else {
                        (p.warp.lfo.as_ref(), ModTarget::WarpLfo)
                    };
                    lfo_node(lfo, param, target)
                }
                _ => Lookup::Absent,
            },
            fm if fm.starts_with("fm_") => {
                let Some(p) = synth else {
                    return Some(Lookup::Absent);
                };
                let rest = &fm[3..];
                let (n, gain) = match rest.strip_suffix("_gain") {
                    Some(n) => (n, true),
                    None => (rest, false),
                };
                let Some(n) = n.parse::<usize>().ok().filter(|n| (1..=8).contains(n)) else {
                    return Some(Lookup::Absent);
                };
                let fm = &p.fm;
                let used = (0..=8).any(|j| fm.amt[n][j] != 0.0 || fm.amt[j][n] != 0.0);
                if !used {
                    return Some(Lookup::Absent);
                }
                match (gain, param) {
                    (true, "gain") => {
                        let amt = (0..=8)
                            .map(|j| fm.amt[n][j])
                            .find(|a| *a != 0.0)
                            .unwrap_or(0.0);
                        found(ModTarget::FmGain(n as u8), amt as f64)
                    }
                    (false, "frequency") => found(
                        ModTarget::FmFreq(n as u8),
                        (p.freq * fm.ops[n].ratio) as f64,
                    ),
                    _ => Lookup::NoParam,
                }
            }
            _ => return None,
        })
    }

    fn filters(&self) -> FilterSet {
        match self.spec {
            VoiceSpec::Synth(p) => FilterSet {
                lp: p.lp,
                hp: p.hp,
                bp: p.bp,
            },
            VoiceSpec::Sampler(p) => p.filters,
            VoiceSpec::Drum(p) => p.filters,
            VoiceSpec::Zzfx(p) => p.filters,
            VoiceSpec::ByteBeat(p) => p.filters,
            VoiceSpec::Bus(p) => p.filters,
        }
    }

    fn main(&self, node: &str, param: &str) -> Lookup {
        if let Some(l) = insert_node(&self.filters(), self.fx, self.controls, node, param) {
            return l;
        }
        if let Some(l) = self.source_part(node, param) {
            return l;
        }
        let send = self.send;
        let cfg = &send.delay_cfg;
        match (node, param) {
            ("source", p) => self.source(p),
            ("gain", "gain") => found(
                ModTarget::Gain,
                self.spec.mod_base(ModTarget::Gain, self.fx) as f64,
            ),
            ("post", "gain") => found(ModTarget::Postgain, self.fx.postgain as f64),
            ("gain" | "post", _) => Lookup::NoParam,
            // `delay > 0 && delaytime > 0 && delayfeedback > 0`
            ("delay" | "delay_mix", _)
                if !(send.delay > 0.0 && cfg.time > 0.0 && cfg.feedback > 0.0) =>
            {
                Lookup::Absent
            }
            ("delay_mix", "gain") => found(ModTarget::DelaySend, send.delay as f64),
            ("delay", "delayTime") => found(ModTarget::DelayTime, cfg.time as f64),
            ("delay", "feedback") => found(ModTarget::DelayFeedback, cfg.feedback as f64),
            ("delay" | "delay_mix", _) => Lookup::NoParam,
            ("room" | "room_mix", _) if send.room <= 0.0 => Lookup::Absent,
            ("room_mix", "gain") => found(ModTarget::RoomSend, send.room as f64),
            ("room" | "room_mix", _) => Lookup::NoParam,
            ("djf", "value") => match send.djf {
                Some(v) => found(ModTarget::Djf, v as f64),
                None => Lookup::Absent,
            },
            ("djf", _) if send.djf.is_some() => Lookup::NoParam,
            _ => Lookup::Absent,
        }
    }

    fn stage(&self, i: usize, node: &str, param: &str) -> Lookup {
        let (Some(stage), Some(map)) = (self.stages.get(i), self.stage_maps.get(i)) else {
            return Lookup::Absent;
        };
        if let Some(l) = insert_node(&stage.filters, &stage.fx, map, node, param) {
            return l;
        }
        match (node, param) {
            ("gain", "gain") => found(ModTarget::Gain, stage.gain as f64),
            ("gain", _) => Lookup::NoParam,
            _ => Lookup::Absent,
        }
    }
}

fn is_pulse(p: &VoiceParams) -> bool {
    p.waveform == crate::oscillator::Waveform::Pulse
        && p.additive.is_none()
        && p.noise.is_none()
        && !p.supersaw
        && p.wavetable.is_none()
}

/// A synth voice's `source` node.
fn synth_source(p: &VoiceParams, param: &str) -> Lookup {
    let freq = p.freq as f64;
    if p.noise.is_some() {
        // The noise synths play an `AudioBufferSourceNode`.
        return match param {
            "frequency" => found(ModTarget::Detune, 0.0),
            _ => Lookup::NoParam,
        };
    }
    if p.wavetable.is_some() {
        return match param {
            "frequency" => found(ModTarget::Frequency, freq),
            "freqspread" => found(ModTarget::Freqspread, p.freqspread as f64),
            "panspread" => found(ModTarget::Panspread, p.panspread as f64),
            "position" => found(ModTarget::WtPosition, p.wt.offset as f64),
            "warp" => found(ModTarget::WtWarp, p.warp.offset as f64),
            _ => Lookup::NoParam,
        };
    }
    if p.supersaw {
        return match param {
            "frequency" => found(ModTarget::Frequency, freq),
            "freqspread" => found(ModTarget::Freqspread, p.freqspread as f64),
            "panspread" => found(ModTarget::Panspread, p.panspread as f64),
            _ => Lookup::NoParam,
        };
    }
    if is_pulse(p) {
        return match param {
            "frequency" => found(ModTarget::Frequency, freq),
            "pulsewidth" => found(ModTarget::Pulsewidth, p.pw as f64),
            _ => Lookup::NoParam,
        };
    }
    // An `OscillatorNode`, or the PeriodicWave one `partials` builds.
    match param {
        "frequency" => found(ModTarget::Frequency, freq),
        _ => Lookup::NoParam,
    }
}

impl ModGraph for HapGraph<'_> {
    fn lookup(&self, fxi: Fxi, node: &str, param: &str) -> Lookup {
        match fxi {
            Fxi::Main => self.main(node, param),
            Fxi::Stage(i) => self.stage(i, node, param),
        }
    }
}
