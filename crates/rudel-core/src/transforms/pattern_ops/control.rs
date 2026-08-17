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
        stack(&[self._degrade_by(prob), f(&self._undegrade_by(1.0 - prob))])
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
        self._undegrade_by(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{fastcat, pure};

    fn notes(pat: &Pattern, cycles: i64) -> Vec<f64> {
        pat.query_arc(Frac::zero(), Frac::int(cycles))
            .into_iter()
            .filter_map(|h| h.value.as_f64())
            .collect()
    }

    fn two() -> Pattern {
        fastcat(&[pure(Value::Int(0)), pure(Value::Int(1))])
    }

    #[test]
    fn keepif_keeps_the_values_the_mask_lets_through() {
        let mask = fastcat(&[pure(Value::Bool(true)), pure(Value::Bool(false))]);
        assert_eq!(notes(&two().keepif(mask), 1), vec![0.0]);
    }

    #[test]
    fn invert_swaps_both_ways() {
        let flipped = two().invert();
        let truthy: Vec<bool> = flipped
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value.truthy())
            .collect();
        assert_eq!(truthy, vec![true, false]);
    }

    #[test]
    fn bypass_silences_only_while_it_is_on() {
        assert!(notes(&two().bypass(1), 1).is_empty());
        assert_eq!(notes(&two().bypass(0), 1), vec![0.0, 1.0]);
    }

    #[test]
    fn sometimes_by_at_the_extremes_is_all_or_nothing() {
        let plus = |p: &Pattern| p.add(100);
        // Cycle 1, not 0: `rand` is exactly 0 at time 0, which `degradeBy`'s
        // strict `>` drops whatever the probability is.
        let cycle1 = |pat: Pattern| -> Vec<f64> {
            pat.query_arc(Frac::one(), Frac::int(2))
                .into_iter()
                .filter_map(|h| h.value.as_f64())
                .collect()
        };
        // Never, and always: `1 - prob` decides how much of the pattern the
        // transformed copy keeps.
        assert_eq!(cycle1(two().sometimes_by(0.0, plus)), vec![0.0, 1.0]);
        assert_eq!(cycle1(two().sometimes_by(1.0, plus)), vec![100.0, 101.0]);
    }

    #[test]
    fn some_cycles_by_transforms_whole_cycles_and_never_both_copies() {
        let plus = |p: &Pattern| p.add(100);
        let pat = pure(Value::Int(0)).some_cycles_by(0.5, plus);
        let per_cycle: Vec<f64> = (0..12)
            .map(|c| {
                let haps = pat.query_arc(Frac::int(c), Frac::int(c + 1));
                assert_eq!(haps.len(), 1, "cycle {c} played both copies or neither");
                haps[0].value.as_f64().unwrap()
            })
            .collect();
        // Both branches happen over twelve cycles, and each is one or the
        // other — which only holds while the two masks are complements.
        assert!(per_cycle.contains(&0.0), "{per_cycle:?}");
        assert!(per_cycle.contains(&100.0), "{per_cycle:?}");
    }

    #[test]
    fn jux_flip_swaps_the_ears_on_alternate_cycles() {
        let pan = |pat: &Pattern, cycle: i64| -> Vec<f64> {
            pat.query_arc(Frac::int(cycle), Frac::int(cycle + 1))
                .into_iter()
                .filter_map(|h| match &h.value {
                    Value::Map(m) => m.get("pan").and_then(Value::as_f64),
                    _ => None,
                })
                .collect()
        };
        let pat = crate::controls::s(pure(Value::Str("bd".into()))).jux_flip_by(1.0, |p| p.clone());
        let first = pan(&pat, 0);
        let second = pan(&pat, 1);
        assert_eq!(first.len(), 2);
        // The second cycle is the first with its ears exchanged.
        assert_eq!(second, vec![first[1], first[0]]);
    }

    #[test]
    fn echo_delays_each_copy_by_a_multiple_of_the_time() {
        let pat = pure(Value::Int(0)).echo(3, Frac::new(1, 4), 0.5);
        let mut haps: Vec<(Frac, f64)> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            // Onsets only: the late copies also leave the tail of the previous
            // cycle's events inside this one.
            .filter(|h| h.has_onset())
            .map(|h| {
                let gain = match &h.value {
                    Value::Map(m) => m.get("gain").and_then(Value::as_f64).unwrap(),
                    other => panic!("expected a control map, got {other:?}"),
                };
                (h.part.begin, gain)
            })
            .collect();
        haps.sort_by_key(|(t, _)| *t);
        // Copy `i` is `i` quarter-cycles late with gain `0.5^i`.
        assert_eq!(
            haps,
            vec![
                (Frac::zero(), 1.0),
                (Frac::new(1, 4), 0.5),
                (Frac::new(1, 2), 0.25),
            ]
        );
    }
}
