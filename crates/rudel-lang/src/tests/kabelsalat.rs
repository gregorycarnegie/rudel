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
    assert!(err.contains("not a callable function"), "{err}");
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

// --- module expansions and argument shapes ----------------------------------

/// The constants a program carries, in register order.
fn constants(types: &[String], values: &[Value]) -> Vec<f64> {
    types
        .iter()
        .zip(values)
        .filter(|(t, _)| *t == "n")
        .filter_map(|(_, v)| v.as_f64())
        .collect()
}

#[test]
fn every_module_expands_into_its_primitives() {
    // A module whose arm went missing would build as an unknown type, which
    // the compiler resolves to `thru` — so each is pinned by a primitive only
    // its own expansion produces.
    for (src, wants) in [
        ("K(Kabel.impulse(2).ar(0.1, 0.2).out())", &["adsr"][..]),
        ("K(Kabel.lfnoise(4).out())", &["noise", "impulse", "hold"]),
        ("K(Kabel.sine(1).bipolar().out())", &["mul", "sub"]),
        ("K(Kabel.sine(1).unipolar().out())", &["range"]),
        ("K(Kabel.sine(1).rangex(100, 1000).out())", &["log", "exp"]),
        ("K(Kabel.midin().out())", &["MidiIn"]),
        ("K(Kabel.rng().out())", &["lcgnoise"]),
        ("K(Kabel.sine(1).gt(0.5).out())", &["greater"]),
        ("K(Kabel.sine(1).lt(0.5).out())", &["lower"]),
    ] {
        let (types, ..) = program(src);
        for want in wants {
            assert!(types.iter().any(|t| t == want), "{src}: {types:?}");
        }
        assert!(!types.iter().any(|t| t == "thru"), "{src}: {types:?}");
    }
}

#[test]
fn each_biquad_preset_nails_down_its_own_filter_type() {
    // `qf`'s second inlet is the filter type, 0..=4 in preset order. The q is
    // chosen to collide with none of them.
    for (name, kind) in [
        ("qlpf", 0.0),
        ("qhpf", 1.0),
        ("qbpf", 2.0),
        ("qnf", 3.0),
        ("qapf", 4.0),
    ] {
        let src = format!("K(Kabel.saw(110).{name}(800, 7).out())");
        let (types, values, ..) = program(&src);
        assert!(types.iter().any(|t| t == "qf"), "{src}: {types:?}");
        assert_eq!(
            constants(&types, &values),
            [110.0, kind, 800.0, 7.0],
            "{src}"
        );
    }
}

#[test]
fn fork_copies_the_signal_onto_as_many_channels_as_asked() {
    // Three copies reaching a stereo `.out()` make three outputs, one per
    // source; an unknown `fork` would be a mono `thru` heard twice.
    let (_, _, _, outs) = program("K(Kabel.sine(220).fork(3).out())");
    assert_eq!(outs.len(), 3, "{outs:?}");
}

#[test]
fn out_takes_one_channel_or_a_list_of_them() {
    let channels = |outs: &[Value]| -> Vec<i64> {
        outs.iter()
            .filter_map(|o| match o {
                Value::List(pair) => pair[1].as_f64().map(|c| c as i64),
                _ => None,
            })
            .collect()
    };
    let (_, _, _, outs) = program("K(Kabel.sine(220).out(1))");
    assert_eq!(channels(&outs), [1]);
    let (_, _, _, outs) = program("K(Kabel.sine(220).out([1]))");
    assert_eq!(channels(&outs), [1]);
    // An empty list marks nothing, which leaves the expression as its own
    // stereo output rather than dividing by a channel count of zero.
    let (_, _, _, outs) = program("K(Kabel.sine(220).out([]))");
    assert_eq!(channels(&outs), [0, 1]);
}

#[test]
fn a_feedback_function_may_return_a_number_or_a_multichannel_node() {
    // A number closes the loop with that constant rather than the silence a
    // throwing function leaves.
    let (types, values, ..) = program("K(Kabel.sine(220).add(x => 0.25).out())");
    assert!(
        constants(&types, &values).contains(&0.25),
        "{types:?} {values:?}"
    );
    // A function returning a two-channel node expands its owner, as any other
    // multichannel argument does.
    let (types, ..) = program("K(Kabel.sine(220).add(x => x.mul([0.5, 0.25])).out())");
    assert_eq!(types.iter().filter(|t| *t == "add").count(), 2, "{types:?}");
}

