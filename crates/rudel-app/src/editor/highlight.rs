use eframe::egui;
use std::collections::HashSet;

/// Highlight category for a contiguous byte span of editor text. Mirrors the
/// token categories Strudel's CodeMirror grammar distinguishes, including
/// mini-notation tokens inside string literals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Token {
    /// Plain code (identifiers, whitespace, operators outside strings).
    Normal,
    /// Language keyword, factory, control, or signal name.
    Keyword,
    /// Identifier following a `.` (a method/control call).
    Method,
    /// String delimiters and inert string content (whitespace inside mini).
    Str,
    /// Numeric literal (in code or mini-notation).
    Number,
    /// `//` line comment.
    Comment,
    /// Mini-notation word: sample/synth/note name.
    MiniWord,
    /// Mini-notation operator or grouping: `* / ! @ < > [ ] ( ) { } , . ? : | % -`.
    MiniOp,
    /// Mini-notation rest (`~`).
    MiniRest,
}

pub(super) fn highlighted_editor_job(
    code: &str,
    ui: &egui::Ui,
    wrap_width: f32,
    active: &[(usize, usize)],
    brackets: &[(usize, usize)],
    idents: &HashSet<String>,
) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let normal = egui::TextFormat::simple(font_id.clone(), ui.visuals().text_color());
    let keyword = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(106, 153, 205));
    let method = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(220, 220, 170));
    let string = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(206, 145, 120));
    let number = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(181, 206, 168));
    let comment = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(106, 153, 85));
    let mini_op = egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(197, 134, 192));
    let mini_word = egui::TextFormat::simple(font_id, egui::Color32::from_rgb(156, 220, 254));

    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;

    // Background flashed under spans of code currently producing a hap, and a
    // distinct one under the bracket pair around the cursor.
    let flash = egui::Color32::from_rgb(74, 68, 38);
    let bracket_flash = egui::Color32::from_rgb(60, 84, 104);

    for (start, end, token) in tokenize(code, idents) {
        let base = match token {
            Token::Normal => &normal,
            Token::Keyword => &keyword,
            Token::Method => &method,
            Token::Str | Token::MiniRest => &string,
            Token::Number => &number,
            Token::Comment => &comment,
            Token::MiniWord => &mini_word,
            Token::MiniOp => &mini_op,
        };
        let mut format = base.clone();
        // Active-event flash wins over bracket matching when they coincide.
        if brackets
            .iter()
            .any(|&span| spans_overlap((start, end), span))
        {
            format.background = bracket_flash;
        }
        if active.iter().any(|&span| spans_overlap((start, end), span)) {
            format.background = flash;
        }
        job.append(&code[start..end], 0.0, format);
    }

    job
}

/// Split `code` into contiguous highlighted spans (byte ranges). String
/// literals are further tokenized as mini-notation so words, numbers, rests,
/// and operators get distinct colors.
pub(super) fn tokenize(code: &str, idents: &HashSet<String>) -> Vec<(usize, usize, Token)> {
    let mut tokens = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i] as char;

        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push((start, i, Token::Comment));
        } else if c == '"' || c == '\'' {
            let quote = bytes[i];
            i += 1;
            tokens.push((start, i, Token::Str)); // opening quote
            let body_start = i;
            let mut escaped = false;
            while i < bytes.len() {
                let b = bytes[i];
                if escaped {
                    escaped = false;
                    i += 1;
                } else if b == b'\\' {
                    escaped = true;
                    i += 1;
                } else if b == quote {
                    break;
                } else {
                    i += 1;
                }
            }
            tokenize_mini(&code[body_start..i], body_start, &mut tokens);
            if i < bytes.len() && bytes[i] == quote {
                tokens.push((i, i + 1, Token::Str)); // closing quote
                i += 1;
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '.' | '_' | '/') {
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push((start, i, Token::Number));
        } else if c.is_ascii_alphabetic() || matches!(c, '_' | '$') {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '_' | '$') {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &code[start..i];
            let token = if start > 0 && bytes[start - 1] == b'.' {
                Token::Method
            } else if idents.contains(ident) {
                Token::Keyword
            } else {
                Token::Normal
            };
            tokens.push((start, i, token));
        } else {
            i += 1;
            tokens.push((start, i, Token::Normal));
        }
    }
    tokens
}

