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
fn callback_error_is_surfaced() {
    // Referencing an undefined function inside the callback raises.
    let err = eval(r#"seq(0).every(2, |x| x.nonexistent_method())"#);
    assert!(err.is_err());
}

// --- Transpilation / preprocessing parity -------------------------------------
