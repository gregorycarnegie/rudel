use super::base::{spread_control, value_parts};
use super::registry::control_name;
use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::{Value, ValueMap};

/// Strudel's `distort` multi-control: a `:`-list (`"3:0.5:diode"`) spreads into
/// `distort`/`distortvol`/`distorttype` (waveshape amount, postgain, algorithm).
/// A bare number sets only the amount.
pub fn distort(pat: impl IntoPattern) -> Pattern {
    spread_control(
        &["distort", "distortvol", "distorttype"],
        pat.into_pattern(),
    )
}

/// Build a distortion-algorithm shortcut value: each arg becomes
/// `[amount, volume, algName]` (Strudel's `_distortWithAlg`), spread into the
/// `distort`/`distortvol`/`distorttype` controls. A list arg has the algorithm
/// name appended; a bare amount defaults `volume = 1`.
fn distort_alg(name: &'static str, pat: Pattern) -> Pattern {
    let mapped = pat.fmap(move |v| match v {
        Value::List(mut items) => {
            items.push(Value::Str(name.to_string()));
            Value::List(items)
        }
        other => Value::List(vec![other, Value::Int(1), Value::Str(name.to_string())]),
    });
    spread_control(&["distort", "distortvol", "distorttype"], mapped)
}

/// Generate the waveshaping-distortion shortcuts (`soft`/`hard`/…). Each sets
/// `distorttype` to its own algorithm and the `distort`/`distortvol` amount and
/// postgain from the argument, matching superdough's distortion family.
macro_rules! distort_shortcuts {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($name), "` waveshaping-distortion shortcut.")]
            pub fn $name(pat: impl IntoPattern) -> Pattern {
                distort_alg(stringify!($name), pat.into_pattern())
            }
        )*
        impl Pattern {
            $(
                #[doc = concat!("Apply `", stringify!($name), "` waveshaping distortion (sets `distorttype`).")]
                pub fn $name(&self, x: impl IntoPattern) -> Pattern {
                    self.set($name(x))
                }
            )*
        }
    };
}

distort_shortcuts!(soft, hard, cubic, diode, asym, fold, sinefold, chebyshev);

impl Pattern {
    /// Set the `distort` multi-control (see [`distort`]).
    pub fn distort(&self, x: impl IntoPattern) -> Pattern {
        self.set(distort(x))
    }
}

/// Strudel's `adsr` helper: a `:`-list (`".1:.2:.5:.3"`) expands into
/// `attack`/`decay`/`sustain`/`release`. Missing entries are left unset.
pub fn adsr(pat: impl IntoPattern) -> Pattern {
    spread_control(
        &["attack", "decay", "sustain", "release"],
        pat.into_pattern(),
    )
}

/// Strudel's `ad` helper: `attack:decay`, with `decay` defaulting to the
/// attack time.
pub fn ad(pat: impl IntoPattern) -> Pattern {
    pat.into_pattern().fmap(|v| match v {
        Value::Map(_) => v,
        other => {
            let parts = value_parts(&other);
            let attack = parts.first().cloned().unwrap_or(Value::Int(0));
            let decay = parts.get(1).cloned().unwrap_or_else(|| attack.clone());
            let mut m = ValueMap::new();
            m.insert("attack".to_string(), attack);
            m.insert("decay".to_string(), decay);
            Value::Map(m)
        }
    })
}

/// Strudel's `ds` helper: `decay:sustain`, with `sustain` defaulting to 0.
pub fn ds(pat: impl IntoPattern) -> Pattern {
    pat.into_pattern().fmap(|v| match v {
        Value::Map(_) => v,
        other => {
            let parts = value_parts(&other);
            let decay = parts.first().cloned().unwrap_or(Value::Int(0));
            let sustain = parts.get(1).cloned().unwrap_or(Value::Int(0));
            let mut m = ValueMap::new();
            m.insert("decay".to_string(), decay);
            m.insert("sustain".to_string(), sustain);
            Value::Map(m)
        }
    })
}

/// Strudel's `ar` helper: `attack:release`, with `release` defaulting to the
/// attack time.
pub fn ar(pat: impl IntoPattern) -> Pattern {
    pat.into_pattern().fmap(|v| match v {
        Value::Map(_) => v,
        other => {
            let parts = value_parts(&other);
            let attack = parts.first().cloned().unwrap_or(Value::Int(0));
            let release = parts.get(1).cloned().unwrap_or_else(|| attack.clone());
            let mut m = ValueMap::new();
            m.insert("attack".to_string(), attack);
            m.insert("release".to_string(), release);
            Value::Map(m)
        }
    })
}

impl Pattern {
    /// Strudel's `adsr` envelope helper (see [`adsr`]).
    pub fn adsr(&self, x: impl IntoPattern) -> Pattern {
        self.set(adsr(x))
    }

    /// Strudel's `ad` envelope helper (see [`ad`]).
    pub fn ad(&self, x: impl IntoPattern) -> Pattern {
        self.set(ad(x))
    }

    /// Strudel's `ds` envelope helper (see [`ds`]).
    pub fn ds(&self, x: impl IntoPattern) -> Pattern {
        self.set(ds(x))
    }

    /// Strudel's `ar` envelope helper (see [`ar`]).
    pub fn ar(&self, x: impl IntoPattern) -> Pattern {
        self.set(ar(x))
    }

    /// Strudel's `control([ccn, ccv])` MIDI helper: a `:`-list sets the MIDI
    /// control number and value together.
    pub fn control(&self, x: impl IntoPattern) -> Pattern {
        self.set(spread_control(&["ccn", "ccv"], x.into_pattern()))
    }

    /// Strudel's `sysex([id, data])` MIDI helper: a `:`-list sets the sysex
    /// id and data together.
    pub fn sysex(&self, x: impl IntoPattern) -> Pattern {
        self.set(spread_control(&["sysexid", "sysexdata"], x.into_pattern()))
    }

    /// Strudel's `as(mapping)`: map bare positional values into named
    /// controls, e.g. `pat("c:.5").as_controls(&["note", "clip"])`. Alias
    /// names resolve through [`control_name`].
    pub fn as_controls(&self, names: &[&str]) -> Pattern {
        let keys: Vec<String> = names.iter().map(|n| control_name(n)).collect();
        self.fmap(move |v| {
            let mut m = ValueMap::new();
            for (key, val) in keys.iter().zip(value_parts(&v)) {
                m.insert(key.clone(), val);
            }
            Value::Map(m)
        })
    }

    /// Strudel's `scrub(positions)`: scrub through a sample like a tape loop.
    /// Structure comes from the positions pattern; a `:`-list (`"0.5:2"`)
    /// also scales playback speed. Events are clipped to their span.
    pub fn scrub(&self, positions: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        positions.into_pattern().outer_bind(move |v| {
            let parts = value_parts(&v);
            let begin_v = parts.first().cloned().unwrap_or(Value::Int(0));
            let speed_mul = parts.get(1).and_then(Value::as_f64).unwrap_or(1.0);
            pat.begin(begin_v).fmap(move |v| match v {
                Value::Map(mut m) => {
                    let speed = m.get("speed").and_then(Value::as_f64).unwrap_or(1.0);
                    m.insert("speed".to_string(), Value::F64(speed * speed_mul));
                    m.insert("clip".to_string(), Value::Int(1));
                    Value::Map(m)
                }
                other => other,
            })
        })
    }
}
