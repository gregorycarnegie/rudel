use super::common::*;

#[test]
fn pick_supports_lists_methods_and_string_pattern_chains() {
    let pat = eval(r#"pick(["a", "b"], 1)"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Str("b".to_string())]);

    let pat = eval(r#""1".pick(["a", "b"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Str("b".to_string())]);

    let pat = eval(
        r#"
xs = ["0", "1"]
pick(xs, "<0 1>".slow(2))
"#,
    )
    .expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(0)]);
}

#[test]
fn pick_variants_are_bound_as_methods_and_factories() {
    // pickmod wraps instead of clamping
    let pat = eval(r#""5".pickmod(["a", "b"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::Str("b".to_string())]);

    // inhabit (squeeze join) fits one cycle of the picked pattern per event;
    // pickSqueeze is its alias
    let pat = eval(r#""0 0".inhabit(["x y"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);
    let pat = eval(r#""0 0".pickSqueeze(["x y"])"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 4);

    // map lookup with the outer join keeps the selector's structure
    let pat = eval(r#""a b".pickOut({a: "0 1", b: "2"})"#).expect("eval");
    assert_eq!(values(&pat, 0, 1).len(), 2);

    // retriggering joins and standalone forms are bound
    for code in [
        r#""<0 1>".pickRestart(["x", "y z"])"#,
        r#""<0 1>".pickReset(["x", "y z"])"#,
        r#"pickmodSqueeze(["x y"], "0 5")"#,
        r#"squeeze("<0@2 [1!2] 2>", ["g a", "f g f g", "g a c d"])"#,
    ] {
        let pat = eval(code).expect("eval");
        assert!(!values(&pat, 0, 2).is_empty(), "no haps for {code}");
    }
}

#[test]
fn pick_f_picks_functions_from_lists_and_maps() {
    // index 0 -> rev, index 1 -> fast(2): cycle 0 reverses, cycle 1 doubles
    let pat = eval(r#""a b".pickF("<0 1>", [|x| x.rev(), |x| x.fast(2)])"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Str("b".into()), Value::Str("a".into())]
    );
    assert_eq!(values(&pat, 1, 2).len(), 4);

    // name lookup + pickmodF index wrapping
    let pat = eval(r#""a b".pickmodF("<r 3>", {r: |x| x.rev()})"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Str("b".into()), Value::Str("a".into())]
    );
    let pat = eval(r#""a b".pickmodF("3", [|x| x.rev()])"#).expect("eval");
    assert_eq!(
        values(&pat, 0, 1),
        vec![Value::Str("b".into()), Value::Str("a".into())]
    );

    // callback errors surface instead of being swallowed
    assert!(eval(r#""a".pickF("0", [|x| nope()])"#).is_err());
}
