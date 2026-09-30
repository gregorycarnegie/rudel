//! The kabelsalat node DSL, as script values.
//!
//! `saw(110).lpf(sine(1).range(.3,.8)).out()` builds a graph; [`K`] compiles it
//! and hands the result to the audio side as a `worklet` control.
//!
//! Every node type is both a function and a method, because kabelsalat's
//! `register` puts it on `Node.prototype` as well as in scope: `sine(220)` and
//! `impulse(4).sine()` are the same node, the receiver sliding in as the first
//! argument. The names are registered at runtime off
//! [`crate::kabelsalat::TYPES`] rather than hand-listed, which keeps them from
//! drifting from the table the compiler reads — and sidesteps the handful
//! (`mod`, `not`, `and`) that Rust cannot spell as a method name.
//!
//! The names collide with Strudel's on purpose: `sine`, `saw`, `noise`, `time`
//! and `range` all mean something else at the top level of a pattern. They live
//! under a `Kabel` map for that reason, the way hydra's sources live under
//! `Hydra`. Writing them bare — which is how every kabelsalat patch in the wild
//! reads — needs the preprocessor to inject the scope, exactly as Strudel's
//! transpiler does by stringifying the expression and evaluating it in
//! kabelsalat's own scope.
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    js::{self, Arg, Scope},
    kabelsalat::{self, Arg as Inlet, MODULES, NodeId, TYPES},
};
use rudel_core::Value;

/// Read one call argument as a graph input.
///
/// Numbers become constant nodes, arrays become multichannel `poly` nodes, and
/// a function becomes a feedback edge — kabelsalat calls it with the node being
/// built, so `add(x => x.delay(.2).mul(.8))` wires a node's own output back
/// into it. A pattern reaching an inlet is upstream's `S(...)` placeholder: it
/// is sampled per hap rather than baked into the graph, so it becomes a `pat`
/// node. Mini-notation lands here too, since `m("220 440")` is a pattern by
/// the time the graph sees it — which is exactly the second form upstream's
/// extractor looks for.
fn arg(value: &Arg) -> Inlet {
    match value {
        Arg::Kabel(id) => Inlet::Node(*id),
        Arg::Pat(p) => Inlet::Node(kabelsalat::pattern_input(p.clone())),
        Arg::Num(n) => Inlet::Node(kabelsalat::constant(*n)),
        Arg::List(items) => Inlet::Poly(items.iter().map(arg).collect()),
        Arg::Func(_) => {
            let func = value.clone();
            // Called while the node is being built, inside the same native
            // call, so the script is still there to call.
            Inlet::Feedback(Box::new(move |owner| {
                match js::call(&func, vec![Arg::Kabel(owner)]) {
                    Ok(Arg::Kabel(id)) => Inlet::Node(id),
                    Ok(Arg::Num(n)) => Inlet::Node(kabelsalat::constant(n)),
                    // A feedback function that throws, or returns something
                    // else, leaves silence in the loop rather than failing the
                    // whole evaluation.
                    _ => Inlet::Node(kabelsalat::constant(0.0)),
                }
            }))
        }
        // Anything else — a string, `null` — is a zero, as `Node.parseInput`
        // falls back to when it does not recognise an input.
        _ => Inlet::Node(kabelsalat::constant(0.0)),
    }
}

fn args(values: &[Arg]) -> Vec<Inlet> {
    values.iter().map(arg).collect()
}

