// pattern.rs - core pattern representation, ported from strudel/packages/core/pattern.mjs
// Copyright (C) 2025 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::{Frac, lcm_opt};
use crate::hap::{Context, Hap};
use crate::state::State;
use crate::timespan::TimeSpan;
use crate::value::Value;
use std::sync::Arc;

type QueryFn = Arc<dyn Fn(&State) -> Vec<Hap> + Send + Sync>;

/// A pattern is a function from a query [`State`] to a list of [`Hap`]s,
/// plus an optional step count (used by step-based combinators).
#[derive(Clone)]
pub struct Pattern {
    query: QueryFn,
    pub steps: Option<Frac>,
    /// Set when this pattern is a `pure` value, enabling the patternify
    /// fast-path (Strudel's `__pure`). Cleared by transforms.
    pub pure_value: Option<Box<Value>>,
    /// Source location of the `pure` value, when it came from mini-notation
    /// (Strudel's `__pure_loc`). Lets the patternify fast-path keep the
    /// argument's location even though the argument pattern is bypassed.
    pub pure_loc: Option<(usize, usize)>,
    /// The raw mini-notation source text this pattern was parsed from, when it
    /// came directly from an `m("...", offset)` literal. Lets functions that
    /// want the raw string (e.g. `samples`, `scale`, `chord`) recover it after
    /// every string literal is wrapped for source-location tracking. Not
    /// preserved across transforms. Boxed to keep `Pattern` small.
    pub source: Option<Box<String>>,
}

impl Pattern {
    pub fn new<F>(query: F) -> Pattern
    where
        F: Fn(&State) -> Vec<Hap> + Send + Sync + 'static,
    {
        Pattern {
            query: Arc::new(query),
            steps: None,
            pure_value: None,
            pure_loc: None,
            source: None,
        }
    }

    /// Remember the raw mini-notation source text (see [`Pattern::source`]).
    pub fn with_source(mut self, source: impl Into<String>) -> Pattern {
        self.source = Some(Box::new(source.into()));
        self
    }

    pub fn query(&self, state: &State) -> Vec<Hap> {
        (self.query)(state)
    }

    pub fn set_steps(mut self, steps: Option<Frac>) -> Pattern {
        self.steps = steps;
        self
    }

    /// Query the haps inside `[begin, end)`.
    pub fn query_arc(&self, begin: Frac, end: Frac) -> Vec<Hap> {
        self.query(&State::new(TimeSpan::new(begin, end)))
    }

    //////////////////////////////////////////////////////////////////////
    // Functor

