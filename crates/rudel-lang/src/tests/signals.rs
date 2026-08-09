use super::common::*;

#[test]
fn range_scales_signal() {
    let pat = eval(r#"seq(0, 1).range(10, 20)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(10.0), Value::F64(20.0)]);
}

#[test]
fn signals_are_values_and_segment() {
    // sine is a value (no parens) and can be segmented + ranged
    let pat = eval(r#"sine.range(0, 10).segment(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    // run(4) -> 0 1 2 3
    let pat = eval(r#"run(4)"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    // rand / perlin / saw2 usable bare
    for s in [
        "rand.segment(8)",
        "perlin.segment(8)",
        "saw2.segment(4)",
        "irand(8).segment(4)",
    ] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }
}

#[test]
fn signal_module_additions_via_koto() {
    // The newly exposed signal.mjs members all parse and segment as values/fns.
    for s in [
        "itri.segment(4)",
        "itri2.segment(4)",
        "berlin.segment(8)",
        "brand.segment(8)",
        "brandBy(0.3).segment(8)",
        "steady(0.5).segment(4)",
        "per.struct(\"1 1\")",
        "perCycle.struct(\"1 1\")",
        "cyclesPer.struct(\"1 1\")",
        "perx.struct(\"1 1\")",
        "choose(0, 1, 2).segment(8)",
        "chooseIn(0, 1, 2).segment(8)",
        "chooseOut(0, 1, 2).segment(8)",
        "sine.choose(\"a\", \"b\", \"c\").segment(8)",
        "rand2.choose2(\"a\", \"b\").segment(8)",
    ] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }

    // itri is the mirror of tri: tri rises 0->1 over the cycle, itri falls 1->0.
    let tri = values(&eval("tri.segment(4)").unwrap(), 0, 1);
    let itri = values(&eval("itri.segment(4)").unwrap(), 0, 1);
    let nums = |vs: Vec<Value>| vs.iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert_eq!(nums(tri), vec![0.0, 0.5, 1.0, 0.5]);
    assert_eq!(nums(itri), vec![1.0, 0.5, 0.0, 0.5]);

    // seed(n) changes which events `degrade` keeps (compare kept onsets).
    let onsets = |src: &str| -> Vec<Frac> {
        let mut bs: Vec<Frac> = eval(src)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.part.begin)
            .collect();
        bs.sort();
        bs
    };
    assert_ne!(
        onsets(r#"s("hh*8").degrade()"#),
        onsets(r#"s("hh*8").degrade().seed(1)"#)
    );

    // degradeBy / undegradeBy are bound as methods (snake_case and camelCase),
    // and are complementary: an event kept by one is dropped by the other.
    for src in [
        r#"s("hh*8").degradeBy(0.3)"#,
        r#"s("hh*8").degrade_by(0.3)"#,
        r#"s("hh*8").undegradeBy(0.3)"#,
        r#"s("hh*8").undegrade_by(0.3)"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
    let kept = onsets(r#"s("hh*8").degradeBy(0.4)"#);
    let dropped = onsets(r#"s("hh*8").undegradeBy(0.6)"#);
    // degradeBy(x) keeps events where rand >= x; undegradeBy(1-x) keeps the
    // complement, so together they partition all 8 onsets without overlap.
    assert_eq!(kept.len() + dropped.len(), 8);
    assert!(kept.iter().all(|b| !dropped.contains(b)));

    // degradeByWith drives the degradation from an arbitrary pattern instead of
    // the built-in `rand`, as a method, camelCase, and pattern-last standalone.
    // `saw` rises 0..1 over the cycle and is sampled at each hap's start, so
    // the strict `> 0.5` keeps the steps after the midpoint (0.5 itself fails).
    let expected: Vec<Frac> = (5..8).map(|i| Frac::new(i, 8)).collect();
    for src in [
        r#"s("hh*8").degradeByWith(saw, 0.5)"#,
        r#"s("hh*8").degrade_by_with(saw, 0.5)"#,
        r#"degradeByWith(saw, 0.5, s("hh*8"))"#,
        // curried: the trailing pattern arrives in a later call
        r#"degradeByWith(saw, 0.5)(s("hh*8"))"#,
    ] {
        assert_eq!(onsets(src), expected, "degradeByWith: {src}");
    }
}

#[test]
fn factories_resolve() {
    // slowcat: one value per cycle
    let pat = eval(r#"slowcat(0, 1, 2)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(1));
    // pure literal, gap silence, fastcat/randcat resolve
    assert_eq!(
        values(&eval("pure(60)").unwrap(), 0, 1),
        vec![Value::Int(60)]
    );
    assert!(
        eval("gap(2)")
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .is_empty()
    );
    for s in ["fastcat(0, 1, 2)", "randcat(0, 1)", "chooseCycles(0, 1)"] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }
}

#[test]
fn binary_and_bitwise_via_koto() {
    // binary(5) -> "1 0 1": three steps.
    let pat = eval("binary(5)").expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(1), Value::Int(0), Value::Int(1)]
    );
    // binaryN with explicit width matches Strudel's documented example length.
    assert_eq!(values(&eval("binaryN(55532, 16)").unwrap(), 0, 1).len(), 16);
    // bitwise composer methods and a pattern-last standalone.
    assert_eq!(
        values(&eval("pure(6).band(3)").unwrap(), 0, 1),
        vec![Value::Int(2)]
    );
    assert_eq!(
        values(&eval("pure(1).blshift(3)").unwrap(), 0, 1),
        vec![Value::Int(8)]
    );
    assert_eq!(
        values(&eval("brshift(2, pure(16))").unwrap(), 0, 1),
        vec![Value::Int(4)]
    );
}

#[test]
fn binary_lists_and_randl_via_koto() {
    // binaryL(5) packs the bits into a 3-element list value.
    match &values(&eval("binaryL(5)").unwrap(), 0, 1)[0] {
        Value::List(items) => assert_eq!(items.len(), 3),
        other => panic!("expected a list, got {other:?}"),
    }
    // randL(8) is a list of 8 random numbers.
    match &values(&eval("randL(8)").unwrap(), 0, 1)[0] {
        Value::List(items) => assert_eq!(items.len(), 8),
        other => panic!("expected a list, got {other:?}"),
    }
}

#[test]
fn degrade_by_patternifies_its_amount() {
    // Upstream registers `degradeBy` with patternify, so a signal or mini
    // pattern is sampled per cycle. Collapsing it to one number kept events
    // upstream drops — tunes use `degradeBy(sine.range(0,.5).slow(32))` to make
    // the density breathe.
    let literal = eval(r#"note("0 1 2 3").degradeBy(0.5)"#).expect("eval");
    let patterned = eval(r#"note("0 1 2 3").degradeBy(pure(0.5))"#).expect("eval");
    assert_eq!(
        values(&literal, 0, 1),
        values(&patterned, 0, 1),
        "a pure pattern must decide exactly as the bare number does"
    );
    // A per-cycle amount really does vary. Cycle 1's amount of 1 drops
    // everything; cycle 0's amount of 0 keeps what a bare 0 would (upstream's
    // filter is `v > x`, so a `rand` of exactly 0 goes either way — which is
    // why this compares against the literal rather than asserting all four).
    let alternating = eval(r#"note("0 1 2 3").degradeBy("<0 1>")"#).expect("eval");
    let zero = eval(r#"note("0 1 2 3").degradeBy(0)"#).expect("eval");
    assert_eq!(values(&alternating, 0, 1), values(&zero, 0, 1));
    assert!(!values(&alternating, 0, 1).is_empty());
    assert_eq!(values(&alternating, 1, 2).len(), 0);
}
