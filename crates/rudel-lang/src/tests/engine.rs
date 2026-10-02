//! The engine vocabulary a script can build a combinator out of: `Pattern`,
//! `Hap`, `TimeSpan`, `Fraction`, and the `Pattern.prototype` patch that binds
//! one as a method.
//!
//! Unlike every other callback in the bindings these run *during the query*, so
//! what they need pinning is that the state going in and the haps coming back
//! survive the round trip — a query function that silently returns nothing
//! still evaluates, and the pattern just goes quiet.

use super::common::*;

#[test]
fn a_pattern_can_be_built_from_a_query_function() {
    // The simplest combinator there is: one hap covering whatever was asked
    // for. It proves the state reaches the script and the haps come back.
    let pat = eval(
        r#"
new Pattern(state => [new Hap(state.span, state.span, 7)])
"#,
    )
    .expect("eval");
    let haps = shape(&pat, 1);
    assert_eq!(haps.len(), 1, "{haps:?}");
    assert_eq!(haps[0].0, Frac::zero());
    assert_eq!(haps[0].1, Frac::one());
    assert_eq!(haps[0].2, Value::Int(7));
}

#[test]
fn a_query_function_sees_the_span_it_was_asked_about() {
    // `splitQueries` hands the function one cycle at a time, which is what a
    // combinator reasoning about "this cycle" relies on.
    let pat = eval(
        r#"
new Pattern(state => [new Hap(state.span, state.span, state.span.begin.toNumber())]).splitQueries()
"#,
    )
    .expect("eval");
    let values = values(&pat, 0, 3);
    assert_eq!(
        values,
        vec![Value::F64(0.0), Value::F64(1.0), Value::F64(2.0)],
        "one hap per cycle, each told which cycle it is"
    );
}

#[test]
fn fractions_stay_exact() {
    // The whole reason fractions are an object rather than a float: a third of
    // a cycle has to come back a third, or spans stop lining up.
    let pat = eval(
        r#"
new Pattern(state => [new Hap(state.span, state.span, Fraction(1).div(3).mul(3).toNumber())])
"#,
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(1.0)]);
}

#[test]
fn query_reads_the_span_out_of_the_state_it_is_given() {
    // Not the enclosing query's span: the state map decides, which is what
    // lets a combinator look at a different cycle than the one being asked
    // about.
    let pat = eval(
        r#"
let inner = "<10 20 30>"
new Pattern(state => inner.query({ span: Fraction(1).wholeCycle() }))
"#,
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(20)]);
}

#[test]
fn fraction_arithmetic_goes_the_right_way() {
    // div and mul are covered by `fractions_stay_exact`; add and sub agree
    // with it only on operands the other direction would also fit.
    let n = |script: &str| {
        values(&eval(&format!("pure({script})")).expect("eval"), 0, 1)[0]
            .as_f64()
            .expect("a number")
    };
    assert_eq!(n("Fraction(1).add(2).toNumber()"), 3.0);
    assert_eq!(n("Fraction(3).sub(2).toNumber()"), 1.0);
    assert_eq!(n("Fraction(1).add(Fraction(2)).toNumber()"), 3.0);
    // A fraction added to nothing is itself, and dividing by zero is zero
    // rather than an error.
    assert_eq!(n("Fraction(2).add(0).toNumber()"), 2.0);
    assert_eq!(n("Fraction(2).div(0).toNumber()"), 0.0);
}

#[test]
fn a_prototype_patch_binds_a_method_that_can_read_its_own_haps() {
    // `enumerate` is the combinator scripts in the wild write this way: number
    // each hap of the cycle, and say how many there were. It cannot be done by
    // probing ahead of time, which is why the query path is open at all.
    let pat = eval(
        r#"
Pattern.prototype.enumerate = function () {
  const pat = this.sortHapsByPart()
  return new Pattern(state => {
    const haps = pat.query(state.withSpan(span => span.begin.wholeCycle()))
    const chunks = haps.length
    return haps.map((hap, i) => new Hap(hap.whole, hap.part.intersection(state.span), [hap.value, i, chunks])
                  ).filter(hap => hap.part != undefined)
  }).splitQueries()
}
"a b c".enumerate()
"#,
    )
    .expect("eval");

    let got = values(&pat, 0, 1);
    let expected: Vec<Value> = ["a", "b", "c"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            Value::List(vec![
                Value::Str((*name).into()),
                Value::Int(i as i64),
                Value::Int(3),
            ])
        })
        .collect();
    assert_eq!(got, expected, "each hap carries [value, index, count]");
}

