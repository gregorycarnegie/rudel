//! The kabelsalat node DSL, as Koto values.
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

use super::pattern::KPattern;
use crate::kabelsalat::{self, Arg, MODULES, NodeId, TYPES};
use koto::{
    derive::*,
    prelude::*,
    runtime::{KotoEntries, KotoObject, Result as KotoResult},
};
use rudel_core::Value;
use std::sync::Mutex;

/// A Koto handle on one node of the graph being built.
///
/// Just an index: the nodes live in [`crate::kabelsalat`]'s arena, because a
/// kabelsalat graph is cyclic and a tree of owning handles could not hold one.
#[derive(Clone, Copy, KotoCopy, KotoType)]
pub struct KKabel(pub NodeId);

impl KotoObject for KKabel {
    fn display(&self, ctx: &mut DisplayContext) -> KotoResult<()> {
        ctx.append(format!("kabel({})", self.0));
        Ok(())
    }
}

#[koto_impl]
impl KKabel {}

impl From<KKabel> for KValue {
    fn from(k: KKabel) -> KValue {
        KObject::from(k).into()
    }
}

/// Read one call argument as a graph input.
///
/// Numbers become constant nodes, lists become multichannel `poly` nodes, and a
/// function becomes a feedback edge — kabelsalat calls it with the node being
/// built, so `add(x => x.delay(.2).mul(.8))` wires a node's own output back
/// into it.
fn arg(value: &KValue, vm: &KotoVm) -> Arg {
    match value {
        KValue::Object(o) => Arg::Node(node_from_object(o)),
        KValue::Number(n) => Arg::Node(kabelsalat::constant(f64::from(n))),
        KValue::List(items) => Arg::Poly(items.data().iter().map(|v| arg(v, vm)).collect()),
        KValue::Tuple(items) => Arg::Poly(items.iter().map(|v| arg(v, vm)).collect()),
        KValue::Function(_) | KValue::NativeFunction(_) => {
            let func = value.clone();
            // The closure outlives this call, so it needs a VM of its own.
            let vm = Mutex::new(vm.spawn_shared_vm());
            Arg::Feedback(Box::new(move |owner| {
                let called = vm
                    .lock()
                    .expect("kabelsalat feedback vm")
                    .call_function(func.clone(), CallArgs::Single(KKabel(owner).into()));
                match called {
                    Ok(value) => {
                        // Re-entering `arg` here would need the outer vm; a
                        // feedback function returns a node or a number, and
                        // both are cheap to read directly.
                        match value {
                            KValue::Object(o) => Arg::Node(node_from_object(&o)),
                            KValue::Number(n) => Arg::Node(kabelsalat::constant(f64::from(n))),
                            _ => Arg::Node(kabelsalat::constant(0.0)),
                        }
                    }
                    // A feedback function that throws leaves silence in the
                    // loop rather than failing the whole evaluation.
                    Err(_) => Arg::Node(kabelsalat::constant(0.0)),
                }
            }))
        }
        // Anything else — a string, `null` — is a zero, as `Node.parseInput`
        // falls back to when it does not recognise an input.
        _ => Arg::Node(kabelsalat::constant(0.0)),
    }
}

/// A graph node, a Strudel pattern, or neither.
///
/// A pattern reaching a graph inlet is upstream's `S(...)` placeholder: it is
/// sampled per hap rather than baked into the graph, so it becomes a `pat`
/// node. Mini-notation lands here too, since `m("220 440")` is a pattern by
/// the time the graph sees it — which is exactly the second form upstream's
/// extractor looks for.
fn node_from_object(o: &KObject) -> NodeId {
    if let Ok(k) = o.cast::<KKabel>() {
        return k.0;
    }
    if let Ok(p) = o.cast::<KPattern>() {
        return kabelsalat::pattern_input(p.0.clone());
    }
    kabelsalat::constant(0.0)
}

fn args(values: &[KValue], vm: &KotoVm) -> Vec<Arg> {
    values.iter().map(|v| arg(v, vm)).collect()
}

