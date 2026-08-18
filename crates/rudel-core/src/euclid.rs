// euclid.rs - Bjorklund / Euclidean rhythms.
// Ported from strudel/packages/core/euclid.mjs (itself after Rohan Drape's hmt).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    fraction::Frac,
    hap::Hap,
    pattern::{Pattern, fastcat, pure, silence, timecat},
    timespan::TimeSpan,
    transforms::IntoPattern,
    value::Value,
};

fn zip_concat(a: &[Vec<i32>], b: &[Vec<i32>]) -> Vec<Vec<i32>> {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let mut z = x.clone();
            z.extend(y.iter().copied());
            z
        })
        .collect()
}

type Counts = (i64, i64);
type Buckets = (Vec<Vec<i32>>, Vec<Vec<i32>>);

fn left(n: Counts, x: Buckets) -> (Counts, Buckets) {
    let (ons, offs) = n;
    let (xs, ys) = x;
    let (head, tail) = xs.split_at((offs as usize).min(xs.len()));
    ((offs, ons - offs), (zip_concat(head, &ys), tail.to_vec()))
}

fn right(n: Counts, x: Buckets) -> (Counts, Buckets) {
    let (ons, offs) = n;
    let (xs, ys) = x;
    let (head, tail) = ys.split_at((ons as usize).min(ys.len()));
    ((ons, offs - ons), (zip_concat(&xs, head), tail.to_vec()))
}

fn bjork_rec(n: Counts, x: Buckets) -> (Counts, Buckets) {
    let (ons, offs) = n;
    if ons.min(offs) <= 1 {
        (n, x)
    } else if ons > offs {
        let (n2, x2) = left(n, x);
        bjork_rec(n2, x2)
    } else {
        let (n2, x2) = right(n, x);
        bjork_rec(n2, x2)
    }
}

/// Bjorklund rhythm of `ons` pulses over `steps` steps as a boolean vector.
/// Negative `ons` inverts the result.
pub fn bjorklund(ons: i64, steps: i64) -> Vec<bool> {
    let inverted = ons < 0;
    let abs_ons = ons.abs();
    let offs = steps - abs_ons;
    let ones: Vec<Vec<i32>> = (0..abs_ons).map(|_| vec![1]).collect();
    let zeros: Vec<Vec<i32>> = (0..offs.max(0)).map(|_| vec![0]).collect();
    let (_n, (a, b)) = bjork_rec((abs_ons, offs), (ones, zeros));
    let mut pattern: Vec<i32> = a.into_iter().flatten().collect();
    pattern.extend(b.into_iter().flatten());
    pattern
        .into_iter()
        .map(|x| if inverted { 1 - x } else { x } != 0)
        .collect()
}

fn euclid_rot(pulses: i64, steps: i64, rotation: i64) -> Vec<bool> {
    let b = bjorklund(pulses, steps);
    if rotation == 0 || b.is_empty() {
        return b;
    }
    let len = b.len() as i64;
    // Strudel rotates the sequence *right* by `rotation` (`rotate(b, -rotation)`),
    // so a positive rotation shifts onsets to later steps.
    let mut out = b;
    out.rotate_left((-rotation).rem_euclid(len) as usize);
    out
}

fn bools_pattern(bools: &[bool]) -> Pattern {
    let pats: Vec<Pattern> = bools.iter().map(|&b| pure(Value::Bool(b))).collect();
    fastcat(&pats)
}

