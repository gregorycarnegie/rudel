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
        let sends = &stage.sends;
        match (node, param) {
            ("gain", "gain") => found(ModTarget::Gain, stage.gain as f64),
            ("gain", _) => Lookup::NoParam,
            // The stage's own delay and reverb, there on the same terms.
            ("delay" | "delay_mix", _) if sends.delay.is_none() => Lookup::Absent,
            ("delay_mix", "gain") => found(
                ModTarget::DelaySend,
                sends.delay.map_or(0.0, |d| d.0) as f64,
            ),
            ("delay", "delayTime") => found(
                ModTarget::DelayTime,
                sends.delay.map_or(0.0, |d| d.1.time) as f64,
            ),
            ("delay", "feedback") => found(
                ModTarget::DelayFeedback,
                sends.delay.map_or(0.0, |d| d.1.feedback) as f64,
            ),
            ("delay" | "delay_mix", _) => Lookup::NoParam,
            ("room" | "room_mix", _) => match &sends.room {
                None => Lookup::Absent,
                Some(room) if (node, param) == ("room_mix", "gain") => {
                    found(ModTarget::RoomSend, room.wet as f64)
                }
                Some(_) => Lookup::NoParam,
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bus::{BusParams, DelayConfig},
        bytebeat::ByteBeatParams,
        drum::{DrumKind, DrumParams},
        oscillator::{NoiseKind, Waveform},
        postfx::Vowel,
        sampler::{Sample, SamplerParams},
        stage_sends::StageSends,
        wavetable::{ParamMod, WaveTable},
        zzfx::ZzfxParams,
    };
    use CompParam as C;
    use Lookup::{Absent, NoParam};
    use ModParam as P;
    use rstest::rstest;
    use rudel_core::Value;
    use std::sync::Arc;

    /// An LFO whose six params all differ, so a swapped arm reads wrong.
    fn lfo(frequency: f64) -> LfoConfig {
        LfoConfig {
            frequency,
            depth: 2.5,
            skew: 0.25,
            curve: 3.5,
            dcoffset: 0.125,
            shape: 4,
            ..LfoConfig::default()
        }
    }

    struct G {
        spec: VoiceSpec,
        fx: PostFx,
        send: OrbitSend,
        controls: ValueMap,
        stages: Vec<FxStage>,
        maps: Vec<ValueMap>,
    }

    impl G {
        fn new(spec: VoiceSpec) -> G {
            G {
                spec,
                fx: PostFx::default(),
                send: OrbitSend::default(),
                controls: ValueMap::default(),
                stages: Vec::new(),
                maps: Vec::new(),
            }
        }
        fn synth(p: VoiceParams) -> G {
            G::new(VoiceSpec::Synth(Box::new(p)))
        }
        fn graph(&self) -> HapGraph<'_> {
            HapGraph {
                spec: &self.spec,
                fx: &self.fx,
                send: &self.send,
                controls: &self.controls,
                stages: &self.stages,
                stage_maps: &self.maps,
            }
        }
        fn main(&self, node: &str, param: &str) -> Lookup {
            self.graph().lookup(Fxi::Main, node, param)
        }
        fn stage(&self, i: usize, node: &str, param: &str) -> Lookup {
            self.graph().lookup(Fxi::Stage(i), node, param)
        }
    }

    fn sampler() -> SamplerParams {
        SamplerParams::new(Arc::new(Sample {
            data: vec![0.0; 4],
            sample_rate: 44100.0,
        }))
    }

    fn pulse() -> VoiceParams {
        VoiceParams {
            waveform: Waveform::Pulse,
            pw_lfo: Some(lfo(1.5)),
            ..VoiceParams::default()
        }
    }

    fn wavetable() -> Option<WaveTable> {
        Some(WaveTable::from_samples(&[0.0; 512], 256))
    }

    fn f(v: f32) -> f64 {
        v as f64
    }

    #[rstest]
    #[case("frequency", P::Frequency, 1.5)]
    #[case("depth", P::Depth, 2.5)]
    #[case("skew", P::Skew, 0.25)]
    #[case("curve", P::Curve, 3.5)]
    #[case("dcoffset", P::Dcoffset, 0.125)]
    #[case("shape", P::Shape, 4.0)]
    fn an_lfo_node_reads_each_of_its_params(
        #[case] name: &str,
        #[case] p: ModParam,
        #[case] v: f64,
    ) {
        assert_eq!(
            G::synth(pulse()).main("pw_lfo", name),
            found(ModTarget::PwLfo(p), v)
        );
    }

    #[test]
    fn an_lfo_node_without_the_param_or_the_lfo_has_none() {
        assert_eq!(G::synth(pulse()).main("pw_lfo", "phaseoffset"), NoParam);
        // A pulse without `pwrate` has the node key but no LFO under it.
        let g = G::synth(VoiceParams {
            pw_lfo: None,
            ..pulse()
        });
        assert_eq!(g.main("pw_lfo", "frequency"), NoParam);
    }

    #[test]
    fn a_plain_pulse_has_a_width_lfo() {
        let depth = found(ModTarget::PwLfo(P::Depth), 2.5);
        assert_eq!(G::synth(pulse()).main("pw_lfo", "depth"), depth);
    }

    #[rstest]
    #[case::saw(VoiceParams { waveform: Waveform::Saw, ..pulse() })]
    #[case::additive(VoiceParams { additive: Some(vec![1.0]), ..pulse() })]
    #[case::noise(VoiceParams { noise: Some(NoiseKind::White), ..pulse() })]
    #[case::supersaw(VoiceParams { supersaw: true, ..pulse() })]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), ..pulse() })]
    fn anything_but_a_plain_pulse_has_no_width_lfo(#[case] p: VoiceParams) {
        assert_eq!(G::synth(p).main("pw_lfo", "frequency"), Absent);
    }

    fn filtered() -> G {
        let slot = |freq: f32, q: f32, rate: f64| FilterParams {
            freq: Some(freq),
            q,
            lfo: Some(lfo(rate)),
            ..FilterParams::default()
        };
        G::synth(VoiceParams {
            lp: slot(800.0, 3.0, 1.5),
            hp: slot(200.0, 1.5, 2.5),
            bp: FilterParams {
                model: FilterModel::Ladder,
                ..slot(1000.0, 4.0, 3.5)
            },
            ..VoiceParams::default()
        })
    }

    #[rstest]
    #[case("lpf", "frequency", found(ModTarget::Cutoff, 800.0))]
    #[case("lpf", "Q", found(ModTarget::Resonance, 3.0))]
    #[case("hpf", "frequency", found(ModTarget::Hcutoff, 200.0))]
    #[case("hpf", "Q", found(ModTarget::Hresonance, 1.5))]
    #[case("bpf", "frequency", found(ModTarget::Bandf, 1000.0))]
    // The ladder's resonance param is `q`.
    #[case("bpf", "Q", NoParam)]
    #[case("lpf", "gain", NoParam)]
    #[case(
        "lpf_lfo",
        "frequency",
        found(ModTarget::FilterLfo(FilterSlot::Low, P::Frequency), 1.5)
    )]
    #[case(
        "hpf_lfo",
        "frequency",
        found(ModTarget::FilterLfo(FilterSlot::High, P::Frequency), 2.5)
    )]
    #[case(
        "bpf_lfo",
        "frequency",
        found(ModTarget::FilterLfo(FilterSlot::Band, P::Frequency), 3.5)
    )]
    fn a_filter_with_a_frequency_is_there(
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(filtered().main(node, param), want);
    }

    #[rstest]
    fn a_filter_without_a_frequency_is_absent(#[values("lpf", "lpf_lfo")] node: &str) {
        assert_eq!(
            G::synth(VoiceParams::default()).main(node, "frequency"),
            Absent
        );
    }

    fn all_fx() -> PostFx {
        PostFx {
            vowel: Some(Vowel::A),
            coarse: Some(4.0),
            crush: Some(6.0),
            shape: Some(0.3),
            shapevol: 0.9,
            distort: Some(0.4),
            distortvol: 0.7,
            tremolo_lfo: Some(lfo(1.5)),
            tremolodepth: 0.25,
            compressor: Some(-20.0),
            comp_ratio: 4.0,
            comp_knee: 6.0,
            comp_attack: 0.01,
            comp_release: 0.2,
            phaser: Some(2.0),
            phaserdepth: 0.3,
            phasercenter: 500.0,
            phasersweep: 300.0,
            stretch: Some(1.5),
            transient: Some(0.5),
            postgain: 0.6,
            ..PostFx::default()
        }
    }

    #[rstest]
    #[case("vowel", "frequency", found(ModTarget::Vowel, 660.0))]
    #[case("vowel", "Q", NoParam)]
    #[case("coarse", "coarse", found(ModTarget::Coarse, 4.0))]
    #[case("coarse", "gain", NoParam)]
    #[case("crush", "crush", found(ModTarget::Crush, 6.0))]
    #[case("crush", "gain", NoParam)]
    #[case("shape", "shape", found(ModTarget::Shape, f(0.3)))]
    #[case("shape", "postgain", found(ModTarget::Shapevol, f(0.9)))]
    #[case("shape", "gain", NoParam)]
    #[case("distort", "distort", found(ModTarget::Distort, f(0.4)))]
    #[case("distort", "postgain", found(ModTarget::Distortvol, f(0.7)))]
    #[case("distort", "gain", NoParam)]
    #[case("tremolo", "skew", found(ModTarget::Tremolo(P::Skew), 0.25))]
    #[case("tremolo_gain", "gain", found(ModTarget::TremoloGain, 0.75))]
    #[case("tremolo_gain", "frequency", NoParam)]
    #[case("compressor", "threshold", found(ModTarget::Compressor(C::Threshold), -20.0))]
    #[case("compressor", "ratio", found(ModTarget::Compressor(C::Ratio), 4.0))]
    #[case("compressor", "knee", found(ModTarget::Compressor(C::Knee), 6.0))]
    #[case(
        "compressor",
        "attack",
        found(ModTarget::Compressor(C::Attack), f(0.01))
    )]
    #[case(
        "compressor",
        "release",
        found(ModTarget::Compressor(C::Release), f(0.2))
    )]
    #[case("compressor", "gain", NoParam)]
    #[case("pan", "pan", found(ModTarget::Pan, 0.5))]
    #[case("pan", "gain", NoParam)]
    #[case("phaser", "frequency", found(ModTarget::PhaserCenter, 782.0))]
    #[case("phaser", "Q", found(ModTarget::PhaserQ, 2.0 - f(0.3) * 2.0))]
    #[case("phaser", "depth", NoParam)]
    #[case(
        "phaser_lfo",
        "frequency",
        found(ModTarget::PhaserLfo(P::Frequency), 2.0)
    )]
    #[case("phaser_lfo", "depth", found(ModTarget::PhaserLfo(P::Depth), 600.0))]
    #[case("phaser_lfo", "skew", found(ModTarget::PhaserLfo(P::Skew), 0.0))]
    #[case("phaser_lfo", "Q", NoParam)]
    #[case("stretch", "pitchFactor", found(ModTarget::Stretch, 1.5))]
    #[case("stretch", "gain", NoParam)]
    #[case("transient", "attack", NoParam)]
    fn an_active_post_effect_has_its_params(
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        let mut g = G::synth(VoiceParams::default());
        g.fx = all_fx();
        g.controls.insert("pan".into(), Value::F64(0.75));
        assert_eq!(g.main(node, param), want);
    }

    #[rstest]
    fn an_inactive_post_effect_is_absent(
        #[values(
            "vowel",
            "coarse",
            "crush",
            "shape",
            "distort",
            "tremolo",
            "tremolo_gain",
            "compressor",
            "pan",
            "phaser",
            "phaser_lfo",
            "stretch",
            "transient"
        )]
        node: &str,
        #[values("frequency", "pan")] param: &str,
    ) {
        assert_eq!(G::synth(VoiceParams::default()).main(node, param), Absent);
    }

    #[rstest]
    fn a_phaser_needs_a_positive_depth(#[values("phaser", "phaser_lfo")] node: &str) {
        let mut g = G::synth(VoiceParams::default());
        g.fx = PostFx {
            phaserdepth: 0.0,
            ..all_fx()
        };
        assert_eq!(g.main(node, "frequency"), Absent);
    }

    fn spread() -> VoiceParams {
        VoiceParams {
            freq: 440.0,
            freqspread: 0.25,
            panspread: 0.625,
            ..VoiceParams::default()
        }
    }

    #[rstest]
    #[case::osc(VoiceParams { waveform: Waveform::Saw, freq: 220.0, ..VoiceParams::default() }, "frequency", found(ModTarget::Frequency, 220.0))]
    #[case::osc(VoiceParams { waveform: Waveform::Saw, ..VoiceParams::default() }, "pulsewidth", NoParam)]
    #[case::pulse(VoiceParams { freq: 330.0, ..pulse() }, "frequency", found(ModTarget::Frequency, 330.0))]
    #[case::pulse(VoiceParams { pw: 0.375, ..pulse() }, "pulsewidth", found(ModTarget::Pulsewidth, 0.375))]
    #[case::pulse(pulse(), "freqspread", NoParam)]
    #[case::supersaw(VoiceParams { supersaw: true, ..spread() }, "frequency", found(ModTarget::Frequency, 440.0))]
    #[case::supersaw(VoiceParams { supersaw: true, ..spread() }, "freqspread", found(ModTarget::Freqspread, 0.25))]
    #[case::supersaw(VoiceParams { supersaw: true, ..spread() }, "panspread", found(ModTarget::Panspread, 0.625))]
    #[case::supersaw(VoiceParams { supersaw: true, ..spread() }, "position", NoParam)]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), freq: 550.0, ..spread() }, "frequency", found(ModTarget::Frequency, 550.0))]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), ..spread() }, "freqspread", found(ModTarget::Freqspread, 0.25))]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), ..spread() }, "panspread", found(ModTarget::Panspread, 0.625))]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), wt: ParamMod { offset: 0.125, ..ParamMod::default() }, ..spread() }, "position", found(ModTarget::WtPosition, 0.125))]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), warp: ParamMod { offset: 0.875, ..ParamMod::default() }, ..spread() }, "warp", found(ModTarget::WtWarp, 0.875))]
    #[case::wavetable(VoiceParams { wavetable: wavetable(), ..spread() }, "pulsewidth", NoParam)]
    #[case::noise(VoiceParams { noise: Some(NoiseKind::Pink), freq: 220.0, ..VoiceParams::default() }, "frequency", found(ModTarget::Detune, 0.0))]
    #[case::noise(VoiceParams { noise: Some(NoiseKind::Pink), ..VoiceParams::default() }, "freqspread", NoParam)]
    fn a_synth_source_has_its_kinds_params(
        #[case] p: VoiceParams,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(G::synth(p).main("source", param), want);
    }

    fn bytebeat() -> VoiceSpec {
        let mut bb = ByteBeatParams::from_controls(&ValueMap::default(), 1.0);
        bb.freq = 330.0;
        VoiceSpec::ByteBeat(Box::new(bb))
    }

    #[rstest]
    #[case::sampler(
        VoiceSpec::Sampler(sampler()),
        "frequency",
        found(ModTarget::Detune, 0.0)
    )]
    #[case::sampler(VoiceSpec::Sampler(sampler()), "gain", NoParam)]
    #[case::drum(
        VoiceSpec::Drum(DrumParams::new(DrumKind::Bd)),
        "frequency",
        found(ModTarget::Detune, 0.0)
    )]
    #[case::zzfx(VoiceSpec::Zzfx(Box::new(ZzfxParams::from_controls("z_sine", &ValueMap::default(), 1.0))), "frequency", found(ModTarget::Detune, 0.0))]
    #[case::bytebeat(bytebeat(), "frequency", found(ModTarget::Frequency, 330.0))]
    #[case::bytebeat(bytebeat(), "byteBeatStartTime", NoParam)]
    #[case::bus(VoiceSpec::Bus(BusParams::from_controls(&ValueMap::default(), 1.0)), "frequency", NoParam)]
    fn other_sources_have_detune_or_nothing(
        #[case] spec: VoiceSpec,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(G::new(spec).main("source", param), want);
    }

    fn vibrato_sampler() -> VoiceSpec {
        let mut s = sampler();
        s.pitch.vib = Some(4.0);
        s.pitch.vibmod = 0.25;
        VoiceSpec::Sampler(s)
    }

    fn wt_lfos(wavetable: Option<WaveTable>) -> VoiceSpec {
        let lfo_mod = |rate| ParamMod {
            lfo: Some(lfo(rate)),
            ..ParamMod::default()
        };
        VoiceSpec::Synth(Box::new(VoiceParams {
            wavetable,
            wt: lfo_mod(2.0),
            warp: lfo_mod(3.0),
            ..VoiceParams::default()
        }))
    }

    fn synth_spec(p: VoiceParams) -> VoiceSpec {
        VoiceSpec::Synth(Box::new(p))
    }

    #[rstest]
    #[case::synth_vib(synth_spec(VoiceParams { vib: Some(5.0), ..VoiceParams::default() }), "vib", "frequency", found(ModTarget::VibFreq, 5.0))]
    #[case::synth_vib(synth_spec(VoiceParams { vib: Some(5.0), vibmod: 0.375, ..VoiceParams::default() }), "vib_gain", "gain", found(ModTarget::VibGain, 37.5))]
    #[case::synth_vib(synth_spec(VoiceParams { vib: Some(5.0), ..VoiceParams::default() }), "vib", "gain", NoParam)]
    // `if (vib > 0)`
    #[case::no_vib(synth_spec(VoiceParams { vib: Some(0.0), ..VoiceParams::default() }), "vib", "frequency", Absent)]
    #[case::sampler_vib(vibrato_sampler(), "vib", "frequency", found(ModTarget::VibFreq, 4.0))]
    #[case::sampler_vib(vibrato_sampler(), "vib_gain", "gain", found(ModTarget::VibGain, 25.0))]
    #[case::wavetable(
        wt_lfos(wavetable()),
        "wt_lfo",
        "frequency",
        found(ModTarget::WtLfo(P::Frequency), 2.0)
    )]
    #[case::wavetable(
        wt_lfos(wavetable()),
        "warp_lfo",
        "frequency",
        found(ModTarget::WarpLfo(P::Frequency), 3.0)
    )]
    #[case::no_wavetable(wt_lfos(None), "wt_lfo", "frequency", Absent)]
    fn source_parts_vibrato_and_wavetable_lfos(
        #[case] spec: VoiceSpec,
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(G::new(spec).main(node, param), want);
    }

    fn fm() -> VoiceSpec {
        let mut p = VoiceParams {
            freq: 200.0,
            ..VoiceParams::default()
        };
        // Operator 2 modulates the carrier; operator 1 modulates operator 2.
        p.fm.amt[2][0] = 3.0;
        p.fm.amt[1][2] = 1.25;
        p.fm.ops[2].ratio = 1.5;
        synth_spec(p)
    }

    #[rstest]
    #[case(fm(), "fm_2_gain", "gain", found(ModTarget::FmGain(2), 3.0))]
    #[case(fm(), "fm_2", "frequency", found(ModTarget::FmFreq(2), 300.0))]
    #[case(fm(), "fm_1_gain", "gain", found(ModTarget::FmGain(1), 1.25))]
    #[case(fm(), "fm_2", "gain", NoParam)]
    #[case(fm(), "fm_3", "frequency", Absent)]
    #[case(fm(), "fm_3_gain", "gain", Absent)]
    #[case(VoiceSpec::Sampler(sampler()), "fm_2", "frequency", Absent)]
    fn fm_operators_are_there_when_wired(
        #[case] spec: VoiceSpec,
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(G::new(spec).main(node, param), want);
    }

    fn delay_send() -> OrbitSend {
        OrbitSend {
            delay: 0.5,
            delay_cfg: DelayConfig {
                time: 0.25,
                feedback: 0.375,
            },
            ..OrbitSend::default()
        }
    }

    #[rstest]
    #[case("gain", "gain", found(ModTarget::Gain, f(0.7)))]
    #[case("gain", "frequency", NoParam)]
    #[case("post", "gain", found(ModTarget::Postgain, f(0.6)))]
    #[case("post", "frequency", NoParam)]
    #[case("delay_mix", "gain", found(ModTarget::DelaySend, 0.5))]
    #[case("delay", "delayTime", found(ModTarget::DelayTime, 0.25))]
    #[case("delay", "feedback", found(ModTarget::DelayFeedback, 0.375))]
    #[case("delay", "gain", NoParam)]
    #[case("room_mix", "gain", found(ModTarget::RoomSend, f(0.3)))]
    #[case("room", "gain", NoParam)]
    #[case("djf", "value", found(ModTarget::Djf, f(0.8)))]
    #[case("djf", "gain", NoParam)]
    #[case("dry", "gain", Absent)]
    fn the_main_chain_gain_and_orbit_sends(
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        let mut g = G::synth(VoiceParams {
            gain: 0.7,
            ..VoiceParams::default()
        });
        g.fx.postgain = 0.6;
        g.send = OrbitSend {
            room: 0.3,
            djf: Some(0.8),
            ..delay_send()
        };
        assert_eq!(g.main(node, param), want);
    }

    #[rstest]
    #[case::no_send(OrbitSend { delay: 0.0, ..delay_send() })]
    #[case::no_time(OrbitSend { delay_cfg: DelayConfig { time: 0.0, feedback: 0.375 }, ..delay_send() })]
    #[case::no_feedback(OrbitSend { delay_cfg: DelayConfig { time: 0.25, feedback: 0.0 }, ..delay_send() })]
    fn the_delay_needs_send_time_and_feedback_all_positive(#[case] send: OrbitSend) {
        let mut g = G::synth(VoiceParams::default());
        g.send = send;
        assert_eq!(g.main("delay_mix", "gain"), Absent);
    }

    #[rstest]
    #[case("room_mix", "gain")]
    #[case("djf", "value")]
    #[case("djf", "gain")]
    fn unused_orbit_nodes_are_absent(#[case] node: &str, #[case] param: &str) {
        assert_eq!(G::synth(VoiceParams::default()).main(node, param), Absent);
    }

    fn staged() -> G {
        let mut g = G::synth(VoiceParams::default());
        let map: ValueMap = [
            ("delay", 0.4),
            ("delaytime", 0.2),
            ("delayfeedback", 0.3),
            ("room", 0.6),
            ("cutoff", 900.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), Value::F64(v)))
        .collect();
        g.stages = vec![FxStage {
            gain: 0.5,
            filters: FilterSet::from_controls(&map),
            sends: StageSends::from_controls(&map, 1.0),
            ..FxStage::default()
        }];
        g.maps = vec![map];
        g
    }

    #[rstest]
    #[case("gain", "gain", found(ModTarget::Gain, 0.5))]
    #[case("gain", "frequency", NoParam)]
    #[case("lpf", "frequency", found(ModTarget::Cutoff, 900.0))]
    #[case("delay_mix", "gain", found(ModTarget::DelaySend, f(0.4)))]
    #[case("delay", "delayTime", found(ModTarget::DelayTime, f(0.2)))]
    #[case("delay", "feedback", found(ModTarget::DelayFeedback, f(0.3)))]
    #[case("delay", "gain", NoParam)]
    #[case("room_mix", "gain", found(ModTarget::RoomSend, f(0.6)))]
    #[case("room", "gain", NoParam)]
    #[case("room_mix", "frequency", NoParam)]
    #[case("djf", "value", Absent)]
    fn an_fx_stage_has_its_own_gain_inserts_and_sends(
        #[case] node: &str,
        #[case] param: &str,
        #[case] want: Lookup,
    ) {
        assert_eq!(staged().stage(0, node, param), want);
    }

    #[rstest]
    #[case(1, "gain", "gain")]
    #[case(0, "delay_mix", "gain")]
    #[case(0, "room_mix", "gain")]
    fn a_missing_stage_or_send_is_absent(
        #[case] i: usize,
        #[case] node: &str,
        #[case] param: &str,
    ) {
        let mut g = staged();
        g.stages[0].sends = StageSends::default();
        assert_eq!(g.stage(i, node, param), Absent);
    }
}
