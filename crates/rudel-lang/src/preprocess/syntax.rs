//! The source passes that are about JavaScript rather than Strudel: what the
//! engine would read differently from how a Strudel script means it.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::scanner::{Chunk, chunks, is_tagged_template};

/// Drop JavaScript's comments, keeping the newlines they covered so line
/// numbers hold. Strings are chunks of their own, so a `//` inside one is
/// content and survives.
///
/// The engine reads comments itself; this is for the passes after it. The
/// label rewrite finds where a `name:` expression ends by counting brackets
/// line by line, and a comment carrying on past the expression would be copied
/// into the call it is wrapped in — `// kick)` closing it early.
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

/// Operators that carry an alignment, and the method each is bound as. `mod`
/// is bound as `modulo`, the spelling Rust can use.
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
/// In Strudel `pat.add` is a function whose properties are the aligned
/// variants, so the alignment is reached by a second property access. Here the
/// matrix is bound flat — one method per cell — and the two spellings differ
/// only in that dot.
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
            for (js, bound) in ALIGNED_OPS {
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
                        out.push_str(bound);
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

#[cfg(test)]
mod tests {
    use super::*;

    // Every rewriter here shares the same guard: skip over strings and comments
    // so their contents are copied through untouched. Getting this wrong does
    // not error. It rewrites a mini-notation string or a comment and hands the
    // user back source they did not write.

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
        for (f, snippet) in [
            (strip_await as fn(&str) -> String, "await foo"),
            (rewrite_alignment_getters, "x.add.out(1)"),
        ] {
            leaves_quoted_and_commented_alone(f, snippet);
        }
    }

    #[test]
    fn strip_comments_keeps_structure_and_spares_strings() {
        assert_eq!(strip_comments("a // note\nb"), "a \nb");
        assert_eq!(strip_comments("a\n// whole line\nb"), "a\n\nb");
        // A comment running to the end of the source.
        assert_eq!(strip_comments("a // end"), "a ");
        // Block comments go too, keeping any newlines they covered.
        assert_eq!(strip_comments("a /* note */ b"), "a  b");
        assert_eq!(strip_comments("a /* two\nlines */ b"), "a \n b");
        // A `//` inside a string is content.
        assert_eq!(strip_comments(r#"s("a//b")"#), r#"s("a//b")"#);
        let url = r#"samples('https://example.com/x.json')"#;
        assert_eq!(strip_comments(url), url);
    }

    #[test]
    fn strip_await_only_removes_the_keyword_itself() {
        assert_eq!(strip_await("await foo()"), "foo()");
        assert_eq!(strip_await("x = await bar"), "x = bar");
        // Part of a longer name: not the keyword.
        assert_eq!(strip_await("myawait()"), "myawait()");
        assert_eq!(strip_await("awaited"), "awaited");
        assert_eq!(strip_await("x.await"), "x.await");
        assert_eq!(strip_await("await_thing"), "await_thing");
        // Nothing to do.
        assert_eq!(strip_await("plain source"), "plain source");
    }

    #[test]
    fn a_comment_does_not_stand_in_for_what_came_before_it() {
        // The pass tracks the last *code* character to decide whether it is at
        // a word boundary. A comment is not one, so it must leave that
        // decision untouched rather than answering it with `/`.
        assert_eq!(
            strip_await("x /* c */ await b"),
            "x /* c */ await b",
            "`await` is still inside an identifier boundary"
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
    fn a_tagged_template_is_called_but_a_plain_one_is_not() {
        // A backtick string with a tag in front is a call; on its own it is
        // just a literal and wrapping it in brackets changes what it means.
        assert_eq!(
            rewrite_tagged_templates("x = tag`a ${b} c`\ny = `plain ${d}`"),
            "x = tag(`a ${b} c`)\ny = `plain ${d}`"
        );
    }

    #[test]
    fn an_alignment_getter_becomes_its_flat_method() {
        assert_eq!(rewrite_alignment_getters("x.add.out(1)"), "x.add_out(1)");
        assert_eq!(
            rewrite_alignment_getters("x.mod.squeeze(1)"),
            "x.modulo_squeeze(1)"
        );
        // `in` is the plain operator, and the camelCase spellings normalise.
        assert_eq!(rewrite_alignment_getters("x.add.in(1)"), "x.add(1)");
        assert_eq!(
            rewrite_alignment_getters("x.set.squeezeIn(1)"),
            "x.set_squeeze(1)"
        );
        // Only an alignment that is called: a longer name is something else.
        assert_eq!(
            rewrite_alignment_getters("x.add.outer(1)"),
            "x.add.outer(1)"
        );
        assert_eq!(rewrite_alignment_getters("x.add.out"), "x.add.out");
    }
}
