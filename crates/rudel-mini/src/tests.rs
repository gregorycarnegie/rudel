use super::*;
use rudel_core::{Frac, Value};

fn vals(src: &str) -> Vec<Value> {
    let pat = parse(src).expect("parse");
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter().map(|h| h.value).collect()
}

fn begins(src: &str) -> Vec<Frac> {
    let pat = parse(src).expect("parse");
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter().map(|h| h.part.begin).collect()
}

#[test]
fn simple_sequence() {
    assert_eq!(
        vals("bd sd hh"),
        vec![
            Value::Str("bd".into()),
            Value::Str("sd".into()),
            Value::Str("hh".into())
        ]
    );
    assert_eq!(
        begins("bd sd hh"),
        vec![Frac::new(0, 3), Frac::new(1, 3), Frac::new(2, 3)]
    );
}

#[test]
fn numbers_parse_as_numbers() {
    assert_eq!(
        vals("0 1 2"),
        vec![Value::Int(0), Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn js_number_tokens() {
    // Which spellings count as numbers is settled atom by atom in
    // js_number_semantics_decide_which_atoms_are_numbers; what this checks is
    // that the grammar hands the classifier whole tokens — a `.`, `-` or `~`
    // inside a word must not split it.
    assert_eq!(vals("1."), vec![Value::Int(1)]);
    assert_eq!(vals("-3"), vec![Value::Int(-3)]);
    assert_eq!(vals("-x"), vec![Value::Str("-x".into())]);
    assert_eq!(vals("bd.cp"), vec![Value::Str("bd.cp".into())]);
    assert_eq!(vals("a~b"), vec![Value::Str("a~b".into())]);
}

#[test]
fn sub_cycle_groups() {
    // "bd [hh hh]" -> bd at 0..1/2, two hh in the second half
    assert_eq!(
        begins("bd [hh hh]"),
        vec![Frac::new(0, 1), Frac::new(1, 2), Frac::new(3, 4)]
    );
}

#[test]
fn fast_op() {
    // "bd*2" -> two bd
    assert_eq!(begins("bd*2"), vec![Frac::new(0, 1), Frac::new(1, 2)]);
}

#[test]
fn rest_leaves_gap() {
    // "bd ~ sd" -> only bd and sd, at 0 and 2/3
    assert_eq!(
        vals("bd ~ sd"),
        vec![Value::Str("bd".into()), Value::Str("sd".into())]
    );
    assert_eq!(begins("bd ~ sd"), vec![Frac::new(0, 3), Frac::new(2, 3)]);
}

#[test]
fn alternation_one_per_cycle() {
    let pat = parse("<a b c>").unwrap();
    let cyc = |n: i64| {
        pat.query_arc(Frac::int(n), Frac::int(n + 1))[0]
            .value
            .clone()
    };
    assert_eq!(cyc(0), Value::Str("a".into()));
    assert_eq!(cyc(1), Value::Str("b".into()));
    assert_eq!(cyc(2), Value::Str("c".into()));
}

#[test]
fn interval_tokens_stay_strings() {
    // named intervals keep their quality suffix (for transpose), unlike a
    // bare number which still parses as a number.
    assert_eq!(
        vals("3M 5P -2M"),
        vec![
            Value::Str("3M".into()),
            Value::Str("5P".into()),
            Value::Str("-2M".into()),
        ]
    );
    assert_eq!(vals("3"), vec![Value::Int(3)]);
}

#[test]
fn weight_elongates() {
    // "a@3 b" -> a occupies 3/4, b occupies 1/4
    assert_eq!(begins("a@3 b"), vec![Frac::new(0, 1), Frac::new(3, 4)]);
}

#[test]
fn replicate() {
    assert_eq!(
        vals("a!3"),
        vec![
            Value::Str("a".into()),
            Value::Str("a".into()),
            Value::Str("a".into())
        ]
    );
}

#[test]
fn repeated_bare_replicate_accumulates() {
    // "a ! !" == "a!3" (krill folds repeated ! into one op)
    assert_eq!(begins("a ! ! b"), begins("a!3 b"));
}

#[test]
fn euclid_op() {
    // "x(3,8)" -> 3 onsets in 8 steps
    let pat = parse("x(3,8)").unwrap();
    let onsets = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .filter(|h| h.has_onset())
        .count();
    assert_eq!(onsets, 3);
}

#[test]
fn patterned_euclid() {
    // "a(<3 5>,8)" alternates pulse counts per cycle
    let pat = parse("a(<3 5>,8)").unwrap();
    let onsets = |n: i64| {
        pat.query_arc(Frac::int(n), Frac::int(n + 1))
            .into_iter()
            .filter(|h| h.has_onset())
            .count()
    };
    assert_eq!(onsets(0), 3);
    assert_eq!(onsets(1), 5);
}

#[test]
fn stack_with_comma() {
    let n = parse("a, b c")
        .unwrap()
        .query_arc(Frac::zero(), Frac::one())
        .len();
    assert_eq!(n, 3); // a (whole cycle) + b + c
}

#[test]
fn range_expands() {
    // krill needs whitespace (or a bracket) before "..": "0..3" is one token
    assert_eq!(
        vals("0 .. 3"),
        vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    assert_eq!(vals("0..3"), vec![Value::Str("0..3".into())]);
}

#[test]
fn patterned_range() {
    // "<0 1> .. 2" -> 0 1 2 in cycle 0, 1 2 in cycle 1
    let pat = parse("<0 1> .. 2").unwrap();
    let count = |n: i64| pat.query_arc(Frac::int(n), Frac::int(n + 1)).len();
    assert_eq!(count(0), 3);
    assert_eq!(count(1), 2);
}

#[test]
fn tail_makes_list() {
    assert_eq!(
        vals("bd:3"),
        vec![Value::List(vec![Value::Str("bd".into()), Value::Int(3)])]
    );
}

#[test]
fn chord_name_tails_stay_lists() {
    // `c:maj7` / `g:7` keep their chord-symbol tails as list values for
    // `.chord()`/`.voicing()` to read.
    assert_eq!(
        vals("c:maj7"),
        vec![Value::List(vec![
            Value::Str("c".into()),
            Value::Str("maj7".into()),
        ])]
    );
    assert_eq!(
        vals("g:7"),
        vec![Value::List(vec![Value::Str("g".into()), Value::Int(7)])]
    );
}

#[test]
fn non_numeric_tail_preserved() {
    // a non-numeric `:` tail survives as a string element.
    assert_eq!(
        vals("bd:foo"),
        vec![Value::List(vec![
            Value::Str("bd".into()),
            Value::Str("foo".into()),
        ])]
    );
}

#[test]
fn steps_marker_scales_steps() {
    // mini('a [^b c]')._steps == 4 in Strudel
    assert_eq!(parse("a [^b c]").unwrap().steps, Some(Frac::int(4)),);
    assert_eq!(parse("[^b c]!3").unwrap().steps, Some(Frac::int(6)));
    assert_eq!(
        parse("[^a b c] [d [^e f]]").unwrap().steps,
        Some(Frac::int(24)),
    );
}

#[test]
fn leaf_locations_match_strudel() {
    // Strudel's getLeafLocations tests, shifted by -1 (no wrapping quote).
    assert_eq!(leaf_locations("bd sd").unwrap(), vec![(0, 2), (3, 5)]);
    assert_eq!(
        leaf_locations("bd*2 [sd cp]").unwrap(),
        vec![(0, 2), (3, 4), (6, 8), (9, 11)],
    );
    assert_eq!(
        leaf_locations("bd*<2 3>").unwrap(),
        vec![(0, 2), (4, 5), (6, 7)],
    );
}

#[test]
fn haps_carry_source_locations() {
    let pat = parse("bd sd").unwrap();
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    assert_eq!(haps[0].context.locations, vec![(0, 2)]);
    assert_eq!(haps[1].context.locations, vec![(3, 5)]);

    // op arguments keep their location too, even through the pure
    // fast-path ("2" in bd*2)
    let pat = parse("bd*2").unwrap();
    let hap = &pat.query_arc(Frac::zero(), Frac::one())[0];
    let mut locs = hap.context.locations.clone();
    locs.sort_unstable();
    assert_eq!(locs, vec![(0, 2), (3, 4)]);

    // parse_with_offset shifts everything by the embedding position
    let pat = parse_with_offset("bd sd", 10).unwrap();
    let mut haps = pat.query_arc(Frac::zero(), Frac::one());
    haps.sort_by_key(|h| h.part.begin);
    assert_eq!(haps[0].context.locations, vec![(10, 12)]);
    assert_eq!(haps[1].context.locations, vec![(13, 15)]);
}

#[test]
fn install_hook_parses_strings_through_core() {
    // After install(), &str arguments anywhere in rudel-core parse as mini.
    install();
    // note("0 2 4").fast(2) -> 6 events, all {note: ...} maps
    let pat = rudel_core::note("0 2 4").fast(2);
    let haps = pat.query_arc(Frac::zero(), Frac::one());
    assert_eq!(haps.len(), 6);
    assert!(
        haps.iter()
            .all(|h| matches!(&h.value, Value::Map(m) if m.contains_key("note")))
    );

    // s("bd:3") splits into {s, n} via the list produced by the tail op
    let s = rudel_core::s("bd:3");
    match &s.query_arc(Frac::zero(), Frac::one())[0].value {
        Value::Map(m) => {
            assert_eq!(m.get("s"), Some(&Value::Str("bd".into())));
            assert_eq!(m.get("n"), Some(&Value::Int(3)));
        }
        other => panic!("expected map, got {other:?}"),
    }
}

// --- gaps found by cargo-mutants -------------------------------------------

/// JavaScript `Number()` semantics, atom by atom. The parity tests exercise
/// plain integers, which leaves every other branch of the classifier free to
/// be wrong: mutation testing replaced `is_js_decimal` with `true` and nothing
/// noticed.
#[test]
fn js_number_semantics_decide_which_atoms_are_numbers() {
    // Tagged, not compared as `Value`: `Value`'s PartialEq deliberately equates
    // numbers across variants (`Int(3) == F64(3.0)`), so assert_eq! cannot see
    // the Int/F64 split that `num_value` exists to make — and that split is
    // load-bearing downstream, where rudel-osc encodes an Int as `i` and an F64
    // as `f`. Mutation testing is what surfaced the blind spot.
    fn tag(src: &str) -> String {
        match crate::atom::atom_value(src) {
            None => "rest".to_string(),
            Some(Value::Int(n)) => format!("int {n}"),
            Some(Value::F64(x)) => format!("f64 {x}"),
            Some(Value::Str(s)) => format!("str {s}"),
            Some(other) => format!("other {other:?}"),
        }
    }

    assert_eq!(tag("~"), "rest");
    assert_eq!(tag("-"), "rest");

    // Decimals, including every exponent form JS accepts.
    assert_eq!(tag("3"), "int 3");
    assert_eq!(tag("3.5"), "f64 3.5");
    assert_eq!(tag(".5"), "f64 0.5");
    assert_eq!(tag("-2.5"), "f64 -2.5");
    assert_eq!(tag("1e3"), "int 1000");
    assert_eq!(tag("1E3"), "int 1000");
    assert_eq!(tag("1e+3"), "int 1000");
    assert_eq!(tag("1e-3"), "f64 0.001");

    // ...and the near-misses it rejects, which stay strings.
    assert_eq!(tag("1e"), "str 1e");
    assert_eq!(tag("."), "str .");
    assert_eq!(tag("1.2.3"), "str 1.2.3");
    assert_eq!(tag("bd"), "str bd");

    // Rust's f64 parser accepts these spellings and JS's `Number()` does not,
    // which is the whole reason the literal is validated before parsing.
    assert_eq!(tag("inf"), "str inf");
    assert_eq!(tag("nan"), "str nan");
    assert_eq!(tag("NaN"), "str NaN");
    assert_eq!(tag("Infinity"), "f64 inf");
    assert_eq!(tag("-Infinity"), "f64 -inf");

    // Radix literals in both letter cases; JS rejects a signed one.
    assert_eq!(tag("0x1f"), "int 31");
    assert_eq!(tag("0X1f"), "int 31");
    assert_eq!(tag("0b101"), "int 5");
    assert_eq!(tag("0B101"), "int 5");
    assert_eq!(tag("0o17"), "int 15");
    assert_eq!(tag("0O17"), "int 15");
    assert_eq!(tag("-0x10"), "str -0x10");

    // A whole float collapses to `Int` only below 2^53, where i64 is exact.
    assert_eq!(tag("9007199254740991"), "int 9007199254740991");
    assert_eq!(tag("9007199254740992"), "f64 9007199254740992");
}

/// Ops inside a euclid argument are discarded — `mini.mjs` enters the slice
/// and throws the ops away — but the parser still walks them for their random
/// seeds. Skip that bookkeeping and every later `?` in the pattern silently
/// draws a different seed.
#[test]
fn discarded_ops_inside_euclid_args_still_consume_seeds() {
    fn rows(src: &str) -> Vec<String> {
        let pat = parse(src).expect("parse");
        let mut rows: Vec<String> = pat
            .query_arc(Frac::zero(), Frac::int(8))
            .iter()
            .map(|h| format!("{:?}|{:?}", h.part.begin, h.value))
            .collect();
        rows.sort();
        rows
    }

    // Each pair plays the same notes; only the seed the trailing `?` draws
    // differs, because the op inside the euclid argument consumed one first.
    for (with_op, without) in [
        ("bd(3?,8) sd?", "bd(3,8) sd?"),
        ("bd(3*[2?],8) sd?", "bd(3*[2],8) sd?"),
        ("bd(3(2?,4),8) sd?", "bd(3(2,4),8) sd?"),
    ] {
        assert_ne!(
            rows(with_op),
            rows(without),
            "{with_op} and {without} must not draw the same seeds"
        );
    }
}
