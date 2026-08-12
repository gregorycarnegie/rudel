use super::scanner::{Chunk, chunks, code_mask, is_ident_char, is_tagged_template};

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

/// Rewrite a JavaScript tagged template — a call written as a function name
/// with a backtick literal stuck straight onto it — into an ordinary call.
///
/// ```text
/// loadCsound`instr X ... endin`   ->   loadCsound(`instr X ... endin`)
/// ```
///
/// This is how the Csound tunes pass an orchestra, and it is the only place
/// Strudel's own examples use the form: a multi-line body with quotes and
/// apostrophes in it, which no other literal survives. Nothing is interpolated,
/// so the tag receives the text as its single argument, which is what upstream's
/// `loadCsound` also reduces the form to.
///
/// Runs after the mini pass, so inserting these two characters cannot move a
/// recorded source location — those are already emitted as literal offsets.
pub(super) fn rewrite_tagged_templates(src: &str) -> String {
    if !src.contains('`') {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len() + 8);
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind == Chunk::Str && is_tagged_template(src, start) {
            out.push('(');
            out.push_str(text);
            out.push(')');
        } else {
            out.push_str(text);
        }
    }
    out
}

/// Operators that carry an alignment, and how each is spelled in Koto. `mod` is
/// a Koto keyword, so it is bound as `modulo`.
const ALIGNED_OPS: &[(&str, &str)] = &[
    ("add", "add"),
    ("sub", "sub"),
    ("mul", "mul"),
    ("div", "div"),
    ("set", "set"),
    ("keep", "keep"),
    ("mod", "modulo"),
    ("modulo", "modulo"),
    ("pow", "pow"),
];

/// Alignments, and the suffix each becomes. `in` is the default and *is* the
/// plain method, so it collapses to nothing; the camelCase and `squeezein`
/// spellings normalise here rather than needing an alias apiece.
const ALIGNMENTS: &[(&str, &str)] = &[
    ("in", ""),
    ("out", "_out"),
    ("mix", "_mix"),
    ("squeeze", "_squeeze"),
    ("squeezein", "_squeeze"),
    ("squeezeIn", "_squeeze"),
    ("squeezeout", "_squeezeout"),
    ("squeezeOut", "_squeezeout"),
    ("reset", "_reset"),
    ("restart", "_restart"),
    ("poly", "_poly"),
];

/// Rewrite Strudel's alignment *getters* (`.add.out(x)`) into the single method
/// Rudel binds them as (`.add_out(x)`).
///
/// In Strudel `pat.add` is an object whose properties are the aligned variants,
/// so the alignment is reached by a second property access. Koto has no
/// property getters, so the matrix is bound flat — one name per cell — and the
/// two spellings differ only in that dot.
///
/// Only `.op.align(` is rewritten: the alignment has to be immediately applied,
/// which is the only form that means anything on either side. String literals
/// and comments are skipped.
pub(super) fn rewrite_alignment_getters(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        'scan: while let Some(dot) = rest.find('.') {
            for (js, koto) in ALIGNED_OPS {
                let Some(after_op) = rest[dot..].strip_prefix(&format!(".{js}.")) else {
                    continue;
                };
                for (align, suffix) in ALIGNMENTS {
                    // The `(` is what tells `.add.out(x)` from a chain that
                    // merely happens to read `.add.outSomething`.
                    if after_op
                        .strip_prefix(align)
                        .is_some_and(|tail| tail.starts_with('('))
                    {
                        out.push_str(&rest[..dot]);
                        out.push('.');
                        out.push_str(koto);
                        out.push_str(suffix);
                        rest = &after_op[align.len()..];
                        continue 'scan;
                    }
                }
            }
            out.push_str(&rest[..dot + 1]);
            rest = &rest[dot + 1..];
        }
        out.push_str(rest);
    }
    out
}

