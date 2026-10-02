// fraction.rs - rational time values, ported from strudel/packages/core/fraction.mjs
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use num_integer::Integer;
use num_rational::Ratio;
use num_traits::{Signed, ToPrimitive, Zero};
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    ops::{Add, Div, Mul, Neg, Rem, Sub},
};

/// The integer backing [`Frac`]. `i128` gives ample headroom so deep
/// `lcm`/`compress` arithmetic doesn't overflow (the `Rational64` version did).
type Rat = Ratio<i128>;

/// A rational number used for all time values in the pattern engine.
///
/// Wraps `Ratio<i128>`. Mirrors the `Fraction.prototype.*` helpers Strudel
/// attaches in `fraction.mjs` (`sam`, `nextSam`, `cyclePos`, ...).
///
/// The arithmetic, comparison and `floor` below take a 64-bit path whenever the
/// operands are small enough, which in pattern time is nearly always: `Ratio`
/// on `i128` reduces and compares with software 128-bit division
/// (`__divti3`), which was over a third of all query time. Results are the
/// same reduced rationals either way.
#[derive(Clone, Copy)]
pub struct Frac(pub Rat);

/// Largest denominator a converted `f64` may take.
const MAX_FROM_F64_DENOM: i128 = 1_000_000;

/// Small enough that a product of two, plus another such product, still fits
/// in an `i128`.
fn fits(x: i128) -> bool {
    x.unsigned_abs() < 1 << 62
}

