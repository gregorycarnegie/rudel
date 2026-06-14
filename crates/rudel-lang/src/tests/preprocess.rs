use super::common::*;

// --- Transpilation / preprocessing parity -------------------------------------

#[test]
fn preprocess_rewrites_arrow_functions_to_koto_lambdas() {
    // bare single identifier parameter
    assert_eq!(preprocess_strudel("f(x => x.fast(2))"), "f(|x| x.fast(2))");
    // parenthesised single parameter
    assert_eq!(
        preprocess_strudel("f((x) => x.fast(2))"),
        "f(|x| x.fast(2))"
    );
    // multiple parameters
    assert_eq!(preprocess_strudel("f((a, b) => a)"), "f(|a, b| a)");
    // zero parameters -> Koto's `||`
    assert_eq!(preprocess_strudel("f(() => 1)"), "f(|| 1)");
    // an `=>` inside a string literal is left intact; the string is wrapped in
    // `m(literal, offset)` for source-location tracking (offset 6 = the byte
    // position of the content just after `note("`).
    assert_eq!(
        preprocess_strudel(r#"note("a => b")"#),
        r#"note(m("a => b", 6))"#
    );
    // a comparison operator is never mistaken for an arrow
    assert_eq!(preprocess_strudel("f(x >= 2)"), "f(x >= 2)");
}

#[test]
fn empty_or_commented_out_script_falls_back_to_silence() {
    assert_eq!(preprocess_strudel(""), "silence()");
    assert_eq!(preprocess_strudel("   \n  \n"), "silence()");
    assert_eq!(preprocess_strudel("// just a comment\n"), "silence()");
    // and it evaluates to an actually-empty pattern
    let pat = eval("// nothing here\n").expect("eval");
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}

#[test]
fn arrow_and_pipe_callbacks_are_equivalent() {
    // Differential check: arrow-function and Koto-lambda spellings of the same
    // callback must produce identical haps across the combinator surface.
    let pairs = [
        (
            r#"seq(0).every(2, x => x.add(10))"#,
            r#"seq(0).every(2, |x| x.add(10))"#,
        ),
        (
            r#"seq(0).superimpose((x) => x.add(7))"#,
            r#"seq(0).superimpose(|x| x.add(7))"#,
        ),
        (
            r#"seq(0, 1, 2, 3).within(0, 0.4, x => x.add(10))"#,
            r#"seq(0, 1, 2, 3).within(0, 0.4, |x| x.add(10))"#,
        ),
        (
            r#"seq(0).layer([x => x.add(0), x => x.add(7)])"#,
            r#"seq(0).layer([|x| x.add(0), |x| x.add(7)])"#,
        ),
    ];
    for (arrow, pipe) in pairs {
        let a = eval(arrow).unwrap_or_else(|e| panic!("arrow eval {arrow}: {e}"));
        let b = eval(pipe).unwrap_or_else(|e| panic!("pipe eval {pipe}: {e}"));
        assert_eq!(values(&a, 0, 2), values(&b, 0, 2), "mismatch for {arrow}");
    }
}
