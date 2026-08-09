use crate::{
    pattern::Pattern,
    transforms::IntoPattern,
    value::{Value, ValueMap},
};

pub(super) fn single(name: &str, v: Value) -> Value {
    let mut m = ValueMap::new();
    m.insert(name.to_string(), v);
    Value::Map(m)
}

/// One value wrapped into `{ name: value }`, which is Strudel's `withVal`.
///
/// A value that is already a map keeps its keys — except for an *unnamed*
/// `value`, which is promoted into the control's own key:
///
/// ```js
/// if (typeof xs === 'object' && xs.value !== undefined) {
///   bag = { ...xs }; xs = xs.value; delete bag.value;
/// }
/// ```
///
/// That is how `"A5".color('#54C571').note()` becomes
/// `{note: "A5", color: '#54C571'}`. Leaving the map alone instead carries an
/// inert `value` alongside a control that was never set, which reaches the
/// voices as silence — and tunes routinely colour or label a layer before
/// naming its sound. `createParam` applies this on all three of its paths (bare
/// method, standalone function, and argument), so every caller here does too.
fn with_val(name: &str, v: Value) -> Value {
    match v {
        Value::Map(mut m) if m.contains_key("value") => {
            if let Some(inner) = m.shift_remove("value") {
                m.insert(name.to_string(), inner);
            }
            Value::Map(m)
        }
        Value::Map(_) => v,
        other => single(name, other),
    }
}

/// Wrap each value of `pat` into `{ name: value }`.
pub(super) fn control(name: &'static str, pat: Pattern) -> Pattern {
    pat.fmap(move |v| with_val(name, v))
}

/// Wrap each value of `pat` into `{ name: value }` for a runtime control name
/// (the `'static` variant above can't take an owned `String`). Powers the
/// generic `ctrl(name, value)` setter for controls without a dedicated method.
pub fn control_dyn(name: impl Into<String>, pat: impl IntoPattern) -> Pattern {
    let name = name.into();
    pat.into_pattern().fmap(move |v| with_val(&name, v))
}

/// Wrap each current value of `pat` into `{ name: value }`. This is the no-arg
/// control method behavior used by Strudel for chains like
/// `i(...).tune(...).freq()`.
pub fn wrap_control_dyn(name: impl Into<String>, pat: impl IntoPattern) -> Pattern {
    control_dyn(name, pat)
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
            let mut m = ValueMap::new();
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
