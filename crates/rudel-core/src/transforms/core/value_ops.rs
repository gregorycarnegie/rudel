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