#[test]
fn the_span_forms_match_their_two_argument_versions() {
    // `compressSpan`/`focusSpan`/`zoomArc` only differ from `compress`/`focus`/
    // `zoom` in taking the span as one object, which a script can only build
    // because `TimeSpan` is exposed.
    for (name, two_arg) in [
        ("compressSpan", "compress(0.25, 0.75)"),
        ("focusSpan", "focus(0.25, 0.75)"),
        ("zoomArc", "zoom(0.25, 0.75)"),
    ] {
        let want = shape(&eval(&format!(r#""a b c d".{two_arg}"#)).expect("eval"), 2);
        assert!(!want.is_empty(), "{two_arg} produced nothing");
        // The method and the standalone form, which takes the pattern last.
        for form in [
            format!(r#""a b c d".{name}(TimeSpan(0.25, 0.75))"#),
            format!(r#"{name}(TimeSpan(0.25, 0.75), "a b c d")"#),
        ] {
            let got = shape(&eval(&form).expect("eval"), 2);
            assert_eq!(got, want, "{form} vs {two_arg}");
        }
    }
}

#[test]
fn a_prototype_method_gets_its_argument_whole() {
    // A combinator reads its argument pattern's haps, so — unlike `register` —
    // the argument must not be sampled per cycle on the way in.
    let pat = eval(
        r#"
Pattern.prototype.tally = function (other) {
  const pat = this
  return new Pattern(state => {
    const haps = other.query(state)
    return [new Hap(state.span, state.span, haps.length)]
  }).splitQueries()
}
"x".tally("a b c d")
"#,
    )
    .expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Int(4)],
        "the argument arrived as a whole pattern, not one sampled value"
    );
}

#[test]
fn an_engine_panic_is_the_script_s_error_and_the_next_one_runs() {
    // boa's `sort` panics on a comparator that is not a total order, which is
    // exactly what the shuffle idiom is. That has to come back as an error
    // rather than taking the host down, and leave the engine usable.
    let script = "pure([...Array(100).keys()].sort(() => Math.random() - 0.5)[0])";
    let mut failed = false;
    // Random, so a lucky run may sort without tripping the check.
    for _ in 0..20 {
        if let Err(e) = eval(script) {
            assert!(e.contains("sort"), "{e}");
            failed = true;
            break;
        }
    }
    assert!(failed, "the inconsistent comparator never tripped");
    assert_eq!(
        values(&eval("pure(1)").expect("eval"), 0, 1),
        vec![Value::Int(1)]
    );
}

#[test]
fn a_script_can_read_a_pattern_and_build_a_signal() {
    // `queryArc`/`firstCycle` hand back haps, as upstream's do.
    let pat = eval("pure(n(\"0 1 2\").firstCycle().length + n(\"0 1\").queryArc(0, 2).length)")
        .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(7)]);
    // `signal(t => …)` is sampled at each query's start.
    let pat = eval("signal(t => t * 2).segment(2)").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0), Value::Int(1)]);
    // `id` is the identity.
    assert_eq!(
        values(&eval("id(pure(3))").expect("eval"), 0, 1),
        vec![Value::Int(3)]
    );
}

