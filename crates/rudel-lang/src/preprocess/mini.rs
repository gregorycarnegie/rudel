use super::{scanner::is_ident_char, widgets::VISUAL_WIDGET_METHODS};

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
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut locations = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            while i < chars.len() && chars[i].1 != '\n' {
                out.push(chars[i].1);
                i += 1;
            }
            continue;
        }
        if c != '"' && c != '\'' {
            out.push(c);
            i += 1;
            continue;
        }

        let quote = c;
        let lit_start = chars[i].0;
        let content_byte = chars.get(i + 1).map(|x| x.0).unwrap_or(src.len());
        i += 1;
        let mut escaped = false;
        let mut content_end = src.len();
        while i < chars.len() {
            let (byte, ch) = chars[i];
            i += 1;
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                content_end = byte;
                break;
            }
        }
        let lit_end = chars.get(i).map(|x| x.0).unwrap_or(src.len());
        let literal = &src[lit_start..lit_end];

        // A string immediately followed by `:` is a map key, not a pattern.
        // Generated slider ids are runtime strings inserted by the widget pass,
        // so they must also stay out of mini-notation/source-location metadata.
        let mut j = i;
        while j < chars.len() && chars[j].1.is_whitespace() {
            j += 1;
        }
        if chars.get(j).map(|x| x.1) == Some(':') || is_slider_id_literal(src, lit_start) {
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
