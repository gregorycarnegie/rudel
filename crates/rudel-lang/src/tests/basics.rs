use super::common::*;

#[test]
fn standalone_transforms_match_their_methods() {
    // Strudel registers transforms both as methods and as curried standalone
    // functions; the standalone form takes the pattern last. Each pairing must
    // produce identical haps.
    let pairs = [
        (r#"fast(2, s("bd sd"))"#, r#"s("bd sd").fast(2)"#),
        (r#"slow(2, s("bd sd"))"#, r#"s("bd sd").slow(2)"#),
        (r#"ply(2, s("bd sd"))"#, r#"s("bd sd").ply(2)"#),
        (r#"iter(4, note("0 1 2 3"))"#, r#"note("0 1 2 3").iter(4)"#),
        (r#"add(7, note("0 1"))"#, r#"note("0 1").add(7)"#),
        (r#"euclid(3, 8, s("bd"))"#, r#"s("bd").euclid(3, 8)"#),
        (r#"palindrome(s("bd sd"))"#, r#"s("bd sd").palindrome()"#),
        (
            r#"compress(0.25, 0.75, s("bd sd"))"#,
            r#"s("bd sd").compress(0.25, 0.75)"#,
        ),
        (r#"hurry(2, s("bd sd"))"#, r#"s("bd sd").hurry(2)"#),
        (r#"range(0, 7, n("0 1"))"#, r#"n("0 1").range(0, 7)"#),
        (r#"chop(2, s("bd"))"#, r#"s("bd").chop(2)"#),
    ];
    for (standalone, method) in pairs {
        let a = eval(standalone).unwrap_or_else(|e| panic!("standalone {standalone}: {e}"));
        let b = eval(method).unwrap_or_else(|e| panic!("method {method}: {e}"));
        assert_eq!(shape(&a, 2), shape(&b, 2), "mismatch for `{standalone}`");
    }
}

#[test]
fn standalone_callback_transforms_match_their_methods() {
    // The higher-order combinators also have standalone forms taking a
    // transform function and the pattern last (`jux(rev, pat)`).
    let pairs = [
        (r#"jux(rev, s("bd sd"))"#, r#"s("bd sd").jux(|x| x.rev())"#),
        (
            r#"superimpose(|x| x.fast(2), s("bd sd"))"#,
            r#"s("bd sd").superimpose(|x| x.fast(2))"#,
        ),
        (
            r#"every(2, |x| x.fast(2), s("bd sd"))"#,
            r#"s("bd sd").every(2, |x| x.fast(2))"#,
        ),
        (
            r#"off(0.25, |x| x.add(12), note("0 2"))"#,
            r#"note("0 2").off(0.25, |x| x.add(12))"#,
        ),
        (
            r#"within(0, 0.5, |x| x.fast(2), s("a b c d"))"#,
            r#"s("a b c d").within(0, 0.5, |x| x.fast(2))"#,
        ),
        (
            r#"sometimes(|x| x.fast(2), s("a b c d"))"#,
            r#"s("a b c d").sometimes(|x| x.fast(2))"#,
        ),
    ];
    for (standalone, method) in pairs {
        let a = eval(standalone).unwrap_or_else(|e| panic!("standalone {standalone}: {e}"));
        let b = eval(method).unwrap_or_else(|e| panic!("method {method}: {e}"));
        assert_eq!(shape(&a, 2), shape(&b, 2), "mismatch for `{standalone}`");
    }
}

#[test]
fn standalone_long_tail_matches_methods_and_camelcase_aliases() {
    // The long tail of standalone forms, exercising the extra arg groups and
    // Strudel's camelCase names against the equivalent methods.
    let pairs = [
        (r#"fastGap(2, s("a b"))"#, r#"s("a b").fastGap(2)"#),
        (
            r#"iterBack(2, note("0 1 2 3"))"#,
            r#"note("0 1 2 3").iterBack(2)"#,
        ),
        (r#"expand(2, s("a b"))"#, r#"s("a b").expand(2)"#),
        (r#"range2(0, 7, n("0 1"))"#, r#"n("0 1").range2(0, 7)"#),
        (r#"focus(0, 0.5, s("a b"))"#, r#"s("a b").focus(0, 0.5)"#),
        (
            r#"swingBy(0.25, 4, s("a b c d"))"#,
            r#"s("a b c d").swingBy(0.25, 4)"#,
        ),
        (
            r#"euclidLegato(3, 8, s("bd"))"#,
            r#"s("bd").euclidLegato(3, 8)"#,
        ),
        (
            r#"euclidRot(3, 8, 1, s("bd"))"#,
            r#"s("bd").euclidRot(3, 8, 1)"#,
        ),
        (
            r#"echo(3, 0.125, 0.5, s("bd"))"#,
            r#"s("bd").echo(3, 0.125, 0.5)"#,
        ),
        (
            r#"stut(3, 0.5, 0.125, s("bd"))"#,
            r#"s("bd").stut(3, 0.5, 0.125)"#,
        ),
        (r#"degradeBy(0.4, s("a*8"))"#, r#"s("a*8").degradeBy(0.4)"#),
        // callback long tail (i64/f64/frac/pattern + function)
        (
            r#"firstOf(2, |x| x.fast(2), s("a b"))"#,
            r#"s("a b").firstOf(2, |x| x.fast(2))"#,
        ),
        (
            r#"chunk(2, |x| x.fast(2), s("a b c d"))"#,
            r#"s("a b c d").chunk(2, |x| x.fast(2))"#,
        ),
        (
            r#"juxBy(0.5, rev, s("a b"))"#,
            r#"s("a b").juxBy(0.5, |x| x.rev())"#,
        ),
        (
            r#"inside(2, rev, s("a b c d"))"#,
            r#"s("a b c d").inside(2, |x| x.rev())"#,
        ),
        (
            r#"someCycles(|x| x.fast(2), s("a b"))"#,
            r#"s("a b").someCycles(|x| x.fast(2))"#,
        ),
    ];
    for (standalone, method) in pairs {
        let a = eval(standalone).unwrap_or_else(|e| panic!("standalone {standalone}: {e}"));
        let b = eval(method).unwrap_or_else(|e| panic!("method {method}: {e}"));
        assert_eq!(shape(&a, 2), shape(&b, 2), "mismatch for `{standalone}`");
    }
}

#[test]
fn reference_surface_is_generated_from_the_runtime() {
    let r = crate::reference();
    for f in [
        "note", "n", "s", "stack", "cat", "sine", "silence", "m", "pat",
    ] {
        assert!(
            r.functions.iter().any(|x| x == f),
            "missing function {f}: {:?}",
            r.functions
        );
    }
    for m in ["fast", "slow", "gain", "lpf", "every", "scale"] {
        assert!(
            r.methods.iter().any(|x| x == m),
            "missing method {m}: {:?}",
            r.methods
        );
    }
    for c in ["lpf", "room", "delay", "crush", "speed"] {
        assert!(
            r.controls.iter().any(|x| x == c),
            "missing control {c}: {:?}",
            r.controls
        );
    }
    // generated, so it is sorted/deduped and substantial
    assert!(
        r.functions.windows(2).all(|w| w[0] < w[1]),
        "functions not sorted/unique"
    );
    assert!(
        r.methods.len() > 100,
        "expected many methods, got {}",
        r.methods.len()
    );
}

#[test]
fn per_hap_locations_are_absolute_to_source() {
    // Every string literal is wrapped as `m("...", offset)`, so per-hap source
    // locations come back as absolute byte offsets into the original source.
    // In `s("bd sd")`, `bd` is at 3..5 and `sd` at 6..8.
    let pat = eval(r#"s("bd sd")"#).expect("eval");
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    assert!(
        haps[0].context.locations.contains(&(3, 5)),
        "bd: {:?}",
        haps[0].context.locations
    );
    assert!(
        haps[1].context.locations.contains(&(6, 8)),
        "sd: {:?}",
        haps[1].context.locations
    );
}

#[test]
fn locations_distinguish_multiple_source_strings() {
    // Two mini strings on one line must each map to their own source offset.
    // `stack(s("bd"), note("e"))`: `bd` content at 9..11, `e` content at 21..22.
    let pat = eval(r#"stack(s("bd"), note("e"))"#).expect("eval");
    let locs: Vec<(usize, usize)> = pat
        .query_arc(Frac::zero(), Frac::one())
        .iter()
        .flat_map(|h| h.context.locations.clone())
        .collect();
    assert!(locs.contains(&(9, 11)), "bd loc missing: {locs:?}");
    assert!(locs.contains(&(21, 22)), "e loc missing: {locs:?}");
}

#[test]
fn eval_simple_pattern() {
    let pat = eval(r#"note("c4 e4 g4").fast(2)"#).expect("eval");
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 6);
}

#[test]
fn eval_stack_and_controls() {
    let pat = eval(r#"stack(s("bd*2"), note("c4 e4").gain(0.5))"#).expect("eval");
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn non_pattern_result_errors() {
    assert!(eval("1 + 2").is_err());
}

#[test]
fn log2_takes_base_two_logarithm_of_values() {
    // log2 maps bare numeric values (like floor/ceil), so apply it before
    // wrapping the result in a control.
    let pat = eval(r#"n("1 2 4 8".log2())"#).expect("eval");
    let ns: Vec<f64> = values(&pat, 0, 1)
        .iter()
        .map(|v| match v {
            Value::Map(m) => m.get("n").and_then(|x| x.as_f64()).expect("n key"),
            other => panic!("expected control map, got {other:?}"),
        })
        .collect();
    assert_eq!(ns, vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn parray_packs_one_value_per_pattern_into_a_list() {
    // parray([a, b, c]) emits [va, vb, vc] per hap; wholes are the intersection.
    let pat = eval(r#"parray(["0", "1", "2"])"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::List(items) => {
            let nums: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap_or(-1.0)).collect();
            assert_eq!(nums, vec![0.0, 1.0, 2.0]);
        }
        other => panic!("expected list value, got {other:?}"),
    }
}

#[test]
fn a_hap_level_callback_repeats_its_probe_window_forever() {
    // `filter`/`fmap` run the callback over a fixed 16-cycle probe and then
    // repeat that window, so anything past cycle 16 is the window's own cycle
    // `n mod 16`. Nothing had ever queried past the first window, which left
    // the whole repeat calculation unexercised.
    let pat = eval(r#"note("<0 1 2 3 4>").filter |hap| true"#).expect("eval");
    let at = |cycle: i64| {
        values(&pat, cycle, cycle + 1)
            .iter()
            .filter_map(|v| match v {
                Value::Map(m) => m.get("note").and_then(Value::as_f64),
                other => other.as_f64(),
            })
            .collect::<Vec<f64>>()
    };
    // Inside the window the pattern is itself: `<0 1 2 3 4>` at cycle 4 is 4.
    assert_eq!(at(4), vec![4.0]);
    // Cycle 20 is the window's cycle 4 repeated, not silence and not cycle 0.
    assert_eq!(at(20), at(4), "the probe window should repeat");
    assert_eq!(at(17), at(1));
    assert_eq!(at(33), at(1));
}

#[test]
fn a_callback_that_returns_something_else_leaves_the_pattern_alone() {
    // The callback's result is only taken when it *is* a pattern; anything
    // else (here a `Fraction`, the one other object rudel hands scripts) means
    // "no change" rather than being cast into one.
    let haps = |script: &str| values(&eval(script).expect("eval"), 0, 2);
    // `every` goes through `Callback::apply`: returning a non-pattern is the
    // same as returning the pattern it was handed.
    assert_eq!(
        haps(r#"s("bd sd").every(1, |x| Fraction(1))"#),
        haps(r#"s("bd sd").every(1, |x| x)"#)
    );
    // ...and `echoWith` through the indexed `apply2`.
    assert_eq!(
        haps(r#"s("bd").echoWith(2, 0.25, |x, i| Fraction(i))"#),
        haps(r#"s("bd").echoWith(2, 0.25, |x, i| x)"#)
    );
}

#[test]
fn filter_keeps_only_matching_haps() {
    // Strudel's own example: `s("hh!7 oh").filter(hap => hap.value.s === 'hh')`.
    // Single-quoted strings are plain strings (double quotes are
    // mini-notation), which is how upstream's example compares against one.
    let pat = eval(r#"s("hh!7 oh").filter |hap| hap.value.s == 'hh'"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 7, "the `oh` should be dropped");
    assert!(vals.iter().all(|v| match v {
        Value::Map(m) => m.get("s").and_then(|x| x.as_str()) == Some("hh"),
        _ => false,
    }));
}

#[test]
fn tag_marks_haps_for_a_later_filter() {
    // `tag` writes Hap.context.tags, which the predicate sees as `hap.tags`.
    let pat = eval(r#"stack(s("bd").tag('keep'), s("sd")).filter |hap| hap.tags.contains 'keep'"#)
        .expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 1);
    assert!(matches!(&vals[0], Value::Map(m)
        if m.get("s").and_then(|x| x.as_str()) == Some("bd")));
}

#[test]
fn filter_when_selects_by_onset_time() {
    // `filterWhen` receives the whole's begin in cycles.
    let pat = eval(r#"s("bd*4").filterWhen |t| t < 0.5"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2, "first half of the cycle only");
    // The predicate sees absolute cycle time, so it can select whole cycles.
    let one = eval(r#"s("bd*4").filterWhen |t| t < 1"#).expect("eval");
    assert_eq!(values(&one, 0, 1).len(), 4, "cycle 0 kept");
    assert_eq!(values(&one, 1, 2).len(), 0, "cycle 1 dropped");
}

#[test]
fn the_euclid_family_takes_patterned_counts() {
    // Upstream's `register` patternifies every euclid argument, so the counts
    // may alternate by cycle. Mini-notation's operator always could; the method
    // and standalone spellings share its implementation, so all three agree.
    let mini = eval(r#"s("bd(<3 5>,8)")"#).expect("mini");
    for src in [
        r#"s("bd").euclid("<3 5>", 8)"#,
        r#"euclid("<3 5>", 8, s("bd"))"#,
    ] {
        let pat = eval(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        assert_eq!(shape(&pat, 4), shape(&mini, 4), "{src}");
    }
    // Literal counts keep the direct path, and must not have drifted from it.
    assert_eq!(
        shape(&eval(r#"s("bd").euclid(3, 8)"#).expect("literal"), 2),
        shape(&eval(r#"s("bd(3,8)")"#).expect("mini literal"), 2)
    );
    // The rotation and legato variants patternify too.
    for src in [
        r#"s("bd").euclidRot("<3 5>", 8, "<0 2>")"#,
        r#"s("bd").euclidLegato("<3 5>", 8)"#,
        r#"euclidLegatoRot("<3 5>", 8, 2, s("bd"))"#,
    ] {
        let pat = eval(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        assert!(!shape(&pat, 4).is_empty(), "{src} produced nothing");
    }
}

/// The stepwise counts patternify (https://strudel.cc/learn/stepwise/), and the
/// standalone spelling has to agree with the method it wraps. `stepwise_parity`
/// pins the events themselves against Strudel; this pins that both ways in reach
/// them.
#[test]
fn the_stepwise_counts_patternify_in_both_spellings() {
    let pairs = [
        (
            r#"expand("3 2 1", s("bd sd"))"#,
            r#"s("bd sd").expand("3 2 1")"#,
        ),
        (
            r#"take("1 2", s("bd sd cp"))"#,
            r#"s("bd sd cp").take("1 2")"#,
        ),
        (
            r#"drop("1 2", s("bd sd cp"))"#,
            r#"s("bd sd cp").drop("1 2")"#,
        ),
        (
            r#"shrink("1 -1", s("bd sd cp"))"#,
            r#"s("bd sd cp").shrink("1 -1")"#,
        ),
        (
            r#"grow("1 -1", s("bd sd cp"))"#,
            r#"s("bd sd cp").grow("1 -1")"#,
        ),
    ];
    for (standalone, method) in pairs {
        let a = eval(standalone).unwrap_or_else(|e| panic!("standalone {standalone}: {e}"));
        let b = eval(method).unwrap_or_else(|e| panic!("method {method}: {e}"));
        assert_eq!(shape(&a, 2), shape(&b, 2), "mismatch for `{standalone}`");
        assert!(!shape(&a, 2).is_empty(), "`{standalone}` produced nothing");
    }
    // A literal count still takes the direct path, and must agree with itself
    // patterned.
    assert_eq!(
        shape(&eval(r#"s("bd sd cp").expand(2)"#).expect("literal"), 2),
        shape(&eval(r#"s("bd sd cp").expand("2")"#).expect("patterned"), 2)
    );
}

#[test]
fn a_script_that_evaluates_to_something_else_is_not_a_pattern() {
    // Only a `KPattern` result is taken as the pattern; any other object is
    // reported rather than cast.
    assert!(eval("Fraction(1)").is_err());
    assert!(eval("42").is_err());
}

#[test]
fn a_transform_called_with_no_argument_is_the_transform_itself() {
    // Strudel's `register` curries, so `rev()` is a function, and passing it
    // where a transform is expected has to keep working.
    assert_eq!(
        shape(
            &eval(r#"s("bd sd hh").sometimesBy(1, rev())"#).expect("rev()"),
            1
        ),
        shape(
            &eval(r#"s("bd sd hh").sometimesBy(1, rev)"#).expect("rev"),
            1
        ),
    );
    // Applied normally it still transforms.
    assert_eq!(
        shape(&eval(r#"rev(s("bd sd"))"#).expect("rev(pat)"), 1),
        shape(&eval(r#"s("bd sd").rev()"#).expect("pat.rev()"), 1),
    );
}

#[test]
fn a_list_argument_is_a_sequence() {
    // Strudel reifies an array into a fastcat, so `seq([a, b])` lays both out
    // across one cycle rather than evaluating to silence.
    for (list, spread) in [
        (r#"seq([s("bd"), s("hh")])"#, r#"seq(s("bd"), s("hh"))"#),
        (r#"cat([s("bd"), s("hh")])"#, r#"seq(s("bd"), s("hh"))"#),
    ] {
        assert_eq!(
            shape(&eval(list).expect(list), 1),
            shape(&eval(spread).expect(spread), 1),
            "{list}"
        );
    }
}

#[test]
fn a_spread_argument_expands_into_separate_arguments() {
    // `stack(...xs)` layers the patterns; `stack(xs)` is one *sequenced*
    // pattern. Passing the list through unchanged would quietly turn the first
    // into the second, so the spread has to survive to the call itself.
    let xs = r#"let xs = [s("bd"), s("hh")]"#;
    for (spread, expanded) in [
        ("stack(...xs)", r#"stack(s("bd"), s("hh"))"#),
        ("seq(...xs)", r#"seq(s("bd"), s("hh"))"#),
        (
            r#"stack(s("cp"), ...xs)"#,
            r#"stack(s("cp"), s("bd"), s("hh"))"#,
        ),
    ] {
        let a = eval(&format!("{xs}\n{spread}")).unwrap_or_else(|e| panic!("{spread}: {e}"));
        let b = eval(expanded).unwrap_or_else(|e| panic!("{expanded}: {e}"));
        assert_eq!(shape(&a, 1), shape(&b, 1), "{spread}");
    }
    // ...and a list that was *not* spread still means one sequenced pattern.
    let listed = eval(&format!("{xs}\nstack(xs)")).expect("stack(xs)");
    let stacked = eval(&format!("{xs}\nstack(...xs)")).expect("stack(...xs)");
    assert_ne!(
        shape(&listed, 1),
        shape(&stacked, 1),
        "a list is not a spread"
    );
}

#[test]
fn math_matches_javascript() {
    // Every value below was read out of a real JS engine, because several of
    // these disagree with the obvious Rust spelling: `Math.round` breaks ties
    // towards +infinity (Rust breaks them away from zero), `Math.sign` keeps a
    // signed zero, and `max`/`min` with no arguments are the infinities.
    for (expr, want) in [
        ("Math.floor(-1.5)", -2.0),
        ("Math.round(-0.5)", 0.0),
        ("Math.round(2.5)", 3.0),
        ("Math.sign(-0)", 0.0),
        ("Math.trunc(-4.7)", -4.0),
        ("Math.max()", f64::NEG_INFINITY),
        ("Math.min()", f64::INFINITY),
        ("Math.hypot(3, 4)", 5.0),
        ("Math.clz32(1)", 31.0),
        ("Math.imul(3, 4)", 12.0),
        ("Math.fround(5.5)", 5.5),
        ("Math.expm1(1)", 1.718_281_828_459_045),
        ("Math.log1p(1)", std::f64::consts::LN_2),
        ("Math.cbrt(27)", 3.0),
        ("Math.PI", std::f64::consts::PI),
        ("Math.SQRT2", std::f64::consts::SQRT_2),
        ("Math.LOG10E", std::f64::consts::LOG10_E),
        ("Math.abs(-2)", 2.0),
        ("Math.pow(2, 10)", 1024.0),
        ("Math.atan2(1, 1)", std::f64::consts::FRAC_PI_4),
    ] {
        let pat = eval(&format!("pure({expr})")).unwrap_or_else(|e| panic!("{expr}: {e}"));
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        let got = haps[0].value.as_f64().unwrap_or(f64::NAN);
        assert!(
            (got - want).abs() < 1e-12 || (got == want),
            "{expr}: got {got}, want {want}"
        );
    }
    // NaN propagates through max/min, as it does in JS.
    let nan = eval("pure(Math.max(1, 0/0))").expect("max with NaN");
    let v = nan.query_arc(Frac::zero(), Frac::one())[0].value.as_f64();
    assert!(v.is_none_or(f64::is_nan), "max(1, NaN) is NaN, got {v:?}");
    // `Math.random` is in range and does move.
    let draws: Vec<f64> = (0..8)
        .map(|_| {
            let p = eval("pure(Math.random())").expect("random");
            p.query_arc(Frac::zero(), Frac::one())[0]
                .value
                .as_f64()
                .unwrap_or(-1.0)
        })
        .collect();
    assert!(draws.iter().all(|d| (0.0..1.0).contains(d)), "{draws:?}");
    assert!(draws.windows(2).any(|w| w[0] != w[1]), "random never moved");
}

#[test]
fn set_gain_curve_installs_the_curve_it_was_given() {
    // superdough's own example: `setGainCurve((x) => x * x)` makes `.gain(0.5)`
    // sound like 0.25. The curve is global and persists across evaluations, as
    // the module-level one upstream does, so put it back afterwards.
    rudel_core::clear_gain_curve();
    assert_eq!(
        rudel_core::apply_gain_curve(0.5),
        0.5,
        "identity by default"
    );

    eval("setGainCurve(|x| x * x)\ns(\"bd\")").expect("install a curve");
    let squared = rudel_core::apply_gain_curve(0.5);
    assert!((squared - 0.25).abs() < 1e-6, "0.5 -> {squared}, want 0.25");
    // Sampled, so check across the range rather than at one point.
    for x in [0.0, 0.1, 0.75, 1.0, 2.0, 7.5] {
        let got = rudel_core::apply_gain_curve(x);
        assert!((got - x * x).abs() < 1e-3, "{x} -> {got}, want {}", x * x);
    }
    // Past the sampled range it carries on along the slope rather than
    // flattening, and a non-finite value is left alone.
    assert!(rudel_core::apply_gain_curve(12.0) > rudel_core::apply_gain_curve(9.0));
    assert!(rudel_core::apply_gain_curve(f64::NAN).is_nan());
    rudel_core::clear_gain_curve();
}

#[test]
fn javascript_string_arithmetic_and_the_methods_that_go_with_it() {
    // `register('mask' + n, …)` is how the binary-mask helper going round
    // strudel.cc names its methods, and Koto's `+` refuses a string and a
    // number outright. The rest of that helper —
    // `dec.toString(2).padStart(len, '0').split('').map(Number)` — is the same
    // family of JS builtins, and each was missing too.
    for (expr, want) in [
        ("'mask' + 4", "mask4"),
        ("1 + 2 + 'a'", "3a"), // folded left to right, as JS does
        ("'x' + 1 + 2", "x12"),
        ("(9).toString(2)", "1001"),
        ("(9).toString(2).padStart(6, '0')", "001001"),
        // A literal followed by `.method` is mini-notation here, so the string
        // methods are reached the way a script reaches them: through a name.
        ("text.padEnd(4, '-')", "ab--"),
        ("text.padStart(1, '-')", "ab"), // already long enough
        ("text.split('').join('.')", "a.b"),
        ("text.split('b').join('-')", "a-"),
    ] {
        let script = format!(
            "let text = 'ab'
pure({expr})"
        );
        let pat = eval(&script).unwrap_or_else(|e| panic!("{expr}: {e}"));
        let got = values(&pat, 0, 1);
        assert_eq!(got, vec![Value::Str(want.to_string())], "{expr}");
    }
}

#[test]
fn a_callback_may_hand_back_a_pattern_for_a_join_to_flatten() {
    // The shape every `register`ed helper is written in upstream:
    // `pat.fmap(v => <a pattern>).squeezeJoin()`. The callback's pattern used
    // to come back as null unless it had come from a mini-notation literal, so
    // the helper silenced whatever it was applied to.
    let pat = eval(r#"s("hh*4").mask("<x>".fmap(|v| seq(1)).squeezeJoin())"#)
        .expect("mask by a joined callback pattern");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::int(2)).len(), 8);
    // A literal still arrives as its own text, which is what a callback
    // returning a note name means.
    let named = eval(r#"pure(1).fmap(|v| "c3")"#).expect("literal from a callback");
    assert_eq!(values(&named, 0, 1), vec![Value::Str("c3".to_string())]);
}

#[test]
fn add_voicings_registers_a_dictionary_a_chord_can_name() {
    // `addVoicings`'s own jsdoc example, with the notes real Strudel gives for
    // it. The numeric key (`7:`) and the empty one both have to survive the
    // preprocessor and arrive as chord symbols.
    let script = r#"
addVoicings('koto_cookie', {
  7: ['3M 7m 9M 12P 15P', '7m 10M 13M 16M 19P'],
  '^7': ['3M 6M 9M 12P 14M', '7M 10M 13M 16M 19P'],
}, ['C3', 'C6'])
"<C^7>".voicings('koto_cookie')
"#;
    let pat = eval(script).expect("register and voice a dictionary");
    let notes: Vec<f64> = values(&pat, 0, 1)
        .iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("note").and_then(|n| n.as_f64()),
            _ => None,
        })
        .collect();
    assert_eq!(notes, vec![52.0, 57.0, 62.0, 67.0, 71.0]);
}

#[test]
fn speak_marks_the_words_to_say() {
    // `"<[i am] here>".speak('en', "<2 3>")` — the words become a control, and
    // the voice is sampled per cycle like any other patterned argument.
    let pat = eval(r#""<[i am] here>".speak('en', "<2 3>")"#).expect("speak");
    let spoken = |cycle: i64| {
        values(&pat, cycle, cycle + 1)
            .iter()
            .filter_map(|v| match v {
                Value::Map(m) => Some((
                    m.get("speak")?.as_str()?.to_string(),
                    m.get("speaklang")?.as_str()?.to_string(),
                    m.get("speakvoice")?.as_f64()?,
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // `[i am]` is a subsequence, so it is two haps and two utterances.
    assert_eq!(
        spoken(0),
        vec![
            ("i".into(), "en".into(), 2.0),
            ("am".into(), "en".into(), 2.0)
        ]
    );
    assert_eq!(spoken(1), vec![("here".into(), "en".into(), 3.0)]);
}

#[test]
fn set_max_polyphony_installs_the_cap_it_was_given() {
    // Process-global, as superdough's module-level `maxPolyphony` is, so put
    // the default back afterwards.
    eval("setMaxPolyphony(4)\ns(\"bd\")").expect("set the cap");
    assert_eq!(rudel_core::max_polyphony(), 4);
    // `parseInt` of something that is not a number leaves the default
    // standing, which is what upstream's `?? DEFAULT_MAX_POLYPHONY` intends.
    eval("setMaxPolyphony('lots')\ns(\"bd\")").expect("a nonsense cap");
    assert_eq!(
        rudel_core::max_polyphony(),
        rudel_core::DEFAULT_MAX_POLYPHONY
    );
}

#[test]
fn javascript_shifts_and_powers() {
    // Koto has `^` for a power and no shift operator at all, and a script
    // reaches for `>> 0` to truncate and `1 << n` to build a mask.
    for (expr, want) in [
        ("(40 / 12) >> 0", 3.0),
        ("1 << 4", 16.0),
        ("-9 >> 1", -5.0),
        ("1.5 ** 3", 3.375),
        ("2 ** 10 >> 2", 256.0),
        // JS truncates to a signed 32-bit integer first.
        ("4294967297 >> 0", 1.0),
        ("2147483648 >> 0", -2147483648.0),
    ] {
        let pat = eval(&format!("pure({expr})")).unwrap_or_else(|e| panic!("{expr}: {e}"));
        let got = values(&pat, 0, 1)[0].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 1e-9, "{expr}: got {got}, want {want}");
    }
}

#[test]
fn a_pattern_answers_the_arithmetic_operators() {
    // JavaScript has no operator overloading, so `"<1 2>" / 48` in Strudel is a
    // string over a number — `NaN` — and scripts write it meaning the
    // mini-notation. Koto asks the object, so it gets the pattern arithmetic.
    for (expr, want) in [
        ("pure(3) + 4", 7.0),
        ("4 + pure(3)", 7.0),
        ("pure(10) - 4", 6.0),
        ("10 - pure(4)", 6.0),
        ("pure(3) * 4", 12.0),
        ("pure(12) / 4", 3.0),
        ("pure(7) % 4", 3.0),
    ] {
        let pat = eval(expr).unwrap_or_else(|e| panic!("{expr}: {e}"));
        let got = values(&pat, 0, 1)[0].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 1e-9, "{expr}: got {got}, want {want}");
    }
}

#[test]
fn the_pattern_methods_a_script_reaches_for_by_upstream_name() {
    // `filterHaps` is upstream's own name for `filter`; `mod` is `modulo`,
    // which is only spelled that way here because Rust reserves the word.
    // Single quotes: a double-quoted literal is mini-notation here, so it
    // would arrive as a pattern rather than the sound's name.
    let kept = eval(r#"s("bd sd hh").filterHaps(|h| h.value.s != 'hh')"#).expect("filterHaps");
    assert_eq!(values(&kept, 0, 1).len(), 2);
    let modded = eval("pure(7).mod(4)").expect("mod");
    assert_eq!(values(&modded, 0, 1)[0].as_f64(), Some(3.0));
    // `restartJoin`/`resetJoin` flatten a pattern of patterns by retriggering.
    let joined = eval(r#""<0 1>".fmap(|v| seq(1, 2)).restartJoin()"#).expect("restartJoin");
    assert_eq!(values(&joined, 0, 2).len(), 4);
    // `setContext({})` clears the source locations the editor highlights from.
    let plain = eval(r#"s("bd").setContext({})"#).expect("setContext");
    let haps = plain.query_arc(Frac::zero(), Frac::one());
    assert!(haps[0].context.locations.is_empty(), "locations cleared");
    // `floor`/`ceil`/`round` standalone, as `n(floor(rand.range(1, 6)))` uses
    // them.
    let floored = eval("floor(pure(1.7))").expect("floor");
    assert_eq!(values(&floored, 0, 1)[0].as_f64(), Some(1.0));
}

#[test]
fn the_javascript_collection_builtins_helpers_call() {
    // `Array.from({length: n})` is how a script repeats something n times;
    // `flat`/`flatMap` and `Object.entries` are the rest of what they reach for.
    for (expr, want) in [
        ("Array.from({length: 3}).length()", 3.0),
        ("[[1, 2], [3]].flat().length()", 3.0),
        ("[1, 2].flatMap(|v| [v, v]).length()", 4.0),
        ("Object.entries({a: 1, b: 2}).length()", 2.0),
        ("Object.entries({a: 7})[0][1]", 7.0),
    ] {
        let pat = eval(&format!("pure({expr})")).unwrap_or_else(|e| panic!("{expr}: {e}"));
        let got = values(&pat, 0, 1)[0].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 1e-9, "{expr}: got {got}, want {want}");
    }
}
