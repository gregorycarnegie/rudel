use super::common::*;

#[test]
fn every_with_a_callback() {
    // every(2, |x| x.add(10)): cycle 0 -> 10, cycle 1 -> 0
    let pat = eval(r#"seq(0).every(2, x => x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));
}

#[test]
fn superimpose_with_a_callback() {
    // superimpose(|x| x.add(7)) over a single value -> two haps
    let pat = eval(r#"seq(0).superimpose(x => x.add(7))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(7)]);
    // ...and it is variadic, like `layer`: upstream stacks the pattern with a
    // copy through *every* function. Applying only the first dropped a whole
    // voice from any tune that superimposes two.
    let pat = eval(r#"seq(0).superimpose(x => x.add(7), x => x.sub(5))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(0), Value::Int(7), Value::Int(-5)]
    );
    // No functions at all is just the pattern.
    let pat = eval(r#"seq(0).superimpose()"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
}

#[test]
fn jux_with_a_callback() {
    let pat = eval(r#"note("0 1").jux(x => x.rev())"#).expect("eval");
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
fn within_with_a_callback() {
    // apply +10 only to the first 40% of the cycle -> events 0 and 1
    let pat = eval(r#"seq(0, 1, 2, 3).within(0, 0.4, x => x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(11), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn chunk_with_a_callback() {
    // chunk(4, +10): first element bumped on cycle 0
    let pat = eval(r#"seq(0, 1, 2, 3).chunk(4, x => x.add(10))"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(10), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn callback_combinators_accept_patterned_args() {
    // The script is not run from the query path, so a patterned leading arg is
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
    let pat = eval(r#"n("0 1 2 3").chunk("<2 4>", x => x.add(n(10)))"#).expect("eval");
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
    let sby = eval(r#"n("0*4").sometimesBy("<0 1>", x => x.add(n(10)))"#).expect("eval");
    assert!(n_of(&sby, 0, 1).iter().all(|&v| v == 0)); // prob 0
    assert_eq!(n_of(&sby, 1, 2), vec![10, 10, 10, 10]); // prob 1

    // within with a patterned bound.
    let within = eval(r#"n("0 1 2 3").within("<0 0.5>", 0.5, x => x.add(n(10)))"#).expect("eval");
    assert_eq!(n_of(&within, 0, 1), vec![10, 11, 12, 3]);
    assert_eq!(n_of(&within, 1, 2), vec![0, 1, 12, 3]);

    // a scalar leading arg still uses the direct fast path.
    let scalar = eval(r#"n("0 1 2 3").chunk(4, x => x.add(n(10)))"#).expect("eval");
    assert_eq!(n_of(&scalar, 0, 1), vec![10, 1, 2, 3]);
}

#[test]
fn off_with_a_callback() {
    // off(0.25, +12) stacks a shifted, transposed copy: two onsets per cycle
    let pat = eval(r#"note(0).off(0.25, x => x.add(12))"#).expect("eval");
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
    let pat = eval(r#"seq(0).layer([x => x.add(0), x => x.add(7)])"#).expect("eval");
    let mut got = values(&pat, 0, 1);
    got.sort_by_key(|v| v.as_f64().unwrap() as i64);
    assert_eq!(got, vec![Value::Int(0), Value::Int(7)]);
}

#[test]
fn apply_always_never_via_script() {
    // apply/always run the callback; never leaves the pattern unchanged.
    let pat = eval(r#"seq(0).apply(x => x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).always(x => x.add(5))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(5)]);
    let pat = eval(r#"seq(0).never(x => x.add(5))"#).expect("eval");
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
    let pat = eval(r#"seq(0).every(2, x => x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0));

    // lastOf places the transform on the last cycle of each group.
    let pat = eval(r#"seq(0).lastOf(2, x => x.add(10))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(10));

    // standalone form (pattern last) honours the patterned count too.
    let pat = eval(r#"every("<1 2>", x => x.add(10), seq(0))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1)[0], Value::Int(10)); // n=1 -> applied
    assert_eq!(values(&pat, 1, 2)[0], Value::Int(0)); // n=2 -> 1 mod 2 != 0
}

#[test]
fn bool_literals_become_boolean_patterns() {
    // A bare `true`/`false` reifies to `pure(true/false)` (Strudel's
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
    // gets the index; a one-arg callback ignores it.
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
        ns(r#"n("0").echoWith(3, 0.25, (x, i) => x.add(n(i)))"#),
        vec![0, 1, 2, 1, 2]
    );
    // one-arg callback still works (index ignored).
    assert_eq!(
        ns(r#"n("0").echoWith(3, 0.25, x => x.add(n(10)))"#),
        vec![10, 10, 10, 10, 10]
    );
    // stutWith is an alias; standalone takes the pattern last.
    assert_eq!(
        ns(r#"stutWith(3, 0.25, (x, i) => x.add(n(i)), n("0"))"#),
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
        vals(r#""0 1".plyWith(3, x => x.add(10))"#),
        vec![0, 10, 20, 1, 11, 21]
    );
    // plyForEach(3, (p,n) => p+n*2): first copy untransformed, then index-scaled.
    assert_eq!(
        vals(r#""0 1".plyForEach(3, (p, n) => p.add(n * 2))"#),
        vec![0, 2, 4, 1, 3, 5]
    );
    // standalone form takes the pattern last.
    assert_eq!(
        vals(r#"plyWith(3, x => x.add(10), "0 1")"#),
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
        names(r#"s("bd sd ht lt").into("1 0", x => x.hurry(2))"#),
        vec!["bd", "sd", "bd", "sd", "ht", "lt"]
    );
    // chunkInto(4): cycle 0 hurries the first quarter (looped) -> bd, bd, ...
    assert_eq!(
        names(r#"s("bd sd ht lt").chunkInto(4, x => x.hurry(2))"#),
        vec!["bd", "bd", "sd", "ht", "lt"]
    );
    // Two windows in the same cycle are ribboned separately: each gets its own
    // slice of the pattern, so they cannot share one cached transform.
    assert_eq!(
        names(r#"s("bd sd ht lt").into("1 1", x => x.hurry(2))"#),
        vec!["bd", "sd", "bd", "sd", "ht", "lt", "ht", "lt"]
    );
    // standalone form takes the pattern last.
    assert_eq!(
        names(r#"into("1 0", x => x.hurry(2), s("bd sd ht lt"))"#),
        vec!["bd", "sd", "bd", "sd", "ht", "lt"]
    );
}

#[test]
fn callback_error_is_surfaced() {
    // Referencing an undefined function inside the callback raises.
    let err = eval(r#"seq(0).every(2, x => x.nonexistent_method())"#);
    assert!(err.is_err());
}

#[test]
fn an_array_of_callables_layers_like_varargs() {
    // `layer`/`tour` take varargs or one array, and both have to work alike.
    let both = |script: &str| {
        let mut v: Vec<f64> = values(&eval(script).expect("eval"), 0, 1)
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        v.sort_by(f64::total_cmp);
        v
    };
    let want = vec![0.0, 7.0];
    assert_eq!(
        both(r#"seq(0).layer([x => x.add(0), x => x.add(7)])"#),
        want
    );
    assert_eq!(both(r#"seq(0).layer(x => x.add(0), x => x.add(7))"#), want);
}

#[test]
fn ply_needs_a_positive_repeat_count() {
    // `ply(n)` repeats each event n times; zero or fewer has nothing to build
    // and must not try to speed a pattern up by zero.
    let count = |script: &str| values(&eval(script).expect("eval"), 0, 1).len();
    assert_eq!(count(r#"s("bd sd").ply(2)"#), 4);
    assert_eq!(count(r#"s("bd sd").ply(1)"#), 2);
    assert_eq!(count(r#"s("bd sd").ply(0)"#), 0);
    assert_eq!(count(r#"s("bd sd").ply(-1)"#), 0);
}

#[test]
fn a_pattern_of_functions_is_a_callback_too() {
    // `choose(f, g)` is a pattern whose values are functions; upstream's
    // `register` samples it per cycle like any other patterned argument, so a
    // combinator handed one applies whichever function the cycle picked.
    let pat = eval(r#"s("bd*8").sometimesBy(1, choose(x => x.speed(2), x => x.speed(3)))"#)
        .expect("eval");
    let speeds: Vec<f64> = pat
        .query_arc(Frac::zero(), Frac::int(4))
        .into_iter()
        .filter_map(|h| match h.value {
            Value::Map(m) => m.get("speed").and_then(|v| v.as_f64()),
            _ => None,
        })
        .collect();
    assert!(
        !speeds.is_empty(),
        "every event went through a picked function"
    );
    assert!(speeds.iter().all(|s| *s == 2.0 || *s == 3.0), "{speeds:?}");
    // Handed something that is not a function at all, the pattern plays on.
    let pat = eval(r#"s("bd sd").sometimes(n("0 1"))"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
}

#[test]
fn a_patterned_count_first_seen_mid_cycle_still_applies() {
    // The probe has to cover whole cycles, not just their first instant: the
    // `2` here only starts half way through.
    // (`chunk` goes through the shared probe; `every` has its own binding.)
    let pat = eval(r#"s("a b c d").chunk("4 2", x => x)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::new(1, 2), Frac::one()).len(), 2);
    let arp = eval(r#"note("[c,e] [d,f]").arpWith(p => p)"#).expect("eval");
    assert!(!arp.query_arc(Frac::new(1, 2), Frac::one()).is_empty());
}

#[test]
fn ply_with_multiplies_the_step_count() {
    // Six steps of `plyWith` then one of `c`: `c` takes the last seventh.
    let pat = eval(r#"stepcat(s("a b").plyWith(3, x => x), s("c"))"#).expect("eval");
    let c = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .find(|h| matches!(&h.value, Value::Map(m) if m.get("s") == Some(&Value::Str("c".into()))))
        .expect("a c");
    assert_eq!(c.whole.unwrap().begin, Frac::new(6, 7));
}

#[test]
fn pick_f_takes_its_lookup_first_or_second() {
    let a = eval(r#"s("a").pickF("0", [x => x.fast(2), x => x])"#).expect("eval");
    let b = eval(r#"s("a").pickF([x => x.fast(2), x => x], "0")"#).expect("eval");
    assert_eq!(values(&a, 0, 1).len(), 2);
    assert_eq!(values(&a, 0, 1), values(&b, 0, 1));
    // Two arrays: the first is the selector, as written.
    let c = eval(r#"s("a").pickF([0, 1], [x => x.fast(2), x => x.fast(3)])"#).expect("eval");
    assert!(!values(&c, 0, 1).is_empty());
}

#[test]
fn pick_with_two_arrays_reads_the_first_as_the_lookup() {
    let pat = eval(r#"pick([s("a"), s("b")], [1, 0])"#).expect("eval");
    let names: Vec<_> = values(&pat, 0, 1)
        .into_iter()
        .filter_map(|v| match v {
            Value::Map(m) => m.get("s").cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(names, [Value::Str("b".into()), Value::Str("a".into())]);
}

#[test]
fn fmap_maps_at_query_time_however_far_the_pattern_runs() {
    // Upstream maps each hap as it is queried. An eager 16-cycle probe used to
    // repeat its window, so cycle 20 of `<0 1 2 3 4>` came out as cycle 4.
    let plain = eval(r#"n("<0 1 2 3 4>")"#).expect("eval");
    let mapped = eval(r#"n("<0 1 2 3 4>").fmap(v => v)"#).expect("eval");
    for cycle in [0, 4, 16, 20, 21, 99] {
        assert_eq!(
            values(&mapped, cycle, cycle + 1),
            values(&plain, cycle, cycle + 1),
            "cycle {cycle}"
        );
    }
    let shifted = eval(r#"n("<0 1 2 3 4>").withValue(v => ({ n: v.n + 10 }))"#).expect("eval");
    assert_eq!(
        values(&shifted, 20, 21),
        values(&eval(r#"n(10)"#).unwrap(), 0, 1)
    );
}

#[test]
fn a_value_fmap_throws_on_is_kept_and_the_error_logged() {
    rudel_core::drain_log();
    let pat = eval(r#"n("0 1").fmap(v => { throw new Error('nope') })"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        values(&eval(r#"n("0 1")"#).unwrap(), 0, 1)
    );
    assert!(
        rudel_core::drain_log()
            .iter()
            .any(|l| l == "withValue: nope")
    );
}

/// The values `src` gives over cycle `cycle`.
fn at(src: &str, cycle: i64) -> Vec<Value> {
    let pat = eval(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    values(&pat, cycle, cycle + 1)
}

// Each of these once probed 16 cycles at evaluation and baked the result, so
// past cycle 16 it repeated its window, or went silent on anything it had not
// seen. Cycle 20 is where each would have been wrong.

#[test]
fn filter_values_and_filter_when_judge_every_cycle() {
    let zero = at(r#"n(0)"#, 0);
    let even = r#"n("<0 1 2 3 4>").filterValues(v => v.n == 0)"#;
    assert_eq!(at(even, 20), zero);
    assert!(at(even, 21).is_empty());
    // Nothing in the old window passed, so this was silent for good.
    let late = r#"n("<0 1 2 3 4>").filterWhen(t => t >= 20)"#;
    assert!(at(late, 19).is_empty());
    assert_eq!(at(late, 20), zero);
}

#[test]
fn arp_with_plays_a_chord_first_heard_after_cycle_sixteen() {
    let src = format!(
        r#"note("<{}[64,67]>").arpWith(c => c)"#,
        "[60,64] ".repeat(20)
    );
    assert_eq!(at(&src, 20), at(r#"note("64 67")"#, 0));
}

#[test]
fn a_patterned_callback_argument_meets_new_values_late() {
    // `4` first appears at cycle 20; the scalar form is the reference.
    let patterned = at(r#"n("0 1 2 3").chunk("<2!20 4>", x => x.add(n(10)))"#, 20);
    assert_ne!(
        patterned,
        at(r#"n("0 1 2 3")"#, 20),
        "a chunk is transformed"
    );
    assert_eq!(
        patterned,
        at(r#"n("0 1 2 3").chunk(4, x => x.add(n(10)))"#, 20)
    );
}

#[test]
fn ply_with_copies_a_value_first_heard_after_cycle_sixteen() {
    assert_eq!(
        at(r#"n("<0!20 5>").plyWith(2, x => x.add(n(1)))"#, 20),
        at(r#"n("5 6")"#, 0)
    );
}

#[test]
fn chunk_into_transforms_every_piece_however_late() {
    let src = r#"n("0 1 2 3").chunkInto(4, x => x.add(n(10)))"#;
    assert_ne!(
        at(src, 0),
        at(r#"n("0 1 2 3")"#, 0),
        "the first piece is transformed"
    );
    // Four pieces, so cycle 20 is cycle 0 again.
    assert_eq!(at(src, 20), at(src, 0));
}

#[test]
fn log_values_logs_what_each_cycle_plays() {
    let logged = |cycle| {
        at(r#"n("<0 1 2 3 4>").logValues(v => v.n)"#, cycle)
            .into_iter()
            .filter_map(|v| match v {
                Value::Map(m) => m.get("_log").cloned(),
                _ => None,
            })
            .collect::<Vec<Value>>()
    };
    assert_eq!(logged(20), vec![Value::Str("0".into())]);
}

#[test]
fn a_registered_method_samples_a_patterned_argument_every_cycle() {
    let src = r#"register('plus', (x, pat) => pat.add(n(x))); n("0").plus("<0!20 7>")"#;
    assert_eq!(at(src, 20), at(r#"n(7)"#, 0));
}

#[test]
fn echo_takes_patterns_for_every_argument() {
    // `register` patternifies all three; "forgotten flower" passes
    // `reify(n)` and `pure(cycles).div(n)` through its own `nest`.
    let plain = values(&eval("s(\"bd\").echo(3, 0.25, 1)").unwrap(), 0, 1).len();
    for src in [
        "s(\"bd\").echo(reify(3), 0.25, 1)",
        "s(\"bd\").echo(3, pure(1).div(4), 1)",
        "s(\"bd\").echo(reify(3), pure(0.25), pure(1))",
        "echo(reify(3), pure(0.25), 1, s(\"bd\"))",
        "s(\"bd\").stut(reify(3), 1, pure(0.25))",
    ] {
        assert_eq!(values(&eval(src).unwrap(), 0, 1).len(), plain, "{src}");
    }
    assert_eq!(
        plain, 5,
        "three copies, two carried over from the cycle before"
    );
}
