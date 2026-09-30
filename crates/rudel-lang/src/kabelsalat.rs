//! Kabelsalat graphs, compiled to a register program.
//!
//! [Kabelsalat](https://kabel.salat.dev/) builds a modular synth by chaining
//! nodes — `saw(110).lpf(sine(1).range(.3,.8)).out()` — and compiles the graph
//! into a flat list of instructions over a register file. Strudel reaches it
//! through `K(...)`, which its transpiler rewrites into a `worklet(...)`
//! control carrying the *source text*; superdough then evaluates that source,
//! compiles the graph, and posts the result to a `generic-processor`
//! AudioWorklet that runs `new Function(src)` once per sample.
//!
//! Rudel skips the source round-trip. `K(...)` evaluates inline — the node
//! builders in [`crate::bindings::kabelsalat`] are native functions, so the
//! graph is built by the same interpreter that runs the rest of the pattern —
//! and the compiled program travels to the audio thread as plain control
//! [`Value`]s. Nothing is stringified, and nothing is evaluated per sample.
//!
//! The compile step is upstream's, near enough verbatim (`core/compiler.js`):
//! depth-first topological sort, one register per node. Where upstream's
//! backends emit JavaScript or C source (`lib/src/lang/{js,c}.js`), this one
//! emits an instruction list for [`rudel_dsp`]'s interpreter to walk — a third
//! backend in the same slot.
//!
//! Three arrays make up the runtime state, exactly as upstream names them:
//! `r` (one register per node), `o` (output channels, cleared each sample) and
//! `s` (the *previous* sample's outputs, which `src` reads to close a feedback
//! loop).
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod table;

pub use table::{MODULES, NodeType, TYPES, lookup};

/// The table's own `&'static str` for a name, which the arena stores. `None`
/// when the name is not a kabelsalat node type at all.
pub fn type_name(name: &str) -> Option<&'static str> {
    lookup(name).map(|t| t.name)
}

use rudel_core::{Pattern, Value, ValueMap};
use std::sync::{LazyLock, Mutex};

/// An index into the arena. Nodes refer to each other by index rather than by
/// pointer because kabelsalat graphs are *cyclic*: `x => x.delay(.2)` hands a
/// node its own future self, and the cycle is what makes feedback work.
pub type NodeId = usize;

#[derive(Clone)]
struct NodeData {
    kind: &'static str,
    /// The node's own payload — `n`'s number, `cc`'s name, `bytebeat`'s source.
    value: Option<Value>,
    ins: Vec<NodeId>,
}

/// One `K(...)` expression's nodes. Cleared between evaluations.
#[derive(Default)]
struct Arena {
    nodes: Vec<NodeData>,
    /// Nodes `.out()` marked as reaching the speakers, with their channel.
    roots: Vec<(NodeId, usize)>,
    /// Strudel patterns written inside the graph, in the order they were met.
    /// Each becomes a `pat` node reading the value sampled for the hap.
    inputs: Vec<Pattern>,
}

static ARENA: LazyLock<Mutex<Arena>> = LazyLock::new(|| Mutex::new(Arena::default()));

/// Drop every node built so far. Called at the start of each evaluation, next
/// to `reset_slots`, so a re-run does not accumulate the previous graph.
pub fn reset() {
    let mut arena = ARENA.lock().unwrap();
    arena.nodes.clear();
    arena.roots.clear();
    arena.inputs.clear();
}

fn push(kind: &'static str, value: Option<Value>, ins: Vec<NodeId>) -> NodeId {
    let mut arena = ARENA.lock().unwrap();
    arena.nodes.push(NodeData { kind, value, ins });
    arena.nodes.len() - 1
}

/// A constant, as kabelsalat's `n()` node. Constants take a register like any
/// other node; upstream inlines them into the generated source instead, which
/// it can do because it generates source.
pub fn constant(v: f64) -> NodeId {
    push("n", Some(Value::F64(v)), Vec::new())
}

/// A Strudel pattern used as a graph input.
///
/// upstream lifts these out of the source text as `pat[0]`, `pat[1]` … and
/// splices the sampled values back in before compiling, once per hap. Here the
/// placeholder is a node like any other, carrying its own index, and the values
/// ride along in a second control.
pub fn pattern_input(pattern: Pattern) -> NodeId {
    let index = {
        let mut arena = ARENA.lock().unwrap();
        arena.inputs.push(pattern);
        arena.inputs.len() - 1
    };
    get_node_with_value("pat", Value::Int(index as i64), Vec::new())
}

/// Take the patterns this graph read, clearing them for the next `K(...)`.
pub fn take_inputs() -> Vec<Pattern> {
    std::mem::take(&mut ARENA.lock().unwrap().inputs)
}

