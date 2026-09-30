use super::common::*;

// --- Transpilation / preprocessing parity -------------------------------------

#[test]
fn preprocess_flattens_alignment_getters() {
    assert_eq!(preprocess_strudel("p.add.out(1)"), "p.add_out(1)");
    // `in` is the default alignment and *is* the plain method
    assert_eq!(preprocess_strudel("p.mul.in(1)"), "p.mul(1)");
    // spelling normalisation: `mod` is bound as `modulo`, and the camelCase and
    // `squeezein` forms are the same cell
    assert_eq!(preprocess_strudel("p.mod.poly(1)"), "p.modulo_poly(1)");
    assert_eq!(preprocess_strudel("p.add.squeezeIn(1)"), "p.add_squeeze(1)");
    assert_eq!(
        preprocess_strudel("p.set.squeezeOut(1)"),
        "p.set_squeezeout(1)"
    );
    // the alignment has to be applied — a chain that merely reads that way is
    // not an alignment, and neither is a string
    assert_eq!(preprocess_strudel("p.add.output"), "p.add.output");
    assert_eq!(
        preprocess_strudel(r#"note("add.out(1)")"#),
        r#"note(m("add.out(1)", 6))"#
    );
}

#[test]
fn empty_or_commented_out_script_falls_back_to_silence() {
    assert_eq!(preprocess_strudel(""), "silence");
    assert_eq!(preprocess_strudel("   \n  \n"), "silence");
    assert_eq!(preprocess_strudel("// just a comment\n"), "silence");
    // and it evaluates to an actually-empty pattern
    let pat = eval("// nothing here\n").expect("eval");
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn preprocess_annotates_mini_notation_with_its_source_offsets() {
    let result = preprocess_strudel_with_meta(r#"s("bd sd").note("c e")"#);
    assert_eq!(result.source, r#"s(m("bd sd", 3)).note(m("c e", 17))"#);
}

#[test]
fn eval_result_carries_editor_metadata() {
    let result = eval_result(r#"s("bd sd")"#).expect("eval");
    assert!(result.meta.widgets.is_empty());
}

#[test]
fn preprocess_rewrites_slider_widgets_like_strudel() {
    let result = preprocess_strudel_with_meta("slider(0.5, 0, 1, 0.01)");

    assert_eq!(result.source, r#"slider_with_id("7:10", 0.5, 0, 1, 0.01)"#);
    assert_eq!(result.widgets.len(), 1);

    let widget = &result.widgets[0];
    assert_eq!(widget.widget_type, "slider");
    assert_eq!(widget.id, "7:10");
    assert_eq!((widget.from, widget.to), (7, 10));
    assert_eq!(widget.index, 0);
    assert_eq!(widget.value.as_deref(), Some("0.5"));
    assert_eq!(widget.min, Some(0.0));
    assert_eq!(widget.max, Some(1.0));
    assert_eq!(widget.step, Some(0.01));
}

#[test]
fn preprocess_keeps_sliders_from_every_statement() {
    // Two sliders in two labeled statements (like a live-coding session with
    // several patterns) must both survive preprocessing with distinct ids and
    // ranges pointing at their own literals.
    let src =
        "bass: n(\"0\").lpf(slider(400, 300, 2000))\n\narp: n(\"1\").lpenv(slider(3.5, 1.25, 6))";
    let result = preprocess_strudel_with_meta(src);

    let sliders: Vec<_> = result
        .widgets
        .iter()
        .filter(|w| w.widget_type == "slider")
        .collect();
    assert_eq!(sliders.len(), 2, "both sliders should be kept");
    assert_ne!(sliders[0].id, sliders[1].id);
    assert_eq!(&src[sliders[0].from..sliders[0].to], "400");
    assert_eq!(&src[sliders[1].from..sliders[1].to], "3.5");
}

#[test]
fn slider_scanner_ignores_strings_comments_and_method_calls() {
    let result = preprocess_strudel_with_meta(
        r#"
// slider(0.1)
s("slider(0.2)")
foo.slider(0.3)
slider(0.4)
"#,
    );

    assert_eq!(result.widgets.len(), 1);
    let widget = &result.widgets[0];
    assert_eq!(widget.value.as_deref(), Some("0.4"));
    assert!(result.source.contains(r#"s(m("slider(0.2)","#));
    assert!(result.source.contains("foo.slider(0.3)"));
    assert!(result.source.contains(r#"slider_with_id(""#));
}

#[test]
fn public_visualizer_names_rewrite_to_inline_widget() {
    // The public `pianoroll` / `pitchwheel` / `wordfall` spellings create the
    // same widget (canonical `_`-prefixed type, rewritten to the same host
    // call) as their `_`-prefixed inline variants.
    for (call, widget_type, host) in [
        ("pianoroll", "_pianoroll", "rudel_widget_pianoroll"),
        ("punchcard", "_punchcard", "rudel_widget_punchcard"),
        ("spiral", "_spiral", "rudel_widget_spiral"),
        ("pitchwheel", "_pitchwheel", "rudel_widget_pitchwheel"),
        ("wordfall", "_wordfall", "rudel_widget_wordfall"),
        ("scope", "_scope", "rudel_widget_scope"),
        ("tscope", "_scope", "rudel_widget_scope"),
        ("fscope", "_fscope", "rudel_widget_fscope"),
        ("spectrum", "_spectrum", "rudel_widget_spectrum"),
        ("claviature", "_claviature", "rudel_widget_claviature"),
    ] {
        let result = preprocess_strudel_with_meta(&format!(r#"s("bd sd").{call}()"#));
        assert_eq!(result.widgets.len(), 1, "{call}");
        assert_eq!(result.widgets[0].widget_type, widget_type, "{call}");
        assert!(result.source.contains(host), "{call}: {}", result.source);
    }
}

#[test]
fn a_shader_body_survives_its_newlines_and_commas() {
    // A WGSL body is a multi-line single-quoted string full of top-level-looking
    // commas. Single quotes keep it out of mini-notation; the option scanner has
    // to keep it out of its own comma split, and hand it back byte for byte --
    // including the leading newline, which the reported error line counts from.
    let body = "
let a = 1.0;
return vec4<f32>(a, 0.5, 0.0, 1.0);
";
    let script = format!("s(\"bd*4\").shader({{ code: '{body}' }})");
    let result = preprocess_strudel_with_meta(&script);
    assert_eq!(result.widgets.len(), 1);
    assert_eq!(result.widgets[0].widget_type, "_shader");
    assert_eq!(
        result.widgets[0].options.get("code"),
        Some(&crate::WidgetOption::String(body.to_string())),
        "the shader body reached the widget changed"
    );
}

#[test]
fn slider_drags_reach_already_evaluated_patterns() {
    // The editor's slider drag calls `set_slider_value` without re-evaluating;
    // the playing pattern's signal closure must read the new value on its next
    // query (Strudel's realtime slider behavior).
    let result = eval_result("s(\"bd\").lpf(slider(725, 300, 2000))").expect("eval");
    let id = result.meta.widgets[0].id.clone();

    let before: Vec<_> = result
        .pattern
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter_map(|hap| match &hap.value {
            Value::Map(map) => map.get("cutoff").cloned(),
            _ => None,
        })
        .collect();
    assert!(before.contains(&Value::F64(725.0)), "got {before:?}");

    assert!(crate::set_slider_value(&id, 1400.0));
    let after: Vec<_> = result
        .pattern
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter_map(|hap| match &hap.value {
            Value::Map(map) => map.get("cutoff").cloned(),
            _ => None,
        })
        .collect();
    assert!(after.contains(&Value::F64(1400.0)), "got {after:?}");
}

#[test]
fn eval_result_carries_slider_widget_metadata() {
    let result = eval_result("slider(0.5, 0, 1)").expect("eval");

    assert_eq!(result.meta.widgets.len(), 1);
    let widget = &result.meta.widgets[0];
    assert_eq!(widget.widget_type, "slider");
    assert_eq!(widget.value.as_deref(), Some("0.5"));
    assert_eq!(values(&result.pattern, 0, 1), vec![Value::F64(0.5)]);
}

#[test]
fn block_eval_metadata_uses_absolute_source_ranges() {
    let result =
        eval_result_with_source_range(r#"note("c")._spiral()"#, (20, 39)).expect("block eval");

    assert_eq!(result.meta.widgets.len(), 1);
    let widget = &result.meta.widgets[0];
    assert_eq!(widget.widget_type, "_spiral");
    assert_eq!((widget.from, widget.to), (20, 39));
    assert!(widget.id.ends_with("_20-39"));
    assert_eq!(
        result
            .pattern
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .flat_map(|hap| hap.context.locations)
            .collect::<Vec<_>>(),
        vec![(26, 27)]
    );
}

#[test]
fn block_eval_slider_ids_use_absolute_source_ranges() {
    let result = eval_result_with_source_range("slider(0.5, 0, 1)", (40, 57)).expect("block eval");

    let widget = &result.meta.widgets[0];
    assert_eq!(widget.widget_type, "slider");
    assert_eq!(widget.id, "47:50");
    assert_eq!((widget.from, widget.to), (47, 50));
}

#[test]
fn mini_locations_stay_aligned_when_a_slider_precedes_a_pattern() {
    // The slider rewrite lengthens the source before mini-notation offsets are
    // recorded, so offsets after it must be mapped back to original positions
    // (both in the metadata and in the `m(literal, offset)` runtime locations).
    let script = r#"note("c").lpf(slider(0.5)).s("bd")"#;
    let result = preprocess_strudel_with_meta(script);

    // The offsets baked into the `m(...)` calls point back at the originals.
    assert!(result.source.contains(r#"m("c", 6)"#), "{}", result.source);
    assert!(
        result.source.contains(r#"m("bd", 30)"#),
        "{}",
        result.source
    );
    assert_eq!(&script[6..7], "c");
    assert_eq!(&script[30..32], "bd");

    // The runtime hap locations (embedded by `m(...)`) match the originals too.
    let pattern = eval(script).expect("eval");
    let locations: Vec<_> = pattern
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .flat_map(|hap| hap.context.locations)
        .collect();
    assert!(locations.contains(&(30, 32)), "got {locations:?}");
}

#[test]
fn slider_with_id_reads_live_registry_at_query_time() {
    let result = eval_result("          slider(0.5, 0, 1)").expect("eval");
    let id = result.meta.widgets[0].id.clone();

    assert_eq!(slider_value(&id).and_then(|v| v.as_f64()), Some(0.5));
    assert_eq!(values(&result.pattern, 0, 1), vec![Value::F64(0.5)]);
    assert!(set_slider_value(&id, 0.75));
    assert_eq!(values(&result.pattern, 0, 1), vec![Value::F64(0.75)]);
    assert!(!set_slider_value("missing-slider", 0.25));

    let rerun = eval_result("          slider(0.7, 0, 1)").expect("eval");
    assert_eq!(rerun.meta.widgets[0].id, id);
    assert_eq!(slider_value(&id).and_then(|v| v.as_f64()), Some(0.7));
}

#[test]
fn preprocess_rewrites_visual_widget_methods_like_strudel() {
    let script = r#"note("c")._pianoroll({ fold: 2 })"#;
    let result = preprocess_strudel_with_meta(script);
    let widget = &result.widgets[0];

    assert_eq!(result.widgets.len(), 1);
    assert_eq!(widget.widget_type, "_pianoroll");
    assert_eq!((widget.from, widget.to), (0, script.len()));
    assert_eq!(widget.index, 0);
    assert_eq!(
        widget.options.get("fold"),
        Some(&crate::WidgetOption::Number(2.0))
    );
    assert_eq!(
        widget.id,
        format!("_widget__pianoroll_0_0-{}", script.len())
    );
    assert!(result.source.contains(&format!(
        r#".rudel_widget_pianoroll("{}", {{ fold: 2 }})"#,
        widget.id
    )));
    assert!(result.source.contains(r#"m("c", 6)"#), "{}", result.source);
}

#[test]
fn visual_widget_methods_are_indexed_per_type() {
    let result = preprocess_strudel_with_meta(
        r#"stack(note("c")._pianoroll(), note("d")._pianoroll(), note("e")._spiral())"#,
    );

    assert_eq!(result.widgets.len(), 3);
    assert_eq!(
        result
            .widgets
            .iter()
            .map(|w| (w.widget_type.as_str(), w.index))
            .collect::<Vec<_>>(),
        vec![("_pianoroll", 0), ("_pianoroll", 1), ("_spiral", 0)]
    );
}

#[test]
fn visual_widget_scanner_ignores_strings_and_comments() {
    let result = preprocess_strudel_with_meta(
        r#"
// note("c")._spiral()
s("._pianoroll()")
note("c")._scope()
"#,
    );

    assert_eq!(result.widgets.len(), 1);
    assert_eq!(result.widgets[0].widget_type, "_scope");
    assert!(result.source.contains(r#"s(m("._pianoroll()","#));
}

#[test]
fn visual_widget_rewrite_survives_earlier_slider_in_the_same_chain() {
    let script = r#"note("c").lpf(slider(725,300,2000))._punchcard({height:200, width:1670})"#;
    let result = preprocess_strudel_with_meta(script);

    assert_eq!(
        result
            .widgets
            .iter()
            .map(|widget| widget.widget_type.as_str())
            .collect::<Vec<_>>(),
        vec!["slider", "_punchcard"]
    );
    assert!(result.source.contains("slider_with_id("));
    assert!(result.source.contains(".rudel_widget_punchcard("));
    assert!(!result.source.contains("._punchcard("));
    assert_eq!(
        result.widgets[1].options.get("height"),
        Some(&crate::WidgetOption::Number(200.0))
    );

    eval_result(script).expect("widget chain with slider and options should eval");
}

#[test]
fn labelled_visual_widget_allows_unindented_dot_continuation() {
    let script = r#"
drums: stack(
  s("bd")
)
._punchcard({height:200, width:1670})
"#;
    let result = preprocess_strudel_with_meta(script);

    assert_eq!(
        result
            .widgets
            .iter()
            .map(|widget| widget.widget_type.as_str())
            .collect::<Vec<_>>(),
        vec!["_punchcard"]
    );
    // The label's expression runs on through the continuation, so the widget
    // hangs off the stack rather than off nothing.
    assert!(
        result.source.contains("\n.rudel_widget_punchcard("),
        "{}",
        result.source
    );

    let result = eval_result(script).expect("labelled stack with trailing widget should eval");
    let id = &result.meta.widgets[0].id;
    assert!(
        result
            .pattern
            .query_arc(Frac::zero(), Frac::one())
            .iter()
            .all(|hap| hap.has_tag(id))
    );
}

#[test]
fn visual_widget_methods_pass_the_pattern_through_and_tag_haps() {
    let plain = eval(r#"note("c")"#).expect("plain eval");
    let result = eval_result(r#"note("c")._spiral()"#).expect("widget eval");
    let widget_id = result.meta.widgets[0].id.clone();

    assert_eq!(result.meta.widgets.len(), 1);
    assert_eq!(result.meta.widgets[0].widget_type, "_spiral");
    assert_eq!(shape(&result.pattern, 1), shape(&plain, 1));
    assert!(
        result
            .pattern
            .query_arc(Frac::zero(), Frac::one())
            .iter()
            .all(|hap| hap.has_tag(&widget_id))
    );
}

// --- JavaScript literal/operator conveniences ---------------------------------

#[test]
fn await_is_stripped() {
    // Rudel's `samples`/`midin`/`loadSoundfont` are synchronous host effects,
    // so the keyword upstream needs is simply dropped.
    assert_eq!(preprocess_strudel("x = await midin('a')"), "x = midin('a')");
    assert_eq!(preprocess_strudel("await samples('a')"), "samples('a')");
    // Identifiers that merely contain or end with `await` are left alone.
    assert_eq!(preprocess_strudel("awaiting(1)"), "awaiting(1)");
    assert_eq!(preprocess_strudel("x.await_(1)"), "x.await_(1)");
    assert_eq!(preprocess_strudel(r#"f('await x')"#), r#"f('await x')"#);
}

#[test]
fn js_conveniences_evaluate_end_to_end() {
    // The combination a Strudel snippet actually arrives in.
    let pat = eval(r#"s("hh!7 oh").filter(hap => hap.value.s === 'hh').gain(.8)"#)
        .expect("filter + strict equality + leading-dot decimal");
    let haps = pat.query_arc(Frac::new(0, 1), Frac::new(1, 1));
    assert_eq!(haps.len(), 7, "only the `hh` haps survive the filter");

    // `hap.hasTag(...)` reads as it does upstream.
    let tagged = eval(r#"s("bd sd").tag('x').filter(hap => hap.hasTag('x'))"#)
        .expect("hasTag on the marshalled hap");
    assert_eq!(tagged.query_arc(Frac::new(0, 1), Frac::new(1, 1)).len(), 2);
}

#[test]
fn chained_factory_methods_take_the_receiver_first() {
    // Upstream installs `stack`/`cat`/`seq` as methods, with `this` as the
    // first pattern.
    let one = |src: &str| {
        eval(src)
            .unwrap()
            .query_arc(Frac::new(0, 1), Frac::new(1, 1))
            .len()
    };
    assert_eq!(one(r#"s("hh*4").stack(s("bd"))"#), 5);
    assert_eq!(one(r#"s("hh*4").seq(s("bd"))"#), 5);
    // `cat` alternates per cycle, so the first cycle is just the receiver.
    assert_eq!(one(r#"s("hh*4").cat(s("bd"))"#), 4);
    // `hush()` discards the pattern, which is how a stacked voice gets muted.
    assert_eq!(one(r#"stack(s("bd").hush(), s("hh*3"))"#), 3);
}

#[test]
fn every_control_has_a_standalone_factory() {
    // Strudel's `registerControl` exports a top-level function as well as a
    // method, so a control name must be callable on its own. (The factory takes
    // structure from its own argument, as upstream's does — it is not the same
    // pattern as the method form applied to something else.)
    let values = |src: &str| -> Vec<String> {
        eval(src)
            .unwrap_or_else(|e| panic!("{src}: {e}"))
            .query_arc(Frac::new(0, 1), Frac::new(1, 1))
            .into_iter()
            .map(|h| format!("{:?}", h.value))
            .collect()
    };
    // Registry-generated: `speed` and `squiz` had no standalone form before.
    assert_eq!(
        values(r#"speed("1 2")"#),
        ["{\"speed\": 1}", "{\"speed\": 2}"]
    );
    assert_eq!(
        values(r#"squiz("2 4")"#),
        ["{\"squiz\": 2}", "{\"squiz\": 4}"]
    );
    // Chaining onto a factory reaches the same controls as the method order.
    let chained = values(r#"speed(2).s("bd")"#);
    assert_eq!(chained.len(), 1);
    assert!(chained[0].contains("speed"), "{chained:?}");
    assert!(chained[0].contains("bd"), "{chained:?}");
    // Hand-written prelude bindings still win over the generated ones.
    assert!(eval(r#"note("c e g")"#).is_ok());
    assert!(eval(r#"n("0 2 4")"#).is_ok());
    // The list-valued additive controls got explicit factories.
    assert!(eval(r#"s("saw").partials(partials([1, 1, 1]))"#).is_ok());
}

#[test]
fn computed_widget_options_reach_the_widget_config() {
    use crate::WidgetOption;

    // The source scan can only read literals, so a computed option is absent
    // from the preprocess metadata...
    let script = "let n = 2 * 4\nnote(\"c\")._pianoroll({ cycles: n, vertical: true })";
    let scanned = &preprocess_strudel_with_meta(script).widgets[0];
    assert!(!scanned.options.contains_key("cycles"));
    // ...while a literal alongside it is picked up as before.
    assert_eq!(
        scanned.options.get("vertical"),
        Some(&WidgetOption::Bool(true))
    );

    // Running the script fills it in: the transpiler passes the option map
    // through to the widget method, which records what the script evaluated.
    let widget = &crate::eval_result(script).expect("eval").meta.widgets[0];
    assert_eq!(
        widget.options.get("cycles"),
        Some(&WidgetOption::Number(8.0))
    );
    assert_eq!(
        widget.options.get("vertical"),
        Some(&WidgetOption::Bool(true))
    );

    // Strings and per-widget isolation both survive the round trip.
    let two = crate::eval_result(
        "let shape = 'polygon'\nstack(note(\"c\")._pitchwheel({ mode: shape }), note(\"d\")._spiral())",
    )
    .expect("eval");
    let wheel = two
        .meta
        .widgets
        .iter()
        .find(|w| w.widget_type == "_pitchwheel")
        .expect("pitchwheel widget");
    assert_eq!(
        wheel.options.get("mode"),
        Some(&WidgetOption::String("polygon".to_string()))
    );
    let spiral = two
        .meta
        .widgets
        .iter()
        .find(|w| w.widget_type == "_spiral")
        .expect("spiral widget");
    assert!(
        spiral.options.is_empty(),
        "options must not leak between widgets"
    );

    // A previous evaluation's options do not survive into the next one.
    let plain = crate::eval_result(r#"note("c")._pianoroll()"#).expect("eval");
    assert!(plain.meta.widgets[0].options.is_empty());
}

#[test]
fn widget_options_coerce_between_their_three_shapes() {
    use crate::WidgetOption::{Bool, Number, String as Str};

    // The host reads every option through these, whatever the script wrote.
    // Each arm needs both polarities: a deleted arm falls through to the
    // catch-all, which agrees with the arm for one of the two answers.
    assert_eq!(Bool(true).as_bool(), Some(true));
    assert_eq!(Bool(false).as_bool(), Some(false));
    assert_eq!(Number(2.0).as_bool(), Some(true));
    assert_eq!(Number(0.0).as_bool(), Some(false));
    assert_eq!(Str("true".into()).as_bool(), Some(true));
    assert_eq!(Str("1".into()).as_bool(), Some(true));
    assert_eq!(Str("false".into()).as_bool(), Some(false));
    assert_eq!(Str("0".into()).as_bool(), Some(false));
    assert_eq!(Str("polygon".into()).as_bool(), None);

    assert_eq!(Bool(true).as_f64(), Some(1.0));
    assert_eq!(Bool(false).as_f64(), Some(0.0));
    assert_eq!(Number(2.5).as_f64(), Some(2.5));
    assert_eq!(Str("2.5".into()).as_f64(), Some(2.5));
    assert_eq!(Str("polygon".into()).as_f64(), None);

    assert_eq!(Str("polygon".into()).as_str(), Some("polygon"));
    assert_eq!(Number(2.0).as_str(), None);
    assert_eq!(Bool(true).as_str(), None);
}

// --- JavaScript the songs corpus leans on -----------------------------------
//
// Each of these was once a whole cluster of real scripts that would not
// evaluate, back when a script was translated into another language before it
// ran. The engine reads JavaScript itself now; these pin that the constructs
// still mean what they say by *running* them.

/// The single value `script` evaluates to, wrapped in `pure(...)`.
fn js_value(script: &str) -> Value {
    let pat = eval(script).unwrap_or_else(|e| panic!("{script}: {e}"));
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1, "{script}: {vals:?}");
    vals.into_iter().next().unwrap()
}

#[test]
fn javascript_expressions_mean_what_javascript_says() {
    for (script, want) in [
        ("pure(1 ? 2 : 3)", Value::Int(2)),
        ("pure(0 ? 2 : 1 ? 3 : 4)", Value::Int(3)),
        ("pure(true && false || !false)", Value::Bool(true)),
        ("pure(1 === 1 && 1 !== 2)", Value::Bool(true)),
        ("pure(2 ** 3 ** 2)", Value::Int(512)),
        ("pure(1 << 4 | 1 >> 1)", Value::Int(16)),
        (
            "pure(typeof 'x' + typeof 1)",
            Value::Str("stringnumber".into()),
        ),
        ("pure([1, 2, 3].length)", Value::Int(3)),
        ("pure('abc'.length)", Value::Int(3)),
        ("pure({0: 'a', 1: 'b'}[1])", Value::Str("b".into())),
        ("pure({...{a: 1}, b: 2}.b)", Value::Int(2)),
        ("pure(Math.max(...[1, 5, 3]))", Value::Int(5)),
        ("pure(.5 + .25)", Value::F64(0.75)),
        ("pure('a' + 1 + 2)", Value::Str("a12".into())),
        ("pure(1 + 2 + 'a')", Value::Str("3a".into())),
        ("pure(JSON.parse('[1, 2]')[1])", Value::Int(2)),
        ("pure(Object.entries({a: 1})[0][0])", Value::Str("a".into())),
        ("pure([1, 2].flatMap(v => [v, v]).length)", Value::Int(4)),
        ("pure((5).toString(2))", Value::Str("101".into())),
    ] {
        assert_eq!(js_value(script), want, "{script}");
    }
}

#[test]
fn block_bodies_declarations_and_loops_run() {
    // A block-bodied arrow with an early return.
    assert_eq!(
        js_value("const f = x => { if (x) { return 1 } else { return 2 } }\npure(f(0))"),
        Value::Int(2)
    );
    // A function declared below the code that calls it is hoisted.
    assert_eq!(
        js_value("pure(g(3))\nfunction g(n) { return n * 2 }"),
        Value::Int(6)
    );
    // `let` and `+=` on a string, built up a piece at a time.
    let script = "let melo = '['\nfor (let i = 0; i < 3; i++) {\n  melo += ' ' + (48+i)\n}\nmelo += ']'\nnote(mini(melo))";
    let pat = eval(script).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 3);
    // A C-style loop inside a function body.
    let script = "function f(n) {\n  let t = 0\n  for (let i = 0; i < n; i++) {\n    t += i\n  }\n  return t\n}\npure(f(4))";
    assert_eq!(js_value(script), Value::Int(6));
    // `reduce` with a seed after a block body.
    assert_eq!(
        js_value("pure([1, 2, 3].reduce((a, x) => { a.push(x); return a }, []).length)"),
        Value::Int(3)
    );
    // Names that were keywords in the old scripting language are ordinary ones.
    assert_eq!(
        js_value("const as = 1, loop = 2, match = 3\npure(as + loop + match)"),
        Value::Int(6)
    );
}

#[test]
fn a_statement_split_across_lines_is_one_statement() {
    // A value on the line after `=`, a chain continued by a leading dot, and a
    // call broken across lines are all one expression to JavaScript.
    assert_eq!(js_value("const x =\n  [1, 2]\npure(x[1])"), Value::Int(2));
    let pat =
        eval("s(\"bd sd\")\n  .fast(2)\n\n  // a comment in the chain\n  .gain(.5)").expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    let pat = eval("stack(s(\"a\"),\n  s(\"b\")\n)").expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
}

#[test]
fn a_bare_silence_statement_is_silence() {
    // Upstream's `silence` is the pattern, not a factory, and a scratch pad
    // ends with the bare word to go quiet.
    let pat = eval("s(\"bd\")\nsilence").expect("eval");
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
    assert!(eval("silence.fast(2)").is_ok());
    // In a string it is a sample name.
    assert!(preprocess_strudel(r#"s("silence")"#).contains(r#"m("silence""#));
}