/// The node types that are not spelled the way they are built: kabelsalat's
/// `module` and alias definitions, which expand into primitives at graph time
/// rather than reaching the interpreter as nodes of their own.
///
/// ponytail: the eleven that patches actually reach for. `fork`, `pan`,
/// `rangex`, `split`, `scope` and the `mouse*` pair are not here yet; they
/// build as unknown types, which the compiler resolves to `thru`. Add one by
/// naming its expansion below — each is two or three lines in `lib.js`.
fn expand(name: &str, mut args: Vec<Inlet>) -> Option<NodeId> {
    // `n` marks a constant node, whose value some expansions need at graph time.
    // Pad to `n` arguments with the module's own defaults, since these are
    // JavaScript default parameters rather than compile-time padding.
    let pad = |defaults: &[f64], args: &mut Vec<Inlet>| {
        for &d in defaults.iter().skip(args.len()) {
            args.push(Inlet::Node(kabelsalat::constant(d)));
        }
    };
    let node = |kind, args| Inlet::Node(kabelsalat::get_node(kind, args));
    match name {
        // ar(gate, att, rel) => gate.adsr(att, 0, 1, rel)
        "ar" => {
            pad(&[0.0, 0.02, 0.1], &mut args);
            let (gate, att, rel) = (args.remove(0), args.remove(0), args.remove(0));
            let zero = Inlet::Node(kabelsalat::constant(0.0));
            let one = Inlet::Node(kabelsalat::constant(1.0));
            Some(kabelsalat::get_node(
                "adsr",
                vec![gate, att, zero, one, rel],
            ))
        }
        // ad(gate, att, dec) => gate.adsr(att, dec, 0, dec)
        "ad" => {
            pad(&[0.0, 0.02, 0.1], &mut args);
            let (gate, att, dec) = (args.remove(0), args.remove(0), args.remove(0));
            // Decay and release are the same node, not a copy of it.
            let release = clone_arg(&dec);
            let zero = Inlet::Node(kabelsalat::constant(0.0));
            Some(kabelsalat::get_node(
                "adsr",
                vec![gate, att, dec, zero, release],
            ))
        }
        // perc(gate, decay) => gate.adsr(0, 0, 1, decay)
        "perc" => {
            pad(&[0.0, 0.1], &mut args);
            let (gate, decay) = (args.remove(0), args.remove(0));
            let zero = Inlet::Node(kabelsalat::constant(0.0));
            let zero2 = Inlet::Node(kabelsalat::constant(0.0));
            let one = Inlet::Node(kabelsalat::constant(1.0));
            Some(kabelsalat::get_node(
                "adsr",
                vec![gate, zero, zero2, one, decay],
            ))
        }
        // lpf is filter under another name.
        "lpf" => Some(kabelsalat::get_node("filter", args)),
        // hpf(in, cutoff, reso) => in.sub(in.lpf(cutoff, reso))
        "hpf" => {
            pad(&[0.0, 1.0, 0.0], &mut args);
            let (input, cutoff, reso) = (args.remove(0), args.remove(0), args.remove(0));
            let low = node("filter", vec![clone_arg(&input), cutoff, reso]);
            Some(kabelsalat::get_node("sub", vec![input, low]))
        }
        // lfnoise(freq) => noise().hold(impulse(freq))
        "lfnoise" => {
            pad(&[1.0], &mut args);
            let freq = args.remove(0);
            let noise = node("noise", Vec::new());
            let trig = node("impulse", vec![freq]);
            Some(kabelsalat::get_node("hold", vec![noise, trig]))
        }
        // bipolar(u) => u.mul(2).sub(1)
        "bipolar" => {
            pad(&[0.0], &mut args);
            let two = Inlet::Node(kabelsalat::constant(2.0));
            let one = Inlet::Node(kabelsalat::constant(1.0));
            let scaled = node("mul", vec![args.remove(0), two]);
            Some(kabelsalat::get_node("sub", vec![scaled, one]))
        }
        // unipolar(b) => range(b, 0, 1), which is `(b + 1) * 0.5`.
        "unipolar" => {
            pad(&[0.0], &mut args);
            let zero = Inlet::Node(kabelsalat::constant(0.0));
            let one = Inlet::Node(kabelsalat::constant(1.0));
            Some(kabelsalat::get_node(
                "range",
                vec![args.remove(0), zero, one],
            ))
        }
        // rangex(sig, min, max): an exponential range, interpolated in the
        // log domain and brought back with `exp`.
        "rangex" => {
            pad(&[0.0, 1.0, 2.0], &mut args);
            let (sig, min, max) = (args.remove(0), args.remove(0), args.remove(0));
            let log_min = node("log", vec![min]);
            let log_max = node("log", vec![max]);
            let span = node("sub", vec![log_max, clone_arg(&log_min)]);
            let uni = Inlet::Node(expand("unipolar", vec![sig])?);
            let scaled = node("mul", vec![uni, span]);
            let shifted = node("add", vec![scaled, log_min]);
            Some(kabelsalat::get_node("exp", vec![shifted]))
        }
        // The five biquad presets, each `qf` with its type nailed down.
        "qlpf" | "qhpf" | "qbpf" | "qnf" | "qapf" => {
            let kind = match name {
                "qlpf" => 0.0,
                "qhpf" => 1.0,
                "qbpf" => 2.0,
                "qnf" => 3.0,
                _ => 4.0,
            };
            pad(&[0.0, 500.0, 10.0], &mut args);
            let (input, freq, q) = (args.remove(0), args.remove(0), args.remove(0));
            let kind = Inlet::Node(kabelsalat::constant(kind));
            Some(kabelsalat::get_node("qf", vec![input, kind, freq, q]))
        }
        // fork(in, times): the same signal on `times` channels.
        "fork" => {
            pad(&[0.0, 1.0], &mut args);
            let input = args.remove(0);
            let times = constant_of(&args.remove(0)).unwrap_or(1.0).max(1.0) as usize;
            let copies = (0..times).map(|_| clone_arg(&input)).collect();
            Some(kabelsalat::get_node("poly", copies))
        }
        // pan(in, pos): equal-power placement, `pos` bipolar.
        "pan" => {
            pad(&[0.0, 0.0], &mut args);
            let (input, pos) = (args.remove(0), args.remove(0));
            let one = Inlet::Node(kabelsalat::constant(1.0));
            // `mul` is variadic, and upstream passes PI and 0.25 separately:
            // `n(pos).add(1).mul(PI, 0.25)`.
            let pi = Inlet::Node(kabelsalat::constant(std::f64::consts::PI));
            let quarter = Inlet::Node(kabelsalat::constant(0.25));
            let shifted = node("add", vec![pos, one]);
            let angle = node("mul", vec![shifted, pi, quarter]);
            let left = node("cos", vec![clone_arg(&angle)]);
            let right = node("sin", vec![angle]);
            let spread = Inlet::Poly(vec![left, right]);
            Some(kabelsalat::get_node("mul", vec![input, spread]))
        }
        // mix(in, channels): fold a multichannel signal down to one or two.
        // A signal that is not multichannel is already mixed.
        "mix" => {
            pad(&[0.0, 1.0], &mut args);
            let input = args.remove(0);
            let channels = constant_of(&args.remove(0)).unwrap_or(1.0);
            let Inlet::Node(id) = input else {
                return None;
            };
            let Some(spread) = kabelsalat::poly_channels(id) else {
                return Some(id);
            };
            if channels < 2.0 {
                return Some(kabelsalat::get_node(
                    "mix",
                    spread.into_iter().map(Inlet::Node).collect(),
                ));
            }
            // Two channels: place the inputs evenly across the image and sum.
            // The positions are fixed once the graph is built, so upstream
            // computes the gains here rather than emitting nodes for them.
            let last = (spread.len().max(2) - 1) as f64;
            let panned: Vec<Inlet> = spread
                .iter()
                .enumerate()
                .map(|(i, &channel)| {
                    let pos = (i as f64 / last) * 2.0 - 1.0;
                    let deg = (pos + 1.0) * std::f64::consts::FRAC_PI_4;
                    let gains = Inlet::Poly(vec![
                        Inlet::Node(kabelsalat::constant(deg.cos())),
                        Inlet::Node(kabelsalat::constant(deg.sin())),
                    ]);
                    Inlet::Node(kabelsalat::get_node(
                        "mul",
                        vec![Inlet::Node(channel), gains],
                    ))
                })
                .collect();
            Some(kabelsalat::get_node("add", panned))
        }
        "rng" => Some(kabelsalat::get_node("lcgnoise", args)),
        "midin" => Some(kabelsalat::get_node("MidiIn", args)),
        "gt" => Some(kabelsalat::get_node("greater", args)),
        "lt" => Some(kabelsalat::get_node("lower", args)),
        _ => None,
    }
}

