use super::scanner::{skip_block_comment, skip_line_comment, skip_string};

pub(super) fn strip_line_comments(src: &str) -> String {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            let end = skip_block_comment(&chars, i);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.push_str(&src[byte..end_byte]);
            i = end;
            continue;
        }
        if c == '"' || c == '\'' {
            let end = skip_string(&chars, i, c);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.push_str(&src[byte..end_byte]);
            i = end;
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            i = skip_line_comment(&chars, i);
            if chars.get(i).map(|x| x.1) == Some('\n') {
                out.push('\n');
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
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
pub(super) fn rewrite_arrow_functions(src: &str) -> String {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            let end = skip_block_comment(&chars, i);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.extend(src[byte..end_byte].chars());
            i = end;
            continue;
        }
        if c == '"' || c == '\'' {
            let end = skip_string(&chars, i, c);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.extend(src[byte..end_byte].chars());
            i = end;
            continue;
        }
        // An arrow is the two-char sequence `=>` (never `>=`, which has the
        // opposite order, so comparison operators are untouched).
        if c == '=' && chars.get(i + 1).map(|x| x.1) == Some('>') {
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
                while i < chars.len() && (chars[i].1 == ' ' || chars[i].1 == '\t') {
                    i += 1;
                }
                if i < chars.len() && chars[i].1 != '\n' && chars[i].1 != '\r' {
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

pub(super) fn rewrite_const_declarations(src: &str) -> String {
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

pub(super) fn rewrite_string_method_chains(src: &str) -> String {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            let end = skip_block_comment(&chars, i);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.push_str(&src[byte..end_byte]);
            i = end;
            continue;
        }
        if c != '"' && c != '\'' {
            out.push(c);
            i += 1;
            continue;
        }

        let end = skip_string(&chars, i, c);
        let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
        let literal = normalize_string_literal(&src[byte..end_byte]);
        let mut j = end;
        while j < chars.len() && chars[j].1.is_whitespace() && chars[j].1 != '\n' {
            j += 1;
        }
        let method_chain =
            j + 1 < chars.len() && chars[j].1 == '.' && chars[j + 1].1.is_ascii_alphabetic();
        if method_chain {
            out.push_str("pat(");
            out.push_str(&literal);
            out.push(')');
        } else {
            out.push_str(&literal);
        }
        i = end;
    }
    out
}

pub(super) fn indent_dot_continuations(src: &str) -> String {
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