/// Set a node's inputs after the fact. Feedback needs this: the node has to
/// exist before the closure that reads it can run.
fn set_ins(id: NodeId, ins: Vec<NodeId>) {
    ARENA.lock().unwrap().nodes[id].ins = ins;
}

fn kind_of(id: NodeId) -> &'static str {
    ARENA.lock().unwrap().nodes[id].kind
}

/// The number behind an `n` node, for the expansions that need it at graph
/// time rather than as a signal.
pub fn constant_value(id: NodeId) -> Option<f64> {
    let arena = ARENA.lock().unwrap();
    let node = &arena.nodes[id];
    (node.kind == "n").then(|| node.value.as_ref().and_then(|v| v.as_f64()))?
}

/// The channels of a `poly` node, or `None` for anything else. `mix` and
/// `fork` need to see the spread to fold or widen it.
pub fn poly_channels(id: NodeId) -> Option<Vec<NodeId>> {
    let arena = ARENA.lock().unwrap();
    (arena.nodes[id].kind == "poly").then(|| arena.nodes[id].ins.clone())
}

fn ins_of(id: NodeId) -> Vec<NodeId> {
    ARENA.lock().unwrap().nodes[id].ins.clone()
}

/// An argument to a node builder, before multichannel expansion.
pub enum Arg {
    Node(NodeId),
    /// An array literal, which desugars to a `poly` node — kabelsalat's
    /// multichannel expansion. `sine([220, 330])` is two oscillators.
    Poly(Vec<Arg>),
    /// A function argument, which kabelsalat calls with the node currently
    /// being built. This is how a graph closes a loop back on itself.
    Feedback(Box<dyn Fn(NodeId) -> Arg>),
}

/// kabelsalat's `poly` marker node, holding one input per channel.
fn poly(ids: Vec<NodeId>) -> NodeId {
    push("poly", None, ids)
}

/// Resolve an argument against the node being built, as `Node.parseInput`
/// does. `owner` is that node, which a feedback closure receives.
fn parse_input(arg: &Arg, owner: NodeId) -> NodeId {
    match arg {
        Arg::Node(id) => *id,
        Arg::Poly(items) => {
            // A one-element array is not multichannel, it is just the element.
            if items.len() == 1 {
                return parse_input(&items[0], owner);
            }
            let ids = items.iter().map(|a| parse_input(a, owner)).collect();
            poly(ids)
        }
        Arg::Feedback(f) => parse_input(&f(owner), owner),
    }
}

/// How many channels an argument spreads to, which decides whether the node
/// itself has to be cloned per channel.
fn channels(arg: &Arg) -> usize {
    match arg {
        Arg::Node(id) if kind_of(*id) == "poly" => ins_of(*id).len(),
        Arg::Poly(items) if items.len() > 1 => items.len(),
        // Upstream peeks a feedback function with a throwaway node to see
        // whether it expands. The peeked nodes are dead weight in the arena;
        // they cost a few registers and no samples, which is why upstream does
        // not bother collecting them either.
        Arg::Feedback(f) => {
            let peek = push("peek", None, Vec::new());
            channels(&f(peek))
        }
        _ => 1,
    }
}

/// Build a node of `kind`, expanding multichannel arguments — kabelsalat's
/// `getNode`. `node([a,b], x)` becomes `poly(node(a,x), node(b,x))`, so a
/// stereo chain is written the same way as a mono one.
pub fn get_node(kind: &'static str, args: Vec<Arg>) -> NodeId {
    let expansions = args.iter().map(channels).max().unwrap_or(1);
    if expansions == 1 {
        let id = push(kind, None, Vec::new());
        let ins = args.iter().map(|a| parse_input(a, id)).collect();
        set_ins(id, ins);
        return id;
    }
    let clones = (0..expansions)
        .map(|i| {
            let id = push(kind, None, Vec::new());
            let ins = args
                .iter()
                .map(|arg| {
                    let input = parse_input(arg, id);
                    // Wrap around: a 3-channel node fed a 2-channel argument
                    // reads channels 0, 1, 0.
                    if kind_of(input) == "poly" {
                        let chans = ins_of(input);
                        return chans[i % chans.len()];
                    }
                    input
                })
                .collect();
            set_ins(id, ins);
            id
        })
        .collect();
    poly(clones)
}

/// Resolve a single argument to a node, outside any call — what kabelsalat's
/// `n()` does, which is `Node.parseInput` with nothing to attach a feedback
/// function to.
pub fn resolve(arg: &Arg) -> NodeId {
    let owner = push("peek", None, Vec::new());
    parse_input(arg, owner)
}

/// A node carrying a literal payload — `n`, `cc`, `bytebeat` and friends.
pub fn get_node_with_value(kind: &'static str, value: Value, args: Vec<Arg>) -> NodeId {
    let id = get_node(kind, args);
    ARENA.lock().unwrap().nodes[id].value = Some(value);
    id
}

