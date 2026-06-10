use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

pub(super) fn single(name: &str, v: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(name.to_string(), v);
    Value::Map(m)
}

/// Wrap each value of `pat` into `{ name: value }`. If a value is already a
/// map it is left untouched (it already carries its keys).
pub(super) fn control(name: &'static str, pat: Pattern) -> Pattern {
    pat.fmap(move |v| match v {
        Value::Map(_) => v,
        other => single(name, other),
    })
}

/// Wrap each value of `pat` into `{ name: value }` for a runtime control name
/// (the `'static` variant above can't take an owned `String`). Powers the
/// generic `ctrl(name, value)` setter for controls without a dedicated method.
pub fn control_dyn(name: impl Into<String>, pat: impl IntoPattern) -> Pattern {
    let name = name.into();
    pat.into_pattern().fmap(move |v| match v {
        Value::Map(_) => v,
        other => single(&name, other),
    })
}

/// Wrap each current value of `pat` into `{ name: value }`. This is the no-arg
/// control method behavior used by Strudel for chains like
/// `i(...).tune(...).freq()`.
pub fn wrap_control_dyn(name: impl Into<String>, pat: impl IntoPattern) -> Pattern {
    let name = name.into();
    pat.into_pattern().fmap(move |v| match v {
        Value::Map(mut m) if m.contains_key("value") => {
            if let Some(value) = m.remove("value") {
                m.insert(name.clone(), value);
            }
            Value::Map(m)
        }
        Value::Map(_) => v,
        other => single(&name, other),
    })
}

/// View a value as positional parts: a list yields its items, anything else
/// is a single part. Mini-notation `a:b:c` values arrive as lists.
pub(super) fn value_parts(v: &Value) -> Vec<Value> {
    match v {
        Value::List(items) => items.clone(),
        other => vec![other.clone()],
    }
}

/// Wrap positional values into the given control keys: `[x, y]` becomes
/// `{ names[0]: x, names[1]: y }`. Extra parts are dropped, missing parts
/// leave their key unset. Powers Strudel's multi-control helpers.
pub(super) fn spread_control(names: &'static [&'static str], pat: Pattern) -> Pattern {
    pat.fmap(move |v| match v {
        Value::Map(_) => v,
        other => {
            let mut m = BTreeMap::new();
            for (key, val) in names.iter().zip(value_parts(&other)) {
                m.insert(key.to_string(), val);
            }
            Value::Map(m)
        }
    })
}

impl Pattern {
    /// Wrap this pattern's current values into a control map.
    pub fn wrap_control(&self, name: impl Into<String>) -> Pattern {
        wrap_control_dyn(name, self.clone())
    }

    /// Set an arbitrary named control, keeping this pattern's structure. The
    /// escape hatch for controls without a dedicated method.
    pub fn ctrl(&self, name: impl Into<String>, x: impl IntoPattern) -> Pattern {
        self.set(control_dyn(name, x))
    }
}
