// rudel-core - the pattern engine for Rudel, a Rust fork of Strudel.
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Strudel (https://codeberg.org/uzu/strudel) is the JS port of TidalCycles.
// A `Pattern` is a pure function `State -> Vec<Hap>`; everything is built from
// the functor / applicative / monad combinators in `pattern`.

pub mod color;
pub mod controls;
pub mod draw;
pub mod edo;
pub mod euclid;
pub mod fraction;
pub mod hap;
pub mod host;
pub mod impure;
pub mod input;
pub mod midimap;
pub mod modulate;
pub mod pattern;
pub mod query;
pub mod samples;
pub mod signal;
pub mod state;
pub mod timespan;
pub mod tonal;
pub mod transforms;
mod tune_table;
pub mod value;
pub mod voicing;
pub mod xen;

pub use fraction::Frac;
pub use hap::{Context, Hap};
pub use impure::reset_timelines;
pub use modulate::modulate;
pub use pattern::{
    Pattern, arrange, cat, fastcat, gap, nothing, parray, parse_string, polymeter, pure, reify,
    sequence, set_string_parser, silence, slowcat, slowcat_prime, stack, stack_centre, stack_left,
    stack_right, stepcat, timecat, value_to_pattern,
};
pub use state::State;
pub use timespan::TimeSpan;
pub use transforms::IntoPattern;
pub use value::{Value, ValueMap};

// Signals and randomness.
pub use signal::{
    berlin, binary, binary_l, binary_n, binary_nl, brand, brand_by, cosine, cosine2, cycles_per,
    irand, isaw, isaw2, itri, itri2, per, perlin, perx, rand, rand_l, rand2, randrun, run, saw,
    saw2, scan, sine, sine2, square, square2, steady, time, tri, tri2,
};
// Euclidean rhythms.
pub use euclid::{bjorklund, euclid_bools};
// Cycle-random combinators.
pub use transforms::{
    choose, choose_cycles, choose_in, choose_in_with, choose_with, morph, randcat, ratio_value,
    stepalt, wchoose, wrandcat, xfade, zip,
};
// Pick combinators (select patterns from a list/table via a selector pattern).
pub use transforms::{PickJoin, pick_list, pick_map};
// Controls (also available as chaining methods on `Pattern`).
pub use controls::{
    bend_range, control_builders, control_dyn, control_name, freq, i, lpf, lpq, mpe, n, note,
    numbered_control_names, s, sound,
};
// MIDI input bus (written by `rudel-midi`, read via the `cc_in` signal).
pub use input::{
    cc_in, cc_in_from, clear_cc, clear_keys, clear_midi_notes, get_cc, get_cc_from, get_pointer,
    key_down, keys_down, midi_keys, mousex, mousey, push_midi_note, set_cc, set_cc_from,
    set_keys_held, set_pointer, take_midi_notes,
};
// MIDI output CC maps (written by the language layer, read by `rudel-midi`).
pub use midimap::{CcMapping, has_midimap, midimap_ccs, set_midimap};
// Host-published tables read back by scripts (sample durations, the log ring).
pub use host::{
    GAIN_CURVE_MAX, GAIN_CURVE_POINTS, apply_gain_curve, clear_gain_curve, clear_sample_durations,
    drain_log, log_line, sample_duration, set_gain_curve, set_gain_curve_samples,
    set_sample_duration,
};
// Tonal: note names, scales, chords.
pub use tonal::{
    chord_notes, chord_symbols, note_to_midi, note_to_midi_with_octave, scale_names, scale_offset,
    scale_step, value_to_midi,
};
// Xenharmonic helpers.
pub use xen::{edo_ratios, freq_to_midi, get_freq, midi_to_freq};
// CSS named colors + color/hex -> number conversion (draw/color.mjs).
pub use color::{convert_color_to_number, convert_hex_to_number, css_color_hex};
// Scheduler-agnostic event extraction (shared by audio / MIDI / OSC).
pub use query::{ControlEvent, LOG_KEY, TRIGGER_KEY, query_controls, to_control_map};

/// Convenience: build a `pure` pattern from anything convertible to a [`Value`].
pub fn p(v: impl Into<Value>) -> Pattern {
    pure(v.into())
}

/// Convenience: build a fastcat sequence from a list of values.
pub fn seq<I, T>(items: I) -> Pattern
where
    I: IntoIterator<Item = T>,
    T: Into<Value>,
{
    let pats: Vec<Pattern> = items.into_iter().map(|v| pure(v.into())).collect();
    fastcat(&pats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect (part_begin, part_end, value) triples for a queried arc, for
    /// compact snapshot-style assertions.
    fn arc(pat: &Pattern, b: i64, e: i64) -> Vec<(Frac, Frac, Value)> {
        pat.query_arc(Frac::int(b), Frac::int(e))
            .into_iter()
            .map(|h| (h.part.begin, h.part.end, h.value))
            .collect()
    }

    #[test]
    fn pure_repeats_once_per_cycle() {
        let pat = p(3);
        let haps = arc(&pat, 0, 2);
        assert_eq!(
            haps,
            vec![
                (Frac::int(0), Frac::int(1), Value::Int(3)),
                (Frac::int(1), Frac::int(2), Value::Int(3)),
            ]
        );
        // pure has a whole spanning the cycle
        let first = &pat.query_arc(Frac::zero(), Frac::one())[0];
        assert_eq!(first.whole, Some(TimeSpan::new(Frac::zero(), Frac::one())));
        assert!(first.has_onset());
    }

    #[test]
    fn fastcat_divides_the_cycle() {
        let pat = seq([0, 1, 2]);
        let haps = arc(&pat, 0, 1);
        assert_eq!(
            haps,
            vec![
                (Frac::new(0, 1), Frac::new(1, 3), Value::Int(0)),
                (Frac::new(1, 3), Frac::new(2, 3), Value::Int(1)),
                (Frac::new(2, 3), Frac::new(1, 1), Value::Int(2)),
            ]
        );
        assert_eq!(pat.steps, Some(Frac::int(3)));
    }

    #[test]
    fn slowcat_one_per_cycle() {
        let pat = cat(&[p(0), p(1), p(2)]);
        assert_eq!(
            arc(&pat, 0, 1),
            vec![(Frac::int(0), Frac::int(1), Value::Int(0))]
        );
        assert_eq!(
            arc(&pat, 1, 2),
            vec![(Frac::int(1), Frac::int(2), Value::Int(1))]
        );
        assert_eq!(
            arc(&pat, 3, 4),
            vec![(Frac::int(3), Frac::int(4), Value::Int(0))]
        );
    }

    #[test]
    fn fast_speeds_up() {
        let pat = p(1).fast(Frac::int(2));
        let haps = arc(&pat, 0, 1);
        assert_eq!(
            haps,
            vec![
                (Frac::new(0, 1), Frac::new(1, 2), Value::Int(1)),
                (Frac::new(1, 2), Frac::new(1, 1), Value::Int(1)),
            ]
        );
    }

    /// Values in part-begin order (haps aren't guaranteed sorted; Strudel's
    /// tests sort too).
    fn sorted_values(pat: &Pattern) -> Vec<Value> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|a| a.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    }

    #[test]
    fn rev_reverses_within_cycle() {
        let pat = seq([0, 1, 2]).rev();
        assert_eq!(
            sorted_values(&pat),
            vec![Value::Int(2), Value::Int(1), Value::Int(0)]
        );
    }

    #[test]
    fn stack_overlays() {
        let pat = stack(&[p(0), p(1)]);
        let haps = arc(&pat, 0, 1);
        assert_eq!(haps.len(), 2);
        assert_eq!(haps[0].2, Value::Int(0));
        assert_eq!(haps[1].2, Value::Int(1));
    }

    #[test]
    fn ply_repeats_each_event() {
        // "0 1".ply(2) => 0 0 1 1
        let pat = seq([0, 1]).ply(Frac::int(2));
        let values: Vec<Value> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(
            values,
            vec![Value::Int(0), Value::Int(0), Value::Int(1), Value::Int(1)]
        );
    }

    #[test]
    fn struct_keeps_values_on_bool_onsets() {
        // "a".struct("x ~ x") => a at step 0 and step 2
        let pat = p("a").struct_pat(seq([true, false, true]));
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        let parts: Vec<(Frac, Value)> = haps
            .iter()
            .map(|h| (h.part.begin, h.value.clone()))
            .collect();
        assert_eq!(
            parts,
            vec![
                (Frac::new(0, 3), Value::Str("a".into())),
                (Frac::new(2, 3), Value::Str("a".into())),
            ]
        );
    }

    #[test]
    fn mask_silences_false_regions() {
        // "0 1 2 3".mask("1 0") keeps the first half only
        let pat = seq([0, 1, 2, 3]).mask(seq([true, false]));
        assert_eq!(sorted_values(&pat), vec![Value::Int(0), Value::Int(1)]);
    }

    #[test]
    fn add_lifts_constant() {
        let pat = seq([0, 1, 2]).add(10);
        assert_eq!(
            sorted_values(&pat),
            vec![Value::Int(10), Value::Int(11), Value::Int(12)]
        );
    }

    #[test]
    fn segment_discretizes_a_signal() {
        let pat = saw().segment(4);
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 4);
        // saw sampled at left edge of each of the 4 segments: 0, 1/4, 1/2, 3/4
        let vals: Vec<f64> = haps.iter().map(|h| h.value.as_f64().unwrap()).collect();
        assert_eq!(vals, vec![0.0, 0.25, 0.5, 0.75]);
    }

    #[test]
    fn euclid_3_8_has_three_onsets() {
        let pat = p("x").euclid(3, 8);
        let onsets = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter(|h| h.has_onset())
            .count();
        assert_eq!(onsets, 3);
    }

    #[test]
    fn every_applies_on_first_of_n() {
        // every(2, +10): cycle 0 -> 10, cycle 1 -> 0
        let pat = p(0).every(2, |p| p.add(10));
        assert_eq!(
            pat.query_arc(Frac::zero(), Frac::one())[0].value,
            Value::Int(10)
        );
        assert_eq!(
            pat.query_arc(Frac::one(), Frac::int(2))[0].value,
            Value::Int(0)
        );
    }

    #[test]
    fn fast_patternified_pure_arg() {
        // .fast(2) where 2 is lifted from i64 takes the pure fast-path
        let pat = p(1).fast(2);
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 2);
    }

    #[test]
    fn add_via_applicative() {
        // pattern of functions (+10) applied to "0 1 2"
        let nums = seq([0, 1, 2]);
        let adder = pure(Value::func(|v| Value::Int(v.as_f64().unwrap() as i64 + 10)));
        let result = adder.app_left(&nums);
        let values: Vec<Value> = result
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(values, vec![Value::Int(10), Value::Int(11), Value::Int(12)]);
    }

    #[test]
    fn stepcat_concatenates_by_steps() {
        // stepcat("0 1 2", "3 4") == "0 1 2 3 4": a 5-step weighted cat.
        let a = seq([0, 1, 2]);
        let b = seq([3, 4]);
        let pat = stepcat(&[a, b]);
        assert_eq!(pat.steps, Some(Frac::int(5)));
        let values: Vec<Value> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(
            values,
            vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]
        );
    }

    #[test]
    fn arrange_lays_sections_over_cycles() {
        // arrange([2, "0"], [1, "1"]): "0" for two cycles, "1" for one, total 3.
        let pat = arrange(&[(Frac::int(2), p(0)), (Frac::int(1), p(1))]);
        assert_eq!(arc(&pat, 0, 1)[0].2, Value::Int(0));
        assert_eq!(arc(&pat, 1, 2)[0].2, Value::Int(0));
        assert_eq!(arc(&pat, 2, 3)[0].2, Value::Int(1));
        // and it loops every 3 cycles
        assert_eq!(arc(&pat, 3, 4)[0].2, Value::Int(0));
    }

    #[test]
    fn polymeter_aligns_to_lcm_steps() {
        // polymeter("0 1 2", "a b"): steps lcm(3,2) = 6.
        let a = seq([0, 1, 2]);
        let b = fastcat(&[pure(Value::Str("a".into())), pure(Value::Str("b".into()))]);
        let pat = polymeter(&[a, b]);
        assert_eq!(pat.steps, Some(Frac::int(6)));
        // 6 steps from each of the two stacked patterns = 12 haps per cycle.
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 12);
    }

    #[test]
    fn overlay_stacks_two_patterns() {
        let pat = p(0).overlay(p(7));
        let values: Vec<Value> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert!(values.contains(&Value::Int(0)) && values.contains(&Value::Int(7)));
    }

    #[test]
    fn pace_sets_step_count() {
        // "0 1 2" (3 steps) paced to 4 steps -> 4 events, steps = 4.
        let pat = seq([0, 1, 2]).pace(Frac::int(4));
        assert_eq!(pat.steps, Some(Frac::int(4)));
        assert_eq!(pat.query_arc(Frac::zero(), Frac::one()).len(), 4);
    }
}

