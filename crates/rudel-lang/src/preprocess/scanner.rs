//! The one scanner every preprocessor pass is built on.
//!
//! Positions are byte offsets throughout. Everything these scanners branch on —
//! quotes, comment markers, brackets, commas — is ASCII, and a UTF-8
//! continuation byte can never be mistaken for one, so they walk bytes and step
//! over multi-byte characters without having to index them. Slices are only ever
//! cut at a returned position, which is always a character boundary.

pub(super) struct CallInfo {
    /// Byte offset of the closing `)`.
    pub close: usize,
    pub first_arg: Option<(usize, usize)>,
    pub args: Vec<(usize, usize)>,
}

/// `src`'s bytes with every non-code byte flattened to `_` (newlines kept), so
/// a pass can scan one expression across the string literals inside it without
/// seeing their contents. Indices line up with `src` byte for byte, and every
/// structural character a pass branches on is ASCII, so a position found in the
/// mask is always a character boundary in `src`.
pub(super) fn code_mask(src: &str) -> Vec<u8> {
    let mut mask = src.as_bytes().to_vec();
    for (kind, start, end) in chunks(src) {
        if kind == Chunk::Code {
            continue;
        }
        for byte in &mut mask[start..end] {
            if *byte != b'\n' {
                *byte = b'_';
            }
        }
    }
    mask
}

pub(super) fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Whether the string literal starting at `at` is a JS *tagged* template — a
/// backtick body stuck straight onto a function name, `loadCsound`` ... ` ``.
///
/// It decides two things that have to agree: a tagged template is a call
/// argument, not mini-notation (upstream's `plugin-mini` says exactly this,
/// `TemplateLiteral && parent !== TaggedTemplateExpression`), and it is the
/// form `rewrite_tagged_templates` puts parentheses around. Disagreeing once
/// meant the mini pass glued its `m(` onto the tag, making `loadCsound` into
/// the undefined `loadCsoundm`.
pub(super) fn is_tagged_template(src: &str, at: usize) -> bool {
    src[at..].starts_with('`') && src[..at].chars().next_back().is_some_and(is_ident_char)
}

pub(super) fn previous_non_ws(src: &str, at: usize) -> Option<char> {
    src[..at].chars().rev().find(|c| !c.is_whitespace())
}

pub(super) fn trim_range(src: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end {
        let Some(c) = src[start..end].chars().next() else {
            break;
        };
        if !c.is_whitespace() {
            break;
        }
        start += c.len_utf8();
    }
    while start < end {
        let Some(c) = src[start..end].chars().next_back() else {
            break;
        };
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }
    (start, end)
}

/// Just past the string literal `quote` opens at `at`. An unterminated literal
/// runs to the end rather than looping.
fn scan_string(b: &[u8], at: usize, quote: u8) -> usize {
    let mut i = at + 1;
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c == b'\\' {
            // Whatever follows is literal. Landing mid-character is harmless:
            // continuation bytes match neither a quote nor a backslash.
            i += 1;
        } else if c == quote {
            return i;
        }
    }
    b.len()
}

