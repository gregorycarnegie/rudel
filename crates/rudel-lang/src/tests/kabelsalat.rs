//! `K(...)`: kabelsalat graphs, built and compiled.

use super::common::*;

/// The compiled program a `K(...)` pattern carries, as the four parallel lists
/// the audio side reads.
fn program(src: &str) -> (Vec<String>, Vec<Value>, Vec<Value>, Vec<Value>) {
    let pat = eval(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    let Some(Value::Map(controls)) = values(&pat, 0, 1).into_iter().next() else {
        panic!("{src} should evaluate to a control map");
    };
    let Some(Value::Map(map)) = controls.get("worklet") else {
        panic!("{src} should carry a `worklet` control, got {controls:?}");
    };
    let list = |key: &str| match map.get(key) {
        Some(Value::List(items)) => items.clone(),
        other => panic!("`{key}` should be a list, got {other:?}"),
    };
    let types = list("types")
        .iter()
        .map(|t| t.as_str().unwrap_or_default().to_string())
        .collect();
    (types, list("values"), list("ins"), list("outs"))
}

#[test]
fn a_graph_compiles_to_its_nodes_in_topological_order() {
    // `sine(220).mul(0.5).out()` — every input has to land in a register
    // before the node that reads it, which is the whole job of the sort.
    let (types, values, ins, outs) = program("K(Kabel.sine(220).mul(0.5).out())");
    assert_eq!(types, ["n", "sine", "n", "mul"], "{types:?}");
    assert_eq!(values[0].as_f64(), Some(220.0));
    assert_eq!(values[2].as_f64(), Some(0.5));

    // `sine` reads register 0 and takes kabelsalat's defaults for the two
    // inlets the patch left off.
    let Value::List(sine_ins) = &ins[1] else {
        panic!("{ins:?}");
    };
    assert_eq!(sine_ins.len(), 3, "sine has three inlets: {sine_ins:?}");
    assert!(matches!(sine_ins[0], Value::Int(0)));
    assert!(matches!(sine_ins[1], Value::F64(f) if f == 0.0));

    // `.out()` with no argument is stereo, so both channels take the `mul`.
    assert_eq!(outs.len(), 2, "{outs:?}");
}

#[test]
fn a_method_call_slides_the_receiver_in_as_the_first_argument() {
    // kabelsalat's `register` puts every node on the prototype, so
    // `impulse(4).sine()` and `sine(impulse(4))` are the same graph.
    let (chained, ..) = program("K(Kabel.impulse(4).sine().out())");
    let (nested, ..) = program("K(Kabel.sine(Kabel.impulse(4)).out())");
    assert_eq!(chained, nested, "{chained:?} vs {nested:?}");
    assert_eq!(chained, ["n", "impulse", "sine"]);
}

#[test]
fn an_array_argument_expands_the_node_per_channel() {
    // `sine([220, 330])` is two oscillators, not one — kabelsalat's
    // multichannel expansion, and the reason a stereo patch reads like a mono
    // one.
    let (types, _, _, outs) = program("K(Kabel.sine([220, 330]).out())");
    assert_eq!(
        types.iter().filter(|t| *t == "sine").count(),
        2,
        "{types:?}"
    );
    // A poly node reaching `.out()` spreads across the channels rather than
    // stacking on one, so each oscillator gets its own.
    assert_eq!(outs.len(), 2, "{outs:?}");
    let channels: Vec<i64> = outs
        .iter()
        .filter_map(|o| match o {
            Value::List(pair) => pair[1].as_f64().map(|c| c as i64),
            _ => None,
        })
        .collect();
    assert_eq!(channels, [0, 1]);
}

#[test]
fn a_module_expands_into_the_primitives_it_is_made_of() {
    // `perc` is not a node type: kabelsalat defines it as
    // `gate.adsr(0, 0, 1, decay)`, expanded while the graph is built.
    let (types, ..) = program("K(Kabel.impulse(4).perc(0.1).out())");
    assert!(types.contains(&"adsr".to_string()), "{types:?}");
    assert!(!types.contains(&"perc".to_string()), "{types:?}");

    // `lpf` is `filter` under another name.
    let (types, ..) = program("K(Kabel.saw(110).lpf(0.5).out())");
    assert!(types.contains(&"filter".to_string()), "{types:?}");
}

#[test]
fn a_feedback_function_closes_the_loop_without_repeating_the_node() {
    // `x => x.delay(...)` hands the node its own self. The cycle must appear
    // once in the sort — a second visit would loop forever.
    let (types, ..) = program("K(Kabel.sine(220).add(x => x.delay(0.2).mul(0.8)).out())");
    assert_eq!(types.iter().filter(|t| *t == "add").count(), 1, "{types:?}");
    assert!(types.contains(&"delay".to_string()), "{types:?}");
}

#[test]
fn an_expression_without_out_is_its_own_output() {
    // Strudel's transpiler calls a block-bodied `K(() => {...})` so it can use
    // `out()`; the expression form usually does not bother, so the expression
    // itself has to be the output.
    let (types, _, _, outs) = program("K(Kabel.sine(220))");
    assert_eq!(types, ["n", "sine"]);
    assert_eq!(
        outs.len(),
        2,
        "a bare expression is heard in stereo: {outs:?}"
    );
}

#[test]
fn each_call_compiles_only_its_own_graph() {
    // `.out()` marks roots in a shared arena, so a second `K(...)` in the same
    // script must not inherit the first one's outputs.
    let (types, ..) =
        program("let a = K(Kabel.sine(220).out())\nlet b = K(Kabel.saw(110).out())\nb");
    assert_eq!(types, ["n", "saw"], "{types:?}");
}

#[test]
fn a_node_type_without_an_interpreter_still_compiles() {
    // `midigate` is a real kabelsalat node this port has no DSP for: the MIDI
    // nodes read a live input device. It has to reach the audio side as
    // itself, because the interpreter is what falls back to `thru` for a type
    // it does not know (upstream's `fallbackType`) — so a patch using one
    // plays quietly wrong rather than not at all.
    let (types, ..) = program("K(Kabel.saw(110).midigate().out())");
    assert_eq!(types, ["n", "saw", "midigate"], "{types:?}");
}

#[test]
fn a_misspelt_node_is_an_error_rather_than_silence() {
    // Upstream has nothing on `Node.prototype` for a typo either, and throws.
    // Reporting it beats compiling a graph that quietly drops the call.
    let Err(err) = eval("K(Kabel.sine(220).notAKabelsalatNode().out())") else {
        panic!("a name that is not a node should not evaluate");
    };
    assert!(err.contains("notAKabelsalatNode"), "{err}");
}

// --- the preprocessor pass -------------------------------------------------

#[test]
fn a_patch_written_with_bare_names_reaches_the_graph() {
    // The spelling every kabelsalat patch in the wild uses. Nothing is
    // qualified, and it has to build the same graph as the explicit form.
    let (bare, ..) = program("K(saw(110).lpf(0.5).out())");
    let (qualified, ..) = program("K(Kabel.saw(110).lpf(0.5).out())");
    assert_eq!(bare, qualified, "{bare:?} vs {qualified:?}");
    assert_eq!(bare, ["n", "saw", "n", "filter"]);
}

#[test]
fn only_kabelsalat_names_are_qualified() {
    // `sine` is kabelsalat's inside `K(...)`; `mul` is a method and needs no
    // scope; `foo` is not a node at all and must stay reachable, since that is
    // how a patch borrows a cutoff or a tempo from the script around it.
    let scoped = preprocess_strudel("K(sine(220).mul(foo(1)).out())");
    assert!(scoped.contains("Kabel.sine(220)"), "{scoped}");
    assert!(scoped.contains(".mul("), "{scoped}");
    assert!(!scoped.contains("Kabel.mul"), "{scoped}");
    assert!(!scoped.contains("Kabel.foo"), "{scoped}");
}

#[test]
fn nothing_outside_a_k_call_is_touched() {
    // `sine` and `saw` are Strudel's own signals at the top level. Qualifying
    // one of those would break patterns that never mentioned kabelsalat.
    let src = r#"note("c e g").lpf(sine.range(200, 2000))"#;
    assert_eq!(preprocess_strudel(src), preprocess_strudel(src));
    assert!(!preprocess_strudel(src).contains("Kabel."));
}

#[test]
fn a_name_inside_a_string_is_left_alone() {
    // The pass scans the code mask, so mini-notation naming a sample `saw`
    // stays a sample name.
    let scoped = preprocess_strudel(r#"K(sine(220).out()) ; s("saw sine")"#);
    assert!(scoped.contains("saw sine"), "{scoped}");
    assert!(!scoped.contains("Kabel.saw sine"), "{scoped}");
}

#[test]
fn the_pass_is_idempotent() {
    // An already-qualified name is behind a `.`, so a second run must not
    // produce `Kabel.Kabel.sine`.
    let once = preprocess_strudel("K(sine(220).out())");
    assert!(!once.contains("Kabel.Kabel"), "{once}");
}

#[test]
fn a_pattern_inside_the_graph_becomes_a_per_hap_input() {
    // `S(x)` is upstream's marker for a Strudel pattern inside the graph. The
    // marker unwraps, and the pattern becomes a `pat` node sampled per hap
    // rather than a constant baked into the graph.
    let (types, values, ..) = program(r#"K(sine(S("220 440")).out())"#);
    assert!(types.contains(&"pat".to_string()), "{types:?}");
    let index = types.iter().position(|t| t == "pat").expect("a pat node");
    assert_eq!(
        values[index].as_f64(),
        Some(0.0),
        "the first input is index 0"
    );

    // Two patterns index in the order they are written.
    let (types, values, ..) = program(r#"K(sine(S("220 440")).mul(S("0.2 0.8")).out())"#);
    let indices: Vec<f64> = types
        .iter()
        .zip(&values)
        .filter(|(t, _)| *t == "pat")
        .filter_map(|(_, v)| v.as_f64())
        .collect();
    assert_eq!(indices, [0.0, 1.0], "{types:?}");
}

#[test]
fn a_patterned_graph_takes_its_structure_from_the_pattern() {
    // The graph is fixed, so `K(sine(S("220 440")).out())` has to be two
    // events a cycle — the pattern's, not the graph's.
    let pat = eval(r#"K(sine(S("220 440")).out())"#).expect("evaluates");
    let haps = values(&pat, 0, 1);
    assert_eq!(haps.len(), 2, "{haps:?}");
    let Value::Map(first) = &haps[0] else {
        panic!("{haps:?}");
    };
    // Both controls ride along: the graph once, the sampled values per hap.
    assert!(first.get("worklet").is_some(), "{first:?}");
    let Some(Value::List(inputs)) = first.get("workletInputs") else {
        panic!("no workletInputs: {first:?}");
    };
    assert_eq!(inputs[0].as_f64(), Some(220.0), "{inputs:?}");
}

#[test]
fn the_haps_own_frequency_and_gate_are_node_calls() {
    // upstream splices `sFreq` and `sGate` into the source text before
    // compiling. They are nodes here, so a graph can read them like any input.
    let (types, ..) = program("K(sine(sFreq).mul(sGate).out())");
    assert!(types.contains(&"sfreq".to_string()), "{types:?}");
    assert!(types.contains(&"sgate".to_string()), "{types:?}");
}
