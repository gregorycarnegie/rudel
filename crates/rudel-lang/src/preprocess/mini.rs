use super::{
    scanner::{Chunk, classify, is_ident_char},
    widgets::VISUAL_WIDGET_METHODS,
};

/// Map a byte position in the widget-rewritten source back to the original
/// editor source using the verbatim-copy anchors gathered during the widget
/// pass. `anchors` is `(rewritten_start, original_start)` for each unchanged
/// chunk, in ascending order; positions inside a chunk shift by a constant.
fn map_to_source(anchors: &[(usize, usize)], pos: usize) -> usize {
    // The last chunk starting at or before `pos` is the one it belongs to;
    // before the first chunk there is nothing to shift by.
    match anchors.partition_point(|(out_start, _)| *out_start <= pos) {
        0 => pos,
        n => {
            let (out_start, src_start) = anchors[n - 1];
            src_start + (pos - out_start)
        }
    }
}

/// Wrap every mini-notation string literal `"..."` / `'...'` in `m(literal,
/// offset)`, where `offset` is the byte position of the string *content* in
/// the original source. This is the analog of Strudel's `plugin-mini` rewrite
/// (`m(value, location)`): it lets per-hap source locations be reported as
/// absolute offsets into the editor text. Runs after the widget pass, using
/// `anchors` to keep offsets aligned with the raw editor source.
///
/// Map keys (`"x": ...`) are left alone — they are not patterns — and string
/// interiors and `//` comments are skipped so an apostrophe or quote inside
/// them does not desync the scanner.
pub(super) fn annotate_mini_offsets(
    src: &str,
    node_offset: usize,
    anchors: &[(usize, usize)],
) -> (String, Vec<(usize, usize)>) {
    let mut out = String::with_capacity(src.len() + 16);
    let mut locations = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let Some((kind, end)) = classify(src, i) else {
            let c = src[i..].chars().next().unwrap_or_default();
            out.push(c);
            i += c.len_utf8();
            continue;
        };
        // Comments are copied through untouched; string literals are what this
        // pass is for.
        if kind != Chunk::Str {
            out.push_str(&src[i..end]);
            i = end;
            continue;
        }

        let lit_start = i;
        let lit_end = end;
        let quote = src[i..].chars().next().unwrap_or('"');
        let content_byte = lit_start + quote.len_utf8();
        // A closed literal ends *on* its quote; an unterminated one ran to the
        // end of the source and has no closing quote to stop before.
        let content_end = if src[..lit_end].ends_with(quote) && lit_end > content_byte {
            lit_end - quote.len_utf8()
        } else {
            src.len()
        };
        i = lit_end;
        let literal = &src[lit_start..lit_end];

        // Only *double*-quoted strings are mini-notation, matching Strudel's
        // `plugin-mini` (`isStringWithDoubleQuotes`). Single quotes are the
        // escape hatch for a plain string — which is how upstream examples such
        // as `.filter(hap => hap.value.s === 'hh')` compare against one.
        //
        // A string immediately followed by `:` is a map key, not a pattern.
        // Generated slider ids are runtime strings inserted by the widget pass,
        // so they must also stay out of mini-notation/source-location metadata.
        if quote == '\''
            || src[i..].trim_start().starts_with(':')
            || is_slider_id_literal(src, lit_start)
        {
            out.push_str(literal);
        } else {
            let content_start = map_to_source(anchors, content_byte) + node_offset;
            let content_finish = map_to_source(anchors, content_end) + node_offset;
            locations.push((content_start, content_finish));
            out.push_str("m(");
            out.push_str(literal);
            out.push_str(", ");
            out.push_str(&content_start.to_string());
            out.push(')');
        }
    }
    (out, locations)
}

