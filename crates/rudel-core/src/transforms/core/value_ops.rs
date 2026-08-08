use crate::value::Value;
use std::sync::Arc;

/// A shared two-argument value combiner (the per-element op behind `add`, `set`,
/// ... before map-structural composition).
pub(super) type ValueOp = Arc<dyn Fn(&Value, &Value) -> Value + Send + Sync>;

fn as_map(v: &Value) -> Value {
    match v {
        Value::Map(_) => v.clone(),
        other => {
            let mut m = crate::value::ValueMap::new();
            m.insert("value".to_string(), other.clone());
            Value::Map(m)
        }
    }
}

/// Combine two values with `op`, unioning structurally when either is a map
/// (`_composeOp`).
pub(super) fn compose_op(
    a: &Value,
    b: &Value,
    op: &(dyn Fn(&Value, &Value) -> Value + Send + Sync),
) -> Value {
    match (a, b) {
        (Value::Map(_), _) | (_, Value::Map(_)) => as_map(a).union_with(&as_map(b), op),
        _ => op(a, b),
    }
}

pub(super) fn num_add(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0)),
    }
}

pub(super) fn num_sub(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x - y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)),
    }
}

pub(super) fn num_mul(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Int(x * y),
        _ => Value::F64(a.as_f64().unwrap_or(0.0) * b.as_f64().unwrap_or(0.0)),
    }
}

pub(super) fn num_div(a: &Value, b: &Value) -> Value {
    Value::F64(a.as_f64().unwrap_or(0.0) / b.as_f64().unwrap_or(1.0))
}

pub(crate) fn num_mod(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x.rem_euclid(*y)),
        _ => Value::F64(
            a.as_f64()
                .unwrap_or(0.0)
                .rem_euclid(b.as_f64().unwrap_or(1.0)),
        ),
    }
}

pub(crate) fn num_pow(a: &Value, b: &Value) -> Value {
    Value::F64(a.as_f64().unwrap_or(0.0).powf(b.as_f64().unwrap_or(0.0)))
}

// Bitwise value ops (`band`/`bor`/`bxor`/`blshift`/`brshift`). Strudel wraps
// these in `numeralArgs`, so operands are parsed as numerals (note names ->
// midi) and JS bitwise acts on int32; we mirror that with `i32` arithmetic.
fn numeral_i32(v: &Value) -> i32 {
    let n = v
        .as_f64()
        .or_else(|| {
            v.as_str()
                .and_then(|s| crate::tonal::note_to_midi(s).map(|m| m as f64))
        })
        .unwrap_or(0.0);
    n as i64 as i32
}

pub(super) fn bit_and(a: &Value, b: &Value) -> Value {
    Value::Int((numeral_i32(a) & numeral_i32(b)) as i64)
}

pub(super) fn bit_or(a: &Value, b: &Value) -> Value {
    Value::Int((numeral_i32(a) | numeral_i32(b)) as i64)
}

pub(super) fn bit_xor(a: &Value, b: &Value) -> Value {
    Value::Int((numeral_i32(a) ^ numeral_i32(b)) as i64)
}

pub(super) fn bit_lshift(a: &Value, b: &Value) -> Value {
    // JS shifts mask the count to 5 bits (`b & 31`).
    Value::Int(numeral_i32(a).wrapping_shl(numeral_i32(b) as u32 & 31) as i64)
}

pub(super) fn bit_rshift(a: &Value, b: &Value) -> Value {
    // `>>` is an arithmetic (sign-propagating) shift, like JS.
    Value::Int((numeral_i32(a) >> (numeral_i32(b) as u32 & 31)) as i64)
}

// Comparison / logic value ops (the `lt`/`gt`/.../`and`/`or` COMPOSERS). They
// compare numerically when both sides are numbers (or numeric strings), else
// lexically; results are booleans, handy as `struct`/`mask` gates.
fn value_ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

pub(super) fn cmp_lt(a: &Value, b: &Value) -> Value {
    Value::Bool(value_ordering(a, b) == Some(std::cmp::Ordering::Less))
}

