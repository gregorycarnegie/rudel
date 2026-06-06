// rudel-mini - Strudel mini-notation parser.
// Parses strings like "bd [hh hh] <sd cp>*2" into rudel-core patterns.
// SPDX-License-Identifier: AGPL-3.0-or-later

use pest::Parser;
use pest::iterators::Pair;
use rudel_core::{Frac, Pattern, Value, fastcat, pure, randcat, silence, stack, timecat};

#[derive(pest_derive::Parser)]
#[grammar = "mini.pest"]
struct MiniParser;

/// Parse a mini-notation string into a pattern.
pub fn parse(input: &str) -> Result<Pattern, String> {
    let mut pairs = MiniParser::parse(Rule::mini, input).map_err(|e| e.to_string())?;
    let mini = pairs.next().ok_or("empty parse")?;
    let soc = mini
        .into_inner()
        .find(|p| p.as_rule() == Rule::stack_or_choose)
        .ok_or("no pattern")?;
    Ok(build_stack_or_choose(soc))
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

fn build_stack_or_choose(pair: Pair<Rule>) -> Pattern {
    let mut inner = pair.into_inner();
    let head = inner.next().expect("stack_or_choose head");
    let head_pat = build_sequence(head).0;

    let Some(tail) = inner.next() else {
        return head_pat;
    };
    let rule = tail.as_rule();
    let mut pats = vec![head_pat];
    for seq in tail.into_inner() {
        pats.push(build_sequence(seq).0);
    }
    match rule {
        Rule::stack_tail => stack(&pats),
        Rule::choose_tail => randcat(&pats),
        Rule::dot_tail => fastcat(&pats), // feet: each foot becomes one step
        _ => stack(&pats),
    }
}

/// Build a sequence, returning the pattern and its step count (sum of weights).
fn build_sequence(pair: Pair<Rule>) -> (Pattern, Frac) {
    let mut elems: Vec<(Frac, Pattern)> = Vec::new();
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::slice_with_ops {
            let (pat, weight, reps) = build_slice_with_ops(inner);
            for _ in 0..reps {
                elems.push((weight, pat.clone()));
            }
        }
    }
    let total = elems.iter().fold(Frac::zero(), |a, (w, _)| a + *w);
    let all_unit = elems.iter().all(|(w, _)| *w == Frac::one());
    let pat = if elems.is_empty() {
        silence()
    } else if all_unit {
        let pats: Vec<Pattern> = elems.iter().map(|(_, p)| p.clone()).collect();
        fastcat(&pats)
    } else {
        timecat(&elems)
    };
    (pat, total)
}

/// Build one element, returning its pattern plus its weight and replication.
fn build_slice_with_ops(pair: Pair<Rule>) -> (Pattern, Frac, usize) {
    let mut inner = pair.into_inner();
    let slice = inner.next().expect("slice");
    let mut pat = build_slice(slice);
    let mut weight = Frac::one();
    let mut reps = 1usize;

    for op in inner {
        match op.as_rule() {
            Rule::op_weight => {
                let a = number_in(&op).unwrap_or(2.0);
                weight = weight + Frac::from_f64(a) - Frac::one();
            }
            Rule::op_replicate => {
                let a = number_in(&op).unwrap_or(2.0);
                reps = (reps as f64 + a - 1.0).max(0.0) as usize;
            }
            Rule::op_fast => pat = pat.fast(build_factor(op)),
            Rule::op_slow => pat = pat.slow(build_factor(op)),
            Rule::op_degrade => pat = pat.degrade_by(number_in(&op).unwrap_or(0.5)),
            Rule::op_euclid => pat = build_euclid(pat, op),
            Rule::op_range => pat = build_range(pat, op),
            Rule::op_tail => pat = build_tail(pat, op),
            _ => {}
        }
    }
    (pat, weight, reps)
}

fn build_slice(pair: Pair<Rule>) -> Pattern {
    let inner = pair.into_inner().next().expect("slice inner");
    match inner.as_rule() {
        Rule::rest => silence(),
        Rule::step => build_step(inner),
        Rule::sub_cycle => {
            build_stack_or_choose(inner.into_inner().next().expect("sub_cycle body"))
        }
        Rule::slow_sequence => build_slow_sequence(inner),
        Rule::polymeter => build_polymeter(inner),
        _ => silence(),
    }
}