fn is_slider_id_literal(src: &str, quote_start: usize) -> bool {
    let mut end = quote_start;
    while end > 0 {
        let Some(c) = src[..end].chars().next_back() else {
            return false;
        };
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }
    if end == 0 || !src[..end].ends_with('(') {
        return false;
    }
    end -= '('.len_utf8();

    while end > 0 {
        let Some(c) = src[..end].chars().next_back() else {
            return false;
        };
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }

    let mut start = end;
    while start > 0 {
        let Some(c) = src[..start].chars().next_back() else {
            break;
        };
        if !is_ident_char(c) {
            break;
        }
        start -= c.len_utf8();
    }
    matches!(&src[start..end], "slider_with_id" | "sliderWithID")
        || src[start..end].starts_with("rudel_widget_")
        || VISUAL_WIDGET_METHODS.contains(&&src[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    // This module had no tests of its own — only two end-to-end assertions in
    // `tests/preprocess.rs` — so its helpers were reached but never pinned. The
    // 2026-08 mutation run put 11 of its 17 survivors inside
    // `is_slider_id_literal`, whose backwards walk over whitespace was never
    // once given whitespace to walk over.

    /// The `m(...)` locations recorded for `src`, with no widget pass in front.
    fn locations(src: &str) -> Vec<(usize, usize)> {
        annotate_mini_offsets(src, 0, &[]).1
    }

    fn rewritten(src: &str) -> String {
        annotate_mini_offsets(src, 0, &[]).0
    }

    #[test]
    fn a_double_quoted_literal_is_annotated_with_its_content_range() {
        let src = r#"s("bd sd")"#;
        assert_eq!(locations(src), [(3, 8)]);
        assert_eq!(&src[3..8], "bd sd", "the range covers the content only");
        assert_eq!(rewritten(src), r#"s(m("bd sd", 3))"#);
    }

    #[test]
    fn single_quotes_and_map_keys_stay_out_of_the_metadata() {
        // Single quotes are the escape hatch for a plain string, matching
        // `isStringWithDoubleQuotes` upstream.
        assert!(locations("x = 'hh'").is_empty());
        // A literal used as a map key is not a pattern.
        assert!(locations(r#"{"fold": 1}"#).is_empty());
        // Whitespace before the colon does not make it one either.
        assert!(locations(r#"{"fold"  : 1}"#).is_empty());
        // ...but the same literal not followed by a colon is a pattern.
        assert_eq!(locations(r#"{a: "bd"}"#).len(), 1);
    }

    #[test]
    fn a_literal_inside_a_comment_is_not_a_pattern() {
        // Both comment kinds: this pass runs before `strip_line_comments`, so
        // it is the one that has to know.
        assert!(locations(r#"// s("bd")"#).is_empty());
        assert!(locations(r#"/* s("bd") */"#).is_empty());
        // And the source comes back untouched.
        let src = r#"/* "bd" */"#;
        assert_eq!(rewritten(src), src);
    }

    #[test]
    fn an_unterminated_literal_reports_content_to_the_end_of_the_source() {
        // Malformed input still has to produce *some* range rather than
        // panicking or reporting one that runs backwards.
        let src = "x = \"ab";
        assert_eq!(locations(src), [(5, src.len())]);
        // A bare opening quote has no content at all, so the range is empty
        // rather than reaching back before the quote.
        let bare = "x = \"";
        assert_eq!(locations(bare), [(5, 5)]);
        // A closed literal at the very end still stops on its closing quote.
        let closed = "x = \"ab\"";
        assert_eq!(locations(closed), [(5, 7)]);
    }

    #[test]
    fn the_node_offset_shifts_every_recorded_location() {
        let src = r#"s("bd")"#;
        let (_, shifted) = annotate_mini_offsets(src, 100, &[]);
        assert_eq!(shifted, [(103, 105)]);
    }

    #[test]
    fn map_to_source_shifts_by_the_chunk_a_position_lands_in() {
        // `(rewritten_start, original_start)` pairs, ascending.
        let anchors = [(0, 0), (10, 4), (30, 9)];
        // Before the first chunk there is nothing to shift by.
        assert_eq!(map_to_source(&[(5, 2)], 3), 3);
        // Exactly on a chunk's start maps to that chunk's origin.
        assert_eq!(map_to_source(&anchors, 10), 4);
        assert_eq!(map_to_source(&anchors, 30), 9);
        // Inside a chunk the shift is constant.
        assert_eq!(map_to_source(&anchors, 12), 6);
        assert_eq!(map_to_source(&anchors, 33), 12);
        // Past the last chunk it keeps using that chunk's shift.
        assert_eq!(map_to_source(&anchors, 100), 79);
        // With no anchors at all, positions pass through.
        assert_eq!(map_to_source(&[], 7), 7);
    }

    #[test]
    fn a_generated_slider_id_is_not_mini_notation() {
        // The widget pass inserts these as runtime strings; annotating them
        // would put a source location on text the user never wrote.
        for src in [
            r#"slider_with_id("0:3", 0.5)"#,
            r#"sliderWithID("0:3", 0.5)"#,
            r#"rudel_widget_spiral("w_0")"#,
            r#"_spiral("w_0")"#,
            r#"pianoroll("w_0")"#,
        ] {
            assert!(locations(src).is_empty(), "id literal annotated: {src}");
        }
    }

    #[test]
    fn the_slider_id_walk_steps_over_whitespace_on_both_sides_of_the_paren() {
        // The backwards walk skips blanks between the name and `(`, and between
        // `(` and the literal. Without either, a formatted call is annotated as
        // a pattern and the editor grows a highlight over a generated id.
        for src in [
            r#"slider_with_id( "0:3")"#,
            r#"slider_with_id ("0:3")"#,
            r#"slider_with_id ( "0:3")"#,
            "slider_with_id(\n    \"0:3\")",
            "_spiral\n(\n\"w_0\")",
        ] {
            assert!(locations(src).is_empty(), "id literal annotated: {src}");
        }
    }

    #[test]
    fn only_a_call_of_a_widget_name_suppresses_annotation() {
        // The `(` is required. A widget name used as a plain variable is
        // ordinary code, and the pattern assigned to it is still a pattern.
        assert_eq!(locations(r#"spiral = "bd""#).len(), 1, "assignment");
        assert_eq!(locations(r#"spiral "bd""#).len(), 1, "no parenthesis");
        // A different function of a similar name is not a widget.
        assert_eq!(locations(r#"myslider_with_id("bd")"#).len(), 1);
        // Nor is a bare call with no name in front of the paren.
        assert_eq!(locations(r#"("bd")"#).len(), 1);
        // A literal at the very start of the source has nothing behind it.
        assert_eq!(locations(r#""bd""#).len(), 1);
    }
}
