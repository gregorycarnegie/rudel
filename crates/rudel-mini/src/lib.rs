//! rudel-mini - Strudel mini-notation parser.
//! Parses strings like "bd [hh hh] <sd cp>*2" into rudel-core patterns,
//! mirroring strudel/packages/mini (krill.pegjs grammar + mini.mjs builder).
//! SPDX-License-Identifier: AGPL-3.0-or-later

use pest::Parser;
use pest::iterators::Pair;
use rudel_core::{Frac, Pattern, Value, fastcat, pure, rand, silence, stack, timecat};
use std::sync::Arc;

/// The Pest-based parser for Rudel's mini-notation.
#[derive(pest_derive::Parser)]
#[grammar = "mini.pest"]
struct MiniParser;

/// Strudel offsets each `?`/`|` PRNG stream by `0.0003 * seed` cycles, where
/// `seed` counts those operators left-to-right within one parsed string.
const RAND_OFFSET: f64 = 0.0003;

/// Parse a mini-notation string into a pattern.
pub fn parse(input: &str) -> Result<Pattern, String> {
    let mut pairs = MiniParser::parse(Rule::mini, input).map_err(|e| e.to_string())?;
    let mini = pairs.next().ok_or("empty parse")?;
    let soc = mini
        .into_inner()
        .find(|p| p.as_rule() == Rule::stack_or_choose)
        .ok_or("no pattern")?;
    let mut seed = 0;
    Ok(build_stack_or_choose(soc, &mut seed).pat)
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

// --- AST -> pattern --------------------------------------------------------

/// A built pattern plus the metadata Strudel's mini builder tracks alongside
/// it. `weight` is Strudel's `__weight`: for sequences the sum of element
/// weights (drives `<>` slowdown and `{}` alignment), for elements the
/// `@`/`!` weight (drives `timecat` proportions). `steps_source` is Strudel's
/// `__steps_source`: true when this node or a descendant carries the `^`
/// steps marker, which makes its step count override `_steps` upward.
struct Built {
    pat: Pattern,
    weight: Frac,
    steps_source: bool,
}

impl Built {
    fn plain(pat: Pattern) -> Built {
        Built {
            pat,
            weight: Frac::one(),
            steps_source: false,
        }
    }
}

fn next_seed(seed: &mut i64) -> i64 {
    let s = *seed;
    *seed += 1;
    s
}

/// `rand` shifted earlier by the per-operator seed offset.
fn seeded_rand(seed: i64) -> Pattern {
    rand()._early(Frac::from_f64(RAND_OFFSET * seed as f64))
}

/// lcm of the step counts of `^`-marked children, if any.
fn marked_lcm(children: &[Built]) -> Option<Frac> {
    let mut steps = children
        .iter()
        .filter(|c| c.steps_source)
        .map(|c| c.pat.steps.unwrap_or_else(Frac::one));
    let first = steps.next()?;
    Some(steps.fold(first, |a, b| a.lcm(b)))
}

fn build_stack_or_choose(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let mut inner = pair.into_inner();
    let head = build_sequence(inner.next().expect("stack_or_choose head"), seed);
    let Some(tail) = inner.next() else {
        return head;
    };
    let rule = tail.as_rule();
    let mut children = vec![head];
    for s in tail.into_inner() {
        children.push(build_sequence(s, seed));
    }
    let pats: Vec<Pattern> = children.iter().map(|c| c.pat.clone()).collect();
    let pat = match rule {
        Rule::stack_tail => {
            let mut p = stack(&pats);
            if let Some(l) = marked_lcm(&children) {
                p = p.set_steps(Some(l));
            }
            p
        }
        Rule::choose_tail => {
            let s = next_seed(seed);
            let mut p = choose_in_with(seeded_rand(s).segment(1), pats);
            if let Some(l) = marked_lcm(&children) {
                p = p.set_steps(Some(l));
            }
            p
        }
        // Feet: each foot becomes one step. krill burns a seed for the group.
        _ => {
            next_seed(seed);
            fastcat(&pats)
        }
    };
    Built {
        pat,
        weight: Frac::one(),
        steps_source: children.iter().any(|c| c.steps_source),
    }
}

/// Build a sequence node: a weighted `timecat` of its elements, with
/// `_steps` = weight sum, scaled by the lcm of `^`-marked children.
fn build_sequence(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let mut marked = false;
    let mut elems: Vec<Built> = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::steps_marker => marked = true,
            Rule::slice_with_ops => elems.push(build_slice_with_ops(inner, seed)),
            _ => {}
        }
    }
    let weight_sum = elems.iter().fold(Frac::zero(), |a, e| a + e.weight);
    let pairs: Vec<(Frac, Pattern)> = elems.iter().map(|e| (e.weight, e.pat.clone())).collect();
    let mut steps = weight_sum;
    let child_marked = marked_lcm(&elems);
    if let Some(l) = child_marked {
        steps = weight_sum * l;
    }
    Built {
        pat: timecat(&pairs).set_steps(Some(steps)),
        weight: weight_sum,
        steps_source: marked || child_marked.is_some(),
    }
}

