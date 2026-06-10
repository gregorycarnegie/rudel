use super::base::single;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

/// The `s`/`sound` control, with `"name:index"` splitting into `{ s, n }`.
pub fn s(pat: impl IntoPattern) -> Pattern {
    pat.into_pattern().fmap(|v| match v {
        Value::Str(ref string) if string.contains(':') => {
            let mut parts = string.splitn(2, ':');
            let mut m = BTreeMap::new();
            m.insert(
                "s".to_string(),
                Value::Str(parts.next().unwrap_or("").to_string()),
            );
            if let Some(idx) = parts.next() {
                // Numeric tails become an integer `n`; non-numeric tails (chord
                // symbols, named samples) are preserved as a string `n`.
                let n = match idx.parse::<i64>() {
                    Ok(n) => Value::Int(n),
                    Err(_) => Value::Str(idx.to_string()),
                };
                m.insert("n".to_string(), n);
            }
            Value::Map(m)
        }
        // mini-notation produces a list for `bd:3`
        Value::List(ref items) if !items.is_empty() => {
            let mut m = BTreeMap::new();
            m.insert("s".to_string(), items[0].clone());
            if let Some(idx) = items.get(1) {
                m.insert("n".to_string(), idx.clone());
            }
            Value::Map(m)
        }
        Value::Map(_) => v,
        other => single("s", other),
    })
}

/// Alias for [`s`].
pub fn sound(pat: impl IntoPattern) -> Pattern {
    s(pat)
}

/// The `mode` control. A `:`-list value (`"below:G4"`, which mini-notation
/// spells as the list `["below", "G4"]`) also sets `anchor`, matching Strudel's
/// `registerControl(['mode', 'anchor'])`.
pub fn mode(pat: impl IntoPattern) -> Pattern {
    pat.into_pattern().fmap(|v| match v {
        Value::Map(_) => v,
        Value::List(ref items) if !items.is_empty() => {
            let mut m = BTreeMap::new();
            m.insert("mode".to_string(), items[0].clone());
            if let Some(anchor) = items.get(1) {
                m.insert("anchor".to_string(), anchor.clone());
            }
            Value::Map(m)
        }
        Value::Str(ref s) if s.contains(':') => {
            let mut parts = s.splitn(2, ':');
            let mut m = BTreeMap::new();
            m.insert(
                "mode".to_string(),
                Value::Str(parts.next().unwrap_or("").to_string()),
            );
            if let Some(anchor) = parts.next() {
                m.insert("anchor".to_string(), Value::Str(anchor.to_string()));
            }
            Value::Map(m)
        }
        other => single("mode", other),
    })
}

impl Pattern {
    /// Set the `s`/`sound` control (with `name:index` splitting).
    pub fn s(&self, x: impl IntoPattern) -> Pattern {
        self.set(s(x))
    }

    /// Set the `mode` control, also setting `anchor` for `"mode:anchor"` values.
    pub fn mode(&self, x: impl IntoPattern) -> Pattern {
        self.set(mode(x))
    }
}