/// The value behind a constant argument, for the expansions that branch on it
/// (`fork`'s channel count, `mix`'s width) rather than wiring it as a signal.
fn constant_of(arg: &Inlet) -> Option<f64> {
    match arg {
        Inlet::Node(id) => kabelsalat::constant_value(*id),
        _ => None,
    }
}

/// Reuse an argument in two places. Only nodes can be shared — a fresh
/// constant is as good as a shared one, and a feedback closure would have to
/// run twice.
fn clone_arg(arg: &Inlet) -> Inlet {
    match arg {
        Inlet::Node(id) => Inlet::Node(*id),
        _ => Inlet::Node(kabelsalat::constant(0.0)),
    }
}

/// Build a node, taking the module expansions first so `lpf` means `filter`
/// rather than an unknown type.
fn build(name: &str, args: Vec<Inlet>, source: Option<String>) -> NodeId {
    // `bytebeat`, `floatbeat` and `raw` carry a body rather than an inlet:
    // upstream writes it as a tagged template, which the preprocessor has
    // already turned into a trailing string argument.
    if let Some(code) = source {
        // `floatbeat` builds a `bytebeat` node upstream — see `CodedKind` in
        // rudel-dsp for why that is not a typo here.
        let kind = if name == "floatbeat" {
            "bytebeat"
        } else {
            name
        };
        let Some(kind) = kabelsalat::type_name(kind) else {
            return kabelsalat::get_node("thru", args);
        };
        return kabelsalat::get_node_with_value(kind, Value::Str(code), args);
    }
    // `n(x)` — and its `B`/`_` aliases — is not a node taking an inlet: it
    // *is* the value. A number becomes a constant, an array becomes a
    // multichannel spread, and a node passes straight through, which is what
    // makes `n(sine(1))` a no-op rather than a wrapper.
    if matches!(name, "n" | "B" | "_") {
        return match args.first() {
            Some(arg) => kabelsalat::resolve(arg),
            None => kabelsalat::constant(0.0),
        };
    }
    if let Some(id) = expand(name, args_for_expand(&args)) {
        return id;
    }
    match kabelsalat::type_name(name) {
        Some(kind) => kabelsalat::get_node(kind, args),
        // Unreachable in practice: only the table's own names are registered,
        // and a typo fails in the script before reaching here — as it does
        // upstream, where `Node.prototype` has nothing to call.
        None => kabelsalat::get_node("thru", args),
    }
}

