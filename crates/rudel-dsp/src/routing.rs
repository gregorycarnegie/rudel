// routing.rs - which parameter a modulator lands on, as superdough decides it.
//
// modulators.mjs resolves a modulator's `control` (+ `subControl`) through
// `CONTROL_TARGETS` (superdoughdata.mjs) to a node key and an AudioParam name,
// then looks the node up in the graph it just built for the hap. Three things
// can happen, and they are not equivalent:
//
// - the param is there: it is modulated;
// - the control is unknown, or the node is not in this hap's graph: the
//   modulator is logged and skipped, and the rest still run;
// - the node is there but has no such param: `targetParams[0].value` throws,
//   and that aborts every modulator after it for this hap.
//
// The table is ported verbatim; [`ModGraph`] answers the graph half, per hap.
// docs/MODULATION_TARGETS.md lists every control and what it reaches.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::modulator::ModTarget;

/// `CONTROL_TARGETS`: control name -> (node key, AudioParam name).
const CONTROL_TARGETS: &[(&str, &str, &str)] = &[
    ("stretch", "stretch", "pitchFactor"),
    ("gain", "gain", "gain"),
    ("postgain", "post", "gain"),
    ("pan", "pan", "pan"),
    ("tremolo", "tremolo", "frequency"),
    ("tremolosync", "tremolo", "frequency"),
    ("tremolodepth", "tremolo_gain", "gain"),
    ("tremoloskew", "tremolo", "skew"),
    ("tremolophase", "tremolo", "phase"),
    ("tremoloshape", "tremolo", "shape"),
    // MODULATORS
    ("lfo", "lfo", "frequency"),
    ("lfo_rate", "lfo", "frequency"),
    ("lfo_sync", "lfo", "frequency"),
    ("lfo_depth", "lfo", "depth"),
    ("lfo_depthabs", "lfo", "depth"),
    ("lfo_skew", "lfo", "skew"),
    ("lfo_curve", "lfo", "curve"),
    ("lfo_dcoffset", "lfo", "dcoffset"),
    ("env", "env", "depth"),
    ("env_attack", "env", "attack"),
    ("env_decay", "env", "decay"),
    ("env_sustain", "env", "sustain"),
    ("env_release", "env", "release"),
    ("bmod", "bmod", "depth"),
    ("bmod_depth", "bmod", "depth"),
    ("bmod_depthabs", "bmod", "depth"),
    // LPF
    ("cutoff", "lpf", "frequency"),
    ("resonance", "lpf", "Q"),
    ("lprate", "lpf_lfo", "rate"),
    ("lpsync", "lpf_lfo", "sync"),
    ("lpdepth", "lpf_lfo", "depth"),
    ("lpdepthfrequency", "lpf_lfo", "depth"),
    ("lpshape", "lpf_lfo", "shape"),
    ("lpdc", "lpf_lfo", "dcoffset"),
    ("lpskew", "lpf_lfo", "skew"),
    // HPF
    ("hcutoff", "hpf", "frequency"),
    ("hresonance", "hpf", "Q"),
    ("hprate", "hpf_lfo", "rate"),
    ("hpsync", "hpf_lfo", "sync"),
    ("hpdepth", "hpf_lfo", "depth"),
    ("hpdepthfrequency", "hpf_lfo", "depth"),
    ("hpshape", "hpf_lfo", "shape"),
    ("hpdc", "hpf_lfo", "dcoffset"),
    ("hpskew", "hpf_lfo", "skew"),
    // BPF
    ("bandf", "bpf", "frequency"),
    ("bandq", "bpf", "Q"),
    ("bprate", "bpf_lfo", "rate"),
    ("bpsync", "bpf_lfo", "sync"),
    ("bpdepth", "bpf_lfo", "depth"),
    ("bpdepthfrequency", "bpf_lfo", "depth"),
    ("bpshape", "bpf_lfo", "shape"),
    ("bpdc", "bpf_lfo", "dcoffset"),
    ("bpskew", "bpf_lfo", "skew"),
    ("vowel", "vowel", "frequency"),
    // DISTORTION
    ("coarse", "coarse", "coarse"),
    ("crush", "crush", "crush"),
    ("shape", "shape", "shape"),
    ("shapevol", "shape", "postgain"),
    ("distort", "distort", "distort"),
    ("distortvol", "distort", "postgain"),
    ("distorttype", "distort", "distort"),
    // COMPRESSOR
    ("compressor", "compressor", "threshold"),
    ("compressorRatio", "compressor", "ratio"),
    ("compressorKnee", "compressor", "knee"),
    ("compressorAttack", "compressor", "attack"),
    ("compressorRelease", "compressor", "release"),
    // PHASER
    ("phaserrate", "phaser_lfo", "frequency"),
    ("phasersweep", "phaser_lfo", "depth"),
    ("phasercenter", "phaser", "frequency"),
    ("phaserdepth", "phaser", "Q"),
    // ORBIT EFFECTS
    ("delay", "delay_mix", "gain"),
    ("delaytime", "delay", "delayTime"),
    ("delayfeedback", "delay", "feedback"),
    ("delaysync", "delay", "delayTime"),
    ("dry", "dry", "gain"),
    ("room", "room_mix", "gain"),
    ("djf", "djf", "value"),
    ("busgain", "bus", "gain"),
    // SYNTHS
    ("s", "source", "frequency"),
    ("detune", "source", "freqspread"),
    ("wt", "source", "position"),
    ("warp", "source", "warp"),
    ("freq", "source", "frequency"),
    ("note", "source", "frequency"),
    ("wtdc", "wt_lfo", "dc"),
    ("wtskew", "wt_lfo", "skew"),
    ("wtrate", "wt_lfo", "frequency"),
    ("wtsync", "wt_lfo", "frequency"),
    ("wtdepth", "wt_lfo", "depth"),
    ("warpdc", "warp_lfo", "dc"),
    ("warpskew", "warp_lfo", "skew"),
    ("warprate", "warp_lfo", "frequency"),
    ("warpsync", "warp_lfo", "frequency"),
    ("warpdepth", "warp_lfo", "depth"),
    ("fmi", "fm_1_gain", "gain"),
    ("fmi2", "fm_2_gain", "gain"),
    ("fmi3", "fm_3_gain", "gain"),
    ("fmi4", "fm_4_gain", "gain"),
    ("fmi5", "fm_5_gain", "gain"),
    ("fmi6", "fm_6_gain", "gain"),
    ("fmi7", "fm_7_gain", "gain"),
    ("fmi8", "fm_8_gain", "gain"),
    ("fmh", "fm_1", "frequency"),
    ("fmh2", "fm_2", "frequency"),
    ("fmh3", "fm_3", "frequency"),
    ("fmh4", "fm_4", "frequency"),
    ("fmh5", "fm_5", "frequency"),
    ("fmh6", "fm_6", "frequency"),
    ("fmh7", "fm_7", "frequency"),
    ("fmh8", "fm_8", "frequency"),
    ("pw", "source", "pulsewidth"),
    ("pwrate", "pw_lfo", "frequency"),
    ("pwsweep", "pw_lfo", "depth"),
    ("vib", "vib", "frequency"),
    ("vibmod", "vib_gain", "gain"),
    ("byteBeatStartTime", "source", "byteBeatStartTime"),
    ("spread", "source", "panspread"),
    ("transient", "transient", "attack"),
];

