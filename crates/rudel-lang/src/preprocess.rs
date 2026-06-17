use crate::WidgetOption;
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct PreprocessMeta {
    pub mini_locations: Vec<(usize, usize)>,
    pub widgets: Vec<PreprocessWidget>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct PreprocessWidget {
    pub widget_type: String,
    pub id: String,
    pub from: usize,
    pub to: usize,
    pub index: usize,
    pub options: BTreeMap<String, WidgetOption>,
    pub value: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreprocessResult {
    pub source: String,
    pub meta: PreprocessMeta,
}

/// Wrap every mini-notation string literal `"..."` / `'...'` in `m(literal,
/// offset)`, where `offset` is the byte position of the string *content* in
/// the original source. This is the analog of Strudel's `plugin-mini` rewrite
/// (`m(value, location)`): it lets per-hap source locations be reported as
/// absolute offsets into the editor text. Runs first, on the raw source, so
/// the offsets match what the editor displays; later passes only move
/// surrounding code, never the baked-in offset constants.
///
/// Map keys (`"x": ...`) are left alone — they are not patterns — and string
/// interiors and `//` comments are skipped so an apostrophe or quote inside
/// them does not desync the scanner.
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

/// `anchors` maps positions in `src` (the widget-rewritten source) back to the
/// original editor source so the recorded mini-notation offsets — including the
/// ones embedded in `m(literal, offset)` at runtime — stay aligned with the
/// text the editor actually displays. An empty `anchors` means `src` is the
/// original source and the mapping is the identity.
fn annotate_mini_offsets(
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

struct CallInfo {
    close_char: usize,
    first_arg: Option<(usize, usize)>,
    args: Vec<(usize, usize)>,
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn next_byte(chars: &[(usize, char)], i: usize, len: usize) -> usize {
    chars.get(i + 1).map(|x| x.0).unwrap_or(len)
}

fn previous_non_ws(chars: &[(usize, char)], i: usize) -> Option<char> {
    chars[..i]
        .iter()
        .rev()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(_, c)| *c)
}

fn trim_range(src: &str, mut start: usize, mut end: usize) -> (usize, usize) {
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

fn skip_string(chars: &[(usize, char)], mut i: usize, quote: char) -> usize {
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

fn skip_line_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    while i < chars.len() && chars[i].1 != '\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(chars: &[(usize, char)], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() {
        if chars[i].1 == '*' && chars[i + 1].1 == '/' {
            return i + 2;
        }
        i += 1;
    }
    chars.len()
}

fn parse_call(src: &str, chars: &[(usize, char)], open: usize) -> Option<CallInfo> {
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

fn numeric_arg(src: &str, range: Option<&(usize, usize)>) -> Option<f64> {
    let (start, end) = *range?;
    src[start..end].trim().parse().ok()
}

fn parse_widget_options(
    src: &str,
    range: Option<&(usize, usize)>,
) -> BTreeMap<String, WidgetOption> {
    let Some(&(start, end)) = range else {
        return BTreeMap::new();
    };
    let text = src[start..end].trim();
    let Some(inner) = text.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return BTreeMap::new();
    };

    top_level_ranges(inner, ',')
        .into_iter()
        .filter_map(|(from, to)| {
            let entry = inner[from..to].trim();
            let split = top_level_split(entry, ':')?;
            let key = normalize_option_key(entry[..split].trim())?;
            let value = parse_widget_option(entry[split + 1..].trim())?;
            Some((key, value))
        })
        .collect()
}

fn top_level_ranges(text: &str, delimiter: char) -> Vec<(usize, usize)> {
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

fn top_level_split(text: &str, delimiter: char) -> Option<usize> {
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

fn normalize_option_key(key: &str) -> Option<String> {
    if let Some(unquoted) = unquote_string(key) {
        return Some(unquoted);
    }
    key.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
        .then(|| key.to_string())
}

fn parse_widget_option(value: &str) -> Option<WidgetOption> {
    if let Some(unquoted) = unquote_string(value) {
        return Some(WidgetOption::String(unquoted));
    }
    match value {
        "true" => Some(WidgetOption::Bool(true)),
        "false" => Some(WidgetOption::Bool(false)),
        _ => value.parse::<f64>().ok().map(WidgetOption::Number),
    }
}

fn unquote_string(value: &str) -> Option<String> {
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    value
        .strip_prefix(quote)
        .and_then(|v| v.strip_suffix(quote))
        .map(|v| v.replace("\\\"", "\"").replace("\\'", "'"))
}

const VISUAL_WIDGET_METHODS: &[&str] = &[
    "_pianoroll",
    "_punchcard",
    "_spiral",
    "_scope",
    "_pitchwheel",
    "_spectrum",
    "_wordfall",
];

fn visual_widget_method_at(
    src: &str,
    chars: &[(usize, char)],
    dot: usize,
) -> Option<(&'static str, usize)> {
    if chars.get(dot).map(|(_, c)| *c) != Some('.') {
        return None;
    }
    let method_start = next_byte(chars, dot, src.len());
    let rest = &src[method_start..];
    let method = VISUAL_WIDGET_METHODS
        .iter()
        .copied()
        .find(|method| rest.starts_with(method))?;
    let method_end = method_start + method.len();
    if src[method_end..].chars().next().is_some_and(is_ident_char) {
        return None;
    }
    let mut open = dot + 1 + method.chars().count();
    while open < chars.len() && chars[open].1.is_whitespace() {
        open += 1;
    }
    (chars.get(open).map(|(_, c)| *c) == Some('(')).then_some((method, open))
}

fn is_expression_boundary(c: char) -> bool {
    matches!(
        c,
        ',' | ';' | '=' | ':' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!' | '&' | '|' | '?'
    )
}

fn call_expression_start(src: &str, chars: &[(usize, char)], dot: usize) -> usize {
    let mut i = dot;
    let mut depth = 0i32;
    while i > 0 {
        i -= 1;
        let (byte, c) = chars[i];
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                if depth == 0 {
                    return trim_range(src, next_byte(chars, i, src.len()), chars[dot].0).0;
                }
                depth -= 1;
            }
            _ => {}
        }
        if depth == 0 && is_expression_boundary(c) {
            return trim_range(src, next_byte(chars, i, src.len()), chars[dot].0).0;
        }
        if byte == 0 {
            break;
        }
    }
    trim_range(src, 0, chars[dot].0).0
}

fn widget_id(base_id: &str, widget_type: &str, index: usize, from: usize, to: usize) -> String {
    format!("{base_id}_widget_{widget_type}_{index}_{from}-{to}")
}

fn koto_widget_method(widget_type: &str) -> &'static str {
    match widget_type {
        "_pianoroll" => "rudel_widget_pianoroll",
        "_punchcard" => "rudel_widget_punchcard",
        "_spiral" => "rudel_widget_spiral",
        "_scope" => "rudel_widget_scope",
        "_pitchwheel" => "rudel_widget_pitchwheel",
        "_spectrum" => "rudel_widget_spectrum",
        "_wordfall" => "rudel_widget_wordfall",
        _ => "rudel_widget",
    }
}

fn rewrite_editor_widgets_with_context(
    src: &str,
    node_offset: usize,
    widget_base_id: &str,
) -> (String, Vec<PreprocessWidget>, Vec<(usize, usize)>) {
    const NAME: &str = "slider";
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len());
    let mut widgets: Vec<PreprocessWidget> = Vec::new();
    // `(rewritten_start, original_start)` for each verbatim chunk copied from
    // `src`, so mini-notation offsets recorded against the rewritten output can
    // be mapped back to the original editor source (the widget rewrite changes
    // lengths). Pattern string literals only ever live in these chunks.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '"' || c == '\'' {
            i = skip_string(&chars, i, c);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            i = skip_line_comment(&chars, i);
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            i = skip_block_comment(&chars, i);
            continue;
        }
        if let Some((method, open)) = visual_widget_method_at(src, &chars, i) {
            let local_from = call_expression_start(src, &chars, i);
            let dot_byte = chars[i].0;
            if dot_byte < last {
                i += 1;
                continue;
            }
            let Some(call) = parse_call(src, &chars, open) else {
                i += 1;
                continue;
            };
            let local_to = next_byte(&chars, call.close_char, src.len());
            let from = local_from + node_offset;
            let to = local_to + node_offset;
            let index = widgets
                .iter()
                .filter(|widget| widget.widget_type == method)
                .count();
            let id = widget_id(widget_base_id, method, index, from, to);
            widgets.push(PreprocessWidget {
                widget_type: method.to_string(),
                id: id.clone(),
                from,
                to,
                index,
                options: parse_widget_options(src, call.args.first()),
                ..Default::default()
            });

            let open_byte = chars[open].0;
            let close_byte = chars[call.close_char].0;
            anchors.push((out.len(), last));
            out.push_str(&src[last..dot_byte + 1]);
            out.push_str(koto_widget_method(method));
            out.push('(');
            out.push_str(&format!("{id:?}"));
            let args = src[open_byte + 1..close_byte].trim();
            if !args.is_empty() {
                out.push_str(", ");
                out.push_str(args);
            }
            out.push(')');
            last = local_to;
            i = call.close_char + 1;
            continue;
        }
        if c != 's' || !src[byte..].starts_with(NAME) {
            i += 1;
            continue;
        }
        if i > 0 && is_ident_char(chars[i - 1].1) {
            i += 1;
            continue;
        }
        if previous_non_ws(&chars, i) == Some('.') {
            i += 1;
            continue;
        }
        let name_end = byte + NAME.len();
        if src[name_end..].chars().next().is_some_and(is_ident_char) {
            i += 1;
            continue;
        }
        let mut open = i + NAME.chars().count();
        while open < chars.len() && chars[open].1.is_whitespace() {
            open += 1;
        }
        if chars.get(open).map(|x| x.1) != Some('(') {
            i += 1;
            continue;
        }
        let Some(call) = parse_call(src, &chars, open) else {
            i += 1;
            continue;
        };
        let Some((local_from, local_to)) = call.first_arg else {
            i += 1;
            continue;
        };
        let from = local_from + node_offset;
        let to = local_to + node_offset;
        let id = format!("{from}:{to}");
        let index = widgets
            .iter()
            .filter(|widget| widget.widget_type == "slider")
            .count();
        widgets.push(PreprocessWidget {
            widget_type: "slider".to_string(),
            id: id.clone(),
            from,
            to,
            index,
            options: BTreeMap::new(),
            value: Some(src[local_from..local_to].to_string()),
            min: numeric_arg(src, call.args.get(1)).or(Some(0.0)),
            max: numeric_arg(src, call.args.get(2)).or(Some(1.0)),
            step: numeric_arg(src, call.args.get(3)),
        });

        let open_byte = chars[open].0;
        let close_byte = chars[call.close_char].0;
        let after_close = next_byte(&chars, call.close_char, src.len());
        anchors.push((out.len(), last));
        out.push_str(&src[last..byte]);
        out.push_str("slider_with_id(");
        out.push_str(&format!("{id:?}"));
        let args = src[open_byte + 1..close_byte].trim();
        if !args.is_empty() {
            out.push_str(", ");
            out.push_str(args);
        }
        out.push(')');
        last = after_close;
        i = call.close_char + 1;
    }
    if widgets.is_empty() {
        return (src.to_string(), widgets, Vec::new());
    }
    anchors.push((out.len(), last));
    out.push_str(&src[last..]);
    (out, widgets, anchors)
}