/// The node types whose last argument is source text rather than a signal.
const CODED: &[&str] = &["bytebeat", "floatbeat", "raw", "cc"];

/// Split a coded node's trailing source string off its inlets.
fn take_source(name: &str, values: &[Arg]) -> Option<String> {
    if !CODED.contains(&name) {
        return None;
    }
    match values.last() {
        Some(Arg::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

/// `expand` consumes its arguments, so hand it the node ids and keep the
/// originals for the ordinary path.
fn args_for_expand(args: &[Inlet]) -> Vec<Inlet> {
    args.iter().map(clone_arg).collect()
}

/// Build node `name` from `values`, which may end in a coded node's source.
fn build_call(name: &str, lead: Option<NodeId>, values: &[Arg]) -> Arg {
    let source = take_source(name, values);
    let taken = values.len() - usize::from(source.is_some());
    let mut all: Vec<Inlet> = lead.map(Inlet::Node).into_iter().collect();
    all.extend(args(&values[..taken]));
    Arg::Kabel(build(name, all, source))
}

/// Register `Kabel.<name>` for every node type, the same names as node
/// methods, and `K` both as a function and as a pattern method.
pub(crate) fn register(prelude: &Scope, pattern: &Scope) {
    let namespace = prelude.namespace("Kabel");
    let node = Scope::kabel();
    let names = TYPES.iter().map(|t| t.name).chain(MODULES.iter().copied());
    for name in names {
        // As a function: `sine(220)`.
        namespace.func(name, move |a| Ok(build_call(name, None, a)));
        // As a method: `impulse(4).sine()`, the receiver sliding in first.
        node.method(name, move |this, a| match this {
            Arg::Kabel(id) => Ok(build_call(name, Some(*id), a)),
            _ => Err(format!("{name}: not called on a node")),
        });
    }
    // `.out(channels)`: mark this node as reaching the speakers. Returns the
    // node, as upstream does, so a chain can carry on past it.
    node.method("out", |this, a| {
        let Arg::Kabel(id) = this else {
            return Err("out: not called on a node".to_string());
        };
        kabelsalat::out(*id, &out_channels(a));
        Ok(Arg::Kabel(*id))
    });
    // `.apply(f)`: hand the node to a function, which is how a patch uses one
    // node twice without naming it.
    node.method("apply", |this, a| match (this, a.first()) {
        (Arg::Kabel(id), Some(f)) => js::call(f, vec![Arg::Kabel(*id)]),
        (this, _) => Ok(this.clone()),
    });
    // `K(graph)` on its own is a control pattern, to be combined like any
    // other.
    prelude.func("K", |a| match a.first() {
        Some(value) => Ok(compile_arg(value).into()),
        None => Err("K: expected a kabelsalat expression".to_string()),
    });
    // `pat.K(graph)` sets the control on a pattern, which is the form a tune
    // actually uses — upstream's transpiler rewrites it to `pat.worklet(...)`.
    // The graph is fixed once, so the pattern keeps its own structure.
    crate::bindings::method(pattern, "K", |pat, a| match a.first() {
        Some(value) => Ok(pat.set(compile_arg(value)).into()),
        None => Err("K: expected a kabelsalat expression".to_string()),
    });
}

/// `.out()` with no argument is stereo, as kabelsalat's `[0, 1]` default.
fn out_channels(rest: &[Arg]) -> Vec<usize> {
    match rest.first() {
        Some(Arg::Num(n)) => vec![n.max(0.0) as usize],
        Some(Arg::List(items)) => items
            .iter()
            .filter_map(|v| match v {
                Arg::Num(n) => Some(n.max(0.0) as usize),
                _ => None,
            })
            .collect(),
        _ => vec![0, 1],
    }
}

/// Compile a `K(...)` argument into the pattern that carries it.
///
/// The graph's outputs are whatever `.out()` marked; a patch that never called
/// it — `K(sine(220))` — is read as if the expression itself were the output,
/// which is what makes the shorthand work.
fn compile_arg(value: &Arg) -> rudel_core::Pattern {
    let mut roots = kabelsalat::take_roots();
    if roots.is_empty()
        && let Arg::Kabel(id) = value
    {
        roots = vec![(*id, 0), (*id, 1)];
    }
    let program = rudel_core::control_dyn("worklet", rudel_core::pure(kabelsalat::compile(&roots)));
    let inputs = kabelsalat::take_inputs();
    if inputs.is_empty() {
        return program;
    }
    // The graph itself is fixed, so the structure has to come from the
    // patterns written inside it: `K(sine(S("220 440")).out())` is two notes.
    rudel_core::control_dyn("workletInputs", rudel_core::parray(&inputs)).set(program)
}
