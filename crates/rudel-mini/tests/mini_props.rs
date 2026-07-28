// mini_props.rs - property tests for the mini-notation parser and the patterns
// it builds. The parity tests next door pin *specific* patterns against golden
// output from Strudel's engine; these check the invariants that must hold for
// every pattern, over inputs nobody thought to write down.
// SPDX-License-Identifier: AGPL-3.0-or-later

use proptest::prelude::*;
use rudel_core::{Frac, Pattern};

/// Parse generated source. A generated string that will not parse is a failure
/// worth reporting, not a case to skip: either the generator below or the
/// grammar is wrong about what mini-notation accepts.
fn parsed(src: &str) -> Result<Pattern, TestCaseError> {
    rudel_mini::parse(src).map_err(TestCaseError::fail)
}

/// Timing and value of every hap, sorted, queried one cycle at a time. Context
/// is left out on purpose: source locations are metadata, not pattern
/// semantics. The per-cycle query matters — `rev` splits its queries at cycle
/// boundaries (as upstream does), so a hap whose whole straddles a boundary
/// comes back as two parts from `rev` and one from the pattern underneath.
/// Asking both for a single cycle compares like with like.
fn rows(pat: &Pattern) -> Vec<String> {
    (0..2)
        .flat_map(|cycle| {
            let mut rows: Vec<String> = pat
                .query_arc(Frac::int(cycle), Frac::int(cycle + 1))
                .iter()
                .map(|h| format!("{:?}|{:?}|{:?}", h.part, h.whole, h.value))
                .collect();
            rows.sort();
            rows
        })
        .collect()
}

/// Well-formed mini-notation: a few atoms, then the operators layered over them.
/// Modifier targets are bracketed so a generated sequence keeps its shape
/// (`[a b]*2`, not `a b*2`, which is valid but means something else).
fn mini_source() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        Just("bd".to_string()),
        Just("sd".to_string()),
        Just("~".to_string()),
        Just("0".to_string()),
        Just("3".to_string()),
        Just("c4".to_string()),
    ];
    // Depth 3 and 24 nodes: enough to nest operators, small enough that a
    // failure is still readable and `*n` inside `*n` cannot explode.
    atom.prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 2..4).prop_map(|xs| xs.join(" ")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("[{a}, {b}]")),
            prop::collection::vec(inner.clone(), 2..4).prop_map(|xs| format!("<{}>", xs.join(" "))),
            (inner.clone(), 2i64..4).prop_map(|(a, n)| format!("[{a}]*{n}")),
            (inner.clone(), 2i64..4).prop_map(|(a, n)| format!("[{a}]/{n}")),
            (inner.clone(), 2i64..4).prop_map(|(a, n)| format!("[{a}]!{n}")),
            (inner.clone(), 2i64..4).prop_map(|(a, n)| format!("[{a}]@{n}")),
            (inner.clone(), 1i64..8, 2i64..9).prop_map(|(a, p, s)| format!(
                "[{a}]({},{})",
                p.min(s),
                s
            )),
            inner.clone().prop_map(|a| format!("[{a}]?")),
            (inner.clone(), inner, 2i64..5).prop_map(|(a, b, n)| format!("{{{a}, {b}}}%{n}")),
        ]
    })
}

proptest! {
    /// Junk in, `Err` out — never a panic. `parse` is where user text enters the
    /// engine, and callers like `parse_or_silence` turn an `Err` into silence,
    /// so an unexpected panic is the one failure mode they cannot absorb.
    #[test]
    fn arbitrary_input_never_panics(src in r"[a-z0-9~<>\[\]{}()*/!@?,.:_ -]{0,40}") {
        let _ = rudel_mini::parse(&src);
        let _ = rudel_mini::leaf_locations(&src);
    }

    /// Every hap lands inside the queried span, and inside its own whole. Both
    /// are assumed all over the scheduler and the drawing code.
    #[test]
    fn haps_stay_inside_the_query_span_and_their_whole(src in mini_source()) {
        let pat = parsed(&src)?;
        for h in pat.query_arc(Frac::zero(), Frac::int(2)) {
            prop_assert!(
                h.part.begin >= Frac::zero() && h.part.end <= Frac::int(2),
                "part {:?} escapes the query span in {src:?}",
                h.part
            );
            if let Some(whole) = h.whole {
                prop_assert!(
                    whole.begin <= h.part.begin && h.part.end <= whole.end,
                    "part {:?} escapes whole {whole:?} in {src:?}",
                    h.part
                );
            }
        }
    }

    /// Querying is a pure function of the span. Anything that carries state
    /// between queries — a cached RNG, a global registry — shows up here.
    #[test]
    fn querying_twice_gives_the_same_haps(src in mini_source()) {
        let pat = parsed(&src)?;
        prop_assert_eq!(rows(&pat), rows(&pat));
    }

    /// `rev` is its own inverse: reflecting a cycle twice is the identity.
    #[test]
    fn reversing_twice_restores_the_pattern(src in mini_source()) {
        let pat = parsed(&src)?;
        prop_assert_eq!(rows(&pat.rev().rev()), rows(&pat));
    }

    /// `slow` undoes `fast`: the two are one time scaling and its reciprocal.
    #[test]
    fn slowing_undoes_speeding_up(src in mini_source(), n in 2i32..5) {
        let pat = parsed(&src)?;
        prop_assert_eq!(rows(&pat.fast(n).slow(n)), rows(&pat));
    }
}