    /// Apply `f` to the value of each hap (`withValue`/`fmap`).
    pub fn with_value<F>(&self, f: F) -> Pattern
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        let pat = self.clone();
        let f = Arc::new(f);
        Pattern::new(move |state| {
            let f = f.clone();
            pat.query(state)
                .into_iter()
                .map(move |hap| hap.with_value(&*f))
                .collect()
        })
        .set_steps(self.steps)
    }

    pub fn fmap<F>(&self, f: F) -> Pattern
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        self.with_value(f)
    }

    //////////////////////////////////////////////////////////////////////
    // Query/hap span helpers

    pub fn with_query_span<F>(&self, f: F) -> Pattern
    where
        F: Fn(TimeSpan) -> TimeSpan + Send + Sync + 'static,
    {
        let pat = self.clone();
        Pattern::new(move |state| pat.query(&state.with_span(&f)))
    }

    /// Like `with_query_span`, but if `f` returns `None` the query yields nothing.
    pub fn with_query_span_maybe<F>(&self, f: F) -> Pattern
    where
        F: Fn(TimeSpan) -> Option<TimeSpan> + Send + Sync + 'static,
    {
        let pat = self.clone();
        Pattern::new(move |state| match f(state.span) {
            Some(span) => pat.query(&state.set_span(span)),
            None => vec![],
        })
    }

    pub fn with_query_time<F>(&self, f: F) -> Pattern
    where
        F: Fn(Frac) -> Frac + Send + Sync + 'static,
    {
        self.with_query_span(move |span| span.with_time(&f))
    }

    pub fn with_hap_span<F>(&self, f: F) -> Pattern
    where
        F: Fn(TimeSpan) -> TimeSpan + Send + Sync + 'static,
    {
        let pat = self.clone();
        let f = Arc::new(f);
        Pattern::new(move |state| {
            let f = f.clone();
            pat.query(state)
                .into_iter()
                .map(move |hap| hap.with_span(&*f))
                .collect()
        })
    }

    pub fn with_hap_time<F>(&self, f: F) -> Pattern
    where
        F: Fn(Frac) -> Frac + Send + Sync + 'static,
    {
        self.with_hap_span(move |span| span.with_time(&f))
    }

    pub fn with_haps<F>(&self, f: F) -> Pattern
    where
        F: Fn(Vec<Hap>, &State) -> Vec<Hap> + Send + Sync + 'static,
    {
        let pat = self.clone();
        Pattern::new(move |state| f(pat.query(state), state)).set_steps(self.steps)
    }

    pub fn with_hap<F>(&self, f: F) -> Pattern
    where
        F: Fn(Hap) -> Hap + Send + Sync + 'static,
    {
        self.with_haps(move |haps, _| haps.into_iter().map(&f).collect())
    }

    pub fn set_context(&self, context: Context) -> Pattern {
        self.with_hap(move |hap| hap.set_context(context.clone()))
    }

    /// Rewrite the context of every hap (`withContext`). Preserves the `pure`
    /// fast-path metadata, like Strudel.
    pub fn with_context<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Context) -> Context + Send + Sync + 'static,
    {
        let mut result = self.with_hap(move |hap| {
            let context = f(&hap.context);
            hap.set_context(context)
        });
        result.steps = self.steps;
        result.pure_value = self.pure_value.clone();
        result.pure_loc = self.pure_loc;
        result
    }

    /// Tag every hap with a source location (`withLoc`), used by mini-notation
    /// so editors can map events back to the code that produced them.
    pub fn with_loc(&self, start: usize, end: usize) -> Pattern {
        let mut result = self.with_context(move |context| {
            let mut context = context.clone();
            context.locations.push((start, end));
            context
        });
        if result.pure_value.is_some() {
            result.pure_loc = Some((start, end));
        }
        result
    }

    /// Split queries at cycle boundaries so every hap stays within one cycle.
    pub fn split_queries(&self) -> Pattern {
        let pat = self.clone();
        Pattern::new(move |state| {
            state
                .span
                .span_cycles()
                .into_iter()
                .flat_map(|sub| pat.query(&state.set_span(sub)))
                .collect()
        })
    }

    /// Keep only discrete haps (those with a `whole`).
    pub fn discrete_only(&self) -> Pattern {
        self.with_haps(|haps, _| haps.into_iter().filter(|h| h.whole.is_some()).collect())
    }

    /// Keep only haps with an onset.
    pub fn onsets_only(&self) -> Pattern {
        self.with_haps(|haps, _| haps.into_iter().filter(|h| h.has_onset()).collect())
    }

    /// Keep only haps whose hap passes `pred` (`filterHaps`).
    pub fn filter_haps<F>(&self, pred: F) -> Pattern
    where
        F: Fn(&Hap) -> bool + Send + Sync + 'static,
    {
        self.with_haps(move |haps, _| haps.into_iter().filter(|h| pred(h)).collect())
            .set_steps(self.steps)
    }

    /// Keep only haps whose value passes `pred` (`filterValues`).
    pub fn filter_values<F>(&self, pred: F) -> Pattern
    where
        F: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        self.filter_haps(move |h| pred(&h.value))
    }

    //////////////////////////////////////////////////////////////////////
    // Applicative

    /// `appWhole`: apply a pattern of functions to a pattern of values, using
    /// `whole_func` to combine the wholes.
    pub fn app_whole<W>(&self, whole_func: W, pat_val: &Pattern) -> Pattern
    where
        W: Fn(Option<TimeSpan>, Option<TimeSpan>) -> Option<TimeSpan> + Send + Sync + 'static,
    {
        let pat_func = self.clone();
        let pat_val = pat_val.clone();
        let whole_func = Arc::new(whole_func);
        Pattern::new(move |state| {
            let hap_funcs = pat_func.query(state);
            let hap_vals = pat_val.query(state);
            let mut out = Vec::new();
            for hf in &hap_funcs {
                for hv in &hap_vals {
                    if let Some(part) = hf.part.intersection(&hv.part) {
                        let whole = whole_func(hf.whole, hv.whole);
                        let value = hf.value.apply(hv.value.clone());
                        out.push(Hap::new(whole, part, value).with_context(hv.combine_context(hf)));
                    }
                }
            }
            out
        })
    }

    /// `appBoth` (Tidal `<*>`): wholes are the intersection.
    pub fn app_both(&self, pat_val: &Pattern) -> Pattern {
        let result = self.app_whole(
            |a, b| match (a, b) {
                (Some(a), Some(b)) => Some(a.intersection_e(&b)),
                _ => None,
            },
            pat_val,
        );
        result.set_steps(lcm_opt([self.steps, pat_val.steps]))
    }

    /// `appLeft`: structure (wholes) preserved from the function (left) pattern.
    pub fn app_left(&self, pat_val: &Pattern) -> Pattern {
        let pat_func = self.clone();
        let pat_val = pat_val.clone();
        Pattern::new(move |state| {
            let mut out = Vec::new();
            for hf in pat_func.query(state) {
                let hap_vals = pat_val.query(&state.set_span(hf.whole_or_part()));
                for hv in hap_vals {
                    if let Some(part) = hf.part.intersection(&hv.part) {
                        let value = hf.value.apply(hv.value.clone());
                        out.push(
                            Hap::new(hf.whole, part, value).with_context(hv.combine_context(&hf)),
                        );
                    }
                }
            }
            out
        })
        .set_steps(self.steps)
    }

    /// `appRight`: structure preserved from the value (right) pattern.
    pub fn app_right(&self, pat_val: &Pattern) -> Pattern {
        let pat_func = self.clone();
        let pat_val2 = pat_val.clone();
        Pattern::new(move |state| {
            let mut out = Vec::new();
            for hv in pat_val2.query(state) {
                let hap_funcs = pat_func.query(&state.set_span(hv.whole_or_part()));
                for hf in hap_funcs {
                    if let Some(part) = hf.part.intersection(&hv.part) {
                        let value = hf.value.apply(hv.value.clone());
                        out.push(
                            Hap::new(hv.whole, part, value).with_context(hv.combine_context(&hf)),
                        );
                    }
                }
            }
            out
        })
        .set_steps(pat_val.steps)
    }

    //////////////////////////////////////////////////////////////////////
    // Monad

    pub fn bind_whole<C, F>(&self, choose_whole: C, func: F) -> Pattern
    where
        C: Fn(Option<TimeSpan>, Option<TimeSpan>) -> Option<TimeSpan> + Send + Sync + 'static,
        F: Fn(Value) -> Pattern + Send + Sync + 'static,
    {
        let pat_val = self.clone();
        let choose_whole = Arc::new(choose_whole);
        let func = Arc::new(func);
        Pattern::new(move |state| {
            let mut out = Vec::new();
            for a in pat_val.query(state) {
                let inner = func(a.value.clone());
                for b in inner.query(&state.set_span(a.part)) {
                    let whole = choose_whole(a.whole, b.whole);
                    out.push(
                        Hap::new(whole, b.part, b.value.clone())
                            .with_context(a.context.combine(&b.context)),
                    );
                }
            }
            out
        })
    }

    pub fn bind<F>(&self, func: F) -> Pattern
    where
        F: Fn(Value) -> Pattern + Send + Sync + 'static,
    {
        self.bind_whole(
            |a, b| match (a, b) {
                (Some(a), Some(b)) => Some(a.intersection_e(&b)),
                _ => None,
            },
            func,
        )
    }

    pub fn inner_bind<F>(&self, func: F) -> Pattern
    where
        F: Fn(Value) -> Pattern + Send + Sync + 'static,
    {
        self.bind_whole(|_, b| b, func)
    }

    pub fn outer_bind<F>(&self, func: F) -> Pattern
    where
        F: Fn(Value) -> Pattern + Send + Sync + 'static,
    {
        self.bind_whole(|a, _| a, func).set_steps(self.steps)
    }

    pub fn inner_join(&self) -> Pattern {
        self.inner_bind(value_to_pattern)
    }

    pub fn outer_join(&self) -> Pattern {
        self.outer_bind(value_to_pattern)
    }

    pub fn join(&self) -> Pattern {
        self.bind(value_to_pattern)
    }

    /// `squeezeJoin`: each outer hap's whole is filled with one cycle of the
    /// inner pattern (value-as-pattern), focused into that span.
    pub fn squeeze_join(&self) -> Pattern {
        let pat_of_pats = self.clone();
        Pattern::new(move |state| {
            let haps = pat_of_pats.discrete_only().query(state);
            let mut out = Vec::new();
            for outer in haps {
                let inner_pat =
                    value_to_pattern(outer.value.clone())._focus_span(outer.whole_or_part());
                for inner in inner_pat.query(&state.set_span(outer.part)) {
                    let whole = match (inner.whole, outer.whole) {
                        (Some(i), Some(o)) => match i.intersection(&o) {
                            Some(w) => Some(w),
                            None => continue,
                        },
                        _ => None,
                    };
                    let Some(part) = inner.part.intersection(&outer.part) else {
                        continue;
                    };
                    out.push(
                        Hap::new(whole, part, inner.value.clone())
                            .with_context(inner.context.combine(&outer.context)),
                    );
                }
            }
            out
        })
    }

    pub fn squeeze_bind<F>(&self, func: F) -> Pattern
    where
        F: Fn(Value) -> Value + Send + Sync + 'static,
    {
        self.fmap(func).squeeze_join()
    }

    /// `resetJoin`/`restartJoin`: flatten a pattern of patterns by retriggering
    /// each inner pattern at the onsets of the outer pattern. `reset` aligns the
    /// inner pattern's cycle position to the onset; `restart` aligns the inner
    /// pattern's cycle zero to the onset.
    fn reset_join_impl(&self, restart: bool) -> Pattern {
        let pat_of_pats = self.clone();
        Pattern::new(move |state| {
            let mut out = Vec::new();
            for outer in pat_of_pats.discrete_only().query(state) {
                let Some(owhole) = outer.whole else { continue };
                let shift = if restart {
                    owhole.begin
                } else {
                    owhole.begin.cycle_pos()
                };
                let inner_pat = value_to_pattern(outer.value.clone())._late(shift);
                for inner in inner_pat.query(state) {
                    let whole = match inner.whole {
                        Some(iw) => match iw.intersection(&owhole) {
                            Some(w) => Some(w),
                            None => continue,
                        },
                        None => None,
                    };
                    let Some(part) = inner.part.intersection(&outer.part) else {
                        continue;
                    };
                    out.push(
                        Hap::new(whole, part, inner.value.clone())
                            .with_context(outer.context.combine(&inner.context)),
                    );
                }
            }
            out
        })
    }

    /// Retrigger inner patterns at outer onsets, aligned to cycle position
    /// (`resetJoin`).
    pub fn reset_join(&self) -> Pattern {
        self.reset_join_impl(false)
    }

    /// Retrigger inner patterns at outer onsets, aligned to cycle zero
    /// (`restartJoin`).
    pub fn restart_join(&self) -> Pattern {
        self.reset_join_impl(true)
    }

    /// `polyJoin`: flatten a pattern of patterns polymetrically — each inner
    /// pattern is `extend`ed so its step count matches the outer pattern's,
    /// then outer-joined.
    pub fn poly_join(&self) -> Pattern {
        let outer_steps = self.steps;
        self.fmap(move |v| {
            let inner = value_to_pattern(v);
            let factor = match (outer_steps, inner.steps) {
                (Some(a), Some(b)) if b != crate::fraction::Frac::zero() => a / b,
                _ => crate::fraction::Frac::one(),
            };
            Value::Pat(Box::new(inner.extend(factor)))
        })
        .outer_join()
    }

    //////////////////////////////////////////////////////////////////////
    // Time transforms
    //
    // These are the raw (unpatternified) ops, named with a leading `_` as in
    // Strudel. The patternified, argument-lifting public versions (`fast`,
    // `slow`, ...) live in the `transforms` module.

    pub fn _fast(&self, factor: Frac) -> Pattern {
        if factor == Frac::zero() {
            return silence();
        }
        self.with_query_time(move |t| t * factor)
            .with_hap_time(move |t| t / factor)
            .set_steps(self.steps)
    }

    pub fn _slow(&self, factor: Frac) -> Pattern {
        if factor == Frac::zero() {
            return silence();
        }
        self._fast(Frac::one() / factor)
    }

    pub fn _early(&self, offset: Frac) -> Pattern {
        self.with_query_time(move |t| t + offset)
            .with_hap_time(move |t| t - offset)
    }

    pub fn _late(&self, offset: Frac) -> Pattern {
        self._early(-offset)
    }

    pub fn rev(&self) -> Pattern {
        let pat = self.clone();
        Pattern::new(move |state| {
            let span = state.span;
            let cycle = span.begin.sam();
            let next_cycle = span.begin.next_sam();
            let reflect = move |s: TimeSpan| {
                // reflect each endpoint, then swap begin/end
                let b = cycle + (next_cycle - s.begin);
                let e = cycle + (next_cycle - s.end);
                TimeSpan::new(e, b)
            };
            pat.query(&state.set_span(reflect(span)))
                .into_iter()
                .map(|hap| hap.with_span(reflect))
                .collect()
        })
        .split_queries()
        .set_steps(self.steps)
    }

    /// `fastGap`: speed up but leave a gap, rather than repeating.
    pub fn _fast_gap(&self, factor: Frac) -> Pattern {
        let pat = self.clone();
        let qf = move |span: TimeSpan| -> Option<TimeSpan> {
            let cycle = span.begin.sam();
            let bpos = ((span.begin - cycle) * factor).min(Frac::one());
            let epos = ((span.end - cycle) * factor).min(Frac::one());
            if bpos >= Frac::one() {
                return None;
            }
            Some(TimeSpan::new(cycle + bpos, cycle + epos))
        };
        let ef = move |hap: Hap| -> Hap {
            let begin = hap.part.begin;
            let end = hap.part.end;
            let cycle = begin.sam();
            let beginpos = ((begin - cycle) / factor).min(Frac::one());
            let endpos = ((end - cycle) / factor).min(Frac::one());
            let new_part = TimeSpan::new(cycle + beginpos, cycle + endpos);
            let new_whole = hap.whole.map(|w| {
                TimeSpan::new(
                    new_part.begin - (begin - w.begin) / factor,
                    new_part.end + (w.end - end) / factor,
                )
            });
            Hap::new(new_whole, new_part, hap.value.clone()).with_context(hap.context.clone())
        };
        pat.with_query_span_maybe(qf).with_hap(ef).split_queries()
    }

    /// `focus`: like compress but without gaps; focus span can exceed a cycle.
    pub fn _focus(&self, b: Frac, e: Frac) -> Pattern {
        self._early(b.sam())._fast(Frac::one() / (e - b))._late(b)
    }

    pub fn _focus_span(&self, span: TimeSpan) -> Pattern {
        self._focus(span.begin, span.end)
    }

    /// `compress`: squeeze each cycle into `[b, e]`, leaving a gap.
    pub fn _compress(&self, b: Frac, e: Frac) -> Pattern {
        if b > e || b > Frac::one() || e > Frac::one() || b < Frac::zero() || e < Frac::zero() {
            return silence();
        }
        self._fast_gap(Frac::one() / (e - b))._late(b)
    }

    /// Repeat each event `factor` times within its own span (`ply`).
    pub fn _ply(&self, factor: Frac) -> Pattern {
        let result = self.squeeze_bind(move |v| Value::Pat(Box::new(pure(v)._fast(factor))));
        result.set_steps(self.steps.map(|s| factor * s))
    }

    /// Stack another pattern on top of this one.
    pub fn stack_with(&self, other: &Pattern) -> Pattern {
        stack(&[self.clone(), other.clone()])
    }

    /// Stack `other` on top of this pattern (`overlay`). Like [`stack_with`] but
    /// accepts anything patternifiable (numbers, mini-notation strings, …).
    pub fn overlay(&self, other: impl crate::transforms::IntoPattern) -> Pattern {
        self.stack_with(&other.into_pattern())
    }

    /// Speed the pattern up/down so it has `target` steps per cycle, preserving
    /// its step count metadata (`pace`). A pattern with no step count, or zero
    /// steps, is returned unchanged / as silence respectively.
    pub fn pace(&self, target: Frac) -> Pattern {
        match self.steps {
            None => self.clone(),
            Some(s) if s == Frac::zero() => silence(),
            Some(s) => self._fast(target / s).set_steps(Some(target)),
        }
    }
}