/// Rewrite JavaScript's logical operators into the words Koto spells them with:
/// `&&` -> `and`, `||` -> `or`, and a leading `!` -> `not`.
///
/// `!=` is a comparison, not a negation, and keeps its `!`. Strings are skipped,
/// so mini-notation's replicate operator (`"x!4"`) and any `|` inside a pattern
/// are untouched.
pub(super) fn rewrite_logical_operators(src: &str) -> String {
    // Bytes, not chars: every operator matched here is ASCII and a UTF-8
    // continuation byte can never look like one, so the rest passes through
    // untouched and the result is still valid UTF-8.
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let bytes = &src.as_bytes()[start..end];
        if kind != Chunk::Code {
            out.extend_from_slice(bytes);
            continue;
        }
        let mut i = 0;
        while i < bytes.len() {
            // Keyword operators need separating from what is around them, but
            // only where a blank is not already there — `a && b` should not
            // come out as `a  and  b`.
            let word = |out: &mut Vec<u8>, keyword: &[u8], width: usize| {
                if !out.last().is_none_or(|b| b.is_ascii_whitespace()) {
                    out.push(b' ');
                }
                out.extend_from_slice(keyword);
                if !bytes.get(i + width).is_none_or(|b| b.is_ascii_whitespace()) {
                    out.push(b' ');
                }
                i + width
            };
            i = match bytes.get(i..i + 2) {
                Some(b"&&") => word(&mut out, b"and", 2),
                Some(b"||") => word(&mut out, b"or", 2),
                _ if bytes[i] == b'!' && bytes.get(i + 1) != Some(&b'=') => {
                    word(&mut out, b"not", 1)
                }
                _ => {
                    out.push(bytes[i]);
                    i + 1
                }
            };
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Just past the `}` matching the `{` at `open`, skipping nested braces.
fn matching_brace(mask: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &byte) in mask.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split a function body into statements at the `;` and newlines that are not
/// inside brackets, dropping the empties.
fn body_statements(body: &str) -> Vec<&str> {
    let mask = code_mask(body);
    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, &byte) in mask.iter().enumerate() {
        let delta = bracket_delta(byte);
        if delta != 0 {
            depth += delta;
            continue;
        }
        if depth == 0 && (byte == b';' || byte == b'\n') {
            out.push(&body[start..i]);
            start = i + 1;
        }
    }
    out.push(&body[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Rewrite one JS statement into its Koto spelling: `if (c) x` takes Koto's
/// `then`, and a `return` in tail position is just the value.
fn koto_statement(stmt: &str, tail: bool) -> String {
    let stmt = stmt.trim();
    if let Some(rest) = stmt.strip_prefix("if") {
        let rest = rest.trim_start();
        if rest.starts_with('(') {
            let mask = code_mask(rest);
            let mut depth = 0i32;
            for (i, &byte) in mask.iter().enumerate() {
                depth += bracket_delta(byte);
                if depth == 0 {
                    let cond = &rest[1..i];
                    let body = koto_statement(&rest[i + 1..], tail);
                    return format!("if {} then {body}", cond.trim());
                }
            }
        }
    }
    // A `return` at the end of the body is the block's value; Koto allows the
    // keyword anywhere, but reads a bare trailing expression the same way.
    // `return(...)` with no space is what an earlier pass leaves behind when it
    // lifts a keyword off a parenthesised expression.
    match stmt.strip_prefix("return") {
        Some(value) if tail && value.starts_with(|c: char| c.is_whitespace() || c == '(') => {
            value.trim().to_string()
        }
        _ => stmt.to_string(),
    }
}

/// A brace-delimited function body found in the source. Everything from `from`
/// to just past the `}` at `close` is replaced by `head` plus the rendered body.
struct BlockBody {
    from: usize,
    head: String,
    open: usize,
    close: usize,
}

/// Index of the last non-whitespace byte before `at`.
fn last_code_before(mask: &[u8], at: usize) -> Option<usize> {
    mask[..at].iter().rposition(|b| !b.is_ascii_whitespace())
}

/// Where the identifier ending at `end` begins.
fn ident_start(src: &str, end: usize) -> usize {
    src[..end]
        .char_indices()
        .rev()
        .find(|&(_, c)| !is_ident_char(c))
        .map_or(0, |(i, c)| i + c.len_utf8())
}

/// The first `{ ... }` that is a *function body* — either an arrow's, or a
/// `function` declaration's. Object literals and Koto maps have neither an `=>`
/// nor a `function` header in front of them and are skipped.
fn find_block_body(src: &str, mask: &[u8]) -> Option<BlockBody> {
    for open in mask
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'{')
        .map(|(i, _)| i)
    {
        let Some(prev) = last_code_before(mask, open) else {
            continue;
        };
        // `... => { body }`. The parameters to the left of the `=>` are left
        // for `rewrite_arrow_functions`, which now sees an expression body.
        if mask[prev] == b'>' && prev > 0 && mask[prev - 1] == b'=' {
            return Some(BlockBody {
                from: open,
                head: String::new(),
                open,
                close: matching_brace(mask, open)?,
            });
        }
        // `function name(a, b) { body }`, or the anonymous form.
        if mask[prev] != b')' {
            continue;
        }
        let mut depth = 0i32;
        let params_open = (0..=prev).rev().find(|&i| {
            depth -= bracket_delta(mask[i]);
            mask[i] == b'(' && depth == 0
        })?;
        let name_end = mask[..params_open]
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map_or(0, |i| i + 1);
        let name_start = ident_start(src, name_end);
        let keyword_end = mask[..name_start]
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map_or(0, |i| i + 1);
        let (name, from) = if &src[name_start..name_end] == "function" {
            ("", name_start)
        } else if src[..keyword_end].ends_with("function") {
            (&src[name_start..name_end], keyword_end - "function".len())
        } else {
            continue;
        };
        let params = &src[params_open + 1..prev];
        // An anonymous `function (x) {}` is just a lambda; a named one binds.
        let head = if name.is_empty() {
            format!("|{params}|")
        } else {
            format!("{name} = |{params}|")
        };
        return Some(BlockBody {
            from,
            head,
            open,
            close: matching_brace(mask, open)?,
        });
    }
    None
}

/// Rewrite JavaScript function bodies written as a brace block into Koto's
/// indented blocks:
///
/// ```text
/// pat.withValue((v) => { const x = v.n; return x + 1 })
///
/// pat.withValue((v) =>
///     x = v.n
///     x + 1
///   )
/// ```
///
/// The closing bracket has to end up on a line of its own: Koto reads the
/// indented lines as the function's body and will not let the enclosing call
/// close on the body's last line. The body is indented two columns past the
/// line the construct starts on, and everything after the `}` follows at that
/// line's own indent, which is the shape Koto accepts for a block passed as an
/// argument.
///
/// Runs before [`rewrite_arrow_functions`], which then sees an ordinary
/// expression-bodied arrow and converts the parameters as usual. `function`
/// declarations become plain assignments (`function f(a) {}` -> `f = |a|`).
///
/// ponytail: one construct per pass, repeated — nesting is at most a few deep
/// in practice and the whole source is small.
pub(super) fn rewrite_block_bodies(src: &str) -> String {
    let mut current = src.to_string();
    // Each round removes one `{`; that count is the ceiling.
    for _ in 0..src.matches('{').count() {
        let mask = code_mask(&current);
        let Some(block) = find_block_body(&current, &mask) else {
            break;
        };
        let line_start = current[..block.from].rfind('\n').map_or(0, |i| i + 1);
        let indent = current[line_start..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let body = &current[block.open + 1..block.close];
        let statements = body_statements(body);
        let last = statements.len().saturating_sub(1);
        let mut rendered = String::new();
        for (i, stmt) in statements.iter().enumerate() {
            rendered.push('\n');
            rendered.push_str(&" ".repeat(indent + CONTINUATION_INDENT));
            rendered.push_str(&koto_statement(stmt, i == last));
        }
        rendered.push('\n');
        rendered.push_str(&" ".repeat(indent));
        // The brackets that close the construct move onto the line just
        // emitted, however many newlines the source left between them and the
        // `}` — a blank line there would end the block early. Anything else
        // after the `}` starts a statement of its own and keeps its lines.
        let rest = &current[block.close + 1..];
        let closers = rest.trim_start();
        let rest = if closers.starts_with([')', ']', '}', ',', '.']) {
            closers
        } else {
            rest.trim_start_matches([' ', '\t'])
        };
        current = format!("{}{}{}{rest}", &current[..block.from], block.head, rendered,);
    }
    current
}

/// Rewrite JavaScript's object spread into a call Koto can make:
///
/// ```text
/// {...v, value: result}   ->   rudel_spread(v, {value: result})
/// ```
///
/// Koto has no spread in a map declaration. `rudel_spread` copies the base map
/// and lays the overrides on top, which is what the JS form means. Songs use it
/// to return a hap value with one field replaced.
///
/// Only a spread that *opens* the literal is handled — the form every use in
/// the wild takes, and the only one where "copy, then override" is the whole
/// story.
pub(super) fn rewrite_object_spreads(src: &str) -> String {
    let mut current = src.to_string();
    for _ in 0..src.matches("...").count() {
        let mask = code_mask(&current);
        let Some(open) =
            (0..mask.len()).find(|&i| mask[i] == b'{' && mask[i + 1..].starts_with(b"..."))
        else {
            break;
        };
        let Some(close) = matching_brace(&mask, open) else {
            break;
        };
        let inner = &current[open + 4..close];
        // The spread base runs to the first comma outside brackets; whatever
        // follows is the ordinary part of the literal.
        let inner_mask = code_mask(inner);
        let mut depth = 0i32;
        let split = inner_mask
            .iter()
            .position(|&b| {
                depth += bracket_delta(b);
                depth == 0 && b == b','
            })
            .unwrap_or(inner.len());
        let base = inner[..split].trim();
        let overrides = inner[split.min(inner.len())..]
            .trim_start_matches(',')
            .trim();
        current = format!(
            "{}rudel_spread({base}, {{{overrides}}}){}",
            &current[..open],
            &current[close + 1..],
        );
    }
    current
}

/// Fold a `{ ... }` map literal that is spread over several lines onto one.
///
/// Read `x.value` the way JavaScript does — the field if `x` is an object,
/// `undefined` if it is not.
///
/// `hap.value` is Strudel's own name for a control map's payload, and helpers
/// branch on it (`const isobj = v.value !== undefined`) to tell a plain note
/// from a map of controls. Koto has no property access and errors on a field
/// read against a string or list, so the test that was meant to *detect* the
/// bare case is the thing that fails on it.
///
/// Only `value` is rewritten, and only when read as a property: `.value(`
/// is a call and `.values` is a different name.
pub(super) fn rewrite_value_property(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some(at) = rest.find(".value") {
            let after = &rest[at + ".value".len()..];
            let receiver_start = ident_start(rest, at);
            if after.starts_with('(') || after.starts_with(is_ident_char) || receiver_start == at {
                out.push_str(&rest[..at + ".value".len()]);
            } else {
                out.push_str(&rest[..receiver_start]);
                out.push_str(&format!(
                    "rudel_prop({}, 'value')",
                    &rest[receiver_start..at]
                ));
            }
            rest = after;
        }
        out.push_str(rest);
    }
    out
}

/// Turn JavaScript's `.length` *property* into the method call Koto needs.
///
/// `v.length` is how every JS helper asks how long a list or string is; Koto has
/// no property access, so it has to become `v.length()`. A `.length(` that is
/// already a call is left alone.
pub(super) fn rewrite_length_property(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some(at) = rest.find(".length") {
            let after = &rest[at + ".length".len()..];
            out.push_str(&rest[..at]);
            // Not a property if a call already follows, or if the name runs on
            // (`.lengthen`).
            if after.starts_with('(') || after.starts_with(is_ident_char) {
                out.push_str(".length");
            } else {
                out.push_str(".length()");
            }
            rest = after;
        }
        out.push_str(rest);
    }
    out
}

/// Quote a map key that is written as a bare number.
///
/// JS object literals are keyed by number constantly — `{0: "...", 1: "..."}`
/// is how songs name the sections a `pickRestart` selects between. Koto's map
/// declaration takes an identifier or a string, so a numeric key is "expected
/// '}' at end of map declaration" pointing at the key itself, which reads as a
/// complaint about the brace instead. Quoting is faithful: JS object keys are
/// strings too, and the pick lookup matches on the key's text.
///
/// Only a number in key position — right after the `{` or a `,` that opens an
/// entry, and immediately followed by `:` — is touched.
pub(super) fn quote_numeric_map_keys(src: &str) -> String {
    if !src.contains('{') {
        return src.to_string();
    }
    let mask = code_mask(src);
    let mut out = String::with_capacity(src.len());
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < mask.len() {
        let byte = mask[i];
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        out.push_str(&src[i..i + 1]);
        i += 1;
        if depth <= 0 || !matches!(byte, b'{' | b',') {
            continue;
        }
        let blanks = mask[i..]
            .iter()
            // Newlines included: a JS object literal puts each entry on its own
            // line, so the key rarely shares a line with the comma before it.
            .take_while(|b| b.is_ascii_whitespace())
            .count();
        let key_start = i + blanks;
        let key_len = mask[key_start..]
            .iter()
            .take_while(|b| b.is_ascii_digit() || **b == b'.' || **b == b'-')
            .count();
        if key_len == 0 || mask.get(key_start + key_len) != Some(&b':') {
            continue;
        }
        out.push_str(&src[i..key_start]);
        out.push('\'');
        out.push_str(&src[key_start..key_start + key_len]);
        out.push('\'');
        i = key_start + key_len;
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
            // `let` is a Koto keyword too, but for a *typed* binding
            // (`let x: Number = 1`), so a JS `let parts = {…}` parses as one
            // and the map that follows is read as the type annotation.
            match ["const ", "let ", "var "]
                .iter()
                .find_map(|kw| rest.strip_prefix(kw))
            {
                Some(stripped) => split_declarations(indent, stripped),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Break one JS declaration of several names — `const sk = 80, sh = silence` —
/// into a line per name. Koto has no comma-separated binding, and reads the
/// second name as another assignment target for the first value ("expected
/// target for assignment"). Only commas outside brackets separate declarations;
/// the ones inside a value's own list or map stay put.
fn split_declarations(indent: &str, rest: &str) -> String {
    let mask = code_mask(rest);
    let mut depth = 0i32;
    let mut out = String::with_capacity(rest.len());
    let mut start = 0usize;
    for (i, &byte) in mask.iter().enumerate() {
        let delta = bracket_delta(byte);
        if delta != 0 {
            depth += delta;
        } else if depth == 0 && byte == b',' {
            out.push_str(indent);
            out.push_str(rest[start..i].trim());
            out.push('\n');
            start = i + 1;
        }
    }
    out.push_str(indent);
    out.push_str(rest[start..].trim_start());
    out
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

/// Bracket characters, as `(opens, closes)`.
fn bracket_delta(byte: u8) -> i32 {
    match byte {
        b'(' | b'[' | b'{' => 1,
        b')' | b']' | b'}' => -1,
        _ => 0,
    }
}

/// Start of the expression the `?` at `at` tests, scanning left through the
/// masked source. Brackets are skipped whole; the first thing at the outer
/// level that cannot be part of an expression ends the walk.
fn condition_start(mask: &[u8], at: usize) -> usize {
    let mut depth = 0i32;
    let mut i = at;
    while i > 0 {
        i -= 1;
        let byte = mask[i];
        let closing = matches!(byte, b')' | b']' | b'}');
        if closing {
            depth += 1;
            continue;
        }
        if bracket_delta(byte) > 0 {
            if depth == 0 {
                return i + 1;
            }
            depth -= 1;
            continue;
        }
        if depth > 0 {
            continue;
        }
        let boundary = match byte {
            b',' | b';' | b'?' | b':' | b'\n' => true,
            // The `>` of a `=>`; a comparison `>` has no `=` before it.
            b'>' => i > 0 && mask[i - 1] == b'=',
            // A plain assignment, not `==`, `!=`, `<=`, `>=` or `=>`.
            b'=' => {
                !matches!(mask.get(i.wrapping_sub(1)), Some(b'=' | b'!' | b'<' | b'>'))
                    && mask.get(i + 1) != Some(&b'=')
            }
            _ => false,
        };
        if boundary {
            return i + 1;
        }
    }
    0
}

/// The `:` belonging to the `?` at `at`, skipping bracketed groups and the
/// colons of any ternary nested in the true branch.
fn matching_colon(mask: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut nested = 0usize;
    for (i, &byte) in mask.iter().enumerate().skip(at + 1) {
        let delta = bracket_delta(byte);
        if delta != 0 {
            depth += delta;
            if depth < 0 {
                return None;
            }
            continue;
        }
        if depth > 0 {
            continue;
        }
        match byte {
            b'?' => nested += 1,
            b':' if nested == 0 => return Some(i),
            b':' => nested -= 1,
            _ => {}
        }
    }
    None
}

/// Just past the else branch that starts after the `:` at `at`.
fn else_end(mask: &[u8], at: usize) -> usize {
    let mut depth = 0i32;
    let mut nested = 0usize;
    for (i, &byte) in mask.iter().enumerate().skip(at + 1) {
        let delta = bracket_delta(byte);
        if delta != 0 {
            depth += delta;
            if depth < 0 {
                return i;
            }
            continue;
        }
        if depth > 0 {
            continue;
        }
        match byte {
            b',' | b';' | b'\n' => return i,
            b'?' => nested += 1,
            b':' if nested == 0 => return i,
            b':' => nested -= 1,
            _ => {}
        }
    }
    mask.len()
}

/// Rewrite JavaScript's conditional operator into Koto's `if`/`then`/`else`
/// expression:
///
/// ```text
/// v.endsWith('m') ? [v, 'minor'] : [v, 'major']
/// (if v.endsWith('m') then [v, 'minor'] else [v, 'major'])
/// ```
///
/// The result is parenthesised so it stays a single operand wherever the
/// ternary was — inside an argument list, an array, or a longer chain.
///
/// The *last* `?` is rewritten first and the pass repeats, which is what makes
/// nesting work without tracking it: a ternary nested in another's condition or
/// branches always sits to the right of, or inside brackets belonging to, an
/// unprocessed one, so by the time an outer `?` is reached its parts are plain
/// expressions again. `return` is the one keyword that can sit directly left of
/// a condition and be swallowed by it, so it is put back.
pub(super) fn rewrite_ternaries(src: &str) -> String {
    let mut current = src.to_string();
    // Each round consumes one `?`; the count in the original is the ceiling.
    for _ in 0..src.matches('?').count() {
        let mask = code_mask(&current);
        let Some(at) = mask.iter().rposition(|&b| b == b'?') else {
            break;
        };
        let Some(colon) = matching_colon(&mask, at) else {
            break;
        };
        let mut start = condition_start(&mask, at);
        let end = else_end(&mask, colon);
        // `return x ? y : z` — the condition is `x`, not `return x`.
        for keyword in ["return", "then", "else"] {
            let head = current[start..at].trim_start();
            if let Some(rest) = head.strip_prefix(keyword)
                && rest.starts_with(|c: char| c.is_whitespace() || c == '(')
            {
                start = at - rest.len();
            }
        }
        current = format!(
            "{}(if {} then {} else {}){}",
            &current[..start],
            current[start..at].trim(),
            current[at + 1..colon].trim(),
            current[colon + 1..end].trim(),
            &current[end..],
        );
    }
    current
}

/// The last significant character of each line, ignoring blanks — a `"` stands
/// in for any string literal and `/` for a comment, so a line ending inside one
/// is never mistaken for a line ending in code. Lines with nothing on them get
/// `None`. Shared by the two joining passes below.
fn line_tails(src: &str) -> Vec<Option<char>> {
    let mut tails = vec![None; src.split('\n').count()];
    let mut line = 0usize;
    for (kind, start, end) in chunks(src) {
        for c in src[start..end].chars() {
            if c == '\n' {
                line += 1;
            } else if !c.is_whitespace() {
                tails[line] = Some(match kind {
                    Chunk::Code => c,
                    Chunk::Str => '"',
                    _ => '/',
                });
            }
        }
    }
    tails
}

/// Join a line whose code ends in `=` or `=>` onto the next one.
///
/// Songs write a long map or array on the lines *after* the assignment, and an
/// arrow function's body on the line after its parameters:
///
/// ```text
/// const fingering =
/// {o:"x:x:x", g:"3:x:x"}
///
/// register('toscale', (pat) => pat.withValue((v) =>
///   v.endsWith('m') ? [...] : [...]))
/// ```
///
/// JS does not care where the value starts; Koto ends the statement at the
/// newline. For `=` that is "expected expression after assignment operator"
/// against a line that looks complete. For `=>` it is worse: the body becomes
/// an *indented block*, which Koto will not let the enclosing call close on the
/// body's own line (`... 'major']))` — one `)` too many), so the error lands on
/// a paren that is perfectly balanced. Joining sidesteps both.
///
/// `==`, `!=`, `<=` and `>=` are comparisons, not assignments, and are left
/// alone — as is a line ending in `=` inside a string or comment, which
/// `line_tails` already distinguishes.
pub(super) fn join_dangling_operators(src: &str) -> String {
    let tails = line_tails(src);
    let mut lines: Vec<String> = src.split('\n').map(str::to_string).collect();
    // Back to front, so joining does not invalidate the indices still to come.
    for i in (0..lines.len().saturating_sub(1)).rev() {
        if !matches!(tails[i], Some('=') | Some('>')) {
            continue;
        }
        let code = lines[i].trim_end().to_string();
        let dangling = if tails[i] == Some('>') {
            code.ends_with("=>")
        } else {
            !["==", "<=", ">=", "!="].iter().any(|op| code.ends_with(op))
        };
        if !dangling {
            continue;
        }
        // Blank lines between the `=` and its value go with it.
        let Some(value) = (i + 1..lines.len()).find(|&j| !lines[j].trim().is_empty()) else {
            continue;
        };
        let next = lines.drain(i + 1..=value).next_back().unwrap_or_default();
        lines[i] = format!("{code} {}", next.trim_start());
    }
    lines.join("\n")
}

/// Drop a `;` that ends a line. JS statements may carry one; Koto has no
/// statement terminator and reads it as a stray token — and once a `.`
/// continuation has been folded onto the line above, the `;` lands *inside* the
/// call being closed (`.gain(0.3);)`), so the error names the paren instead.
/// A `;` with code after it separates two statements and is left for
/// [`rewrite_block_bodies`] to split, since only there is the indent known.
pub(super) fn strip_trailing_semicolons(src: &str) -> String {
    if !src.contains(';') {
        return src.to_string();
    }
    let tails = line_tails(src);
    let mut out = String::with_capacity(src.len());
    let mut line = 0usize;
    for (kind, start, end) in chunks(src) {
        for (offset, c) in src[start..end].char_indices() {
            // Only the *last* `;` on the line is the terminator; `tails` says
            // this line ends in one, so it is this `;` if nothing but blanks
            // follow it before the newline.
            if kind == Chunk::Code
                && c == ';'
                && tails[line] == Some(';')
                && src[start + offset + 1..]
                    .chars()
                    .take_while(|&c| c != '\n')
                    .all(char::is_whitespace)
            {
                continue;
            }
            if c == '\n' {
                line += 1;
            }
            out.push(c);
        }
    }
    out
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
/// Undo the line break that separates the text just emitted from the `.` about
/// to be, so a chain continues on the line that closed the call it chains off.
/// Refuses when the break is a blank line or more — that is a deliberate
/// separation, and swallowing it would join two statements — and reports
/// whether it joined.
fn join_onto_previous(out: &mut String) -> bool {
    let kept = out.trim_end_matches([' ', '\t', '\r', '\n']).len();
    if out[kept..].chars().filter(|&c| c == '\n').count() != 1 {
        return false;
    }
    out.truncate(kept);
    true
}

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
    // How the previous non-blank line began: its bracket depth, and whether its
    // first character was a closing bracket.
    //
    // A `.` line cannot be indented into place after a line that *continued* an
    // expression and closed a multi-line call while doing it — `.slow(2))`,
    // ending an argument list opened lines earlier. Koto will not carry the
    // chain onto the next line there however far the `.` is pushed, but it does
    // accept the chain written on the closing line itself, so that is what this
    // one gets joined to. A line that *opens* with the closing bracket (`)` on
    // its own, or `).note()`) is the shape Koto already continues from, and
    // joining it would only run two readable lines together.
    let mut previous_depth = 0usize;
    let mut previous_began_closing = false;

    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            // A string or comment is content: the line is no longer blank
            // unless the chunk itself ended it.
            line_blank = text.ends_with('\n');
            indent = 0;
            // A *template literal* spans lines, so the line now being emitted
            // began inside it: its content starts at column zero and it opened
            // no bracket. Leaving the previous line's numbers in place here is
            // what made a `.`-continuation after a multi-line literal get
            // measured against a line several above it, and land at a column
            // Koto reads as the end of the argument list.
            if text.contains('\n') {
                line_col = 0;
                previous_depth = depth;
                previous_began_closing = false;
            }
            continue;
        }
        for c in text.chars() {
            if line_blank
                && c == '.'
                && previous_depth > depth
                && !previous_began_closing
                && join_onto_previous(&mut out)
            {
                // Joined: this line *is* the previous one now, so it keeps that
                // line's column and none of the indentation machinery applies.
                //
                // `previous_depth` deliberately stays where it was. The line
                // just joined onto still *ends* a call opened several lines up,
                // so a second `.` line after it needs joining for the same
                // reason the first did — resetting it here left the second one
                // stranded on its own line, which Koto reads as the end of the
                // argument list (`"0 1"\n.chord(…)\n.s(…)` — the shape a tune
                // writes when a chain gets long).
                changed = true;
                out.push(c);
                indent = 0;
                line_blank = false;
                continue;
            }
            if line_blank && !c.is_whitespace() {
                // First non-blank character of the line: settle its column.
                previous_depth = depth;
                previous_began_closing = matches!(c, ')' | ']' | '}');
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
