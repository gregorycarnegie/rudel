use super::PreprocessWidget;
use super::scanner::{
    is_ident_char, next_byte, parse_call, previous_non_ws, skip_block_comment, skip_line_comment,
    skip_string, top_level_ranges, top_level_split, trim_range,
};
use crate::WidgetOption;
use std::collections::BTreeMap;

pub(super) const VISUAL_WIDGET_METHODS: &[&str] = &[
    "_pianoroll",
    "_punchcard",
    "_spiral",
    "_scope",
    "_pitchwheel",
    "_spectrum",
    "_wordfall",
];

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

pub(super) fn rewrite_editor_widgets_with_context(
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
