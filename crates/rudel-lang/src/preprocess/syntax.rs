use super::scanner::{Chunk, chunks};

/// Drop `//` comments, keeping the newline each sat on so line numbers hold.
/// Strings and block comments are chunks of their own, so a `//` inside one is
/// content and survives.
pub(super) fn strip_line_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        if kind != Chunk::LineComment {
            out.push_str(&src[start..end]);
        }
    }
    out
}

/// Whether the arrow at byte `arrow` has a brace-block body (`x => { ... }`).
///
/// Those are left alone. Koto's blocks are indentation-based, so `{ ... }` there
/// is a map literal, and converting produces `|x| { ... }` — which Koto rejects
/// with "expected '}' at end of map declaration", naming a map the user never
/// wrote. Leaving the `=>` in place fails on the construct they actually typed
/// instead. (Strudel needs no equivalent: block-bodied arrows are native JS
/// there, though without a `return` they still evaluate to `undefined`.)
///
/// Converting them properly would mean rewriting the braced block into Koto's
/// indented form, which is a different job from this one.
fn body_is_a_block(src: &str, arrow: usize) -> bool {
    src[arrow + 2..].trim_start().starts_with('{')
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
    let mut out: Vec<char> = Vec::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.extend(text.chars());
            continue;
        }
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (byte, c) = chars[i];
            // An arrow is the two-char sequence `=>` (never `>=`, which has the
            // opposite order, so comparison operators are untouched).
            if c == '='
                && chars.get(i + 1).map(|x| x.1) == Some('>')
                && !body_is_a_block(src, start + byte)
            {
                // Boundary of the parameter list: everything already emitted,
                // minus trailing whitespace between the params and the `=>`.
                let mut param_end = out.len();
                while param_end > 0 && out[param_end - 1].is_whitespace() {
                    param_end -= 1;
                }
                let converted = if param_end == 0 {
                    false
                } else if out[param_end - 1] == ')' {
                    // Parenthesised list: walk back to the matching `(`.
                    let mut depth = 0i32;
                    let mut open = None;
                    let mut k = param_end - 1;
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
                        out.truncate(param_end);
                        let last = out.len() - 1;
                        out[last] = '|';
                        out[open_idx] = '|';
                        true
                    } else {
                        false
                    }
                } else {
                    // Bare single identifier parameter.
                    let mut k = param_end;
                    while k > 0 {
                        let ch = out[k - 1];
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                            k -= 1;
                        } else {
                            break;
                        }
                    }
                    if k == param_end {
                        false
                    } else {
                        out.truncate(param_end);
                        out.push('|');
                        out.insert(k, '|');
                        true
                    }
                };

                if converted {
                    i += 2; // skip `=>`
                    // Collapse the whitespace after `=>` to a single space (or
                    // none, if the body starts on the next line) for predictable
                    // output.
                    while i < chars.len() && (chars[i].1 == ' ' || chars[i].1 == '\t') {
                        i += 1;
                    }
                    // The body may begin in the next chunk — `x => "bd"` puts it
                    // in a string — so fall through to the rest of the source.
                    let next = chars
                        .get(i)
                        .map(|x| x.1)
                        .or_else(|| src[end..].chars().next());
                    if next.is_some_and(|c| c != '\n' && c != '\r') {
                        out.push(' ');
                    }
                    continue;
                }
            }
            out.push(c);
            i += 1;
        }
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
    if !matches!(quote, '"' | '\'' | '`') {
        return literal.to_string();
    }
    let content = &literal[1..literal.len().saturating_sub(1)];
    // A backtick literal is a JS template and has no Koto spelling, so it always
    // gets rewritten. `ponytail: a `${...}` in one stays literal text rather
    // than interpolating; no tune uses one.
    if quote != '`' && !content.contains('{') && !content.contains('}') {
        return literal.to_string();
    }
    let mut hashes = "#".to_string();
    while content.contains(&format!("'{}", hashes)) {
        hashes.push('#');
    }
    format!("r{hashes}'{content}'{hashes}")
}

