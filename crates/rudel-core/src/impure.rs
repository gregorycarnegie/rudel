// impure.rs - stateful/impure pattern methods, ported from
// strudel/packages/core/impure.mjs (the "file of shame"). `timeline` carries
// cross-query state used to align live-coded patterns to cue points.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Per-timeline cycle offsets, keyed by timeline id. Process-global and
/// long-lived, mirroring Strudel's module-level `timelines` singleton: once a
/// timeline id is activated (first seen from the scheduler) its offset persists
/// until it is reset or the id is negated.
static TIMELINES: LazyLock<RwLock<HashMap<Frac, Frac>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Clear all stored timeline offsets (`reset_timelines`). Called on REPL
/// reset/restart so cued patterns realign from scratch.
pub fn reset_timelines() {
    TIMELINES.write().unwrap().clear();
}

/// Reset impure pattern state (`reset_state`). Currently just the timelines.
pub fn reset_state() {
    reset_timelines();
}

impl Pattern {
    /// Switch a pattern between numbered "timelines", for cueing patterns up
    /// when live coding (`timeline`). The timeline id pattern `tpat` selects an
    /// offset: id `0` plays unshifted; a fresh id captures the offset of the
    /// cycle it first appears in (or the *next* cycle if first seen more than
    /// halfway through, so a just-typed timeline starts cleanly), and that
    /// offset then persists. Negating an id resets it (the opposite-signed
    /// entry is dropped). Only scheduler queries (those carrying the `cyclist`
    /// marker) mutate the persistent state; other queries (e.g. the visualiser)
    /// read it but never write, matching Strudel.
    pub fn timeline(&self, tpat: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        let tpat = tpat.into_pattern();
        let steps = self.steps;
        Pattern::new(move |state| {
            let scheduler = state.controls.contains_key("cyclist");
            let mut result = Vec::new();
            for timehap in tpat.query(state) {
                let tlid = timehap.value.to_frac();
                let offset = if tlid == Frac::zero() {
                    Frac::zero()
                } else if let Some(off) = TIMELINES.read().unwrap().get(&tlid).copied() {
                    off
                } else {
                    let arc = timehap.whole_or_part();
                    if !scheduler || state.span.begin < arc.midpoint() {
                        arc.begin
                    } else {
                        arc.end
                    }
                };
                if scheduler {
                    let mut tl = TIMELINES.write().unwrap();
                    tl.insert(tlid, offset);
                    if tlid != Frac::zero() {
                        tl.remove(&(-tlid));
                    }
                }
                let inner = state.set_span(timehap.part);
                for h in pat._late(offset).query(&inner) {
                    let ctx = h.combine_context(&timehap);
                    result.push(h.set_context(ctx));
                }
            }
            result
        })
        .set_steps(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use crate::timespan::TimeSpan;
    use crate::value::Value;
    use crate::value::ValueMap;
    use crate::{pure, sequence};

    fn cycle(n: i64) -> TimeSpan {
        TimeSpan::new(Frac::int(n), Frac::int(n + 1))
    }

    /// Query a pattern over one cycle, optionally marking the query as coming
    /// from the scheduler (sets the `cyclist` control like the back-ends do).
    fn query_cycle(pat: &Pattern, n: i64, scheduler: bool) -> Vec<crate::hap::Hap> {
        let controls = if scheduler {
            ValueMap::from([("cyclist".to_string(), Value::Str("cyclist".into()))])
        } else {
            ValueMap::new()
        };
        pat.query(&State::with_controls(cycle(n), controls))
    }

    // TIMELINES is process-global, so (like the MIDI CC-bus tests) these use
    // disjoint timeline ids rather than `reset_timelines`, which would clear
    // state out from under tests running in parallel.

    #[test]
    fn timeline_zero_plays_unshifted() {
        // timeline(0) is a no-op offset: the pattern plays as-is, and id 0 is
        // never stored.
        let pat =
            sequence(&[pure(Value::Int(0)), pure(Value::Int(1))]).timeline(pure(Value::Int(0)));
        let haps = query_cycle(&pat, 3, true);
        let vals: Vec<_> = haps.iter().map(|h| h.value.clone()).collect();
        assert_eq!(vals, vec![Value::Int(0), Value::Int(1)]);
        assert_eq!(haps[0].part.begin, Frac::int(3));
    }

    #[test]
    fn timeline_offsets_to_the_activation_cycle() {
        // Activating timeline 11 at cycle 3 captures offset 3, so the cycle-3
        // query sees the inner pattern's cycle 0 (it is shifted late by 3).
        let inner = sequence(&[pure(Value::Int(10)), pure(Value::Int(20))]);
        let pat = inner.timeline(pure(Value::Int(11)));
        let haps = query_cycle(&pat, 3, true);
        let vals: Vec<_> = haps.iter().map(|h| h.value.clone()).collect();
        assert_eq!(vals, vec![Value::Int(10), Value::Int(20)]);
        // The captured offset persists in the global map.
        assert_eq!(
            TIMELINES.read().unwrap().get(&Frac::int(11)).copied(),
            Some(Frac::int(3))
        );
    }

    #[test]
    fn non_scheduler_query_does_not_mutate_state() {
        let pat = pure(Value::Int(0)).timeline(pure(Value::Int(12)));
        // A visualiser-style query (no cyclist marker) must not write state.
        let _ = query_cycle(&pat, 2, false);
        assert!(TIMELINES.read().unwrap().get(&Frac::int(12)).is_none());
    }

    #[test]
    fn negating_a_timeline_id_resets_it() {
        let inner = pure(Value::Int(7));
        // Activate timeline 13 at cycle 4 -> offset 4 stored.
        let _ = query_cycle(&inner.timeline(pure(Value::Int(13))), 4, true);
        assert_eq!(
            TIMELINES.read().unwrap().get(&Frac::int(13)).copied(),
            Some(Frac::int(4))
        );
        // Switching to -13 at cycle 9 drops the +13 entry and stores -13 -> 9.
        let _ = query_cycle(&inner.timeline(pure(Value::Int(-13))), 9, true);
        let tl = TIMELINES.read().unwrap();
        assert!(
            tl.get(&Frac::int(13)).is_none(),
            "the +13 entry should be reset"
        );
        assert_eq!(tl.get(&Frac::int(-13)).copied(), Some(Frac::int(9)));
    }
}