pub(super) fn cmp_gt(a: &Value, b: &Value) -> Value {
    Value::Bool(value_ordering(a, b) == Some(std::cmp::Ordering::Greater))
}

pub(super) fn cmp_lte(a: &Value, b: &Value) -> Value {
    Value::Bool(matches!(
        value_ordering(a, b),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ))
}

pub(super) fn cmp_gte(a: &Value, b: &Value) -> Value {
    Value::Bool(matches!(
        value_ordering(a, b),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    ))
}

/// Loose equality (`==`): numeric coercion when both look like numbers.
fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

pub(super) fn cmp_eq(a: &Value, b: &Value) -> Value {
    Value::Bool(loose_eq(a, b))
}

pub(super) fn cmp_ne(a: &Value, b: &Value) -> Value {
    Value::Bool(!loose_eq(a, b))
}

/// Strict equality (`===`): no string/number coercion (`Value` equality).
pub(super) fn cmp_eqt(a: &Value, b: &Value) -> Value {
    Value::Bool(a == b)
}

pub(super) fn cmp_net(a: &Value, b: &Value) -> Value {
    Value::Bool(a != b)
}

/// JS `&&`/`||`: return one operand based on the left's truthiness.
pub(super) fn logic_and(a: &Value, b: &Value) -> Value {
    if a.truthy() { b.clone() } else { a.clone() }
}

