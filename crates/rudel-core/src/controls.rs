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
    postgain,
    pan,
    speed,
    room,
    size,
    shape,
    crush,
    cutoff,
    resonance,
    hcutoff,
    hresonance,
    bandf,
    bandq,
    // filter envelopes
    lpenv,
    lpattack,
    lpdecay,
    lpsustain,
    lprelease,
    hpenv,
    hpattack,
    hpdecay,
    hpsustain,
    hprelease,
    bpenv,
    bpattack,
    bpdecay,
    bpsustain,
    bprelease,
    fanchor,
    delay,
    delaytime,
    delayfeedback,
    attack,
    decay,
    sustain,
    release,
    vowel,
    bank,
    cut,
    accelerate,
    coarse,
    orbit,
    velocity,
    begin,
    end,
    legato,
    clip,
    unit,
    // synth: supersaw + FM + ADSR shortcuts
    unison,
    detune,
    spread,
    fm,
    fmh,
    fmi,
    fmwave,
    fmattack,
    fmdecay,
    fmsustain,
    fmrelease,
    // FM operator 2 (chain `op2 -> op1`); higher operators / arbitrary `fmiIJ`
    // edges go through the generic `ctrl(name, value)` method.
    fmi2,
    fmh2,
    fmwave2,
    fmattack2,
    fmdecay2,
    fmsustain2,
    fmrelease2,
    pw,
    noise,
    pcurve,
    adsr,
    ad,
    ar,
    hold,
    // vibrato + pitch envelope
    vib,
    vibmod,
    penv,
    pattack,
    pdecay,
    psustain,
    prelease,
    panchor,
    // post-fx: tremolo + phaser
    tremolo,
    tremolodepth,
    phaser,
    phaserrate,
    phaserdepth,
    phasercenter,
    phasersweep,
    // tonal / voicing controls
    mtranspose,
    ctranspose,
    dictionary,
    anchor,
    offset,
    octaves,
);

// Common aliases (Strudel exposes these via `registerControl(names, ...aliases)`).
macro_rules! control_aliases {
    ($($alias:ident => $target:ident),* $(,)?) => {
        $(
            #[doc = concat!("Alias for [`", stringify!($target), "`].")]
            pub fn $alias(pat: impl IntoPattern) -> Pattern {
                $target(pat)
            }
        )*
        impl Pattern {
            $(
                #[doc = concat!("Alias for [`", stringify!($target), "`](Self::", stringify!($target), ").")]
                pub fn $alias(&self, x: impl IntoPattern) -> Pattern {
                    self.$target(x)
                }
            )*
        }
    };
}

control_aliases!(
    lpf => cutoff,
    lp => cutoff,
    ctf => cutoff,
    lpq => resonance,
    hpf => hcutoff,
    hp => hcutoff,
    hpq => hresonance,
    bpf => bandf,
    bp => bandf,
    bpq => bandq,
    vel => velocity,
    att => attack,
    rel => release,
    sus => sustain,
    dec => decay,
    delayt => delaytime,
    delayfb => delayfeedback,
    o => orbit,
    // filter-envelope aliases
    lpe => lpenv,
    lpa => lpattack,
    lpd => lpdecay,
    lps => lpsustain,
    lpr => lprelease,
    hpe => hpenv,
    hpa => hpattack,
    hpd => hpdecay,
    hps => hpsustain,
    hpr => hprelease,
    bpe => bpenv,
    bpa => bpattack,
    bpd => bpdecay,
    bps => bpsustain,
    bpr => bprelease,
    // vibrato + pitch-envelope aliases
    vibrato => vib,
    vmod => vibmod,
    patt => pattack,
    pdec => pdecay,
    psus => psustain,
    prel => prelease,
    // voicing dictionary alias
    dict => dictionary,
);

// Sample-loop controls. The Strudel keys are `loop`/`loopBegin`/`loopEnd`, but
// `loop` is a Rust keyword, so the builder fns are named `loop_play`/
// `loop_begin`/`loop_end` while still writing the Strudel control keys.
macro_rules! loop_controls {
    ($($fn:ident => $key:literal),* $(,)?) => {
        $(
            #[doc = concat!("The `", $key, "` control.")]
            pub fn $fn(pat: impl IntoPattern) -> Pattern {
                control($key, pat.into_pattern())
            }
        )*
        impl Pattern {
            $(
                #[doc = concat!("Set the `", $key, "` control, keeping this pattern's structure.")]
                pub fn $fn(&self, x: impl IntoPattern) -> Pattern {
                    self.set($fn(x))
                }
            )*
        }
    };
}

loop_controls!(
    loop_play => "loop",
    loop_begin => "loopBegin",
    loop_end => "loopEnd",
);

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
    /// Set the `mode` control, also setting `anchor` for `"mode:anchor"` values.
    pub fn mode(&self, x: impl IntoPattern) -> Pattern {
        self.set(mode(x))
    }

    /// Set an arbitrary named control, keeping this pattern's structure. The
    /// escape hatch for controls without a dedicated method (e.g. FM-matrix
    /// edges `ctrl("fmi20", 3)` or higher operators `ctrl("fmh3", 2)`).
    pub fn ctrl(&self, name: impl Into<String>, x: impl IntoPattern) -> Pattern {
        self.set(control_dyn(name, x))
    }
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
    fn s_preserves_non_numeric_tail() {
        // `s("name:tail")` keeps a non-numeric tail as a string `n`.
        let pat = s("bd:foo".into_pattern());
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("s"), Some(&Value::Str("bd".to_string())));
                assert_eq!(m.get("n"), Some(&Value::Str("foo".to_string())));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn mode_splits_into_mode_and_anchor() {
        // `mode("below:G4")` (a `:`-list) sets both `mode` and `anchor`.
        let pat = mode(Value::List(vec![
            Value::Str("below".into()),
            Value::Str("G4".into()),
        ]));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("mode"), Some(&Value::Str("below".to_string())));
                assert_eq!(m.get("anchor"), Some(&Value::Str("G4".to_string())));
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
