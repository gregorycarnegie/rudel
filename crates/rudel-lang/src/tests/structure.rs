use super::common::*;

#[test]
fn newly_bound_transforms_resolve() {
    for s in [
        r#"note(0).hurry(2)"#,
        r#"seq(0, 1, 2, 3).focus(0, 0.5)"#,
        r#"seq(0, 1).press_by(0.5)"#,
        r#"s("x").euclid_rot(3, 8, 1)"#,
    ] {
        assert!(eval(s).is_ok(), "should eval: {s}");
    }
}

#[test]
fn alignment_via_koto() {
    // add.out takes structure from the right pattern -> 3 onsets
    let pat = eval(r#"seq(0, 1).add_out("10 20 30")"#).expect("eval");
    let onsets = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count();
    assert_eq!(onsets, 3);
    // set.squeeze merges the s control into each note event -> 4 haps
    let pat = eval(r#"note("0 1").set_squeeze(s("a b"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
}

#[test]
fn linger_invert_replicate_applyn_and_aliases() {
    // linger(0.25): repeat the first quarter (just "0") to fill the cycle.
    assert_eq!(
        values(&eval(r#""0 1 2 3".linger(0.25)"#).unwrap(), 0, 1),
        vec![Value::Int(0); 4]
    );
    // invert/inv: flip a boolean pattern.
    let inv: Vec<bool> = eval(r#""1 0 1".invert()"#)
        .unwrap()
        .query_arc(Frac::zero(), Frac::one())
        .iter()
        .map(|h| h.value.truthy())
        .collect();
    assert_eq!(inv, vec![false, true, false]);
    // replicate(2) == fast(2) here (one cycle of "0 1" repeated).
    assert_eq!(
        values(&eval(r#""0 1".replicate(2)"#).unwrap(), 0, 1),
        vec![Value::Int(0), Value::Int(1), Value::Int(0), Value::Int(1)]
    );
    // applyN(3, +1): apply the callback three times.
    assert_eq!(
        values(&eval(r#""0".applyN(3, |x| x.add(1))"#).unwrap(), 0, 1),
        vec![Value::Int(3)]
    );
    // aliases: sparsity == slow (method + standalone), sequence == seq,
    // polyrhythm == stack, nothing == silence.
    assert_eq!(
        values(&eval(r#""0 1".sparsity(2)"#).unwrap(), 0, 1),
        vec![Value::Int(0)]
    );
    assert_eq!(
        values(&eval(r#"sequence(0, 1, 2)"#).unwrap(), 0, 1).len(),
        3
    );
    assert_eq!(
        eval(r#"polyrhythm("0 1", "2 3 4")"#)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .len(),
        5
    );
    assert!(
        eval(r#"nothing()"#)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .is_empty()
    );
}

#[test]
fn chunk_jux_flip_and_keepif_variants() {
    let ns = |src: &str, b, e| -> Vec<i64> {
        eval(src)
            .unwrap()
            .query_arc(Frac::int(b), Frac::int(e))
            .iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("n").and_then(|x| x.as_f64()).map(|f| f as i64),
                _ => None,
            })
            .collect()
    };
    // fastChunk(2): cycle 0 bumps the 1st half, cycle 1 the 2nd (looped
    // subcycle, no slow-down). Matches Strudel.
    assert_eq!(
        ns(r#"n("0 1 2 3").fastChunk(2, |x| x.add(n(10)))"#, 0, 1),
        vec![10, 11, 2, 3]
    );
    assert_eq!(
        ns(r#"n("0 1 2 3").fastChunk(2, |x| x.add(n(10)))"#, 1, 2),
        vec![0, 1, 12, 13]
    );
    // slowChunk is an alias for chunk.
    assert_eq!(
        ns(r#"n("0 1 2 3").slowChunk(2, |x| x.add(n(10)))"#, 0, 1),
        vec![10, 11, 2, 3]
    );
    // keepif keeps the control value where the bool is truthy (drops the rest),
    // even on a control pattern (unlike the merging composers).
    assert_eq!(ns(r#"n("0 1 2 3").keepif("1 0 1 0")"#, 0, 1), vec![0, 2]);
    assert_eq!(ns(r#"keepif("1 0", n("5 6"))"#, 0, 1), vec![5]);
    // juxFlip/flux pans copies left/right, swapping ears each cycle.
    let pans = |b, e| -> Vec<f64> {
        let mut ps: Vec<f64> = eval(r#"n("0").juxFlip(rev)"#)
            .unwrap()
            .query_arc(Frac::int(b), Frac::int(e))
            .iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("pan").and_then(|x| x.as_f64()),
                _ => None,
            })
            .collect();
        ps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ps
    };
    assert_eq!(pans(0, 1), vec![0.0, 1.0]);
    assert_eq!(pans(1, 2), vec![0.0, 1.0]);
}

#[test]
fn comparison_and_logic_composers() {
    // Boolean composers (pattern.mjs COMPOSERS). On plain values they compare;
    // their main use is gating `struct`/`mask`. Verified against Strudel.
    let b = |src: &str| -> Vec<bool> {
        eval(src)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .iter()
            .map(|h| h.value.truthy())
            .collect()
    };
    assert_eq!(b(r#""0 1 2 3".lte(1)"#), vec![true, true, false, false]);
    assert_eq!(b(r#""0 1 2 3".gt(1)"#), vec![false, false, true, true]);
    assert_eq!(b(r#""a b a".eq("a")"#), vec![true, false, true]);
    assert_eq!(b(r#""0 1 2".ne(1)"#), vec![true, false, true]);
    assert_eq!(b(r#""0 1 2 3".gte(2)"#), vec![false, false, true, true]);
    assert_eq!(b(r#""1 2 3".eqt(2)"#), vec![false, true, false]);
    // and/or pick an operand by left-truthiness (method-only: Koto keywords).
    assert_eq!(b(r#""1 0 1".and("1 1 0")"#), vec![true, false, false]);
    assert_eq!(b(r#""1 0 0".or("0 1 0")"#), vec![true, true, false]);
    // idiomatic use: gate a struct.
    let pat = eval(r#"s("a*4").struct("0 1 2 3".gt(1))"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
}

#[test]
fn chop_via_koto() {
    let pat = eval(r#"s("bd").chop(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
}

#[test]
fn slice_via_koto() {
    let pat = eval(r#"s("bd").slice(4, "0 2")"#).expect("eval");
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 2);
    match &haps[0].value {
        Value::Map(m) => assert_eq!(m.get("begin"), Some(&Value::F64(0.0))),
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn bite_via_koto() {
    // bite(4, "0 2") picks pattern slices 0 and 2, squeezed into each step.
    let pat = eval(r#"s("a b c d").bite(4, "0 2")"#).expect("eval");
    let vals = values(&pat, 0, 1);
    assert_eq!(vals.len(), 2);
    let names: Vec<Option<&str>> = vals
        .iter()
        .map(|v| match v {
            Value::Map(m) => m.get("s").and_then(|x| x.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec![Some("a"), Some("c")]);
    // standalone form takes the pattern last
    let standalone = eval(r#"bite(4, "0 2", s("a b c d"))"#).expect("eval");
    assert_eq!(shape(&standalone, 1), shape(&pat, 1));
}

#[test]
fn loop_at_cps_via_koto() {
    // loopAtCps(2, 1.0): speed = (1/2)*1 = 0.5, unit 'c'.
    let pat = eval(r#"s("bd").loopAtCps(2, 1.0)"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => {
            assert_eq!(m.get("speed").and_then(|v| v.as_f64()), Some(0.5));
            assert_eq!(m.get("unit").and_then(|v| v.as_str()), Some("c"));
        }
        other => panic!("expected control map, got {other:?}"),
    }
}

#[test]
fn euclidish_and_eish_via_koto() {
    // perc=0 == euclid(3,8); the eish alias resolves to the same.
    let morphed = eval(r#"s("bd").euclidish(3, 8, 0)"#).expect("eval");
    let plain = eval(r#"s("bd").euclid(3, 8)"#).expect("eval");
    assert_eq!(shape(&morphed, 1), shape(&plain, 1));
    let eish = eval(r#"s("bd").eish(3, 8, 0)"#).expect("eval");
    assert_eq!(shape(&eish, 1), shape(&plain, 1));
    // standalone form takes the pattern last.
    let standalone = eval(r#"euclidish(3, 8, 0, s("bd"))"#).expect("eval");
    assert_eq!(shape(&standalone, 1), shape(&plain, 1));
}

#[test]
fn euclidish_samples_continuous_perc_per_cycle() {
    // With a continuous `perc` (`sine.slow(8)`), Strudel samples `by` once per
    // cycle via appLeft (not once at the query start). So a single [0,2) query
    // must equal querying each cycle separately, and the two cycles must differ
    // (proving per-cycle sampling rather than a shifted copy).
    let pat = eval(r#"s("bd").euclidish(7, 12, sine.slow(8))"#).expect("eval");
    let onset = |b: i64, e: i64| -> Vec<Frac> {
        let mut hs = pat.query_arc(Frac::int(b), Frac::int(e));
        hs.sort_by_key(|h| h.part.begin);
        hs.iter().map(|h| h.part.begin).collect()
    };
    let single = onset(0, 2);
    let mut split = onset(0, 1);
    split.extend(onset(1, 2));
    assert_eq!(
        single, split,
        "single multi-cycle query must match per-cycle"
    );

    // relative onsets within each cycle differ -> `by` was re-sampled per cycle.
    let cyc0: Vec<Frac> = onset(0, 1);
    let cyc1: Vec<Frac> = onset(1, 2).iter().map(|t| *t - Frac::one()).collect();
    assert_ne!(cyc0, cyc1, "continuous perc should vary by cycle");
}

#[test]
fn stack_alignment_variants() {
    // Pad shorter patterns to the longest's step count. Verified against
    // Strudel: stackLeft pads "a b" on the right, stackRight on the left.
    let begins = |src: &str| -> Vec<(f64, String)> {
        let mut hs = eval(src).unwrap().query_arc(Frac::zero(), Frac::one());
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .map(|h| {
                let v = match &h.value {
                    Value::Str(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                ((h.part.begin.to_f64() * 100.0).round() / 100.0, v)
            })
            .collect()
    };
    // "a b" (2 steps) padded to 3 with a trailing gap -> a@0, b@0.33.
    let left = begins(r#"stackLeft("0 1 2", "a b")"#);
    assert!(left.contains(&(0.0, "a".into())) && left.contains(&(0.33, "b".into())));
    // padded on the left instead -> a@0.33, b@0.67.
    let right = begins(r#"stackRight("0 1 2", "a b")"#);
    assert!(right.contains(&(0.33, "a".into())) && right.contains(&(0.67, "b".into())));
    // centred over 4 steps -> a@0.25, b@0.5.
    let centre = begins(r#"stackCentre("0 1 2 3", "a b")"#);
    assert!(centre.contains(&(0.25, "a".into())) && centre.contains(&(0.5, "b".into())));
}

#[test]
fn bjork_tuple_via_koto() {
    // bjork([3,8,2]) == euclidRot(3,8,2), as method and standalone.
    let bjork = eval(r#"s("bd").bjork([3, 8, 2])"#).expect("eval");
    let rot = eval(r#"s("bd").euclidRot(3, 8, 2)"#).expect("eval");
    assert_eq!(shape(&bjork, 1), shape(&rot, 1));
    let standalone = eval(r#"bjork([3, 8, 2], s("bd"))"#).expect("eval");
    assert_eq!(shape(&standalone, 1), shape(&rot, 1));
}

#[test]
fn factories_stepcat_arrange_polymeter() {
    // stepcat("0 1 2", "3 4") -> 5 evenly-weighted steps
    let pat = eval(r#"stepcat("0 1 2", "3 4")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 5);
    // explicit [weight, pat] pairs: "0"@3 then "1" -> 2 onsets, 0 dominates
    let pat = eval(r#"stepcat([3, "0"], [1, "1"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
    // arrange: "0" for 2 cycles, "1" for 1
    let pat = eval(r#"arrange([2, "0"], [1, "1"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 2, 3)[0], Value::Int(1));
    // polymeter / pm align to lcm(3,2)=6 steps -> 12 haps stacked
    let pat = eval(r#"polymeter("0 1 2", "4 5")"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 12);
    assert!(eval(r#"pm("0 1", "2 3 4")"#).is_ok());
}

#[test]
fn take_drop_scan_via_koto() {
    // seq(0,1,2,3).take(2) -> "0 1"; drop(1) -> "1 2 3"
    let pat = eval(r#"seq(0, 1, 2, 3).take(2)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
    let pat = eval(r#"seq(0, 1, 2, 3).drop(1)"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    // scan(3): cycle 0 -> [0], cycle 2 -> [0 1 2]
    let pat = eval(r#"scan(3)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
    assert_eq!(
        values(&pat, 2, 3),
        vec![Value::Int(0), Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn shuffle_scramble_tour_zip_via_koto() {
    // shuffle(4): a permutation — each cycle plays every part exactly once.
    let pat = eval(r#"pat("0 1 2 3").shuffle(4)"#).expect("eval");
    for c in 0..4 {
        let mut v = values(&pat, c, c + 1);
        v.sort_by_key(|x| x.as_f64().map(|f| f as i64));
        assert_eq!(
            v,
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)],
            "shuffle cycle {c} should be a permutation"
        );
    }
    // scramble(4): four parts per cycle, possibly with repeats.
    let pat = eval(r#"pat("0 1 2 3").scramble(4)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
    // randrun(3): a permutation of 0..3 each cycle.
    let pat = eval(r#"randrun(3)"#).expect("eval");
    let mut v = values(&pat, 1, 2);
    v.sort_by_key(|x| x.as_f64().map(|f| f as i64));
    assert_eq!(v, vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
    // tour with one pattern: "a b" + x appended, then x prepended, all in one
    // cycle: "a b x x a b" stepwise.
    let pat = eval(r#"pat("x").tour("a b")"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![
            Value::Str("a".into()),
            Value::Str("b".into()),
            Value::Str("x".into()),
            Value::Str("x".into()),
            Value::Str("a".into()),
            Value::Str("b".into()),
        ]
    );
    // zip: steps interleave — step k of each pattern in turn ("a c", then "b d").
    let pat = eval(r#"zip("a b", "c d")"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Str("a".into()), Value::Str("c".into())]
    );
    assert_eq!(
        values(&pat, 1, 2),
        vec![Value::Str("b".into()), Value::Str("d".into())]
    );
    // `steps` (alias of pace) and the deprecated s_* stepwise aliases resolve.
    let pat = eval(r#"zip("a b", "c d").steps(4)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
    assert!(eval(r#"s_zip("a b", "c d")"#).is_ok());
    assert!(eval(r#"pat("x").s_tour("a b")"#).is_ok());
    assert!(eval(r#"pat("0 1 2 3").s_taper(1)"#).is_ok());
    assert!(eval(r#"pat("0 1 2 3").s_add(2)"#).is_ok());
    assert!(eval(r#"s_cat("0 1", "2")"#).is_ok());
    assert!(eval(r#"s_alt(["0 1", "2"], "3")"#).is_ok());
}

#[test]
fn weighted_choosers_and_stepalt_via_koto() {
    // wrandcat: heavy weight on 0 dominates, one value per cycle
    let pat = eval(r#"wrandcat([0, 1000], [1, 1])"#).expect("eval");
    let mut zeros = 0;
    for c in 0..12 {
        let v = values(&pat, c, c + 1);
        assert_eq!(v.len(), 1);
        if v[0] == Value::Int(0) {
            zeros += 1;
        }
    }
    assert!(zeros >= 10, "heavy weight should dominate (got {zeros}/12)");
    // wchooseCycles is the same function; wchoose evaluates as continuous
    assert!(eval(r#"wchooseCycles(["a", 2], ["b", 1])"#).is_ok());
    assert!(eval(r#"wchoose([0, 1], [1, 1]).segment(4)"#).is_ok());
    // stepalt(["0 1", "2"], "3") == "0 1 3 2 3"
    let pat = eval(r#"stepalt(["0 1", "2"], "3")"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(3),
            Value::Int(2),
            Value::Int(3),
        ]
    );
}

#[test]
fn ribbon_and_seg_via_koto() {
    // ribbon loops the window [1,3) of "<0 1 2 3>": cycle 0 -> 1, cycle 2 -> 1
    let pat = eval(r#"n("<0 1 2 3>").ribbon(1, 2)"#).expect("eval");
    let n_at = |c: i64| match &pat.query_arc(Frac::int(c), Frac::int(c + 1))[0].value {
        Value::Map(m) => m.get("n").and_then(|v| v.as_f64()).unwrap(),
        other => other.as_f64().unwrap(),
    };
    assert_eq!(n_at(0), 1.0);
    assert_eq!(n_at(1), 2.0);
    assert_eq!(n_at(2), 1.0); // looped
    // rib alias resolves; seg == segment (8 discrete events)
    assert!(eval(r#"n("<0 1>").rib(0, 1)"#).is_ok());
    let pat = eval(r#"rand.seg(8)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 8);
}

#[test]
fn overlay_and_pace_via_koto() {
    let pat = eval(r#"seq(0).overlay(7)"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
    let pat = eval(r#"seq(0, 1, 2).pace(4)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
}

#[test]
fn camel_case_aliases_resolve() {
    // Strudel-style camelCase aliases should evaluate without error.
    for src in [
        r#"seq(0, 1, 2).iterBack(2)"#,
        r#"s("bd sd").fastGap(2)"#,
        r#"seq(0, 1).repeatCycles(2)"#,
        r#"seq(0, 1).pressBy(0.5)"#,
        r#"seq(0, 1, 2, 3).swingBy(0.25, 2)"#,
        r#"s("x").euclidRot(3, 8, 1)"#,
        r#"note("c3").euclidLegato(3, 8)"#,
        r#"note("c3").euclidLegatoRot(3, 5, 2)"#,
        r#"n("0").scale("C:major").scaleTranspose(2)"#,
        r#"n("0").scale("C:major").scaleTrans(2)"#,
        r#"pure("Am7").rootNotes(3)"#,
        r#"s("bd").loopAt(2)"#,
        r#"sine.toBipolar()"#,
        r#"sine.fromBipolar()"#,
        r#"seq(0, 1).firstOf(2, |x| x.add(10))"#,
        r#"seq(0, 1).lastOf(2, |x| x.add(10))"#,
        r#"seq(0, 1, 2, 3).chunkBack(2, |x| x.add(10))"#,
        r#"note("0 1").juxBy(0.5, rev)"#,
        r#"seq(0, 1).sometimesBy(0.5, |x| x.add(7))"#,
        r#"seq(0, 1).someCycles(|x| x.add(7))"#,
        r#"seq(0, 1).someCyclesBy(0.5, |x| x.add(7))"#,
        r#"seq(0, 1).almostAlways(|x| x.add(7))"#,
        r#"seq(0, 1).almostNever(|x| x.add(7))"#,
    ] {
        assert!(eval(src).is_ok(), "should eval: {src}");
    }
}

#[test]
fn step_count_transforms_via_koto() {
    // contract halves the step count; shrink/grow concatenate shrinking views.
    let pat = eval(r#"seq(0, 1, 2, 3).contract(2)"#).expect("eval");
    assert_eq!(pat.steps, Some(Frac::int(2)));
    let pat = eval(r#"seq(0, 1, 2, 3).shrink(1)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 10);
    let pat = eval(r#"seq(0, 1, 2, 3).grow(1)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 10);
}