pub(super) fn logic_or(a: &Value, b: &Value) -> Value {
    if a.truthy() { a.clone() } else { b.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Value {
        Value::Str(x.to_string())
    }

    /// Int op Int stays an `Int` — Strudel snapshots `n("0 1 2")` as integers,
    /// so falling through to the f64 arm would print `0.0` everywhere.
    #[test]
    fn integer_arithmetic_keeps_its_type_and_sign() {
        // `Value`'s equality coerces across Int/F64, so the *variant* is what
        // has to be asserted here — `Int(5) == F64(5.0)` is true.
        for (op, want) in [
            (num_add(&Value::Int(2), &Value::Int(3)), 5),
            (num_sub(&Value::Int(2), &Value::Int(3)), -1),
            (num_mul(&Value::Int(2), &Value::Int(3)), 6),
            (num_mod(&Value::Int(-1), &Value::Int(3)), 2),
        ] {
            assert!(matches!(op, Value::Int(x) if x == want), "{op:?} != Int({want})");
        }

        // Anything else coerces through f64, including numeric strings.
        assert_eq!(num_add(&Value::F64(0.5), &Value::Int(1)), Value::F64(1.5));
        assert_eq!(num_sub(&Value::F64(0.5), &Value::Int(2)), Value::F64(-1.5));
        assert_eq!(num_sub(&s("7"), &s("2")), Value::F64(5.0));
        // A non-numeric operand reads as 0 rather than erroring.
        assert_eq!(num_sub(&Value::Int(3), &Value::Null), Value::F64(3.0));
        assert_eq!(num_sub(&Value::Null, &Value::Int(3)), Value::F64(-3.0));
    }

    #[test]
    fn division_and_modulo_have_their_own_fallbacks() {
        assert_eq!(num_div(&Value::Int(7), &Value::Int(2)), Value::F64(3.5));
        // A missing divisor defaults to 1, so `div` never yields infinity by
        // accident; a missing dividend is 0.
        assert_eq!(num_div(&Value::Int(7), &Value::Null), Value::F64(7.0));

        // `mod` is Euclidean, so a negative left operand still lands in range.
        assert_eq!(num_mod(&Value::Int(-1), &Value::Int(3)), Value::Int(2));
        assert_eq!(num_mod(&Value::Int(7), &Value::Int(3)), Value::Int(1));
        assert!(matches!(num_mod(&Value::Int(7), &Value::Int(3)), Value::Int(_)));
        // A zero integer modulus would panic, so it takes the f64 path (NaN)
        // rather than the `rem_euclid` on ints.
        assert!(matches!(
            num_mod(&Value::Int(7), &Value::Int(0)),
            Value::F64(x) if x.is_nan()
        ));
        assert_eq!(num_pow(&Value::Int(2), &Value::Int(10)), Value::F64(1024.0));
    }

    /// The bitwise ops go through `numeralArgs`, so note names become midi and
    /// the arithmetic is int32.
    #[test]
    fn bitwise_ops_work_on_int32_numerals() {
        // 6 and 3 differ under and/or/xor, so none can stand in for another.
        assert_eq!(bit_and(&Value::Int(6), &Value::Int(3)), Value::Int(2));
        assert_eq!(bit_or(&Value::Int(6), &Value::Int(3)), Value::Int(7));
        assert_eq!(bit_xor(&Value::Int(6), &Value::Int(3)), Value::Int(5));
        assert_eq!(bit_lshift(&Value::Int(3), &Value::Int(2)), Value::Int(12));
        // Arithmetic (sign-propagating) shift, like JS `>>`.
        assert_eq!(bit_rshift(&Value::Int(-8), &Value::Int(2)), Value::Int(-2));
        // A note name is a numeral: c5 is midi 72.
        assert_eq!(bit_and(&s("c5"), &Value::Int(0xFF)), Value::Int(72));
    }

    #[test]
    fn comparisons_coerce_numerically_then_fall_back_to_string_order() {
        let t = Value::Bool(true);
        let f = Value::Bool(false);

        // Numeric, including across Int/F64/numeric-string.
        assert_eq!(cmp_lt(&Value::Int(1), &Value::F64(1.5)), t);
        assert_eq!(cmp_lt(&Value::F64(1.5), &Value::Int(1)), f);
        assert_eq!(cmp_gt(&Value::F64(1.5), &Value::Int(1)), t);
        assert_eq!(cmp_gt(&Value::Int(1), &Value::F64(1.5)), f);
        // Equal is neither less nor greater, but satisfies both `<=` and `>=`.
        assert_eq!(cmp_lt(&Value::Int(2), &s("2")), f);
        assert_eq!(cmp_gt(&Value::Int(2), &s("2")), f);
        assert_eq!(cmp_lte(&Value::Int(2), &s("2")), t);
        assert_eq!(cmp_gte(&Value::Int(2), &s("2")), t);

        // Non-numeric strings order lexically.
        assert_eq!(cmp_lt(&s("bd"), &s("sd")), t);
        assert_eq!(cmp_gt(&s("bd"), &s("sd")), f);
        // Incomparable operands are false either way, not a panic.
        assert_eq!(cmp_lt(&Value::Null, &Value::Int(1)), f);
        assert_eq!(cmp_gt(&Value::Null, &Value::Int(1)), f);
        assert_eq!(cmp_lte(&Value::Null, &Value::Int(1)), f);
        assert_eq!(cmp_gte(&Value::Null, &Value::Int(1)), f);
    }

    /// `==` coerces `2` and `"2"`; `===` does not. Both directions matter — a
    /// negated result passes any test that only ever checks one outcome.
    #[test]
    fn loose_equality_coerces_where_strict_equality_does_not() {
        let t = Value::Bool(true);
        let f = Value::Bool(false);

        assert_eq!(cmp_eq(&Value::Int(2), &s("2")), t);
        assert_eq!(cmp_eq(&Value::Int(2), &Value::F64(2.0)), t);
        assert_eq!(cmp_eq(&Value::Int(2), &Value::Int(3)), f);
        assert_eq!(cmp_ne(&Value::Int(2), &s("2")), f);
        assert_eq!(cmp_ne(&Value::Int(2), &Value::Int(3)), t);
        // Neither side is numeric: falls back to `Value` equality.
        assert_eq!(cmp_eq(&s("bd"), &s("bd")), t);
        assert_eq!(cmp_eq(&s("bd"), &s("sd")), f);

        assert_eq!(cmp_eqt(&Value::Int(2), &s("2")), f);
        assert_eq!(cmp_eqt(&Value::Int(2), &Value::Int(2)), t);
        assert_eq!(cmp_net(&Value::Int(2), &s("2")), t);
        assert_eq!(cmp_net(&Value::Int(2), &Value::Int(2)), f);
    }
}