pub(super) fn rewrite_string_method_chains(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Str {
            out.push_str(text);
            continue;
        }
        let literal = normalize_string_literal(text);
        // A method call on the literal makes it a pattern. Blanks other than a
        // newline may separate the two; a newline (or a comment) breaks it.
        let mut after = src[end..]
            .trim_start_matches(|c: char| c.is_whitespace() && c != '\n')
            .chars();
        let method_chain =
            after.next() == Some('.') && after.next().is_some_and(|c| c.is_ascii_alphabetic());
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

/// Move a `,` that opens a line up to the end of the previous non-blank line.
///
/// Tunes separate the arguments of a long `stack(...)` with a comma on its own
/// line, so each layer can be commented and reordered:
///
/// ```text
/// stack(
///   s("bd sd").room(0.5)
///   ,
///   s("hh*8")
/// )
/// ```
///
/// Koto ends the argument at the newline and then meets a stray `,`. Hoisting
/// keeps the line count (and so the line numbers in error messages) intact,
/// unlike joining the lines. A comma is left where it is when the line above
/// already ends in `,` or an opening bracket, since moving it there would only
/// produce a different syntax error.
pub(super) fn hoist_leading_commas(src: &str) -> String {
    let mut line_blank = true;
    let mut line = 0usize;
    let mut leading = Vec::new();
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            line += text.matches('\n').count();
            // A string or comment is content: the line is no longer blank
            // unless the chunk itself ended it.
            line_blank = text.ends_with('\n');
            continue;
        }
        for c in text.chars() {
            if line_blank && c == ',' {
                leading.push(line);
            }
            if c == '\n' {
                line += 1;
            }
            line_blank = c == '\n' || (line_blank && c.is_whitespace());
        }
    }
    if leading.is_empty() {
        return src.to_string();
    }

    let mut lines: Vec<String> = src.split('\n').map(str::to_string).collect();
    for i in leading {
        let Some(prev) = (0..i).rev().find(|&j| !lines[j].trim().is_empty()) else {
            continue;
        };
        if lines[prev].trim_end().ends_with([',', '(', '[', '{']) {
            continue;
        }
        lines[i] = lines[i].replacen(',', "", 1);
        let at = lines[prev].trim_end().len();
        lines[prev].insert(at, ',');
    }
    lines.join("\n")
}

/// Extra indentation added to a `.`-continuation line, in spaces.
const CONTINUATION_INDENT: usize = 2;

