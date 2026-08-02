use super::{
    PreprocessWidget,
    scanner::{
        is_ident_char, next_byte, parse_call, previous_non_ws, skip_block_comment,
        skip_line_comment, skip_string, top_level_ranges, top_level_split, trim_range,
    },
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
    "_claviature",
    "_fscope",
    // Public (non-underscore) visualizer names render the same inline widget as
    // their `_`-prefixed variants. `canonical_widget_type` maps them back to the
    // `_`-prefixed type the painter/host key on. `tscope` is Strudel's alias
    // for `scope` (same painter).
    "pianoroll",
    "punchcard",
    "spiral",
    "scope",
    "tscope",
    "fscope",
    "pitchwheel",
    "spectrum",
    "wordfall",
    "claviature",
];

/// Normalize a matched widget method name to the `_`-prefixed widget type that
/// the native painter (`widgets/visual.rs`) and host key on. Public spellings
/// (`pianoroll`) map to the same type as their inline variant (`_pianoroll`).
fn canonical_widget_type(method: &str) -> &'static str {
    match method {
        "pianoroll" | "_pianoroll" => "_pianoroll",
        "punchcard" | "_punchcard" => "_punchcard",
        "spiral" | "_spiral" => "_spiral",
        "scope" | "tscope" | "_scope" => "_scope",
        "fscope" | "_fscope" => "_fscope",
        "pitchwheel" | "_pitchwheel" => "_pitchwheel",
        "spectrum" | "_spectrum" => "_spectrum",
        "wordfall" | "_wordfall" => "_wordfall",
        "claviature" | "_claviature" => "_claviature",
        _ => "_pianoroll",
    }
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
        "_fscope" => "rudel_widget_fscope",
        "_pitchwheel" => "rudel_widget_pitchwheel",
        "_spectrum" => "rudel_widget_spectrum",
        "_wordfall" => "rudel_widget_wordfall",
        "_claviature" => "rudel_widget_claviature",
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
            // Public spellings (`pianoroll`) share the inline variant's type so
            // both feed the same painter/host and one widget-index counter.
            let widget_type = canonical_widget_type(method);
            let index = widgets
                .iter()
                .filter(|widget| widget.widget_type == widget_type)
                .count();
            let id = widget_id(widget_base_id, widget_type, index, from, to);
            widgets.push(PreprocessWidget {
                widget_type: widget_type.to_string(),
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
            out.push_str(koto_widget_method(widget_type));
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

#[cfg(test)]
mod tests {
    use super::*;

    // The survivors here sat in the small helpers that decide *where* a widget
    // call is — `call_expression_start` walking back to the start of the
    // expression the method hangs off, `visual_widget_method_at` deciding a dot
    // begins one, and the option parsers. The existing tests drive the whole
    // rewrite on well-formed input, which never exercises the boundaries those
    // helpers are made of.

    fn rewrite(src: &str) -> (String, Vec<PreprocessWidget>) {
        let (out, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        (out, widgets)
    }

    #[test]
    fn public_and_underscored_widget_spellings_share_a_type() {
        // `pianoroll` and `_pianoroll` are the same widget; the painter keys on
        // the underscored form, so both have to land there.
        for (public, inline) in [
            ("pianoroll", "_pianoroll"),
            ("punchcard", "_punchcard"),
            ("spiral", "_spiral"),
            ("fscope", "_fscope"),
            ("pitchwheel", "_pitchwheel"),
            ("spectrum", "_spectrum"),
            ("wordfall", "_wordfall"),
            ("claviature", "_claviature"),
        ] {
            assert_eq!(canonical_widget_type(public), inline, "{public}");
            assert_eq!(canonical_widget_type(inline), inline, "{inline}");
        }
        // `scope` has a third spelling.
        for spelling in ["scope", "tscope", "_scope"] {
            assert_eq!(canonical_widget_type(spelling), "_scope", "{spelling}");
        }
    }

    #[test]
    fn unquote_string_only_unwraps_a_matched_pair() {
        assert_eq!(unquote_string(r#""abc""#).as_deref(), Some("abc"));
        assert_eq!(unquote_string("'abc'").as_deref(), Some("abc"));
        // Escaped quotes inside come back unescaped.
        assert_eq!(unquote_string(r#""a\"b""#).as_deref(), Some(r#"a"b"#));
        // Unquoted, half-quoted and empty input are not strings.
        assert_eq!(unquote_string("abc"), None);
        assert_eq!(unquote_string(r#""abc"#), None);
        assert_eq!(unquote_string(""), None);
        // A bare pair of quotes is an empty string, not `None`.
        assert_eq!(unquote_string(r#""""#).as_deref(), Some(""));
    }

    #[test]
    fn widget_option_values_keep_their_types() {
        assert!(matches!(
            parse_widget_option("true"),
            Some(WidgetOption::Bool(true))
        ));
        assert!(matches!(
            parse_widget_option("false"),
            Some(WidgetOption::Bool(false))
        ));
        assert!(matches!(
            parse_widget_option("2"),
            Some(WidgetOption::Number(n)) if (n - 2.0).abs() < 1e-9
        ));
        assert!(matches!(
            parse_widget_option("-0.5"),
            Some(WidgetOption::Number(n)) if (n + 0.5).abs() < 1e-9
        ));
        assert!(matches!(
            parse_widget_option(r#""hi""#),
            Some(WidgetOption::String(ref s)) if s == "hi"
        ));
        // A quoted number stays a string, not a number.
        assert!(matches!(
            parse_widget_option(r#""2""#),
            Some(WidgetOption::String(ref s)) if s == "2"
        ));
        // Anything else is dropped rather than guessed at.
        assert!(parse_widget_option("someIdent").is_none());
    }

    #[test]
    fn option_keys_accept_identifiers_and_quoted_names_only() {
        assert_eq!(normalize_option_key("fold").as_deref(), Some("fold"));
        assert_eq!(normalize_option_key("fold_2").as_deref(), Some("fold_2"));
        assert_eq!(normalize_option_key("$id").as_deref(), Some("$id"));
        assert_eq!(normalize_option_key(r#""fold""#).as_deref(), Some("fold"));
        // A key with punctuation in it is not a key.
        assert_eq!(normalize_option_key("fold-2"), None);
        assert_eq!(normalize_option_key("a b"), None);
    }

    #[test]
    fn a_widget_method_is_only_matched_as_a_whole_call() {
        // `.pianoroll(` is a widget; `.pianorollish(` is a different method
        // that merely starts the same way, and a name with no call after it is
        // not a widget either.
        let (out, widgets) = rewrite(r#"note("c").pianoroll()"#);
        assert_eq!(widgets.len(), 1, "the plain call is a widget: {out}");

        let (_, none) = rewrite(r#"note("c").pianorollish()"#);
        assert!(none.is_empty(), "a longer method name is not the widget");

        let (_, none) = rewrite(r#"note("c").pianoroll"#);
        assert!(none.is_empty(), "a widget name with no call is not one");

        // Whitespace between the name and its parenthesis is still a call.
        let (_, spaced) = rewrite("note(\"c\").pianoroll ()");
        assert_eq!(spaced.len(), 1, "whitespace before the paren is allowed");
    }

    #[test]
    fn the_widget_source_range_covers_the_whole_chained_expression() {
        // `call_expression_start` walks back past the chain so the editor
        // highlights the expression the widget belongs to, not just the method.
        let src = r#"note("c e g").fast(2)._spiral()"#;
        let (_, widgets) = rewrite(src);
        let w = &widgets[0];
        assert_eq!(
            &src[w.from..w.to],
            src,
            "the range should span the whole chain"
        );

        // Inside an argument list it stops at the comma, not at the start of
        // the outer call.
        let src = r#"stack(note("a"), note("b")._spiral())"#;
        let (_, widgets) = rewrite(src);
        let w = &widgets[0];
        assert_eq!(
            &src[w.from..w.to],
            r#"note("b")._spiral()"#,
            "a comma bounds the expression"
        );

        // An operator bounds it too.
        let src = r#"x = note("a")._spiral()"#;
        let (_, widgets) = rewrite(src);
        let w = &widgets[0];
        assert_eq!(&src[w.from..w.to], r#"note("a")._spiral()"#);

        // An opening bracket bounds it.
        let src = r#"[note("a")._spiral()]"#;
        let (_, widgets) = rewrite(src);
        let w = &widgets[0];
        assert_eq!(&src[w.from..w.to], r#"note("a")._spiral()"#);
    }

    #[test]
    fn widget_ids_are_unique_and_carry_the_source_span() {
        // Two widgets in one script must not collide, or the host reuses one
        // surface for both.
        let src = r#"note("a")._spiral()
note("b")._pitchwheel()"#;
        let (_, widgets) = rewrite(src);
        assert_eq!(widgets.len(), 2);
        assert_ne!(widgets[0].id, widgets[1].id, "ids must be distinct");
        assert_eq!(widgets[0].widget_type, "_spiral");
        assert_eq!(widgets[1].widget_type, "_pitchwheel");
        // Each `to` is its own expression's end...
        assert_eq!(&src[..widgets[0].to], r#"note("a")._spiral()"#);
        assert_eq!(widgets[1].to, src.len());

        // ...but the second widget's `from` reaches back over the first line.
        // A newline is deliberately *not* an expression boundary, because a
        // Koto chain continues across lines with a leading dot — which is what
        // `indent_dot_continuations` exists for — so there is no cheap way to
        // tell "next statement" from "continued chain" here. Placement keys on
        // `to`, so this does not move a widget; it does mean `from` spans more
        // than the widget's own expression on any line but the first. Recorded
        // in todo.md; pinned so a change to it is deliberate.
        assert_eq!(widgets[0].from, 0);
        assert_eq!(
            widgets[1].from, 0,
            "the second widget's start reaches back past the newline"
        );
    }

    #[test]
    fn a_widget_name_inside_a_string_is_not_rewritten() {
        // The rewriter walks the source; a pattern string mentioning a widget
        // method must be copied through untouched.
        let src = r#"s("bd")._spiral()"#;
        let (out, widgets) = rewrite(src);
        assert_eq!(widgets.len(), 1);
        assert!(
            out.contains(r#""bd""#),
            "the pattern string survives: {out}"
        );

        let quoted = r#"s("._spiral()")"#;
        let (out, none) = rewrite(quoted);
        assert!(
            none.is_empty(),
            "a widget call inside a string is not a widget: {out}"
        );
        assert_eq!(out, quoted, "and the source is unchanged");
    }
}
