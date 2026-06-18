use super::*;
use crate::fraction::Frac;
use crate::pattern::{Pattern, pure, slowcat, stack};
use crate::seq;
use crate::signal::rand;
use crate::value::Value;

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
    assert_eq!(
        values(&pat.take(2), 0, 1),
        vec![Value::Int(0), Value::Int(1)]
    );
    assert_eq!(
        values(&pat.take(-2), 0, 1),
        vec![Value::Int(2), Value::Int(3)]
    );
    // taking >= all steps returns the pattern; a stepless pattern -> silence
    assert_eq!(values(&pat.take(9), 0, 1).len(), 4);
    assert!(
        rand()
            .take(2)
            .query_arc(Frac::zero(), Frac::one())
            .is_empty()
    );
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
    assert!(
        got.iter()
            .all(|v| *v == Value::Int(5) || *v == Value::Int(9))
    );
}

#[test]
fn ribbon_loops_a_window() {
    // slowcat 0 1 2 3 (one per cycle); ribbon(1, 2) loops the window [1,3)
    let src = slowcat(&[
        pure(Value::Int(0)),
        pure(Value::Int(1)),
        pure(Value::Int(2)),
        pure(Value::Int(3)),
    ]);
    let pat = src.ribbon(Frac::int(1), Frac::int(2));
    assert_eq!(values(&pat, 0, 1), vec![Value::Int(1)]);
    assert_eq!(values(&pat, 1, 2), vec![Value::Int(2)]);
    // loops every 2 cycles
    assert_eq!(values(&pat, 2, 3), vec![Value::Int(1)]);
    assert_eq!(values(&pat, 3, 4), vec![Value::Int(2)]);
}

#[test]
fn collect_groups_simultaneous_haps() {
    // three stacked values collapse into one hap holding a list
    let pat = stack(&[
        pure(Value::Int(0)),
        pure(Value::Int(1)),
        pure(Value::Int(2)),
    ])
    .collect();
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 1);
    assert_eq!(
        haps[0].value,
        Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)])
    );
}

#[test]
fn beat_places_value_in_its_division() {
    // beat(2, 4): the value is compressed into the [2/4, 3/4] slice.
    let pat = pure(Value::Int(1)).beat(2, 4);
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 1);
    let whole = haps[0].whole.unwrap();
    assert_eq!(whole.begin, Frac::new(1, 2));
    assert_eq!(whole.end, Frac::new(3, 4));
}

#[test]
fn morph_interpolates_onset_between_rhythms() {
    // from has its single onset at position 0, to at position 2/4. The
    // morphed onset slides linearly between them as `by` goes 0 -> 1.
    let from = Value::List(vec![1, 0, 0, 0].into_iter().map(Value::Int).collect());
    let to = Value::List(vec![0, 0, 1, 0].into_iter().map(Value::Int).collect());
    let onset = |by: Frac| {
        let p = morph(from.clone(), to.clone(), Value::Frac(by));
        let haps = p.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 1, "expected one onset");
        assert_eq!(haps[0].value, Value::Bool(true));
        haps[0].whole.unwrap().begin
    };
    assert_eq!(onset(Frac::zero()), Frac::zero()); // fully `from`
    assert_eq!(onset(Frac::one()), Frac::new(1, 2)); // fully `to`
    assert_eq!(onset(Frac::new(1, 2)), Frac::new(1, 4)); // halfway
}

#[test]
fn xfade_sets_complementary_gains() {
    // pos=0: left full (gain 1), right silent (gain 0).
    let pat = crate::s(Value::Str("a".into())).xfade(0, crate::s(Value::Str("b".into())));
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    let gain_of = |name: &str| {
        haps.iter().find_map(|h| match &h.value {
            Value::Map(m) if m.get("s") == Some(&Value::Str(name.into())) => {
                m.get("gain").and_then(Value::as_f64)
            }
            _ => None,
        })
    };
    assert_eq!(gain_of("a"), Some(1.0));
    assert_eq!(gain_of("b"), Some(0.0));
}

#[test]
fn arp_selects_chord_notes_by_index() {
    let chord = stack(&[
        pure(Value::Int(0)),
        pure(Value::Int(1)),
        pure(Value::Int(2)),
    ]);
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
    let chord = stack(&[
        pure(Value::Int(5)),
        pure(Value::Int(7)),
        pure(Value::Int(9)),
    ]);
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
