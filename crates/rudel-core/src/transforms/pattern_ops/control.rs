use super::helpers::set_key;
use crate::{
    fraction::Frac,
    pattern::{Pattern, silence, stack},
    signal::rand,
    state::State,
    transforms::IntoPattern,
    value::Value,
};

impl Pattern {
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

    /// Like [`jux_by`](Self::jux_by), but swaps the ears each cycle
    /// (`juxFlipBy`/`fluxBy`): Strudel's `juxBy(slowcat(by, -by), f)`.
    pub fn jux_flip_by<F>(&self, by: f64, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        crate::pattern::slowcat_prime(&[self.jux_by(by, &f), self.jux_by(-by, &f)])
    }
    /// `juxFlipBy(1, f)` (`juxFlip`/`flux`).
    pub fn jux_flip<F>(&self, f: F) -> Pattern
    where
        F: Fn(&Pattern) -> Pattern,
    {
        self.jux_flip_by(1.0, f)
    }

    /// Keep this pattern's whole value where `other` is truthy, else drop the
    /// event (`keepif`). Structure comes from this pattern, so unlike the other
    /// composers it keeps the control value intact rather than merging maps.
    pub fn keepif(&self, other: impl IntoPattern) -> Pattern {
        self.fmap(|a| Value::func(move |b| if b.truthy() { a.clone() } else { Value::Null }))
            .app_left(&other.into_pattern())
            .filter_values(|v| !matches!(v, Value::Null))
    }

    /// Swap true/false in a boolean pattern (`invert`/`inv`).
    pub fn invert(&self) -> Pattern {
        self.fmap(|x| Value::Bool(!x.truthy()))
    }

    /// Silence this pattern when `on` is truthy, else play it unchanged
    /// (`bypass`). `on` may be a pattern, sampled per cycle.
    pub fn bypass(&self, on: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        on.into_pattern()
            .fmap(move |v| {
                let muted = v.as_f64().unwrap_or(0.0) != 0.0;
                Value::Pat(Box::new(if muted { silence() } else { pat.clone() }))
            })
            .inner_join()
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

    /// `seed(n)`: set the `randSeed` control for this pattern, changing the
    /// output of `rand` (and everything built on it: `degrade`, `shuffle`,
    /// `sometimes`, ...). Mirrors Strudel's `withSeed(() => n, pat)`.
    pub fn seed(&self, n: Frac) -> Pattern {
        let pat = self.clone();
        Pattern::new(move |state| {
            let mut controls = state.controls.clone();
            controls.insert("randSeed".to_string(), Value::Frac(n));
            pat.query(&State::with_controls(state.span, controls))
        })
        .set_steps(self.steps)
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

    /// `undegradeBy(0.5)` (`undegrade`).
    pub fn undegrade(&self) -> Pattern {
        self.undegrade_by(0.5)
    }
}