fn strip_line_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut quote = None;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            i += 1;
        } else if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() {
                out.push('\n');
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Rewrite JavaScript arrow functions into Koto lambdas so users can paste
/// Strudel-style callbacks (`x => x.fast(2)`) instead of Koto's `|x| x.fast(2)`.
///
/// Handles the parameter list to the left of `=>` (a bare identifier, a
/// parenthesised list, or `()`), turning it into `|...|` and dropping the `=>`.
/// Expression bodies map cleanly; block bodies (`x => { ... }`) are *not*
/// converted — Koto would read `{ ... }` as a map literal — which mirrors the
/// expression-bodied callbacks Strudel's docs use. String literals are skipped
/// so an `=>` inside a pattern string is left intact.
fn rewrite_arrow_functions(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            i += 1;
            continue;
        }
        // An arrow is the two-char sequence `=>` (never `>=`, which has the
        // opposite order, so comparison operators are untouched).
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
            // Boundary of the parameter list: everything already emitted, minus
            // trailing whitespace between the params and the `=>`.
            let mut end = out.len();
            while end > 0 && out[end - 1].is_whitespace() {
                end -= 1;
            }
            let converted = if end == 0 {
                false
            } else if out[end - 1] == ')' {
                // Parenthesised list: walk back to the matching `(`.
                let mut depth = 0i32;
                let mut open = None;
                let mut k = end - 1;
                loop {
                    match out[k] {
                        ')' => depth += 1,
                        '(' => {
                            depth -= 1;
                            if depth == 0 {
                                open = Some(k);
                                break;
                            }
                        }
                        _ => {}
                    }
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                }
                if let Some(open_idx) = open {
                    out.truncate(end);
                    let last = out.len() - 1;
                    out[last] = '|';
                    out[open_idx] = '|';
                    true
                } else {
                    false
                }
            } else {
                // Bare single identifier parameter.
                let mut k = end;
                while k > 0 {
                    let ch = out[k - 1];
                    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                        k -= 1;
                    } else {
                        break;
                    }
                }
                if k == end {
                    false
                } else {
                    out.truncate(end);
                    out.push('|');
                    out.insert(k, '|');
                    true
                }
            };

            if converted {
                i += 2; // skip `=>`
                // Collapse the whitespace after `=>` to a single space (or none,
                // if the body starts on the next line) for predictable output.
                while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                if i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                    out.push(' ');
                }
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out.iter().collect()
}