#[test]
fn register_takes_an_array_of_names() {
    let pat = eval(
        "const {twice, twice2} = register(['twice', 'twice2'], (pat) => pat.fast(2))\n\
         s(\"bd\").twice2()",
    )
    .expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
    // `filter` handed something that is not a predicate plays on unfiltered.
    let pat = eval(r#"s("bd sd").filter(500)"#).expect("eval");
    assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
}

#[test]
fn a_script_nested_past_what_the_parser_survives_is_refused() {
    // boa's parser overflows the engine thread's stack a few hundred levels
    // in, and an overflow aborts the process. The limit sits well inside what
    // it survives, even in a debug build.
    let nested = |n: usize| format!("pure({}1{})", "(".repeat(n), ")".repeat(n));
    // `pure(` is one level of its own.
    assert_eq!(
        values(&eval(&nested(127)).expect("at the limit"), 0, 1),
        vec![Value::Int(1)]
    );
    let Err(err) = eval(&nested(5000)) else {
        panic!("far past the limit, it should be refused");
    };
    assert!(err.contains("nested"), "{err}");
    // Brackets inside a string are text, not nesting.
    let quoted = format!("pure('{}')", "(".repeat(5000));
    assert!(eval(&quoted).is_ok());
}

#[test]
fn seq_p_loop_lays_sections_out_by_start_and_stop_as_strudel_does() {
    // Each `(begin end s)` below is what real Strudel's `seqPLoop` gives over
    // three cycles, queried from `@strudel/core` directly.
    let haps = |src: &str| -> Vec<String> {
        let mut out: Vec<String> = eval(src)
            .expect(src)
            .query_arc(Frac::zero(), Frac::int(3))
            .iter()
            .map(|h| {
                let whole = h.whole.unwrap();
                let s = match &h.value {
                    Value::Map(m) => m.get("s").and_then(|v| v.as_str()).unwrap_or(""),
                    _ => "",
                };
                // Strudel prints a whole number without its `/1`.
                let frac = |f: Frac| f.to_string().trim_end_matches("/1").to_string();
                format!("{} {} {s}", frac(whole.begin), frac(whole.end))
            })
            .collect();
        out.sort();
        out
    };
    let sorted = |lines: &[&str]| {
        let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    };
    // Overlapping sections: `cp` starts while `bd` is still playing.
    assert_eq!(
        haps(r#"seqPLoop([0, 2, "bd(3,8)"], [1, 3, "cp(3,8)"]).sound()"#),
        sorted(&[
            "0 1/8 bd",
            "3/8 1/2 bd",
            "3/4 7/8 bd",
            "1 9/8 bd",
            "11/8 3/2 bd",
            "7/4 15/8 bd",
            "1 9/8 cp",
            "11/8 3/2 cp",
            "7/4 15/8 cp",
            "2 17/8 cp",
            "19/8 5/2 cp",
            "11/4 23/8 cp",
        ])
    );
    // A two-element part starts where the one before it stopped.
    assert_eq!(
        haps(r#"seqPLoop([1, "a b"], [2, "c"]).sound()"#),
        sorted(&["0 1/2 a", "1/2 1 b", "2 5/2 a", "5/2 3 b", "1 2 c"])
    );
}

#[test]
fn a_one_argument_method_called_with_none_is_silence() {
    // Upstream's `register` makes the missing argument `sequence()`, which is
    // silence; a method with more arguments missing still fails.
    for method in ["rarely", "sometimes", "jux"] {
        let pat = eval(&format!(r#"s("bd*4").{method}()"#)).expect(method);
        assert!(
            pat.query_arc(Frac::zero(), Frac::one()).is_empty(),
            "{method}"
        );
    }
    assert!(eval(r#"s("bd*4").every(2)"#).is_err());
}

#[test]
fn a_script_can_query_its_own_pattern_while_it_is_still_running() {
    // The query function is the script's own, asked for back mid-evaluation —
    // a different path from the scheduler's, which waits for the engine.
    let pat = eval(
        "const p = new Pattern(state => [new Hap(state.span, state.span, 7)])\n\
         pure(p.firstCycle().length)",
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), [Value::Int(1)]);
}

#[test]
fn a_thrown_error_reads_as_its_message() {
    let err = eval("throw new Error('boom')").err().expect("throws");
    assert_eq!(err, "boom");
}

#[test]
fn a_script_that_returns_a_non_pattern_says_what_it_got() {
    let err = eval("5").err().expect("not a pattern");
    assert!(err.ends_with("(got 5)"), "{err}");
}

#[test]
fn a_script_ending_on_a_statement_plays_silence() {
    let pat = eval("let cpm = 30;").expect("a statement is not an error");
    assert!(values(&pat, 0, 1).is_empty());
}

#[test]
fn calling_a_non_function_names_what_it_was() {
    let err = eval("K(Kabel.sine(1).apply(3))")
        .err()
        .expect("3 is not a function");
    assert!(err.contains("got a number"), "{err}");
}

#[test]
fn a_self_referential_value_is_cut_off_rather_than_followed_forever() {
    // The bridge descends a fixed depth, then gives up; a cycle must not
    // overflow the stack.
    let pat = eval("const a = [1]; a.push(a); pure(a)").expect("eval");
    // An object refers back to itself just as well.
    eval("const o = {x: 1}; o.self = o; pure(1).set(o)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 1);
}

#[test]
fn a_built_in_method_is_never_replaced_by_register() {
    // A polyfill registered over a built-in is ignored; the built-in wins.
    let pat = eval("register('fast', (n, pat) => pat.slow(n))\ns(\"bd\").fast(2)").expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);
}

#[test]
fn the_next_evaluation_s_engine_is_built_ahead_of_time() {
    // Built on the engine thread after the evaluation returns, so the next
    // one does not wait for it. Holding the lock keeps another evaluation
    // from taking the spare between the two steps.
    eval("s(\"bd\")").expect("eval");
    let _guard = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let spare = crate::js::on_js_thread(|| {
        let spare = crate::SPARE.take();
        let ready = spare.is_some();
        crate::SPARE.set(spare);
        ready
    });
    assert!(spare, "no engine was prepared for the next evaluation");
}

#[test]
fn preprocessed_is_the_source_the_engine_runs() {
    let src = r#"s("bd sd")"#;
    assert_eq!(crate::preprocessed(src), preprocess_strudel(src));
    assert!(crate::preprocessed(src).contains("bd sd"));
}
