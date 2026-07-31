//! rudel-mini - Strudel mini-notation parser.
//! Parses strings like "bd [hh hh] <sd cp>*2" into rudel-core patterns,
//! mirroring strudel/packages/mini (krill.pegjs grammar + mini.mjs builder).
//! SPDX-License-Identifier: AGPL-3.0-or-later

mod atom;
mod build;

use build::Ctx;
use pest::{Parser, iterators::Pair};
use rudel_core::{Pattern, silence};

/// The Pest-based parser for Rudel's mini-notation.
#[derive(pest_derive::Parser)]
#[grammar = "mini.pest"]
struct MiniParser;

/// Parse a mini-notation string into a pattern. Leaf locations are byte
/// offsets into `input`.
pub fn parse(input: &str) -> Result<Pattern, String> {
    parse_with_offset(input, 0)
}

/// Like [`parse`], shifting every leaf location by `offset` - the position of
/// `input` within the surrounding source code (mirrors Strudel's `m(str,
/// offset)`, used when mini strings are embedded in larger programs).
pub fn parse_with_offset(input: &str, offset: usize) -> Result<Pattern, String> {
    let mut pairs = parse_pairs(input)?;
    let mini = pairs.next().ok_or("empty parse")?;
    let soc = mini
        .into_inner()
        .find(|p| p.as_rule() == Rule::stack_or_choose)
        .ok_or("no pattern")?;
    let mut ctx = Ctx::new(offset);
    Ok(build::build_stack_or_choose(soc, &mut ctx).pat)
}

/// Byte spans of every mini-notation leaf (steps, op arguments, rests) in
/// source order (mirrors Strudel's `getLeafLocations`, which editors use to
/// map tokens to events).
pub fn leaf_locations(input: &str) -> Result<Vec<(usize, usize)>, String> {
    let mut pairs = parse_pairs(input)?;
    let mini = pairs.next().ok_or("empty parse")?;
    let mut locs = Vec::new();
    collect_steps(mini, &mut locs);
    Ok(locs)
}

/// Grouping and operator nesting past this many levels is rejected before the
/// grammar sees it. Every layer of `[]`/`<>`/`{}`/`()` recurses through pest's
/// generated parser, and every chained operator recurses through
/// [`build`], so a pathological string (pasted, or built by a script that
/// generates mini-notation) overflows the stack and kills the process — an
/// abort no `Result` can catch. Real patterns nest a handful deep; 64 is far
/// past anything playable and far below where either recursion runs out.
const MAX_NESTING: usize = 64;

/// Run the grammar over `input`, rejecting absurdly nested strings first.
/// Both public parse entry points go through here so the depth guard can't be
/// bypassed by using one instead of the other.
fn parse_pairs(input: &str) -> Result<pest::iterators::Pairs<'_, Rule>, String> {
    check_nesting(input)?;
    MiniParser::parse(Rule::mini, input).map_err(|e| e.to_string())
}

fn check_nesting(input: &str) -> Result<(), String> {
    let mut groups = 0usize;
    // Chained operators (`a*2*2*2...`) nest the built pattern just like
    // brackets nest the parse tree, so they count against the same budget. The
    // run resets at whitespace or a bracket, where a fresh term starts.
    let mut ops = 0usize;
    let mut deepest = 0usize;
    for b in input.bytes() {
        match b {
            b'[' | b'<' | b'{' | b'(' => {
                groups += 1;
                ops = 0;
            }
            b']' | b'>' | b'}' | b')' => {
                groups = groups.saturating_sub(1);
                ops = 0;
            }
            b'*' | b'/' | b'!' | b'?' | b'@' | b':' | b'%' | b'_' => ops += 1,
            b if b.is_ascii_whitespace() || b == b',' => ops = 0,
            _ => {}
        }
        deepest = deepest.max(groups + ops);
    }
    if deepest > MAX_NESTING {
        return Err(format!(
            "mini-notation nested {deepest} levels deep (max {MAX_NESTING})"
        ));
    }
    Ok(())
}

fn collect_steps(pair: Pair<Rule>, out: &mut Vec<(usize, usize)>) {
    if pair.as_rule() == Rule::step {
        let span = pair.as_span();
        out.push((span.start(), span.end()));
        return;
    }
    for inner in pair.into_inner() {
        collect_steps(inner, out);
    }
}

/// Parse, falling back to silence on error (used as the installed string
/// parser, where a `Pattern` must always be returned).
pub fn parse_or_silence(input: &str) -> Pattern {
    parse(input).unwrap_or_else(|_| silence())
}

/// Install mini-notation as the parser used for all `&str` patterns in
/// rudel-core (mirrors Strudel's `miniAllStrings`).
pub fn install() {
    rudel_core::set_string_parser(parse_or_silence);
}

#[cfg(test)]
mod tests;
