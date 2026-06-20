use crate::fraction::Frac;
use crate::pattern::{Pattern, fastcat, pure};
use crate::signal::rand;
use crate::value::Value;
use std::collections::BTreeMap;

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
    let note = pure(Value::Map(BTreeMap::from([("note".into(), Value::Int(0))])));
    let s = fastcat(&[
        pure(Value::Map(BTreeMap::from([(
            "s".into(),
            Value::Str("a".into()),
        )]))),
        pure(Value::Map(BTreeMap::from([(
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
    let a = pure(Value::Map(BTreeMap::from([(
        "s".into(),
        Value::Str("bd".into()),
    )])));
    let b = pure(Value::Map(BTreeMap::from([
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