/// Turn a [`Value`] into a [`Pattern`]: patterns pass through, everything else
/// becomes `pure`.
pub fn value_to_pattern(value: Value) -> Pattern {
    match value {
        Value::Pat(p) => *p,
        other => pure(other),
    }
}

/// Reify an arbitrary value into a pattern (mini-notation string parsing will
/// hook in here in a later phase).
pub fn reify(value: Value) -> Pattern {
    value_to_pattern(value)
}

//////////////////////////////////////////////////////////////////////
// Constructors

/// A pattern that repeats `value` once per cycle.
pub fn pure(value: Value) -> Pattern {
    let pure_value = Some(Box::new(value.clone()));
    let mut pat = Pattern::new(move |state| {
        state
            .span
            .span_cycles()
            .into_iter()
            .map(|sub| Hap::new(Some(whole_cycle(sub.begin)), sub, value.clone()))
            .collect()
    })
    .set_steps(Some(Frac::one()));
    pat.pure_value = pure_value;
    pat
}

fn whole_cycle(t: Frac) -> TimeSpan {
    TimeSpan::new(t.sam(), t.next_sam())
}

/// An empty pattern occupying `steps` steps.
pub fn gap(steps: Frac) -> Pattern {
    Pattern::new(|_| vec![]).set_steps(Some(steps))
}