#[test]
fn an_empty_array_argument_is_one_channel_not_none() {
    let (types, ..) = program("K(Kabel.sine([]).out())");
    assert!(types.iter().any(|t| t == "sine"), "{types:?}");
}

#[test]
fn a_coded_node_keeps_its_source_and_floatbeat_builds_a_bytebeat() {
    let (types, values, ..) = program(r#"K(Kabel.bytebeat("t*2").out())"#);
    let at = types
        .iter()
        .position(|t| t == "bytebeat")
        .expect("a bytebeat node");
    assert_eq!(values[at].as_str(), Some("t*2"), "{values:?}");
    // `floatbeat` is a `bytebeat` node upstream; `raw` stays itself.
    let (types, ..) = program(r#"K(Kabel.floatbeat("t/8").out())"#);
    assert!(types.iter().any(|t| t == "bytebeat"), "{types:?}");
    assert!(!types.iter().any(|t| t == "floatbeat"), "{types:?}");
    let (types, ..) = program(r#"K(Kabel.raw("x").out())"#);
    assert!(types.iter().any(|t| t == "raw"), "{types:?}");
}

#[test]
fn an_evaluation_starts_from_an_empty_graph() {
    // A script that marks an output and then fails leaves a root in the
    // arena; the next evaluation must not compile it into its own graph.
    let _ = eval("Kabel.sine(220).out(); throw new Error('stop')");
    let (types, ..) = program("K(Kabel.saw(110))");
    assert_eq!(types, ["n", "saw"], "{types:?}");
}

#[test]
fn the_pass_finds_k_only_as_a_name_of_its_own() {
    // `MK(...)` is some other function ending in K; `K (...)` with a space is
    // still the call.
    let other = preprocess_strudel("const MK = x => x; MK(sine(1))");
    assert!(!other.contains("Kabel."), "{other}");
    let spaced = preprocess_strudel("K (sine(1).out())");
    assert!(spaced.contains("Kabel.sine(1)"), "{spaced}");
}

#[test]
fn a_name_ending_the_k_argument_is_still_scoped() {
    // The last identifier runs to the end of the region rather than to a
    // following non-name character.
    assert_eq!(preprocess_strudel("K(1 + sGate)"), "K(1 + Kabel.sgate())");
}

#[test]
fn an_inlet_left_off_takes_kabelsalat_s_default_including_the_negative_ones() {
    // The defaults land in the compiled `ins` as literals; the first inlet,
    // where given, is a register and is skipped.
    for (src, node, skip, want) in [
        ("K(Kabel.midifreq().out())", "midifreq", 0, &[-1.0][..]),
        ("K(Kabel.midigate().out())", "midigate", 0, &[-1.0]),
        ("K(Kabel.midivel().out())", "midivel", 0, &[-1.0]),
        ("K(Kabel.midicc().out())", "midicc", 0, &[-1.0, -1.0]),
        ("K(Kabel.sine(1).clamp().out())", "clamp", 1, &[-1.0, 1.0]),
        (
            "K(Kabel.sine(1).remap().out())",
            "remap",
            1,
            &[-1.0, 1.0, -1.0, 1.0],
        ),
        ("K(Kabel.sine(1).clip().out())", "clip", 1, &[-1.0, 1.0]),
        ("K(Kabel.sine(1).trig().out())", "trig", 1, &[-1.0, 1.0]),
    ] {
        let (types, _, ins, _) = program(src);
        let at = types
            .iter()
            .position(|t| t == node)
            .unwrap_or_else(|| panic!("{src}: {types:?}"));
        let Value::List(inlets) = &ins[at] else {
            panic!("{src}: {ins:?}");
        };
        let got: Vec<f64> = inlets[skip..].iter().filter_map(Value::as_f64).collect();
        assert_eq!(got, want, "{src}: {inlets:?}");
    }
}

#[test]
fn a_stereo_mix_spreads_the_channels_evenly_across_the_image() {
    // Three channels sit at -1, 0 and +1; the middle one gets equal-power
    // gains of 1/sqrt(2) on both sides.
    let (types, values, ..) = program("K(Kabel.sine([220, 330, 440]).mix(2).out())");
    let centred = constants(&types, &values)
        .into_iter()
        .filter(|g| (g - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12)
        .count();
    assert_eq!(centred, 2, "{values:?}");
    // One channel's worth of output folds into a single `mix`.
    let (types, ..) = program("K(Kabel.sine([220, 330]).mix().out())");
    assert!(types.iter().any(|t| t == "mix"), "{types:?}");
}