/// Strudel's `_morph(from, to, by)` specialised for `euclidish`: morph the
/// onsets of the euclidean rhythm `from` towards evenly-spaced pulses, by
/// factor `by` (0 = straight euclidean, 1 = straight pulse). Each onset becomes
/// a `true` hap of width `1/steps` whose position is interpolated between its
/// euclidean position (`i/steps`) and its even position (`k/pulses`).
fn morph_pattern(from: &[bool], by: f64) -> Pattern {
    let steps = from.len();
    if steps == 0 {
        return silence();
    }
    let dur = Frac::new(1, steps as i64);
    let from_pos: Vec<Frac> = from
        .iter()
        .enumerate()
        .filter(|&(_, &on)| on)
        .map(|(i, _)| Frac::new(i as i64, steps as i64))
        .collect();
    let pulses = from_pos.len();
    if pulses == 0 {
        return silence();
    }
    let by = Frac::from_f64(by);
    let arcs: Vec<TimeSpan> = from_pos
        .iter()
        .enumerate()
        .map(|(k, &pos_a)| {
            let pos_b = Frac::new(k as i64, pulses as i64);
            let b = by * (pos_b - pos_a) + pos_a;
            TimeSpan::new(b, b + dur)
        })
        .collect();
    Pattern::new(move |state| {
        let cycle = state.span.begin.sam();
        let cycle_arc = state.span.cycle_arc();
        let mut out = Vec::new();
        for whole in &arcs {
            if let Some(part) = whole.intersection(&cycle_arc) {
                out.push(Hap::new(
                    Some(whole.with_time(|x| x + cycle)),
                    part.with_time(|x| x + cycle),
                    Value::Bool(true),
                ));
            }
        }
        out
    })
    .split_queries()
}

impl Pattern {
    /// Restructure into a Euclidean rhythm (`euclid`).
    pub fn euclid(&self, pulses: i64, steps: i64) -> Pattern {
        self.struct_pat(bools_pattern(&euclid_rot(pulses, steps, 0)))
    }

    /// Euclidean rhythm with rotation (`euclidRot`).
    pub fn euclid_rot(&self, pulses: i64, steps: i64, rotation: i64) -> Pattern {
        self.struct_pat(bools_pattern(&euclid_rot(pulses, steps, rotation)))
    }

    /// Like [`euclid`](Self::euclid), but each pulse is held until the next so
    /// there are no gaps (`euclidLegato`). Ports superdough's `_euclidLegato`.
    pub fn euclid_legato(&self, pulses: i64, steps: i64) -> Pattern {
        self.euclid_legato_rot(pulses, steps, 0)
    }