/// The newline ending the `//` comment at `at`, exclusive — so the newline
/// itself stays code and line structure survives dropping the comment.
fn scan_line_comment(b: &[u8], at: usize) -> usize {
    b[at..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(b.len(), |n| at + n)
}

/// Just past the `*/` closing the block comment at `at`.
fn scan_block_comment(b: &[u8], at: usize) -> usize {
    let mut i = at + 2;
    while i + 1 < b.len() {
        if b[i] == b'*' && b[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    b.len()
}

/// A run of source, as the rewriters need to tell it apart: code they may
/// change, or text they must copy through untouched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Chunk {
    Code,
    Str,
    LineComment,
    BlockComment,
}

/// What begins at byte `at`, if it is not plain code, and the byte just past
/// it.
///
/// This is the single place that knows a quote from a comment opener. Every
/// rewriter used to carry its own copy of these three tests, which is why one
/// missed guard was really the same bug repeated eight times — and why the
/// mutation run put survivors on the comment-detection line of each.
pub(super) fn classify(src: &str, at: usize) -> Option<(Chunk, usize)> {
    let b = src.as_bytes();
    match (*b.get(at)?, b.get(at + 1).copied()) {
        // A backtick opens a JS template literal, which tunes use for the
        // multi-line mini-notation of a whole melody. Scanning it as a string
        // keeps its brackets and quotes out of every pass that counts them;
        // `normalize_string_literal` turns it into a Koto string.
        (quote @ (b'"' | b'\'' | b'`'), _) => Some((Chunk::Str, scan_string(b, at, quote))),
        (b'/', Some(b'/')) => Some((Chunk::LineComment, scan_line_comment(b, at))),
        (b'/', Some(b'*')) => Some((Chunk::BlockComment, scan_block_comment(b, at))),
        _ => None,
    }
}

/// `src` split into `(kind, start, end)` byte ranges: maximal runs of code, and
/// the string literals and comments between them.
///
/// Tokenising once per rewrite lets each pass say what it does to code and copy
/// everything else in bulk, rather than re-deriving the boundaries a character
/// at a time.
pub(super) fn chunks(src: &str) -> Vec<(Chunk, usize, usize)> {
    let mut out = Vec::new();
    let mut code_start = 0;
    let mut i = 0;
    while i < src.len() {
        let Some((kind, end)) = classify(src, i) else {
            i += 1;
            continue;
        };
        if code_start < i {
            out.push((Chunk::Code, code_start, i));
        }
        out.push((kind, i, end));
        code_start = end;
        i = end;
    }
    if code_start < src.len() {
        out.push((Chunk::Code, code_start, src.len()));
    }
    out
}

pub(super) fn parse_call(src: &str, open: usize) -> Option<CallInfo> {
    let b = src.as_bytes();
    let mut i = open + 1;
    let mut depth = 0i32;
    let mut arg_start = open + 1;
    let mut args = Vec::new();
    let push_arg = |args: &mut Vec<_>, from, to| {
        let range = trim_range(src, from, to);
        if range.0 < range.1 {
            args.push(range);
        }
    };
    while i < src.len() {
        if let Some((_, end)) = classify(src, i) {
            i = end;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' if depth == 0 => {
                push_arg(&mut args, arg_start, i);
                let first_arg = args.first().copied();
                return Some(CallInfo {
                    close: i,
                    first_arg,
                    args,
                });
            }
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                push_arg(&mut args, arg_start, i);
                arg_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split `text` on `delimiter` where it sits outside brackets and strings.
pub(super) fn top_level_ranges(text: &str, delimiter: char) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < text.len() {
        if let Some((Chunk::Str, end)) = classify(text, i) {
            i = end;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            // `i` walks bytes, so a multi-byte character would be sliced
            // through the middle of.
            _ if depth == 0 && text.is_char_boundary(i) && text[i..].starts_with(delimiter) => {
                if start < i {
                    ranges.push((start, i));
                }
                start = i + delimiter.len_utf8();
            }
            _ => {}
        }
        i += 1;
    }
    if start < text.len() {
        ranges.push((start, text.len()));
    }
    ranges
}

/// The first `delimiter` outside brackets and strings.
pub(super) fn top_level_split(text: &str, delimiter: char) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < text.len() {
        if let Some((Chunk::Str, end)) = classify(text, i) {
            i = end;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 0 && text.is_char_boundary(i) && text[i..].starts_with(delimiter) => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {

    // --- the source scanner -----------------------------------------------------
    //
    // Everything in `preprocess` is built on this: it finds the argument ranges the
    // widget and mini rewriters then splice into. A scanner that miscounts a quote,
    // a comment or a nesting level does not error — it rewrites the wrong span, and
    // the user gets a pattern they did not write.

    use super::*;

    /// The argument slices `parse_call` found, as strings.
    fn call_args(src: &str) -> Option<Vec<String>> {
        let open = src.find('(')?;
        let info = parse_call(src, open)?;
        Some(
            info.args
                .iter()
                .map(|&(a, b)| src[a..b].to_string())
                .collect(),
        )
    }

    #[test]
    fn identifier_characters_are_koto_identifiers() {
        for c in ['a', 'Z', '0', '9', '_', '$'] {
            assert!(is_ident_char(c), "{c:?} belongs in an identifier");
        }
        for c in ['-', '.', ' ', '(', '"', '\n', 'é'] {
            assert!(!is_ident_char(c), "{c:?} does not");
        }
    }

    #[test]
    fn previous_non_ws_skips_back_over_blanks_only() {
        let src = "ab  \n\t c";
        // From the final 'c', the previous non-blank is 'b'.
        assert_eq!(previous_non_ws(src, 7), Some('b'));
        // From the very start there is nothing behind.
        assert_eq!(previous_non_ws(src, 0), None);
        // Immediately after 'a' it is 'a' itself, not skipped past.
        assert_eq!(previous_non_ws(src, 1), Some('a'));
    }

    #[test]
    fn trim_range_strips_blanks_from_both_ends() {
        let src = "  hello  ";
        assert_eq!(trim_range(src, 0, src.len()), (2, 7));
        // An all-blank range collapses rather than inverting.
        let (a, b) = trim_range("     ", 0, 5);
        assert!(a >= b, "an all-blank range should be empty, got {a}..{b}");
        // Already-tight ranges are left alone.
        assert_eq!(trim_range("abc", 0, 3), (0, 3));
        // Multi-byte characters are stepped over whole, not split.
        let s = "  é  ";
        let (a, b) = trim_range(s, 0, s.len());
        assert_eq!(&s[a..b], "é");
    }

    #[test]
    fn a_string_run_ends_at_the_closing_quote_and_honours_escapes() {
        let go = |src: &str| classify(src, src.find('"').unwrap()).unwrap().1;
        // The end lands just past the closing quote.
        assert_eq!(go(r#""ab"x"#), 4);
        // An escaped quote does not end the string...
        assert_eq!(go(r#""a\"b"x"#), 6);
        // ...and an escaped backslash does not escape the quote after it.
        assert_eq!(go(r#""a\\"x"#), 5);
        // An unterminated string runs to the end rather than looping.
        let src = r#""abc"#;
        assert_eq!(go(src), src.len());
        // Multi-byte contents are stepped over whole, so the end stays on a
        // character boundary and the slice does not panic.
        let wide = r#""éé"x"#;
        assert_eq!(&wide[..go(wide)], r#""éé""#);
        // Including one that is *escaped*, which the byte scan skips a byte of.
        let escaped = r#""a\é" x"#;
        assert_eq!(&escaped[..go(escaped)], r#""a\é""#);
    }

    #[test]
    fn comment_runs_end_at_their_own_terminators() {
        let end = |src: &str| classify(src, src.find('/').unwrap()).unwrap().1;

        // A line comment stops *at* the newline, not past it, so the newline
        // survives as code and line numbers hold.
        let line = "a // note\nb";
        assert_eq!(&line[end(line)..], "\nb");
        // One with no newline at all ends with the source.
        assert_eq!(end("a // note"), "a // note".len());

        let block = "a /* note */ b";
        assert_eq!(&block[end(block)..], " b", "should resume after the close");

        // The scan starts *past* the opener, so the `/` of `/*` cannot be read
        // back as the `/` of a `*/`.
        let slash_first = "/*/ x */";
        assert_eq!(end(slash_first), slash_first.len());
        assert_eq!(
            end("/**/"),
            4,
            "an empty block comment closes at its own end"
        );

        // An unterminated comment whose last character is `*` must not read one
        // byte past the end looking for the `/` that would close it.
        let dangling = "/* abc *";
        assert_eq!(end(dangling), dangling.len());

        // A `*` or `/` inside the comment does not close it early.
        let tricky = "/* a * b / c */x";
        assert_eq!(&tricky[end(tricky)..], "x");

        // An unterminated block comment consumes the rest rather than looping.
        let open = "/* never closed";
        assert_eq!(end(open), open.len());
    }

    #[test]
    fn chunks_separate_code_from_the_text_copied_through() {
        // The whole preprocessor now leans on this one split, so it has to name
        // every run and lose none of the source.
        let kinds = |src: &str| {
            chunks(src)
                .into_iter()
                .map(|(kind, a, b)| (kind, src[a..b].to_string()))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            kinds(r#"s("bd") // go"#),
            [
                (Chunk::Code, "s(".into()),
                (Chunk::Str, r#""bd""#.into()),
                (Chunk::Code, ") ".into()),
                (Chunk::LineComment, "// go".into()),
            ]
        );
        // A line comment stops *before* its newline, so the newline stays code
        // and `strip_line_comments` keeps the line structure by dropping the
        // comment chunk alone.
        assert_eq!(
            kinds("a // x\nb"),
            [
                (Chunk::Code, "a ".into()),
                (Chunk::LineComment, "// x".into()),
                (Chunk::Code, "\nb".into()),
            ]
        );
        // Comment openers and quotes nested in one another belong to whichever
        // run started first.
        assert_eq!(
            kinds(r#"/* "a" // b */"#),
            [(Chunk::BlockComment, r#"/* "a" // b */"#.into())]
        );
        assert_eq!(
            kinds(r#""// not a comment""#),
            [(Chunk::Str, r#""// not a comment""#.into())]
        );
        // A division is not a comment.
        assert_eq!(kinds("a / b"), [(Chunk::Code, "a / b".into())]);
        // Unterminated runs consume the rest rather than being dropped.
        assert_eq!(
            kinds(r#"x = "ab"#),
            [(Chunk::Code, "x = ".into()), (Chunk::Str, r#""ab"#.into()),]
        );
        assert_eq!(kinds(""), []);

        // Whatever the split, concatenating it reproduces the source exactly.
        for src in [
            r#"s("bd*2 [~ sd]").gain(.5) // hi"#,
            "/* a */'b'//c\nd",
            "no chunks at all",
            r#"'é' /* é */ é"#,
        ] {
            let joined: String = chunks(src).iter().map(|&(_, a, b)| &src[a..b]).collect();
            assert_eq!(joined, src, "chunks must partition the source");
        }
    }

    #[test]
    fn classify_names_only_what_starts_at_the_index() {
        let src = r#"a"b"//c"#;
        assert_eq!(classify(src, 0), None, "plain code is not classified");
        assert!(matches!(classify(src, 1), Some((Chunk::Str, _))));
        assert!(matches!(classify(src, 4), Some((Chunk::LineComment, _))));
        // A lone `/` is division, not a comment.
        assert_eq!(classify("a / b", 2), None);
        // Past the end there is nothing rather than a panic.
        assert_eq!(classify(src, src.len()), None);
    }

    #[test]
    fn parse_call_splits_arguments_at_top_level_commas_only() {
        assert_eq!(call_args("f(a, b, c)").unwrap(), ["a", "b", "c"]);
        // Commas inside nested brackets belong to the inner call.
        assert_eq!(call_args("f(g(a, b), c)").unwrap(), ["g(a, b)", "c"]);
        assert_eq!(
            call_args("f([a, b], {c, d})").unwrap(),
            ["[a, b]", "{c, d}"]
        );
        // Arguments are trimmed.
        assert_eq!(call_args("f(  a  ,  b  )").unwrap(), ["a", "b"]);
        // Empty argument lists and trailing separators produce no empty entries.
        assert!(call_args("f()").unwrap().is_empty());
        assert_eq!(call_args("f(a,)").unwrap(), ["a"]);
    }

    #[test]
    fn parse_call_ignores_delimiters_inside_strings_and_comments() {
        // A comma or paren in a string is text, not structure — this is the whole
        // reason the scanner exists rather than a `split(',')`.
        assert_eq!(call_args(r#"f("a,b", c)"#).unwrap(), [r#""a,b""#, "c"]);
        assert_eq!(call_args(r#"f("a)b", c)"#).unwrap(), [r#""a)b""#, "c"]);
        assert_eq!(call_args("f('x,y', z)").unwrap(), ["'x,y'", "z"]);
        // Same for comments.
        assert_eq!(
            call_args("f(a /* , b */ , c)").unwrap(),
            ["a /* , b */", "c"]
        );
        let with_line = "f(a, // , b\n c)";
        assert_eq!(call_args(with_line).unwrap(), ["a", "// , b\n c"]);
        // Strings nested inside an argument are skipped at any depth, so a
        // bracket quoted in there cannot unbalance the outer call.
        assert_eq!(
            call_args(r#"f(g("a)b"), c)"#).unwrap(),
            [r#"g("a)b")"#, "c"]
        );
        assert_eq!(call_args(r#"f([" ,"], c)"#).unwrap(), [r#"[" ,"]"#, "c"]);
    }

    #[test]
    fn parse_call_reports_the_first_argument_and_the_closing_paren() {
        let src = "note(\"c e g\", 2)";
        let info = parse_call(src, src.find('(').unwrap()).expect("a call");
        let (a, b) = info.first_arg.expect("a first argument");
        assert_eq!(&src[a..b], "\"c e g\"");
        assert_eq!(&src[info.close..], ")", "close is the paren's own offset");
        assert_eq!(info.close, src.len() - 1);

        // Multi-byte arguments come back on character boundaries.
        let wide = "f(\"é\", 2)";
        let info = parse_call(wide, 1).expect("a call");
        let (a, b) = info.first_arg.expect("a first argument");
        assert_eq!(&wide[a..b], "\"é\"");
        assert_eq!(&wide[info.close..], ")");

        // An unclosed call is not a call.
        assert!(parse_call("note(\"c e g\"", 4).is_none());
        // ...and one with no arguments has no first argument.
        assert_eq!(parse_call("f()", 1).unwrap().first_arg, None);
    }

    #[test]
    fn top_level_ranges_and_split_respect_nesting_and_quotes() {
        let pieces = |text: &str, d: char| {
            top_level_ranges(text, d)
                .into_iter()
                .map(|(a, b)| text[a..b].to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(pieces("a,b,c", ','), ["a", "b", "c"]);
        // A delimiter inside brackets or quotes is not a separator.
        assert_eq!(pieces("a,[b,c],d", ','), ["a", "[b,c]", "d"]);
        assert_eq!(pieces(r#"a,"b,c",d"#, ','), ["a", r#""b,c""#, "d"]);
        // No delimiter at all gives the whole text as one range.
        assert_eq!(pieces("abc", ','), ["abc"]);
        // A delimiter with nothing on one side of it yields no empty range —
        // the widget option parser would otherwise see a keyless entry.
        assert_eq!(pieces(",a", ','), ["a"], "no empty range before a leader");
        assert_eq!(pieces("a,", ','), ["a"], "none after a trailer either");
        assert_eq!(pieces(",", ','), Vec::<String>::new());

        // `top_level_split` finds the first separator at depth zero, or nothing.
        fn at(text: &str, d: char) -> Option<&str> {
            top_level_split(text, d).map(|i| &text[..i])
        }
        assert_eq!(at("a:b", ':'), Some("a"));
        assert_eq!(at("[a:b]:c", ':'), Some("[a:b]"));
        assert_eq!(at(r#""a:b":c"#, ':'), Some(r#""a:b""#));
        assert_eq!(at("abc", ':'), None);
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn splitting_steps_over_a_multi_byte_character() {
        // The cursor walks bytes, so slicing at one to test for the delimiter
        // panicked in the middle of any character wider than ASCII — reachable
        // from any source with an emoji in it.
        assert_eq!(top_level_split("🌸, b", ','), Some(4));
        assert_eq!(top_level_split("🌸 b", ','), None);
        assert_eq!(
            top_level_ranges("🌸, b", ','),
            vec![(0, 4), (5, "🌸, b".len())]
        );
    }
}
