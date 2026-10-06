//! `K(...)`: put the kabelsalat names in scope inside the expression.
//!
//! A kabelsalat patch is written with bare names — `saw(110).lpf(.5).out()` —
//! and nearly every one of them already means something else in Strudel.
//! `sine`, `saw`, `noise`, `time`, `range`, `clock` and `delay` are all core
//! pattern functions or controls, so the two vocabularies cannot share a scope.
//!
//! Strudel resolves this by never evaluating the expression where it is
//! written: its transpiler plugin stringifies the whole `K(...)` argument and
//! hands the text to superdough, which evaluates it inside kabelsalat's own
//! `localScope`. Rudel evaluates inline, so the scope has to arrive some other
//! way — and the cheapest one is to qualify the names in the source, which is
//! what this pass does:
//!
//! ```text
//! K(saw(110).lpf(sine(1).range(.3,.8)).out())
//! K(Kabel.saw(110).lpf(Kabel.sine(1).range(.3,.8)).out())
//! ```
//!
//! Only names in call position are touched, and only kabelsalat's own. A
//! method call is already unambiguous — `.lpf(...)` can only be a node method —
//! and a name the table does not have is left alone on purpose, so a patch can
//! still reach an outer variable for a cutoff or a tempo.
//!
//! Two more spellings come from upstream's plugin:
//!
//! - `S(x)` marks a Strudel pattern inside the graph, which upstream lifts out
//!   as a `pat[i]` placeholder. Here the marker is redundant — a pattern
//!   reaching an inlet already becomes a `pat` node — so it unwraps to `(x)`.
//! - `sFreq` and `sGate` are the hap's frequency and gate, which upstream
//!   textually substitutes before compiling. They become node calls.
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::scanner::{code_mask, is_ident_char, parse_call, previous_non_ws};
use crate::kabelsalat::{MODULES, TYPES};

/// Qualify kabelsalat names inside every `K(...)` in `src`.
pub(super) fn scope_kabelsalat_calls(src: &str) -> String {
    let mask = code_mask(src);
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    while let Some((open, close)) = find_kabel_call(src, &mask, at) {
        out.push_str(&src[at..open + 1]);
        out.push_str(&scope_expression(
            &src[open + 1..close],
            &mask[open + 1..close],
        ));
        out.push(')');
        at = close + 1;
    }
    out.push_str(&src[at..]);
    out
}

/// The parentheses of the next `K(...)` call at or after `from`, as
/// `(open, close)`. Matches the method form too, since `.K(...)` on a pattern
/// is how a tune usually reaches for one.
fn find_kabel_call(src: &str, mask: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut at = from;
    while let Some(found) = mask[at..].iter().position(|b| *b == b'K') {
        let k = at + found;
        at = k + 1;
        // A lone `K`, not the tail of `TRACK` or the head of `Kabel`.
        if src[..k].ends_with(is_ident_char) || src[k + 1..].starts_with(is_ident_char) {
            continue;
        }
        // Not a map key (`K: ...`) and not a property being defined.
        let Some(paren) = mask[k + 1..].iter().position(|b| !b.is_ascii_whitespace()) else {
            continue;
        };
        let open = k + 1 + paren;
        if mask[open] != b'(' {
            continue;
        }
        let Some(call) = parse_call(src, open) else {
            continue;
        };
        return Some((open, call.close));
    }
    None
}

/// Rewrite one `K(...)` argument region.
fn scope_expression(text: &str, mask: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let Some(found) = mask[at..].iter().position(|b| is_ident_char(*b as char)) else {
            break;
        };
        let start = at + found;
        let len = mask[start..]
            .iter()
            .position(|b| !is_ident_char(*b as char))
            .unwrap_or(mask.len() - start);
        let end = start + len;
        let name = &text[start..end];
        out.push_str(&text[at..start]);
        at = end;

        // A method call is already a node method, and a digit is a number.
        let after_dot = previous_non_ws(text, start) == Some('.');
        if after_dot || name.starts_with(|c: char| c.is_ascii_digit()) {
            out.push_str(name);
            continue;
        }
        match name {
            // `sFreq`/`sGate`: the two per-hap values, as node calls.
            "sFreq" => out.push_str("Kabel.sfreq()"),
            "sGate" => out.push_str("Kabel.sgate()"),
            // kabelsalat's `mouseX`/`mouseY` are values, `cc("mouseX")`.
            "mouseX" | "mouseY" => out.push_str(&format!("Kabel.cc('{name}')")),
            // `S(x)` drops to `(x)`: the parentheses are already there, so
            // erasing the name leaves a grouping expression behind.
            "S" if next_is_paren(mask, end) => {}
            _ if next_is_paren(mask, end) && is_kabelsalat_name(name) => {
                out.push_str("Kabel.");
                out.push_str(name);
            }
            _ => out.push_str(name),
        }
    }
    out.push_str(&text[at..]);
    out
}

fn next_is_paren(mask: &[u8], at: usize) -> bool {
    mask[at..]
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| *b == b'(')
}

/// Whether `name` is one of kabelsalat's own, and so belongs to the graph
/// rather than to the pattern around it.
fn is_kabelsalat_name(name: &str) -> bool {
    // The graph-time markers are not spellings a patch uses.
    const INTERNAL: &[&str] = &["poly", "peek", "pat", "sfreq", "sgate", "src", "output"];
    if INTERNAL.contains(&name) {
        return false;
    }
    TYPES.iter().any(|t| t.name == name) || MODULES.contains(&name)
}
