//! Properties of the front end that no list of examples pins down: what holds
//! for *any* script, including the half-typed ones the editor evaluates
//! between keystrokes.
use super::common::*;
use proptest::prelude::*;

/// Fragments of the things scripts are made of — labels, calls, chains,
/// widgets, sliders, strings of every quote, comments, mondo — so the
/// generated source is shaped like a script without having to parse.
fn fragment() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "$: ",
        "a: ",
        "_m: ",
        "\n",
        "\n  ",
        " ",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        ",",
        ";",
        ".",
        "s(",
        "note(",
        "\"bd sd\"",
        "\"<c e>\"",
        "'plain'",
        "`c e`",
        "`",
        "\"",
        "'",
        ".fast(2)",
        ".every(2, x => x.rev())",
        ".pianoroll()",
        "._spiral({ size: 2 })",
        "slider(0.5, 0, 1)",
        "// note\n",
        "/* c */",
        "mondo`s bd`",
        "await ",
        "K(sine(1))",
        "x => ",
        "=",
        "+",
        "?",
        ":",
        "é",
        "🌸",
        "\u{200b}",
        "\u{301}",
        "\\",
        "\\n",
        "\\\"",
        "\\'",
        "\\`",
        "\\u{1F338}",
        "\\u00e9",
        "\\x41",
        "${",
        "${x}",
        "\"\\\"bd\\\" sd\"",
        "`c ${'e'} g`",
    ])
}

/// Deep but well-formed: `depth` levels of one kind of nesting around a
/// pattern, the shapes a generated or pasted script could pile up.
fn deeply_nested(depth: usize) -> Vec<String> {
    let wrap = |open: &str, inner: &str, close: &str| {
        format!("{}{inner}{}", open.repeat(depth), close.repeat(depth))
    };
    vec![
        wrap("(", "s('bd')", ")"),
        wrap("[", "s('bd')", "][0]"),
        wrap("stack(", "s('bd')", ")"),
        wrap("{a:", "s('bd')", "}.a"),
        wrap("(x => ", "s('bd')", ")()"),
        wrap("`${", "1", "}`"),
        wrap("/*", "*/", ""),
        format!("s(\"{}\")", wrap("[", "bd", "]")),
        format!("s(`{}`)", wrap("<", "bd", ">")),
        format!("s('bd'){}", ".fast(1)".repeat(depth)),
        format!(
            "s('bd'){}",
            ".every(2, x => x".repeat(depth) + &")".repeat(depth)
        ),
        format!("{}s('bd')", "-".repeat(depth)),
    ]
}

/// A loop can build a pattern far deeper than any bracket limit allows, and
/// the scheduler and the UI then query and free it on their own threads.
#[test]
fn a_pattern_built_by_a_long_loop_plays_and_is_freed() {
    let pat = eval("let p = s('bd'); for (let i = 0; i < 20000; i++) p = p.fast(1); p")
        .expect("evaluates");
    std::thread::spawn(move || {
        assert_eq!(values(&pat, 0, 1).len(), 1);
        drop(pat);
    })
    .join()
    .expect("queried and freed on a default-sized thread");
}

/// However deep a script nests, the preprocessor and evaluation return — an
/// error is fine — rather than overflowing a stack and aborting the app.
#[test]
fn deep_nesting_is_an_error_or_a_pattern_never_a_crash() {
    for src in deeply_nested(5_000) {
        let _ = preprocess_strudel_with_meta(&src);
        if let Err(e) = eval(&src) {
            assert!(
                !e.starts_with("the JavaScript engine failed"),
                "{}…: {e}",
                &src[..40]
            );
        }
    }
}

fn script() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..24).prop_map(|parts| parts.concat())
}