fn rewrite_const_declarations(src: &str) -> String {
    src.lines()
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            if let Some(stripped) = rest.strip_prefix("const ") {
                format!("{indent}{stripped}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_string_literal(literal: &str) -> String {
    let Some(quote) = literal.chars().next() else {
        return literal.to_string();
    };
    if quote != '"' && quote != '\'' {
        return literal.to_string();
    }
    let content = &literal[1..literal.len().saturating_sub(1)];
    if !content.contains('{') && !content.contains('}') {
        return literal.to_string();
    }
    let mut hashes = "#".to_string();
    while content.contains(&format!("'{}", hashes)) {
        hashes.push('#');
    }
    format!("r{hashes}'{content}'{hashes}")
}

fn rewrite_string_method_chains(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '"' && c != '\'' {
            out.push(c);
            i += 1;
            continue;
        }

        let start = i;
        let quote = c;
        let mut escaped = false;
        i += 1;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                break;
            }
        }
        let literal = normalize_string_literal(&chars[start..i].iter().collect::<String>());
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() && chars[j] != '\n' {
            j += 1;
        }
        let method_chain =
            j + 1 < chars.len() && chars[j] == '.' && chars[j + 1].is_ascii_alphabetic();
        if method_chain {
            out.push_str("pat(");
            out.push_str(&literal);
            out.push(')');
        } else {
            out.push_str(&literal);
        }
    }
    out
}