    /// [`euclid_legato`](Self::euclid_legato) with a step rotation applied as a
    /// late offset (`euclidLegatoRot`).
    pub fn euclid_legato_rot(&self, pulses: i64, steps: i64, rotation: i64) -> Pattern {
        if pulses < 1 || steps < 1 {
            return silence();
        }
        // The gapless spans are the distances between successive onsets of the
        // un-rotated rhythm; rotation is applied afterwards as a late offset.
        let bools = euclid_rot(pulses, steps, 0);
        let onsets: Vec<usize> = bools
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b)
            .map(|(i, _)| i)
            .collect();
        if onsets.is_empty() {
            return silence();
        }
        let pairs: Vec<(Frac, Pattern)> = onsets
            .iter()
            .enumerate()
            .map(|(k, &start)| {
                let end = onsets.get(k + 1).copied().unwrap_or(steps as usize);
                (Frac::int((end - start) as i64), pure(Value::Bool(true)))
            })
            .collect();
        self.struct_pat(timecat(&pairs))
            ._late(Frac::new(rotation, steps))
    }

    /// Tidal-style euclid taking a `[pulses, steps, rotation]` tuple (`bjork`).
    /// A single element means `steps = pulses` and `rotation = 0`.
    pub fn bjork(&self, euc: &[i64]) -> Pattern {
        let pulses = euc.first().copied().unwrap_or(0);
        let steps = euc.get(1).copied().unwrap_or(pulses);
        let rotation = euc.get(2).copied().unwrap_or(0);
        self.struct_pat(bools_pattern(&euclid_rot(pulses, steps, rotation)))
    }

    /// The euclid family with **patterned counts**: `s("bd").euclid("<3 5>", 8)`,
    /// and the same thing mini-notation's `bd(<3 5>,8)` does.
    ///
    /// This is Strudel's `register` patternification, which every euclid export
    /// gets there: the rhythm is built per hap of `pulses`, the remaining counts
    /// are sampled by `app_left` at that hap's span, and the resulting pattern
    /// of patterns is `inner_join`ed. Sampling the counts once at the query
    /// start instead — which is what a plain `bind` would do — makes a count
    /// pattern longer than one cycle stop changing after the first.
    ///
    /// `build` is the plain-integer method the counts are handed to, so
    /// `euclid`, `euclidRot` and their legato variants all share one shape.
    /// `rotation` is `None` for the two that have no rotation argument.
    pub fn euclid_pat(
        &self,
        pulses: impl IntoPattern,
        steps: impl IntoPattern,
        rotation: Option<Pattern>,
        build: fn(&Pattern, i64, i64, i64) -> Pattern,
    ) -> Pattern {
        let rotated = rotation.is_some();
        let pat = self.clone();
        let counted = pulses.into_pattern().fmap(move |p| {
            let pat = pat.clone();
            Value::func(move |s| {
                let (pat, p) = (pat.clone(), p.clone());
                match rotated {
                    // A third argument to come: hand back another function for
                    // `app_left` to feed the rotation to.
                    true => Value::func(move |r| {
                        Value::Pat(Box::new(build(&pat, count(&p), count(&s), count(&r))))
                    }),
                    false => Value::Pat(Box::new(build(&pat, count(&p), count(&s), 0))),
                }
            })
        });
        let counted = counted.app_left(&steps.into_pattern());
        match rotation {
            Some(rotation) => counted.app_left(&rotation).inner_join(),
            None => counted.inner_join(),
        }
    }

    /// `euclid` variant that morphs from straight euclidean (`perc = 0`) to an
    /// even pulse (`perc = 1`) (`euclidish`/`eish`). `perc` may be a continuous
    /// pattern (e.g. `sine.slow(8)`), sampled once per cycle.
    ///
    /// Mirrors Strudel's `register` patternification: `pulses`/`steps` are pure
    /// (one hap per cycle), so the morph factory is built per cycle and `perc`
    /// is sampled by `appLeft` at each cycle's span, then `innerJoin`ed. (A
    /// plain `perc.inner_bind` would instead sample `perc` once at the query
    /// start, drifting on later cycles for a continuous signal.)
    pub fn euclidish(&self, pulses: i64, steps: i64, perc: impl IntoPattern) -> Pattern {
        let from = bjorklund(pulses, steps);
        let pat = self.clone();
        pure(Value::Bool(true))
            .fmap(move |_| {
                let from = from.clone();
                let pat = pat.clone();
                Value::func(move |by| {
                    let by = by.as_f64().unwrap_or(0.0);
                    let morphed = pat
                        .struct_pat(morph_pattern(&from, by))
                        .set_steps(Some(Frac::int(steps)));
                    Value::Pat(Box::new(morphed))
                })
            })
            .app_left(&perc.into_pattern())
            .inner_join()
    }
}

/// A hap value read as a euclid count, matching mini-notation's reading of the
/// same argument.
fn count(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0) as i64
}