/// One element's pending operator, mirroring krill's `options_.ops`.
enum Op {
    Fast(Pattern),
    Slow(Pattern),
    /// Amount comes from the element's accumulated `reps`.
    Replicate,
    Euclid {
        pulse: Pattern,
        step: Pattern,
        rotation: Option<Pattern>,
    },
    Degrade {
        amount: f64,
        seed: i64,
    },
    Tail(Pattern),
    Range(Pattern),
}

/// Build one element: its slice with all ops applied, plus its weight.
fn build_slice_with_ops(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let mut inner = pair.into_inner();
    let built = build_slice(inner.next().expect("slice"), seed);
    let mut weight = Frac::one();
    let mut reps = Frac::one();
    let mut ops: Vec<Op> = Vec::new();
    for op in inner {
        match op.as_rule() {
            Rule::op_weight => {
                weight = weight + Frac::from_f64(number_in(&op).unwrap_or(2.0)) - Frac::one();
            }
            Rule::op_replicate => {
                // `x!4` and `x ! !` accumulate into one replicate op kept at
                // the end of the op list; the element weight becomes `reps`.
                reps = reps + Frac::from_f64(number_in(&op).unwrap_or(2.0)) - Frac::one();
                weight = reps;
                ops.retain(|o| !matches!(o, Op::Replicate));
                ops.push(Op::Replicate);
            }
            Rule::op_fast => ops.push(Op::Fast(op_slice(op, seed))),
            Rule::op_slow => ops.push(Op::Slow(op_slice(op, seed))),
            Rule::op_degrade => ops.push(Op::Degrade {
                amount: number_in(&op).unwrap_or(0.5),
                seed: next_seed(seed),
            }),
            Rule::op_euclid => {
                let mut args = op.into_inner().map(|a| build_euclid_arg(a, seed));
                let pulse = args.next().expect("euclid pulse");
                let step = args.next().expect("euclid steps");
                let rotation = args.next();
                ops.push(Op::Euclid {
                    pulse,
                    step,
                    rotation,
                });
            }
            Rule::op_tail => ops.push(Op::Tail(op_slice(op, seed))),
            Rule::op_range => ops.push(Op::Range(op_slice(op, seed))),
            _ => {}
        }
    }
    let mut pat = built.pat;
    for op in ops {
        pat = apply_op(pat, op, reps);
    }
    Built {
        pat,
        weight,
        steps_source: built.steps_source,
    }
}

fn apply_op(pat: Pattern, op: Op, reps: Frac) -> Pattern {
    match op {
        Op::Fast(f) => pat.fast(f),
        Op::Slow(f) => pat.slow(f),
        Op::Replicate => {
            // Strudel: pat._repeatCycles(n)._fast(n), preserving `_steps`.
            let steps = pat.steps;
            pat.repeat_cycles(reps.to_f64() as i64)
                ._fast(reps)
                .set_steps(steps)
        }
        Op::Euclid {
            pulse,
            step,
            rotation,
        } => euclid_op(&pat, pulse, step, rotation),
        Op::Degrade { amount, seed } => pat.degrade_by_with(seeded_rand(seed), amount),
        Op::Tail(t) => pat
            .fmap(|a| Value::func(move |b| list_append(a.clone(), b)))
            .app_left(&t),
        Op::Range(t) => range_op(&pat, t),
    }
}

