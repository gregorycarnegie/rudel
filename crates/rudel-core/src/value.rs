// value.rs - dynamic hap value model. Mirrors Strudel's dynamic JS hap values
// plus value.mjs `unionWithObj`.
// Copyright (C) 2022 Strudel contributors; 2026 Rudel contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fraction::Frac;
use crate::pattern::Pattern;
use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A control map value. An [`IndexMap`] (insertion-ordered) rather than a plain
/// sorted map so that key *insertion* order is preserved — mirroring JS object
/// key order, which Strudel relies on (e.g. `modulate`'s "default to the control
/// applied just before" rule reads `Object.keys(v).at(-1)`). Equality is
/// order-independent (`IndexMap`'s `PartialEq` ignores order), and the `Debug`
/// impl renders keys sorted so snapshot output stays deterministic.
pub type ValueMap = IndexMap<String, Value>;

/// A boxed value-transforming function, used as a hap value during the
/// applicative steps (`appLeft`/`appRight`/`appBoth`) of patternification.
pub type ValueFn = Arc<dyn Fn(Value) -> Value + Send + Sync>;

/// A dynamically-typed pattern value.
///
/// Strudel haps carry plain JS values (numbers, strings, control objects, and —
/// transiently — functions and patterns). We model that with an enum. `Map`
/// uses an insertion-ordered [`ValueMap`] to mirror JS object key order, while
/// `Debug` renders sorted for deterministic snapshot output.
#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    /// Integer literal (kept distinct from `F64` so e.g. `n("0 1 2")` snapshots
    /// as integers like Strudel does).
    Int(i64),
    F64(f64),
    Frac(Frac),
    Str(String),
    List(Vec<Value>),
    Map(ValueMap),
    /// A function value (pattern of functions during applicative application).
    Func(ValueFn),
    /// A pattern value (used by `squeezeJoin`, `inhabit`, mini-notation, ...).
    Pat(Box<Pattern>),
}

impl Value {
    pub fn func<F: Fn(Value) -> Value + Send + Sync + 'static>(f: F) -> Value {
        Value::Func(Arc::new(f))
    }

    pub fn is_nothing(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Best-effort numeric coercion (numbers and numeric strings).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::F64(n) => Some(*n),
            Value::Frac(f) => Some(f.to_f64()),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Str(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Coerce to a [`Frac`] for use as a time/parameter value. Non-numeric
    /// values fall back to zero.
    pub fn to_frac(&self) -> Frac {
        match self {
            Value::Frac(f) => *f,
            Value::Int(n) => Frac::int(*n),
            other => Frac::from_f64(other.as_f64().unwrap_or(0.0)),
        }
    }

    /// Truthiness used by `struct`/`mask` and boolean patterns. Mirrors the
    /// values mini-notation produces: `t`/`x`/`true`/non-zero are true,
    /// `f`/`~`/`false`/`0`/empty are false.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::F64(n) => *n != 0.0,
            Value::Frac(f) => *f != Frac::zero(),
            Value::Null => false,
            Value::Str(s) => !matches!(s.as_str(), "f" | "~" | "false" | "0" | ""),
            _ => true,
        }
    }

    /// Apply, assuming this is a `Func`. Panics if it isn't (mirrors JS calling
    /// a non-function, which throws).
    pub fn apply(&self, arg: Value) -> Value {
        match self {
            Value::Func(f) => f(arg),
            other => panic!("Value::apply called on non-function value {other:?}"),
        }
    }

    /// Structural merge of two map values (`value.mjs` `unionWithObj`): keys
    /// present in both are combined with `func`, others are unioned (b wins).
    ///
    /// Mirrors the issue #1026 guard: a single-key `{value: x}` right operand is
    /// a bare scalar wrapped by the compose path, so arithmetic between a control
    /// map and a scalar is refused — `self` is returned unchanged. (Strudel logs
    /// `[warn]: Can't do arithmetic on control pattern.`; rudel-core has no
    /// logger, so the no-op pass-through is the only observable effect.)
    pub fn union_with(&self, other: &Value, func: impl Fn(&Value, &Value) -> Value) -> Value {
        match (self, other) {
            (Value::Map(a), Value::Map(b)) => {
                if b.len() == 1 && b.contains_key("value") {
                    return Value::Map(a.clone());
                }
                let mut out = a.clone();
                for (k, bv) in b {
                    out.entry(k.clone())
                        .and_modify(|av| *av = func(av, bv))
                        .or_insert_with(|| bv.clone());
                }
                Value::Map(out)
            }
            // Non-map values: combine directly (e.g. `n("0").add("1")`).
            _ => func(self, other),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (F64(a), F64(b)) => a == b,
            (Frac(a), Frac(b)) => a == b,
            // numeric cross-type equality (Int vs F64 vs Frac)
            (Int(_), F64(_)) | (F64(_), Int(_)) | (Frac(_), _) | (_, Frac(_)) => {
                matches!((self.as_f64(), other.as_f64()), (Some(x), Some(y)) if x == y)
            }
            (Str(a), Str(b)) => a == b,
            (List(a), List(b)) => a == b,
            (Map(a), Map(b)) => a == b,
            // Functions and patterns are never structurally equal.
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::F64(n) => write!(f, "{n}"),
            Value::Frac(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::List(l) => write!(f, "{l:?}"),
            // Render keys sorted so Debug/snapshot output stays deterministic
            // even though the map preserves insertion order internally.
            Value::Map(m) => {
                let sorted: BTreeMap<&String, &Value> = m.iter().collect();
                write!(f, "{sorted:?}")
            }
            Value::Func(_) => write!(f, "<func>"),
            Value::Pat(_) => write!(f, "<pattern>"),
        }
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}
impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Int(n as i64)
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::F64(n)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}
impl From<Frac> for Value {
    fn from(x: Frac) -> Self {
        Value::Frac(x)
    }
}
