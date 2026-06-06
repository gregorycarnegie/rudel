// transforms/pattern_ops.rs - pattern-level transform operations built on the
// machinery in transforms/core.rs. Ported from
// strudel/packages/core/{pattern,signal}.mjs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::hap::{Context, Hap};
use crate::pattern::{Pattern, fastcat, pure, silence, slowcat, stack, value_to_pattern};
use crate::signal::rand;
use crate::timespan::TimeSpan;
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Set `key` to `value` on a map value, leaving non-maps untouched (used by
/// `jux`/`hurry`).
fn set_key(v: Value, key: &str, value: Value) -> Value {
    match v {
        Value::Map(mut m) => {
            m.insert(key.to_string(), value);
            Value::Map(m)
        }
        other => other,
    }
}

fn frac(n: impl Into<Frac>) -> Frac {
    n.into()
}

impl Pattern {
    // -- Raw helpers used below --------------------------------------------

    /// Reverse a whole pattern across the timeline (`revv`).
    pub fn revv(&self) -> Pattern {
        let negate = |s: TimeSpan| TimeSpan::new(-s.end, -s.begin);
        self.with_query_span(negate).with_hap_span(negate)
    }

    /// Repeat each cycle `n` times (`repeatCycles`).
    pub fn repeat_cycles(&self, n: i64) -> Pattern {
        if n <= 1 {
            return self.clone();
        }
        let pat = self.clone();
        let n = Frac::int(n);
        Pattern::new(move |state| {
            let cycle = state.span.begin.sam();
            let source_cycle = (cycle / n).sam();
            let delta = cycle - source_cycle;
            let shifted = state.with_span(|span| span.with_time(|t| t - delta));
            pat.query(&shifted)
                .into_iter()
                .map(|hap| hap.with_span(|span| span.with_time(|t| t + delta)))
                .collect()
        })
        .split_queries()
    }

    /// Keep only haps whose onset time passes `test` (`filterWhen`).
    pub fn filter_when<F>(&self, test: F) -> Pattern
    where
        F: Fn(Frac) -> bool + Send + Sync + 'static,
    {
        self.filter_haps(move |h| test(h.whole_or_part().begin))
    }

    /// `zoom`: play the `[s, e]` slice of a pattern over the full cycle.
    pub fn zoom(&self, s: Frac, e: Frac) -> Pattern {
        let d = e - s;
        if d <= Frac::zero() {
            return silence();
        }
        self.with_query_span(move |span| span.with_cycle(|t| t * d + s))
            .with_hap_span(move |span| span.with_cycle(|t| (t - s) / d))
            .split_queries()
    }

    /// Apply transform `f` only where the boolean pattern is true (`when`).
    pub fn when<F>(&self, bools: impl IntoPattern, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let yes = Box::new(f(self));
        let no = Box::new(self.clone());
        bools
            .into_pattern()
            .fmap(move |b| Value::Pat(if b.truthy() { yes.clone() } else { no.clone() }))
            .inner_join()
    }

    // -- Cycle-stepping ----------------------------------------------------

    fn iter_impl(&self, times: i64, back: bool) -> Pattern {
        if times <= 0 {
            return self.clone();
        }
        let t = Frac::int(times);
        let pats: Vec<Pattern> = (0..times)
            .map(|i| {
                let off = Frac::int(i) / t;
                if back {
                    self._late(off)
                } else {
                    self._early(off)
                }
            })
            .collect();
        slowcat(&pats)
    }

    /// Shift the pattern forward by one `n`th each cycle (`iter`).
    pub fn iter(&self, n: i64) -> Pattern {
        self.iter_impl(n, false)
    }
    /// Like `iter`, but shifts backward (`iterBack`).
    pub fn iter_back(&self, n: i64) -> Pattern {
        self.iter_impl(n, true)
    }

    /// Alternate forwards/backwards each cycle (`palindrome`).
    pub fn palindrome(&self) -> Pattern {
        self.last_of(2, |p| p.rev())
    }

    /// Breakbeat feel: every other cycle, played twice as fast and nudged
    /// (`brak`).
    pub fn brak(&self) -> Pattern {
        self.when(
            slowcat(&[pure(Value::Bool(false)), pure(Value::Bool(true))]),
            |x| fastcat(&[x.clone(), silence()])._late(Frac::new(1, 4)),
        )
    }

    fn chunk_impl<F>(&self, n: i64, f: F, back: bool) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        if n <= 0 {
            return self.clone();
        }
        let mut bins = vec![pure(Value::Bool(true))];
        for _ in 1..n {
            bins.push(pure(Value::Bool(false)));
        }
        let binary_pat = fastcat(&bins).iter_impl(n, !back);
        self.repeat_cycles(n).when(binary_pat, f)
    }

    /// Cycle through `n` chunks, applying `f` to one chunk per cycle (`chunk`).
    pub fn chunk<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.chunk_impl(n, f, false)
    }
    /// Like `chunk`, but moves backwards through the chunks (`chunkBack`).
    pub fn chunk_back<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.chunk_impl(n, f, true)
    }

    // -- Inside / outside / within -----------------------------------------

    /// Apply `f` to a slowed-down view, then speed back up (`inside`).
    pub fn inside<F>(&self, n: impl Into<Frac>, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let n = frac(n);
        f(&self._slow(n))._fast(n)
    }
    /// Apply `f` to a sped-up view, then slow back down (`outside`).
    pub fn outside<F>(&self, n: impl Into<Frac>, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let n = frac(n);
        f(&self._fast(n))._slow(n)
    }

    /// Apply `f` only to the `[a, b]` portion of each cycle (`within`).
    pub fn within<F>(&self, a: Frac, b: Frac, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let inside = self.filter_when(move |t| {
            let p = t.cycle_pos();
            p >= a && p <= b
        });
        let outside = self.filter_when(move |t| {
            let p = t.cycle_pos();
            p < a || p > b
        });
        stack(&[f(&inside), outside])
    }

    // -- Press -------------------------------------------------------------

    /// Shift each event `r` of the way into its own timespan (`pressBy`).
    pub fn press_by(&self, r: Frac) -> Pattern {
        self.squeeze_bind(move |x| Value::Pat(Box::new(pure(x)._compress(r, Frac::one()))))
    }
    /// `pressBy(0.5)` (`press`).
    pub fn press(&self) -> Pattern {
        self.press_by(Frac::new(1, 2))
    }

    // -- Jux ---------------------------------------------------------------

    /// Pan a copy left, transform another panned right, and stack (`juxBy`).
    pub fn jux_by<F>(&self, by: f64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let by = by / 2.0;
        let left = self.fmap(move |v| set_key(v, "pan", Value::F64(0.5 - by)));
        let right = f(&self.fmap(move |v| set_key(v, "pan", Value::F64(0.5 + by))));
        stack(&[left, right])
    }
    /// `juxBy(1, f)`: hard-pan a transformed copy to the right ear (`jux`).
    pub fn jux<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.jux_by(1.0, f)
    }

    // -- Echo / stut -------------------------------------------------------

    /// Superimpose `times` delayed copies, transformed by `f(copy, i)`
    /// (`echoWith`).
    pub fn echo_with<F>(&self, times: i64, time: Frac, f: F) -> Pattern
    where
        F: Fn(&Pattern, i64) -> Pattern,
    {
        let pats: Vec<Pattern> = (0..times)
            .map(|i| f(&self._late(time * Frac::int(i)), i))
            .collect();
        stack(&pats)
    }

    /// Echo with decreasing gain (`echo`).
    pub fn echo(&self, times: i64, time: Frac, feedback: f64) -> Pattern {
        self.echo_with(times, time, move |p, i| p.gain(feedback.powi(i as i32)))
    }

    /// Deprecated arg order of [`echo`] (`stut`).
    pub fn stut(&self, times: i64, feedback: f64, time: Frac) -> Pattern {
        self.echo(times, time, feedback)
    }

    // -- Randomized application --------------------------------------------

    /// Apply `f` to a random `prob` fraction of events (`sometimesBy`).
    pub fn sometimes_by<F>(&self, prob: f64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        stack(&[self.degrade_by(prob), f(&self.undegrade_by(1.0 - prob))])
    }
    /// `sometimesBy(0.5, f)` (`sometimes`).
    pub fn sometimes<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.sometimes_by(0.5, f)
    }
    /// Apply `f` on a random `prob` fraction of *whole cycles*
    /// (`someCyclesBy`).
    pub fn some_cycles_by<F>(&self, prob: f64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        let per_cycle = rand().segment(1);
        let inv = rand()
            .fmap(|v| Value::F64(1.0 - v.as_f64().unwrap_or(0.0)))
            .segment(1);
        stack(&[
            self.degrade_by_with(per_cycle, prob),
            f(&self.degrade_by_with(inv, 1.0 - prob)),
        ])
    }
    /// `someCyclesBy(0.5, f)` (`someCycles`).
    pub fn some_cycles<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.some_cycles_by(0.5, f)
    }

    // -- Numeric value transforms ------------------------------------------

    /// Round each numeric value (`round`).
    pub fn round(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).round()))
    }
    /// Floor each numeric value (`floor`).
    pub fn floor(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).floor()))
    }
    /// Ceil each numeric value (`ceil`).
    pub fn ceil(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).ceil()))
    }
    /// Scale a unipolar (0..1) value to bipolar (-1..1) (`toBipolar`).
    pub fn to_bipolar(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0) * 2.0 - 1.0))
    }
    /// Scale a bipolar (-1..1) value to unipolar (0..1) (`fromBipolar`).
    pub fn from_bipolar(&self) -> Pattern {
        self.fmap(|v| Value::F64((v.as_f64().unwrap_or(0.0) + 1.0) / 2.0))
    }
    /// Scale a bipolar signal into `min..max` (`range2`).
    pub fn range2(&self, min: f64, max: f64) -> Pattern {
        self.from_bipolar().range(min, max)
    }
    /// Exponential variant of [`range`](Self::range) (`rangex`).
    pub fn rangex(&self, min: f64, max: f64) -> Pattern {
        self.range(min.ln(), max.ln())
            .fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).exp()))
    }

    /// Both speed up the pattern and the sample playback (`hurry`).
    pub fn hurry(&self, r: impl Into<Frac>) -> Pattern {
        let r = frac(r);
        let mut m = BTreeMap::new();
        m.insert("speed".to_string(), Value::Frac(r));
        self._fast(r).mul(pure(Value::Map(m)))
    }

    /// Apply a function to the whole pattern (`apply`).
    pub fn apply<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        f(self)
    }

    // -- sometimesBy probability aliases ------------------------------------

    /// `sometimesBy(0.75, f)` (`often`).
    pub fn often<F: Fn(&Pattern) -> Pattern>(&self, f: F) -> Pattern {
        self.sometimes_by(0.75, f)
    }
    /// `sometimesBy(0.25, f)` (`rarely`).
    pub fn rarely<F: Fn(&Pattern) -> Pattern>(&self, f: F) -> Pattern {
        self.sometimes_by(0.25, f)
    }
    /// `sometimesBy(0.9, f)` (`almostAlways`).
    pub fn almost_always<F: Fn(&Pattern) -> Pattern>(&self, f: F) -> Pattern {
        self.sometimes_by(0.9, f)
    }
    /// `sometimesBy(0.1, f)` (`almostNever`).
    pub fn almost_never<F: Fn(&Pattern) -> Pattern>(&self, f: F) -> Pattern {
        self.sometimes_by(0.1, f)
    }
    /// Always apply `f` (`always`).
    pub fn always<F: Fn(&Pattern) -> Pattern>(&self, f: F) -> Pattern {
        f(self)
    }
    /// Never apply `f` (`never`).
    pub fn never<F: Fn(&Pattern) -> Pattern>(&self, _f: F) -> Pattern {
        self.clone()
    }

    // -- more math ops -----------------------------------------------------

    /// Modulo each value by `other` (`mod`).
    pub fn modulo(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x.rem_euclid(*y)),
            _ => Value::F64(
                a.as_f64()
                    .unwrap_or(0.0)
                    .rem_euclid(b.as_f64().unwrap_or(1.0)),
            ),
        })
    }
    /// Raise each value to the power `other` (`pow`).
    pub fn pow(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), |a, b| {
            Value::F64(a.as_f64().unwrap_or(0.0).powf(b.as_f64().unwrap_or(0.0)))
        })
    }

    /// Reduce `":"`-list values to a single divided number (`ratio`).
    pub fn ratio(&self) -> Pattern {
        self.fmap(|v| ratio_value(&v))
    }

    /// `undegradeBy(0.5)` (`undegrade`).
    pub fn undegrade(&self) -> Pattern {
        self.undegrade_by(0.5)
    }

    /// Break each cycle into `n` slices and delay the off-beats by `amount`
    /// (`swingBy`).
    pub fn swing_by(&self, amount: Frac, n: impl Into<Frac>) -> Pattern {
        self.inside(n, move |p| {
            p.late(seq2(Frac::zero(), amount / Frac::int(2)))
        })
    }
    /// `swingBy(1/3, n)` (`swing`).
    pub fn swing(&self, n: impl Into<Frac>) -> Pattern {
        self.swing_by(Frac::new(1, 3), n)
    }

    /// Squeeze each cycle into `[b, e]`, leaving a gap (`compress`).
    pub fn compress(&self, b: impl Into<Frac>, e: impl Into<Frac>) -> Pattern {
        self._compress(frac(b), frac(e))
    }
    /// Like `compress` without gaps; can exceed a cycle (`focus`).
    pub fn focus(&self, b: impl Into<Frac>, e: impl Into<Frac>) -> Pattern {
        self._focus(frac(b), frac(e))
    }

    /// `collect`: group simultaneous (congruent) haps into a single hap whose
    /// value is a [`Value::List`] of the grouped values, preserving order.
    pub fn collect(&self) -> Pattern {
        self.with_haps(|haps, _| {
            let mut groups: Vec<(Option<TimeSpan>, TimeSpan, Vec<Value>, Context)> = Vec::new();
            for hap in haps {
                match groups
                    .iter_mut()
                    .find(|(w, p, _, _)| *w == hap.whole && *p == hap.part)
                {
                    Some(group) => group.2.push(hap.value),
                    None => groups.push((hap.whole, hap.part, vec![hap.value], hap.context)),
                }
            }
            groups
                .into_iter()
                .map(|(whole, part, values, ctx)| {
                    Hap::new(whole, part, Value::List(values)).set_context(ctx)
                })
                .collect()
        })
    }

    /// `arpWith`: collect simultaneous notes into chords, then for each chord
    /// build a pattern with `func` (given the chord's values) and play it within
    /// the chord's timespan.
    pub fn arp_with<F>(&self, func: F) -> Pattern
    where
        F: Fn(&[Value]) -> Pattern + Send + Sync + 'static,
    {
        let func = Arc::new(func);
        self.collect().inner_bind(move |list_val| {
            let notes = match list_val {
                Value::List(v) => v,
                other => vec![other],
            };
            if notes.is_empty() {
                silence()
            } else {
                func(&notes)
            }
        })
    }

    /// `arp`: arpeggiate chords by selecting their notes with an index pattern
    /// (`haps[i % len]`), e.g. `note("<[c,eb,g]>").arp("0 1 2 1")`.
    pub fn arp(&self, indices: impl IntoPattern) -> Pattern {
        let indices = indices.into_pattern();
        self.arp_with(move |notes| {
            let notes = Arc::new(notes.to_vec());
            indices.clone().fmap(move |idx| {
                let i = idx.as_f64().unwrap_or(0.0).max(0.0) as usize;
                notes[i % notes.len()].clone()
            })
        })
    }

    /// `arpeggiate`: play each chord's notes in sequence across its timespan.
    pub fn arpeggiate(&self) -> Pattern {
        self.arp_with(|notes| {
            let pats: Vec<Pattern> = notes.iter().cloned().map(pure).collect();
            fastcat(&pats)
        })
    }
}

