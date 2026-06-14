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