/// The node types that are not spelled the way they are built: kabelsalat's
/// `module` and alias definitions, which expand into primitives at graph time
/// rather than reaching the interpreter as nodes of their own.
///
/// ponytail: the eleven that patches actually reach for. `fork`, `pan`,
/// `rangex`, `split`, `scope` and the `mouse*` pair are not here yet; they
/// build as unknown types, which the compiler resolves to `thru`. Add one by
/// naming its expansion below — each is two or three lines in `lib.js`.
fn expand(name: &str, mut args: Vec<Arg>) -> Option<NodeId> {
    // `n` marks a constant node, whose value some expansions need at graph time.
    // Pad to `n` arguments with the module's own defaults, since these are
    // JavaScript default parameters rather than compile-time padding.
    let pad = |defaults: &[f64], args: &mut Vec<Arg>| {
        for &d in defaults.iter().skip(args.len()) {
            args.push(Arg::Node(kabelsalat::constant(d)));
        }
    };
    let node = |kind, args| Arg::Node(kabelsalat::get_node(kind, args));
    match name {
        // ar(gate, att, rel) => gate.adsr(att, 0, 1, rel)
        "ar" => {
            pad(&[0.0, 0.02, 0.1], &mut args);
            let (gate, att, rel) = (args.remove(0), args.remove(0), args.remove(0));
            let zero = Arg::Node(kabelsalat::constant(0.0));
            let one = Arg::Node(kabelsalat::constant(1.0));
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
            let zero = Arg::Node(kabelsalat::constant(0.0));
            Some(kabelsalat::get_node(
                "adsr",
                vec![gate, att, dec, zero, release],
            ))
        }
        // perc(gate, decay) => gate.adsr(0, 0, 1, decay)
        "perc" => {
            pad(&[0.0, 0.1], &mut args);
            let (gate, decay) = (args.remove(0), args.remove(0));
            let zero = Arg::Node(kabelsalat::constant(0.0));
            let zero2 = Arg::Node(kabelsalat::constant(0.0));
            let one = Arg::Node(kabelsalat::constant(1.0));
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
            let two = Arg::Node(kabelsalat::constant(2.0));
            let one = Arg::Node(kabelsalat::constant(1.0));
            let scaled = node("mul", vec![args.remove(0), two]);
            Some(kabelsalat::get_node("sub", vec![scaled, one]))
        }
        // unipolar(b) => range(b, 0, 1), which is `(b + 1) * 0.5`.
        "unipolar" => {
            pad(&[0.0], &mut args);
            let zero = Arg::Node(kabelsalat::constant(0.0));
            let one = Arg::Node(kabelsalat::constant(1.0));
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
            let uni = Arg::Node(expand("unipolar", vec![sig])?);
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
            let kind = Arg::Node(kabelsalat::constant(kind));
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
            let one = Arg::Node(kabelsalat::constant(1.0));
            // `mul` is variadic, and upstream passes PI and 0.25 separately:
            // `n(pos).add(1).mul(PI, 0.25)`.
            let pi = Arg::Node(kabelsalat::constant(std::f64::consts::PI));
            let quarter = Arg::Node(kabelsalat::constant(0.25));
            let shifted = node("add", vec![pos, one]);
            let angle = node("mul", vec![shifted, pi, quarter]);
            let left = node("cos", vec![clone_arg(&angle)]);
            let right = node("sin", vec![angle]);
            let spread = Arg::Poly(vec![left, right]);
            Some(kabelsalat::get_node("mul", vec![input, spread]))
        }
        // mix(in, channels): fold a multichannel signal down to one or two.
        // A signal that is not multichannel is already mixed.
        "mix" => {
            pad(&[0.0, 1.0], &mut args);
            let input = args.remove(0);
            let channels = constant_of(&args.remove(0)).unwrap_or(1.0);
            let Arg::Node(id) = input else {
                return None;
            };
            let Some(spread) = kabelsalat::poly_channels(id) else {
                return Some(id);
            };
            if channels < 2.0 {
                return Some(kabelsalat::get_node(
                    "mix",
                    spread.into_iter().map(Arg::Node).collect(),
                ));
            }
            // Two channels: place the inputs evenly across the image and sum.
            // The positions are fixed once the graph is built, so upstream
            // computes the gains here rather than emitting nodes for them.
            let last = (spread.len().max(2) - 1) as f64;
            let panned: Vec<Arg> = spread
                .iter()
                .enumerate()
                .map(|(i, &channel)| {
                    let pos = (i as f64 / last) * 2.0 - 1.0;
                    let deg = (pos + 1.0) * std::f64::consts::FRAC_PI_4;
                    let gains = Arg::Poly(vec![
                        Arg::Node(kabelsalat::constant(deg.cos())),
                        Arg::Node(kabelsalat::constant(deg.sin())),
                    ]);
                    Arg::Node(kabelsalat::get_node("mul", vec![Arg::Node(channel), gains]))
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
fn constant_of(arg: &Arg) -> Option<f64> {
    match arg {
        Arg::Node(id) => kabelsalat::constant_value(*id),
        _ => None,
    }
}

/// Reuse an argument in two places. Only nodes can be shared — a fresh
/// constant is as good as a shared one, and a feedback closure would have to
/// run twice.
fn clone_arg(arg: &Arg) -> Arg {
    match arg {
        Arg::Node(id) => Arg::Node(*id),
        _ => Arg::Node(kabelsalat::constant(0.0)),
    }
}

/// Build a node, taking the module expansions first so `lpf` means `filter`
/// rather than an unknown type.
fn build(name: &str, args: Vec<Arg>, source: Option<String>) -> NodeId {
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
        // and a typo fails at the Koto level before reaching here — as it does
        // upstream, where `Node.prototype` has nothing to call.
        None => kabelsalat::get_node("thru", args),
    }
}

/// The node types whose last argument is source text rather than a signal.
const CODED: &[&str] = &["bytebeat", "floatbeat", "raw", "cc"];

/// Split a coded node's trailing source string off its inlets.
fn take_source(name: &str, values: &[KValue]) -> Option<String> {
    if !CODED.contains(&name) {
        return None;
    }
    match values.last() {
        Some(KValue::Str(text)) => Some(text.to_string()),
        _ => None,
    }
}

/// `expand` consumes its arguments, so hand it the node ids and keep the
/// originals for the ordinary path.
fn args_for_expand(args: &[Arg]) -> Vec<Arg> {
    args.iter().map(clone_arg).collect()
}

/// Register `Kabel.<name>` for every node type, and the same names as methods.
pub(crate) fn register(prelude: &KMap) {
    let map = KMap::new();
    let Some(entries) = KKabel(0).entries() else {
        return;
    };
    let names: Vec<&'static str> = TYPES
        .iter()
        .map(|t| t.name)
        .chain(MODULES.iter().copied())
        .collect();

    for name in names {
        // As a function: `sine(220)`.
        map.add_fn(name, move |ctx| {
            let source = take_source(name, ctx.args());
            let taken = ctx.args().len() - usize::from(source.is_some());
            let built = build(name, args(&ctx.args()[..taken], ctx.vm), source);
            Ok(KKabel(built).into())
        });
        // As a method: `impulse(4).sine()`, the receiver sliding in first.
        entries.insert(
            name,
            KValue::NativeFunction(KNativeFunction::new(move |ctx| {
                let (this, rest) = instance_and_args(ctx)?;
                let source = take_source(name, &rest);
                let taken = rest.len() - usize::from(source.is_some());
                let mut all = vec![Arg::Node(this)];
                all.extend(args(&rest[..taken], ctx.vm));
                Ok(KKabel(build(name, all, source)).into())
            })),
        );
    }

    // `.out(channels)`: mark this node as reaching the speakers. Returns the
    // node, as upstream does, so a chain can carry on past it.
    entries.insert(
        "out",
        KValue::NativeFunction(KNativeFunction::new(|ctx| {
            let (this, rest) = instance_and_args(ctx)?;
            let channels = out_channels(&rest);
            kabelsalat::out(this, &channels);
            Ok(KKabel(this).into())
        })),
    );
    // `.apply(f)`: hand the node to a function, which is how a patch uses one
    // node twice without naming it.
    entries.insert(
        "apply",
        KValue::NativeFunction(KNativeFunction::new(|ctx| {
            let (this, rest) = instance_and_args(ctx)?;
            let Some(f) = rest.first() else {
                return Ok(KKabel(this).into());
            };
            ctx.vm
                .spawn_shared_vm()
                .call_function(f.clone(), CallArgs::Single(KKabel(this).into()))
        })),
    );

    prelude.insert("Kabel", map);
    // `K(graph)` on its own is a control pattern, to be combined like any
    // other.
    prelude.add_fn("K", |ctx| {
        let Some(value) = ctx.args().first() else {
            return runtime_error!("K: expected a kabelsalat expression");
        };
        Ok(KPattern(compile_arg(value)).into())
    });
    // `pat.K(graph)` sets the control on a pattern, which is the form a tune
    // actually uses — upstream's transpiler rewrites it to `pat.worklet(...)`.
    // The graph is fixed once, so the pattern keeps its own structure.
    if let Some(entries) = KPattern(rudel_core::silence()).entries() {
        entries.insert(
            "K",
            KValue::NativeFunction(KNativeFunction::new(|ctx| {
                let (this, rest) = pattern_and_args(ctx)?;
                let Some(value) = rest.first() else {
                    return runtime_error!("K: expected a kabelsalat expression");
                };
                Ok(KPattern(this.set(compile_arg(value))).into())
            })),
        );
    }
}

/// The receiving pattern and the remaining arguments of `pat.K(...)`.
fn pattern_and_args(
    ctx: &mut koto::runtime::CallContext,
) -> KotoResult<(rudel_core::Pattern, Vec<KValue>)> {
    use koto::runtime::{ErrorKind, runtime_error};
    match ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KPattern::type_static())? {
        (KValue::Object(o), rest) => match o.cast::<KPattern>() {
            Ok(p) => Ok((p.0.clone(), rest.to_vec())),
            Err(_) => runtime_error!(ErrorKind::UnexpectedError),
        },
        _ => runtime_error!(ErrorKind::UnexpectedError),
    }
}

/// `.out()` with no argument is stereo, as kabelsalat's `[0, 1]` default.
fn out_channels(rest: &[KValue]) -> Vec<usize> {
    match rest.first() {
        Some(KValue::Number(n)) => vec![f64::from(n).max(0.0) as usize],
        Some(KValue::List(items)) => items
            .data()
            .iter()
            .filter_map(|v| match v {
                KValue::Number(n) => Some(f64::from(n).max(0.0) as usize),
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
fn compile_arg(value: &KValue) -> rudel_core::Pattern {
    let mut roots = kabelsalat::take_roots();
    if roots.is_empty()
        && let KValue::Object(o) = value
        && let Ok(k) = o.cast::<KKabel>()
    {
        roots = vec![(k.0, 0), (k.0, 1)];
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

/// The receiver and the remaining arguments of a runtime-registered method.
fn instance_and_args(ctx: &mut koto::runtime::CallContext) -> KotoResult<(NodeId, Vec<KValue>)> {
    use koto::runtime::{ErrorKind, runtime_error};
    match ctx.instance_and_args(|i| matches!(i, KValue::Object(_)), KKabel::type_static())? {
        (KValue::Object(o), rest) => match o.cast::<KKabel>() {
            Ok(k) => Ok((k.0, rest.to_vec())),
            Err(_) => runtime_error!(ErrorKind::UnexpectedError),
        },
        _ => runtime_error!(ErrorKind::UnexpectedError),
    }
}