fn control_target(name: &str) -> Option<(&'static str, &'static str)> {
    CONTROL_TARGETS
        .iter()
        .find(|(control, ..)| *control == name)
        .map(|&(_, node, param)| (node, param))
}

/// `getControlData`: `${control}_${subControl}` first, then the control alone,
/// after stripping an `_<id>` suffix (`lfo_1` -> `lfo`). An absent
/// `subControl` is JS `undefined`, which the template turns into the string
/// "undefined" and so never matches.
pub(crate) fn control_data(
    control: &str,
    sub_control: Option<&str>,
) -> Option<(&'static str, &'static str)> {
    let base = control.split('_').next().unwrap_or(control);
    let sub = sub_control.unwrap_or("undefined");
    control_target(&format!("{base}_{sub}")).or_else(|| control_target(base))
}

/// What the graph holds under a node key, for one param of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lookup {
    /// No node under that key: the modulator is skipped.
    Absent,
    /// The node is there without the param: setup throws.
    NoParam,
    /// The param, with its current value (what a relative `depth` scales).
    Found(ModTarget, f64),
}

/// Where a modulator connects: the main chain, or one `FX(...)` stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fxi {
    Main,
    Stage(usize),
}

/// One hap's audio graph, as far as modulation can see it.
pub trait ModGraph {
    /// `nodes[fxi][node]`, looked up for `param`.
    fn lookup(&self, fxi: Fxi, node: &str, param: &str) -> Lookup;
}