/// A script that parses: a `stack` of calls on mini-notation literals, with
/// the widgets and sliders that shift source positions in between.
fn valid_script() -> impl Strategy<Value = String> {
    let call = prop::sample::select(vec![
        r#"s("bd sd")"#,
        r#"note("c e g").fast(2)"#,
        r#"n("0 1").lpf(slider(400, 100, 900))"#,
        r#"s("hh*4")._pianoroll()"#,
        r#"note(`<c e>`).spiral()"#,
    ]);
    prop::collection::vec(call, 1..5)
        .prop_map(|calls| format!("stack(\n  {}\n)", calls.join(",\n  ")))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The preprocessor sees every keystroke's worth of half-written source.
    /// Whatever it is handed, it hands something back.
    #[test]
    fn the_preprocessor_never_panics(src in script()) {
        let _ = preprocess_strudel_with_meta(&src);
    }

    /// Any printable Unicode at all, not just the fragments above. The
    /// preprocessor slices source by byte offset, which is where a
    /// multi-byte character ends up split.
    #[test]
    fn the_preprocessor_never_panics_on_any_unicode(src in r"\PC{0,200}") {
        let _ = preprocess_strudel_with_meta(&src);
    }

    /// Evaluation reports a broken script as an error. An engine panic is
    /// caught and reported too, but only as a last resort — reaching it means
    /// something rudel does, or lets through, took the engine down.
    #[test]
    fn evaluation_never_fails_by_panicking(src in script()) {
        if let Err(e) = eval(&src) {
            prop_assert!(!e.starts_with("the JavaScript engine failed"), "{src:?}: {e}");
        }
    }

    /// Every `m("…", offset)` the mini pass writes points at its literal in
    /// the source the user wrote — through sliders and widgets, which rewrite
    /// the text in front of it. The editor highlights from these offsets.
    #[test]
    fn mini_offsets_point_at_their_literal(src in valid_script()) {
        let out = preprocess_strudel_with_meta(&src).source;
        let mut rest = out.as_str();
        let mut seen = 0;
        while let Some(at) = rest.find("m(") {
            rest = &rest[at + 2..];
            let quote = rest.chars().next().unwrap();
            if !matches!(quote, '"' | '`') {
                continue;
            }
            let close = rest[1..].find(quote).unwrap() + 1;
            let content = &rest[1..close];
            let offset: usize = rest[close + 1..]
                .trim_start_matches([',', ' '])
                .split(')')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            prop_assert_eq!(&src[offset..offset + content.len()], content, "{}", out);
            seen += 1;
        }
        prop_assert!(seen > 0, "no literal was annotated: {}", out);
    }
}

/// A value a script can write as a literal: numbers, strings, arrays and
/// objects, nested.
fn literal() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        (-1000i64..1000).prop_map(Value::Int),
        (-1000.0f64..1000.0)
            .prop_filter("a fraction", |x| x.fract() != 0.0)
            .prop_map(Value::F64),
        any::<bool>().prop_map(Value::Bool),
        "[a-z ]{0,6}".prop_map(Value::Str),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::List),
            prop::collection::vec(("[a-z]{1,4}", inner), 0..4).prop_map(|entries| {
                // Later duplicates win, as they do in a JavaScript literal.
                Value::Map(entries.into_iter().collect())
            }),
        ]
    })
}

/// `value` written as JavaScript source.
fn js_literal(value: &Value) -> String {
    match value {
        Value::Int(n) => n.to_string(),
        Value::F64(x) => format!("{x:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => format!("'{s}'"),
        Value::List(items) => {
            let items: Vec<String> = items.iter().map(js_literal).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Map(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", js_literal(v)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        other => unreachable!("not generated: {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// A literal crosses from the script into a control unchanged: the value
    /// bridge reads arrays and objects all the way down, keeps object keys in
    /// the script's order, and keeps a whole number whole.
    #[test]
    fn a_literal_reaches_a_control_unchanged(value in literal()) {
        let script = format!("s('a').partials({})", js_literal(&value));
        let pat = eval(&script).map_err(|e| TestCaseError::fail(format!("{script}: {e}")))?;
        let got = values(&pat, 0, 1);
        let Some(Value::Map(controls)) = got.first() else {
            return Err(TestCaseError::fail(format!("{script}: {got:?}")));
        };
        prop_assert_eq!(controls.get("partials"), Some(&value), "{}", script);
    }
}
