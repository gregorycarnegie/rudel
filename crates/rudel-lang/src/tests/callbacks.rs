use super::common::*;

#[test]
fn every_with_koto_callback() {
    // every(2, |x| x.add(10)): cycle 0 -> 10, cycle 1 -> 0
    let pat = eval(r#"seq(0).every(2, |x| x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));
}

#[test]
fn superimpose_with_koto_callback() {
    // superimpose(|x| x.add(7)) over a single value -> two haps
    let pat = eval(r#"seq(0).superimpose(|x| x.add(7))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(7)]);
}

#[test]
fn jux_with_koto_callback() {
    let pat = eval(r#"note("0 1").jux(|x| x.rev())"#).expect("eval");
    let pans: Vec<f64> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter_map(|h| match h.value {
            Value::Map(m) => m.get("pan").and_then(|v| v.as_f64()),
            _ => None,
        })
        .collect();
    assert!(pans.contains(&0.0) && pans.contains(&1.0));

    let pat = eval(r#"note("0 1").jux(rev)"#).expect("eval");
    assert!(!pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn within_with_koto_callback() {
    // apply +10 only to the first 40% of the cycle -> events 0 and 1
    let pat = eval(r#"seq(0, 1, 2, 3).within(0, 0.4, |x| x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(11), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn chunk_with_koto_callback() {
    // chunk(4, +10): first element bumped on cycle 0
    let pat = eval(r#"seq(0, 1, 2, 3).chunk(4, |x| x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn callback_combinators_accept_patterned_args() {
    // The Koto VM can't run in the query path, so a patterned leading arg is
    // resolved by probing distinct values and baking the combinator result per
    // value, then selecting per cycle. Verified hap-for-hap against Strudel.
    let n_of = |pat: &rudel_core::Pattern, b, e| -> Vec<i64> {
        let mut hs = pat.query_arc(Frac::int(b), Frac::int(e));
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("n").and_then(|x| x.as_f64()).map(|f| f as i64),
                _ => None,
            })
            .collect()
    };

    // chunk("<2 4>"): cycle 0 bumps the 1st half (n=2), cycle 1 the 2nd
    // quarter (n=4).
    let pat = eval(r#"n("0 1 2 3").chunk("<2 4>", |x| x.add(n(10)))"#).expect("eval");
    assert_eq!(n_of(&pat, 0, 1), vec![10, 11, 2, 3]);
    assert_eq!(n_of(&pat, 1, 2), vec![0, 11, 2, 3]);

    // inside("<2 4>", rev): the fast/slow factor varies per cycle.
    let inside = eval(r#"s("a b c d").inside("<2 4>", rev)"#).expect("eval");
    let names = |b, e| -> Vec<String> {
        let mut hs = inside.query_arc(Frac::int(b), Frac::int(e));
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("s").and_then(|x| x.as_str()).map(String::from),
                _ => None,
            })
            .collect()
    };
    assert_eq!(names(0, 1), vec!["b", "a", "d", "c"]);
    assert_eq!(names(1, 2), vec!["a", "b", "c", "d"]);

    // sometimesBy("<0 1>") — the randomized probability varies per cycle
    // (camelCase routes through the patternified path too).
    let sby = eval(r#"n("0*4").sometimesBy("<0 1>", |x| x.add(n(10)))"#).expect("eval");
    assert!(n_of(&sby, 0, 1).iter().all(|&v| v == 0)); // prob 0
    assert_eq!(n_of(&sby, 1, 2), vec![10, 10, 10, 10]); // prob 1

    // within with a patterned bound.
    let within = eval(r#"n("0 1 2 3").within("<0 0.5>", 0.5, |x| x.add(n(10)))"#).expect("eval");
    assert_eq!(n_of(&within, 0, 1), vec![10, 11, 12, 3]);
    assert_eq!(n_of(&within, 1, 2), vec![0, 1, 12, 3]);

    // a scalar leading arg still uses the direct fast path.
    let scalar = eval(r#"n("0 1 2 3").chunk(4, |x| x.add(n(10)))"#).expect("eval");
    assert_eq!(n_of(&scalar, 0, 1), vec![10, 1, 2, 3]);
}

#[test]
fn off_with_koto_callback() {
    // off(0.25, +12) stacks a shifted, transposed copy: two onsets per cycle
    let pat = eval(r#"note(0).off(0.25, |x| x.add(12))"#).expect("eval");
    let onsets = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count();
    assert_eq!(onsets, 2);
}

#[test]
fn layer_stacks_callback_results() {
    // layer([|x| x.add(0), |x| x.add(7)]) over a single value -> two haps
    let pat = eval(r#"seq(0).layer([|x| x.add(0), |x| x.add(7)])"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
}

#[test]
fn apply_always_never_via_koto() {
    // apply/always run the callback; never leaves the pattern unchanged.
    let pat = eval(r#"seq(0).apply(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).always(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).never(|x| x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
}

#[test]
fn every_first_last_accept_a_patterned_cycle_count() {
    // every("<2 1>", rev): cycle 0 uses n=2 (0 mod 2 == 0 -> applied), cycle 1
    // uses n=1 (every cycle) -> both cycles reversed. Matches Strudel.
    let pat = eval(r#"s("a b").every("<2 1>", rev)"#).expect("eval");
    let names = |b, e| -> Vec<String> {
        values(&pat, b, e)
            .iter()
            .filter_map(|v| match v {
                Value::Map(m) => m.get("s").and_then(|x| x.as_str()).map(String::from),
                _ => None,
            })
            .collect()
    };
    assert_eq!(names(0, 1), vec!["b", "a"]);
    assert_eq!(names(1, 2), vec!["b", "a"]);

    // scalar still works: every(2) applies on cycle 0 only.
    let pat = eval(r#"seq(0).every(2, |x| x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));

    // lastOf places the transform on the last cycle of each group.
    let pat = eval(r#"seq(0).lastOf(2, |x| x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(10));

    // standalone form (pattern last) honours the patterned count too.
    let pat = eval(r#"every("<1 2>", |x| x.add(10), seq(0))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10)); // n=1 -> applied
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0)); // n=2 -> 1 mod 2 != 0
}

#[test]
fn bool_literals_become_boolean_patterns() {
    // A bare Koto `true`/`false` reifies to `pure(true/false)` (Strudel's
    // `reify(true)`), so `when`/`struct` accept bool literals.
    let pat = eval(r#"n("0 1").when(true, rev)"#).expect("eval");
    let ns: Vec<f64> = values(&pat, 0, 1)
        .iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("n").and_then(|x| x.as_f64()),
            _ => None,
        })
        .collect();
    assert_eq!(ns, vec![1.0, 0.0]); // reversed
    let pat = eval(r#"n("0 1").when(false, rev)"#).expect("eval");
    let ns: Vec<f64> = values(&pat, 0, 1)
        .iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("n").and_then(|x| x.as_f64()),
            _ => None,
        })
        .collect();
    assert_eq!(ns, vec![0.0, 1.0]); // unchanged
    // struct with a bool keeps (true) or drops (false) the event.
    assert_eq!(
        eval(r#"n("0").struct(true)"#)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .len(),
        1
    );
    assert_eq!(
        eval(r#"n("0").struct(false)"#)
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .len(),
        0
    );
}

#[test]
fn echo_with_passes_the_index_to_the_callback() {
    // echoWith(3, 0.25, f): three copies, each f(copy, i). A two-arg callback
    // gets the index; a one-arg callback ignores it (Koto arity fallback).
    let ns = |src: &str| -> Vec<i64> {
        let mut hs = eval(src).unwrap().query_arc(Frac::zero(), Frac::one());
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("n").and_then(|x| x.as_f64()).map(|f| f as i64),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        ns(r#"n("0").echoWith(3, 0.25, |x, i| x.add(n(i)))"#),
        vec![0, 1, 2, 1, 2]
    );
    // one-arg callback still works (index ignored).
    assert_eq!(
        ns(r#"n("0").echoWith(3, 0.25, |x| x.add(n(10)))"#),
        vec![10, 10, 10, 10, 10]
    );
    // stutWith is an alias; standalone takes the pattern last.
    assert_eq!(
        ns(r#"stutWith(3, 0.25, |x, i| x.add(n(i)), n("0"))"#),
        vec![0, 1, 2, 1, 2]
    );
}

#[test]
fn ply_with_and_ply_for_each() {
    // plyWith(3, +10): each event becomes [x, x+10, x+20] within its step.
    let vals = |src: &str| -> Vec<i64> {
        let mut hs = eval(src).unwrap().query_arc(Frac::zero(), Frac::one());
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .filter_map(|h| h.value.as_f64().map(|f| f as i64))
            .collect()
    };
    assert_eq!(
        vals(r#""0 1".plyWith(3, |x| x.add(10))"#),
        vec![0, 10, 20, 1, 11, 21]
    );
    // plyForEach(3, (p,n) => p+n*2): first copy untransformed, then index-scaled.
    assert_eq!(
        vals(r#""0 1".plyForEach(3, |p, n| p.add(n * 2))"#),
        vec![0, 2, 4, 1, 3, 5]
    );
    // standalone form takes the pattern last.
    assert_eq!(
        vals(r#"plyWith(3, |x| x.add(10), "0 1")"#),
        vec![0, 10, 20, 1, 11, 21]
    );
}

#[test]
fn into_and_chunk_into() {
    // into("1 0", f): the first half (piece "1") is looped and transformed by f,
    // the second half ("0") plays unchanged. Verified hap-for-hap vs Strudel.
    let names = |src: &str| -> Vec<String> {
        let mut hs = eval(src).unwrap().query_arc(Frac::zero(), Frac::one());
        hs.sort_by_key(|h| h.part.begin);
        hs.iter()
            .filter_map(|h| match &h.value {
                Value::Map(m) => m.get("s").and_then(|x| x.as_str()).map(String::from),
                _ => None,
            })
            .collect()
    };
    // hurry(2) on the looped first half -> "bd sd" played twice in [0,0.5).
    assert_eq!(
        names(r#"s("bd sd ht lt").into("1 0", |x| x.hurry(2))"#),
        vec!["bd", "sd", "bd", "sd", "ht", "lt"]
    );
    // chunkInto(4): cycle 0 hurries the first quarter (looped) -> bd, bd, ...
    assert_eq!(
        names(r#"s("bd sd ht lt").chunkInto(4, |x| x.hurry(2))"#),
        vec!["bd", "bd", "sd", "ht", "lt"]
    );
    // standalone form takes the pattern last.
    assert_eq!(
        names(r#"into("1 0", |x| x.hurry(2), s("bd sd ht lt"))"#),
        vec!["bd", "sd", "bd", "sd", "ht", "lt"]
    );
}

#[test]
fn callback_error_is_surfaced() {
    // Referencing an undefined function inside the callback raises.
    let err = eval(r#"seq(0).every(2, |x| x.nonexistent_method())"#);
    assert!(err.is_err());
}

// --- Transpilation / preprocessing parity -------------------------------------