/// How a modulator's setup ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// It modulates this param, which has this value now.
    Connect(ModTarget, f64),
    /// Logged and skipped; the hap's other modulators still run.
    Skip,
    /// Setup throws: this modulator and every one after it are dropped.
    Throw,
}

/// `getTargetParamsForControl`: resolve `control`/`subControl` in `graph`.
/// `nodes[targetInfo.node] ? targetInfo.node : control` is why a modulator
/// reaches another one by its full name (`lfo_0`): no node is called `lfo`.
pub(crate) fn resolve(
    graph: &dyn ModGraph,
    fxi: Fxi,
    control: &str,
    sub_control: Option<&str>,
) -> Outcome {
    let Some((node, param)) = control_data(control, sub_control) else {
        return Outcome::Skip;
    };
    let lookup = match graph.lookup(fxi, node, param) {
        Lookup::Absent => graph.lookup(fxi, control, param),
        found => found,
    };
    match lookup {
        Lookup::Absent => Outcome::Skip,
        Lookup::NoParam => Outcome::Throw,
        Lookup::Found(target, current) => Outcome::Connect(target, current),
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    /// Every `(node, param)` row of the table.
    pub(crate) fn all_rows() -> impl Iterator<Item = (&'static str, &'static str)> {
        super::CONTROL_TARGETS
            .iter()
            .map(|&(_, node, param)| (node, param))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sub_control_is_tried_before_the_control_alone() {
        assert_eq!(
            control_data("lfo_0", Some("rate")),
            Some(("lfo", "frequency"))
        );
        assert_eq!(control_data("lfo_0", Some("depth")), Some(("lfo", "depth")));
        assert_eq!(control_data("lfo_0", None), Some(("lfo", "frequency")));
        // An unknown sub-control falls back to the control.
        assert_eq!(
            control_data("cutoff", Some("nope")),
            Some(("lpf", "frequency"))
        );
        assert_eq!(control_data("velocity", None), None);
        // `_` splits the control: `lfo_rate` itself is looked up as `lfo`.
        assert_eq!(control_data("lfo_rate", None), Some(("lfo", "frequency")));
    }

    struct Fake(&'static [(&'static str, &'static str)]);
    impl ModGraph for Fake {
        fn lookup(&self, _: Fxi, node: &str, param: &str) -> Lookup {
            if !self.0.iter().any(|(n, _)| *n == node) {
                return Lookup::Absent;
            }
            match self.0.iter().find(|(n, p)| *n == node && *p == param) {
                Some(_) => Lookup::Found(ModTarget::Gain, 1.0),
                None => Lookup::NoParam,
            }
        }
    }

    #[test]
    fn the_three_outcomes() {
        let graph = Fake(&[
            ("lpf", "frequency"),
            ("tremolo", "frequency"),
            ("lfo_0", "frequency"),
        ]);
        let outcome = |c, s| resolve(&graph, Fxi::Main, c, s);
        assert!(matches!(outcome("cutoff", None), Outcome::Connect(..)));
        assert_eq!(
            outcome("hcutoff", None),
            Outcome::Skip,
            "no hpf in the graph"
        );
        assert_eq!(outcome("velocity", None), Outcome::Skip, "not a target");
        assert_eq!(
            outcome("tremolophase", None),
            Outcome::Throw,
            "tremolo has no `phase`"
        );
        assert!(
            matches!(outcome("lfo_0", Some("rate")), Outcome::Connect(..)),
            "another modulator, by its full name"
        );
        assert_eq!(
            outcome("lfo", None),
            Outcome::Skip,
            "no node is called `lfo`"
        );
    }
}