/// The empty pattern (one step).
pub fn silence() -> Pattern {
    gap(Frac::one())
}

/// The empty pattern occupying zero steps.
pub fn nothing() -> Pattern {
    gap(Frac::zero())
}

/// Play all patterns at once.
pub fn stack(pats: &[Pattern]) -> Pattern {
    let pats: Vec<Pattern> = pats.to_vec();
    let steps = lcm_opt(pats.iter().map(|p| p.steps));
    Pattern::new(move |state| pats.iter().flat_map(|p| p.query(state)).collect()).set_steps(steps)
}

/// Concatenate patterns, one per cycle (`slowcat`/`cat`).
pub fn slowcat(pats: &[Pattern]) -> Pattern {
    if pats.len() == 1 {
        return pats[0].clone();
    }
    let pats: Vec<Pattern> = pats.to_vec();
    let steps = lcm_opt(pats.iter().map(|p| p.steps));
    let len = pats.len() as i64;
    Pattern::new(move |state| {
        let span = state.span;
        let pat_n = span.begin.sam().numer().rem_euclid(len as i128) as usize;
        let pat = &pats[pat_n];
        // Keep cycles from constituent patterns from being skipped.
        let offset = span.begin.floor() - (span.begin / Frac::int(len)).floor();
        pat.with_hap_time(move |t| t + offset)
            .query(&state.set_span(span.with_time(|t| t - offset)))
    })
    .split_queries()
    .set_steps(steps)
}

