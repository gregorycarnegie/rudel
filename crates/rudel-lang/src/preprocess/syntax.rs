use super::scanner::{
    Chunk, chunks, classify, code_mask, is_ident_char, is_tagged_template, top_level_ranges,
    top_level_split,
};

/// Drop JavaScript's comments, keeping the newlines they covered so line
/// numbers hold. Strings are chunks of their own, so a `//` inside one is
/// content and survives.
///
/// Koto spells a block comment `#- … -#`, so a `/* … */` left in place is read
/// as code — as a division, and then as whatever follows it. Scripts use one to
/// number the entries of a long list, where the error lands on the list's
/// closing bracket.
pub(super) fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        match kind {
            Chunk::LineComment => {}
            Chunk::BlockComment => {
                // A block comment may span lines; keep them so later passes and
                // error messages still line up with the source.
                out.extend(src[start..end].matches('\n'));
            }
            _ => out.push_str(&src[start..end]),
        }
    }
    out
}

/// Rewrite JavaScript's `**` into Koto's `^`.
///
/// Both raise to a power and both bind tighter than `*`, so the operator is
/// simply respelled. They associate differently — `2 ** 3 ** 2` is 512 in
/// JavaScript and 64 here — which no pattern in the wild leans on; a chain of
/// three is not a shape anyone writes.
pub(super) fn rewrite_exponentiation(src: &str) -> String {
    if !src.contains("**") {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let mut rest = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(rest);
            continue;
        }
        while let Some(at) = rest.find("**") {
            out.push_str(&rest[..at]);
            out.push('^');
            rest = &rest[at + 2..];
        }
        out.push_str(rest);
    }
    out
}

/// Rewrite JavaScript's bitwise shifts into calls: Koto has no `<<` or `>>`.
///
/// `(note / 12) >> 0` is how a script truncates to an integer, and `1 << n` how
/// it builds a bit mask. Both operands are taken as far as the arithmetic
/// around the operator runs — to the enclosing bracket, separator or comparison
/// — because a shift binds looser than everything inside it.
pub(super) fn rewrite_shift_operators(src: &str) -> String {
    let mut current = src.to_string();
    // Each round consumes one operator; the count in the original is the ceiling.
    for _ in 0..src.matches(">>").count() + src.matches("<<").count() {
        let mask = code_mask(&current);
        let Some(at) =
            (0..mask.len().saturating_sub(1)).find(|&i| matches!(&mask[i..i + 2], b">>" | b"<<"))
        else {
            break;
        };
        // `>>>` is JavaScript's unsigned shift. It differs from `>>` only on
        // negative inputs, which a note or a step count is not.
        let width = 2 + usize::from(mask.get(at + 2) == Some(&mask[at]));
        // Leave the blank after whatever the operand follows where it is, so
        // the call does not end up wedged against an `=`.
        let start = shift_operand_start(&mask, at);
        let from = start + mask[start..at].iter().take_while(|b| **b == b' ').count();
        let end = shift_operand_end(&mask, at + width);
        let to = end - mask[..end].iter().rev().take_while(|b| **b == b' ').count();
        let call = if mask[at] == b'>' {
            "rudel_shr"
        } else {
            "rudel_shl"
        };
        current = format!(
            "{}{call}({}, {}){}",
            &current[..from],
            current[from..at].trim(),
            current[at + width..to].trim(),
            &current[to..],
        );
    }
    current
}

/// Anything that cannot be part of a shift's operand: a bracket it sits inside,
/// a separator, or an operator that binds looser than a shift does.
fn ends_shift_operand(byte: u8) -> bool {
    matches!(
        byte,
        b',' | b';' | b'\n' | b'?' | b':' | b'=' | b'<' | b'>' | b'&' | b'|' | b'!'
    )
}

/// The start of the left operand of the shift at `at`.
fn shift_operand_start(mask: &[u8], at: usize) -> usize {
    let mut depth = 0i32;
    for i in (0..at).rev() {
        depth -= bracket_delta(mask[i]);
        if depth < 0 || (depth == 0 && ends_shift_operand(mask[i])) {
            return i + 1;
        }
    }
    0
}

/// Just past the right operand of a shift whose operator ends at `at`.
fn shift_operand_end(mask: &[u8], at: usize) -> usize {
    let mut depth = 0i32;
    for (i, &byte) in mask.iter().enumerate().skip(at) {
        depth += bracket_delta(byte);
        if depth < 0 || (depth == 0 && ends_shift_operand(byte)) {
            return i;
        }
    }
    mask.len()
}

/// Turn the Unicode blanks JavaScript accepts as whitespace into plain spaces.
///
/// A no-break space is whitespace to JavaScript and to Rust's `is_whitespace`,
/// but not to Koto's lexer, which reads it as the start of a token — so a line
/// indented with one is not indented at all and a chain continued on it ends
/// the call above. They arrive by the usual routes: pasted from a browser, or
/// typed on a keyboard layout that produces them.
///
/// Only code: a mini-notation string is the pattern author's text, and its
/// parser has its own idea of what separates a word.
pub(super) fn normalize_unicode_blanks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        out.extend(text.chars().map(|c| {
            if c.is_whitespace() && !matches!(c, '\n' | '\r' | '\t' | ' ') {
                ' '
            } else {
                c
            }
        }));
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

/// Words that are keywords in Koto but ordinary identifiers in JavaScript, so a
/// script may bind one as a name. `if`/`return`/`for` and the rest are keywords
/// on both sides and are deliberately absent: renaming those would rewrite the
/// control flow itself.
const KOTO_ONLY_KEYWORDS: &[&str] = &[
    "and", "as", "debug", "export", "from", "loop", "match", "not", "or", "self", "then", "until",
];

