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

/// Rewrite JavaScript's leading-dot decimal literals (`.5`, `-.25`) into the
/// `0.`-prefixed form Koto requires, so Strudel snippets paste unchanged.
///
/// A dot starts a number only when what precedes it cannot be a value: after an
/// operator, an opening bracket, a comma, or the start of the source. A dot
/// following an identifier, a number, `)`, `]`, or a string is method access
/// (`pat.fast`, `1.5`, `f(x).gain`) and is left alone. String literals and
/// comments are skipped.
pub(super) fn rewrite_leading_dot_numbers(src: &str) -> String {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    // The last emitted character that is not whitespace, which decides whether a
    // `.` continues an expression or begins one.
    let mut prev: Option<char> = None;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            let end = skip_block_comment(&chars, i);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.push_str(&src[byte..end_byte]);
            i = end;
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            let end = skip_line_comment(&chars, i);
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
            prev = Some(c);
            continue;
        }
        if c == '.'
            && chars
                .get(i + 1)
                .map(|x| x.1)
                .is_some_and(|d| d.is_ascii_digit())
            && !prev
                .is_some_and(|p| p.is_alphanumeric() || matches!(p, '_' | '$' | ')' | ']' | '.'))
        {
            out.push('0');
        }
        out.push(c);
        if !c.is_whitespace() {
            prev = Some(c);
        }
        i += 1;
    }
    out
}

/// Strip JavaScript `await`. Strudel's async helpers (`samples`, `midin`,
/// `loadSoundfont`) return promises the browser REPL awaits; Rudel's equivalents
/// are synchronous host effects, so the keyword is simply dropped — the same
/// reasoning as the unported `plugin-sample.mjs` `await`-injection pass, run in
/// reverse. String literals and comments are skipped.
pub(super) fn strip_await(src: &str) -> String {
    if !src.contains("await") {
        return src.to_string();
    }
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    // Whether the previous emitted non-whitespace char could end an identifier,
    // so `myawait` / `x.await` are not touched.
    let mut prev: Option<char> = None;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('*') {
            let end = skip_block_comment(&chars, i);
            let end_byte = chars.get(end).map(|x| x.0).unwrap_or(src.len());
            out.push_str(&src[byte..end_byte]);
            i = end;
            continue;
        }
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            let end = skip_line_comment(&chars, i);
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
            prev = Some(c);
            continue;
        }
        let is_word_boundary =
            !prev.is_some_and(|p| p.is_alphanumeric() || p == '_' || p == '$' || p == '.');
        if c == 'a' && is_word_boundary && src[byte..].starts_with("await") {
            let after = chars.get(i + 5).map(|x| x.1);
            if after.is_none_or(|a| a.is_whitespace() || a == '(') {
                // Drop the keyword and the whitespace that separated it from
                // the expression it wrapped.
                i += 5;
                while chars.get(i).is_some_and(|x| x.1 == ' ' || x.1 == '\t') {
                    i += 1;
                }
                continue;
            }
        }
        out.push(c);
        if !c.is_whitespace() {
            prev = Some(c);
        }
        i += 1;
    }
    out
}

/// Rewrite JavaScript's strict equality operators (`===`, `!==`) into the
/// two-character forms Koto uses. Strudel's docs use them freely in callbacks
/// (`hap => hap.value.s === 'hh'`); Koto's `==`/`!=` are already strict, so the
/// rewrite is behaviour-preserving. String literals and comments are skipped.
pub(super) fn rewrite_strict_equality(src: &str) -> String {
    if !src.contains("===") && !src.contains("!==") {
        return src.to_string();
    }
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
        if c == '/' && chars.get(i + 1).map(|x| x.1) == Some('/') {
            let end = skip_line_comment(&chars, i);
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
        let three = |a: char, b: char, d: char| {
            c == a
                && chars.get(i + 1).map(|x| x.1) == Some(b)
                && chars.get(i + 2).map(|x| x.1) == Some(d)
        };
        if three('=', '=', '=') || three('!', '=', '=') {
            out.push(c);
            out.push('=');
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}