fn seq2(a: Frac, b: Frac) -> Pattern {
    fastcat(&[pure(Value::Frac(a)), pure(Value::Frac(b))])
}

/// Pick one of the patterns at random each cycle (`randcat`/`chooseCycles`).
pub fn randcat(pats: &[Pattern]) -> Pattern {
    if pats.is_empty() {
        return silence();
    }
    let pats: Vec<Pattern> = pats.to_vec();
    let len = pats.len();
    let chooser = rand().segment(1);
    let pats = Arc::new(pats);
    chooser
        .fmap(move |v| {
            let idx = ((v.as_f64().unwrap_or(0.0) * len as f64) as usize).min(len - 1);
            Value::Pat(Box::new(pats[idx].clone()))
        })
        .inner_join()
}

/// Shared core of the weighted choosers. `chooser` is a 0..1 signal; each pair
/// is `(pattern, weight)`. Returns a pattern-of-patterns ready to be joined.
fn wchoose_with(chooser: Pattern, pairs: &[(Pattern, f64)]) -> Pattern {
    let pats: Vec<Pattern> = pairs.iter().map(|(p, _)| p.clone()).collect();
    // Running cumulative weights, so a uniform draw maps to a weighted index.
    let mut total = 0.0;
    let cumulative: Vec<f64> = pairs
        .iter()
        .map(|(_, w)| {
            total += w.max(0.0);
            total
        })
        .collect();
    if total <= 0.0 {
        return silence();
    }
    let pats = Arc::new(pats);
    let cumulative = Arc::new(cumulative);
    chooser.fmap(move |v| {
        let target = v.as_f64().unwrap_or(0.0) * total;
        let idx = cumulative
            .iter()
            .position(|&c| c > target)
            .unwrap_or(pats.len() - 1);
        Value::Pat(Box::new(pats[idx].clone()))
    })
}

