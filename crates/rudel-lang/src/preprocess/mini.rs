use super::{
    scanner::{Chunk, classify, is_ident_char},
    widgets::VISUAL_WIDGET_METHODS,
};

/// Map a byte position in the widget-rewritten source back to the original
/// editor source using the verbatim-copy anchors gathered during the widget
/// pass. `anchors` is `(rewritten_start, original_start)` for each unchanged
/// chunk, in ascending order; positions inside a chunk shift by a constant.
fn map_to_source(anchors: &[(usize, usize)], pos: usize) -> usize {
    match anchors.binary_search_by(|(out_start, _)| out_start.cmp(&pos)) {
        Ok(idx) => {
            let (out_start, src_start) = anchors[idx];
            src_start + (pos - out_start)
        }
        Err(0) => pos,
        Err(idx) => {
            let (out_start, src_start) = anchors[idx - 1];
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
