use crate::{
    fraction::Frac,
    pattern::{Pattern, fastcat, pure},
    value::Value,
};

/// Set `key` to `value` on a map value, leaving non-maps untouched (used by
/// `jux`/`hurry`).
pub(super) fn set_key(v: Value, key: &str, value: Value) -> Value {
    match v {
        Value::Map(mut m) => {
            m.insert(key.to_string(), value);
            Value::Map(m)
        }
        other => other,
    }
}

pub(super) fn frac(n: impl Into<Frac>) -> Frac {
    n.into()
}

pub(super) fn seq2(a: Frac, b: Frac) -> Pattern {
    fastcat(&[pure(Value::Frac(a)), pure(Value::Frac(b))])
}
