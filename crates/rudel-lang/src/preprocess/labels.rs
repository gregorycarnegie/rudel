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
    // Indentation means nothing to JavaScript: ` $: s("bd")` is a label too.
    // Outside every bracket (the callers' condition) no other statement
    // starts with a name and a colon — a map key needs its braces.
    let line = line.trim_start();
    let mut end = 0;
    for (i, c) in line.char_indices() {
        // JavaScript identifiers are Unicode, and tunes label their parts in
        // whatever language they are written in (`節奏:`, `armonía:`).
        // `sanitize_label` still gives the variable an ASCII name.
        let ok = if i == 0 {
            c.is_alphabetic() || c == '_' || c == '$'
        } else {
            c.is_alphanumeric() || c == '_' || c == '$'
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
    // JavaScript allows a gap before the colon of a labelled statement, and
    // `sd : s("bd")` is written that way often enough to matter. Nothing else
    // at column zero reaches a colon through spaces alone: a ternary hits its
    // `?` first, and a map key at column zero is excluded by the caller.
    let rest = line[end..].trim_start_matches([' ', '\t']);
    rest.strip_prefix(':')
        .map(|expr| (line[..end].to_string(), expr.trim_start().to_string()))
}

/// Whether `next` carries on the statement `prev` ended, outside every
/// bracket. JavaScript's rule, near enough: a line break ends a statement
/// unless one side of it cannot stand alone — the line before ends on an
/// operator, or the line after starts with one (`.fast(2)`, `+ 1`, `? a`).
/// Indentation plays no part.
fn continues(prev: &str, next: &str) -> bool {
    const OPERATORS: [char; 16] = [
        '.', ',', '+', '-', '*', '/', '%', '&', '|', '=', '<', '>', '?', ':', '!', '^',
    ];
    let next = next.trim_start();
    let starts =
        next.starts_with(OPERATORS) && !next.starts_with('!') || next.starts_with(['(', '[', '`']);
    let ends = prev.trim_end().ends_with(OPERATORS) || prev.trim_end().ends_with(['(', '[', '{']);
    starts || ends
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

/// Put a label that follows a `;` on its own line: `…;$: n("0")` is two
/// statements, and the pass below finds labels only where a line starts.
/// Only outside every bracket, where no map key can be.
fn break_before_labels(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut depth = 0i64;
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        for (i, c) in text.char_indices() {
            out.push(c);
            depth += match c {
                '(' | '[' | '{' => 1,
                ')' | ']' | '}' => -1,
                _ => 0,
            };
            if c != ';' || depth > 0 {
                continue;
            }
            let line = src[start + i + 1..].split('\n').next().unwrap_or("");
            if !line.trim().is_empty() && label_at_line(line).is_some() {
                out.push('\n');
            }
        }
    }
    out
}

pub(super) fn rewrite_labels(src: &str) -> String {
    let src = &break_before_labels(src);
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

        // `$:` alone on its line labels the statement on the next one, as a
        // JavaScript label does, so the expression has not started yet.
        let mut started = !rest.trim().is_empty();
        let mut expr_lines = vec![rest];
        let mut depth = delta(i);
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if !started {
                started = !line.trim().is_empty();
                expr_lines.push(line.to_string());
                depth += delta(i);
                i += 1;
                continue;
            }
            if depth <= 0 && !in_string(i) {
                let last = expr_lines
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map_or("", String::as_str);
                if line.trim().is_empty() {
                    // A blank line ends the label's expression — unless the
                    // chain merely has a gap in it and picks up again with a
                    // leading dot. A comment inside a chain arrives here as a
                    // blank, because `strip_comments` blanks rather than
                    // deletes so error messages still point at the line the
                    // user wrote.
                    let resumes = lines[i + 1..]
                        .iter()
                        .find(|l| !l.trim().is_empty())
                        .is_some_and(|next| continues(last, next));
                    if !resumes {
                        i += 1;
                        break;
                    }
                    expr_lines.push(line.to_string());
                    i += 1;
                    continue;
                }
                if label_at_line(line).is_some() || !continues(last, line) {
                    break;
                }
            }
            expr_lines.push(line.to_string());
            depth += delta(i);
            i += 1;
        }

        open = depth.max(0);
        let var = format!("rudel_label_{}_{}", labels.len(), sanitize_label(&name));
        // A statement's own `;` would land inside the call it is wrapped in.
        let expr = expr_lines.join("\n");
        let expr = expr.trim().trim_end_matches(';').trim_end();
        out.push(format!("const {var} = rudel_label({name:?}, {expr})"));
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
            "const rudel_label_0_a = rudel_label(\"a\", s(\"bd\"))\nstack(rudel_label_0_a)"
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
        // Indentation does not matter: an indented label is a label, and it
        // ends the label before it rather than joining its expression.
        let indented = rewrite_labels("a: s(\"bd\")\n  b: s(\"sd\")");
        assert!(
            indented.contains("rudel_label(\"a\", s(\"bd\"))"),
            "{indented}"
        );
        assert!(
            indented.contains("rudel_label(\"b\", s(\"sd\"))"),
            "{indented}"
        );
        // Inside brackets a `name:` line is still a key, however indented.
        let keyed = "f({\n  a: 1\n})";
        assert_eq!(rewrite_labels(keyed), keyed);
        // A colon with no name before it is a map key, not a label.
        let keyed = r#": s("bd")"#;
        assert_eq!(rewrite_labels(keyed), keyed);
    }

    #[test]
    fn a_label_after_a_semicolon_starts_its_own_statement() {
        // Minified one-liners put every `$:` on the first line.
        let two = rewrite_labels(r#"$:s("bd");$:s("sd");"#);
        assert!(two.contains(r#"rudel_label("$", s("bd"))"#), "{two}");
        assert!(two.contains(r#"rudel_label("$", s("sd"))"#), "{two}");
        // Not inside brackets, where `;x:` cannot be a label, nor after a `;`
        // that ends a statement which is no label.
        let braced = "f(() => {a();b: 1})";
        assert_eq!(rewrite_labels(braced), braced);
        let ternary = "x();a ? b : c";
        assert_eq!(rewrite_labels(ternary), ternary);
    }

    #[test]
    fn a_gap_before_the_colon_still_labels() {
        // JavaScript allows it and strudel.cc patterns are written that way.
        assert!(rewrite_labels("sd : s(\"bd\")").contains("rudel_label(\"sd\""));
        assert!(rewrite_labels("$ : s(\"bd\")").contains("rudel_label(\"$\""));
        assert!(rewrite_labels("sd\t: s(\"bd\")").contains("rudel_label(\"sd\""));
        // A ternary at column zero reaches its `?` first and is not a label.
        let ternary = "a ? b : c";
        assert_eq!(rewrite_labels(ternary), ternary);
    }

    #[test]
    fn a_labelled_chain_survives_a_gap_in_it() {
        // A comment inside a chain arrives here as a blank line, because
        // `strip_comments` blanks rather than deletes so error messages still
        // point at the line the user wrote. The chain continues below it.
        let out = rewrite_labels("p1: s(\"bd\")\n  .fast(2)\n\n  .rev()");
        assert!(out.contains(".rev()"), "the chain keeps its tail: {out}");
        assert_eq!(out.matches("rudel_label(").count(), 1, "{out}");
        // A blank line *not* followed by a continuation still ends the label.
        let two = rewrite_labels("a: s(\"bd\")\n\nb: s(\"hh\")");
        assert_eq!(two.matches("rudel_label(").count(), 2, "{two}");
    }

    #[test]
    fn the_generated_variable_name_carries_the_label_through_sanitising() {
        // Only characters outside ASCII identifiers are replaced, and the
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
        // An unindented `.fast(2)` is still part of the chain above, as
        // JavaScript reads it.
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
        assert!(out.contains("s(\"bd\"))\nconst rudel_label_1_b"), "{out}");
    }

    #[test]
    fn a_label_alone_on_its_line_labels_the_next_statement() {
        let out = rewrite_labels("$:\n\ns(\"bd\")\n.fast(2)\nplain");
        assert!(
            out.contains("rudel_label(\"$\", s(\"bd\")\n.fast(2))"),
            "{out}"
        );
        assert!(out.contains("\nplain\n"), "{out}");
    }

    #[test]
    fn a_labels_own_semicolon_stays_outside_the_call() {
        assert_eq!(
            rewrite_labels("$: s(\"bd\").midi();"),
            "const rudel_label_0__ = rudel_label(\"$\", s(\"bd\").midi())\nstack(rudel_label_0__)"
        );
        // Only the one ending the statement: a `;` inside the expression is
        // the script's.
        assert!(rewrite_labels("a: f(() => { x(); y() });").contains("{ x(); y() })"));
    }

    #[test]
    fn a_blank_line_ends_a_label_and_is_consumed_with_it() {
        // The blank line is the terminator, so it does not survive into the
        // output as a stray empty statement.
        let out = rewrite_labels("a: s(\"bd\")\n\nplain");
        assert_eq!(
            out,
            "const rudel_label_0_a = rudel_label(\"a\", s(\"bd\"))\nplain\nstack(rudel_label_0_a)"
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
    fn a_statement_runs_on_only_across_an_operator() {
        // A new statement, however it is indented.
        assert!(!continues("s(\"bd\")", "s(\"sd\")"));
        assert!(!continues("s(\"bd\")", "    n(\"0 1\")"));
        // A leading dot chains onto the line above, indented or not...
        assert!(continues("s(\"bd\")", ".fast(2)"));
        assert!(continues("s(\"bd\")", "  .fast(2)"));
        // ...as does any other operator, on either side of the break.
        assert!(continues("s(\"bd\")", "  + 1"));
        assert!(continues("x =", "  1"));
        assert!(continues("f(a,", "b)"));
        // `!` starts a new statement rather than continuing one.
        assert!(!continues("s(\"bd\")", "!x"));
    }

    #[test]
    fn an_indented_statement_after_a_label_is_not_part_of_it() {
        // `f()` then an indented `n(...)` is two statements to JavaScript.
        let out = rewrite_labels("$: s(\"bd\").delay(\".5\")\n  n(\"0 1\")");
        assert!(
            out.contains("s(\"bd\").delay(\".5\"))\n  n(\"0 1\")"),
            "{out}"
        );
    }
}