fn build_slice(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let inner = pair.into_inner().next().expect("slice inner");
    match inner.as_rule() {
        Rule::step => build_step(inner),
        Rule::sub_cycle => {
            build_stack_or_choose(inner.into_inner().next().expect("sub_cycle body"), seed)
        }
        Rule::slow_sequence => build_slow_sequence(inner, seed),
        Rule::polymeter => build_polymeter(inner, seed),
        _ => Built::plain(silence()),
    }
}

fn build_step(pair: Pair<Rule>) -> Built {
    Built::plain(match atom_value(pair.as_str()) {
        Some(v) => pure(v),
        None => silence(),
    })
}

/// `<a b c>`: stack of the sequences, each slowed by its own weight so one
/// step plays per cycle.
fn build_slow_sequence(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let poly = pair.into_inner().next().expect("poly_stack");
    let children: Vec<Built> = poly.into_inner().map(|s| build_sequence(s, seed)).collect();
    let slowed: Vec<Pattern> = children
        .iter()
        .map(|c| {
            if c.weight == Frac::zero() {
                c.pat.clone()
            } else {
                c.pat._slow(c.weight)
            }
        })
        .collect();
    let mut pat = stack(&slowed);
    if let Some(l) = marked_lcm(&children) {
        pat = pat.set_steps(Some(l));
    }
    Built {
        pat,
        weight: Frac::one(),
        steps_source: children.iter().any(|c| c.steps_source),
    }
}

/// `{a b, c d e}%n`: stack of the sequences, each sped up so `n` (or the
/// first sequence's) steps fill one cycle.
fn build_polymeter(pair: Pair<Rule>, seed: &mut i64) -> Built {
    let mut inner = pair.into_inner();
    let poly = inner.next().expect("poly_stack");
    let children: Vec<Built> = poly.into_inner().map(|s| build_sequence(s, seed)).collect();
    let steps_pat = inner
        .next()
        .map(|ps| build_slice(ps.into_inner().next().expect("polymeter_steps slice"), seed).pat);
    let aligned: Vec<Pattern> = match steps_pat {
        None => {
            let spc = children
                .first()
                .map(|c| c.weight)
                .unwrap_or_else(Frac::one);
            children
                .iter()
                .map(|c| {
                    if c.weight == Frac::zero() {
                        c.pat.clone()
                    } else {
                        c.pat._fast(spc / c.weight)
                    }
                })
                .collect()
        }
        Some(sp) => children
            .iter()
            .map(|c| {
                if c.weight == Frac::zero() {
                    return c.pat.clone();
                }
                let w = c.weight;
                c.pat.fast(sp.fmap(move |v| Value::Frac(value_frac(&v) / w)))
            })
            .collect(),
    };
    Built {
        pat: stack(&aligned),
        weight: Frac::one(),
        steps_source: children.iter().any(|c| c.steps_source),
    }
}

/// krill's bjorklund args are `slice_with_ops`, but mini.mjs only enters the
/// slice and discards the ops; their seeds are still consumed by the parser.
fn build_euclid_arg(pair: Pair<Rule>, seed: &mut i64) -> Pattern {
    let mut inner = pair.into_inner();
    let pat = build_slice(inner.next().expect("euclid arg slice"), seed).pat;
    for op in inner {
        consume_op_seeds(op, seed);
    }
    pat
}

/// Walk a discarded op purely for its seed side effects.
fn consume_op_seeds(op: Pair<Rule>, seed: &mut i64) {
    match op.as_rule() {
        Rule::op_degrade => {
            next_seed(seed);
        }
        Rule::op_fast | Rule::op_slow | Rule::op_tail | Rule::op_range => {
            op_slice(op, seed);
        }
        Rule::op_euclid => {
            for arg in op.into_inner() {
                build_euclid_arg(arg, seed);
            }
        }
        _ => {}
    }
}

/// `pat(pulse,steps,rot?)` with patterned args, matching Strudel's
/// `register` patternification: fmap the curried call over the pulse
/// pattern, `appLeft` the remaining args, then `innerJoin`.
fn euclid_op(pat: &Pattern, pulse: Pattern, step: Pattern, rotation: Option<Pattern>) -> Pattern {
    match rotation {
        None => {
            let pat = pat.clone();
            pulse
                .fmap(move |p| {
                    let pat = pat.clone();
                    Value::func(move |s| {
                        Value::Pat(Box::new(pat.euclid(value_i64(&p), value_i64(&s))))
                    })
                })
                .app_left(&step)
                .inner_join()
        }
        Some(rot) => {
            let pat = pat.clone();
            pulse
                .fmap(move |p| {
                    let pat = pat.clone();
                    let p = p.clone();
                    Value::func(move |s| {
                        let pat = pat.clone();
                        let p = p.clone();
                        Value::func(move |r| {
                            Value::Pat(Box::new(pat.euclid_rot(
                                value_i64(&p),
                                value_i64(&s),
                                value_i64(&r),
                            )))
                        })
                    })
                })
                .app_left(&step)
                .app_left(&rot)
                .inner_join()
        }
    }
}

