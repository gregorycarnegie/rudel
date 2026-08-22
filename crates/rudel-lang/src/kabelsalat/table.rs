//! Every kabelsalat node type, with the defaults its inlets fall back to.
//!
//! Generated from `@kabelsalat/lib` 0.4.1's `lib.js` — the defaults are the
//! ones its `compile` functions destructure (`vars: [freq = 0, phase = 0]`),
//! not the `ins[].default` metadata beside them, because the destructuring is
//! what actually reaches the generated code.
//!
//! `variadic` marks the nodes that take any number of inputs — `add`, `mul`,
//! `seq`, `qf` — where there is nothing to pad.
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

/// One entry of kabelsalat's `nodeRegistry`.
pub struct NodeType {
    pub name: &'static str,
    /// One default per inlet, in order.
    pub defaults: &'static [f32],
    pub variadic: bool,
}

/// kabelsalat's `module` and alias names: spellings that expand into
/// primitives while the graph is built, so they never appear in [`TYPES`].
pub static MODULES: &[&str] = &[
    "ar",
    "ad",
    "perc",
    "lpf",
    "hpf",
    "lfnoise",
    "bipolar",
    "unipolar",
    "gt",
    "lt",
    "B",
    "_",
    "rangex",
    "qlpf",
    "qhpf",
    "qbpf",
    "qnf",
    "qapf",
    "fork",
    "pan",
    "mix",
    "floatbeat",
    // Two nodes whose exported name is not their type: a patch writes `rng()`
    // and `midin()`, and the graph gets an `lcgnoise` and a `MidiIn`.
    "rng",
    "midin",
];

/// Look a node type up by the name written in a patch.
pub fn lookup(name: &str) -> Option<&'static NodeType> {
    TYPES.iter().find(|t| t.name == name)
}

