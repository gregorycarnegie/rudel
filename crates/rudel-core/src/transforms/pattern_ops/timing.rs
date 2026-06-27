use super::helpers::{frac, seq2};
use crate::{
    fraction::Frac,
    pattern::{Pattern, fastcat, pure, silence, slowcat, stack},
    timespan::TimeSpan,
    transforms::IntoPattern,
    value::Value,
};

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

    fn chunk_impl<F>(&self, n: i64, f: F, back: bool, fast: bool) -> Pattern
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
        // `fast` chunks loop a subcycle without slowing the source down.
        let base = if fast {
            self.clone()
        } else {
            self.repeat_cycles(n)
        };
        base.when(binary_pat, f)
    }

    /// Cycle through `n` chunks, applying `f` to one chunk per cycle (`chunk`,
    /// a.k.a. `slowChunk`).
    pub fn chunk<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.chunk_impl(n, f, false, false)
    }
    /// Like `chunk`, but moves backwards through the chunks (`chunkBack`).
    pub fn chunk_back<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.chunk_impl(n, f, true, false)
    }
    /// Like `chunk`, but applied to a looped subcycle rather than slowing the
    /// source down (`fastChunk`).
    pub fn fast_chunk<F>(&self, n: i64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.chunk_impl(n, f, false, true)
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

    /// Repeat the first `t` of the cycle to fill it (`linger`). Negative `t`
    /// lingers on the *end* of the cycle.
    pub fn linger(&self, t: Frac) -> Pattern {
        if t == Frac::zero() {
            return silence();
        }
        if t < Frac::zero() {
            self.zoom(t + Frac::one(), Frac::one())._slow(t)
        } else {
            self.zoom(Frac::zero(), t)._slow(t)
        }
    }

    /// Replicate the pattern `factor` times within the cycle, growing the step
    /// count to match (`replicate`).
    pub fn replicate(&self, factor: i64) -> Pattern {
        self.repeat_cycles(factor)
            ._fast(Frac::int(factor))
            .expand(factor)
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

    /// `ribbon`/`rib`: cut a `cycles`-long window starting at cycle `offset` out
    /// of the (infinite) timeline and loop it forever. Like `note("<c d e f>")
    /// .ribbon(1, 2)` playing `d e` on repeat.
    pub fn ribbon(&self, offset: impl Into<Frac>, cycles: impl Into<Frac>) -> Pattern {
        let cycles = cycles.into();
        if cycles <= Frac::zero() {
            return silence();
        }
        // Strudel: pat.early(offset).restart(pure(1).slow(cycles)).
        let trigger = pure(Value::Int(1))._slow(cycles);
        self._early(offset.into()).keep_restart(trigger)
    }

    /// Alias for [`ribbon`](Self::ribbon).
    pub fn rib(&self, offset: impl Into<Frac>, cycles: impl Into<Frac>) -> Pattern {
        self.ribbon(offset, cycles)
    }
}
