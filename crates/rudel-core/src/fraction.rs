// fraction.rs - rational time values, ported from strudel/packages/core/fraction.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use num_integer::Integer;
use num_rational::Ratio;
use num_traits::{Signed, ToPrimitive, Zero};
use std::{
    fmt,
    ops::{Add, Div, Mul, Neg, Rem, Sub},
};

/// The integer backing [`Frac`]. `i128` gives ample headroom so deep
/// `lcm`/`compress` arithmetic doesn't overflow (the `Rational64` version did).
type Rat = Ratio<i128>;

/// A rational number used for all time values in the pattern engine.
///
/// Wraps `Ratio<i128>`. Mirrors the `Fraction.prototype.*` helpers Strudel
/// attaches in `fraction.mjs` (`sam`, `nextSam`, `cyclePos`, ...).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frac(pub Rat);

/// Largest denominator a converted `f64` may take.
const MAX_FROM_F64_DENOM: i128 = 1_000_000;

impl Frac {
    pub fn new(numer: i64, denom: i64) -> Self {
        Frac(Rat::new(numer as i128, denom as i128))
    }

    pub fn int(n: i64) -> Self {
        Frac(Rat::from_integer(n as i128))
    }

    /// Convert from an `f64` parameter value.
    ///
    /// Integers are exact. Everything else takes the simplest rational that the
    /// `f64` still rounds to, found by walking the continued-fraction
    /// convergents and stopping once the denominator would pass
    /// [`MAX_FROM_F64_DENOM`] — which is what Fraction.js does for a JS number,
    /// and why Strudel's spans stay legible.
    ///
    /// The bound matters: the exact rational behind an `f64` has a denominator
    /// near 2^52, and pattern arithmetic multiplies denominators until they
    /// overflow. But rounding onto a fixed grid instead, as this used to,
    /// destroys the simple fractions a tune is actually made of — `1/6` became
    /// `166667/1000000` and `.fast(2/3)` put every span on a denominator of
    /// 666667, which no longer lines up with anything.
    pub fn from_f64(x: f64) -> Self {
        if !x.is_finite() {
            return Frac::zero();
        }
        if x == x.trunc() && x.abs() < 9.0e18 {
            return Frac::int(x as i64);
        }
        // Convergents h/k of the continued fraction for |x|, each the best
        // rational approximation for its denominator.
        let (mut h_prev, mut h) = (0i128, 1i128);
        let (mut k_prev, mut k) = (1i128, 0i128);
        let mut rest = x.abs();
        loop {
            let whole = rest.floor();
            // Guard the cast: a huge term means the remainder has collapsed to
            // numerical noise, and the convergent already in hand is the answer.
            if whole > MAX_FROM_F64_DENOM as f64 {
                break;
            }
            let term = whole as i128;
            let (Some(h_next), Some(k_next)) = (
                term.checked_mul(h).and_then(|t| t.checked_add(h_prev)),
                term.checked_mul(k).and_then(|t| t.checked_add(k_prev)),
            ) else {
                break;
            };
            if k_next > MAX_FROM_F64_DENOM {
                break;
            }
            (h_prev, h) = (h, h_next);
            (k_prev, k) = (k, k_next);
            let frac = rest - whole;
            // Converged: the remaining term is `f64` dust, not structure.
            if frac <= 1e-12 {
                break;
            }
            rest = 1.0 / frac;
            if !rest.is_finite() {
                break;
            }
        }
        if k == 0 {
            return Frac::zero();
        }
        Frac(Rat::new(if x < 0.0 { -h } else { h }, k))
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
    fn from_f64_recovers_the_fraction_the_user_wrote() {
        // The fractions tunes are made of come back exactly, however they were
        // spelled — `1/6` used to arrive as `166667/1000000`, and every span
        // derived from it inherited that denominator.
        for (x, n, d) in [
            (0.1875, 3, 16),
            (1.0 / 6.0, 1, 6),
            (2.0 / 3.0, 2, 3),
            (1.0 / 3.0, 1, 3),
            (0.1, 1, 10),
            (0.125, 1, 8),
            (-0.75, -3, 4),
            (1.0 / 12.0, 1, 12),
        ] {
            assert_eq!(Frac::from_f64(x), Frac::new(n, d), "{x}");
        }
        // Integers stay exact, and non-finite input is zero rather than a panic.
        assert_eq!(Frac::from_f64(4.0), Frac::int(4));
        assert_eq!(Frac::from_f64(-0.0), Frac::zero());
        assert_eq!(Frac::from_f64(f64::NAN), Frac::zero());
        assert_eq!(Frac::from_f64(f64::INFINITY), Frac::zero());
        // A value with no small rational behind it is still bounded, and still
        // close: the denominator cap is what keeps pattern arithmetic from
        // overflowing on 2^52-denominator exact conversions.
        let approx = Frac::from_f64(std::f64::consts::PI);
        assert!(approx.denom() <= MAX_FROM_F64_DENOM, "{approx}");
        assert!(
            (approx.to_f64() - std::f64::consts::PI).abs() < 1e-9,
            "{approx}"
        );
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

    #[test]
    fn whole_numbers_skip_the_continued_fraction() {
        // The convergent loop bails out on any term above the denominator
        // limit, so an integer larger than that only survives by taking the
        // exact-integer path first.
        assert_eq!(Frac::from_f64(2_000_000.0), Frac::int(2_000_000));
        assert_eq!(Frac::from_f64(-2_000_000.0), Frac::int(-2_000_000));
        assert_eq!(Frac::from_f64(3.0), Frac::int(3));
        // Past what an i64 can hold there is no answer to give, and the
        // saturating cast would invent one.
        assert_eq!(Frac::from_f64(1e19), Frac::zero());
        assert_eq!(Frac::from_f64(f64::INFINITY), Frac::zero());
        // Simple fractions stay simple.
        assert_eq!(Frac::from_f64(1.0 / 6.0), Frac::new(1, 6));
        assert_eq!(Frac::from_f64(-0.75), Frac::new(-3, 4));
    }

    #[test]
    fn gcd_over_an_iterator_skips_the_absent_ones() {
        assert_eq!(
            gcd_opt([Some(Frac::new(1, 2)), None, Some(Frac::new(1, 3))]),
            Some(Frac::new(1, 6))
        );
        assert_eq!(
            gcd_opt([Some(Frac::int(4)), Some(Frac::int(6))]),
            Some(Frac::int(2))
        );
        assert_eq!(gcd_opt([None, None]), None);
        assert_eq!(gcd_opt([]), None);
    }
}