/// `n/d` in lowest terms, for `d > 0`.
fn reduced(n: i128, d: i128) -> Frac {
    if let (Ok(abs), Ok(den)) = (u64::try_from(n.unsigned_abs()), u64::try_from(d)) {
        let g = abs.gcd(&den);
        let abs = i128::from(abs / g);
        return Frac(Rat::new_raw(
            if n < 0 { -abs } else { abs },
            i128::from(den / g),
        ));
    }
    Frac(Rat::new(n, d))
}

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
        self.floor()
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
        match (i64::try_from(self.numer()), i64::try_from(self.denom())) {
            (Ok(n), Ok(d)) => Frac::int(n.div_euclid(d)),
            _ => Frac(self.0.floor()),
        }
    }

    pub fn ceil(&self) -> Frac {
        match (i64::try_from(self.numer()), i64::try_from(self.denom())) {
            (Ok(n), Ok(d)) => Frac::int(n.div_euclid(d) + i64::from(n.rem_euclid(d) != 0)),
            _ => Frac(self.0.ceil()),
        }
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

impl Rem for Frac {
    type Output = Frac;
    fn rem(self, rhs: Frac) -> Frac {
        Frac(self.0 % rhs.0)
    }
}

impl Add for Frac {
    type Output = Frac;
    fn add(self, rhs: Frac) -> Frac {
        let (a, b, c, d) = (self.numer(), self.denom(), rhs.numer(), rhs.denom());
        if !(fits(a) && fits(b) && fits(c) && fits(d)) {
            return Frac(self.0 + rhs.0);
        }
        if b == d {
            reduced(a + c, b)
        } else {
            reduced(a * d + c * b, b * d)
        }
    }
}

impl Sub for Frac {
    type Output = Frac;
    fn sub(self, rhs: Frac) -> Frac {
        self + -rhs
    }
}

impl Mul for Frac {
    type Output = Frac;
    fn mul(self, rhs: Frac) -> Frac {
        let (a, b, c, d) = (self.numer(), self.denom(), rhs.numer(), rhs.denom());
        if !(fits(a) && fits(b) && fits(c) && fits(d)) {
            return Frac(self.0 * rhs.0);
        }
        reduced(a * c, b * d)
    }
}

impl Div for Frac {
    type Output = Frac;
    fn div(self, rhs: Frac) -> Frac {
        let (a, b, c, d) = (self.numer(), self.denom(), rhs.numer(), rhs.denom());
        // Division by zero is left to `Ratio`, which panics as it always did.
        if c == 0 || !(fits(a) && fits(b) && fits(c) && fits(d)) {
            return Frac(self.0 / rhs.0);
        }
        let (n, den) = (a * d, b * c);
        if den < 0 {
            reduced(-n, -den)
        } else {
            reduced(n, den)
        }
    }
}

impl Ord for Frac {
    fn cmp(&self, other: &Frac) -> Ordering {
        let (a, b, c, d) = (self.numer(), self.denom(), other.numer(), other.denom());
        if b == d {
            a.cmp(&c)
        } else if fits(a) && fits(b) && fits(c) && fits(d) {
            // Denominators are positive, so cross-multiplying keeps the order.
            (a * d).cmp(&(c * b))
        } else {
            self.0.cmp(&other.0)
        }
    }
}

impl PartialOrd for Frac {
    fn partial_cmp(&self, other: &Frac) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Frac {
    fn eq(&self, other: &Frac) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Frac {}

impl Hash for Frac {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

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
        fn fast_paths_agree_with_ratio(
            (a, b, c, d) in prop_oneof![
                (-1000i128..=1000, 1i128..=1000, -1000i128..=1000, 1i128..=1000),
                // Past `fits`, onto the `Ratio` fallback (but not so far that
                // `Ratio` itself overflows).
                (any::<i64>(), 1i128..=1 << 20, -1i128 << 20..=1 << 20, 1i128..=1 << 20)
                    .prop_map(|(a, b, c, d)| (i128::from(a) << 8, b, c, d)),
            ]
        ) {
            let (x, y) = (Rat::new(a, b), Rat::new(c, d));
            let (fx, fy) = (Frac(x), Frac(y));
            for (got, want) in [(fx + fy, x + y), (fx - fy, x - y), (fx * fy, x * y)] {
                prop_assert_eq!((got.numer(), got.denom()), (*want.numer(), *want.denom()));
            }
            if c != 0 {
                let (got, want) = (fx / fy, x / y);
                prop_assert_eq!((got.numer(), got.denom()), (*want.numer(), *want.denom()));
            }
            prop_assert_eq!(fx.cmp(&fy), x.cmp(&y));
            prop_assert_eq!(fx.floor().0, x.floor());
            prop_assert_eq!(fx.ceil().0, x.ceil());
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
    fn operands_past_64_bits_take_the_ratio_path_rather_than_overflowing() {
        // Cross-multiplying any of these overflows an i128, while `Ratio`
        // gets there through the lcm or by cross-reducing first; only the
        // fallback can answer. The odd numerator keeps `x` from reducing.
        let x = Frac(Rat::new((1 << 60) + 1, 1 << 70));
        let tiny = Frac(Rat::new(1, 1 << 71));
        assert_eq!(x + tiny, Frac(Rat::new((1 << 61) + 3, 1 << 71)));
        assert_eq!(x - tiny, Frac(Rat::new((1 << 61) + 1, 1 << 71)));
        assert_eq!(
            x * Frac(Rat::new(1 << 70, 3)),
            Frac(Rat::new((1 << 60) + 1, 3))
        );
        assert_eq!(x / tiny, Frac::int((1 << 61) + 2));
        assert!(tiny < x);
        assert!(Frac(Rat::new(1 << 125, 3)) > Frac(Rat::new(1 << 125, 5)));
    }

    #[test]
    fn only_operands_that_all_fit_take_the_64_bit_path() {
        // Two coprime 65-bit values: their product overflows an i128, but
        // `Ratio` cross-reduces p/q * q/p to 1 before multiplying.
        let (p, q) = ((1i128 << 64) + 1, (1i128 << 64) + 3);
        assert_eq!(Frac(Rat::new(p, q)) * Frac(Rat::new(q, p)), Frac::one());
        // Three components fit and one does not: the product of the
        // denominators overflows, while `Ratio` only needs their lcm.
        let sum = Frac::new(1, 2) + Frac(Rat::new(1, 1 << 126));
        assert_eq!(sum, Frac(Rat::new((1 << 125) + 1, 1 << 126)));
        // Each operator needs every component to fit, not just some: in each
        // of these the fast path's products overflow, while `Ratio` reduces
        // first and stays in range.
        let big = 1i128 << 126;
        assert_eq!(
            Frac::new(3, 5) * Frac(Rat::new(big, 3)),
            Frac(Rat::new(big, 5))
        );
        assert_eq!(
            Frac::new(3, 5) / Frac(Rat::new(3, big)),
            Frac(Rat::new(big, 5))
        );
        assert_eq!(
            Frac(Rat::new(3, big)) / Frac::new(3, 4),
            Frac(Rat::new(1, 1 << 124))
        );
        assert!(Frac(Rat::new(3, big)) < Frac::new(5, 7));
        // Past nine quintillion `from_f64` stops treating a float as an integer.
        assert_eq!(Frac::from_f64(9.0e18), Frac::zero());
    }

    #[test]
    fn dividing_by_zero_still_panics() {
        assert!(std::panic::catch_unwind(|| Frac::one() / Frac::zero()).is_err());
    }

    #[test]
    fn remainder_and_hash_follow_the_value() {
        use std::hash::{BuildHasher, RandomState};
        assert_eq!(Frac::new(7, 2) % Frac::int(2), Frac::new(3, 2));
        let state = RandomState::new();
        assert_eq!(
            state.hash_one(Frac::new(2, 4)),
            state.hash_one(Frac::new(1, 2))
        );
        assert_ne!(
            state.hash_one(Frac::new(1, 2)),
            state.hash_one(Frac::new(1, 3))
        );
    }

    #[test]
    fn from_f64_takes_terms_and_denominators_up_to_its_limit_inclusive() {
        // A term of exactly a million is allowed, and so is a denominator of
        // exactly a million.
        assert_eq!(Frac::from_f64(1_000_000.5), Frac::new(2_000_001, 2));
        assert_eq!(Frac::from_f64(1e-6), Frac::new(1, 1_000_000));
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
