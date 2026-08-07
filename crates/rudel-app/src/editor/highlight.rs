use eframe::egui;
use std::collections::{HashMap, HashSet};

use super::{decorations::FlashSpan, settings::EditorSettings};

/// Layout space reserved for inline decorations, so block widgets push the
/// following code down and inline sliders push the rest of their line right
/// (instead of the decorations painting on top of the code).
#[derive(Clone, Copy)]
pub(super) struct LayoutReservations<'a> {
    /// Source-line index -> full row height for that line (base row height plus
    /// the vertical gap reserved below it for block widgets).
    pub(super) line_heights: &'a HashMap<usize, f32>,
    /// `(from_byte, to_byte, gap_width)` for each slider literal: a `gap_width`
    /// gap is opened just before the literal (which stays visible) and the
    /// inline slider is drawn in that gap, next to its value like Strudel.
    pub(super) sliders: &'a [(usize, usize, f32)],
}

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

#[allow(clippy::too_many_arguments)]
pub(super) fn highlighted_editor_job(
    code: &str,
    wrap_width: f32,
    active: &[FlashSpan],
    brackets: &[(usize, usize)],
    active_line: Option<(usize, usize)>,
    idents: &HashSet<String>,
    settings: &EditorSettings,
    reservations: LayoutReservations<'_>,
) -> egui::text::LayoutJob {
    let font_id = settings.font_id();
    let palette = settings.theme.palette();
    let normal = egui::TextFormat::simple(font_id.clone(), palette.foreground);
    let keyword = egui::TextFormat::simple(font_id.clone(), palette.keyword);
    let method = egui::TextFormat::simple(font_id.clone(), palette.method);
    let string = egui::TextFormat::simple(font_id.clone(), palette.string);
    let number = egui::TextFormat::simple(font_id.clone(), palette.number);
    let comment = egui::TextFormat::simple(font_id.clone(), palette.comment);
    let mini_op = egui::TextFormat::simple(font_id.clone(), palette.mini_op);
    let mini_word = egui::TextFormat::simple(font_id, palette.mini_word);

    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = if settings.line_wrapping {
        wrap_width
    } else {
        f32::INFINITY
    };

    // Background flashed under spans of code currently producing a hap, and a
    // distinct one under the bracket pair around the cursor.
    let flash = palette.flash;
    let bracket_flash = palette.bracket_flash;
    let active_line_flash = palette.active_line;

    let mut line = 0usize;
    for (start, end, token) in tokenize(code, idents) {
        let piece = &code[start..end];
        let token = if settings.pattern_highlighting {
            token
        } else {
            Token::Normal
        };
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
        if active_line.is_some_and(|span| spans_overlap((start, end), span)) {
            format.background = active_line_flash;
        }
        // Active-event flash wins over bracket matching when they coincide.
        if brackets
            .iter()
            .any(|&span| spans_overlap((start, end), span))
        {
            format.background = bracket_flash;
        }
        // An event's own `markcss`/`color` wins over the theme's flash colour.
        if let Some(&(_, _, color)) = active
            .iter()
            .find(|&&(from, to, _)| spans_overlap((start, end), (from, to)))
        {
            format.background = color.map_or(flash, unpack_color);
        }
        // Reserve the vertical gap for block widgets: make the widget's line
        // tall and top-align its glyphs so the gap opens below the text (and the
        // next line is pushed down). The trailing newline keeps its normal
        // height so the following empty row is not also inflated.
        //
        // Only glyphs *without* a background carry the tall line height. epaint
        // sizes a background rect from the glyph's own `line_height`
        // (`Glyph::logical_rect`), not the row's, so inflating a flashed glyph
        // paints the active-event highlight as a bar running down through the
        // widget's gap. The row still takes its height from the tallest glyph.
        if piece != "\n"
            && let Some(&row_height) = reservations.line_heights.get(&line)
        {
            format.valign = egui::Align::TOP;
            if format.background == egui::Color32::TRANSPARENT {
                format.line_height = Some(row_height);
            }
        }
        // Reserve horizontal space for an inline slider: widen the advance of
        // the glyph just before the value literal (the `(` of `slider(`) so a
        // gap opens between it and the still-visible value, and the slider is
        // drawn in that gap — next to its value, like Strudel's widget.
        if let Some(&(_, _, gap)) = reservations
            .sliders
            .iter()
            .find(|&&(from, _, _)| end == from && start < end)
        {
            let split = piece.char_indices().next_back().map_or(0, |(i, _)| i);
            let mut gap_format = format.clone();
            gap_format.extra_letter_spacing = gap;
            job.append(&piece[..split], 0.0, format);
            job.append(&piece[split..], 0.0, gap_format);
        } else {
            job.append(piece, 0.0, format);
        }
        line += piece.bytes().filter(|&b| b == b'\n').count();
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

/// Unpack a `0xRRGGBBAA` flash colour (see [`FlashSpan`]).
fn unpack_color(packed: u32) -> egui::Color32 {
    let [r, g, b, a] = packed.to_be_bytes();
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Pack a colour into a [`FlashSpan`]'s `0xRRGGBBAA` slot.
pub(crate) fn pack_color(color: egui::Color32) -> u32 {
    u32::from_be_bytes(color.to_srgba_unmultiplied())
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
    fn the_token_stream_tiles_the_whole_source() {
        // Spans are contiguous, non-empty and cover every byte exactly once.
        // Any scanner that fails to advance, or advances twice, breaks this
        // before it produces a visibly wrong colour.
        for code in [
            r#"s("bd*2 ~ hh")"#,
            "// a comment\nx = 1",
            r#"a("q\"z") / 2"#,
            "note(\"c#4 -1.5\") 120",
            r#"s("bd"#, // unterminated: must still reach the end, not loop
            "",
        ] {
            let mut at = 0;
            for (start, end, _) in tokenize(code, &test_idents()) {
                assert_eq!(start, at, "gap or overlap at {at} in {code:?}");
                assert!(end > start, "empty span at {start} in {code:?}");
                at = end;
            }
            assert_eq!(at, code.len(), "did not reach the end of {code:?}");
        }
    }

    #[test]
    fn only_a_doubled_slash_opens_a_comment() {
        // `/` is also mini-notation's slow operator and a division, so a lone
        // one must not grey out the rest of the line.
        let toks = classify("a / b // tail");
        assert!(toks.contains(&("/", Token::Normal)), "{toks:?}");
        assert!(toks.contains(&("// tail", Token::Comment)), "{toks:?}");
        assert!(toks.contains(&("b", Token::Normal)), "{toks:?}");
    }

    #[test]
    fn a_string_ends_at_its_closing_quote() {
        // Located by byte span, not by text: the body of an escaped string
        // contains quote characters of its own.
        //          0123456
        let toks = tokenize(r#"s("bd") x"#, &test_idents());
        assert!(toks.contains(&(2, 3, Token::Str)), "opening: {toks:?}");
        assert!(toks.contains(&(5, 6, Token::Str)), "closing: {toks:?}");
        // ...and what follows is code again, not more string.
        assert!(toks.contains(&(8, 9, Token::Normal)), "after: {toks:?}");

        // An escaped quote belongs to the body; the string ends at the next
        // unescaped one, two bytes later.
        let toks = tokenize(r#"s("a\"b") x"#, &test_idents());
        assert!(toks.contains(&(2, 3, Token::Str)), "opening: {toks:?}");
        assert!(toks.contains(&(7, 8, Token::Str)), "closing: {toks:?}");
        assert!(toks.contains(&(10, 11, Token::Normal)), "after: {toks:?}");
    }

    #[test]
    fn a_number_may_end_the_source() {
        // The digit scan reads one past its own token; at the end of input
        // there is nothing there to read.
        assert!(classify("120").contains(&("120", Token::Number)));
        assert!(classify("x = 0.9").contains(&("0.9", Token::Number)));
    }

    #[test]
    fn mini_words_may_open_with_an_underscore_or_a_sharp() {
        // `_` continues a note and `#` sharpens one, so both start words in
        // mini-notation even though neither is a letter.
        let toks = classify(r#"n("_x #y")"#);
        assert!(toks.contains(&("_x", Token::MiniWord)), "{toks:?}");
        assert!(toks.contains(&("#y", Token::MiniWord)), "{toks:?}");
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
    fn a_widget_gap_opens_on_the_line_that_hosts_it() {
        // The reserved height is looked up per line, so the running line
        // counter has to advance by exactly the newlines it has passed —
        // counting the wrong bytes, or failing to accumulate, would open the
        // gap under the wrong line.
        let code = "s(\"bd\")\ns(\"hh\").spiral()\ns(\"cp\")";
        let brackets: [(usize, usize); 0] = [];
        let tall_line = |line: usize| {
            let job = highlighted_editor_job(
                code,
                400.0,
                &[],
                &brackets,
                None,
                &test_idents(),
                &EditorSettings::default(),
                LayoutReservations {
                    line_heights: &HashMap::from([(line, 220.0f32)]),
                    sliders: &[],
                },
            );
            // Which source lines the inflated sections fall on.
            job.sections
                .iter()
                .filter(|s| s.format.line_height == Some(220.0))
                .map(|s| {
                    job.text[..s.byte_range.start.0]
                        .bytes()
                        .filter(|&b| b == b'\n')
                        .count()
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        for line in [0usize, 1, 2] {
            assert_eq!(
                tall_line(line),
                std::collections::BTreeSet::from([line]),
                "a widget reserved on line {line} should inflate only that line"
            );
        }
    }

    #[test]
    fn a_flash_colour_survives_the_round_trip_through_its_packed_form() {
        // Flash spans carry their colour as a packed `0xRRGGBBAA` word so the
        // span list stays `Copy`; the two halves have to agree.
        for color in [
            egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4),
            egui::Color32::from_rgba_unmultiplied(255, 0, 128, 200),
            egui::Color32::TRANSPARENT,
        ] {
            assert_eq!(unpack_color(pack_color(color)), color, "{color:?}");
        }
        // Distinct colours pack to distinct words, so neither direction is
        // collapsing them.
        assert_ne!(
            pack_color(egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4)),
            pack_color(egui::Color32::from_rgba_unmultiplied(4, 3, 2, 1))
        );
    }

    #[test]
    fn a_slider_gap_opens_on_the_glyph_before_its_literal() {
        // The layouter widens the advance of the character immediately before
        // the value literal, so the slider has somewhere to sit. The gap must
        // land on *that* glyph and nowhere else.
        let code = "slider(0.5)";
        let brackets: [(usize, usize); 0] = [];
        let job_with = |sliders: &[(usize, usize, f32)]| {
            highlighted_editor_job(
                code,
                400.0,
                &[],
                &brackets,
                None,
                &test_idents(),
                &EditorSettings::default(),
                LayoutReservations {
                    line_heights: &HashMap::new(),
                    sliders,
                },
            )
        };
        // The literal `0.5` starts at byte 7, so the gap belongs to byte 6.
        let job = job_with(&[(7, 10, 40.0)]);
        let widened: Vec<_> = job
            .sections
            .iter()
            .filter(|s| s.format.extra_letter_spacing != 0.0)
            .map(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].to_string())
            .collect();
        assert_eq!(widened, vec!["(".to_string()], "{:?}", job.text);

        // With no reservation nothing is widened at all.
        assert!(
            job_with(&[])
                .sections
                .iter()
                .all(|s| s.format.extra_letter_spacing == 0.0),
            "no slider, no gap"
        );
    }

    #[test]
    fn a_widget_line_reserves_its_gap_without_stretching_the_flash() {
        // A line hosting a block widget is made tall so the gap opens below it.
        // epaint sizes a background rect from the glyph's own `line_height`
        // (`Glyph::logical_rect`), not the row's, so a flashed section must keep
        // its normal height or the active-event highlight paints as a bar
        // running down through the widget's gap.
        let code = r#"s("bd*4").spiral()"#;
        let line_heights = HashMap::from([(0usize, 220.0f32)]);
        let brackets: [(usize, usize); 0] = [];
        let job = highlighted_editor_job(
            code,
            400.0,
            &[(3, 7, None)], // the mini-notation leaf inside the string
            &brackets,
            None,
            &test_idents(),
            &EditorSettings::default(),
            LayoutReservations {
                line_heights: &line_heights,
                sliders: &[],
            },
        );

        let flashed: Vec<_> = job
            .sections
            .iter()
            .filter(|s| s.format.background != egui::Color32::TRANSPARENT)
            .collect();
        assert!(
            !flashed.is_empty(),
            "the active span should flash something"
        );
        assert!(
            flashed.iter().all(|s| s.format.line_height.is_none()),
            "a flashed section must not carry the widget's tall line height"
        );
        // The row still gets its height from the unflashed sections.
        assert!(
            job.sections
                .iter()
                .any(|s| s.format.line_height == Some(220.0)),
            "the line must still reserve the widget gap"
        );
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
