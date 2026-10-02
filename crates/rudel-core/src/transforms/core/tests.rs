use crate::{
    fraction::Frac,
    pattern::{Pattern, fastcat, pure},
    signal::rand,
    value::{Value, ValueMap},
};

fn vals(pat: &Pattern) -> Vec<Value> {
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter().map(|h| h.value).collect()
}

fn seq(items: &[i64]) -> Pattern {
    fastcat(
        &items
            .iter()
            .map(|&n| pure(Value::Int(n)))
            .collect::<Vec<_>>(),
    )
}

fn onsets(pat: &Pattern) -> usize {
    pat.query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count()
}

#[test]
fn add_in_takes_left_structure() {
    // "0 1".add("10 20 30") -> 2 onsets (structure from left)
    assert_eq!(onsets(&seq(&[0, 1]).add(seq(&[10, 20, 30]))), 2);
}

#[test]
fn add_out_takes_right_structure() {
    // "0 1".add.out("10 20 30") -> 3 onsets (structure from right)
    assert_eq!(onsets(&seq(&[0, 1]).add_out(seq(&[10, 20, 30]))), 3);
}

#[test]
fn add_squeeze_fits_other_per_event() {
    // each of the 2 events gets a full cycle of "10 20" squeezed in -> 4 haps
    let pat = seq(&[0, 1]).add_squeeze(seq(&[10, 20]));
    assert_eq!(
        vals(&pat),
        vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(11),
            Value::Int(21)
        ]
    );
}