#[cfg(test)]
mod step_alignment_tests {
    use super::*;
    use crate::pattern::{stack_centre, stack_left};

    fn onsets(pat: &Pattern) -> Vec<(Frac, Frac, Value)> {
        let mut haps: Vec<_> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| (h.part.begin, h.part.end, h.value))
            .collect();
        haps.sort_by_key(|(b, e, _)| (*b, *e));
        haps
    }

    /// `compress` needs `0 <= b <= e <= 1`; anything else would divide by a
    /// zero or negative span, so it yields silence instead.
    #[test]
    fn compress_rejects_spans_outside_the_cycle() {
        let f = Frac::new;
        for (b, e) in [
            (f(3, 2), f(2, 1)),  // b past the cycle
            (f(1, 5), f(3, 2)),  // e past the cycle
            (f(-1, 5), f(1, 2)), // b before it
            (f(3, 5), f(2, 5)),  // reversed
        ] {
            assert!(
                onsets(&seq([1, 2])._compress(b, e)).is_empty(),
                "compress({b}, {e}) should be silence"
            );
        }
        // A span inside the cycle still squeezes both events into it.
        assert_eq!(onsets(&seq([1, 2])._compress(f(1, 4), f(3, 4))).len(), 2);
    }

    /// `ply` multiplies the step count, since each step becomes `factor` steps.
    #[test]
    fn ply_multiplies_the_step_count() {
        assert_eq!(seq([1, 2])._ply(Frac::int(3)).steps, Some(Frac::int(6)));
    }

    /// `stackLeft`/`stackCentre` pad the *shorter* patterns up to the longest
    /// step count — taking the minimum instead would truncate the longer one,
    /// and skipping the padding would stretch the shorter one over the cycle.
    #[test]
    fn stacking_pads_short_patterns_to_the_longest_step_count() {
        let short = seq([1]);
        let long = seq([2, 3, 4]);
        let f = Frac::new;

        let left = stack_left(&[short.clone(), long.clone()]);
        assert_eq!(left.steps, Some(Frac::int(3)));
        assert_eq!(
            onsets(&left),
            vec![
                (f(0, 1), f(1, 3), Value::Int(1)),
                (f(0, 1), f(1, 3), Value::Int(2)),
                (f(1, 3), f(2, 3), Value::Int(3)),
                (f(2, 3), f(1, 1), Value::Int(4)),
            ],
            "the 1-step pattern keeps one step and gets trailing gaps"
        );

        // Centred, the two gap steps are split evenly either side, so the lone
        // event lands in the middle step rather than at an edge.
        let centre = stack_centre(&[short, long]);
        assert_eq!(centre.steps, Some(Frac::int(3)));
        assert_eq!(
            onsets(&centre)
                .into_iter()
                .filter(|(_, _, v)| *v == Value::Int(1))
                .collect::<Vec<_>>(),
            vec![(f(1, 3), f(2, 3), Value::Int(1))]
        );
    }

    /// `parray` curries one packer per input, so the count it is built with has
    /// to shrink by exactly one per applied pattern — off by one and the list
    /// closes early (a `Func` leaks into the output) or never closes at all.
    #[test]
    fn parray_packs_one_value_per_pattern() {
        use crate::pattern::parray;
        let pat = parray(&[
            pure(Value::Int(1)),
            pure(Value::Int(2)),
            pure(Value::Int(3)),
        ]);
        assert_eq!(
            onsets(&pat),
            vec![(
                Frac::int(0),
                Frac::int(1),
                Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
            )]
        );
        // One input still yields a one-element list, not the bare value.
        assert_eq!(
            onsets(&parray(&[pure(Value::Int(1))])),
            vec![(Frac::int(0), Frac::int(1), Value::List(vec![Value::Int(1)]))]
        );
        // No inputs: a constant empty list, not silence.
        assert_eq!(
            onsets(&parray(&[])),
            vec![(Frac::int(0), Frac::int(1), Value::List(Vec::new()))]
        );
    }

    /// `tag` marks a pattern for an editor widget; re-tagging must not stack
    /// duplicates, since the widget list is keyed by it.
    #[test]
    fn tag_does_not_repeat_itself() {
        let tagged = pure(Value::Int(1)).tag("scope").tag("scope").tag("spiral");
        let haps = tagged.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps[0].context.tags, vec!["scope", "spiral"]);
    }

    /// `pace` rescales to a step count; a zero-step pattern would divide by
    /// zero, so it becomes silence instead.
    #[test]
    fn pace_rescales_steps_and_guards_the_zero_case() {
        assert_eq!(seq([1, 2]).pace(Frac::int(4)).steps, Some(Frac::int(4)));
        // No step count at all: unchanged.
        let stepless = seq([1, 2]).set_steps(None);
        assert_eq!(stepless.pace(Frac::int(4)).steps, None);
        assert_eq!(onsets(&stepless.pace(Frac::int(4))), onsets(&stepless));
        // Zero steps: silence rather than a division by zero.
        let zero = seq([1, 2]).set_steps(Some(Frac::zero()));
        assert!(onsets(&zero.pace(Frac::int(4))).is_empty());
    }
}
