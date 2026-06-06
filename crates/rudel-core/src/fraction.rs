// fraction.rs - rational time values, ported from strudel/packages/core/fraction.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use num_integer::Integer;
use num_rational::Ratio;
use num_traits::{Signed, ToPrimitive, Zero};
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

/// The integer backing [`Frac`]. `i128` gives ample headroom so deep
/// `lcm`/`compress` arithmetic doesn't overflow (the `Rational64` version did).
type Rat = Ratio<i128>;

/// A rational number used for all time values in the pattern engine.
///
/// Wraps `Ratio<i128>`. Mirrors the `Fraction.prototype.*` helpers Strudel
/// attaches in `fraction.mjs` (`sam`, `nextSam`, `cyclePos`, ...).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frac(pub Rat);

/// Grid used to quantize `f64` inputs into bounded rationals (1µ-cycle).
const FROM_F64_DENOM: i128 = 1_000_000;

impl Frac {
    pub fn new(numer: i64, denom: i64) -> Self {
        Frac(Rat::new(numer as i128, denom as i128))
    }

    pub fn int(n: i64) -> Self {
        Frac(Rat::from_integer(n as i128))
    }

    /// Convert from an `f64` parameter value. Integers are exact; other values
    /// are quantized to a fixed grid so the resulting rational stays small —
    /// exact `f64` fractions have denominator 2^52 and overflow under pattern
    /// arithmetic.
    pub fn from_f64(x: f64) -> Self {
        if !x.is_finite() {
            return Frac::zero();
        }
        if x == x.trunc() && x.abs() < 9.0e18 {
            return Frac::int(x as i64);
        }
        Frac(Rat::new(
            (x * FROM_F64_DENOM as f64).round() as i128,
            FROM_F64_DENOM,
        ))
    }

    pub fn zero() -> Self {
        Frac(Rat::zero())
    }

    pub fn one() -> Self {
        Frac(Rat::from_integer(1))
    }

    pub fn numer(&self) -> i128 {
        *self.0.numer()
    }

    pub fn denom(&self) -> i128 {
        *self.0.denom()
    }

    /// Returns the start of the cycle (floor).
    pub fn sam(&self) -> Frac {
        Frac(self.0.floor())
    }

    /// Returns the start of the next cycle.
    pub fn next_sam(&self) -> Frac {
        self.sam() + Frac::one()
    }

    /// The position of a time value relative to the start of its cycle.
    pub fn cycle_pos(&self) -> Frac {
        *self - self.sam()
    }

    pub fn floor(&self) -> Frac {
        Frac(self.0.floor())
    }

    pub fn ceil(&self) -> Frac {
        Frac(self.0.ceil())
    }

    pub fn min(self, other: Frac) -> Frac {
        if self < other { self } else { other }
    }

    pub fn max(self, other: Frac) -> Frac {
        if self > other { self } else { other }
    }

    pub fn abs(&self) -> Frac {
        Frac(self.0.abs())
    }

    pub fn to_f64(&self) -> f64 {
        self.0.to_f64().unwrap_or(f64::NAN)
    }

    /// gcd of two rationals: gcd(n1,n2) / lcm(d1,d2)
    pub fn gcd(self, other: Frac) -> Frac {
        let n = self.numer().gcd(&other.numer());
        let d = self.denom().lcm(&other.denom());
        Frac(Rat::new(n, d))
    }

    /// lcm of two rationals: lcm(n1,n2) / gcd(d1,d2)
    pub fn lcm(self, other: Frac) -> Frac {
        let n = self.numer().lcm(&other.numer());
        let d = self.denom().gcd(&other.denom());
        Frac(Rat::new(n, d))
    }
}

/// `lcm` over an iterator of optional fractions, matching `fraction.mjs` `lcm`:
/// any `None` poisons the result to `None`; an empty input yields `None`.
pub fn lcm_opt<I: IntoIterator<Item = Option<Frac>>>(iter: I) -> Option<Frac> {
    let mut items = iter.into_iter();
    let mut acc = items.next()??;
    for item in items {
        acc = acc.lcm(item?);
    }
    Some(acc)
}

/// `gcd` over an iterator, skipping `None`s (matches `fraction.mjs` `gcd`,
/// which calls `removeUndefineds`). Empty input yields `None`.
pub fn gcd_opt<I: IntoIterator<Item = Option<Frac>>>(iter: I) -> Option<Frac> {
    let mut acc: Option<Frac> = None;
    for item in iter.into_iter().flatten() {
        acc = Some(match acc {
            Some(a) => a.gcd(item),
            None => item,
        });
    }
    acc
}

macro_rules! impl_binop {
    ($trait:ident, $method:ident) => {
        impl $trait for Frac {
            type Output = Frac;
            fn $method(self, rhs: Frac) -> Frac {
                Frac($trait::$method(self.0, rhs.0))
            }
        }
    };
}
impl_binop!(Add, add);
impl_binop!(Sub, sub);
impl_binop!(Mul, mul);
impl_binop!(Div, div);
impl_binop!(Rem, rem);

impl Neg for Frac {
    type Output = Frac;
    fn neg(self) -> Frac {
        Frac(-self.0)
    }
}

impl From<i64> for Frac {
    fn from(n: i64) -> Self {
        Frac::int(n)
    }
}

impl fmt::Display for Frac {
    // matches Fraction.prototype.show: `${s*n}/${d}`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numer(), self.denom())
    }
}

impl fmt::Debug for Frac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn small_frac() -> impl Strategy<Value = Frac> {
        (-10_000i64..=10_000, 1i64..=10_000).prop_map(|(n, d)| Frac::new(n, d))
    }

    #[test]
    fn sam_and_cycle_pos() {
        let t = Frac::new(5, 4);
        assert_eq!(t.sam(), Frac::int(1));
        assert_eq!(t.next_sam(), Frac::int(2));
        assert_eq!(t.cycle_pos(), Frac::new(1, 4));
    }

    #[test]
    fn lcm_gcd_rationals() {
        assert_eq!(Frac::new(1, 2).lcm(Frac::new(1, 3)), Frac::int(1));
        assert_eq!(Frac::new(1, 2).gcd(Frac::new(1, 3)), Frac::new(1, 6));
        assert_eq!(
            lcm_opt([Some(Frac::int(2)), Some(Frac::int(3))]),
            Some(Frac::int(6))
        );
        assert_eq!(lcm_opt([Some(Frac::int(2)), None]), None);
    }

    proptest! {
        #[test]
        fn cycle_pos_is_normalized(t in small_frac()) {
            let pos = t.cycle_pos();

            prop_assert!(pos >= Frac::zero());
            prop_assert!(pos < Frac::one());
            prop_assert_eq!(t.sam() + pos, t);
            prop_assert!(t.sam() <= t);
            prop_assert!(t < t.next_sam());
            prop_assert_eq!(t.next_sam(), t.sam() + Frac::one());
        }

        #[test]
        fn from_f64_quantizes_finite_values(x in -1_000_000.0f64..=1_000_000.0) {
            let got = Frac::from_f64(x).to_f64();
            prop_assert!(
                (got - x).abs() <= 0.000001,
                "expected {x} to round-trip within the fixed grid, got {got}"
            );
        }

        #[test]
        fn integer_gcd_lcm_product_identity(a in 1i64..=10_000, b in 1i64..=10_000) {
            let a = Frac::int(a);
            let b = Frac::int(b);

            prop_assert_eq!(a.gcd(b) * a.lcm(b), a * b);
            prop_assert_eq!(a.gcd(b), b.gcd(a));
            prop_assert_eq!(a.lcm(b), b.lcm(a));
        }
    }
}
