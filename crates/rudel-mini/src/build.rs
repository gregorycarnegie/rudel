use crate::Rule;
use crate::atom::{atom_value, num_value};
use pest::iterators::Pair;
use rudel_core::{Frac, Pattern, Value, fastcat, pure, rand, silence, stack, timecat};
use std::sync::Arc;

/// Strudel offsets each `?`/`|` PRNG stream by `0.0003 * seed` cycles, where
/// `seed` counts those operators left-to-right within one parsed string.
const RAND_OFFSET: f64 = 0.0003;

/// A built pattern plus the metadata Strudel's mini builder tracks alongside
/// it. `weight` is Strudel's `__weight`: for sequences the sum of element
/// weights (drives `<>` slowdown and `{}` alignment), for elements the
/// `@`/`!` weight (drives `timecat` proportions). `steps_source` is Strudel's
/// `__steps_source`: true when this node or a descendant carries the `^`
/// steps marker, which makes its step count override `_steps` upward.
pub(crate) struct Built {
    pub(crate) pat: Pattern,
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

/// Per-parse state: the krill-order PRNG seed counter and the position of
/// the mini string within the surrounding source code (added to every leaf
/// location).
pub(crate) struct Ctx {
    seed: i64,
    offset: usize,
}

impl Ctx {
    pub(crate) fn new(offset: usize) -> Self {
        Self { seed: 0, offset }
    }

    fn next_seed(&mut self) -> i64 {
        let s = self.seed;
        self.seed += 1;
        s
    }
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

pub(crate) fn build_stack_or_choose(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let mut inner = pair.into_inner();
    let head = build_sequence(inner.next().expect("stack_or_choose head"), ctx);
    let Some(tail) = inner.next() else {
        return head;
    };
    let rule = tail.as_rule();
    let mut children = vec![head];
    for s in tail.into_inner() {
        children.push(build_sequence(s, ctx));
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
            let s = ctx.next_seed();
            let mut p = choose_in_with(seeded_rand(s).segment(1), pats);
            if let Some(l) = marked_lcm(&children) {
                p = p.set_steps(Some(l));
            }
            p
        }
        // Feet: each foot becomes one step. krill burns a seed for the group.
        _ => {
            ctx.next_seed();
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
fn build_sequence(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let mut marked = false;
    let mut elems: Vec<Built> = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::steps_marker => marked = true,
            Rule::slice_with_ops => elems.push(build_slice_with_ops(inner, ctx)),
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
///
/// Variants intentionally hold `Pattern` by value: these are built once while
/// parsing mini-notation (not on the query hot path), and boxing each one to
/// equalize variant sizes would add allocations for no real benefit.
#[allow(clippy::large_enum_variant)]
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
fn build_slice_with_ops(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let mut inner = pair.into_inner();
    let built = build_slice(inner.next().expect("slice"), ctx);
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
            Rule::op_fast => ops.push(Op::Fast(op_slice(op, ctx))),
            Rule::op_slow => ops.push(Op::Slow(op_slice(op, ctx))),
            Rule::op_degrade => ops.push(Op::Degrade {
                amount: number_in(&op).unwrap_or(0.5),
                seed: ctx.next_seed(),
            }),
            Rule::op_euclid => {
                let mut args = op.into_inner().map(|a| build_euclid_arg(a, ctx));
                let pulse = args.next().expect("euclid pulse");
                let step = args.next().expect("euclid steps");
                let rotation = args.next();
                ops.push(Op::Euclid {
                    pulse,
                    step,
                    rotation,
                });
            }
            Rule::op_tail => ops.push(Op::Tail(op_slice(op, ctx))),
            Rule::op_range => ops.push(Op::Range(op_slice(op, ctx))),
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

fn build_slice(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let inner = pair.into_inner().next().expect("slice inner");
    match inner.as_rule() {
        Rule::step => build_step(inner, ctx),
        Rule::sub_cycle => {
            build_stack_or_choose(inner.into_inner().next().expect("sub_cycle body"), ctx)
        }
        Rule::slow_sequence => build_slow_sequence(inner, ctx),
        Rule::polymeter => build_polymeter(inner, ctx),
        _ => Built::plain(silence()),
    }
}

fn build_step(pair: Pair<Rule>, ctx: &Ctx) -> Built {
    let span = pair.as_span();
    Built::plain(match atom_value(pair.as_str()) {
        Some(v) => pure(v).with_loc(span.start() + ctx.offset, span.end() + ctx.offset),
        None => silence(),
    })
}

/// `<a b c>`: stack of the sequences, each slowed by its own weight so one
/// step plays per cycle.
fn build_slow_sequence(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let poly = pair.into_inner().next().expect("poly_stack");
    let children: Vec<Built> = poly.into_inner().map(|s| build_sequence(s, ctx)).collect();
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
fn build_polymeter(pair: Pair<Rule>, ctx: &mut Ctx) -> Built {
    let mut inner = pair.into_inner();
    let poly = inner.next().expect("poly_stack");
    let children: Vec<Built> = poly.into_inner().map(|s| build_sequence(s, ctx)).collect();
    let steps_pat = inner
        .next()
        .map(|ps| build_slice(ps.into_inner().next().expect("polymeter_steps slice"), ctx).pat);
    let aligned: Vec<Pattern> = match steps_pat {
        None => {
            let spc = children.first().map(|c| c.weight).unwrap_or_else(Frac::one);
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
                c.pat
                    .fast(sp.fmap(move |v| Value::Frac(value_frac(&v) / w)))
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
fn build_euclid_arg(pair: Pair<Rule>, ctx: &mut Ctx) -> Pattern {
    let mut inner = pair.into_inner();
    let pat = build_slice(inner.next().expect("euclid arg slice"), ctx).pat;
    for op in inner {
        consume_op_seeds(op, ctx);
    }
    pat
}

/// Walk a discarded op purely for its seed side effects.
fn consume_op_seeds(op: Pair<Rule>, ctx: &mut Ctx) {
    match op.as_rule() {
        Rule::op_degrade => {
            ctx.next_seed();
        }
        Rule::op_fast | Rule::op_slow | Rule::op_tail | Rule::op_range => {
            op_slice(op, ctx);
        }
        Rule::op_euclid => {
            for arg in op.into_inner() {
                build_euclid_arg(arg, ctx);
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

fn op_slice(op: Pair<Rule>, ctx: &mut Ctx) -> Pattern {
    let slice = op.into_inner().next().expect("op slice");
    build_slice(slice, ctx).pat
}

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
