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
    // for. It proves the state reaches Koto and the haps come back.
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
fn a_query_function_may_return_a_tuple_of_haps() {
    // Koto's own iterator adaptors hand back tuples, so a combinator written
    // as `[...].to_tuple()` has to work like the list form.
    let pat = eval(
        r#"
new Pattern(state => [new Hap(state.span, state.span, 7)].to_tuple())
"#,
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(7)]);
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