/// `wchoose`: continuously choose from weighted `(pattern, weight)` pairs.
pub fn wchoose(pairs: &[(Pattern, f64)]) -> Pattern {
    if pairs.is_empty() {
        return silence();
    }
    wchoose_with(rand(), pairs).outer_join()
}

/// `wchooseCycles`/`wrandcat`: pick one weighted pattern per cycle.
pub fn wrandcat(pairs: &[(Pattern, f64)]) -> Pattern {
    if pairs.is_empty() {
        return silence();
    }
    wchoose_with(rand().segment(Frac::one()), pairs).inner_join()
}

/// `stepalt`: alternate stepwise between groups, taking one element from each
/// group per cycle and cycling within each group independently. The result's
/// step count is the sum of the chosen patterns' steps.
pub fn stepalt(groups: &[Vec<Pattern>]) -> Pattern {
    if groups.is_empty() {
        return silence();
    }
    // Repeat for LCM(group lengths) cycles so every group realigns.
    let cycles = groups
        .iter()
        .map(|g| Frac::int(g.len().max(1) as i64))
        .reduce(|a, b| a.lcm(b))
        .unwrap_or_else(Frac::one);
    let cycles = (cycles.to_f64().round() as i64).max(1);
    let mut chosen: Vec<Pattern> = Vec::new();
    for cycle in 0..cycles {
        for group in groups {
            if group.is_empty() {
                continue;
            }
            chosen.push(group[(cycle as usize) % group.len()].clone());
        }
    }
    crate::pattern::stepcat(&chosen)
}

