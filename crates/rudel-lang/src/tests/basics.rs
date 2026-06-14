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
fn standalone_transform_names_are_all_registered() {
    // Completeness guard: every transform that should have a standalone form is
    // exposed as a top-level function (in both snake and camelCase where they
    // differ).
    let reference = crate::reference();
    let funcs: std::collections::HashSet<&str> =
        reference.functions.iter().map(String::as_str).collect();
    let expected = [
        "fast",
        "slow",
        "ply",
        "segment",
        "add",
        "sub",
        "mul",
        "div",
        "early",
        "late",
        "fastGap",
        "fast_gap",
        "transpose",
        "scaleTranspose",
        "scaleTrans",
        "palindrome",
        "degrade",
        "undegrade",
        "press",
        "brak",
        "iter",
        "iterBack",
        "repeatCycles",
        "expand",
        "extend",
        "contract",
        "shrink",
        "grow",
        "chop",
        "striate",
        "take",
        "drop",
        "rootNotes",
        "shuffle",
        "scramble",
        "degradeBy",
        "undegradeBy",
        "hurry",
        "swing",
        "pressBy",
        "loopAt",
        "pace",
        "seed",
        "range",
        "range2",
        "rangex",
        "focus",
        "compress",
        "zoom",
        "ribbon",
        "rib",
        "swingBy",
        "euclid",
        "euclidLegato",
        "euclidRot",
        "euclidLegatoRot",
        "echo",
        "stut",
        "slice",
        "splice",
        // callbacks
        "superimpose",
        "jux",
        "sometimes",
        "often",
        "rarely",
        "almostAlways",
        "almostNever",
        "someCycles",
        "apply",
        "always",
        "never",
        "every",
        "firstOf",
        "lastOf",
        "chunk",
        "chunkBack",
        "juxBy",
        "sometimesBy",
        "someCyclesBy",
        "inside",
        "outside",
        "off",
        "when",
        "within",
    ];
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|n| !funcs.contains(n))
        .collect();
    assert!(
        missing.is_empty(),
        "standalone functions not registered: {missing:?}"
    );
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