/// Two half-open byte ranges overlap.
fn spans_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Tokenize the body of a string literal as mini-notation, pushing spans
/// offset by `offset` (the byte position of the body within the full source).
fn tokenize_mini(body: &str, offset: usize, tokens: &mut Vec<(usize, usize, Token)>) {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i] as char;
        let next_is_digit = bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit());

        if c == '~' {
            i += 1;
            tokens.push((offset + start, offset + i, Token::MiniRest));
        } else if c.is_ascii_digit() || (c == '-' && next_is_digit) {
            i += 1;
            while i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.') {
                i += 1;
            }
            tokens.push((offset + start, offset + i, Token::Number));
        } else if c.is_ascii_alphabetic() || c == '_' || c == '#' {
            i += 1;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_alphanumeric() || matches!(b, '#' | '_' | '\'') {
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push((offset + start, offset + i, Token::MiniWord));
        } else if matches!(
            c,
            '*' | '/'
                | '!'
                | '@'
                | '<'
                | '>'
                | '['
                | ']'
                | '('
                | ')'
                | '{'
                | '}'
                | ','
                | '.'
                | '?'
                | ':'
                | '|'
                | '%'
                | '-'
        ) {
            i += 1;
            tokens.push((offset + start, offset + i, Token::MiniOp));
        } else {
            i += 1;
            tokens.push((offset + start, offset + i, Token::Str));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small highlight-ident set standing in for the runtime-generated one.
    fn test_idents() -> HashSet<String> {
        ["stack", "note", "n", "s", "seq", "cat"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// Collect `(text, token)` pairs so tests can assert on classification
    /// without depending on egui colors.
    fn classify(code: &str) -> Vec<(&str, Token)> {
        tokenize(code, &test_idents())
            .into_iter()
            .map(|(start, end, token)| (&code[start..end], token))
            .collect()
    }

    #[test]
    fn highlights_keywords_methods_and_numbers_in_code() {
        let toks = classify("stack(x).gain(0.9)");
        assert!(toks.contains(&("stack", Token::Keyword)));
        assert!(toks.contains(&("gain", Token::Method)));
        assert!(toks.contains(&("0.9", Token::Number)));
        assert!(toks.contains(&("x", Token::Normal)));
    }

    #[test]
    fn tokenizes_mini_notation_inside_strings() {
        let toks = classify(r#"s("bd*2 ~ [hh hh:3]")"#);
        assert!(toks.contains(&("bd", Token::MiniWord)));
        assert!(toks.contains(&("hh", Token::MiniWord)));
        assert!(toks.contains(&("*", Token::MiniOp)));
        assert!(toks.contains(&("[", Token::MiniOp)));
        assert!(toks.contains(&(":", Token::MiniOp)));
        assert!(toks.contains(&("2", Token::Number)));
        assert!(toks.contains(&("3", Token::Number)));
        assert!(toks.contains(&("~", Token::MiniRest)));
        // Delimiters are still string-colored.
        assert_eq!(toks.first(), Some(&("s", Token::Keyword)));
        assert!(toks.contains(&("\"", Token::Str)));
    }

    #[test]
    fn tokenizes_note_names_and_decimals_in_mini() {
        let toks = classify(r#"note("c#4 -1.5")"#);
        assert!(toks.contains(&("c#4", Token::MiniWord)));
        assert!(toks.contains(&("-1.5", Token::Number)));
    }

    #[test]
    fn active_spans_overlap_token_ranges() {
        // A leaf location (3,5) should flash the token covering bytes 3..5.
        assert!(spans_overlap((3, 5), (3, 5)));
        assert!(spans_overlap((3, 5), (4, 9))); // partial overlap
        assert!(!spans_overlap((3, 5), (5, 7))); // adjacent, half-open
        assert!(!spans_overlap((3, 5), (0, 3)));
    }

    #[test]
    fn tokens_cover_the_whole_source_contiguously() {
        let code = r#"n("0 1").s("piano") // hi"#;
        let mut next = 0;
        for (start, end, _) in tokenize(code, &test_idents()) {
            assert_eq!(start, next, "gap before {start}");
            assert!(end > start);
            next = end;
        }
        assert_eq!(next, code.len());
    }
}