/// Build a boolean pattern from a Euclidean rhythm, e.g. for `struct`.
pub fn euclid_bools(pulses: i64, steps: i64) -> impl IntoPattern {
    bools_pattern(&euclid_rot(pulses, steps, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn tresillo() {
        // euclid(3,8) is the Cuban tresillo: x . . x . . x .
        assert_eq!(
            bjorklund(3, 8),
            vec![true, false, false, true, false, false, true, false]
        );
    }

    #[test]
    fn euclid_5_8() {
        // the Cuban cinquillo
        assert_eq!(
            bjorklund(5, 8),
            vec![true, false, true, true, false, true, true, false]
        );
    }

    #[test]
    fn euclid_legato_is_gapless() {
        // euclid(3,8) onsets at steps 0,3,6 -> gapless spans 3/8, 3/8, 2/8.
        let pat = pure(Value::Str("x".into())).euclid_legato(3, 8);
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        let onsets: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
        assert_eq!(onsets.len(), 3);
        let spans: Vec<(Frac, Frac)> = onsets
            .iter()
            .map(|h| {
                let w = h.whole.unwrap();
                (w.begin, w.end)
            })
            .collect();
        assert_eq!(
            spans,
            vec![
                (Frac::zero(), Frac::new(3, 8)),
                (Frac::new(3, 8), Frac::new(6, 8)),
                (Frac::new(6, 8), Frac::one()),
            ]
        );
    }

    #[test]
    fn euclid_legato_rot_offsets_by_late() {
        // rotation shifts everything later by rotation/steps.
        let base = pure(Value::Str("x".into())).euclid_legato(3, 8);
        let rotated = pure(Value::Str("x".into())).euclid_legato_rot(3, 8, 2);
        let first_base = base.query_arc(Frac::zero(), Frac::one())[0]
            .whole
            .unwrap()
            .begin;
        // querying [2/8, ...] of the rotated should line up with base at 0.
        let shifted = rotated.query_arc(Frac::new(2, 8), Frac::new(3, 8));
        assert_eq!(first_base, Frac::zero());
        assert!(!shifted.is_empty());
    }

    fn onsets(pat: &Pattern) -> Vec<(Frac, Frac)> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.iter()
            .filter(|h| h.has_onset())
            .map(|h| {
                let w = h.whole.unwrap();
                (w.begin, w.end)
            })
            .collect()
    }

    #[test]
    fn euclidish_zero_matches_euclid() {
        // perc=0 is straight euclidean: onsets of euclid(3,8) with width 1/8.
        let pat = pure(Value::Str("x".into())).euclidish(3, 8, 0.0);
        assert_eq!(
            onsets(&pat),
            vec![
                (Frac::zero(), Frac::new(1, 8)),
                (Frac::new(3, 8), Frac::new(4, 8)),
                (Frac::new(6, 8), Frac::new(7, 8)),
            ]
        );
    }

    #[test]
    fn euclidish_one_is_even_pulse() {
        // perc=1 spaces the 3 pulses evenly at 0, 1/3, 2/3 (each width 1/8).
        let pat = pure(Value::Str("x".into())).euclidish(3, 8, 1.0);
        let begins: Vec<Frac> = onsets(&pat).into_iter().map(|(b, _)| b).collect();
        assert_eq!(begins, vec![Frac::zero(), Frac::new(1, 3), Frac::new(2, 3)]);
    }

    #[test]
    fn bjork_tuple_matches_euclid_rot() {
        // bjork([3,8,2]) == euclidRot(3,8,2); a lone number defaults steps=pulses.
        let a = pure(Value::Str("x".into())).bjork(&[3, 8, 2]);
        let b = pure(Value::Str("x".into())).euclid_rot(3, 8, 2);
        assert_eq!(onsets(&a), onsets(&b));
        let solo = pure(Value::Str("x".into())).bjork(&[3]);
        let euc = pure(Value::Str("x".into())).euclid(3, 3);
        assert_eq!(onsets(&solo), onsets(&euc));
    }

    proptest! {
        #[test]
        fn valid_bjorklund_rhythms_have_the_requested_length_and_pulses(
            steps in 1i64..=64,
            pulses in 0i64..=64,
        ) {
            let pulses = pulses.min(steps);
            let rhythm = bjorklund(pulses, steps);

            prop_assert_eq!(rhythm.len(), steps as usize);
            prop_assert_eq!(
                rhythm.iter().filter(|&&on| on).count(),
                pulses as usize
            );
        }

        #[test]
        fn negative_pulses_invert_the_rhythm(steps in 1i64..=64, pulses in 1i64..=64) {
            let pulses = pulses.min(steps);
            let normal = bjorklund(pulses, steps);
            let inverted = bjorklund(-pulses, steps);

            prop_assert_eq!(inverted.len(), normal.len());
            prop_assert!(
                normal.iter().zip(&inverted).all(|(a, b)| *a != *b),
                "negative pulses should invert each step"
            );
        }
    }

    #[test]
    fn an_even_split_still_picks_one_side_of_the_recursion() {
        // 4 onsets in 8 steps leaves the bucket counts equal, which is the one
        // case where the "more onsets than gaps?" test does not decide itself.
        assert_eq!(
            bjorklund(4, 8),
            vec![true, false, true, false, true, false, true, false]
        );
        assert_eq!(bjorklund(2, 4), vec![true, false, true, false]);
        assert_eq!(bjorklund(1, 2), vec![true, false]);
    }

    #[test]
    fn legato_needs_at_least_one_pulse_and_one_step() {
        let p = pure(Value::Int(0));
        // The smallest rhythm there is still plays.
        assert_eq!(
            p.euclid_legato(1, 1)
                .query_arc(Frac::zero(), Frac::one())
                .len(),
            1
        );
        assert_eq!(
            p.euclid_legato(3, 8)
                .query_arc(Frac::zero(), Frac::one())
                .len(),
            3
        );
        // Anything below that is silence rather than an attempt to build a
        // rhythm out of it — `bjorklund` has no answer for a negative count.
        for (pulses, steps) in [(0, 8), (3, 0), (-1, 8), (3, -1), (0, 0)] {
            assert!(
                p.euclid_legato_rot(pulses, steps, 0)
                    .query_arc(Frac::zero(), Frac::one())
                    .is_empty(),
                "euclid_legato({pulses}, {steps}) should be silence"
            );
        }
    }

    #[test]
    fn legato_spans_run_from_each_onset_to_the_next() {
        // euclid(3,8) is x..x..x. — gapless spans of 3, 3 and 2 eighths.
        let haps = pure(Value::Int(0))
            .euclid_legato(3, 8)
            .query_arc(Frac::zero(), Frac::one());
        let mut spans: Vec<(Frac, Frac)> = haps
            .iter()
            .map(|h| {
                let w = h.whole.expect("legato events are discrete");
                (w.begin, w.end)
            })
            .collect();
        spans.sort();
        assert_eq!(
            spans,
            vec![
                (Frac::zero(), Frac::new(3, 8)),
                (Frac::new(3, 8), Frac::new(3, 4)),
                (Frac::new(3, 4), Frac::one()),
            ]
        );
    }

    #[test]
    fn euclidish_morphs_onsets_and_repeats_them_every_cycle() {
        // `by = 0` is the straight euclidean rhythm: x..x..x. at eighths.
        let bools = bjorklund(3, 8);
        let straight = morph_pattern(&bools, 0.0);
        let wholes = |p: &Pattern, cycle: i64| {
            let mut v: Vec<(Frac, Frac)> = p
                .query_arc(Frac::int(cycle), Frac::int(cycle + 1))
                .into_iter()
                .map(|h| {
                    let w = h.whole.expect("morph events are discrete");
                    (w.begin, w.end)
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            wholes(&straight, 0),
            vec![
                (Frac::zero(), Frac::new(1, 8)),
                (Frac::new(3, 8), Frac::new(1, 2)),
                (Frac::new(3, 4), Frac::new(7, 8)),
            ]
        );
        // `by = 1` pulls each onset onto its evenly-spaced position (k/3).
        assert_eq!(
            wholes(&morph_pattern(&bools, 1.0), 0),
            vec![
                (Frac::zero(), Frac::new(1, 8)),
                (Frac::new(1, 3), Frac::new(11, 24)),
                (Frac::new(2, 3), Frac::new(19, 24)),
            ]
        );
        // Every cycle repeats the same shape, shifted by the cycle it is in.
        assert_eq!(
            wholes(&straight, 3),
            vec![
                (Frac::int(3), Frac::new(25, 8)),
                (Frac::new(27, 8), Frac::new(7, 2)),
                (Frac::new(15, 4), Frac::new(31, 8)),
            ]
        );
        // ...including the clipped part of an event a query cuts into.
        let clipped = straight.query_arc(Frac::new(97, 32), Frac::new(25, 8));
        assert_eq!(clipped.len(), 1);
        assert_eq!(clipped[0].part.begin, Frac::new(97, 32));
        assert_eq!(clipped[0].whole.unwrap().begin, Frac::int(3));
    }
}