fn build_step(pair: Pair<Rule>) -> Pattern {
    let s = pair.as_str();
    if let Ok(i) = s.parse::<i64>() {
        pure(Value::Int(i))
    } else if let Ok(f) = s.parse::<f64>() {
        pure(Value::F64(f))
    } else {
        pure(Value::Str(s.to_string()))
    }
}

fn build_slow_sequence(pair: Pair<Rule>) -> Pattern {
    let poly = pair.into_inner().next().expect("poly_stack");
    let mut pats = Vec::new();
    for seq in poly.into_inner() {
        let (p, steps) = build_sequence(seq);
        // <a b c> = (a b c).slow(3): one element per cycle
        pats.push(if steps == Frac::zero() {
            p
        } else {
            p._slow(steps)
        });
    }
    stack(&pats)
}

fn build_polymeter(pair: Pair<Rule>) -> Pattern {
    let mut inner = pair.into_inner();
    let poly = inner.next().expect("poly_stack");
    let seqs: Vec<(Pattern, Frac)> = poly.into_inner().map(build_sequence).collect();
    let steps_per_cycle = match inner.next() {
        Some(ps) => {
            let slice = ps.into_inner().next().expect("polymeter_steps slice");
            Frac::from_f64(const_f64(&build_slice(slice)))
        }
        None => seqs.first().map(|(_, s)| *s).unwrap_or(Frac::one()),
    };
    let aligned: Vec<Pattern> = seqs
        .iter()
        .map(|(p, l)| {
            if *l == Frac::zero() {
                p.clone()
            } else {
                p._fast(steps_per_cycle / *l)
            }
        })
        .collect();
    stack(&aligned)
}

fn build_euclid(pat: Pattern, op: Pair<Rule>) -> Pattern {
    let args: Vec<Pattern> = op.into_inner().map(|s| build_sequence(s).0).collect();
    if args.len() < 2 {
        return pat;
    }
    let pulses = const_i64(&args[0]);
    let steps = const_i64(&args[1]);
    if args.len() >= 3 {
        pat.euclid_rot(pulses, steps, const_i64(&args[2]))
    } else {
        pat.euclid(pulses, steps)
    }
}

fn build_range(pat: Pattern, op: Pair<Rule>) -> Pattern {
    let slice = op.into_inner().next().expect("range slice");
    let b = const_i64(&build_slice(slice));
    let a = const_i64(&pat);
    let nums: Vec<Pattern> = if a <= b {
        (a..=b).map(|i| pure(Value::Int(i))).collect()
    } else {
        (b..=a).rev().map(|i| pure(Value::Int(i))).collect()
    };
    fastcat(&nums)
}

fn build_tail(pat: Pattern, op: Pair<Rule>) -> Pattern {
    let slice = op.into_inner().next().expect("tail slice");
    let tail_pat = build_slice(slice);
    pat.fmap(|a| Value::func(move |b| list_append(a.clone(), b)))
        .app_left(&tail_pat)
}

fn build_factor(op: Pair<Rule>) -> Pattern {
    let factor = op.into_inner().next().expect("factor");
    let slice = factor.into_inner().next().expect("factor slice");
    build_slice(slice)
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

fn const_f64(pat: &Pattern) -> f64 {
    pat.query_arc(Frac::zero(), Frac::one())
        .first()
        .and_then(|h| h.value.as_f64())
        .unwrap_or(0.0)
}

fn const_i64(pat: &Pattern) -> i64 {
    const_f64(pat) as i64
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
    fn stack_with_comma() {
        let n = parse("a, b c")
            .unwrap()
            .query_arc(Frac::zero(), Frac::one())
            .len();
        assert_eq!(n, 3); // a (whole cycle) + b + c
    }

    #[test]
    fn range_expands() {
        assert_eq!(
            vals("0..3"),
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn tail_makes_list() {
        assert_eq!(
            vals("bd:3"),
            vec![Value::List(vec![Value::Str("bd".into()), Value::Int(3)])]
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