/// Rename a variable whose name is a Koto keyword.
///
/// `const as = register('as', …)` is fine JavaScript and a parse error in Koto,
/// reported as "expected expression" against the assignment — which says nothing
/// about the name being the problem.
///
/// Only names the script *declares* are renamed, found by looking for an
/// assignment to one at the start of a line. That keeps the pass from touching
/// `loop` and `match`, which are also Rudel functions: a bare `loop(…)` call is
/// only renamed if this script also assigned to `loop`, in which case the name
/// really does refer to its own binding. Property access (`.as(…)`) and map keys
/// (`{as: 1}`) are never renamed — they are not identifiers in Koto's grammar.
///
/// Runs before the passes that *introduce* `and`/`or`/`not`/`then`, so it only
/// ever sees words the author wrote.
pub(super) fn rename_koto_keywords(src: &str) -> String {
    let declared: Vec<&&str> = KOTO_ONLY_KEYWORDS
        .iter()
        .filter(|name| {
            src.lines().any(|line| {
                let line = line.trim_start();
                let rest = ["const ", "let ", "var "]
                    .iter()
                    .find_map(|kw| line.strip_prefix(kw))
                    .unwrap_or(line);
                rest.strip_prefix(**name)
                    .and_then(|tail| tail.trim_start().strip_prefix('='))
                    .is_some_and(|tail| !tail.starts_with('='))
            })
        })
        .collect();
    if declared.is_empty() {
        return src.to_string();
    }

    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some(at) = rest.find(|c: char| is_ident_char(c)) {
            out.push_str(&rest[..at]);
            let word = &rest[at..];
            let len = word.find(|c: char| !is_ident_char(c)).unwrap_or(word.len());
            let (name, tail) = word.split_at(len);
            // Not an identifier if it follows a `.`, and not a binding if a `:`
            // makes it a map key.
            let after_dot = out.ends_with('.');
            let is_key = tail.trim_start().starts_with(':');
            out.push_str(name);
            if !after_dot && !is_key && declared.iter().any(|k| ***k == *name) {
                out.push('_');
            }
            rest = tail;
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

/// Rewrite JavaScript's overloaded `+` around a string literal into a call:
///
/// ```text
/// register('mask' + n, …)
/// register(rudel_concat('mask', n), …)
/// ```
///
/// Koto's `+` concatenates two strings and errors on a string and a number,
/// which is what a script building a name, a URL or a sample path out of parts
/// asks it to do. The whole additive chain the literal sits in is replaced at
/// once, because JavaScript folds it left to right — `1 + 2 + 'a'` is `'3a'` —
/// and only `rudel_concat` seeing every operand can reproduce that.
///
/// Only chains containing a literal are touched: a `+` between two values is
/// ordinary arithmetic, or pattern arithmetic, and stays that way.
pub(super) fn rewrite_string_concatenation(src: &str) -> String {
    let mask = code_mask(src);
    let literals = chunks(src);
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    for &(kind, start, end) in &literals {
        if kind != Chunk::Str || start < copied {
            continue;
        }
        let (from, to) = additive_chain(src, &mask, &literals, start, end);
        if (from, to) == (start, end) {
            continue;
        }
        let chain = &src[from..to];
        let operands: Vec<&str> = top_level_ranges(chain, '+')
            .iter()
            .map(|&(a, b)| chain[a..b].trim())
            .collect();
        if operands.len() < 2 {
            continue;
        }
        out.push_str(&src[copied..from]);
        out.push_str("rudel_concat(");
        out.push_str(&operands.join(", "));
        out.push(')');
        copied = to;
    }
    out.push_str(&src[copied..]);
    out
}

/// Grow the literal at `start..end` outwards over the `+`s either side of it,
/// giving the bounds of the whole additive chain.
fn additive_chain(
    src: &str,
    mask: &[u8],
    literals: &[(Chunk, usize, usize)],
    start: usize,
    end: usize,
) -> (usize, usize) {
    let (mut from, mut to) = (start, end);
    loop {
        let plus = skip_blanks(mask, to);
        // `++` is an increment and `+=` an assignment, neither of them a chain.
        if mask.get(plus) != Some(&b'+') || matches!(mask.get(plus + 1), Some(b'+' | b'=')) {
            break;
        }
        let next = operand_end(src, mask, plus + 1);
        if next <= plus + 1 {
            break;
        }
        to = next;
    }
    loop {
        let after = rskip_blanks(mask, from);
        let Some(plus) = after.checked_sub(1) else {
            break;
        };
        // A `+` that is the tail of `+=`, `>=`, `!=` … is not a chain, and one
        // with no operand of its own to the left is a unary sign.
        let left_of_plus = rskip_blanks(mask, plus);
        if mask[plus] != b'+'
            || !left_of_plus
                .checked_sub(1)
                .is_some_and(|end| is_operand_byte(mask[end]) || matches!(mask[end], b')' | b']'))
        {
            break;
        }
        let previous = operand_start(mask, literals, plus);
        if previous >= plus {
            break;
        }
        from = previous;
    }
    (from, to)
}

/// Whether `byte` can be part of a name, a number or a member access.
fn is_operand_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.') || byte >= 0x80
}

fn skip_blanks(mask: &[u8], mut at: usize) -> usize {
    while mask.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

fn rskip_blanks(mask: &[u8], mut at: usize) -> usize {
    while at > 0 && mask[at - 1].is_ascii_whitespace() {
        at -= 1;
    }
    at
}

/// Just past the operand starting at `at`: a name, literal or bracketed group
/// with its member accesses and calls, plus any `*`, `/` or `%` continuation,
/// which binds tighter than the `+` that brought us here.
fn operand_end(src: &str, mask: &[u8], at: usize) -> usize {
    let mut i = skip_blanks(mask, at);
    while matches!(mask.get(i), Some(b'-' | b'!')) {
        i = skip_blanks(mask, i + 1);
    }
    loop {
        // A string is a run of `_` in the mask, so step over it in the source
        // instead — a template literal spanning lines would otherwise be cut.
        if let Some((Chunk::Str, end)) = classify(src, i) {
            i = end;
            continue;
        }
        let bracket = match mask.get(i) {
            Some(b'(') => Some((b'(', b')')),
            Some(b'[') => Some((b'[', b']')),
            _ => None,
        };
        if let Some((open, close)) = bracket {
            let Some(end) = matching_delimiter(mask, i, open, close) else {
                break;
            };
            i = end + 1;
            continue;
        }
        match mask.get(i) {
            Some(&byte) if is_operand_byte(byte) => i += 1,
            _ => break,
        }
    }
    let next = skip_blanks(mask, i);
    if matches!(mask.get(next), Some(b'*' | b'/' | b'%')) && mask.get(next + 1) != Some(&b'=') {
        return operand_end(src, mask, next + 1);
    }
    i
}

/// The start of the operand ending just before the `+` at `at` — `operand_end`
/// walked backwards.
fn operand_start(mask: &[u8], literals: &[(Chunk, usize, usize)], at: usize) -> usize {
    let mut i = rskip_blanks(mask, at);
    loop {
        if let Some(&(_, start, _)) = literals
            .iter()
            .find(|&&(kind, _, end)| kind == Chunk::Str && end == i)
        {
            i = start;
            continue;
        }
        let Some(before) = i.checked_sub(1) else {
            break;
        };
        let bracket = match mask[before] {
            b')' => Some((b'(', b')')),
            b']' => Some((b'[', b']')),
            _ => None,
        };
        if let Some((open, close)) = bracket {
            let Some(start) = opening_delimiter(mask, before, open, close) else {
                break;
            };
            i = start;
            continue;
        }
        if !is_operand_byte(mask[before]) {
            break;
        }
        i = before;
    }
    let previous = rskip_blanks(mask, i);
    match previous.checked_sub(1) {
        Some(op)
            if matches!(mask[op], b'*' | b'/' | b'%')
                && op.checked_sub(1).is_some_and(|b| is_operand_byte(mask[b])) =>
        {
            operand_start(mask, literals, op)
        }
        _ => i,
    }
}

/// The index of the opener matching the closer at `close`, skipping nested
/// pairs — `matching_delimiter` run the other way.
fn opening_delimiter(mask: &[u8], close: usize, opener: u8, closer: u8) -> Option<usize> {
    let mut depth = 0i32;
    for i in (0..=close).rev() {
        if mask[i] == closer {
            depth += 1;
        } else if mask[i] == opener {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The index of the closer matching the `open`th delimiter, skipping nested
/// pairs. `mask` is the code mask, so delimiters inside strings/comments are
/// already blanked out.
fn matching_delimiter(mask: &[u8], open: usize, opener: u8, closer: u8) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &byte) in mask.iter().enumerate().skip(open) {
        if byte == opener {
            depth += 1;
        } else if byte == closer {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Just past the `}` matching the `{` at `open`, skipping nested braces.
fn matching_brace(mask: &[u8], open: usize) -> Option<usize> {
    matching_delimiter(mask, open, b'{', b'}')
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
            // A newline before the `{` of a control arm, or before its `else`,
            // is inside one statement — JavaScript puts the brace on its own
            // line as often as not.
            let next = body[i + 1..].trim_start();
            if byte == b'\n' && (next.starts_with('{') || next.starts_with("else")) {
                continue;
            }
            out.push(&body[start..i]);
            start = i + 1;
        }
    }
    out.push(&body[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Rewrite one JS statement into its Koto spelling: `if (c) x` takes Koto's
/// `then`, and a `return` in tail position is just the value.
/// Split `{ … } rest` into the rendered statements of the braced arm and
/// whatever follows it.
fn split_brace_arm(src: &str) -> (String, &str) {
    let mask = code_mask(src);
    let Some(close) = matching_brace(&mask, 0) else {
        return (src.to_string(), "");
    };
    let arm = body_statements(&src[1..close])
        .iter()
        // No statement in a control arm is in tail position: a `return` inside
        // one returns from the function, not from the arm.
        .map(|stmt| koto_statement(stmt, false))
        .collect::<Vec<_>>()
        .join("\n");
    (arm, &src[close + 1..])
}

/// Push every line of an already-rendered block in by one level.
fn indent_block(block: &str) -> String {
    block
        .lines()
        .map(|line| format!("{}{line}", " ".repeat(CONTINUATION_INDENT)))
        .collect::<Vec<_>>()
        .join("\n")
}

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
                    let cond = rest[1..i].trim();
                    let body = rest[i + 1..].trim_start();
                    // `if (c) { … }` — a braced arm, which Koto writes as an
                    // indented block under the condition rather than after a
                    // `then`. Anything after the arm is the `else`.
                    if body.starts_with('{') {
                        let (arm, after) = split_brace_arm(body);
                        let mut out = format!("if {cond}\n{}", indent_block(&arm));
                        let after = after.trim_start();
                        if let Some(alt) = after.strip_prefix("else") {
                            let alt = alt.trim_start();
                            let alt = if alt.starts_with('{') {
                                split_brace_arm(alt).0
                            } else {
                                koto_statement(alt, false)
                            };
                            out.push_str(&format!("\nelse\n{}", indent_block(&alt)));
                        }
                        return out;
                    }
                    return format!("if {cond} then {}", koto_statement(body, tail));
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
        let pad = " ".repeat(indent + CONTINUATION_INDENT);
        for (i, stmt) in statements.iter().enumerate() {
            // A statement may itself render as a block (an `if` arm), so every
            // line of it moves in together and keeps its own shape.
            for line in koto_statement(stmt, i == last).lines() {
                rendered.push('\n');
                rendered.push_str(&pad);
                rendered.push_str(line);
            }
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
        // `{...v}` and `{ ...v }` are the same literal.
        let spread = |i: usize| {
            let after = &current[i + 1..];
            (mask[i] == b'{')
                .then(|| after.len() - after.trim_start().len())
                .filter(|blanks| after[*blanks..].starts_with("..."))
        };
        let Some((open, blanks)) = (0..mask.len()).find_map(|i| spread(i).map(|b| (i, b))) else {
            break;
        };
        let Some(close) = matching_brace(&mask, open) else {
            break;
        };
        let inner = &current[open + 1 + blanks + 3..close];
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

/// Hap fields a script reads as a property. Deliberately just these two: every
/// other `.name` in a Strudel script is a pattern method, and rewriting one into
/// a field read would break it.
const HAP_FIELDS: &[&str] = &["value", "n"];

/// Read a hap field the way JavaScript does — the value if the receiver is an
/// object, `undefined` if it is not.
///
/// `hap.value` is Strudel's own name for a control map's payload, and helpers
/// branch on it (`const isobj = v.value !== undefined`) to tell a plain note
/// from a map of controls. Koto has no property access and errors on a field
/// read against a string or list, so the test that was meant to *detect* the
/// bare case is the thing that fails on it.
///
/// Only a property read is rewritten: `.value(` is a call and `.values` is a
/// different name.
pub(super) fn rewrite_value_property(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some((at, field)) = HAP_FIELDS
            .iter()
            .filter_map(|field| rest.find(&format!(".{field}")).map(|at| (at, field)))
            .min_by_key(|(at, _)| *at)
        {
            let end = at + 1 + field.len();
            let after = &rest[end..];
            let receiver_start = ident_start(rest, at);
            if after.starts_with('(') || after.starts_with(is_ident_char) || receiver_start == at {
                out.push_str(&rest[..end]);
            } else {
                out.push_str(&rest[..receiver_start]);
                out.push_str(&format!(
                    "rudel_prop({}, '{field}')",
                    &rest[receiver_start..at]
                ));
            }
            rest = after;
        }
        out.push_str(rest);
    }
    out
}

/// The name `this` becomes inside a rewritten prototype method. Koto has `self`,
/// but only for a map's own functions, so the receiver arrives as an ordinary
/// trailing parameter instead — which is also how `register` passes it.
const THIS_PARAM: &str = "rudel_this";

/// Rewrite a `Pattern.prototype` method into a registration.
///
/// Defining a combinator by patching the prototype is how Strudel scripts write
/// one before it lands upstream:
///
/// ```text
/// Pattern.prototype.enumerate = function () { … this.sortHapsByPart() … }
/// rudel_prototype('enumerate', function (rudel_this) { … rudel_this.sortHapsByPart() … })
/// ```
///
/// The receiver becomes a trailing parameter, so the body's `this` is renamed
/// to match. `rudel_prototype` registers without patternifying the arguments,
/// which is what a prototype method gets upstream — `register` would sample a
/// pattern argument per cycle, and a combinator wants the whole thing.
pub(super) fn rewrite_prototype_methods(src: &str) -> String {
    const PREFIX: &str = "Pattern.prototype.";
    let mut current = src.to_string();
    let mut patched = false;
    for _ in 0..src.matches(PREFIX).count() {
        let mask = code_mask(&current);
        let Some(at) = current.find(PREFIX).filter(|&i| mask[i] == b'P') else {
            break;
        };
        let after = &current[at + PREFIX.len()..];
        let name_len = after
            .find(|c: char| !is_ident_char(c))
            .unwrap_or(after.len());
        let name = &after[..name_len];
        // Only an assignment to a `function` is a definition to rewrite.
        let Some(value) = after[name_len..]
            .trim_start()
            .strip_prefix('=')
            .filter(|v| !v.starts_with('='))
            .map(str::trim_start)
            .and_then(|v| v.strip_prefix("function"))
        else {
            break;
        };
        let params = value.trim_start();
        let params_at = current.len() - params.len();
        let Some(params_close) = matching_paren(&code_mask(params), 0) else {
            break;
        };
        let body_at = params_at + params_close + 1;
        let Some(open) = current[body_at..].find('{').map(|i| body_at + i) else {
            break;
        };
        let Some(close) = matching_brace(&code_mask(&current), open) else {
            break;
        };
        let inner = &params[1..params_close];
        let params = if inner.trim().is_empty() {
            THIS_PARAM.to_string()
        } else {
            format!("{inner}, {THIS_PARAM}")
        };
        current = format!(
            "{}rudel_prototype('{name}', function ({params}){}){}",
            &current[..at],
            &current[open..=close],
            &current[close + 1..],
        );
        patched = true;
    }
    if patched {
        rename_this(&current)
    } else {
        current
    }
}

/// Just past the `)` matching the `(` at `open`.
fn matching_paren(mask: &[u8], open: usize) -> Option<usize> {
    matching_delimiter(mask, open, b'(', b')')
}

/// Rename `this` to the parameter the receiver arrives in.
fn rename_this(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some(at) = rest.find("this") {
            let after = &rest[at + 4..];
            let is_word = !rest[..at].ends_with(is_ident_char)
                && !rest[..at].ends_with('.')
                && !after.starts_with(is_ident_char);
            out.push_str(&rest[..at]);
            out.push_str(if is_word { THIS_PARAM } else { "this" });
            rest = after;
        }
        out.push_str(rest);
    }
    out
}

/// Drop JavaScript's `new`: Koto constructs with a plain call, and every
/// constructor a script reaches for (`Pattern`, `Hap`, `Fraction`) is bound as
/// an ordinary function.
pub(super) fn strip_new(src: &str) -> String {
    if !src.contains("new ") {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    for (kind, start, end) in chunks(src) {
        let text = &src[start..end];
        if kind != Chunk::Code {
            out.push_str(text);
            continue;
        }
        let mut rest = text;
        while let Some(at) = rest.find("new ") {
            let before_is_word = rest[..at].ends_with(is_ident_char);
            // `new Foo(` only; `renew x` and `new.target` are not it.
            let constructs = !before_is_word && rest[at + 4..].starts_with(is_ident_char);
            out.push_str(&rest[..at]);
            if !constructs {
                out.push_str("new ");
            }
            rest = &rest[at + 4..];
        }
        out.push_str(rest);
    }
    out
}

/// Rewrite JavaScript's `typeof` operator into a call.
///
/// `typeof v === 'string'` is how a helper asks what kind of thing a hap value
/// is before deciding how to read it. Koto has `koto.type`, but it answers with
/// its own names (`String`, `Map`), so the comparison would silently never
/// match; `rudel_typeof` answers with JavaScript's, which is what the script is
/// comparing against.
///
/// The operand is an identifier or a parenthesised expression — the only two
/// forms in the wild, and the two that need no precedence rules.
pub(super) fn rewrite_typeof(src: &str) -> String {
    if !src.contains("typeof") {
        return src.to_string();
    }
    let mask = code_mask(src);
    let mut out = String::with_capacity(src.len());
    let mut at = 0usize;
    while at < src.len() {
        let Some(found) = src[at..].find("typeof").map(|i| at + i) else {
            break;
        };
        let is_word = mask[found] == b't'
            && !src[..found].ends_with(is_ident_char)
            && !src[found + 6..].starts_with(is_ident_char);
        let operand = src[found + 6..].trim_start();
        let skipped = src.len() - operand.len() - found - 6;
        let len = if operand.starts_with('(') {
            let inner = code_mask(operand);
            let mut depth = 0i32;
            inner
                .iter()
                .position(|&b| {
                    depth += bracket_delta(b);
                    depth == 0
                })
                .map_or(0, |i| i + 1)
        } else {
            operand
                .find(|c: char| !is_ident_char(c))
                .unwrap_or(operand.len())
        };
        if !is_word || len == 0 {
            out.push_str(&src[at..found + 6]);
            at = found + 6;
            continue;
        }
        out.push_str(&src[at..found]);
        let start = found + 6 + skipped;
        out.push_str(&format!("rudel_typeof({})", &src[start..start + len]));
        at = start + len;
    }
    out.push_str(&src[at..]);
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

/// Every Koto keyword. A map key spelled as one has to be quoted, where in
/// JavaScript it needs nothing: `{break: 'x.wav'}` names a sample and
/// `{from: 0, to: 4}` sets a widget's window.
const KOTO_KEYWORDS: &[&str] = &[
    "and", "as", "await", "break", "catch", "const", "continue", "debug", "else", "export",
    "false", "finally", "for", "from", "if", "import", "in", "let", "loop", "match", "not", "null",
    "or", "return", "self", "switch", "then", "throw", "true", "try", "until", "while", "yield",
];

/// Quote a map key Koto will not take bare: a number, or one of its keywords.
///
/// JS object literals are keyed by number constantly — `{0: "...", 1: "..."}`
/// is how songs name the sections a `pickRestart` selects between — and by
/// words Koto has reserved and JavaScript has not. Koto's map declaration takes
/// an identifier or a string, so either is "expected '}' at end of map
/// declaration" pointing at the key itself, which reads as a complaint about
/// the brace instead. Quoting is faithful: JS object keys are strings too, and
/// every lookup matches on the key's text.
///
/// Only a key in key position — right after the `{` or a `,` that opens an
/// entry, and immediately followed by `:` — is touched.
pub(super) fn quote_map_keys(src: &str) -> String {
    if !src.contains('{') {
        return src.to_string();
    }
    let mask = code_mask(src);
    let mut out = String::with_capacity(src.len());
    let mut depth = 0i32;
    // Everything before `copied` is already in `out`. Untouched text is copied
    // in runs rather than a byte at a time, so every slice lands on a char
    // boundary: copying byte-by-byte panicked on any multi-byte character in a
    // source containing a brace.
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < mask.len() {
        let byte = mask[i];
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
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
        let numeric = |b: &u8| b.is_ascii_digit() || matches!(b, b'.' | b'-');
        let mut key_len = mask[key_start..].iter().take_while(|b| numeric(b)).count();
        if key_len == 0 {
            // Not a number: a keyword is the other key Koto refuses.
            key_len = mask[key_start..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count();
            if !KOTO_KEYWORDS.contains(&&src[key_start..key_start + key_len]) {
                continue;
            }
        }
        if mask.get(key_start + key_len) != Some(&b':') {
            continue;
        }
        // The key is ASCII after ASCII whitespace, so both ends of the quoted
        // run are char boundaries.
        out.push_str(&src[copied..key_start]);
        out.push('\'');
        out.push_str(&src[key_start..key_start + key_len]);
        out.push('\'');
        i = key_start + key_len;
        copied = i;
    }
    out.push_str(&src[copied..]);
    out
}

/// Rewrite a call whose arguments contain a JavaScript spread.
///
/// `stack(...xs)` cannot become `stack(xs)`: a list argument is a single
/// sequenced pattern, not several stacked ones, so the spread has to reach
/// runtime. Each argument becomes a *group* — a spread passes through as the
/// list it already is, anything else is wrapped in a one-element list — and
/// `rudel_apply` flattens the groups back into an argument list before calling.
///
/// Only a plain function call is rewritten. Every spread call in the strudel.cc
/// corpus is one (`seq`, `stack`, `timeCat`, `cat`, `arrange`); a method call
/// would need its receiver bound, which is a different job.
pub(super) fn rewrite_spread_calls(src: &str) -> String {
    if !src.contains("...") {
        return src.to_string();
    }
    let mask = code_mask(src);
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < mask.len() {
        if mask[i] != b'(' || i == 0 || !ident(mask[i - 1]) {
            i += 1;
            continue;
        }
        let name_len = mask[..i].iter().rev().take_while(|b| ident(**b)).count();
        let name = &src[i - name_len..i];
        if NOT_A_CALL.contains(&name) {
            i += 1;
            continue;
        }
        let Some(close) = matching_delimiter(&mask, i, b'(', b')') else {
            i += 1;
            continue;
        };
        let args = split_top_level_args(&mask[i + 1..close], i + 1);
        if !args
            .iter()
            .any(|&(from, _)| src[from..].trim_start().starts_with("..."))
        {
            i += 1;
            continue;
        }
        out.push_str(&src[copied..i - name_len]);
        out.push_str("rudel_apply(");
        out.push_str(name);
        out.push_str(", [");
        for (n, &(from, to)) in args.iter().enumerate() {
            if n > 0 {
                out.push_str(", ");
            }
            let arg = src[from..to].trim();
            match arg.strip_prefix("...") {
                Some(spread) => out.push_str(spread.trim()),
                None => {
                    out.push('[');
                    out.push_str(arg);
                    out.push(']');
                }
            }
        }
        out.push_str("])");
        copied = close + 1;
        i = close + 1;
    }
    out.push_str(&src[copied..]);
    out
}

/// Byte ranges of the top-level, comma-separated arguments inside a call, given
/// the masked bytes between its parentheses and where they start in the source.
fn split_top_level_args(inner: &[u8], offset: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, &b) in inner.iter().enumerate() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push((offset + start, offset + i));
                start = i + 1;
            }
            _ => {}
        }
    }
    if inner[start..].iter().any(|b| !b.is_ascii_whitespace()) {
        out.push((offset + start, offset + inner.len()));
    }
    out
}

/// Rename `_name` bindings, which Koto reads as *ignored* values.
///
/// A leading underscore marks a value Koto is told to discard, so a script that
/// merely names a variable that way (`let _drums = ...`, then `stack(_drums)`)
/// fails with `attempting to access an ignored value` — pointing at the use,
/// never at the name. JavaScript has no such rule and the convention is common.
///
/// Only bare identifiers are renamed. A member access (`x._foo`) and a map key
/// are not ignored values, and rudel's own inline-widget spellings (`._spiral`)
/// are reached that way, so leaving those alone keeps them working.
///
/// A lone `_` goes too. It is Koto's real discard, but this reads *JavaScript*,
/// where it is an ordinary name — `every(4, _ => _.shuffle())` is a common
/// spelling — and nothing generated by the passes before this one uses Koto's.
pub(super) fn rename_ignored_identifiers(src: &str) -> String {
    let bytes = src.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    // Only code chunks: `code_mask` blanks a string to underscores, which would
    // read as one enormous ignored identifier.
    for (kind, start, end) in chunks(src) {
        if kind != Chunk::Code {
            continue;
        }
        let mut i = start;
        while i < end {
            if bytes[i] != b'_' {
                i += 1;
                continue;
            }
            // Mid-identifier (`foo_bar`) or a member access: step over the name.
            if i > start && (ident(bytes[i - 1]) || bytes[i - 1] == b'.') {
                while i < end && ident(bytes[i]) {
                    i += 1;
                }
                continue;
            }
            let name_start = i;
            while i < end && ident(bytes[i]) {
                i += 1;
            }
            out.push_str(&src[copied..name_start]);
            out.push_str("rudel_u");
            copied = name_start;
        }
    }
    out.push_str(&src[copied..]);
    out
}

/// Close the gaps around a member-access dot: `x . gain(1)` and `x. gain(1)`.
///
/// JavaScript ignores whitespace either side of the dot; Koto wants the name
/// straight after it and reports `expected key after '.' in Map access` on the
/// method the user did write. Only spaces and tabs are closed, and only when
/// the dot already has something to attach to on its own line — a dot opening a
/// continuation line keeps its indentation, which is what
/// [`indent_dot_continuations`] later lines up.
pub(super) fn tighten_member_dots(src: &str) -> String {
    let mask = code_mask(src);
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    // What a member access can be read off: a name, a closing bracket, or the
    // end of a string literal (`"c e".note()`).
    let receiver = |b: u8| ident(b) || matches!(b, b')' | b']' | b'}' | b'"' | b'\'' | b'`');
    let blank = |b: u8| matches!(b, b' ' | b'\t');
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < mask.len() {
        if mask[i] != b'.' {
            i += 1;
            continue;
        }
        // `..` and `...` are ranges and spreads, not member access.
        if mask.get(i + 1) == Some(&b'.') || (i > 0 && mask[i - 1] == b'.') {
            i += 2;
            continue;
        }
        let after = mask[i + 1..].iter().take_while(|b| blank(**b)).count();
        // A name has to follow, so `gain(.5)` and a decimal point are untouched.
        if after == 0
            || !mask
                .get(i + 1 + after)
                .copied()
                .is_some_and(|b| ident(b) && !b.is_ascii_digit())
        {
            let before = mask[..i].iter().rev().take_while(|b| blank(**b)).count();
            if before == 0
                || !mask[..i - before].last().copied().is_some_and(receiver)
                || !mask
                    .get(i + 1)
                    .copied()
                    .is_some_and(|b| ident(b) && !b.is_ascii_digit())
            {
                i += 1;
                continue;
            }
            out.push_str(&src[copied..i - before]);
            copied = i;
            i += 1;
            continue;
        }
        let before = mask[..i].iter().rev().take_while(|b| blank(**b)).count();
        let keep_from = if before > 0 && mask[..i - before].last().copied().is_some_and(receiver) {
            i - before
        } else {
            i
        };
        out.push_str(&src[copied..keep_from]);
        out.push('.');
        copied = i + 1 + after;
        i = copied;
    }
    out.push_str(&src[copied..]);
    out
}

/// Words whose parenthesised head is not an argument list, so the space before
/// it is real. JavaScript's control flow plus the Koto keywords that can stand
/// in front of a parenthesised expression.
const NOT_A_CALL: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "catch",
    "try",
    "finally",
    "return",
    "typeof",
    "instanceof",
    "new",
    "delete",
    "void",
    "in",
    "of",
    "await",
    "async",
    "function",
    "yield",
    "throw",
    "let",
    "const",
    "var",
    "export",
    "import",
    "default",
    "match",
    "loop",
    "until",
    "then",
    "and",
    "or",
    "not",
    "from",
];

/// Close the gap in `f (x)`.
///
/// JavaScript ignores the space, so `stack (a, b)` is an ordinary call. Koto
/// reads the parentheses as an expression of their own, making that
/// `stack((a, b))` — one tuple argument — so the stacked patterns never reach
/// `stack` and the script ends up with a bare tuple, whose later `.pianoroll()`
/// or `.gain()` then "is not found in 'tuple'". 361 of the strudel.cc patterns
/// space a call this way.
pub(super) fn tighten_call_parens(src: &str) -> String {
    let mask = code_mask(src);
    let bytes = src.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    for i in 0..mask.len() {
        if mask[i] != b'(' {
            continue;
        }
        // Only spaces and tabs: a call split across lines is left alone rather
        // than joined, which is a bigger change than this pass is making.
        let blanks = mask[..i]
            .iter()
            .rev()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
        if blanks == 0 || blanks > i {
            continue;
        }
        let name_end = i - blanks;
        // Read the name from `src`, not the mask: the mask blanks strings to
        // `_`, which would otherwise read as an identifier and tighten
        // `"bd" (x)` into a call on a string.
        if name_end == 0 || !ident(bytes[name_end - 1]) {
            continue;
        }
        let name_len = bytes[..name_end]
            .iter()
            .rev()
            .take_while(|b| ident(**b))
            .count();
        if NOT_A_CALL.contains(&&src[name_end - name_len..name_end]) {
            continue;
        }
        out.push_str(&src[copied..name_end]);
        copied = i;
    }
    out.push_str(&src[copied..]);
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
/// Spell a JavaScript parameter list the way Koto does: an array-destructured
/// parameter unpacks as a tuple (`([c, v]) =>` is `|(c, v)|`), and a rest
/// parameter carries its `...` behind the name (`(...args) =>` is `|args...|`).
///
/// Object destructuring (`({a, b}) =>`) has no Koto spelling and is left as it
/// is, to fail where it is written rather than somewhere else.
fn koto_params(params: &str) -> String {
    if !params.contains('[') && !params.contains("...") {
        return params.to_string();
    }
    top_level_ranges(params, ',')
        .iter()
        .map(|&(from, to)| {
            let param = params[from..to].trim();
            if let Some(name) = param.strip_prefix("...") {
                return format!("{name}...");
            }
            match param.strip_prefix('[').and_then(|p| p.strip_suffix(']')) {
                Some(names) => format!("({names})"),
                None => param.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

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
                        let params: String = out[open_idx + 1..last].iter().collect();
                        let koto = koto_params(&params);
                        if koto != params {
                            out.splice(open_idx + 1..last, koto.chars());
                        }
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
    // `code_mask` blanks string and comment bodies but keeps every newline, so
    // its lines stay aligned with the source's byte for byte.
    let mask = code_mask(src);
    let mask = String::from_utf8_lossy(&mask);
    src.lines()
        .zip(mask.lines())
        .map(|(line, masked)| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            // `let` is a Koto keyword too, but for a *typed* binding
            // (`let x: Number = 1`), so a JS `let parts = {…}` parses as one
            // and the map that follows is read as the type annotation.
            //
            // Only when it is *code*, though. All three keywords are ordinary
            // words inside a string, and all three are WGSL, so a shader body
            // written across several lines hits this pass on nearly every one
            // of them — stripping there produced `a = 1.0;` from `let a = 1.0;`
            // and a shader that would not compile.
            match ["const ", "let ", "var "].iter().find_map(|kw| {
                let is_code = masked.get(indent_len..).is_some_and(|m| m.starts_with(*kw));
                is_code.then(|| rest.strip_prefix(*kw)).flatten()
            }) {
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
            // A newline only ends the branch when the next line is not a
            // continuation of it: `… : dec.toString(2)\n  .padStart(len, '0')`
            // is one expression, and cutting it at the newline hangs the rest
            // of the chain off the whole conditional instead of the branch.
            b'\n' if mask[skip_blanks(mask, i)..].starts_with(b".") => continue,
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
        // The condition's leading blank goes with the trim, so put one back
        // when the text to the left needs separating — `x =(if …` is not an
        // expression Koto will read after an assignment.
        let head = &current[..start];
        let glues =
            head.ends_with(|c: char| !c.is_whitespace() && !matches!(c, '(' | '[' | '{' | ','));
        let space = if glues { " " } else { "" };
        // Koto's inline `if … then … else …` has to be one line, and a branch
        // may be a member chain written down several. Flatten it and hand the
        // newlines back just inside the closing bracket, so the line count —
        // and every error position past here — is left alone.
        let (yes, no) = (
            current[at + 1..colon].trim(),
            current[colon + 1..end].trim(),
        );
        let breaks = "\n".repeat(yes.matches('\n').count() + no.matches('\n').count());
        current = format!(
            "{head}{space}(if {} then {} else {}{breaks}){}",
            current[start..at].trim(),
            yes.replace('\n', " "),
            no.replace('\n', " "),
            &current[end..],
        );
    }
    current
}

/// The first and last significant character of each line, ignoring blanks — a
/// `"` stands in for any string literal and `/` for a comment, so a line that
/// begins or ends inside one is never mistaken for code. Lines with nothing on
/// them get `None`. Shared by the joining passes below.
fn line_edges(src: &str) -> Vec<(Option<char>, Option<char>)> {
    let mut edges = vec![(None, None); src.split('\n').count()];
    let mut line = 0usize;
    for (kind, start, end) in chunks(src) {
        for c in src[start..end].chars() {
            if c == '\n' {
                line += 1;
                continue;
            }
            if c.is_whitespace() {
                continue;
            }
            let c = match kind {
                Chunk::Code => c,
                Chunk::Str => '"',
                _ => '/',
            };
            edges[line].0.get_or_insert(c);
            edges[line].1 = Some(c);
        }
    }
    edges
}

/// Join a line onto the next when JavaScript reads them as one expression and
/// Koto would not.
///
/// Three shapes, all of them ordinary in the wild:
///
/// ```text
/// const fingering =            // a long value below its assignment
/// {o:"x:x:x", g:"3:x:x"}
///
/// register('toscale', (pat) => pat.withValue((v) =>
///   v.endsWith('m') ? [...] : [...]))    // an arrow body below its params
///
/// stack                        // a call whose `(` opens the next line
/// ("<0 1>".pickRestart(...)
/// )
/// ```
///
/// JS does not care where any of them starts; Koto ends the statement at the
/// newline. For `=` that is "expected expression after assignment operator"
/// against a line that looks complete. For `=>` the body becomes an *indented
/// block*, which Koto will not let the enclosing call close on the body's own
/// line (`... 'major']))` — one `)` too many), so the error lands on a paren
/// that is perfectly balanced. For the split call the parentheses become a
/// *tuple*, and the failure surfaces much later as `'cpm' not found in 'tuple'`.
///
/// `==`, `!=`, `<=` and `>=` are comparisons, not assignments, and are left
/// alone — as is a line ending in `=` inside a string or comment, which
/// `line_edges` already distinguishes.
pub(super) fn join_dangling_operators(src: &str) -> String {
    let edges = line_edges(src);
    let mut lines: Vec<String> = src.split('\n').map(str::to_string).collect();
    // Back to front, so joining does not invalidate the indices still to come.
    for i in (0..lines.len().saturating_sub(1)).rev() {
        let tail = edges[i].1;
        let code = lines[i].trim_end().to_string();
        // A `(` opening the next line calls what this one ends with — an
        // identifier, or a bracket that closed a value.
        let split_call = edges.get(i + 1).is_some_and(|(head, _)| *head == Some('('))
            && tail.is_some_and(|c| is_ident_char(c) || c == ')' || c == ']');
        let dangling = match tail {
            Some('>') => code.ends_with("=>"),
            Some('=') => !["==", "<=", ">=", "!="].iter().any(|op| code.ends_with(op)),
            // A `$:` or `name:` alone on its line: the pattern it labels starts
            // below it, and the label rewriter needs the two together or it
            // names an empty expression.
            Some(':') => true,
            // A chain broken after the dot: `.gain(.32).` with `lpf(2500)` on
            // the line below. By here a number's own trailing point is already
            // `72.0`, so a line still ending in one is a member access whose
            // key is on the next line. `..` is a range, not a chain.
            Some('.') => !code.ends_with(".."),
            _ => false,
        };
        if !dangling && !split_call {
            continue;
        }
        // Blank lines between the `=` and its value go with it.
        let Some(value) = (i + 1..lines.len()).find(|&j| !lines[j].trim().is_empty()) else {
            continue;
        };
        let next = lines.drain(i + 1..=value).next_back().unwrap_or_default();
        // A split call is one expression, so its `(` has to land against the
        // name — as does the key after a member dot; the operator forms read
        // better with the blank kept.
        let gap = if split_call || tail == Some('.') {
            ""
        } else {
            " "
        };
        lines[i] = format!("{code}{gap}{}", next.trim_start());
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
    let tails: Vec<Option<char>> = line_edges(src).into_iter().map(|(_, tail)| tail).collect();
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

/// Fold onto one line any bracketed group that spans lines and is *not* the
/// last thing in the list it belongs to.
///
/// Koto takes a nested call whose own arguments run over several lines only in
/// final position. Written anywhere else it ends the outer list, and the error
/// arrives at a closing paren further down:
///
/// ```text
/// stack(pure(1).fast(
///   2),                 <- rejected: another argument follows
///   pure(3).fast(
///     4))               <- accepted: nothing follows
/// ```
///
/// JavaScript does not care where those breaks fall, so the ones Koto cannot
/// read are taken out and the rest of the layout is left alone. A group holding
/// a lambda block is never folded — its lines are the block.
pub(super) fn flatten_non_final_groups(src: &str) -> String {
    let mask = code_mask(src);
    let mut flatten = vec![false; src.len()];
    for open in 0..mask.len() {
        if !matches!(mask[open], b'(' | b'[') {
            continue;
        }
        let mut depth = 0i32;
        let Some(close) = (open..mask.len()).find(|&i| {
            depth += bracket_delta(mask[i]);
            depth == 0
        }) else {
            continue;
        };
        let group = &src[open..=close];
        // A `|` ending a line inside the group opens a block; its lines mean
        // something to Koto and have to stay.
        let has_block = group.lines().any(|line| line.trim_end().ends_with('|'));
        if !group.contains('\n') || has_block {
            continue;
        }
        // Something else follows in the same list only if a `,` comes before
        // the bracket that closes it.
        let mut after = 0i32;
        let followed = mask[close + 1..].iter().find_map(|&byte| match byte {
            b',' if after == 0 => Some(true),
            b')' | b']' | b'}' if after == 0 => Some(false),
            _ => {
                after += bracket_delta(byte);
                None
            }
        });
        // Koto will not take a further argument on a new line once the first
        // one started on the opening line and carried on below it — a shape a
        // tune writes whenever it opens `stack(` with a pattern and puts the
        // rest of the layers underneath.
        let inner = &src[open + 1..close];
        let carries_on = inner
            .split('\n')
            .next()
            .is_some_and(|first| !first.trim().is_empty())
            && top_level_split(inner, ',').is_some();
        if followed == Some(true) || carries_on {
            flatten[open..=close].fill(true);
        }
    }
    // A line break inside a string literal is the author's own text — a
    // template literal's layout, or a `\` continuation — and folding it away
    // rewrites what the pattern says.
    for (kind, start, end) in chunks(src) {
        if kind != Chunk::Code {
            flatten[start..end].fill(false);
        }
    }
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'\n' || !flatten[i] {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        // Take the line break and the indentation after it. A single space
        // stands in, except before punctuation that has to sit against what it
        // follows — Koto reads `) .fast(2)` as a map access, not a chain.
        // Trailing blanks go with the break, or they stand in for it and put
        // the space back in front of a `.` that must not have one.
        while out.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
            out.pop();
        }
        // Blank lines are taken with it too: a comment between two links of a
        // chain leaves one behind, and stopping at it did the same thing.
        i += 1;
        while bytes.get(i).is_some_and(|b| {
            matches!(b, b' ' | b'\t') || (*b == b'\n' && flatten.get(i).is_some_and(|f| *f))
        }) {
            i += 1;
        }
        if !bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'.' | b',' | b')' | b']' | b'}'))
        {
            out.push(b' ');
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Split source into top-level statements, as line ranges. A statement runs
/// from an unindented line that starts one until the next, carrying any
/// continuation with it — indented lines, lines opening with a closing bracket
/// or a `.`, and anything inside brackets it left open.
fn top_level_statements(src: &str) -> Vec<std::ops::Range<usize>> {
    let mask = code_mask(src);
    let lines: Vec<&str> = src.split('\n').collect();
    let mut deltas = vec![0i32; lines.len()];
    let mut line = 0usize;
    for (i, &byte) in mask.iter().enumerate() {
        if src.as_bytes()[i] == b'\n' {
            line += 1;
            continue;
        }
        deltas[line] += bracket_delta(byte);
    }

    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut depth = 0i32;
    for (i, text) in lines.iter().enumerate() {
        let starts = depth <= 0
            && !text.starts_with([' ', '\t'])
            && !text.trim_start().is_empty()
            && !text.trim_start().starts_with(['.', ')', ']', '}', ',']);
        match (starts, out.last_mut()) {
            (false, Some(last)) => last.end = i + 1,
            _ => out.push(i..i + 1),
        }
        depth = (depth + deltas[i]).max(0);
    }
    out
}

/// Move a declaration above the code that uses it.
///
/// JavaScript resolves a name inside a function when the function *runs*, so a
/// helper may be written above the data it reads:
///
/// ```text
/// const guitar = (fingers) => fingers.pickOut(fingering)   // uses it here
/// const fingering = {o: "x:x:x", g: "3:x:x"}               // defines it here
/// ```
///
/// Koto captures at definition instead, so the helper fails with
/// `'fingering' not found` — pointing at the line that reads it, several lines
/// above the one that would have fixed it. Reordering the declarations gives the
/// script the order Koto needs without changing what it means.
///
/// The sort is stable, so anything with no dependency between it and its
/// neighbours stays exactly where it was, and a cycle (two functions calling
/// each other) is left in source order rather than being broken arbitrarily.
pub(super) fn order_declarations(src: &str) -> String {
    let lines: Vec<&str> = src.split('\n').collect();
    let statements = top_level_statements(src);
    // What each statement binds, and every name it mentions.
    let mut defines: Vec<Option<String>> = Vec::new();
    let mut uses: Vec<Vec<String>> = Vec::new();
    for range in &statements {
        let text = lines[range.clone()].join("\n");
        let head = text.trim_start();
        let name_len = head.find(|c: char| !is_ident_char(c)).unwrap_or(head.len());
        let assigns = name_len > 0
            && head[name_len..]
                .trim_start()
                .strip_prefix('=')
                .is_some_and(|rest| !rest.starts_with('='));
        defines.push(assigns.then(|| head[..name_len].to_string()));
        let mut names = Vec::new();
        for (kind, start, end) in chunks(&text) {
            if kind != Chunk::Code {
                continue;
            }
            let mut rest = &text[start..end];
            while let Some(at) = rest.find(is_ident_char) {
                let word = &rest[at..];
                let len = word.find(|c: char| !is_ident_char(c)).unwrap_or(word.len());
                // Skip a name reached through a `.`; it is a method, not a
                // binding this statement depends on.
                if !rest[..at].ends_with('.') {
                    names.push(word[..len].to_string());
                }
                rest = &word[len..];
            }
        }
        uses.push(names);
    }

    let index: std::collections::HashMap<&str, usize> = defines
        .iter()
        .enumerate()
        .filter_map(|(i, name)| name.as_deref().map(|name| (name, i)))
        .collect();
    // Statements this one has to follow.
    let needs: Vec<Vec<usize>> = uses
        .iter()
        .enumerate()
        .map(|(i, names)| {
            let mut deps: Vec<usize> = names
                .iter()
                .filter_map(|name| index.get(name.as_str()).copied())
                .filter(|&j| j != i)
                .collect();
            deps.sort_unstable();
            deps.dedup();
            deps
        })
        .collect();
    if needs
        .iter()
        .enumerate()
        .all(|(i, deps)| deps.iter().all(|&j| j < i))
    {
        return src.to_string();
    }

    // Kahn's algorithm, always taking the earliest ready statement so anything
    // that did not have to move stays put.
    let mut remaining: Vec<bool> = vec![true; statements.len()];
    let mut order: Vec<usize> = Vec::with_capacity(statements.len());
    while order.len() < statements.len() {
        let ready = (0..statements.len())
            .find(|&i| remaining[i] && needs[i].iter().all(|&j| !remaining[j]));
        // No statement is ready: a cycle. Take the earliest one still left and
        // carry on, which leaves the cycle in source order.
        let next = ready.or_else(|| (0..statements.len()).find(|&i| remaining[i]));
        let Some(next) = next else { break };
        remaining[next] = false;
        order.push(next);
    }

    order
        .into_iter()
        .map(|i| lines[statements[i].clone()].join("\n"))
        .collect::<Vec<_>>()
        .join("\n")
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
/// Drop the blank lines JavaScript writes for readability and Koto reads as the
/// end of something.
///
/// Two places they hurt. A line opening with a `.` is always a continuation —
/// no statement can begin with a member access — so a gap above it is noise,
/// usually a commented-out link of the chain that `strip_comments` left as an
/// empty line. And inside brackets Koto ends the argument list at a blank line,
/// so a tune that spaces out the parts of a long `stack(` loses everything
/// below the first gap.
///
/// Removing an empty line never joins two statements, so this cannot change
/// what the source means; a line inside a template literal is content and is
/// left alone.
pub(super) fn close_expression_gaps(src: &str) -> String {
    let mask = code_mask(src);
    let lines: Vec<&str> = src.split('\n').collect();
    let strings: Vec<(usize, usize)> = chunks(src)
        .into_iter()
        .filter(|(kind, ..)| *kind == Chunk::Str)
        .map(|(_, start, end)| (start, end))
        .collect();
    let opens_chain = |from: usize| {
        lines[from..]
            .iter()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| {
                let rest = line.trim_start();
                rest.starts_with('.') && !rest.starts_with("..")
            })
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut at = 0usize;
    let mut depth = 0i32;
    for (i, line) in lines.iter().enumerate() {
        let start = at;
        at += line.len() + 1;
        let inside = depth > 0;
        depth += mask[start..(start + line.len()).min(mask.len())]
            .iter()
            .map(|&b| bracket_delta(b))
            .sum::<i32>();
        // Never the first line: there is no expression above it to continue.
        let quoted = strings.iter().any(|&(from, to)| from < start && to > start);
        if line.trim().is_empty() && i > 0 && !quoted && (inside || opens_chain(i)) {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

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
    // The bracket depth the previous non-blank line began at.
    //
    // A `.` line cannot be indented into place after a line that closed a
    // multi-line call — whether it *ended* with the bracket (`.slow(2))`) or
    // *began* with it (`  ]).note()`). Koto will not carry the chain onto the
    // next line there however far the `.` is pushed, but it does accept the
    // chain written on the closing line itself, so that is what this one gets
    // joined to. Both shapes are common: a tune writes them whenever an
    // argument list runs long enough to close on a line of its own.
    let mut previous_depth = 0usize;
    // Whether the previous line's last significant character was a `,`, which is
    // what makes the line after it a new argument rather than a continuation.
    let mut previous_ended_comma = false;
    let mut last_significant: Option<char> = None;
    // The depth of a lambda block whose body is being emitted, if any. A line
    // ending in `|` closes a parameter list, and the indented lines under it are
    // that function's body rather than more arguments — so they keep their own
    // column while everything else in the call is pulled to Koto's.
    // (depth, column) of the line that opened a lambda block, if one is open.
    let mut block: Option<(usize, usize)> = None;

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
                previous_ended_comma = false;
            }
            // A line ending inside a literal does not end with a comma.
            if !text.trim().is_empty() {
                last_significant = Some('"');
            }
            continue;
        }
        for c in text.chars() {
            if line_blank && c == '.' && previous_depth > depth && join_onto_previous(&mut out) {
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
                // A closing bracket ends the block its body was indented under.
                // The block ends at a closing bracket, at a shallower
                // depth, or at a line no further in than the one that opened it.
                if matches!(c, ')' | ']' | '}')
                    || block.is_some_and(|(d, col)| d > depth || (d == depth && indent <= col))
                {
                    block = None;
                }
                let in_block = block.is_some_and(|(d, _)| d == depth);
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
                        let mut col = indent.max(floor(&bumps));
                        // Koto takes the lines of a bracketed group either all
                        // at one column or all within two of the line that
                        // opened it. A tune indents its arguments to taste and
                        // lands in neither, and the error then names the closing
                        // paren several lines later — so a line is pulled back
                        // here as well as pushed out.
                        //
                        // Only a line that follows a `,` is a new argument. A
                        // lambda's body lines continue the argument above and
                        // keep their own indent; retargeting everything inside a
                        // call instead runs those blocks into the call around
                        // them.
                        if depth > 0
                            && previous_ended_comma
                            && !in_block
                            && let Some(&previous) = stmt.get(depth)
                            // Either way: an argument written *out*dented from
                            // the one above breaks Koto exactly as an indented
                            // one does, and JavaScript indents to taste.
                            && col != previous
                            && previous >= open_col.get(depth).copied().unwrap_or(0)
                        {
                            col = previous;
                        }
                        // JavaScript has no meaningful indentation at the top
                        // level, so a statement written indented there is just
                        // formatting — while Koto reads it as a block belonging
                        // to the line above, and the name being assigned on that
                        // line is then not yet bound when the "block" runs
                        // (`'scala' not found`, on the line that reads it). A
                        // function body is the exception, as inside a call.
                        if depth == 0 && !in_block {
                            col = floor(&bumps);
                        }
                        stmt.truncate(depth);
                        stmt.resize(depth + 1, col);
                        stmt[depth] = col;
                        col
                    }
                };
                if column < indent {
                    // Pull the line back over the blanks just emitted for this
                    // line's own indent. `indent` counts *characters*, and a
                    // blank need not be one byte — `char::is_whitespace` is
                    // Unicode-aware, so a line indented with U+00A0 counts one
                    // per two bytes — hence counting back by characters rather
                    // than subtracting the column difference from the length.
                    let drop = indent - column;
                    let cut = out
                        .char_indices()
                        .rev()
                        .nth(drop - 1)
                        .map_or(0, |(at, _)| at);
                    out.truncate(cut);
                    changed = true;
                }
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
                    // A fresh argument list has no first argument yet.
                    stmt.truncate(depth);
                }
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if c == '\n' {
                // A blank line between two arguments is spacing, not an end:
                // the comma before it is still the last thing that was said.
                if let Some(last) = last_significant {
                    previous_ended_comma = last == ',';
                    // A parameter list closing the line opens a block below it.
                    if last == '|' {
                        block = Some((depth, line_col));
                    }
                }
                last_significant = None;
            } else if !c.is_whitespace() {
                last_significant = Some(c);
            }
            out.push(c);
            line_blank = c == '\n' || (line_blank && c.is_whitespace());
        }
    }

    if changed { out } else { src.to_string() }
}

/// Rewrite JavaScript's decimal literals with an implied zero — `.5`, `-.25`
/// and `72.` — into the form Koto requires, so Strudel snippets paste
/// unchanged.
///
/// A dot *starts* a number only when what precedes it cannot be a value: after
/// an operator, an opening bracket, a comma, or the start of the source. A dot
/// following an identifier, a number, `)`, `]`, or a string is method access
/// (`pat.fast`, `1.5`, `f(x).gain`) and is left alone.
///
/// A dot *ends* one when the run in front of it is all digits and nothing that
/// could be a key follows — `cpm(72.)`, `{steady: 1.}`. Requiring pure digits
/// is what keeps `x1.gain` out of it, and `lpf(1000).` — a chain carrying on
/// below, which [`join_dangling_operators`] folds up instead.
///
/// String literals and comments are skipped.
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
        // Whether the name-or-number run being read is all digits, which is
        // what tells a number's own point from a member access.
        let mut digits: Option<bool> = None;
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
            if c == '.'
                && digits == Some(true)
                && !rest
                    .peek()
                    .is_some_and(|d| d.is_alphanumeric() || matches!(d, '_' | '$' | '.'))
            {
                out.push('0');
            }
            digits = match c {
                _ if c.is_ascii_digit() => Some(digits.unwrap_or(true)),
                '.' => digits,
                _ if c.is_alphanumeric() || matches!(c, '_' | '$') => Some(false),
                _ => None,
            };
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
        // Arguments of a chained call have to clear the chain line above them.
        // The chain does not resume on a line of its own afterwards: Koto will
        // not carry it past a line that closed the argument list, so it is
        // written onto that closing line instead.
        assert_eq!(
            indent_dot_continuations("x\n.sup(\n  |v| v,\n).note()\n.gain(1)"),
            "x\n  .sup(\n    |v| v,\n    ).note().gain(1)"
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
    fn strip_comments_keeps_structure_and_spares_strings() {
        // The comment goes, the newline it sat on stays, so line numbers hold.
        assert_eq!(strip_comments("a // note\nb"), "a \nb");
        assert_eq!(strip_comments("a\n// whole line\nb"), "a\n\nb");
        // A trailing comment with no newline just ends.
        assert_eq!(strip_comments("a // end"), "a ");
        // A block comment goes too — Koto spells one `#- -#` and would read a
        // `/* … */` as code — and gives back only the lines it covered.
        assert_eq!(strip_comments("a /* note */ b"), "a  b");
        assert_eq!(strip_comments("a /* two\nlines */ b"), "a \n b");
        // `//` inside a string is content, not a comment.
        assert_eq!(strip_comments(r#"s("a//b")"#), r#"s("a//b")"#);
        // A URL in a string survives, which is the case users hit first.
        let url = r#"samples("https://example.com/x.json")"#;
        assert_eq!(strip_comments(url), url);
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

    // The indenter is a state machine over columns, and the tune corpus in
    // `tests/tunes.rs` does not pin it down: a tune whose layout is re-indented
    // differently still parses and still produces the same haps, so only the
    // layouts below say what the columns actually have to be.

    #[test]
    fn an_argument_is_pulled_back_to_the_column_of_the_first_one() {
        // Koto takes a bracketed group's lines at one column or all within two
        // of the opening line. A tune indents its arguments to taste, so a
        // later argument is pulled back onto the first one's column.
        assert_eq!(
            indent_dot_continuations("stack(\n  a,\n      b\n)"),
            "stack(\n  a,\n  b\n)"
        );
        // Only a line that follows a `,` is a new argument. A lambda's body
        // continues the argument above and keeps its own indent — pulling those
        // back runs the block into the call around it.
        assert_eq!(
            indent_dot_continuations("f(|v|\n  a,\n      b\n)"),
            "f(|v|\n  a,\n      b\n)"
        );
    }

    #[test]
    fn indentation_at_the_top_level_is_dropped_once_the_block_is_over() {
        // JavaScript's top-level indentation is formatting; Koto reads it as a
        // block belonging to the line above, and the name assigned there is
        // then unbound when the "block" runs.
        assert_eq!(indent_dot_continuations("  x = 1\n  y = 2"), "x = 1\ny = 2");
        // The lambda block ends with the call that opened it, so the line after
        // the `)` is flattened even though the body's lines were not.
        assert_eq!(
            indent_dot_continuations("f(|v|\n  a,\n  b\n)\n  c"),
            "f(|v|\n  a,\n  b\n)\nc"
        );
    }

    #[test]
    fn a_second_dot_line_aligns_with_the_first_instead_of_stepping_right() {
        // Each `.` line at a depth records the column it landed on, and the
        // next one reuses it — stepping further right each time is what Koto
        // rejects.
        assert_eq!(
            indent_dot_continuations("x\n  .a()\n.b()"),
            "x\n  .a()\n  .b()"
        );
        // A line opening with `,` closes off the chain it follows, so it keeps
        // the argument column rather than the continuation's.
        assert_eq!(
            indent_dot_continuations("f(\n  a\n  .b()\n  ,\n  c\n)"),
            "f(\n  a\n    .b()\n  ,\n  c\n)"
        );
    }

    #[test]
    fn a_source_opening_with_an_operator_is_left_alone_not_panicked_on() {
        // Both passes look one byte *left* of a `>` to tell `=>` from a
        // comparison. At the very start of the source there is no such byte,
        // and a half-typed line beginning with `>` is something a live-coder
        // has on screen between keystrokes — it has to come back untouched
        // rather than take the evaluation down.
        assert_eq!(rewrite_block_bodies("> {"), "> {");
        assert_eq!(rewrite_ternaries("> {"), "> {");
        assert_eq!(condition_start(&code_mask("> a ? b : c"), 4), 0);
    }

    #[test]
    fn a_declaration_keyword_inside_a_string_is_left_alone() {
        // `let`/`const`/`var` are all WGSL as well as JS, so a shader body
        // written across several lines meets this pass on nearly every line.
        // Stripping there turns `let a = 1.0;` into `a = 1.0;`, and the shader
        // then fails to compile against a line the user never wrote.
        let src = "s(\"bd\").shader({ code: '
let a = 1.0;
var b = 2.0;
const c = 3.0;
' })";
        assert_eq!(rewrite_const_declarations(src), src);

        // Real declarations outside a string still lose their keyword.
        assert_eq!(rewrite_const_declarations("let x = 1"), "x = 1");
        assert_eq!(rewrite_const_declarations("  const y = 2"), "  y = 2");
        assert_eq!(rewrite_const_declarations("var z = 3"), "z = 3");
    }

    #[test]
    fn a_declaration_moves_above_the_code_that_reads_it() {
        // Koto captures at definition, so a helper written above the data it
        // reads has to be moved below it. Runs after `const` is stripped.
        assert_eq!(
            order_declarations("g = (x) => x.pick(tuning)\ntuning = {a: 1}"),
            "tuning = {a: 1}\ng = (x) => x.pick(tuning)"
        );
        // A chain of them ends up fully reversed.
        assert_eq!(
            order_declarations("a = (x) => b\nb = (x) => c\nc = 1"),
            "c = 1\nb = (x) => c\na = (x) => b"
        );
        // Already in order, so nothing moves...
        assert_eq!(
            order_declarations("a = 1\nb = a\nc = b"),
            "a = 1\nb = a\nc = b"
        );
        // ...and a cycle is left in source order rather than hanging.
        assert_eq!(
            order_declarations("a = (x) => b(x)\nb = (x) => a(x)"),
            "a = (x) => b(x)\nb = (x) => a(x)"
        );
    }

    #[test]
    fn only_the_semicolon_that_ends_a_line_is_dropped() {
        // Koto has no statement separator, but a `;` between two statements on
        // one line is load-bearing — only the trailing one goes.
        assert_eq!(strip_trailing_semicolons("a = 1; b = 2;"), "a = 1; b = 2");
        // A comment after it still leaves the `;` line-final.
        assert_eq!(
            strip_trailing_semicolons("a = 1; // note ;\nb = 2;"),
            "a = 1; // note ;\nb = 2"
        );
    }

    #[test]
    fn a_prototype_body_is_found_past_the_parameters_not_before_them() {
        // The body is the `{` after the parameter list. A map earlier in the
        // source, or a default value carrying its own braces, is not it.
        assert_eq!(
            rewrite_prototype_methods(
                "mmmmmmmmmmmmmmmmmm = {a: 1}; Pattern.prototype.foo = function (b) { this }"
            ),
            "mmmmmmmmmmmmmmmmmm = {a: 1}; rudel_prototype('foo', function (b, rudel_this){ rudel_this })"
        );
        assert_eq!(
            rewrite_prototype_methods("Pattern.prototype.foo = function (b = {c: 1}) { this }"),
            "rudel_prototype('foo', function (b = {c: 1}, rudel_this){ rudel_this })"
        );
    }

    #[test]
    fn a_tagged_template_is_called_but_a_plain_one_is_not() {
        // A backtick string with a tag in front is a call; on its own it is
        // just a literal and wrapping it in brackets changes what it means.
        assert_eq!(
            rewrite_tagged_templates("x = tag`a ${b} c`\ny = `plain ${d}`"),
            "x = tag(`a ${b} c`)\ny = `plain ${d}`"
        );
    }

    #[test]
    fn a_logical_operator_keeps_the_spacing_around_it() {
        // The replacement is measured off the operator's own width, so `&&`
        // becoming `and` must not eat or duplicate the blanks either side.
        assert_eq!(
            rewrite_logical_operators("a && b || c\nx&&y\nq ?? r"),
            "a and b or c\nx and y\nq ?? r"
        );
    }

    #[test]
    fn a_block_body_is_indented_from_its_own_line() {
        // The construct's indent is the line it starts on, not the file's
        // margin — and tabs count as indentation the same as spaces.
        assert_eq!(
            rewrite_block_bodies("x\n  f((v) => { a })"),
            "x\n  f((v) => \n    a\n  )"
        );
        assert_eq!(
            rewrite_block_bodies("\tf((v) => { a })"),
            "\tf((v) => \n   a\n )"
        );
    }

    #[test]
    fn a_group_is_folded_when_its_first_argument_started_on_the_opening_line() {
        // Koto takes `f(x\n  .fast(2)\n)` — one argument, carried on below —
        // but not a second argument after it, which is how a tune writes a
        // `stack(` that opens with a pattern.
        assert_eq!(
            flatten_non_final_groups("f(x\n  .fast(2),\n  y\n)"),
            "f(x.fast(2), y)"
        );
        // A line break inside a string is the author's text: a template
        // literal's layout, or JavaScript's `\` continuation.
        let quoted = "f(`a
b`,
  y
)";
        assert_eq!(
            flatten_non_final_groups(quoted),
            "f(`a
b`, y)"
        );
        // One argument spanning lines is fine as it stands, and so is a list
        // whose arguments all start on their own line.
        assert_eq!(
            flatten_non_final_groups("f(x\n  .fast(2)\n)"),
            "f(x\n  .fast(2)\n)"
        );
        assert_eq!(
            flatten_non_final_groups("f(\n  x,\n  y\n)"),
            "f(\n  x,\n  y\n)"
        );
    }

    #[test]
    fn a_line_edge_tells_a_string_from_a_comment() {
        // The joining passes read these: a line ending in a quote is not a line
        // ending in a comment, and `join_dangling_operators` treats them
        // differently.
        assert_eq!(
            line_edges("x = \"a\"\n'b' + c\n// d"),
            vec![
                (Some('x'), Some('"')),
                (Some('"'), Some('c')),
                (Some('/'), Some('/')),
            ]
        );
    }

    #[test]
    fn a_multi_line_group_is_folded_only_when_something_follows_it() {
        // Koto reads a nested call whose arguments span lines only in final
        // position; anywhere else it ends the outer list. The outer `stack(`
        // is folded too, by the rule in the test below: its first argument
        // started on the opening line and another follows it.
        assert_eq!(
            flatten_non_final_groups("stack(pure(1).fast(\n  2),\n  pure(3).fast(\n    4))"),
            "stack(pure(1).fast( 2), pure(3).fast( 4))"
        );
        // Last in its list, so its layout is left alone.
        assert_eq!(flatten_non_final_groups("f(a(\n  1\n))"), "f(a(\n  1\n))");
        // The `,` that decides this has to be the *list's* — one belonging to a
        // call further along the line says nothing about this group.
        assert_eq!(
            flatten_non_final_groups("f(a(\n  1\n) + g(1, 2))"),
            "f(a(\n  1\n) + g(1, 2))"
        );
        // Every group in a list gets the same treatment, and `[` counts.
        assert_eq!(
            flatten_non_final_groups("f(g(\n  1\n), h(2,\n  3), 4)"),
            "f(g( 1), h(2, 3), 4)"
        );
        assert_eq!(flatten_non_final_groups("[a(\n  1\n), b]"), "[a( 1), b]");
    }

    #[test]
    fn the_bracket_that_ends_the_list_ends_the_search_for_a_comma() {
        // The group is last in *its* list; the `,` further along belongs to a
        // call outside it, and the bracket closing the list is what says so.
        assert_eq!(
            flatten_non_final_groups("f(g(\n  1\n)) + h(1, 2)"),
            "f(g(\n  1\n)) + h(1, 2)"
        );
        // ...but a bracket closing something *inside* the list is not that
        // bracket, so the `,` after it still counts.
        assert_eq!(
            flatten_non_final_groups("f(g(\n  1\n).h(a), 2)"),
            "f(g( 1).h(a), 2)"
        );
    }

    #[test]
    fn a_group_holding_a_lambda_block_keeps_its_lines() {
        // The block's lines are its body — folding them onto one line loses the
        // block, and Koto has nowhere to put the statements.
        assert_eq!(
            flatten_non_final_groups("f(g(|v|\n  v\n), 2)"),
            "f(g(|v|\n  v\n), 2)"
        );
    }

    #[test]
    fn a_lambda_block_ends_where_the_lines_stop_being_its_body() {
        // The body ends at a line no further in than the `|…|` that opened it,
        // so the argument after it is an argument again and gets pulled back.
        assert_eq!(
            indent_dot_continuations("g(\n  f(|v|\n    a,\n  b,\n      c\n  )\n)"),
            "g(\n  f(|v|\n    a,\n    b,\n    c\n  )\n)"
        );
        // ...and at a line outside the brackets it was opened in, even when
        // that line does not start with the closing bracket — otherwise the
        // block is still thought to be open when the *next* call reaches the
        // same depth, and that call's arguments are left alone as if they were
        // a function body.
        assert_eq!(
            indent_dot_continuations("g(\n  f(|v|\n    a),\n  h(\n    x,\n        y\n  )\n)"),
            "g(\n  f(|v|\n    a),\n  h(\n    x,\n    y\n  )\n)"
        );
    }

    #[test]
    fn a_continuation_column_survives_until_its_brackets_close() {
        // The recorded column is reused as-is: a `.` line indented further than
        // the minimum keeps its own column for the rest of the chain rather
        // than being recomputed back to it.
        assert_eq!(
            indent_dot_continuations("x\n     .a()\n.b()"),
            "x\n     .a()\n     .b()"
        );
        // Lines inside a continuation sit two columns past it — a floor that is
        // added to the column, not scaled by it, which only an odd column can
        // tell apart.
        assert_eq!(
            indent_dot_continuations("f(\n   a\n   .b(\n     c\n   )\n)"),
            "f(\n   a\n     .b(\n       c\n       )\n)"
        );
        // A line opening with a closing bracket is not a new argument, so the
        // pull-back that follows a `,` does not apply to it.
        assert_eq!(
            indent_dot_continuations("f(\n  a,\n      )"),
            "f(\n  a,\n      )"
        );
    }

    #[test]
    fn a_line_ending_in_a_string_did_not_end_in_a_comma() {
        // The `,` is not the last thing said on the line — the string is — so
        // the line below continues the argument instead of starting one, and
        // keeps its own indent.
        assert_eq!(
            indent_dot_continuations("f(\n  a, \"x\"\n      b\n)"),
            "f(\n  a, \"x\"\n      b\n)"
        );
    }

    #[test]
    fn a_dot_after_a_closing_bracket_is_joined_onto_it() {
        // Koto will not carry a chain past a line that closed a multi-line
        // call, however far the `.` is pushed — but it accepts the chain
        // written on the closing line itself.
        assert_eq!(
            indent_dot_continuations("f(a,\n  b)\n.c()"),
            "f(a,\n  b).c()"
        );
        assert_eq!(
            indent_dot_continuations("x = 1\nf(\n  a,\n  b\n)\n.c()"),
            "x = 1\nf(\n  a,\n  b\n).c()"
        );
        // The same when the closing bracket *begins* the line.
        assert_eq!(
            indent_dot_continuations("f(\n  a\n  .b()\n  )\n.c()"),
            "f(\n  a\n    .b()\n  ).c()"
        );
        // A blank line between the two means the `.` is not joined on — it is
        // only indented, since there is more than one newline to swallow.
        assert_eq!(indent_dot_continuations("f(a)\n\n.c()"), "f(a)\n\n  .c()");
    }

    #[test]
    fn a_line_ending_in_an_operator_takes_the_next_line_with_it() {
        // `=`, `=>` and a bare label each leave the line unfinished.
        assert_eq!(join_dangling_operators("x =\n  1"), "x = 1");
        assert_eq!(join_dangling_operators("f = x =>\n  x"), "f = x => x");
        assert_eq!(join_dangling_operators("$:\n  s(\"bd\")"), "$: s(\"bd\")");
        // Blank lines between the two go with the join.
        assert_eq!(join_dangling_operators("x =\n\n  1"), "x = 1");
        // A comparison is complete as it stands.
        for op in ["==", "!=", "<=", ">="] {
            let src = format!("a {op}\nb");
            assert_eq!(join_dangling_operators(&src), src, "{op}");
        }
    }

    #[test]
    fn a_call_split_before_its_bracket_is_pulled_back_together() {
        // The `(` has to land against what it calls, with no gap...
        assert_eq!(join_dangling_operators("stack\n(a)"), "stack(a)");
        assert_eq!(join_dangling_operators("f(x)\n(a)"), "f(x)(a)");
        assert_eq!(join_dangling_operators("[x]\n(a)"), "[x](a)");
        // ...but a `(` opening a line after something that is not a callee is
        // an ordinary parenthesised expression on its own line.
        assert_eq!(join_dangling_operators("a,\n(b)"), "a,\n(b)");
    }

    #[test]
    fn a_top_level_statement_carries_its_continuations() {
        // One range per statement, as line indices.
        assert_eq!(top_level_statements("a\nb"), vec![0..1, 1..2]);
        // An indented line, a `.` chain and a closing bracket all continue the
        // statement above rather than starting one.
        assert_eq!(top_level_statements("a\n  b"), vec![0..2]);
        assert_eq!(top_level_statements("a\n.b"), vec![0..2]);
        assert_eq!(top_level_statements("f(\n  1\n)"), vec![0..3]);
        // A bracket left open holds the next line in even when that line is
        // flush left and starts with an ordinary character.
        assert_eq!(top_level_statements("f(a,\nb)\nc"), vec![0..2, 2..3]);
        // A blank line belongs to the statement it follows.
        assert_eq!(top_level_statements("a\n\nb"), vec![0..2, 2..3]);
    }

    #[test]
    fn a_spread_becomes_a_copy_plus_overrides() {
        assert_eq!(
            rewrite_object_spreads("{...v, value: r}"),
            "rudel_spread(v, {value: r})"
        );
        // The literal's own spacing, and a spread with nothing after it.
        assert_eq!(
            rewrite_object_spreads("{ ...v, n: 2 }"),
            "rudel_spread(v, {n: 2})"
        );
        assert_eq!(rewrite_object_spreads("{...v}"), "rudel_spread(v, {})");
        // The base runs to the first comma *outside* brackets, so a call's own
        // arguments stay with it.
        assert_eq!(
            rewrite_object_spreads("{...f(a, b), n: 1}"),
            "rudel_spread(f(a, b), {n: 1})"
        );
        // Not a spread, and left alone.
        assert_eq!(rewrite_object_spreads("{a: 1}"), "{a: 1}");
    }

    #[test]
    fn a_hap_field_is_read_as_a_property_only_when_it_is_one() {
        assert_eq!(rewrite_value_property("h.value"), "rudel_prop(h, 'value')");
        assert_eq!(rewrite_value_property("h.n + 1"), "rudel_prop(h, 'n') + 1");
        // A call is a method, and a longer name is a different name.
        assert_eq!(rewrite_value_property("h.value(2)"), "h.value(2)");
        assert_eq!(rewrite_value_property("h.values"), "h.values");
        // With no identifier to the left there is no receiver to pass.
        assert_eq!(rewrite_value_property("f().value"), "f().value");
    }

    #[test]
    fn a_prototype_method_becomes_a_registration_taking_the_receiver() {
        assert_eq!(
            rewrite_prototype_methods("Pattern.prototype.foo = function () { this.x }"),
            "rudel_prototype('foo', function (rudel_this){ rudel_this.x })"
        );
        // Existing parameters keep their place; the receiver goes last.
        assert_eq!(
            rewrite_prototype_methods("Pattern.prototype.foo = function (a) { a }"),
            "rudel_prototype('foo', function (a, rudel_this){ a })"
        );
        // The offsets are into the whole source, so text either side has to
        // survive intact.
        assert_eq!(
            rewrite_prototype_methods("x = 1\nPattern.prototype.foo = function (a = (1)) { a }\ny"),
            "x = 1\nrudel_prototype('foo', function (a = (1), rudel_this){ a })\ny"
        );
        // Only an assignment of a `function` is a definition.
        assert_eq!(
            rewrite_prototype_methods("Pattern.prototype.foo.bar"),
            "Pattern.prototype.foo.bar"
        );
    }

    #[test]
    fn new_is_stripped_only_where_it_constructs() {
        assert_eq!(strip_new("x = new Foo(1)"), "x = Foo(1)");
        // A word ending in `new`, and a `new ` with no constructor after it.
        assert_eq!(strip_new("renew x"), "renew x");
        assert_eq!(strip_new("new (x)"), "new (x)");
    }

    #[test]
    fn typeof_becomes_a_call_on_its_operand() {
        assert_eq!(
            rewrite_typeof("typeof x === 'string'"),
            "rudel_typeof(x) === 'string'"
        );
        // A parenthesised operand is taken whole, brackets and all.
        assert_eq!(
            rewrite_typeof("typeof (a(b)) == 'number'"),
            "rudel_typeof((a(b))) == 'number'"
        );
        // Two of them, so the scan has to resume past the first.
        assert_eq!(
            rewrite_typeof("typeof a + typeof b"),
            "rudel_typeof(a) + rudel_typeof(b)"
        );
        // Part of a longer name, and with no operand to take.
        assert_eq!(rewrite_typeof("mytypeof x"), "mytypeof x");
        assert_eq!(rewrite_typeof("typeof"), "typeof");
    }

    #[test]
    fn a_numeric_map_key_gets_quoted_wherever_the_map_is() {
        assert_eq!(
            quote_map_keys("x = {0: 'a', 1.5: 'b'}"),
            "x = {'0': 'a', '1.5': 'b'}"
        );
        // Nested maps, and a key after an inner map has closed — which needs
        // the depth to come back down by one.
        assert_eq!(
            quote_map_keys("{a: {2: 'x'}, 3: 'y'}"),
            "{a: {'2': 'x'}, '3': 'y'}"
        );
        // A name key is not numeric, and a number that is not a key — the
        // opening line of a block body — is not one either.
        assert_eq!(quote_map_keys("{a: 1}"), "{a: 1}");
        assert_eq!(quote_map_keys("f((v) => { 2 * v })"), "f((v) => { 2 * v })");
    }

    #[test]
    fn a_multi_byte_character_beside_a_brace_is_copied_whole() {
        // Byte-at-a-time copying split these mid-character and panicked. Each
        // shape below reached rudel from a real strudel.cc pattern.
        let nbsp = "s(\"bd\").room(2)\u{a0}// nbsp";
        assert_eq!(quote_map_keys(nbsp), nbsp);
        assert_eq!(quote_map_keys("{ x: 1 }\u{3108}"), "{ x: 1 }\u{3108}");
        assert_eq!(
            quote_map_keys("// \u{2728}\nn(\"0\")"),
            "// \u{2728}\nn(\"0\")"
        );
        // ...and the rewrite itself still happens around them.
        assert_eq!(
            quote_map_keys("pick({ 0: a, 1: b })\u{e9}"),
            "pick({ '0': a, '1': b })\u{e9}"
        );
        assert_eq!(
            quote_map_keys("\u{e9}.pick({ 2: a })"),
            "\u{e9}.pick({ '2': a })"
        );
    }

    #[test]
    fn an_indent_of_multi_byte_blanks_is_pulled_back_whole() {
        // `char::is_whitespace` is Unicode-aware, so U+2006 counts as one
        // column but occupies three bytes; pulling the line back by the column
        // difference used to cut mid-character and panic. Three strudel.cc
        // patterns indent with it.
        let src = "stack(\n\u{2006}\u{2006}s(\"bd\")\n\u{2006}\u{2006}.lpf(700)\n)";
        let out = indent_dot_continuations(src);
        assert!(out.contains("s(\"bd\")"), "{out:?}");
        assert!(out.contains(".lpf(700)"), "{out:?}");
        // A tab and a non-breaking space are the other two shapes that reach it.
        for blank in ["\u{a0}", "\u{3000}", "\u{2028}"] {
            let src = format!("stack(\n{blank}{blank}s(\"bd\")\n{blank}{blank}.lpf(700)\n)");
            assert!(indent_dot_continuations(&src).contains(".lpf(700)"));
        }
    }

    #[test]
    fn a_space_between_a_name_and_its_arguments_is_closed() {
        // Koto reads `stack (a, b)` as `stack((a, b))`, so the arguments arrive
        // as one tuple and the call quietly does nothing.
        assert_eq!(tighten_call_parens("stack (a, b)"), "stack(a, b)");
        assert_eq!(
            tighten_call_parens("s(\"bd\").pianoroll ({})"),
            "s(\"bd\").pianoroll({})"
        );
        assert_eq!(tighten_call_parens("f\t(x)"), "f(x)");
        // Already tight, or nothing before it: unchanged.
        assert_eq!(tighten_call_parens("stack(a, b)"), "stack(a, b)");
        assert_eq!(tighten_call_parens("x = (a + b)"), "x = (a + b)");
        assert_eq!(tighten_call_parens("(a, b)"), "(a, b)");
        // Control flow keeps its space: the parentheses are not arguments.
        assert_eq!(tighten_call_parens("if (x) 1"), "if (x) 1");
        assert_eq!(tighten_call_parens("return (x)"), "return (x)");
        // A call split over lines is left alone rather than joined.
        assert_eq!(tighten_call_parens("stack\n  (a)"), "stack\n  (a)");
        // Strings are not names, and a paren inside one is not a call.
        assert_eq!(tighten_call_parens("\"bd\" (x)"), "\"bd\" (x)");
        assert_eq!(tighten_call_parens("s(\"a (b\")"), "s(\"a (b\")");
    }

    #[test]
    fn whitespace_around_a_member_dot_is_closed() {
        // Koto wants the name straight after the dot and otherwise reports
        // `expected key after '.'` on the method the user did write.
        assert_eq!(
            tighten_member_dots("s(\"bd\"). gain(1)"),
            "s(\"bd\").gain(1)"
        );
        assert_eq!(
            tighten_member_dots("s(\"bd\") .gain(1)"),
            "s(\"bd\").gain(1)"
        );
        assert_eq!(
            tighten_member_dots("s(\"bd\") . gain(1)"),
            "s(\"bd\").gain(1)"
        );
        assert_eq!(tighten_member_dots("f()\t.g()"), "f().g()");
        // Decimals, ranges and spreads are not member access.
        assert_eq!(tighten_member_dots("gain(.5)"), "gain(.5)");
        assert_eq!(tighten_member_dots("n(\"0 .. 3\")"), "n(\"0 .. 3\")");
        assert_eq!(tighten_member_dots("f(...args)"), "f(...args)");
        // A dot opening a continuation line keeps its indentation.
        assert_eq!(
            tighten_member_dots("s(\"bd\")\n  .gain(1)"),
            "s(\"bd\")\n  .gain(1)"
        );
    }

    #[test]
    fn an_argument_written_outdented_from_the_one_above_is_lined_up() {
        // JavaScript indents to taste; Koto wants a bracketed group's lines at
        // one column. Pulling an over-indented argument back was already done,
        // but an argument written *out*dented broke the call just as badly and
        // the error landed on the closing paren lines later.
        let out = indent_dot_continuations("stack(\n    s(\"bd\"),\n  note(\"c\")\n)");
        let cols: Vec<usize> = out
            .lines()
            .filter(|l| l.contains("s(\"bd\")") || l.contains("note(\"c\")"))
            .map(|l| l.len() - l.trim_start().len())
            .collect();
        assert_eq!(cols.len(), 2, "{out:?}");
        assert_eq!(
            cols[0], cols[1],
            "both arguments land on one column: {out:?}"
        );
    }

    #[test]
    fn a_braced_function_body_becomes_an_indented_block() {
        // The arrow keeps its parameters for the next pass; the body's
        // statements move in two columns and the call's `)` closes on a line of
        // its own, which is the only shape Koto accepts here.
        assert_eq!(
            rewrite_block_bodies("p.fmap((v) => { const x = v.n; return x + 1 })"),
            "p.fmap((v) => \n  const x = v.n\n  x + 1\n)"
        );
        // A named `function` binds, an anonymous one is a bare lambda.
        assert_eq!(
            rewrite_block_bodies("function f(a, b) { a + b }"),
            "f = |a, b|\n  a + b\n"
        );
        assert_eq!(
            rewrite_block_bodies("x = function (a) { a }"),
            "x = |a|\n  a\n"
        );
        // A default value in the parameters carries its own brackets, so the
        // scan back to the parameter list has to pair them.
        assert_eq!(
            rewrite_block_bodies("function f(a, b = (1 + 2)) { a }"),
            "f = |a, b = (1 + 2)|\n  a\n"
        );
    }

    #[test]
    fn an_object_literal_is_not_a_function_body() {
        // Nothing to the left says "function", so these braces are a map and
        // must survive untouched — rewriting them silently corrupts a value.
        for src in [
            "x = {a: 1, b: 2}",
            "f({gain: 1})",
            "x = 1\ny = {a: 1}",
            "x = {}",
        ] {
            assert_eq!(rewrite_block_bodies(src), src, "{src}");
        }
    }

    #[test]
    fn a_return_is_dropped_only_in_tail_position() {
        // The last statement is the block's value, so its `return` is noise;
        // an earlier one is a real early exit and keeps the keyword.
        assert_eq!(
            rewrite_block_bodies("f((v) => { return v })"),
            "f((v) => \n  v\n)"
        );
        assert_eq!(
            rewrite_block_bodies("f((v) => { return (v); w })"),
            "f((v) => \n  return (v)\n  w\n)"
        );
        // `returning` merely starts with the keyword.
        assert_eq!(
            rewrite_block_bodies("f((v) => { returning })"),
            "f((v) => \n  returning\n)"
        );
    }

    #[test]
    fn statements_split_on_semicolons_and_newlines_outside_brackets() {
        // A `;` or newline inside a call's brackets is part of one statement,
        // and every line of that statement moves in together.
        assert_eq!(
            rewrite_block_bodies("f((v) => { g(a,\nb); h() })"),
            "f((v) => \n  g(a,\n  b)\n  h()\n)"
        );
        // Splitting that newline off would leave a *second* statement, and
        // only the last one is the block's value — visible here because the
        // `return` is then no longer in tail position and survives.
        assert_eq!(
            rewrite_block_bodies("f((v) => { return g(a,\nb) })"),
            "f((v) => \n  g(a,\n  b)\n)"
        );
        // ...but a newline before a control arm's `{`, or before its `else`,
        // does not end the statement either.
        assert_eq!(
            rewrite_block_bodies("f((v) => { if (v)\n{ a }\nelse\n{ b } })"),
            "f((v) => \n  if v\n    a\n  else\n    b\n)"
        );
    }

    #[test]
    fn an_if_arm_becomes_then_or_a_block_and_keeps_its_else() {
        assert_eq!(
            rewrite_block_bodies("f((v) => { if (v > 1) a; b })"),
            "f((v) => \n  if v > 1 then a\n  b\n)"
        );
        assert_eq!(
            rewrite_block_bodies("f((v) => { if (v) { a } else { b } })"),
            "f((v) => \n  if v\n    a\n  else\n    b\n)"
        );
        // A `return` inside an arm returns from the function, not the arm, so
        // it survives even when the arm is the last statement.
        assert_eq!(
            rewrite_block_bodies("f((v) => { if (v) { return a } })"),
            "f((v) => \n  if v\n    return a\n)"
        );
    }

    #[test]
    fn a_ternary_becomes_a_parenthesised_if_expression() {
        assert_eq!(rewrite_ternaries("a ? b : c"), "(if a then b else c)");
        // Source with no `?` is not touched, and a `?` with no `:` is not a
        // ternary — better to hand it on than to invent an `else` for it.
        assert_eq!(rewrite_ternaries("plain"), "plain");
        assert_eq!(rewrite_ternaries("a ? b"), "a ? b");
    }

    #[test]
    fn the_condition_starts_after_the_thing_that_cannot_be_part_of_it() {
        // Assignment, separators and a newline each end the condition, and the
        // `=` case has to keep the assignment's spacing readable.
        assert_eq!(
            rewrite_ternaries("v = a ? b : c"),
            "v = (if a then b else c)"
        );
        // The space is put back after an `=` however the source spaced it.
        assert_eq!(
            rewrite_ternaries("v =a ? b : c"),
            "v = (if a then b else c)"
        );
        // After a separator or an opening bracket no space is needed.
        assert_eq!(
            rewrite_ternaries("f(x, a ? b : c)"),
            "f(x,(if a then b else c))"
        );
        assert_eq!(rewrite_ternaries("x;a ? b : c"), "x; (if a then b else c)");
        assert_eq!(rewrite_ternaries("x\na ? b : c"), "x\n(if a then b else c)");
        // An operator is not a boundary: `+` binds tighter than `?:`, so the
        // whole sum is the condition.
        assert_eq!(
            rewrite_ternaries("x + a ? b : c"),
            "(if x + a then b else c)"
        );
    }

    #[test]
    fn a_comparison_is_part_of_the_condition_but_an_arrow_is_not() {
        // `>` `<` `>=` `<=` `==` `!=` all belong to the condition...
        for op in [">", "<", ">=", "<=", "==", "!="] {
            assert_eq!(
                rewrite_ternaries(&format!("x {op} 1 ? a : b")),
                format!("(if x {op} 1 then a else b)"),
                "{op} was read as the end of the condition"
            );
        }
        // ...but the `>` of an arrow is not, and neither is a plain `=`.
        assert_eq!(
            rewrite_ternaries("f = x => x ? a : b"),
            "f = x => (if x then a else b)"
        );
    }

    #[test]
    fn brackets_are_skipped_whole_on_the_way_out_of_a_condition() {
        // The `,` inside the call is the condition's, not a boundary: scanning
        // left has to step over the bracketed group as one unit and then still
        // stop at the `=` outside it.
        assert_eq!(
            rewrite_ternaries("z = f(a, b) ? x : y"),
            "z = (if f(a, b) then x else y)"
        );
        assert_eq!(
            rewrite_ternaries("[a, b].includes(c) ? x : y"),
            "(if [a, b].includes(c) then x else y)"
        );
        // An *unclosed* bracket to the left is the enclosing one, so it ends
        // the condition rather than being stepped over.
        assert_eq!(rewrite_ternaries("f(a ? b : c)"), "f((if a then b else c))");
    }

    #[test]
    fn the_else_branch_ends_where_the_expression_does() {
        assert_eq!(
            rewrite_ternaries("[a ? b : c, d]"),
            "[(if a then b else c), d]"
        );
        assert_eq!(rewrite_ternaries("a ? b : c; d"), "(if a then b else c); d");
        assert_eq!(rewrite_ternaries("a ? b : c\nd"), "(if a then b else c)\nd");
        // A closing bracket that was never opened in the branch is the caller's.
        assert_eq!(rewrite_ternaries("f(a ? b : c)"), "f((if a then b else c))");
        // A call in the else branch keeps its own brackets, and what follows
        // the call is still the branch — the `,` and `)` inside it belong to
        // the call, not to the ternary.
        assert_eq!(rewrite_ternaries("a ? b : f(c)"), "(if a then b else f(c))");
        assert_eq!(
            rewrite_ternaries("a ? b : f(c, d) + 1"),
            "(if a then b else f(c, d) + 1)"
        );
    }

    #[test]
    fn a_nested_ternary_pairs_each_colon_with_its_own_question_mark() {
        // Nested in the true branch: the first `:` closes the *inner* one.
        assert_eq!(
            rewrite_ternaries("a ? b ? c : d : e"),
            "(if a then (if b then c else d) else e)"
        );
        // Nested in the else branch, and in the condition.
        assert_eq!(
            rewrite_ternaries("a ? b : c ? d : e"),
            "(if a then b else (if c then d else e))"
        );
        // In the condition, where the source's own brackets stay around the
        // rewrite's — harmless, and cheaper than working out that they are the
        // same pair.
        assert_eq!(
            rewrite_ternaries("(a ? b : c) ? d : e"),
            "(if ((if a then b else c)) then d else e)"
        );
    }

    #[test]
    fn return_keeps_its_keyword_when_the_condition_follows_it() {
        assert_eq!(
            rewrite_ternaries("return a ? b : c"),
            "return (if a then b else c)"
        );
        // The `then`/`else` of an already-rewritten ternary do the same, which
        // is what lets a second round run over the first round's output.
        assert_eq!(
            rewrite_ternaries("x ? y ? 1 : 2 : z ? 3 : 4"),
            "(if x then (if y then 1 else 2) else (if z then 3 else 4))"
        );
        // A name merely *starting* with the keyword is part of the condition.
        assert_eq!(
            rewrite_ternaries("returns ? b : c"),
            "(if returns then b else c)"
        );
    }

    #[test]
    fn a_ternary_inside_a_string_or_comment_is_text() {
        leaves_quoted_and_commented_alone(rewrite_ternaries, "a ? b : c");
    }

    #[test]
    fn a_colon_is_paired_with_its_own_question_mark() {
        // Called directly for the same reason as `else_end` below: the
        // rightmost `?` is always taken first, so these cases never arrive
        // through `rewrite_ternaries`.
        //
        // A `:` inside brackets belongs to whatever the brackets are.
        assert_eq!(matching_colon(&code_mask("a ? f(b:c) : d"), 2), Some(11));
        // A nested ternary's `:` closes the nested one.
        assert_eq!(matching_colon(&code_mask("a ? b ? c : d : e"), 2), Some(14));
        // A `?` with no `:` before the enclosing bracket closes is not a
        // ternary at all.
        assert_eq!(matching_colon(&code_mask("f(a ? b)"), 4), None);
    }

    #[test]
    fn the_else_branch_swallows_a_ternary_nested_in_it() {
        // A `:` that belongs to a nested `?` does not end the branch — the
        // whole `c ? d : e` is the else. Called directly because
        // `rewrite_ternaries` takes the rightmost `?` first, so it never hands
        // `else_end` an unprocessed nested one.
        let src = "a ? b : c ? d : e";
        assert_eq!(else_end(&code_mask(src), 6), src.len());
        // The same branch inside a list still ends at the separator.
        let listed = "[a ? b : c ? d : e, f]";
        assert_eq!(else_end(&code_mask(listed), 7), listed.len() - 4);
        // Each `:` pairs with one `?`, so a colon left over after the nested
        // pair closes is the branch's end.
        assert_eq!(else_end(&code_mask("a ? b : c ? d : e : f"), 6), 18);
    }

    #[test]
    fn the_else_branch_carries_on_over_a_member_chain() {
        // `dec.toString(2)\n  .padStart(…)` is one expression, and stopping at
        // the newline hung the rest of the chain off the whole conditional —
        // so the *true* branch got `.padStart` called on it too.
        // `join_dangling_operators` has already pulled the branch up onto the
        // `:`, which is why the chain below it is what is left to gather.
        assert_eq!(
            rewrite_ternaries("f(s ? [1] : dec.toString(2)\n  .padStart(4)\n)"),
            "f((if s then [1] else dec.toString(2)   .padStart(4)\n)\n)"
        );
        // A newline followed by anything else still ends the branch.
        assert_eq!(rewrite_ternaries("a ? b : c\nd"), "(if a then b else c)\nd");
    }

    #[test]
    fn a_chain_survives_a_comment_between_its_links() {
        // `strip_comments` leaves the commented-out link as a blank line, and
        // Koto will not carry a chain across one. Nothing else can start with a
        // `.`, so the gap is always noise.
        assert_eq!(
            close_expression_gaps("a.b()\n\n.c()"),
            "a.b()\n.c()",
            "a blank line before a `.` line"
        );
        // A blank line before anything else is a real separation and stays, as
        // does one at the very top with no chain above it.
        assert_eq!(close_expression_gaps("a.b()\n\nc()"), "a.b()\n\nc()");
        assert_eq!(close_expression_gaps("\n.b()"), "\n.b()");
        // `..` is a range, not a chain.
        assert_eq!(close_expression_gaps("a\n\n..b"), "a\n\n..b");
    }

    #[test]
    fn flattening_a_group_keeps_a_dot_against_what_it_chains_off() {
        // Both halves of the gap have to go: the indentation of the line being
        // pulled up, and the blanks left at the end of the one above it — plus
        // any blank lines between, which a comment leaves behind.
        assert_eq!(
            flatten_non_final_groups("f(g(a\n  .b()  \n\n  .c()), d)"),
            "f(g(a.b().c()), d)"
        );
    }

    #[test]
    fn a_number_may_end_on_its_point() {
        // `cpm(72.)` and `{steady: 1.}` are ordinary JavaScript.
        assert_eq!(rewrite_leading_dot_numbers("cpm(72.)"), "cpm(72.0)");
        assert_eq!(rewrite_leading_dot_numbers("{s: 1.}"), "{s: 1.0}");
        // Only when the run in front of the point is all digits: a name that
        // ends in one is a member access, and so is a chain broken after the
        // dot, which `join_dangling_operators` folds up instead.
        assert_eq!(rewrite_leading_dot_numbers("x1.gain(2)"), "x1.gain(2)");
        assert_eq!(
            rewrite_leading_dot_numbers("lpf(1).\ngain(2)"),
            "lpf(1).\ngain(2)"
        );
        // And the leading form still works.
        assert_eq!(rewrite_leading_dot_numbers("gain(.5)"), "gain(0.5)");
    }

    #[test]
    fn a_chain_broken_after_the_dot_is_folded_up() {
        assert_eq!(
            join_dangling_operators("s(\"bd\").gain(0.3).\n  lpf(2500)"),
            "s(\"bd\").gain(0.3).lpf(2500)"
        );
    }

    #[test]
    fn a_no_break_space_is_whitespace_here_too() {
        // Koto's lexer reads one as the start of a token, so a line indented
        // with one is not indented at all.
        assert_eq!(normalize_unicode_blanks("a\n\u{a0}\u{a0}.b()"), "a\n  .b()");
        // Not inside a string: that is the pattern author's own text.
        assert_eq!(
            normalize_unicode_blanks("s(\"bd\u{a0}sd\")"),
            "s(\"bd\u{a0}sd\")"
        );
    }

    #[test]
    fn a_lone_underscore_is_a_name_here_not_a_discard() {
        // `every(4, _ => _.shuffle())` is a common spelling, and Koto's own
        // meaning for `_` would make the body unreadable.
        assert_eq!(
            rename_ignored_identifiers("every(4, _ => _.shuffle())"),
            "every(4, rudel_u_ => rudel_u_.shuffle())"
        );
        // A member access is not an ignored value and keeps its name.
        assert_eq!(rename_ignored_identifiers("x._spiral()"), "x._spiral()");
    }

    #[test]
    fn a_parameter_list_is_spelled_the_way_koto_spells_it() {
        assert_eq!(
            rewrite_arrow_functions("([c, v]) => [c, v]"),
            "|(c, v)| [c, v]"
        );
        assert_eq!(
            rewrite_arrow_functions("(...args) => args"),
            "|args...| args"
        );
        assert_eq!(
            rewrite_arrow_functions("(x, ...rest) => rest"),
            "|x, rest...| rest"
        );
        // Ordinary lists are untouched.
        assert_eq!(rewrite_arrow_functions("(a, b) => a"), "|a, b| a");
    }

    #[test]
    fn a_map_key_koto_reserves_is_quoted() {
        // `{break: …}` names a sample and `{from: 0, to: 4}` a widget window;
        // neither word is reserved in JavaScript.
        assert_eq!(
            quote_map_keys("{break: 'x.wav', from: 0, to: 4}"),
            "{'break': 'x.wav', 'from': 0, to: 4}"
        );
        // The numeric form still works, and an ordinary key is left alone.
        assert_eq!(quote_map_keys("{0: a, x: b}"), "{'0': a, x: b}");
    }

    #[test]
    fn javascript_arithmetic_koto_has_no_operator_for() {
        // `**` is `^` here, and binds the same way against `*`.
        assert_eq!(rewrite_exponentiation("1.5 ** 3"), "1.5 ^ 3");
        // Shifts become calls, taking the arithmetic either side with them.
        assert_eq!(
            rewrite_shift_operators("octave = (note / 12) >> 0"),
            "octave = rudel_shr((note / 12), 0)"
        );
        assert_eq!(
            rewrite_shift_operators("m = 1 << n + 1"),
            "m = rudel_shl(1, n + 1)"
        );
        // `>>>` is the unsigned form, which only differs on negatives.
        assert_eq!(rewrite_shift_operators("a >>> 1"), "rudel_shr(a, 1)");
        // The operand stops at a separator and at a looser operator.
        assert_eq!(
            rewrite_shift_operators("f(a >> 1, b)"),
            "f(rudel_shr(a, 1), b)"
        );
        assert_eq!(rewrite_shift_operators("x >> 1 > 2"), "rudel_shr(x, 1) > 2");
    }

    #[test]
    fn a_string_literal_beside_a_plus_becomes_a_concatenation() {
        assert_eq!(
            rewrite_string_concatenation("register('mask'+n, f)"),
            "register(rudel_concat('mask', n), f)"
        );
        // Every operand of the chain goes into the one call, because JavaScript
        // folds left to right and only then does `1 + 2 + 'a'` come out `'3a'`.
        assert_eq!(
            rewrite_string_concatenation("x = 1 + 2 + 'a' + b.c(1) + [d] + (e + f)"),
            "x = rudel_concat(1, 2, 'a', b.c(1), [d], (e + f))"
        );
        // `*` binds tighter than `+`, so its operand travels with it.
        assert_eq!(
            rewrite_string_concatenation("'a' + n * 2 + m / 4 + 'b'"),
            "rudel_concat('a', n * 2, m / 4, 'b')"
        );
        // A `+` with no literal anywhere in the chain is arithmetic, or pattern
        // arithmetic, and is left alone — as is `+=`, which is neither.
        assert_eq!(rewrite_string_concatenation("a + b"), "a + b");
        assert_eq!(rewrite_string_concatenation("s += 'x'"), "s += 'x'");
        assert_eq!(rewrite_string_concatenation("f('a', 'b')"), "f('a', 'b')");
        leaves_quoted_and_commented_alone(rewrite_string_concatenation, "a + 1");
    }
}
