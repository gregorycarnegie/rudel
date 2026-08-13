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