/// `a .. b` with patterned bounds: squeeze each `a` over the `b` pattern and
/// expand to a fastcat of the inclusive range (Strudel's range op).
fn range_op(pat: &Pattern, friend: Pattern) -> Pattern {
    pat.squeeze_bind(move |a| {
        let start = a.as_f64().unwrap_or(0.0);
        let friend = friend.clone();
        Value::Pat(Box::new(friend.bind(move |b| {
            let stop = b.as_f64().unwrap_or(0.0);
            let len = ((stop - start).abs() + 1.0).floor() as usize;
            let pats: Vec<Pattern> = (0..len)
                .map(|i| {
                    let x = if start < stop {
                        start + i as f64
                    } else {
                        start - i as f64
                    };
                    pure(num_value(x))
                })
                .collect();
            fastcat(&pats)
        })))
    })
}

/// Strudel's `chooseInWith`: index the list by the chooser signal, taking
/// structure from the chosen patterns.
fn choose_in_with(chooser: Pattern, pats: Vec<Pattern>) -> Pattern {
    if pats.is_empty() {
        return silence();
    }
    let len = pats.len();
    let pats = Arc::new(pats);
    chooser
        .range(0.0, len as f64)
        .fmap(move |v| {
            let i = (v.as_f64().unwrap_or(0.0).floor() as usize).min(len - 1);
            Value::Pat(Box::new(pats[i].clone()))
        })
        .inner_join()
}

fn op_slice(op: Pair<Rule>, seed: &mut i64) -> Pattern {
    let slice = op.into_inner().next().expect("op slice");
    build_slice(slice, seed).pat
}

// --- helpers ---------------------------------------------------------------

fn list_append(a: Value, b: Value) -> Value {
    match a {
        Value::List(mut v) => {
            v.push(b);
            Value::List(v)
        }
        other => Value::List(vec![other, b]),
    }
}

fn number_in(op: &Pair<Rule>) -> Option<f64> {
    op.clone()
        .into_inner()
        .find(|p| p.as_rule() == Rule::number)
        .and_then(|p| p.as_str().parse().ok())
}

fn value_i64(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0) as i64
}

fn value_frac(v: &Value) -> Frac {
    match v {
        Value::Frac(f) => *f,
        other => Frac::from_f64(other.as_f64().unwrap_or(0.0)),
    }
}

fn num_value(x: f64) -> Value {
    if x.fract() == 0.0 && x.is_finite() && x.abs() < 9.007199254740992e15 {
        Value::Int(x as i64)
    } else {
        Value::F64(x)
    }
}

/// Classify one step token like mini.mjs does: `~`/`-` are rests (None),
/// strings JavaScript's `Number()` accepts become numbers, the rest stay
/// strings.
fn atom_value(s: &str) -> Option<Value> {
    if s == "~" || s == "-" {
        return None;
    }
    Some(match js_number(s) {
        Some(x) => num_value(x),
        None => Value::Str(s.to_string()),
    })
}

/// JavaScript `Number()` semantics for the strings the step rule can produce
/// (no `+`, no whitespace): decimal literals with optional exponent,
/// `0x`/`0o`/`0b` radix literals (unsigned only), and `Infinity`.
fn js_number(s: &str) -> Option<f64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let radix = |prefixes: [&str; 2], radix: u32| {
        let digits = prefixes.iter().find_map(|p| body.strip_prefix(p))?;
        if neg {
            return None; // JS rejects signed radix literals
        }
        u64::from_str_radix(digits, radix).ok().map(|n| n as f64)
    };
    let val = if body == "Infinity" {
        Some(f64::INFINITY)
    } else if body.starts_with("0x") || body.starts_with("0X") {
        radix(["0x", "0X"], 16)
    } else if body.starts_with("0b") || body.starts_with("0B") {
        radix(["0b", "0B"], 2)
    } else if body.starts_with("0o") || body.starts_with("0O") {
        radix(["0o", "0O"], 8)
    } else if is_js_decimal(body) {
        body.parse::<f64>().ok()
    } else {
        None
    };
    val.map(|v| if neg { -v } else { v })
}