pub fn cat(pats: &[Pattern]) -> Pattern {
    slowcat(pats)
}

/// Like `slowcat`, but skips cycles instead of preserving constituent cycle
/// continuity (`slowcatPrime`). Used by `every`/`firstOf`/`lastOf`.
pub fn slowcat_prime(pats: &[Pattern]) -> Pattern {
    let pats: Vec<Pattern> = pats.to_vec();
    let len = pats.len() as i64;
    Pattern::new(move |state| {
        if len == 0 {
            return vec![];
        }
        let pat_n = state.span.begin.sam().numer().rem_euclid(len as i128) as usize;
        pats[pat_n].query(state)
    })
    .split_queries()
}

/// Concatenate patterns, all crammed into one cycle (`fastcat`/`sequence`).
pub fn fastcat(pats: &[Pattern]) -> Pattern {
    let n = pats.len();
    let mut result = slowcat(pats);
    if n > 1 {
        result = result
            ._fast(Frac::int(n as i64))
            .set_steps(Some(Frac::int(n as i64)));
    }
    result
}

pub fn sequence(pats: &[Pattern]) -> Pattern {
    fastcat(pats)
}

/// Weighted concatenation: each `(weight, pattern)` pair fills a proportional
/// slice of the cycle (`timeCat`). Used by mini-notation `@`/`_` weights.
/// Like Strudel's `stepcat`, a single pair returns its pattern uncompressed
/// (so it isn't query-split per cycle), and zero-weight pairs are skipped.
pub fn timecat(pairs: &[(Frac, Pattern)]) -> Pattern {
    if let [(w, p)] = pairs {
        return p.clone().set_steps(Some(*w));
    }
    let total: Frac = pairs.iter().fold(Frac::zero(), |acc, (w, _)| acc + *w);
    if total == Frac::zero() {
        return silence();
    }
    let mut begin = Frac::zero();
    let mut pats = Vec::with_capacity(pairs.len());
    for (w, p) in pairs {
        if *w == Frac::zero() {
            continue;
        }
        let end = begin + *w / total;
        pats.push(p._compress(begin, end));
        begin = end;
    }
    stack(&pats).set_steps(Some(total))
}