/// The registry itself.
///
/// `poly` and `peek` are graph-time markers that never reach the interpreter;
/// `n` is the constant node. `pat`, `sfreq` and `sgate` stand for the three
/// per-hap values upstream splices into the source text before compiling —
/// a pattern written inside the graph, the note's frequency, and its gate.
#[rustfmt::skip]
pub static TYPES: &[NodeType] = &[
    NodeType { name: "n", defaults: &[], variadic: false },
    NodeType { name: "poly", defaults: &[], variadic: true },
    NodeType { name: "peek", defaults: &[], variadic: false },
    NodeType { name: "pat", defaults: &[], variadic: false },
    NodeType { name: "sfreq", defaults: &[], variadic: false },
    NodeType { name: "sgate", defaults: &[], variadic: false },
    NodeType { name: "mix", defaults: &[], variadic: true },
    NodeType { name: "time", defaults: &[], variadic: false },
    NodeType { name: "raw", defaults: &[0.0], variadic: false },
    NodeType { name: "bytebeat", defaults: &[0.0], variadic: false },
    NodeType { name: "adsr", defaults: &[0.0, 0.02, 0.1, 0.2, 0.1], variadic: false },
    NodeType { name: "clock", defaults: &[120.0], variadic: false },
    NodeType { name: "clockdiv", defaults: &[0.0, 2.0], variadic: false },
    NodeType { name: "distort", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "noise", defaults: &[], variadic: false },
    NodeType { name: "lcgnoise", defaults: &[], variadic: false },
    NodeType { name: "pink", defaults: &[], variadic: false },
    NodeType { name: "brown", defaults: &[], variadic: false },
    NodeType { name: "dust", defaults: &[0.0], variadic: false },
    NodeType { name: "impulse", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "saw", defaults: &[0.0], variadic: false },
    NodeType { name: "zaw", defaults: &[0.0], variadic: false },
    NodeType { name: "sine", defaults: &[0.0, 0.0, 0.0], variadic: false },
    NodeType { name: "tri", defaults: &[0.0], variadic: false },
    NodeType { name: "pulse", defaults: &[0.0, 0.5], variadic: false },
    NodeType { name: "slide", defaults: &[0.0, 1.0], variadic: false },
    NodeType { name: "lag", defaults: &[0.0, 1.0], variadic: false },
    NodeType { name: "slew", defaults: &[0.0, 1.0, 1.0], variadic: false },
    NodeType { name: "filter", defaults: &[0.0, 1.0, 0.0], variadic: false },
    NodeType { name: "fold", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "seq", defaults: &[], variadic: true },
    NodeType { name: "delay", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "hold", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "MidiIn", defaults: &[], variadic: false },
    NodeType { name: "midifreq", defaults: &[-1.0], variadic: false },
    NodeType { name: "midigate", defaults: &[-1.0], variadic: false },
    NodeType { name: "midivel", defaults: &[-1.0], variadic: false },
    NodeType { name: "midicc", defaults: &[-1.0, -1.0], variadic: false },
    NodeType { name: "cc", defaults: &[0.0], variadic: false },
    NodeType { name: "audioin", defaults: &[], variadic: false },
    NodeType { name: "log", defaults: &[0.0], variadic: false },
    NodeType { name: "exp", defaults: &[0.0], variadic: false },
    NodeType { name: "pow", defaults: &[0.0, 1.0], variadic: false },
    NodeType { name: "sin", defaults: &[0.0], variadic: false },
    NodeType { name: "cos", defaults: &[0.0], variadic: false },
    NodeType { name: "tan", defaults: &[0.0], variadic: false },
    NodeType { name: "acos", defaults: &[0.0], variadic: false },
    NodeType { name: "asin", defaults: &[0.0], variadic: false },
    NodeType { name: "atan", defaults: &[0.0], variadic: false },
    NodeType { name: "mul", defaults: &[], variadic: true },
    NodeType { name: "add", defaults: &[], variadic: true },
    NodeType { name: "div", defaults: &[], variadic: true },
    NodeType { name: "sub", defaults: &[], variadic: true },
    NodeType { name: "mod", defaults: &[], variadic: true },
    NodeType { name: "abs", defaults: &[0.0], variadic: false },
    NodeType { name: "round", defaults: &[0.0], variadic: false },
    NodeType { name: "clamp", defaults: &[0.0, -1.0, 1.0], variadic: false },
    NodeType { name: "floor", defaults: &[0.0], variadic: false },
    NodeType { name: "ceil", defaults: &[0.0], variadic: false },
    NodeType { name: "sign", defaults: &[0.0], variadic: false },
    NodeType { name: "min", defaults: &[], variadic: true },
    NodeType { name: "max", defaults: &[], variadic: true },
    NodeType { name: "argmin", defaults: &[], variadic: true },
    NodeType { name: "argmax", defaults: &[], variadic: true },
    NodeType { name: "greater", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "lower", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "xor", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "and", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "or", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "not", defaults: &[0.0], variadic: false },
    NodeType { name: "bool", defaults: &[0.0], variadic: false },
    NodeType { name: "ifelse", defaults: &[0.0, 0.0, 0.0], variadic: false },
    NodeType { name: "range", defaults: &[], variadic: true },
    NodeType { name: "remap", defaults: &[0.0, -1.0, 1.0, -1.0, 1.0], variadic: false },
    NodeType { name: "thru", defaults: &[], variadic: false },
    NodeType { name: "midinote", defaults: &[0.0], variadic: false },
    NodeType { name: "src", defaults: &[0.0], variadic: false },
    NodeType { name: "output", defaults: &[0.0, 0.0], variadic: false },
    NodeType { name: "bpf", defaults: &[0.0, 1.0, 0.0], variadic: false },
    NodeType { name: "pick", defaults: &[], variadic: true },
    NodeType { name: "clip", defaults: &[0.0, -1.0, 1.0], variadic: false },
    NodeType { name: "trig", defaults: &[0.0, -1.0, 1.0], variadic: false },
    NodeType { name: "qf", defaults: &[], variadic: true },
];
