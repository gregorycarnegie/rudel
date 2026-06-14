pub(super) use super::super::preprocess::preprocess_strudel;
pub(super) use super::super::{eval, eval_with_samples, filter_output, output_targets};
pub(super) use rudel_core::{Frac, Pattern, Value};

pub(super) fn values(pat: &Pattern, b: i64, e: i64) -> Vec<Value> {
    let mut haps = pat.query_arc(Frac::int(b), Frac::int(e));
    haps.sort_by_key(|h| h.part.begin);
    haps.into_iter().map(|h| h.value).collect()
}

/// Haps as `(begin, end, value)`, ordered, ignoring source-location context so
/// two spellings of the same pattern can be compared.
pub(super) fn shape(pat: &Pattern, cycles: i64) -> Vec<(Frac, Frac, Value)> {
    let mut haps: Vec<(Frac, Frac, Value)> = pat
        .query_arc(Frac::zero(), Frac::int(cycles))
        .into_iter()
        .map(|h| (h.part.begin, h.part.end, h.value))
        .collect();
    haps.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    haps
}
