// controls.rs - control parameters (note, s, gain, pan, ...).
// Mirrors strudel/packages/core/controls.mjs: each control wraps values into a
// single-key map; as a method it merges that key into the pattern.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

fn single(name: &str, v: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(name.to_string(), v);
    Value::Map(m)
}

/// Wrap each value of `pat` into `{ name: value }`. If a value is already a
/// map it is left untouched (it already carries its keys).
fn control(name: &'static str, pat: Pattern) -> Pattern {
    pat.fmap(move |v| match v {
        Value::Map(_) => v,
        other => single(name, other),
    })
}

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
            if let Some(idx) = parts.next()
                && let Ok(n) = idx.parse::<i64>()
            {
                m.insert("n".to_string(), Value::Int(n));
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

macro_rules! controls {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($name), "` control.")]
            pub fn $name(pat: impl IntoPattern) -> Pattern {
                control(stringify!($name), pat.into_pattern())
            }
        )*

        impl Pattern {
            $(
                #[doc = concat!("Set the `", stringify!($name), "` control, keeping this pattern's structure.")]
                pub fn $name(&self, x: impl IntoPattern) -> Pattern {
                    self.set($name(x))
                }
            )*

            /// Set the `s`/`sound` control (with `name:index` splitting).
            pub fn s(&self, x: impl IntoPattern) -> Pattern {
                self.set(s(x))
            }
        }
    };
}

controls!(
    note,
    n,
    gain,
    pan,
    speed,
    room,
    size,
    shape,
    crush,
    cutoff,
    resonance,
    delay,
    delaytime,
    delayfeedback,
    attack,
    decay,
    sustain,
    release,
    vowel,
    accelerate,
    coarse,
    orbit,
    velocity,
    begin,
    end,
    legato,
    clip,
);

// A few common aliases.
/// Alias for [`cutoff`] (low-pass filter frequency).
pub fn lpf(pat: impl IntoPattern) -> Pattern {
    cutoff(pat)
}
/// Alias for [`resonance`].
pub fn lpq(pat: impl IntoPattern) -> Pattern {
    resonance(pat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seq;

    #[test]
    fn note_wraps_into_map() {
        let pat = note(seq([0, 4, 7]));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => assert_eq!(m.get("note"), Some(&Value::Int(0))),
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn s_splits_sample_index() {
        let pat = s("bd:3".into_pattern());
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("s"), Some(&Value::Str("bd".to_string())));
                assert_eq!(m.get("n"), Some(&Value::Int(3)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn gain_method_merges_key() {
        // note(...).gain(0.5) -> { note, gain }
        let pat = note(seq([0, 1])).gain(0.5);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert!(m.contains_key("note"));
                assert_eq!(m.get("gain"), Some(&Value::F64(0.5)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }
}