/// Reduce `":"`-separated list values to a single number (`ratio`).
pub fn ratio_value(v: &Value) -> Value {
    match v {
        Value::List(items) if !items.is_empty() => {
            let mut acc = items[0].as_f64().unwrap_or(0.0);
            for item in &items[1..] {
                acc /= item.as_f64().unwrap_or(1.0);
            }
            Value::F64(acc)
        }
        other => other.clone(),
    }
}

/// Build a pattern that cycles randomly through values each cycle. Convenience
/// wrapper over [`randcat`].
pub fn choose_cycles<I, T>(items: I) -> Pattern
where
    I: IntoIterator<Item = T>,
    T: Into<Value>,
{
    let pats: Vec<Pattern> = items
        .into_iter()
        .map(|v| value_to_pattern(v.into()))
        .collect();
    randcat(&pats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seq;

    fn values(pat: &Pattern, b: i64, e: i64) -> Vec<Value> {
        let mut haps = pat.query_arc(Frac::int(b), Frac::int(e));
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    }

    #[test]
    fn iter_shifts_each_cycle() {
        // "0 1 2 3".iter(4): cycle 0 -> 0 1 2 3, cycle 1 -> 1 2 3 0
        let pat = seq([0, 1, 2, 3]).iter(4);
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert_eq!(
            values(&pat, 1, 2),
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(0)]
        );
    }

    #[test]
    fn palindrome_alternates() {
        let pat = seq([0, 1, 2]).palindrome();
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        assert_eq!(
            values(&pat, 1, 2),
            vec![Value::Int(2), Value::Int(1), Value::Int(0)]
        );
    }

    #[test]
    fn last_of_applies_on_last_cycle() {
        // every-from-last: cycle 0,1 -> original; cycle 2 -> reversed (n=3)
        let pat = seq([0, 1, 2]).last_of(3, |p| p.rev());
        assert_eq!(values(&pat, 0, 1)[0], Value::Int(0));
        assert_eq!(values(&pat, 2, 3)[0], Value::Int(2));
    }

    #[test]
    fn within_only_affects_first_half() {
        // apply +10 only to events whose onset is in [0, 0.4] -> events 0 and 1
        let pat = seq([0, 1, 2, 3]).within(Frac::zero(), Frac::new(2, 5), |p| p.add(10));
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(10), Value::Int(11), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn jux_pans_two_copies() {
        // note(0).jux(rev) -> two haps, panned 0 and 1
        let pat = crate::note(pure(Value::Int(0))).jux(|p| p.rev());
        let pans: Vec<f64> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter_map(|h| match h.value {
                Value::Map(m) => m.get("pan").and_then(|v| v.as_f64()),
                _ => None,
            })
            .collect();
        assert!(pans.contains(&0.0) && pans.contains(&1.0));
    }

    #[test]
    fn chunk_applies_to_one_part_per_cycle() {
        // "0 1 2 3".chunk(4, +10): cycle 0 -> first element bumped
        let pat = seq([0, 1, 2, 3]).chunk(4, |p| p.add(10));
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(10), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert_eq!(
            values(&pat, 1, 2),
            vec![Value::Int(0), Value::Int(11), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn modulo_wraps_values() {
        let pat = seq([3, 4, 5]).modulo(3);
        assert_eq!(
            values(&pat, 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
    }

    #[test]
    fn zoom_plays_a_slice() {
        // "0 1 2 3".zoom(0.5, 1) plays "2 3" across the cycle
        let pat = seq([0, 1, 2, 3]).zoom(Frac::new(1, 2), Frac::one());
        assert_eq!(values(&pat, 0, 1), vec![Value::Int(2), Value::Int(3)]);
    }

    #[test]
    fn take_keeps_first_and_last_steps() {
        // "0 1 2 3" (4 steps): take(2) -> "0 1", take(-2) -> "2 3"
        let pat = seq([0, 1, 2, 3]);
        assert_eq!(pat.take(2).steps, Some(Frac::int(2)));
        assert_eq!(values(&pat.take(2), 0, 1), vec![Value::Int(0), Value::Int(1)]);
        assert_eq!(values(&pat.take(-2), 0, 1), vec![Value::Int(2), Value::Int(3)]);
        // taking >= all steps returns the pattern; a stepless pattern -> silence
        assert_eq!(values(&pat.take(9), 0, 1).len(), 4);
        assert!(rand().take(2).query_arc(Frac::zero(), Frac::one()).is_empty());
    }

    #[test]
    fn drop_discards_first_and_last_steps() {
        // "0 1 2 3": drop(1) -> "1 2 3", drop(-1) -> "0 1 2"
        let pat = seq([0, 1, 2, 3]);
        assert_eq!(
            values(&pat.drop(1), 0, 1),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert_eq!(
            values(&pat.drop(-1), 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
    }

    #[test]
    fn wrandcat_picks_one_per_cycle_weighted() {
        // A vastly heavier weight should dominate; each cycle yields one value.
        let pairs = [(pure(Value::Int(0)), 1000.0), (pure(Value::Int(1)), 1.0)];
        let pat = wrandcat(&pairs);
        let mut zeros = 0;
        for c in 0..12 {
            let v = values(&pat, c, c + 1);
            assert_eq!(v.len(), 1, "one value per cycle");
            assert!(v[0] == Value::Int(0) || v[0] == Value::Int(1));
            if v[0] == Value::Int(0) {
                zeros += 1;
            }
        }
        assert!(zeros >= 10, "heavy weight should dominate (got {zeros}/12)");
    }

    #[test]
    fn wchoose_is_continuous_in_set() {
        // Segmenting the continuous chooser yields values from the set.
        let pairs = [(pure(Value::Int(5)), 1.0), (pure(Value::Int(9)), 1.0)];
        let pat = wchoose(&pairs).segment(Frac::int(8));
        let got = values(&pat, 0, 1);
        assert_eq!(got.len(), 8);
        assert!(got.iter().all(|v| *v == Value::Int(5) || *v == Value::Int(9)));
    }

    #[test]
    fn collect_groups_simultaneous_haps() {
        // three stacked values collapse into one hap holding a list
        let pat = stack(&[pure(Value::Int(0)), pure(Value::Int(1)), pure(Value::Int(2))]).collect();
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 1);
        assert_eq!(
            haps[0].value,
            Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)])
        );
    }

    #[test]
    fn arp_selects_chord_notes_by_index() {
        let chord = stack(&[pure(Value::Int(0)), pure(Value::Int(1)), pure(Value::Int(2))]);
        // "0 1 2" walks up the chord
        assert_eq!(
            values(&chord.arp(seq([0, 1, 2])), 0, 1),
            vec![Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        // indices wrap and may reorder
        assert_eq!(
            values(&chord.arp(seq([2, 0, 3])), 0, 1),
            vec![Value::Int(2), Value::Int(0), Value::Int(0)]
        );
    }

    #[test]
    fn arpeggiate_plays_chord_in_sequence() {
        let chord = stack(&[pure(Value::Int(5)), pure(Value::Int(7)), pure(Value::Int(9))]);
        assert_eq!(
            values(&chord.arpeggiate(), 0, 1),
            vec![Value::Int(5), Value::Int(7), Value::Int(9)]
        );
    }

    #[test]
    fn stepalt_alternates_groups_stepwise() {
        // stepalt(["0 1", "2"], "3") == "0 1 3 2 3"
        let group0 = vec![seq([0, 1]), seq([2])];
        let group1 = vec![seq([3])];
        let pat = stepalt(&[group0, group1]);
        assert_eq!(pat.steps, Some(Frac::int(5)));
        assert_eq!(
            values(&pat, 0, 1),
            vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(3),
                Value::Int(2),
                Value::Int(3),
            ]
        );
    }
}
