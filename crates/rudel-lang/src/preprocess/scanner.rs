pub(super) struct CallInfo {
    pub close_char: usize,
    pub first_arg: Option<(usize, usize)>,
    pub args: Vec<(usize, usize)>,
}

pub(super) fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

pub(super) fn next_byte(chars: &[(usize, char)], i: usize, len: usize) -> usize {
    chars.get(i + 1).map(|x| x.0).unwrap_or(len)
}

pub(super) fn previous_non_ws(chars: &[(usize, char)], i: usize) -> Option<char> {
    chars[..i]
        .iter()
        .rev()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(_, c)| *c)
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

pub(super) fn skip_string(chars: &[(usize, char)], mut i: usize, quote: char) -> usize {
    let mut escaped = false;
    i += 1;
    while i < chars.len() {
        let ch = chars[i].1;
        i += 1;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            break;
        }
    }
    i
}

pub(super) fn skip_line_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    while i < chars.len() && chars[i].1 != '\n' {
        i += 1;
    }
    i
}

pub(super) fn skip_block_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() {
        if chars[i].1 == '*' && chars[i + 1].1 == '/' {
            return i + 2;
        }
        i += 1;
    }
    chars.len()
}

pub(super) fn parse_call(src: &str, chars: &[(usize, char)], open: usize) -> Option<CallInfo> {
    let mut i = open + 1;
    let mut depth = 0i32;
    let mut arg_start = next_byte(chars, open, src.len());
    let mut args = Vec::new();
    while i < chars.len() {
        let (byte, c) = chars[i];
        if (c == '"' || c == '\'') && depth >= 0 {
            i = skip_string(chars, i, c);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            i = skip_line_comment(chars, i);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            i = skip_block_comment(chars, i);
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let range = trim_range(src, arg_start, byte);
                if range.0 < range.1 {
                    args.push(range);
                }
                let first_arg = args.first().copied();
                return Some(CallInfo {
                    close_char: i,
                    first_arg,
                    args,
                });
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                let range = trim_range(src, arg_start, byte);
                if range.0 < range.1 {
                    args.push(range);
                }
                arg_start = next_byte(chars, i, src.len());
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn top_level_ranges(text: &str, delimiter: char) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let (byte, ch) = chars[i];
        if ch == '"' || ch == '\'' {
            i = skip_string(&chars, i, ch);
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                if start < byte {
                    ranges.push((start, byte));
                }
                start = next_byte(&chars, i, text.len());
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

pub(super) fn top_level_split(text: &str, delimiter: char) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let (byte, ch) = chars[i];
        if ch == '"' || ch == '\'' {
            i = skip_string(&chars, i, ch);
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => return Some(byte),
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

    fn chars_of(s: &str) -> Vec<(usize, char)> {
        s.char_indices().collect()
    }

    /// The argument slices `parse_call` found, as strings.
    fn call_args(src: &str) -> Option<Vec<String>> {
        let chars = chars_of(src);
        let open = chars.iter().position(|(_, c)| *c == '(')?;
        let info = parse_call(src, &chars, open)?;
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
        let chars = chars_of(src);
        // From the final 'c', the previous non-blank is 'b'.
        assert_eq!(previous_non_ws(&chars, 7), Some('b'));
        // From the very start there is nothing behind.
        assert_eq!(previous_non_ws(&chars, 0), None);
        // Immediately after 'a' it is 'a' itself, not skipped past.
        assert_eq!(previous_non_ws(&chars, 1), Some('a'));
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
    fn skip_string_stops_at_the_closing_quote_and_honours_escapes() {
        let go = |src: &str| {
            let chars = chars_of(src);
            let start = chars.iter().position(|(_, c)| *c == '"').unwrap();
            skip_string(&chars, start, '"')
        };
        // Index lands just past the closing quote.
        assert_eq!(go(r#""ab"x"#), 4);
        // An escaped quote does not end the string...
        assert_eq!(go(r#""a\"b"x"#), 6);
        // ...and an escaped backslash does not escape the quote after it.
        assert_eq!(go(r#""a\\"x"#), 5);
        // An unterminated string runs to the end rather than looping.
        let src = r#""abc"#;
        assert_eq!(go(src), chars_of(src).len());
    }

    #[test]
    fn comments_are_skipped_to_their_own_terminators() {
        let line = "a // note\nb";
        let chars = chars_of(line);
        let at = chars.iter().position(|(_, c)| *c == '/').unwrap();
        // A line comment stops *at* the newline, not past it.
        assert_eq!(chars[skip_line_comment(&chars, at)].1, '\n');

        let block = "a /* note */ b";
        let chars = chars_of(block);
        let at = chars.iter().position(|(_, c)| *c == '/').unwrap();
        let end = skip_block_comment(&chars, at);
        assert_eq!(
            &block[chars[end].0..],
            " b",
            "should resume after the close"
        );

        // A `*` or `/` inside the comment does not close it early.
        let tricky = "/* a * b / c */x";
        let chars = chars_of(tricky);
        let end = skip_block_comment(&chars, 0);
        assert_eq!(&tricky[chars[end].0..], "x");

        // An unterminated block comment consumes the rest rather than looping.
        let open = "/* never closed";
        let chars = chars_of(open);
        assert_eq!(skip_block_comment(&chars, 0), chars.len());
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
    }

    #[test]
    fn parse_call_reports_the_first_argument_and_the_closing_paren() {
        let src = "note(\"c e g\", 2)";
        let chars = chars_of(src);
        let open = chars.iter().position(|(_, c)| *c == '(').unwrap();
        let info = parse_call(src, &chars, open).expect("a call");
        let (a, b) = info.first_arg.expect("a first argument");
        assert_eq!(&src[a..b], "\"c e g\"");
        assert_eq!(
            chars[info.close_char].1, ')',
            "close_char indexes the paren"
        );
        assert_eq!(info.close_char, chars.len() - 1);

        // An unclosed call is not a call.
        let bad = "note(\"c e g\"";
        let chars = chars_of(bad);
        assert!(parse_call(bad, &chars, 4).is_none());
        // ...and one with no arguments has no first argument.
        let empty = "f()";
        let chars = chars_of(empty);
        assert_eq!(parse_call(empty, &chars, 1).unwrap().first_arg, None);
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
