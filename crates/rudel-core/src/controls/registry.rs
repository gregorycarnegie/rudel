use super::aliases::ALIAS_CONTROL_BUILDERS;
use super::named::{NAMED_CONTROL_BUILDERS, fade_time, fx_release, loop_begin, loop_end};
use super::plain::{PLAIN_CONTROL_BUILDERS, bend_range, warp, warpmode, wt, wtphaserand};
use super::special::{mode, s, sound};
use crate::pattern::Pattern;
use crate::value::Value;

/// Control spellings without a same-named Rust builder fn: bespoke controls
/// (`s` splits `name:index`, `mode` also sets `anchor`) and camelCase /
/// keyword-safe aliases that otherwise only exist in the language bindings.
static EXTRA_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
    ("s", |p| s(p)),
    ("sound", |p| sound(p)),
    ("mode", |p| mode(p)),
    ("bendRange", |p| bend_range(p)),
    ("wavetablePosition", |p| wt(p)),
    ("wavetableWarp", |p| warp(p)),
    ("wavetableWarpMode", |p| warpmode(p)),
    ("wavetablePhaseRand", |p| wtphaserand(p)),
    ("fadeOutTime", |p| fade_time(p)),
    ("FXrel", |p| fx_release(p)),
    ("FXr", |p| fx_release(p)),
    ("loopb", |p| loop_begin(p)),
    ("loope", |p| loop_end(p)),
];

/// Every `(name, builder)` control pair: plain controls, aliases,
/// literal-key controls, and binding-layer spellings. Each builder wraps a
/// value pattern into the control's map; the language bindings use this to
/// expose every control as a pattern method without hand-listing names.
pub fn control_builders() -> impl Iterator<Item = (&'static str, fn(Pattern) -> Pattern)> {
    PLAIN_CONTROL_BUILDERS
        .iter()
        .chain(ALIAS_CONTROL_BUILDERS)
        .chain(NAMED_CONTROL_BUILDERS)
        .chain(EXTRA_CONTROL_BUILDERS)
        .copied()
}

/// `(name, canonical key)` pairs for the numbered FM controls, mirroring
/// Strudel's `registerMultiControl` loops: per-operator families
/// (`fmh1`-`fmh8`, `fmattack1`-`fmattack8`, short spellings like `fmatt3`)
/// and the `fmi{from}{to}` routing matrix with its `fm{from}{to}` aliases
/// (target 0 is the carrier). `{name}1` resolves to the bare control.
///
/// These names are generated rather than declared, so they have no dedicated
/// Rust builder fns (use `ctrl(name, value)` from Rust); the language
/// bindings register them as pattern methods alongside [`control_builders`].
pub fn numbered_control_names() -> Vec<(String, String)> {
    let families: &[(&str, Option<&str>)] = &[
        ("fmh", None),
        ("fmi", None),
        ("fmwave", None),
        ("fmenv", Some("fme")),
        ("fmattack", Some("fmatt")),
        ("fmdecay", Some("fmdec")),
        ("fmsustain", Some("fmsus")),
        ("fmrelease", Some("fmrel")),
    ];
    let mut names = Vec::new();
    for &(family, short) in families {
        for op in 1..=8 {
            let key = if op == 1 {
                family.to_string()
            } else {
                format!("{family}{op}")
            };
            names.push((format!("{family}{op}"), key.clone()));
            if let Some(short) = short {
                names.push((format!("{short}{op}"), key));
            }
        }
    }
    // `fm` ~ `fmi`: `fm1` is the bare `fm`, `fmN` aliases the chain `fmiN`.
    for op in 1..=8 {
        let key = if op == 1 {
            "fm".to_string()
        } else {
            format!("fmi{op}")
        };
        names.push((format!("fm{op}"), key));
    }
    for from in 0..=8 {
        for to in 0..=8 {
            let key = format!("fmi{from}{to}");
            names.push((key.clone(), key.clone()));
            names.push((format!("fm{from}{to}"), key));
        }
    }
    names
}

/// Resolve a control or alias name to the canonical key it writes, mirroring
/// Strudel's `getControlName`. Unknown names resolve to themselves.
pub fn control_name(name: &str) -> String {
    // Probe the builder with a scalar and read back the key it writes. This
    // keeps the alias -> key mapping in one place (the registries above)
    // instead of a second hand-maintained table that could drift.
    if let Some((_, f)) = control_builders().find(|(n, _)| *n == name) {
        let probe = f(crate::pure(Value::Int(0)));
        if let Some(hap) = probe
            .query_arc(crate::Frac::zero(), crate::Frac::one())
            .first()
        {
            if let Value::Map(m) = &hap.value {
                if let Some(k) = m.keys().next() {
                    return k.clone();
                }
            }
        }
    }
    if let Some((_, key)) = numbered_control_names()
        .into_iter()
        .find(|(n, _)| n == name)
    {
        return key;
    }
    name.to_string()
}
