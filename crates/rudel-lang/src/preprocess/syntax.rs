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
    fn strip_await_spares_strings_and_both_comments() {
        leaves_quoted_and_commented_alone(strip_await, "await foo");
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
}