/// Stepwise concatenation (`stepcat`/`timeCat`): like [`fastcat`] but each
/// pattern occupies a slice proportional to its own step count (defaulting to
/// `1` when unknown). `stepcat("bd sd cp", "hh hh")` is the same as
/// `"bd sd cp hh hh"`.
pub fn stepcat(pats: &[Pattern]) -> Pattern {
    let pairs: Vec<(Frac, Pattern)> = pats
        .iter()
        .map(|p| (p.steps.unwrap_or_else(Frac::one), p.clone()))
        .collect();
    timecat(&pairs)
}

/// Arrange `(cycles, pattern)` sections over a timeline (`arrange`). Each
/// section is sped up to fill its own cycle, then the whole thing is slowed so
/// every section spans the requested number of cycles.
pub fn arrange(sections: &[(Frac, Pattern)]) -> Pattern {
    let total: Frac = sections.iter().fold(Frac::zero(), |a, (c, _)| a + *c);
    if total == Frac::zero() {
        return silence();
    }
    let pairs: Vec<(Frac, Pattern)> = sections
        .iter()
        .map(|(cycles, pat)| (*cycles, pat._fast(*cycles)))
        .collect();
    timecat(&pairs)._slow(total)
}

/// Align patterns to a common step count (the LCM of their step counts),
/// creating polymeters (`polymeter`/`pm`). Patterns without a step count are
/// ignored, mirroring Strudel.
pub fn polymeter(pats: &[Pattern]) -> Pattern {
    let steps_list: Vec<Frac> = pats.iter().filter_map(|p| p.steps).collect();
    let Some(steps) = steps_list.into_iter().reduce(|a, b| a.lcm(b)) else {
        return silence();
    };
    if steps == Frac::zero() {
        return silence();
    }
    let paced: Vec<Pattern> = pats
        .iter()
        .filter(|p| p.steps.is_some())
        .map(|p| p.pace(steps))
        .collect();
    stack(&paced).set_steps(Some(steps))
}

// ---------------------------------------------------------------------------
// Settable string parser (mini-notation). rudel-mini installs a hook here so
// that `&str` arguments parse as mini-notation, mirroring Strudel's
// `setStringParser`. Without a hook, strings become `pure` values.

type StringParser = fn(&str) -> Pattern;
static STRING_PARSER: std::sync::RwLock<Option<StringParser>> = std::sync::RwLock::new(None);

/// Install the mini-notation parser used to interpret `&str` patterns.
pub fn set_string_parser(parser: StringParser) {
    *STRING_PARSER.write().unwrap() = Some(parser);
}

/// Parse a string into a pattern via the installed parser, or `pure` if none.
pub fn parse_string(s: &str) -> Pattern {
    match *STRING_PARSER.read().unwrap() {
        Some(parser) => parser(s),
        None => pure(Value::Str(s.to_string())),
    }
}
