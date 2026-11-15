use super::{IntoPattern, patternify::patternify_frac, value_ops::loose_eq};
use crate::{
    fraction::Frac,
    hap::Hap,
    pattern::{Pattern, pure},
    state::State,
    timespan::TimeSpan,
    value::Value,
};

impl Pattern {
    // -- Structure ---------------------------------------------------------

    pub(crate) fn _segment(&self, rate: Frac) -> Pattern {
        self.struct_pat(pure(Value::Bool(true))._fast(rate))
            .set_steps(Some(rate))
    }

    /// Sample a continuous pattern into `n` discrete steps per cycle.
    pub fn segment(&self, n: impl IntoPattern) -> Pattern {
        patternify_frac(self, n.into_pattern(), |p, f| p._segment(f))
    }

    /// Alias for [`segment`](Self::segment) (`seg`).
    pub fn seg(&self, n: impl IntoPattern) -> Pattern {
        self.segment(n)
    }

    /// Restructure to the onsets of a boolean pattern, keeping this pattern's
    /// values (`struct`). Named `struct_pat` because `struct` is a keyword.
    pub fn struct_pat(&self, bools: impl IntoPattern) -> Pattern {
        self.fmap(|a| Value::func(move |b| if b.truthy() { a.clone() } else { Value::Null }))
            .app_right(&bools.into_pattern())
            .filter_values(|v| !matches!(v, Value::Null))
    }

    /// Silence this pattern wherever the mask pattern is false (`mask`).
    pub fn mask(&self, bools: impl IntoPattern) -> Pattern {
        self.fmap(|a| Value::func(move |b| if b.truthy() { a.clone() } else { Value::Null }))
            .app_left(&bools.into_pattern())
            .filter_values(|v| !matches!(v, Value::Null))
    }

    /// Like [`struct_pat`](Self::struct_pat), but each hap of `structure`
    /// plays the next cycle of this pattern (`isobite`, `cycleStruct`). With a
    /// different number of cycles here and haps there, that is an isorhythm.
    pub fn isobite(&self, structure: impl IntoPattern) -> Pattern {
        let structure = structure.into_pattern();
        // The structure's haps, each valued with its place in its cycle.
        let numbered = {
            let structure = structure.clone();
            Pattern::new(move |state| {
                cycle_haps(&structure, state)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, hap)| {
                        let part = state.span.intersection(&hap.part)?;
                        let value = Value::Int(i as i64);
                        Some(Hap { part, value, ..hap })
                    })
                    .collect()
            })
            .split_queries()
        };
        // One hap a cycle: how many haps the structure has in it.
        let count = Pattern::new(move |state| {
            let begin = state.span.begin;
            let cycle = TimeSpan::new(begin.sam(), begin.next_sam());
            let n = cycle_haps(&structure, state).len();
            vec![Hap::new(Some(cycle), state.span, Value::Int(n as i64))]
        })
        .split_queries();
        self.fast(count.clone()).bite(count, numbered)
    }
}

/// The haps `structure` has in the cycle a query starts in, in onset order,
/// without those whose value is `== 0` (`isobite`).
fn cycle_haps(structure: &Pattern, state: &State) -> Vec<Hap> {
    let begin = state.span.begin;
    let cycle = TimeSpan::new(begin.sam(), begin.next_sam());
    let mut haps: Vec<Hap> = structure
        .query(&state.set_span(cycle))
        .into_iter()
        .filter(|hap| !loose_eq(&hap.value, &Value::Int(0)))
        .collect();
    haps.sort_by_key(|hap| hap.whole_or_part().begin);
    haps
}