/// `.out(channels)`: mark a node as reaching the speakers. A poly node spreads
/// across the channels it has, so `sine([220, 330]).out()` is stereo.
pub fn out(id: NodeId, channels: &[usize]) {
    let sources = if kind_of(id) == "poly" {
        ins_of(id)
    } else {
        vec![id]
    };
    if sources.is_empty() || channels.is_empty() {
        return;
    }
    // `.out([0, 1])` is `getNode("output", this, [0, 1])`, so the channel list
    // expands the output node just like any other multichannel argument: one
    // output per channel *or* per source, whichever there are more of, each
    // side wrapping around. A mono node therefore reaches both speakers, and a
    // two-channel node given one channel sums into it.
    let count = sources.len().max(channels.len());
    let mut arena = ARENA.lock().unwrap();
    for i in 0..count {
        arena
            .roots
            .push((sources[i % sources.len()], channels[i % channels.len()]));
    }
}

/// Take the outputs marked so far, clearing them for the next `K(...)`.
pub fn take_roots() -> Vec<(NodeId, usize)> {
    std::mem::take(&mut ARENA.lock().unwrap().roots)
}

// ---------------------------------------------------------------------------
// Compile

/// Depth-first topological sort — upstream's, including the detail that makes
/// feedback work: a node already visited is *not* revisited, so a cycle stops
/// there and the loop reads the register as it stood one sample ago.
fn topo_sort(roots: &[NodeId], nodes: &[NodeData]) -> Vec<NodeId> {
    let mut sorted = Vec::new();
    let mut visited = vec![false; nodes.len()];
    let mut stack: Vec<(NodeId, usize)> = Vec::new();
    for &root in roots {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        stack.push((root, 0));
        while let Some((id, next)) = stack.pop() {
            match nodes[id].ins.get(next) {
                Some(&input) => {
                    stack.push((id, next + 1));
                    if !visited[input] {
                        visited[input] = true;
                        stack.push((input, 0));
                    }
                }
                None => sorted.push(id),
            }
        }
    }
    sorted
}

/// Compile the graph reachable from `roots` into the control value the audio
/// side reads. Four parallel lists, one entry per register:
///
/// - `types`  — the node type, which selects the opcode and any ugen state
/// - `values` — the node's literal payload, or `Null`
/// - `ins`    — one entry per inlet: `Int` reads a register, `F64` is a literal
/// - `outs`   — `[register, channel]` pairs, the graph's speakers
///
/// An inlet the caller left off gets the node type's own default as a literal,
/// which is how upstream's `vars: [freq = 0, phase = 0]` destructuring reads.
///
/// Plain `Value`s rather than a shared struct: `rudel-lang` and `rudel-dsp`
/// only meet through `rudel-core`, and a control map is what already crosses
/// that boundary for every other voice.
pub fn compile(roots: &[(NodeId, usize)]) -> Value {
    // A `poly` that reached the output is a channel spread, already unpacked by
    // `out`; one anywhere else is a stray and compiles as a mix of its inputs.
    let root_ids: Vec<NodeId> = roots.iter().map(|(id, _)| *id).collect();
    let nodes = ARENA.lock().unwrap().nodes.clone();
    let order = topo_sort(&root_ids, &nodes);

    // Register index per node, in the order the interpreter will run them.
    let mut reg = vec![usize::MAX; nodes.len()];
    for (i, &id) in order.iter().enumerate() {
        reg[id] = i;
    }

    let mut types = Vec::with_capacity(order.len());
    let mut values = Vec::with_capacity(order.len());
    let mut ins = Vec::with_capacity(order.len());
    for &id in &order {
        let node = &nodes[id];
        let ty = lookup(node.kind).unwrap_or_else(|| lookup("thru").expect("thru is registered"));
        types.push(Value::Str(ty.name.to_string()));
        values.push(node.value.clone().unwrap_or(Value::Null));

        let mut wired: Vec<Value> = node
            .ins
            .iter()
            .map(|&input| Value::Int(reg[input] as i64))
            .collect();
        // Pad missing inlets with the node's own defaults, which is what
        // upstream's `vars: [freq = 0, phase = 0]` destructuring does.
        if !ty.variadic {
            for &default in ty.defaults.iter().skip(wired.len()) {
                wired.push(Value::F64(f64::from(default)));
            }
        }
        ins.push(Value::List(wired));
    }

    let outs = roots
        .iter()
        .map(|(id, ch)| Value::List(vec![Value::Int(reg[*id] as i64), Value::Int(*ch as i64)]))
        .collect();

    let mut map = ValueMap::new();
    map.insert("types".to_string(), Value::List(types));
    map.insert("values".to_string(), Value::List(values));
    map.insert("ins".to_string(), Value::List(ins));
    map.insert("outs".to_string(), Value::List(outs));
    Value::Map(map)
}