/// Validate a JS decimal literal: `digits[.digits]` or `.digits`, with an
/// optional exponent. Rust's f64 parser is more permissive (`inf`, `nan`),
/// so validation must happen before parsing.
fn is_js_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let mut exp_digits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            exp_digits += 1;
        }
        if exp_digits == 0 {
            return false;
        }
    }
    i == b.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(src: &str) -> Vec<Value> {
        let pat = parse(src).expect("parse");
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    }

    fn begins(src: &str) -> Vec<Frac> {
        let pat = parse(src).expect("parse");
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.part.begin).collect()
    }

    #[test]
    fn simple_sequence() {
        assert_eq!(
            vals("bd sd hh"),
            vec![
                Value::Str("bd".into()),
                Value::Str("sd".into()),
                Value::Str("hh".into())
            ]
        );
        assert_eq!(
            begins("bd sd hh"),
            vec![Frac::new(0, 3), Frac::new(1, 3), Frac::new(2, 3)]
        );
    }

    #[test]
    fn numbers_parse_as_numbers() {
        assert_eq!(
            vals("0 1 2"),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
    }

    #[test]
    fn js_number_tokens() {
        // mini.mjs classifies atoms with JS Number(): exponents, hex, bare
        // dots all count; everything else stays a string.
        assert_eq!(vals("1e3"), vec![Value::Int(1000)]);
        assert_eq!(vals("0x10"), vec![Value::Int(16)]);
        assert_eq!(vals(".5"), vec![Value::F64(0.5)]);
        assert_eq!(vals("1."), vec![Value::Int(1)]);
        assert_eq!(vals("-3"), vec![Value::Int(-3)]);
        assert_eq!(vals("-x"), vec![Value::Str("-x".into())]);
        assert_eq!(vals("bd.cp"), vec![Value::Str("bd.cp".into())]);
        assert_eq!(vals("a~b"), vec![Value::Str("a~b".into())]);
    }

    #[test]
    fn sub_cycle_groups() {
        // "bd [hh hh]" -> bd at 0..1/2, two hh in the second half
        assert_eq!(
            begins("bd [hh hh]"),
            vec![Frac::new(0, 1), Frac::new(1, 2), Frac::new(3, 4)]
        );
    }

    #[test]
    fn fast_op() {
        // "bd*2" -> two bd
        assert_eq!(begins("bd*2"), vec![Frac::new(0, 1), Frac::new(1, 2)]);
    }

    #[test]
    fn rest_leaves_gap() {
        // "bd ~ sd" -> only bd and sd, at 0 and 2/3
        assert_eq!(
            vals("bd ~ sd"),
            vec![Value::Str("bd".into()), Value::Str("sd".into())]
        );
        assert_eq!(begins("bd ~ sd"), vec![Frac::new(0, 3), Frac::new(2, 3)]);
    }

    #[test]
    fn alternation_one_per_cycle() {
        let pat = parse("<a b c>").unwrap();
        let cyc = |n: i64| {
            pat.query_arc(Frac::int(n), Frac::int(n + 1))[0]
                .value
                .clone()
        };
        assert_eq!(cyc(0), Value::Str("a".into()));
        assert_eq!(cyc(1), Value::Str("b".into()));
        assert_eq!(cyc(2), Value::Str("c".into()));
    }

    #[test]
    fn interval_tokens_stay_strings() {
        // named intervals keep their quality suffix (for transpose), unlike a
        // bare number which still parses as a number.
        assert_eq!(
            vals("3M 5P -2M"),
            vec![
                Value::Str("3M".into()),
                Value::Str("5P".into()),
                Value::Str("-2M".into()),
            ]
        );
        assert_eq!(vals("3"), vec![Value::Int(3)]);
    }

    #[test]
    fn weight_elongates() {
        // "a@3 b" -> a occupies 3/4, b occupies 1/4
        assert_eq!(begins("a@3 b"), vec![Frac::new(0, 1), Frac::new(3, 4)]);
    }

    #[test]
    fn replicate() {
        assert_eq!(
            vals("a!3"),
            vec![
                Value::Str("a".into()),
                Value::Str("a".into()),
                Value::Str("a".into())
            ]
        );
    }

    #[test]
    fn repeated_bare_replicate_accumulates() {
        // "a ! !" == "a!3" (krill folds repeated ! into one op)
        assert_eq!(begins("a ! ! b"), begins("a!3 b"));
    }

    #[test]
    fn euclid_op() {
        // "x(3,8)" -> 3 onsets in 8 steps
        let pat = parse("x(3,8)").unwrap();
        let onsets = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter(|h| h.has_onset())
            .count();
        assert_eq!(onsets, 3);
    }

    #[test]
    fn patterned_euclid() {
        // "a(<3 5>,8)" alternates pulse counts per cycle
        let pat = parse("a(<3 5>,8)").unwrap();
        let onsets = |n: i64| {
            pat.query_arc(Frac::int(n), Frac::int(n + 1))
                .into_iter()
                .filter(|h| h.has_onset())
                .count()
        };
        assert_eq!(onsets(0), 3);
        assert_eq!(onsets(1), 5);
    }

    #[test]
    fn stack_with_comma() {
        let n = parse("a, b c")
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .len();
        assert_eq!(n, 3); // a (whole cycle) + b + c
    }

    #[test]
    fn range_expands() {
        // krill needs whitespace (or a bracket) before "..": "0..3" is one token
        assert_eq!(
            vals("0 .. 3"),
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert_eq!(vals("0..3"), vec![Value::Str("0..3".into())]);
    }

    #[test]
    fn patterned_range() {
        // "<0 1> .. 2" -> 0 1 2 in cycle 0, 1 2 in cycle 1
        let pat = parse("<0 1> .. 2").unwrap();
        let count = |n: i64| pat.query_arc(Frac::int(n), Frac::int(n + 1)).len();
        assert_eq!(count(0), 3);
        assert_eq!(count(1), 2);
    }

    #[test]
    fn tail_makes_list() {
        assert_eq!(
            vals("bd:3"),
            vec![Value::List(vec![Value::Str("bd".into()), Value::Int(3)])]
        );
    }

    #[test]
    fn chord_name_tails_stay_lists() {
        // `c:maj7` / `g:7` keep their chord-symbol tails as list values for
        // `.chord()`/`.voicing()` to read.
        assert_eq!(
            vals("c:maj7"),
            vec![Value::List(vec![
                Value::Str("c".into()),
                Value::Str("maj7".into()),
            ])]
        );
        assert_eq!(
            vals("g:7"),
            vec![Value::List(vec![Value::Str("g".into()), Value::Int(7)])]
        );
    }

    #[test]
    fn non_numeric_tail_preserved() {
        // a non-numeric `:` tail survives as a string element.
        assert_eq!(
            vals("bd:foo"),
            vec![Value::List(vec![
                Value::Str("bd".into()),
                Value::Str("foo".into()),
            ])]
        );
    }

    #[test]
    fn steps_marker_scales_steps() {
        // mini('a [^b c]')._steps == 4 in Strudel
        assert_eq!(
            parse("a [^b c]").unwrap().steps,
            Some(Frac::int(4)),
        );
        assert_eq!(parse("[^b c]!3").unwrap().steps, Some(Frac::int(6)));
        assert_eq!(
            parse("[^a b c] [d [^e f]]").unwrap().steps,
            Some(Frac::int(24)),
        );
    }

    #[test]
    fn install_hook_parses_strings_through_core() {
        // After install(), &str arguments anywhere in rudel-core parse as mini.
        install();
        // note("0 2 4").fast(2) -> 6 events, all {note: ...} maps
        let pat = rudel_core::note("0 2 4").fast(2);
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 6);
        assert!(
            haps.iter()
                .all(|h| matches!(&h.value, Value::Map(m) if m.contains_key("note")))
        );

        // s("bd:3") splits into {s, n} via the list produced by the tail op
        let s = rudel_core::s("bd:3");
        match &s.query_arc(Frac::zero(), Frac::one())[0].value {
            Value::Map(m) => {
                assert_eq!(m.get("s"), Some(&Value::Str("bd".into())));
                assert_eq!(m.get("n"), Some(&Value::Int(3)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }
}
