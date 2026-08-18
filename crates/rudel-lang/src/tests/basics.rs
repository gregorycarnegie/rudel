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
