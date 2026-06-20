use super::Align;
use super::IntoPattern;
use super::value_ops::{
    bit_and, bit_lshift, bit_or, bit_rshift, bit_xor, cmp_eq, cmp_eqt, cmp_gt, cmp_gte, cmp_lt,
    cmp_lte, cmp_ne, cmp_net, logic_and, logic_or, num_add, num_div, num_mod, num_mul, num_pow,
    num_sub,
};
use crate::pattern::Pattern;
use crate::value::Value;

/// Generate the six non-default alignment methods for one operator, e.g.
/// `add_out`, `add_squeeze`, ... The default (`in`) variant stays as the plain
/// `add`/`sub`/... method.
macro_rules! aligned_variants {
    ($op:expr; $out:ident $mix:ident $sq:ident $sqo:ident $reset:ident $restart:ident $poly:ident) => {
        #[doc = "Polymetric alignment (`poly`)."]
        pub fn $poly(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Poly, $op)
        }
        #[doc = "Structure from the right (`out` alignment)."]
        pub fn $out(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Out, $op)
        }
        #[doc = "Structure from the intersection of both (`mix` alignment)."]
        pub fn $mix(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Mix, $op)
        }
        #[doc = "Squeeze one cycle of `other` into each event (`squeeze`)."]
        pub fn $sq(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Squeeze, $op)
        }
        #[doc = "Squeeze one cycle of this into each event of `other` (`squeezeOut`)."]
        pub fn $sqo(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::SqueezeOut, $op)
        }
        #[doc = "Retrigger this pattern at each onset of `other` (`reset`)."]
        pub fn $reset(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Reset, $op)
        }
        #[doc = "Retrigger from cycle zero at each onset of `other` (`restart`)."]
        pub fn $restart(&self, other: impl IntoPattern) -> Pattern {
            self.op_align(other.into_pattern(), Align::Restart, $op)
        }
    };
}

macro_rules! op_in_methods {
    ($($(
        #[$attr:meta]
    )* $method:ident => $op:expr),* $(,)?) => {
        $(
            $(#[$attr])*
            pub fn $method(&self, other: impl IntoPattern) -> Pattern {
                self.op_in(other.into_pattern(), $op)
            }
        )*
    };
}

impl Pattern {
    // -- Alignment matrix --------------------------------------------------
    // Each operator's default (`in`) variant is the plain method (`add`, `set`,
    // ...); these generate the remaining alignments (`add_out`, `set_squeeze`, ...).

    aligned_variants!(num_add; add_out add_mix add_squeeze add_squeezeout add_reset add_restart add_poly);
    aligned_variants!(num_sub; sub_out sub_mix sub_squeeze sub_squeezeout sub_reset sub_restart sub_poly);
    aligned_variants!(num_mul; mul_out mul_mix mul_squeeze mul_squeezeout mul_reset mul_restart mul_poly);
    aligned_variants!(num_div; div_out div_mix div_squeeze div_squeezeout div_reset div_restart div_poly);
    aligned_variants!(|_a: &Value, b: &Value| b.clone();
        set_out set_mix set_squeeze set_squeezeout set_reset set_restart set_poly);
    aligned_variants!(|a: &Value, _b: &Value| a.clone();
        keep_out keep_mix keep_squeeze keep_squeezeout keep_reset keep_restart keep_poly);
    aligned_variants!(num_mod; modulo_out modulo_mix modulo_squeeze modulo_squeezeout modulo_reset modulo_restart modulo_poly);
    aligned_variants!(num_pow; pow_out pow_mix pow_squeeze pow_squeezeout pow_reset pow_restart pow_poly);

    // -- Math / value ops --------------------------------------------------

    op_in_methods! {
        add => num_add,
        sub => num_sub,
        mul => num_mul,
        div => num_div,

        /// `set`: override this pattern's values (and map keys) with the other's,
        /// keeping this pattern's structure.
        set => |_, b: &Value| b.clone(),

        /// Less-than (`lt`): boolean pattern, structure from this pattern.
        lt => cmp_lt,
        /// Greater-than (`gt`).
        gt => cmp_gt,
        /// Less-than-or-equal (`lte`).
        lte => cmp_lte,
        /// Greater-than-or-equal (`gte`).
        gte => cmp_gte,
        /// Loose equality (`eq`, numeric coercion).
        eq => cmp_eq,
        /// Strict equality (`eqt`, no coercion).
        eqt => cmp_eqt,
        /// Loose inequality (`ne`).
        ne => cmp_ne,
        /// Strict inequality (`net`).
        net => cmp_net,
        /// Logical and (`and`): JS `a && b` per event.
        and => logic_and,
        /// Logical or (`or`): JS `a || b` per event.
        or => logic_or,

        /// Bitwise AND (`band`): int32 `a & b` per event, structure from the left.
        band => bit_and,
        /// Bitwise OR (`bor`).
        bor => bit_or,
        /// Bitwise XOR (`bxor`).
        bxor => bit_xor,
        /// Bitwise left shift (`blshift`).
        blshift => bit_lshift,
        /// Bitwise right shift (`brshift`).
        brshift => bit_rshift,
    }

    /// Scale a unipolar (0..1) signal into the `min..max` range.
    pub fn range(&self, min: f64, max: f64) -> Pattern {
        self.fmap(move |v| Value::F64(v.as_f64().unwrap_or(0.0) * (max - min) + min))
    }
}
