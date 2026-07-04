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
    ] {
        let result = preprocess_strudel_with_meta(&format!(r#"s("bd sd").{call}()"#));
        assert_eq!(result.meta.widgets.len(), 1, "{call}");
        assert_eq!(result.meta.widgets[0].widget_type, widget_type, "{call}");
        assert!(result.source.contains(host), "{call}: {}", result.source);
    }
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
    assert!(result.source.contains("\n  .rudel_widget_punchcard("));
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