/// Indent a line whose first non-blank character is `.`, so Koto reads the
/// method chain as a continuation of the line above rather than a new statement.
///
/// Existing indentation is kept and deepened, because tunes write their chains
/// *inside* an argument list, where the continuation already sits at the
/// argument's own indent and would otherwise read as the next argument:
///
/// ```text
/// stack(
///   s("bd sd")
///   .fast(2)     <- same indent as the argument, so Koto ends the argument here
/// )
/// ```
///
/// Deepening a line is not enough on its own: anything nested inside brackets
/// that line opens has to move with it, or the arguments of a chained call end
/// up level with the call itself —
///
/// ```text
/// "c3 e3"
/// .superimpose(  <- deepened to indent 2
///   x => x.add(12),  <- must go past 2 as well
/// ).note()
/// ```
///
/// so each continuation carries the column it was pushed to, as a stack keyed by
/// bracket depth. A line nested inside a continuation's brackets is held two
/// columns past it; a further `.` line at the same depth reuses the column its
/// chain already has (Koto wants a chain's lines aligned, not stepping right);
/// a line opening with `)`, `]`, `}` or `,` continues the expression; anything
/// else at or outside the continuation's depth starts a new one and drops it.
///
/// Indentation is only ever added, never taken away, so an indentation-sensitive
/// Koto block the user wrote by hand keeps its shape.
pub(super) fn indent_dot_continuations(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut changed = false;
    // Only blanks seen since the last newline, so a leading `.` is still a
    // continuation however deeply the line is indented.
    let mut line_blank = true;
    // Columns of blanks seen on this line so far, i.e. the line's own indent.
    let mut indent = 0usize;
    let mut depth = 0usize;
    // (bracket depth, emitted column) of each continuation still in force.
    let mut bumps: Vec<(usize, usize)> = Vec::new();
    // Emitted column of the last line that began an expression, per depth, which
    // is what a `.` line has to get past to read as its continuation.
    let mut stmt: Vec<usize> = vec![0];
    // Emitted column of the line that opened each depth. Everything inside has
    // to sit past it, or Koto ends the argument list at the newline — tunes
    // often write `stack(` and its arguments all hard against the left margin.
    let mut open_col: Vec<usize> = vec![0];
    let mut line_col = 0usize;

    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            // A string or comment is content: the line is no longer blank
            // unless the chunk itself ended it.
            line_blank = text.ends_with('\n');
            indent = 0;
            continue;
        }
        for c in text.chars() {
            if line_blank && !c.is_whitespace() {
                // First non-blank character of the line: settle its column.
                while bumps.last().is_some_and(|&(d, _)| d > depth) {
                    bumps.pop();
                }
                // A line that opens with a closing bracket belongs to the depth
                // outside it, so only lines that stay inside are held past the
                // column the bracket was opened at.
                let inside = if depth == 0 || matches!(c, ')' | ']' | '}') {
                    0
                } else {
                    open_col.get(depth).copied().unwrap_or(0) + CONTINUATION_INDENT
                };
                let floor = |bumps: &Vec<(usize, usize)>| {
                    bumps
                        .last()
                        .map_or(0, |&(_, col)| col + CONTINUATION_INDENT)
                        .max(inside)
                };
                let column = match c {
                    '.' if bumps.last().is_some_and(|&(d, _)| d == depth) => {
                        bumps.last().unwrap().1
                    }
                    '.' => {
                        let col = indent
                            .max(stmt.get(depth).copied().unwrap_or(0) + CONTINUATION_INDENT)
                            .max(floor(&bumps));
                        bumps.push((depth, col));
                        col
                    }
                    ')' | ']' | '}' | ',' => {
                        // The chain this line closes off ends with it; only a
                        // continuation opened further out still holds it in.
                        bumps.retain(|&(d, _)| d < depth);
                        indent.max(floor(&bumps))
                    }
                    _ => {
                        bumps.retain(|&(d, _)| d < depth);
                        let col = indent.max(floor(&bumps));
                        stmt.truncate(depth);
                        stmt.resize(depth + 1, col);
                        stmt[depth] = col;
                        col
                    }
                };
                for _ in indent..column {
                    out.push(' ');
                    changed = true;
                }
                line_col = column;
            }
            indent = if c == '\n' {
                0
            } else if line_blank {
                indent + 1
            } else {
                indent
            };
            match c {
                '(' | '[' | '{' => {
                    depth += 1;
                    open_col.truncate(depth);
                    open_col.resize(depth + 1, line_col);
                }
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            out.push(c);
            line_blank = c == '\n' || (line_blank && c.is_whitespace());
        }
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
    let mut out = String::with_capacity(src.len());
    // The last emitted character that is not whitespace, which decides whether a
    // `.` continues an expression or begins one. A comment leaves it alone: what
    // came before the comment is still what the `.` follows.
    let mut prev: Option<char> = None;
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            if kind == Chunk::Str {
                prev = text.chars().next();
            }
            continue;
        }
        let mut rest = text.chars().peekable();
        while let Some(c) = rest.next() {
            if c == '.'
                && rest.peek().is_some_and(|d| d.is_ascii_digit())
                && !prev.is_some_and(|p| {
                    // A closing quote ends a *value*, so the `.` after it is
                    // method access like any other — `"bd".5` is not `"bd"0.5`.
                    p.is_alphanumeric() || matches!(p, '_' | '$' | ')' | ']' | '.' | '"' | '\'')
                })
            {
                out.push('0');
            }
            out.push(c);
            if !c.is_whitespace() {
                prev = Some(c);
            }
        }
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
    let mut out = String::with_capacity(src.len());
    // Whether the previous emitted non-whitespace char could end an identifier,
    // so `myawait` / `x.await` are not touched.
    let mut prev: Option<char> = None;
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            if kind == Chunk::Str {
                prev = text.chars().next();
            }
            continue;
        }
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (byte, c) = chars[i];
            let is_word_boundary =
                !prev.is_some_and(|p| p.is_alphanumeric() || p == '_' || p == '$' || p == '.');
            if c == 'a' && is_word_boundary && text[byte..].starts_with("await") {
                // What follows may begin the next chunk (`await "bd"`).
                let after = chars
                    .get(i + 5)
                    .map(|x| x.1)
                    .or_else(|| src[end..].chars().next());
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
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut i = 0;
        while i < text.len() {
            if text[i..].starts_with("===") || text[i..].starts_with("!==") {
                // Keep the leading `=` or `!` and one `=`, drop the third.
                out.push_str(&text[i..i + 2]);
                i += 3;
                continue;
            }
            let c = text[i..].chars().next().unwrap_or_default();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every rewriter here shares the same guard: skip over strings and comments
    // so their contents are copied through untouched. Those guards are where the
    // surviving mutants clustered — five apiece on the two lines that detect a
    // comment opener — because the existing tests only fed each rewriter the
    // construct it rewrites, never the same construct quoted or commented out.
    //
    // Getting this wrong does not error. It rewrites a mini-notation string or a
    // comment and hands the user back source they did not write.

    /// Applied to `src`, then to the same `src` with the interesting part
    /// wrapped in a string, a line comment and a block comment.
    fn leaves_quoted_and_commented_alone(f: fn(&str) -> String, snippet: &str) {
        let quoted = format!("x = \"{snippet}\"");
        assert_eq!(f(&quoted), quoted, "rewrote inside a double-quoted string");

        let single = format!("x = '{snippet}'");
        assert_eq!(f(&single), single, "rewrote inside a single-quoted string");

        let block = format!("a /* {snippet} */ b");
        assert_eq!(f(&block), block, "rewrote inside a block comment");

        let line = format!("a // {snippet}\nb");
        assert_eq!(f(&line), line, "rewrote inside a line comment");
    }

    #[test]
    fn every_rewriter_spares_strings_and_both_comments() {
        // One shared scan (`scanner::chunks`) decides what is code for all of
        // these, so the guard is now tested once per rewriter rather than
        // re-derived in each. `strip_line_comments` is excluded because removing
        // line comments is its whole job.
        for (f, snippet) in [
            (rewrite_arrow_functions as fn(&str) -> String, "x => y"),
            (rewrite_leading_dot_numbers, "gain(.5)"),
            (rewrite_strict_equality, "a === b"),
            (rewrite_strict_equality, "a !== b"),
            (strip_await, "await foo"),
            (rewrite_string_method_chains, r#""bd".fast(2)"#),
        ] {
            leaves_quoted_and_commented_alone(f, snippet);
        }
    }

    #[test]
    fn indent_dot_continuations_only_indents_code() {
        // A leading dot at the start of a line is a method continuation and gets
        // indented so Koto reads it as one expression.
        assert_eq!(
            indent_dot_continuations("s(\"bd\")\n.fast(2)"),
            "s(\"bd\")\n  .fast(2)"
        );
        // The same shape inside a block comment or a multi-line string is text.
        for src in [
            "a /*\n.fast(2)\n*/ b",
            "x = \"a\n.fast(2)\n\"",
            "x = 'a\n.fast(2)\n'",
        ] {
            assert_eq!(indent_dot_continuations(src), src, "{src}");
        }
        // Source with nothing to indent comes back as it went in.
        assert_eq!(indent_dot_continuations("plain"), "plain");
    }

    #[test]
    fn indent_dot_continuations_holds_a_chain_together_inside_a_call() {
        // Level with the argument it continues, so it has to move past it...
        assert_eq!(
            indent_dot_continuations("stack(\n  s(\"bd\")\n  .fast(2),\n  s(\"hh\")\n)"),
            "stack(\n  s(\"bd\")\n    .fast(2),\n  s(\"hh\")\n)"
        );
        // ...and the next argument drops back to the argument column rather
        // than trailing the chain.
        // A second `.` line at the same depth aligns with the first instead of
        // stepping further right, which Koto rejects.
        assert_eq!(
            indent_dot_continuations("x\n.a()\n.b()"),
            "x\n  .a()\n  .b()"
        );
        // Arguments of a chained call have to clear the chain line above them,
        // and the chain resumes at its own column afterwards.
        assert_eq!(
            indent_dot_continuations("x\n.sup(\n  |v| v,\n).note()\n.gain(1)"),
            "x\n  .sup(\n    |v| v,\n    ).note()\n  .gain(1)"
        );
        // Arguments written hard against the call's own column are pushed in.
        assert_eq!(
            indent_dot_continuations("stack(\ns(\"bd\"),\ns(\"hh\")\n)"),
            "stack(\n  s(\"bd\"),\n  s(\"hh\")\n)"
        );
    }

    #[test]
    fn hoist_leading_commas_moves_the_separator_up_a_line() {
        // The comma joins the argument above it, and the line count holds.
        assert_eq!(
            hoist_leading_commas("stack(\n  s(\"bd\")\n  ,\n  s(\"hh\")\n)"),
            "stack(\n  s(\"bd\"),\n  \n  s(\"hh\")\n)"
        );
        // Blank lines between the two are skipped over.
        assert_eq!(hoist_leading_commas("a\n\n, b"), "a,\n\n b");
        // A line above that cannot take one is left alone, so the error still
        // names what the user wrote.
        for src in ["stack(\n, a\n)", "f(a,\n, b)"] {
            assert_eq!(hoist_leading_commas(src), src, "{src}");
        }
        // A comma opening a line of string or comment content is text.
        for src in ["x = \"a\n, b\"", "a /*\n, b\n*/"] {
            assert_eq!(hoist_leading_commas(src), src, "{src}");
        }
        // Nothing to hoist comes back as it went in.
        assert_eq!(hoist_leading_commas("f(a, b)"), "f(a, b)");
    }

    #[test]
    fn strip_line_comments_keeps_structure_and_spares_strings() {
        // The comment goes, the newline it sat on stays, so line numbers hold.
        assert_eq!(strip_line_comments("a // note\nb"), "a \nb");
        assert_eq!(strip_line_comments("a\n// whole line\nb"), "a\n\nb");
        // A trailing comment with no newline just ends.
        assert_eq!(strip_line_comments("a // end"), "a ");
        // `//` inside a string or a block comment is content, not a comment.
        assert_eq!(strip_line_comments(r#"s("a//b")"#), r#"s("a//b")"#);
        assert_eq!(strip_line_comments("a /* // */ b"), "a /* // */ b");
        // A URL in a string survives, which is the case users hit first.
        let url = r#"samples("https://example.com/x.json")"#;
        assert_eq!(strip_line_comments(url), url);
    }

    #[test]
    fn strip_await_only_removes_the_keyword_itself() {
        // The keyword and the space after it both go.
        assert_eq!(strip_await("await foo()"), "foo()");
        assert_eq!(strip_await("x = await bar"), "x = bar");
        // Identifiers that merely contain or end with `await` are untouched.
        assert_eq!(strip_await("myawait()"), "myawait()");
        assert_eq!(strip_await("awaited"), "awaited");
        assert_eq!(strip_await("x.await"), "x.await");
        assert_eq!(strip_await("await_thing"), "await_thing");
        // Nothing to do at all is returned unchanged.
        assert_eq!(strip_await("plain source"), "plain source");
        leaves_quoted_and_commented_alone(strip_await, "await foo");
    }

    #[test]
    fn rewrite_strict_equality_loosens_both_operators() {
        assert_eq!(rewrite_strict_equality("a === b"), "a == b");
        assert_eq!(rewrite_strict_equality("a !== b"), "a != b");
        // Already-loose comparisons are left as they are.
        assert_eq!(rewrite_strict_equality("a == b"), "a == b");
        assert_eq!(rewrite_strict_equality("a != b"), "a != b");
        // Source with neither is returned untouched by the early exit.
        assert_eq!(rewrite_strict_equality("a < b"), "a < b");
        leaves_quoted_and_commented_alone(rewrite_strict_equality, "a === b");
        leaves_quoted_and_commented_alone(rewrite_strict_equality, "a !== b");
    }

    #[test]
    fn rewrite_leading_dot_numbers_only_fills_in_a_missing_zero() {
        // A `.5` that begins a value becomes `0.5`...
        assert_eq!(rewrite_leading_dot_numbers("gain(.5)"), "gain(0.5)");
        assert_eq!(rewrite_leading_dot_numbers("x = .25"), "x = 0.25");
        assert_eq!(rewrite_leading_dot_numbers("[.1, .2]"), "[0.1, 0.2]");
        // ...but a `.` that continues an expression is a method call, not a
        // number, and must not gain one.
        assert_eq!(
            rewrite_leading_dot_numbers("s(\"bd\").fast(2)"),
            "s(\"bd\").fast(2)"
        );
        assert_eq!(rewrite_leading_dot_numbers("x.5"), "x.5");
        // An already-complete number is untouched.
        assert_eq!(rewrite_leading_dot_numbers("0.5"), "0.5");
        leaves_quoted_and_commented_alone(rewrite_leading_dot_numbers, "gain(.5)");
    }

    #[test]
    fn rewrite_string_method_chains_wraps_only_chained_literals() {
        // A literal with a method called on it becomes a pattern.
        assert_eq!(
            rewrite_string_method_chains(r#""bd sd".fast(2)"#),
            r#"pat("bd sd").fast(2)"#
        );
        // Whitespace between the literal and the dot still counts as a chain.
        assert_eq!(
            rewrite_string_method_chains(r#""bd"  .fast(2)"#),
            r#"pat("bd")  .fast(2)"#
        );
        // A bare literal is left alone — wrapping every string would break
        // ordinary arguments.
        assert_eq!(rewrite_string_method_chains(r#"s("bd")"#), r#"s("bd")"#);
        // A dot that is not a method call does not trigger it either.
        assert_eq!(rewrite_string_method_chains(r#""bd".5"#), r#""bd".5"#);
        // A newline between them breaks the chain.
        assert_eq!(
            rewrite_string_method_chains("\"bd\"\n.fast(2)"),
            "\"bd\"\n.fast(2)"
        );
        // Contents of a block comment are copied through.
        let commented = r#"/* "bd".fast(2) */"#;
        assert_eq!(rewrite_string_method_chains(commented), commented);
    }

    #[test]
    fn rewrite_arrow_functions_maps_expression_bodies_only() {
        assert_eq!(rewrite_arrow_functions("x => x.fast(2)"), "|x| x.fast(2)");
        assert_eq!(rewrite_arrow_functions("(a, b) => a + b"), "|a, b| a + b");
        assert_eq!(rewrite_arrow_functions("() => 1"), "|| 1");
        // A block body is left alone, so the error names the `=>` the user
        // typed rather than a map literal they did not.
        for block in [
            "x => { x }",
            "(a, b) => { a + b }",
            "() => {
  1
}",
        ] {
            assert_eq!(rewrite_arrow_functions(block), block, "{block}");
        }
        // Whitespace and a newline before the brace still make it a block.
        assert_eq!(
            rewrite_arrow_functions(
                "x =>
  { x }"
            ),
            "x =>
  { x }"
        );
        // A body that merely *contains* a brace later is still an expression.
        assert_eq!(
            rewrite_arrow_functions("x => f(x, { a: 1 })"),
            "|x| f(x, { a: 1 })"
        );
        // An `=>` inside a string is pattern text, not a lambda.
        leaves_quoted_and_commented_alone(rewrite_arrow_functions, "x => y");
    }

    #[test]
    fn an_arrow_with_nothing_usable_before_it_is_left_as_it_is() {
        // The parameter scan walks back over the blanks between the params and
        // the `=>`; with nothing but blanks there it must stop at the start of
        // the output rather than stepping off it.
        assert_eq!(rewrite_arrow_functions("  => x"), "  => x");
        assert_eq!(rewrite_arrow_functions("=> x"), "=> x");
        // An operator is not a parameter list either.
        assert_eq!(rewrite_arrow_functions("+ => x"), "+ => x");
    }

    #[test]
    fn an_arrow_needs_no_space_around_it() {
        // Without a blank before the `=>` the walk back to the matching `(`
        // starts on the `)` itself.
        assert_eq!(rewrite_arrow_functions("(a,b)=>x"), "|a,b| x");
        assert_eq!(rewrite_arrow_functions("x=>x"), "|x| x");
        assert_eq!(rewrite_arrow_functions("()=>1"), "|| 1");
    }

    #[test]
    fn an_arrow_at_the_very_end_has_no_body_to_reach_for() {
        // The blank-collapsing walk after `=>` must stop at the end of the
        // source rather than reading past it.
        assert_eq!(rewrite_arrow_functions("x =>"), "|x|");
        assert_eq!(rewrite_arrow_functions("x =>   "), "|x|");
    }

    #[test]
    fn a_body_on_the_next_line_keeps_its_line_break() {
        // The space after `|x|` is only for a body that follows on the same
        // line; adding one before a newline would trail whitespace into every
        // multi-line callback.
        assert_eq!(rewrite_arrow_functions("x =>\n  y"), "|x|\n  y");
        assert_eq!(rewrite_arrow_functions("x =>\r\ny"), "|x|\r\ny");
        // On the same line it does get exactly one space, however many it had.
        assert_eq!(rewrite_arrow_functions("x =>     y"), "|x| y");
    }

    #[test]
    fn a_literal_with_braces_becomes_a_raw_string() {
        // Koto reads `{}` inside an ordinary string as interpolation, so a
        // mini-notation literal using them has to be re-quoted raw or the
        // pattern is evaluated as code.
        assert_eq!(
            rewrite_string_method_chains(r#""{a}".fast(2)"#),
            r#"pat(r#'{a}'#).fast(2)"#
        );
        // Either brace alone is enough to need it.
        assert_eq!(
            rewrite_string_method_chains(r#""a{b".fast(2)"#),
            r#"pat(r#'a{b'#).fast(2)"#
        );
        assert_eq!(
            rewrite_string_method_chains(r#""a}b".fast(2)"#),
            r#"pat(r#'a}b'#).fast(2)"#
        );
        // A literal without them is left in its original quotes.
        assert_eq!(
            rewrite_string_method_chains(r#""bd".fast(2)"#),
            r#"pat("bd").fast(2)"#
        );
        // The rewrite applies to bare literals too, not only chained ones.
        assert_eq!(
            rewrite_string_method_chains(r#"s("{a}")"#),
            r#"s(r#'{a}'#)"#
        );
    }

    #[test]
    fn a_comment_does_not_stand_in_for_what_came_before_it() {
        // Both of these passes track the last *code* character to decide
        // whether they are at the start of a value. A comment is not one, so it
        // must leave that decision untouched rather than answering it with `/`.
        assert_eq!(
            rewrite_leading_dot_numbers("x /* c */ .5"),
            "x /* c */ .5",
            "the dot still continues `x`"
        );
        assert_eq!(
            strip_await("x /* c */ await b"),
            "x /* c */ await b",
            "`await` is still inside an identifier boundary"
        );
    }

    #[test]
    fn a_dot_after_a_string_literal_is_method_access() {
        // A closing quote ends a value, so the dot cannot begin a number.
        assert_eq!(
            rewrite_leading_dot_numbers(r#"x = "bd".5"#),
            r#"x = "bd".5"#
        );
        assert_eq!(rewrite_leading_dot_numbers("x = 'bd'.5"), "x = 'bd'.5");
        // The same dot before a name was already method access.
        assert_eq!(
            rewrite_leading_dot_numbers(r#""bd".fast(2)"#),
            r#""bd".fast(2)"#
        );
    }

    #[test]
    fn await_is_a_word_only_after_something_that_cannot_end_a_name() {
        // Each character class that ends an identifier has to be listed, or a
        // keyword is torn out of the middle of a name.
        assert_eq!(strip_await("_await"), "_await");
        assert_eq!(strip_await("$await"), "$await");
        assert_eq!(strip_await("a9await"), "a9await");
        assert_eq!(strip_await("x.await"), "x.await");
        // ...and after something that does end one, it is the keyword.
        assert_eq!(strip_await("(await x)"), "(x)");
        assert_eq!(strip_await("[await x]"), "[x]");
    }

    #[test]
    fn the_rewriters_compose_without_disturbing_a_pattern_string() {
        // The realistic case: a mini-notation string carrying characters every
        // one of these rewriters looks for, inside a chain they all see.
        let src = r#"s("bd*2 [~ sd]").gain(.5).every(2, x => x.fast(2))"#;
        let out = rewrite_arrow_functions(&rewrite_leading_dot_numbers(
            &rewrite_string_method_chains(&strip_await(&rewrite_strict_equality(src))),
        ));
        assert!(
            out.contains(r#""bd*2 [~ sd]""#),
            "the pattern string must survive intact, got {out}"
        );
        assert!(out.contains("gain(0.5)"), "the leading dot is filled in");
        assert!(out.contains("|x| x.fast(2)"), "the arrow becomes a lambda");
    }
}
