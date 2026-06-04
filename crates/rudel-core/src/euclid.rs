// euclid.rs - Bjorklund / Euclidean rhythms.
// Ported from strudel/packages/core/euclid.mjs (itself after Rohan Drape's hmt).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::pattern::{Pattern, fastcat, pure};
use crate::transforms::IntoPattern;
use crate::value::Value;

fn split_at(n: usize, v: &[Vec<i32>]) -> (Vec<Vec<i32>>, Vec<Vec<i32>>) {
    let n = n.min(v.len());
    (v[..n].to_vec(), v[n..].to_vec())
}

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
    let (_xs, __xs) = split_at(offs as usize, &xs);
    ((offs, ons - offs), (zip_concat(&_xs, &ys), __xs))
}

fn right(n: Counts, x: Buckets) -> (Counts, Buckets) {
    let (ons, offs) = n;
    let (xs, ys) = x;
    let (_ys, __ys) = split_at(ons as usize, &ys);
    ((ons, offs - ons), (zip_concat(&xs, &_ys), __ys))
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
    let r = rotation.rem_euclid(len) as usize;
    let mut out = b[r..].to_vec();
    out.extend_from_slice(&b[..r]);
    out
}

fn bools_pattern(bools: &[bool]) -> Pattern {
    let pats: Vec<Pattern> = bools.iter().map(|&b| pure(Value::Bool(b))).collect();
    fastcat(&pats)
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
}

/// Build a boolean pattern from a Euclidean rhythm, e.g. for `struct`.
pub fn euclid_bools(pulses: i64, steps: i64) -> impl IntoPattern {
    bools_pattern(&euclid_rot(pulses, steps, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
