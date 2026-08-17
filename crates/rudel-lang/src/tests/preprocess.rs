use super::common::*;
use proptest::prelude::*;

// --- Transpilation / preprocessing parity -------------------------------------

#[test]
fn preprocess_rewrites_arrow_functions_to_koto_lambdas() {
    // bare single identifier parameter
    assert_eq!(preprocess_strudel("f(x => x.fast(2))"), "f(|x| x.fast(2))");
    // parenthesised single parameter
    assert_eq!(
        preprocess_strudel("f((x) => x.fast(2))"),
        "f(|x| x.fast(2))"
    );
    // multiple parameters
    assert_eq!(preprocess_strudel("f((a, b) => a)"), "f(|a, b| a)");
    // zero parameters -> Koto's `||`
    assert_eq!(preprocess_strudel("f(() => 1)"), "f(|| 1)");
    // an `=>` inside a string literal is left intact; the string is wrapped in
    // `m(literal, offset)` for source-location tracking (offset 6 = the byte
    // position of the content just after `note("`).
    assert_eq!(
        preprocess_strudel(r#"note("a => b")"#),
        r#"note(m("a => b", 6))"#
    );
    // a comparison operator is never mistaken for an arrow
    assert_eq!(preprocess_strudel("f(x >= 2)"), "f(x >= 2)");
}

#[test]
fn preprocess_flattens_alignment_getters() {
    assert_eq!(preprocess_strudel("p.add.out(1)"), "p.add_out(1)");
    // `in` is the default alignment and *is* the plain method
    assert_eq!(preprocess_strudel("p.mul.in(1)"), "p.mul(1)");
    // spelling normalisation: `mod` is a Koto keyword, and the camelCase and
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
    assert_eq!(preprocess_strudel(""), "silence()");
    assert_eq!(preprocess_strudel("   \n  \n"), "silence()");
    assert_eq!(preprocess_strudel("// just a comment\n"), "silence()");
    // and it evaluates to an actually-empty pattern
    let pat = eval("// nothing here\n").expect("eval");
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn preprocess_metadata_reports_mini_locations() {
    let result = preprocess_strudel_with_meta(r#"s("bd sd").note("c e")"#);
    assert_eq!(result.meta.mini_locations, vec![(3, 8), (17, 20)]);
    assert_eq!(result.source, r#"s(m("bd sd", 3)).note(m("c e", 17))"#);
}

#[test]
fn eval_result_carries_editor_metadata() {
    let result = eval_result(r#"s("bd sd")"#).expect("eval");
    assert_eq!(result.meta.mini_locations, vec![(3, 8)]);
    assert!(result.meta.widgets.is_empty());
    assert!(result.meta.labels.is_empty());
    assert!(!result.meta.cleanup.widget_removed);
}

#[test]
fn preprocess_rewrites_slider_widgets_like_strudel() {
    let result = preprocess_strudel_with_meta("slider(0.5, 0, 1, 0.01)");

    assert_eq!(result.source, r#"slider_with_id("7:10", 0.5, 0, 1, 0.01)"#);
    assert!(result.meta.mini_locations.is_empty());
    assert_eq!(result.meta.widgets.len(), 1);

    let widget = &result.meta.widgets[0];
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
        .meta
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

    assert_eq!(result.meta.widgets.len(), 1);
    let widget = &result.meta.widgets[0];
    assert_eq!(widget.value.as_deref(), Some("0.4"));
    assert!(result.source.contains(r#"s(m("slider(0.2)","#));
    assert!(result.source.contains("foo.slider(0.3)"));
    assert!(result.source.contains(r#"slider_with_id(""#));
}

#[test]
fn public_visualizer_names_rewrite_to_inline_widget() {
    // The public `pianoroll` / `pitchwheel` / `wordfall` spellings create the
    // same widget (canonical `_`-prefixed type, rewritten to the same koto host
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
        assert_eq!(result.meta.widgets.len(), 1, "{call}");
        assert_eq!(result.meta.widgets[0].widget_type, widget_type, "{call}");
        assert!(result.source.contains(host), "{call}: {}", result.source);
    }
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

    assert_eq!(result.meta.mini_locations, vec![(26, 27)]);
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

    assert_eq!(result.meta.mini_locations, vec![(6, 7), (30, 32)]);
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
    let widget = &result.meta.widgets[0];

    assert_eq!(result.meta.widgets.len(), 1);
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
    assert_eq!(result.meta.mini_locations, vec![(6, 7)]);
}

#[test]
fn visual_widget_methods_are_indexed_per_type() {
    let result = preprocess_strudel_with_meta(
        r#"stack(note("c")._pianoroll(), note("d")._pianoroll(), note("e")._spiral())"#,
    );

    assert_eq!(result.meta.widgets.len(), 3);
    assert_eq!(
        result
            .meta
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

    assert_eq!(result.meta.widgets.len(), 1);
    assert_eq!(result.meta.widgets[0].widget_type, "_scope");
    assert!(result.source.contains(r#"s(m("._pianoroll()","#));
}

#[test]
fn visual_widget_rewrite_survives_earlier_slider_in_the_same_chain() {
    let script = r#"note("c").lpf(slider(725,300,2000))._punchcard({height:200, width:1670})"#;
    let result = preprocess_strudel_with_meta(script);

    assert_eq!(
        result
            .meta
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
        result.meta.widgets[1].options.get("height"),
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
            .meta
            .widgets
            .iter()
            .map(|widget| widget.widget_type.as_str())
            .collect::<Vec<_>>(),
        vec!["_punchcard"]
    );
    // The continuation is written onto the line that closed the `stack(`, which
    // is the only place Koto will take it — left on a line of its own it reads
    // as a new statement however far it is indented.
    assert!(
        result.source.contains(").rudel_widget_punchcard("),
        "{}",
        result.source
    );
    assert!(!result.source.contains("\n.rudel_widget_punchcard("));

    eval_result(script).expect("labelled stack with trailing widget should eval");
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

#[test]
fn arrow_and_pipe_callbacks_are_equivalent() {
    // Differential check: arrow-function and Koto-lambda spellings of the same
    // callback must produce identical haps across the combinator surface.
    let pairs = [
        (
            r#"seq(0).every(2, x => x.add(10))"#,
            r#"seq(0).every(2, |x| x.add(10))"#,
        ),
        (
            r#"seq(0).superimpose((x) => x.add(7))"#,
            r#"seq(0).superimpose(|x| x.add(7))"#,
        ),
        (
            r#"seq(0, 1, 2, 3).within(0, 0.4, x => x.add(10))"#,
            r#"seq(0, 1, 2, 3).within(0, 0.4, |x| x.add(10))"#,
        ),
        (
            r#"seq(0).layer([x => x.add(0), x => x.add(7)])"#,
            r#"seq(0).layer([|x| x.add(0), |x| x.add(7)])"#,
        ),
    ];
    for (arrow, pipe) in pairs {
        let a = eval(arrow).unwrap_or_else(|e| panic!("arrow eval {arrow}: {e}"));
        let b = eval(pipe).unwrap_or_else(|e| panic!("pipe eval {pipe}: {e}"));
        assert_eq!(values(&a, 0, 2), values(&b, 0, 2), "mismatch for {arrow}");
    }
}

proptest! {
    #[test]
    fn bare_arrow_rewrites_generated_identifiers(param in "[a-z][a-z0-9_]{0,8}") {
        let src = format!("f({param} => {param}.fast(2))");
        let expected = format!("f(|{param}| {param}.fast(2))");

        prop_assert_eq!(preprocess_strudel(&src), expected);
    }

    #[test]
    fn parenthesized_arrow_rewrites_generated_identifiers(param in "[a-z][a-z0-9_]{0,8}") {
        let src = format!("f(({param}) => {param}.rev())");
        let expected = format!("f(|{param}| {param}.rev())");

        prop_assert_eq!(preprocess_strudel(&src), expected);
    }

    #[test]
    fn generated_comparison_is_not_rewritten_as_arrow(
        lhs in "[a-z][a-z0-9_]{0,8}",
        rhs in 0i32..128,
    ) {
        let src = format!("f({lhs} >= {rhs})");

        prop_assert_eq!(preprocess_strudel(&src), src);
    }
}

// --- JavaScript literal/operator conveniences ---------------------------------

#[test]
fn leading_dot_decimals_become_koto_numbers() {
    // JS allows `.5`; Koto requires `0.5`. The dot starts a number only where a
    // value cannot already be sitting to its left.
    assert_eq!(preprocess_strudel("f(.5)"), "f(0.5)");
    assert_eq!(preprocess_strudel("f(1, .25)"), "f(1, 0.25)");
    assert_eq!(preprocess_strudel("x = -.5"), "x = -0.5");
    assert_eq!(preprocess_strudel("f(a * .5)"), "f(a * 0.5)");
    // Method access and ordinary decimals are untouched.
    assert_eq!(preprocess_strudel("pat.fast(2)"), "pat.fast(2)");
    assert_eq!(preprocess_strudel("f(1.5)"), "f(1.5)");
    assert_eq!(preprocess_strudel("f(x).gain(1)"), "f(x).gain(1)");
    assert_eq!(preprocess_strudel("f(l[0].gain)"), "f(l[0].gain)");
    // Inside a string literal it is mini-notation, not code. (Strings get
    // wrapped in `m(literal, offset)` for source-location tracking.)
    assert_eq!(preprocess_strudel(r#"f(".5")"#), r#"f(m(".5", 3))"#);
}

#[test]
fn strict_equality_becomes_kotos_equality() {
    assert_eq!(preprocess_strudel("f(a === b)"), "f(a == b)");
    assert_eq!(preprocess_strudel("f(a !== b)"), "f(a != b)");
    // Already-Koto spellings and string contents are untouched.
    assert_eq!(preprocess_strudel("f(a == b)"), "f(a == b)");
    assert_eq!(preprocess_strudel(r#"f('a === b')"#), r#"f('a === b')"#);
}

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
    let scanned = &preprocess_strudel_with_meta(script).meta.widgets[0];
    assert!(!scanned.options.contains_key("cycles"));
    // ...while a literal alongside it is picked up as before.
    assert_eq!(
        scanned.options.get("vertical"),
        Some(&WidgetOption::Bool(true))
    );

    // Running the script fills it in: the transpiler passes the option map
    // through to the widget method, which records what Koto evaluated.
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

// --- JavaScript constructs the songs corpus leans on -------------------------
//
// Each of these is a whole cluster of real scripts that would not evaluate
// without it, so the assertions pin the *shape* of the emitted Koto rather than
// just "it parses" — a pass that quietly stops firing still produces valid Koto,
// and only the shape says whether the construct survived.

#[test]
fn a_ternary_becomes_a_parenthesised_if_expression() {
    assert_eq!(
        preprocess_strudel("f(a ? b : c)"),
        "f((if a then b else c))"
    );
    // Nested in both branches, and in the condition.
    assert_eq!(
        preprocess_strudel("f(a ? (b ? c : d) : e)"),
        "f((if a then ((if b then c else d)) else e))"
    );
    assert_eq!(
        preprocess_strudel("f(a ? b : c ? d : e)"),
        "f((if a then b else (if c then d else e)))"
    );
    // `return` is a statement keyword, not part of the condition.
    assert_eq!(
        preprocess_strudel("f(x => { return a ? b : c })"),
        "f(|x|
  (if a then b else c)
)"
    );
    // A `?` inside a string is pattern text; the string becomes `m(literal, n)`.
    assert!(preprocess_strudel(r#"s("a?b")"#).contains(r#"m("a?b""#));
}

#[test]
fn a_block_bodied_arrow_becomes_an_indented_koto_block() {
    // The closing bracket has to end up on its own line: Koto will not let the
    // enclosing call close on the body's last line.
    assert_eq!(
        preprocess_strudel("f(x => { const a = 1; return a })"),
        "f(|x|\n  a = 1\n  a\n)"
    );
    // `if (c) stmt` takes Koto's `then`, and a non-tail `return` stays.
    assert_eq!(
        preprocess_strudel("f(x => { if(x) return 1; return 2 })"),
        "f(|x|\n  if x then return 1\n  2\n)"
    );
    // A `function` declaration binds its name.
    assert_eq!(
        preprocess_strudel("function arr(p, l) { return [l, p] }"),
        "arr = |p, l|\n  [l, p]"
    );
}

#[test]
fn line_continuations_that_koto_would_end_at_the_newline_are_joined() {
    // A value on the line after `=`...
    assert_eq!(preprocess_strudel("const x =\n  [1, 2]"), "x = [1, 2]");
    // ...and an arrow body on the line after `=>`. Left on its own line the
    // body becomes an indented block, which the enclosing `)` cannot close.
    assert_eq!(preprocess_strudel("f((v) =>\n  v)"), "f(|v| v)");
    // A comparison is not an assignment.
    assert_eq!(preprocess_strudel("a ==\nb"), "a ==\nb");
}

#[test]
fn js_operators_and_punctuation_take_their_koto_spelling() {
    assert_eq!(preprocess_strudel("f(a && b || !c)"), "f(a and b or not c)");
    assert_eq!(preprocess_strudel("f(a != b)"), "f(a != b)");
    // A trailing `;` is dropped; `!` inside mini-notation is replication.
    assert_eq!(preprocess_strudel("f(1);"), "f(1)");
    assert!(preprocess_strudel(r#"s("bd!4")"#).contains("bd!4"));
}

#[test]
fn js_object_and_declaration_forms_become_koto_ones() {
    // Numeric keys have to be quoted; Koto's map declaration takes a name.
    assert_eq!(
        preprocess_strudel("x = {0: a, 1: b}"),
        "x = {'0': a, '1': b}"
    );
    // Spread has no Koto syntax, so it becomes a merge call.
    assert_eq!(
        preprocess_strudel("x = {...v, n: 1}"),
        "x = rudel_spread(v, {n: 1})"
    );
    // One declaration per name.
    assert_eq!(preprocess_strudel("const a = 1, b = 2"), "a = 1\nb = 2");
    // A comma inside the value is not a separator.
    assert_eq!(preprocess_strudel("const a = [1, 2]"), "a = [1, 2]");
}

#[test]
fn js_properties_become_the_calls_koto_needs() {
    assert_eq!(preprocess_strudel("f(v.length)"), "f(v.length())");
    // Already a call, or a longer name: left alone.
    assert_eq!(preprocess_strudel("f(v.length(1))"), "f(v.length(1))");
    assert_eq!(preprocess_strudel("f(v.lengthen)"), "f(v.lengthen)");
    // `.value` reads as JS does — absent rather than an error — so a helper can
    // test it to tell a control map from a bare value.
    assert_eq!(
        preprocess_strudel("f(v.value)"),
        "f(rudel_prop(v, 'value'))"
    );
    assert_eq!(preprocess_strudel("f(v.value(1))"), "f(v.value(1))");
}

#[test]
fn control_blocks_and_js_globals_become_koto() {
    // `if (c) { … } else …` is an indented block under the condition, not a
    // `then`, and a `return` inside an arm returns from the function.
    assert_eq!(
        preprocess_strudel("f(x => { if (x) { return 1 } else { return 2 } })"),
        "f(|x|\n  if x\n    return 1\n  else\n    return 2\n)"
    );
    // A brace on its own line is still the same arm.
    assert_eq!(
        preprocess_strudel("f(x => { if (x)\n{ return 1 }\n})"),
        "f(|x|\n  if x\n    return 1\n)"
    );
    // `typeof` is an operator in JS and a call here, answering with JS's names
    // so the comparison the script wrote still matches.
    assert_eq!(
        preprocess_strudel("f(typeof v == 'string')"),
        "f(rudel_typeof(v) == 'string')"
    );
    assert_eq!(
        preprocess_strudel("f(typeof (a) )"),
        "f(rudel_typeof((a)) )"
    );
    // A name that only looks like the operator is left alone.
    assert_eq!(preprocess_strudel("f(typeofx)"), "f(typeofx)");
}

#[test]
fn a_name_koto_reserves_is_renamed_where_the_script_binds_it() {
    // `as` is a Koto keyword and an ordinary JS identifier.
    let out = preprocess_strudel("const as = register('as', f)\nx.as(1)");
    assert!(out.starts_with("as_ = register('as', f)"), "{out}");
    // The method call is a property, not a binding, so it keeps the name.
    assert!(out.contains("x.as(1)"), "{out}");
    // A keyword the script never binds is untouched, so `loop` still reaches
    // the built-in of that name.
    assert_eq!(preprocess_strudel("x.loop(1)"), "x.loop(1)");
}

#[test]
fn a_declaration_moves_above_the_code_that_uses_it() {
    // JavaScript resolves a name inside a function when it runs, so the helper
    // may be written above the data it reads.
    assert_eq!(
        preprocess_strudel("f = |x| x + n\nn = 2"),
        "n = 2\nf = |x| x + n"
    );
    // Anything with no dependency between it and its neighbours stays put.
    assert_eq!(preprocess_strudel("a = 1\nb = 2"), "a = 1\nb = 2");
    // Two names that need each other are a cycle, and keep source order rather
    // than being reordered arbitrarily.
    assert_eq!(
        preprocess_strudel("f = |x| g(x)\ng = |x| f(x)"),
        "f = |x| g(x)\ng = |x| f(x)"
    );
}

#[test]
fn line_breaks_koto_cannot_read_are_taken_out() {
    // A nested call spanning lines is only allowed in final position, so one
    // followed by another argument is folded onto a line...
    assert_eq!(
        preprocess_strudel("stack(a(\n1),\nb)"),
        "stack(a( 1),\n  b)"
    );
    // ...and the last argument keeps its layout.
    assert!(preprocess_strudel("stack(b,\na(\n1))").contains("a(\n"));
    // A call whose `(` opens the next line is one expression; left split, the
    // parentheses become a tuple.
    assert_eq!(preprocess_strudel("stack\n(a, b)"), "stack(a, b)");
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