#[test]
fn set_squeeze_merges_maps() {
    // {note:0} set.squeeze {s:a}{s:b} -> per note event, two {note,s} haps
    let note = pure(Value::Map(ValueMap::from([("note".into(), Value::Int(0))])));
    let s = fastcat(&[
        pure(Value::Map(ValueMap::from([(
            "s".into(),
            Value::Str("a".into()),
        )]))),
        pure(Value::Map(ValueMap::from([(
            "s".into(),
            Value::Str("b".into()),
        )]))),
    ]);
    let pat = note.set_squeeze(s);
    let got = vals(&pat);
    assert_eq!(got.len(), 2);
    match &got[0] {
        Value::Map(m) => {
            assert_eq!(m.get("note"), Some(&Value::Int(0)));
            assert_eq!(m.get("s"), Some(&Value::Str("a".into())));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn expand_scales_step_count_only() {
    // "0 1" has 2 steps; expand(3) -> 6 steps, same timing (2 onsets/cycle)
    let pat = seq(&[0, 1]).expand(3);
    assert_eq!(pat.steps, Some(Frac::int(6)));
    assert_eq!(onsets(&pat), 2);
}

#[test]
fn extend_is_fast_plus_expand() {
    // extend(2) of "0 1" -> fast(2) (4 onsets/cycle) and steps 2*2 = 4
    let pat = seq(&[0, 1]).extend(2);
    assert_eq!(pat.steps, Some(Frac::int(4)));
    assert_eq!(onsets(&pat), 4);
}

#[test]
fn contract_divides_step_count_only() {
    // "0 1 2 3" has 4 steps; contract(2) -> 2 steps, same timing (4 onsets).
    let pat = seq(&[0, 1, 2, 3]).contract(2);
    assert_eq!(pat.steps, Some(Frac::int(2)));
    assert_eq!(onsets(&pat), 4);
}

#[test]
fn shrink_progressively_drops_steps() {
    // "0 1 2 3".shrink(1) == "0 1 2 3 1 2 3 2 3 3" (10 steps).
    let pat = seq(&[0, 1, 2, 3]).shrink(1);
    assert_eq!(pat.steps, Some(Frac::int(10)));
    assert_eq!(
        vals(&pat),
        [0, 1, 2, 3, 1, 2, 3, 2, 3, 3]
            .into_iter()
            .map(Value::Int)
            .collect::<Vec<_>>()
    );
}

#[test]
fn grow_progressively_reveals_steps() {
    // "0 1 2 3".grow(1) == "0 0 1 0 1 2 0 1 2 3" (10 steps).
    let pat = seq(&[0, 1, 2, 3]).grow(1);
    assert_eq!(pat.steps, Some(Frac::int(10)));
    assert_eq!(
        vals(&pat),
        [0, 0, 1, 0, 1, 2, 0, 1, 2, 3]
            .into_iter()
            .map(Value::Int)
            .collect::<Vec<_>>()
    );
}

#[test]
fn shrink_grow_need_step_metadata() {
    // a continuous signal has no step count -> silence.
    assert!(
        rand()
            .shrink(1)
            .query_arc(Frac::zero(), Frac::one())
            .is_empty()
    );
}

#[test]
fn add_poly_aligns_step_counts() {
    // "0 1 2" (3 steps) add.poly "10 20" (2 steps): outer 3 steps drive it,
    // the other is extended to 3 steps -> 3 onsets, first value 0+10.
    let pat = seq(&[0, 1, 2]).add_poly(seq(&[10, 20]));
    assert_eq!(onsets(&pat), 3);
    assert_eq!(vals(&pat)[0], Value::Int(10));
}

#[test]
fn keep_prefers_left_value() {
    // {s:bd} keep {s:sd, n:1} -> keeps s:bd, gains n:1
    let a = pure(Value::Map(ValueMap::from([(
        "s".into(),
        Value::Str("bd".into()),
    )])));
    let b = pure(Value::Map(ValueMap::from([
        ("s".into(), Value::Str("sd".into())),
        ("n".into(), Value::Int(1)),
    ])));
    match &vals(&a.keep(b))[0] {
        Value::Map(m) => {
            assert_eq!(m.get("s"), Some(&Value::Str("bd".into())));
            assert_eq!(m.get("n"), Some(&Value::Int(1)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

#[test]
fn every_cycles_puts_the_transform_on_the_first_or_last_of_each_group() {
    let plain = pure(Value::Int(0));
    let changed = pure(Value::Int(1));
    let at = |pat: &Pattern, cycle: i64| {
        pat.query_arc(Frac::int(cycle), Frac::int(cycle + 1))[0]
            .value
            .clone()
    };
    let first = plain.every_cycles(&changed, 3, false);
    assert_eq!(
        [0, 1, 2, 3].map(|c| at(&first, c)),
        [1, 0, 0, 1].map(Value::Int)
    );
    let last = plain.every_cycles(&changed, 3, true);
    assert_eq!(
        [0, 1, 2, 3].map(|c| at(&last, c)),
        [0, 0, 1, 0].map(Value::Int)
    );
}

#[test]
fn degrading_by_zero_still_drops_where_the_random_value_is_exactly_zero() {
    // `rand` is exactly 0 at time 0, and an event is kept only above `x`.
    assert!(vals(&pure(Value::Int(1)).degrade_by_with(rand(), 0.0)).is_empty());
}

#[test]
fn range_scales_into_a_range_that_does_not_start_at_zero() {
    assert_eq!(
        vals(&pure(Value::F64(0.5)).range(2.0, 10.0)),
        [Value::F64(6.0)]
    );
}

fn map_of(pairs: &[(&str, Value)]) -> Value {
    Value::Map(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

#[test]
fn collect_groups_by_whole_and_part_together() {
    // One whole, two fragments: they are different haps and stay apart.
    use crate::{hap::Hap, timespan::TimeSpan};
    let whole = TimeSpan::new(Frac::zero(), Frac::one());
    let pat = Pattern::new(move |_| {
        vec![
            Hap::new(
                Some(whole),
                TimeSpan::new(Frac::zero(), Frac::new(1, 2)),
                Value::Int(1),
            ),
            Hap::new(
                Some(whole),
                TimeSpan::new(Frac::new(1, 2), Frac::one()),
                Value::Int(2),
            ),
        ]
    });
    assert_eq!(pat.collect().query_arc(Frac::zero(), Frac::one()).len(), 2);
}

#[test]
fn zip_ignores_a_pattern_of_no_steps() {
    let two = fastcat(&[pure(Value::Int(1)), pure(Value::Int(2))]);
    let none = pure(Value::Int(9)).set_steps(Some(Frac::zero()));
    assert_eq!(
        vals(&crate::zip(&[two.clone(), none])),
        vals(&crate::zip(&[two]))
    );
}

#[test]
fn beat_wraps_a_position_past_the_division() {
    // Beat 5 of 4 is beat 1: the second quarter.
    let haps = pure(Value::Int(1))
        .beat(pure(Value::Int(5)), pure(Value::Int(4)))
        .query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 1);
    assert_eq!(haps[0].part.begin, Frac::new(1, 4));
}

#[test]
fn xfade_at_a_quarter_keeps_a_full_and_halves_b() {
    let a = pure(map_of(&[
        ("s", Value::Str("a".into())),
        ("gain", Value::F64(1.0)),
    ]));
    let b = pure(map_of(&[
        ("s", Value::Str("b".into())),
        ("gain", Value::F64(1.0)),
    ]));
    let gains: Vec<_> = vals(&crate::xfade(a, pure(Value::F64(0.25)), b))
        .into_iter()
        .filter_map(|v| match v {
            Value::Map(m) => Some((m.get("s").cloned(), m.get("gain").and_then(Value::as_f64))),
            _ => None,
        })
        .collect();
    assert!(
        gains.contains(&(Some(Value::Str("a".into())), Some(1.0))),
        "{gains:?}"
    );
    assert!(
        gains.contains(&(Some(Value::Str("b".into())), Some(0.5))),
        "{gains:?}"
    );
}

#[test]
fn pick_map_reads_a_numeric_selector_as_its_key() {
    let items = std::collections::HashMap::from([
        ("0".to_string(), pure(Value::Str("zero".into()))),
        ("1".to_string(), pure(Value::Str("one".into()))),
    ]);
    for selector in [Value::Int(1), Value::F64(1.0)] {
        let picked = crate::pick_map(&items, &pure(selector.clone()), crate::PickJoin::Inner);
        assert_eq!(vals(&picked), [Value::Str("one".into())], "{selector:?}");
    }
}

#[test]
fn two_fractions_are_equal_only_exactly() {
    // The f64 nearest 1/3 is a different fraction, though it reads the same.
    let third = Value::Frac(Frac::new(1, 3));
    let near = Value::Frac(Frac(num_rational::Ratio::new(
        6004799503160661,
        18014398509481984,
    )));
    assert_ne!(third, near);
    assert_eq!(third, Value::Frac(Frac::new(2, 6)));
}

#[test]
fn every_built_in_voicing_dictionary_is_listed_beside_a_registered_one() {
    crate::voicing::add_voicings(
        "survivor-test-dict",
        [("M".to_string(), vec!["0 4 7".to_string()])],
    );
    let names: Vec<String> = crate::voicing::voicing_dictionaries()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for name in ["ireal", "lefthand", "survivor-test-dict"] {
        assert_eq!(
            names.iter().filter(|n| *n == name).count(),
            1,
            "{name}: {names:?}"
        );
    }
}

#[test]
fn randrun_shuffles_each_cycle_from_its_own_seed() {
    // Pinned: the shuffle is seeded by the cycle number plus a half, so the
    // first cycle is not simply 0, 1, 2, 3.
    let order = |c: i64| -> Vec<Value> {
        let mut haps = crate::signal::randrun(4).query_arc(Frac::int(c), Frac::int(c + 1));
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter().map(|h| h.value).collect()
    };
    let ints = |xs: [i64; 4]| xs.map(Value::Int).to_vec();
    assert_eq!(order(0), ints([2, 1, 3, 0]));
    assert_eq!(order(1), ints([2, 0, 3, 1]));
    assert_eq!(order(2), ints([1, 0, 2, 3]));
    // `randSeed` adds to the cycle, so a seed of 1 is the next cycle's order.
    let state = crate::state::State::with_controls(
        crate::timespan::TimeSpan::new(Frac::zero(), Frac::one()),
        ValueMap::from([("randSeed".to_string(), Value::F64(1.0))]),
    );
    let mut seeded = crate::signal::randrun(4).query(&state);
    seeded.sort_by_key(|h| h.part.begin);
    let seeded: Vec<Value> = seeded.into_iter().map(|h| h.value).collect();
    assert_eq!(seeded, order(1));
}
