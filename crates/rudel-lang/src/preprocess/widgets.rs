use super::scanner::{
    classify, is_ident_char, parse_call, previous_non_ws, top_level_ranges, top_level_split,
    trim_range,
};
use crate::{WidgetConfig, WidgetOption};
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

/// The widget method called at the `.` at byte `dot`, with the byte offset of
/// its opening parenthesis.
fn visual_widget_method_at(src: &str, dot: usize) -> Option<(&'static str, usize)> {
    if !src[dot..].starts_with('.') {
        return None;
    }
    let method_start = dot + 1;
    let rest = &src[method_start..];
    let method = VISUAL_WIDGET_METHODS
        .iter()
        .copied()
        .find(|method| rest.starts_with(method))?;
    let method_end = method_start + method.len();
    if src[method_end..].chars().next().is_some_and(is_ident_char) {
        return None;
    }
    let open = method_end + (src.len() - method_end - src[method_end..].trim_start().len());
    (src[open..].starts_with('(')).then_some((method, open))
}

fn is_expression_boundary(c: char) -> bool {
    matches!(
        c,
        ',' | ';' | '=' | ':' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!' | '&' | '|' | '?'
    )
}

fn call_expression_start(src: &str, dot: usize) -> usize {
    let mut depth = 0i32;
    for (byte, c) in src[..dot].char_indices().rev() {
        let after = byte + c.len_utf8();
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                if depth == 0 {
                    return trim_range(src, after, dot).0;
                }
                depth -= 1;
            }
            _ => {}
        }
        if depth == 0 && is_expression_boundary(c) {
            return trim_range(src, after, dot).0;
        }
        // A newline ends the expression *unless* the line we are standing in is
        // a dot-continuation of the one above (`note("c")` / newline /
        // `.fast(2)._spiral()`), which `indent_dot_continuations` exists to
        // support. Strudel gets this for free — it reads `node.start` off a
        // parsed CallExpression — so this is the one rule a backwards scan has
        // to encode to reach the same answer.
        if depth == 0 && c == '\n' {
            let line_start = after;
            // The line's own first non-blank character decides it. Note this
            // has to look at the line, not at the span between the line start
            // and `dot`: on a line that *is* `._spiral()` that span is only the
            // indentation, and the dot doing the continuing is `dot` itself.
            if !src[line_start..].trim_start().starts_with('.') {
                return trim_range(src, line_start, dot).0;
            }
        }
    }
    trim_range(src, 0, dot).0
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
) -> (String, Vec<WidgetConfig>, Vec<(usize, usize)>) {
    const NAME: &str = "slider";
    let mut out = String::with_capacity(src.len());
    let mut widgets: Vec<WidgetConfig> = Vec::new();
    // `(rewritten_start, original_start)` for each verbatim chunk copied from
    // `src`, so mini-notation offsets recorded against the rewritten output can
    // be mapped back to the original editor source (the widget rewrite changes
    // lengths). Pattern string literals only ever live in these chunks.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < src.len() {
        // Strings and comments are text, not calls: a widget named in one is not
        // a widget.
        if let Some((_, end)) = classify(src, i) {
            i = end;
            continue;
        }
        if let Some((method, open)) = visual_widget_method_at(src, i) {
            let local_from = call_expression_start(src, i);
            let Some(call) = parse_call(src, open) else {
                i += 1;
                continue;
            };
            let local_to = call.close + 1;
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
            widgets.push(WidgetConfig {
                widget_type: widget_type.to_string(),
                id: id.clone(),
                from,
                to,
                index,
                options: parse_widget_options(src, call.args.first()),
                ..Default::default()
            });

            anchors.push((out.len(), last));
            out.push_str(&src[last..i + 1]);
            out.push_str(koto_widget_method(widget_type));
            out.push('(');
            out.push_str(&format!("{id:?}"));
            let args = src[open + 1..call.close].trim();
            if !args.is_empty() {
                out.push_str(", ");
                out.push_str(args);
            }
            out.push(')');
            last = local_to;
            i = call.close + 1;
            continue;
        }
        if !src[i..].starts_with(NAME) {
            i += 1;
            continue;
        }
        // A standalone `slider`: neither part of a longer identifier nor a
        // method on something else.
        if src[..i].chars().next_back().is_some_and(is_ident_char) {
            i += 1;
            continue;
        }
        if previous_non_ws(src, i) == Some('.') {
            i += 1;
            continue;
        }
        let name_end = i + NAME.len();
        if src[name_end..].chars().next().is_some_and(is_ident_char) {
            i += 1;
            continue;
        }
        let open = src.len() - src[name_end..].trim_start().len();
        if !src[open..].starts_with('(') {
            i += 1;
            continue;
        }
        let Some(call) = parse_call(src, open) else {
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
        widgets.push(WidgetConfig {
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

        anchors.push((out.len(), last));
        out.push_str(&src[last..i]);
        out.push_str("slider_with_id(");
        out.push_str(&format!("{id:?}"));
        let args = src[open + 1..call.close].trim();
        if !args.is_empty() {
            out.push_str(", ");
            out.push_str(args);
        }
        out.push(')');
        last = call.close + 1;
        i = call.close + 1;
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

    fn rewrite(src: &str) -> (String, Vec<WidgetConfig>) {
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

        // ...and each `from` is its own expression's start: a newline ends the
        // expression, so the second widget does not reach back over the first
        // line. Strudel gets this from `node.start` on a parsed CallExpression;
        // the scan here has to encode the rule.
        assert_eq!(
            &src[widgets[0].from..widgets[0].to],
            r#"note("a")._spiral()"#
        );
        assert_eq!(
            &src[widgets[1].from..widgets[1].to],
            r#"note("b")._pitchwheel()"#
        );
        assert!(
            widgets[0].to <= widgets[1].from,
            "the ranges should not overlap"
        );
    }

    #[test]
    fn a_chain_continued_on_the_next_line_stays_one_expression() {
        // The exception the newline rule has to keep: a leading dot continues
        // the expression above, so the range covers both lines.
        let src = "note(\"c\")
  .fast(2)
  ._spiral()";
        let (_, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        assert_eq!(widgets.len(), 1);
        assert_eq!(
            &src[widgets[0].from..widgets[0].to],
            src,
            "a dot-continued chain is one expression"
        );

        // And a statement after such a chain still starts fresh.
        let src = "note(\"c\")
  ._spiral()
note(\"d\")._pitchwheel()";
        let (_, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        assert_eq!(widgets.len(), 2);
        assert_eq!(
            &src[widgets[1].from..widgets[1].to],
            r#"note("d")._pitchwheel()"#
        );
    }

    #[test]
    fn chained_widgets_each_register_from_where_the_last_one_ended() {
        // Scanning resumes at the byte after the previous widget's `)`, so a
        // second widget hanging directly off the first is still seen.
        let src = r#"note("c")._spiral()._pitchwheel()"#;
        let (_, widgets) = rewrite(src);
        assert_eq!(widgets.len(), 2, "both widgets in the chain register");
        assert_eq!(widgets[0].widget_type, "_spiral");
        assert_eq!(widgets[1].widget_type, "_pitchwheel");
        // Three in a row, to pin that it is not just the second one.
        let (_, three) = rewrite(r#"note("c")._spiral()._pitchwheel()._scope()"#);
        assert_eq!(three.len(), 3);
    }

    #[test]
    fn an_unclosed_widget_call_is_left_alone_rather_than_hanging() {
        // `parse_call` returns nothing for these, and the scan has to keep
        // moving or the preprocessor never returns.
        for src in [
            r#"note("c")._spiral("#,
            r#"note("c")._spiral(1, 2"#,
            "slider(0.5",
            "slider(",
        ] {
            let (out, widgets) = rewrite(src);
            assert!(widgets.is_empty(), "no widget from {src:?}");
            assert_eq!(out, src, "source unchanged for {src:?}");
        }
    }

    #[test]
    fn each_widget_type_is_indexed_separately_from_zero() {
        // `index` is what the host uses to tell two surfaces of the same kind
        // apart; counting the wrong ones collapses them onto each other.
        let src = "stack(slider(0.1), slider(0.2))";
        let found = sliders(src);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].index, 0);
        assert_eq!(found[1].index, 1, "the second slider is index 1");

        // A different widget type in between does not advance the count.
        let src = r#"stack(slider(0.1), note("c")._spiral(), slider(0.2))"#;
        let (_, all) = rewrite(src);
        let slider_indices: Vec<_> = all
            .iter()
            .filter(|w| w.widget_type == "slider")
            .map(|w| w.index)
            .collect();
        assert_eq!(slider_indices, [0, 1], "sliders count only sliders");
        let spiral: Vec<_> = all
            .iter()
            .filter(|w| w.widget_type == "_spiral")
            .map(|w| w.index)
            .collect();
        assert_eq!(spiral, [0], "and the spiral starts its own count");
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

    // `slider(...)` is rewritten by a separate branch from the visual widgets,
    // with its own name-boundary checks. The tests above only ever fed the
    // rewriter `_spiral`/`_pianoroll` calls, so none of it was reached.

    fn sliders(src: &str) -> Vec<WidgetConfig> {
        let (_, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        widgets
            .into_iter()
            .filter(|w| w.widget_type == "slider")
            .collect()
    }

    #[test]
    fn a_slider_call_becomes_a_slider_widget() {
        let found = sliders("slider(0.5, 0, 1)");
        assert_eq!(found.len(), 1, "a bare slider call is a widget");
        assert_eq!(found[0].widget_type, "slider");

        // The range is its *first argument* — the live value the editor
        // rewrites when the slider is dragged, not the whole call.
        let src = "slider(0.5, 0, 1)";
        assert_eq!(&src[found[0].from..found[0].to], "0.5");

        // Whitespace before the parenthesis is still a call.
        assert_eq!(sliders("slider (0.25)").len(), 1);
    }

    #[test]
    fn only_a_standalone_slider_name_counts() {
        // Preceded by an identifier character: a different function.
        assert!(
            sliders("myslider(0.5)").is_empty(),
            "myslider is not slider"
        );
        assert!(sliders("x_slider(0.5)").is_empty());
        // Followed by one: also a different function.
        assert!(sliders("sliders(0.5)").is_empty(), "sliders is not slider");
        assert!(sliders("slider2(0.5)").is_empty());
        // A method call of the same name belongs to its receiver.
        assert!(
            sliders("x.slider(0.5)").is_empty(),
            "a .slider method is not the slider widget"
        );
        // The name with no call at all.
        assert!(sliders("slider").is_empty());
        // No arguments means no value to track.
        assert!(sliders("slider()").is_empty());
    }

    #[test]
    fn several_sliders_get_distinct_ids() {
        let src = "stack(slider(0.1), slider(0.2), slider(0.3))";
        let found = sliders(src);
        assert_eq!(found.len(), 3);
        let ids: Vec<_> = found.iter().map(|w| w.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "slider ids must be distinct: {ids:?}");
        // Each points at its own first argument.
        for (w, want) in found.iter().zip(["0.1", "0.2", "0.3"]) {
            assert_eq!(&src[w.from..w.to], want);
        }
    }

    #[test]
    fn a_slider_inside_a_string_or_comment_is_not_rewritten() {
        // The same guards the visual path has, on the slider branch.
        for src in [
            r#"s("slider(0.5)")"#,
            "// slider(0.5)",
            "/* slider(0.5) */",
            r#"x = 'slider(0.5)'"#,
        ] {
            assert!(
                sliders(src).is_empty(),
                "a quoted or commented slider is not a widget: {src}"
            );
        }
    }

    #[test]
    fn a_widget_call_inside_a_comment_is_skipped_on_both_paths() {
        // The visual-widget branch shares the same guards; a commented-out
        // widget must not register, or the editor grows a surface for code that
        // does not run.
        for src in [r#"// note("c")._spiral()"#, r#"/* note("c")._spiral() */"#] {
            let (_, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
            assert!(
                widgets.is_empty(),
                "a commented-out widget is not a widget: {src}"
            );
        }

        // Live code after a comment mentioning one still registers.
        let src = "// _spiral()\nnote(\"c\")._spiral()";
        let (_, widgets, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        assert_eq!(
            widgets.len(),
            1,
            "the real call after a comment still counts"
        );
    }

    #[test]
    fn the_node_offset_shifts_the_recorded_range() {
        // Block evaluation preprocesses a slice of the document, so the ranges
        // have to be reported against the whole document rather than the slice.
        let src = "slider(0.5)";
        let (_, base, _) = rewrite_editor_widgets_with_context(src, 0, "w");
        let (_, shifted, _) = rewrite_editor_widgets_with_context(src, 100, "w");
        assert_eq!(base.len(), 1);
        assert_eq!(shifted.len(), 1);
        assert_eq!(
            shifted[0].from,
            base[0].from + 100,
            "the offset should move the start"
        );
        assert_eq!(shifted[0].to, base[0].to + 100);
    }
}
