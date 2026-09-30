use super::common::*;

// The user-reachable scalar helpers from core/util.mjs. They return numbers,
// so each is wrapped in `pure(...)` to give `eval` a pattern to return.

#[test]
fn midi_to_freq_matches_strudel() {
    // midiToFreq(69) == 440; midiToFreq(57) == 220 (an octave down).
    let pat = eval("pure(midiToFreq(69))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(440.0)]);
    let pat = eval("pure(midiToFreq(57))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(220.0)]);
}

#[test]
fn freq_to_midi_is_the_inverse() {
    // freqToMidi(440) == 69.
    let pat = eval("pure(freqToMidi(440))").expect("eval");
    let got = values(&pat, 0, 1)[0].as_f64().expect("a number");
    assert!((got - 69.0).abs() < 1e-9, "got {got}");
}

#[test]
fn note_to_midi_parses_note_names() {
    // a4 -> 69, c4 -> 60 (default octave 3 is only used when none is given).
    let pat = eval(r#"pure(noteToMidi("a4"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(69.0)]);
    let pat = eval(r#"pure(noteToMidi("c4"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(60.0)]);
    // Default octave 3: a bare "c" is C3 (48).
    let pat = eval(r#"pure(noteToMidi("c"))"#).expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(48.0)]);
}

#[test]
fn note_to_midi_rejects_non_notes() {
    // Strudel throws on a non-note, and so does the binding.
    assert!(eval(r#"pure(noteToMidi("xyz"))"#).is_err());
}

#[test]
fn clamp_limits_to_the_range() {
    let pat = eval("pure(clamp(5, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(1.0)]);
    let pat = eval("pure(clamp(-3, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(0.0)]);
    let pat = eval("pure(clamp(0.5, 0, 1))").expect("eval");
    assert_eq!(values(&pat, 0, 1), vec![Value::F64(0.5)]);
}

#[test]
fn converters_compose_in_patterns() {
    // A realistic use: set a note from a frequency round-tripped through midi.
    let pat = eval(r#"note(freqToMidi(440))"#).expect("eval");
    match &values(&pat, 0, 1)[0] {
        Value::Map(m) => match m.get("note").and_then(Value::as_f64) {
            Some(n) => assert!((n - 69.0).abs() < 1e-9, "got {n}"),
            other => panic!("expected note number, got {other:?}"),
        },
        other => panic!("expected a control map, got {other:?}"),
    }
}

// JavaScript's own builtins. Strudel snippets call these directly, so they are
// part of the language surface even though no rudel script needs them.
// Single-quoted literals stay plain strings; double-quoted ones are
// mini-notation patterns by then, and have no string methods.

#[test]
fn javascript_string_methods_match_javascript() {
    // A double-quoted literal is a *pattern* (that is how `"bd sd".fast(2)`
    // works), so the string methods are reached through a single-quoted one,
    // which is how Strudel snippets use them.
    let one = |script: &str| {
        values(
            &eval(&format!(
                "let s = 'hello world'
{script}"
            ))
            .expect("eval"),
            0,
            1,
        )
    };
    assert_eq!(one("pure(s.substring(0, 5))"), vec!["hello".into()]);
    // Out-of-order and out-of-range arguments clamp and swap rather than panic.
    assert_eq!(one("pure(s.substring(5, 0))"), vec!["hello".into()]);
    assert_eq!(one("pure(s.substring(6))"), vec!["world".into()]);
    assert_eq!(one("pure(s.substring(6, 99))"), vec!["world".into()]);
    assert_eq!(one("pure(s.length)"), vec![Value::Int(11)]);
    assert_eq!(one("pure(s.indexOf('world'))"), vec![Value::Int(6)]);
    // Not found is -1, not 0 and not an error.
    assert_eq!(one("pure(s.indexOf('zzz'))"), vec![Value::Int(-1)]);
    assert_eq!(one("pure(s.startsWith('hello'))"), vec![Value::Bool(true)]);
    assert_eq!(one("pure(s.endsWith('hello'))"), vec![Value::Bool(false)]);
}

#[test]
fn javascript_conversions_match_javascript() {
    let one = |script: &str| values(&eval(script).expect("eval"), 0, 1);
    assert_eq!(one("pure(Number('3'))"), vec![Value::Int(3)]);
    assert_eq!(one("pure(Number(3))"), vec![Value::Int(3)]);
    // A string that will not parse is NaN, as JavaScript says.
    assert!(matches!(one("pure(Number('wat'))")[..], [Value::F64(n)] if n.is_nan()));
    // `String` of either a string or a number round-trips through `Number`.
    assert_eq!(one("pure(Number(String(4)))"), vec![Value::Int(4)]);
    assert_eq!(one("pure(Number(String('4')))"), vec![Value::Int(4)]);
    // `filter(Boolean)` is the idiom for dropping what a `split` left empty.
    assert_eq!(
        one("pure(['a', '', 'b'].filter(Boolean).length)"),
        vec![Value::Int(2)]
    );
    assert_eq!(one("pure(Boolean(0))"), vec![Value::Bool(false)]);
    assert_eq!(one("pure(Boolean('x'))"), vec![Value::Bool(true)]);
    assert_eq!(one("pure(typeof 'a')"), vec!["string".into()]);
    assert_eq!(one("pure(typeof 1)"), vec!["number".into()]);
}

#[test]
fn object_from_entries_builds_an_object() {
    let one = |script: &str| values(&eval(script).expect("eval"), 0, 1);
    assert_eq!(
        one("pure(Object.fromEntries([['a', 5], ['b', 6]]).b)"),
        vec![Value::Int(6)]
    );
    // A non-string key is stringified, as JS object keys are.
    assert_eq!(
        one("pure('1' in Object.fromEntries([[1, 5]]))"),
        vec![Value::Bool(true)]
    );
}

#[test]
fn map_over_a_long_list_takes_a_one_parameter_callback() {
    // `map` passes JS's `(value, index)`, and a callback may ignore the index.
    // A 94-entry table once failed where a 59-entry one worked, back when the
    // index was a retried arity error; long enough here to be past that.
    let list = (0..100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let one = |script: &str| values(&eval(script).expect("eval"), 0, 1);
    assert_eq!(
        one(&format!("pure([{list}].map((v) => v + 1)[99])")),
        vec![Value::Int(100)]
    );
    // The two-parameter form still gets its index.
    assert_eq!(
        one(&format!("pure([{list}].map((v, i) => i)[99])")),
        vec![Value::Int(99)]
    );
}

#[test]
fn reduce_folds_left_in_javascripts_argument_order() {
    let one = |script: &str| values(&eval(script).expect("eval"), 0, 1);
    // Seeded, and the seed is the accumulator's starting type.
    assert_eq!(
        one("pure([1, 2, 3].reduce((a, v) => a + v, 10))"),
        vec![Value::Int(16)]
    );
    // Unseeded: the first entry is the seed, so nothing is folded onto itself.
    assert_eq!(
        one("pure([1, 2, 3].reduce((a, v) => a + v))"),
        vec![Value::Int(6)]
    );
    // The index is JS's third argument, and only reaches a callback that asks.
    assert_eq!(
        one("pure([5, 5, 5].reduce((a, v, i) => a + i, 0))"),
        vec![Value::Int(3)]
    );
    // A block body with a further argument after it.
    assert_eq!(
        one("pure([1, 2].reduce((a, v) => { a.push(v); return a }, []).length)"),
        vec![Value::Int(2)]
    );
}

#[test]
fn json_parse_reads_an_embedded_table_back_as_maps_and_lists() {
    let one = |script: &str| values(&eval(script).expect("eval"), 0, 1);
    // The shape songs use: a table keyed by character, read out by key.
    assert_eq!(
        one(r#"pure(JSON.parse('{"a": [1, 2], "b": "bd"}').b)"#),
        vec!["bd".into()]
    );
    // Nested lists index as lists, and JSON numbers arrive as numbers.
    assert_eq!(
        one(r#"pure(JSON.parse('{"a": [1, 2]}').a[1] + 1)"#),
        vec![Value::Int(3)]
    );
    // Malformed JSON is a script bug, and says so rather than passing silently.
    assert!(eval("pure(JSON.parse('{oops'))").is_err());
    // A table keyed by `"` is written `\\"` in the JS source: the literal
    // collapses that to the `\"` the JSON parser then needs.
    assert_eq!(
        one(r#"pure(JSON.parse('{"\\"": "quote"}')['"'])"#),
        vec!["quote".into()]
    );
}

#[test]
fn mini_parses_its_argument_even_when_the_string_was_built_at_runtime() {
    // A bare string reaching an ordinary pattern argument is deliberately not
    // mini-notation, matching upstream. `mini(...)` *is* the parser, though, and
    // is the only way a string a tune assembled at runtime can be played --
    // through a single-quoted literal the transpiler never wraps, or one built
    // a piece at a time in a loop.
    let haps = |script: &str| {
        eval(script)
            .expect("eval")
            .query_arc(Frac::zero(), Frac::one())
            .len()
    };
    assert_eq!(haps("note(mini('48 49 50'))"), 3);
    assert_eq!(haps("let s = '48 49'\nnote(mini(s))"), 2);
    // Variadic, and the arguments share the cycle (upstream sequences them).
    assert_eq!(haps("note(mini('48 49', '50 51'))"), 4);
    // A non-string argument still passes through as a pattern.
    assert_eq!(haps("note(mini(60))"), 1);
}

#[test]
fn install_mini_makes_rust_side_strings_parse_as_mini_notation() {
    // The host calls this once at startup so that a `&str` handed to a core
    // combinator is mini-notation rather than a one-hap literal. The hook is
    // process-global; nothing else in this crate reads a `&str` as a pattern.
    crate::install_mini();
    assert_eq!(values(&rudel_core::parse_string("bd sd"), 0, 1).len(), 2);
}