fn indent_dot_continuations(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut changed = false;
    let mut at_line_start = true;
    let mut quote = None;
    let mut escaped = false;

    for c in src.chars() {
        if at_line_start && quote.is_none() && c == '.' {
            out.push_str("  ");
            changed = true;
        }

        out.push(c);

        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        }

        at_line_start = c == '\n';
    }

    if changed { out } else { src.to_string() }
}

fn delimiter_delta(line: &str) -> i64 {
    let mut delta = 0;
    let mut quote = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        } else if matches!(c, '(' | '[' | '{') {
            delta += 1;
        } else if matches!(c, ')' | ']' | '}') {
            delta -= 1;
        }
    }
    delta
}

fn label_at_line(line: &str) -> Option<(String, String)> {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let mut end = 0;
    for (i, c) in line.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_' || c == '$'
        } else {
            c.is_ascii_alphanumeric() || c == '_' || c == '$'
        };
        if ok {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let rest = &line[end..];
    rest.strip_prefix(':')
        .map(|expr| (line[..end].to_string(), expr.trim_start().to_string()))
}

fn top_level_boundary(line: &str) -> bool {
    !line.chars().next().is_some_and(char::is_whitespace) && !line.trim_start().starts_with('.')
}

fn sanitize_label(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("anon");
    }
    out
}

fn rewrite_labels(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut labels = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((name, rest)) = label_at_line(lines[i]) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let mut expr_lines = vec![rest];
        let mut depth = delimiter_delta(expr_lines[0].as_str());
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if depth <= 0 {
                if line.trim().is_empty() {
                    i += 1;
                    break;
                }
                if label_at_line(line).is_some() || top_level_boundary(line) {
                    break;
                }
            }
            expr_lines.push(line.to_string());
            depth += delimiter_delta(line);
            i += 1;
        }

        let var = format!("rudel_label_{}_{}", labels.len(), sanitize_label(&name));
        let expr = expr_lines.join("\n").trim().to_string();
        out.push(format!("{var} = rudel_label({name:?}, {expr})"));
        labels.push(var);
    }

    if !labels.is_empty() {
        out.push(format!("stack({})", labels.join(", ")));
    }
    out.join("\n")
}

#[cfg(test)]
pub(crate) fn preprocess_strudel(script: &str) -> String {
    preprocess_strudel_with_meta(script).source
}

pub(crate) fn preprocess_strudel_with_meta(script: &str) -> PreprocessResult {
    preprocess_strudel_with_meta_in_range(script, 0)
}

pub(crate) fn preprocess_strudel_with_meta_in_range(
    script: &str,
    node_offset: usize,
) -> PreprocessResult {
    let (script, widgets, anchors) = rewrite_editor_widgets_with_context(script, node_offset, "");
    let (script, mini_locations) = annotate_mini_offsets(&script, node_offset, &anchors);
    let script = strip_line_comments(&script);
    let script = rewrite_arrow_functions(&script);
    let script = rewrite_const_declarations(&script);
    let script = rewrite_string_method_chains(&script);
    let script = indent_dot_continuations(&script);
    let script = rewrite_labels(&script);
    // Mirror the transpiler's empty-body fallback: an empty (or fully
    // commented-out) script evaluates to silence rather than erroring.
    let source = if script.trim().is_empty() {
        "silence()".to_string()
    } else {
        script
    };
    PreprocessResult {
        source,
        meta: PreprocessMeta {
            mini_locations,
            widgets,
        },
    }
}
