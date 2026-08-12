use super::scanner::{Chunk, chunks};

/// Per line: how far it opens or closes brackets counting only code, and
/// whether it *begins* inside a string or block comment.
///
/// Scanned over the whole source rather than line by line, because a template
/// literal spans lines: scanning one of its lines alone sees the closing quote
/// as an opening one, and every bracket after it disappears into a string that
/// is not there. That miscount is what let a `$:` label swallow the rest of a
/// file — the depth never came back to zero, so no later line could end it.
fn line_shape(src: &str) -> Vec<(i64, bool)> {
    let mut shape = vec![(0i64, false); src.split('\n').count()];
    let mut line = 0usize;
    for (kind, start, end) in chunks(src) {
        let code = kind == Chunk::Code;
        for c in src[start..end].chars() {
            if c == '\n' {
                line += 1;
                // A chunk that has not ended by this newline continues onto the
                // next line, which is therefore inside it.
                shape[line].1 = !code;
                continue;
            }
            if code {
                shape[line].0 += match c {
                    '(' | '[' | '{' => 1,
                    ')' | ']' | '}' => -1,
                    _ => 0,
                };
            }
        }
    }
    shape
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

pub(super) fn rewrite_labels(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let shape = line_shape(src);
    let delta = |i: usize| shape.get(i).map_or(0, |s| s.0);
    // A line that begins inside a template literal is that literal's text, not
    // a statement: it can neither carry a label nor end one.
    let in_string = |i: usize| shape.get(i).is_some_and(|s| s.1);
    let mut out = Vec::new();
    let mut labels = Vec::new();
    let mut i = 0;
    // Brackets left open by the lines already emitted. A `name:` line inside
    // one is a map key, not a label — `samples({\nbd: [...],\n})` writes its
    // keys at column 0 exactly like a label.
    let mut open = 0i64;
    while i < lines.len() {
        let label = if open == 0 && !in_string(i) {
            label_at_line(lines[i])
        } else {
            None
        };
        let Some((name, rest)) = label else {
            open += delta(i);
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let mut expr_lines = vec![rest];
        let mut depth = delta(i);
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if depth <= 0 && !in_string(i) {
                if line.trim().is_empty() {
                    i += 1;
                    break;
                }
                if label_at_line(line).is_some() || top_level_boundary(line) {
                    break;
                }
            }
            expr_lines.push(line.to_string());
            depth += delta(i);
            i += 1;
        }

        open = depth.max(0);
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
mod tests {
    use super::*;

    // Labels are the last rewrite in the pipeline and had no tests of their
    // own: the whole module was reached only through end-to-end evaluation,
    // which cannot tell "the expression ended here" from "the expression ended
    // one line later" as long as both still parse. That is exactly what the
    // 2026-08 mutation survivors clustered on — `delimiter_delta`'s bracket
    // counting and the boundary rules that decide how far a label reaches.

    #[test]
    fn a_labelled_line_becomes_a_named_pattern_and_a_stack() {
        assert_eq!(
            rewrite_labels(r#"a: s("bd")"#),
            "rudel_label_0_a = rudel_label(\"a\", s(\"bd\"))\nstack(rudel_label_0_a)"
        );
        // Several labels stack in source order, each with its own index.
        let two = rewrite_labels("a: s(\"bd\")\nb: s(\"sd\")");
        assert!(two.contains("rudel_label_0_a = "), "{two}");
        assert!(two.contains("rudel_label_1_b = "), "{two}");
        assert!(
            two.ends_with("stack(rudel_label_0_a, rudel_label_1_b)"),
            "{two}"
        );
        // Source with no labels is passed through and gains no stack.
        assert_eq!(rewrite_labels(r#"s("bd")"#), r#"s("bd")"#);
    }

    #[test]
    fn only_an_identifier_followed_by_a_colon_is_a_label() {
        // Digits are allowed after the first character but not as it.
        assert!(rewrite_labels(r#"a1: s("bd")"#).contains("rudel_label_0_a1"));
        assert_eq!(rewrite_labels(r#"1a: s("bd")"#), r#"1a: s("bd")"#);
        // `_` and `$` may lead.
        assert!(rewrite_labels(r#"_x: s("bd")"#).contains(r#"rudel_label("_x""#));
        assert!(rewrite_labels(r#"$x: s("bd")"#).contains(r#"rudel_label("$x""#));
        // A name broken by punctuation is not a label at all.
        let dashed = r#"my-label: s("bd")"#;
        assert_eq!(rewrite_labels(dashed), dashed);
        // An indented line is a continuation, never a label.
        let indented = "  a: s(\"bd\")";
        assert_eq!(rewrite_labels(indented), indented);
        // A colon with no name before it is a map key, not a label.
        let keyed = r#": s("bd")"#;
        assert_eq!(rewrite_labels(keyed), keyed);
    }

    #[test]
    fn the_generated_variable_name_carries_the_label_through_sanitising() {
        // Only characters Koto rejects in an identifier are replaced, and the
        // rest of the name has to survive — a blanket replacement would collide
        // every label onto the same variable.
        assert!(rewrite_labels(r#"$a: s("bd")"#).contains("rudel_label_0__a = "));
        assert!(rewrite_labels(r#"abc: s("bd")"#).contains("rudel_label_0_abc = "));
        // The quoted name kept for display is the original, not the sanitised
        // one.
        assert!(rewrite_labels(r#"$a: s("bd")"#).contains(r#"rudel_label("$a""#));
    }

    #[test]
    fn an_unclosed_bracket_carries_the_label_across_lines() {
        // `delimiter_delta` is what decides this. Miscount and the closing
        // paren lands outside the call it closes.
        let src = "drums: stack(\n  s(\"bd\"),\n  s(\"sd\")\n)\nmore";
        let out = rewrite_labels(src);
        assert!(
            out.contains("  s(\"sd\")\n))"),
            "the closing paren belongs to the label expression: {out}"
        );
        assert!(
            out.contains("\nmore\n"),
            "the line after it does not: {out}"
        );
    }

    #[test]
    fn a_map_key_inside_an_open_bracket_is_not_a_label() {
        // `samples({\nbd: [...]\n})` writes its keys at column 0, exactly like a
        // label. Reading one as a label splits the map open mid-literal.
        let src = "samples({\nbd: ['bd.wav'],\nsd: ['sd.wav'],\n})\ns(\"bd sd\")";
        assert_eq!(rewrite_labels(src), src);
        // A label after the map closes is still a label.
        let after = rewrite_labels("samples({\nbd: ['bd.wav'],\n})\ndrums: s(\"bd\")");
        assert!(after.contains("rudel_label_0_drums = "), "{after}");
        assert!(after.contains("bd: ['bd.wav'],"), "{after}");
    }

    #[test]
    fn a_bracket_inside_a_string_does_not_open_anything() {
        // The reason `delimiter_delta` walks chunks rather than characters: a
        // paren in a pattern string is text, and counting it swallows the next
        // line into the label.
        let out = rewrite_labels("a: s(\"(\")\nnext");
        assert!(out.contains("s(\"(\"))\nnext"), "{out}");
        // Same for a comment.
        let commented = rewrite_labels("a: s(\"bd\") // (\nnext");
        assert!(commented.contains("\nnext"), "{commented}");
    }

    #[test]
    fn a_dot_continuation_extends_the_label_but_a_new_statement_ends_it() {
        // An unindented `.fast(2)` is still part of the chain above — the same
        // rule `indent_dot_continuations` exists to support.
        let out = rewrite_labels("a: s(\"bd\")\n.fast(2)");
        assert!(
            out.contains(".fast(2))"),
            "the chain is inside the label: {out}"
        );

        // An unindented ordinary statement is not.
        let out = rewrite_labels("a: s(\"bd\")\nplain");
        assert!(out.contains("s(\"bd\"))\nplain"), "{out}");

        // Neither is another label.
        let out = rewrite_labels("a: s(\"bd\")\nb: s(\"sd\")");
        assert!(out.contains("s(\"bd\"))\nrudel_label_1_b"), "{out}");
    }

    #[test]
    fn a_blank_line_ends_a_label_and_is_consumed_with_it() {
        // The blank line is the terminator, so it does not survive into the
        // output as a stray empty statement.
        let out = rewrite_labels("a: s(\"bd\")\n\nplain");
        assert_eq!(
            out,
            "rudel_label_0_a = rudel_label(\"a\", s(\"bd\"))\nplain\nstack(rudel_label_0_a)"
        );
        assert!(!out.contains("\n\n"), "no blank line should remain: {out}");
    }

    #[test]
    fn line_shape_counts_only_code_brackets() {
        let delta = |src: &str| line_shape(src)[0].0;
        assert_eq!(delta("f("), 1);
        assert_eq!(delta("f()"), 0);
        assert_eq!(delta(")"), -1);
        assert_eq!(delta("[{("), 3);
        // Brackets quoted or commented are text.
        assert_eq!(delta(r#"s("([{")"#), 0);
        assert_eq!(delta("f() // ((("), 0);
        assert_eq!(delta("f() /* ((( */"), 0);
        // Nothing at all is a flat line.
        assert_eq!(delta("plain text"), 0);
    }

    #[test]
    fn a_multi_line_string_hides_its_brackets_from_every_line_it_covers() {
        // Counting one line at a time reads the literal's *closing* quote as an
        // opening one, so the `))` after it vanish into a string that is not
        // there and the label runs away with the rest of the file.
        let shape = line_shape("a: n(`<0 1\n  2)))>`)\nb: s(\"bd\")");
        assert_eq!(shape[0].0, 1, "only `n(`; the rest of the line is literal");
        assert_eq!(
            shape[1].0, -1,
            "only the real `)` counts, not the three inside"
        );
        assert!(shape[1].1, "the second line begins inside the literal");
        assert!(!shape[2].1, "the third does not");

        let out = rewrite_labels("a: n(`<0 1\n  2>`)\nb: s(\"bd\")");
        assert!(
            out.contains("rudel_label_1_b = "),
            "the second label survives: {out}"
        );
    }

    #[test]
    fn top_level_boundary_admits_only_an_unindented_non_continuation() {
        assert!(top_level_boundary("s(\"bd\")"));
        // Indented, so a continuation of the line above.
        assert!(!top_level_boundary("  s(\"bd\")"));
        // Leading dot, so a method chained onto the line above...
        assert!(!top_level_boundary(".fast(2)"));
        // ...whether or not it is also indented.
        assert!(!top_level_boundary("  .fast(2)"));
    }
}
