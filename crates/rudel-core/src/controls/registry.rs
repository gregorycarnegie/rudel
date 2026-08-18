use super::{
    aliases::{ALIAS_CONTROL_BUILDERS, ALIAS_CONTROL_KEYS},
    multi::{distort, label, shape, transient, vib},
    named::{NAMED_CONTROL_BUILDERS, fade_time, fx_release, loop_begin, loop_end},
    plain::{PLAIN_CONTROL_BUILDERS, bend_range, warp, warpmode, wt, wtphaserand},
    special::{mode, s, sound},
};
use crate::pattern::Pattern;

type ControlBuilder = fn(Pattern) -> Pattern;
type ControlBuilderEntry = (&'static str, ControlBuilder);

/// Control spellings without a same-named Rust builder fn: bespoke controls
/// (`s` splits `name:index`, `mode` also sets `anchor`) and camelCase /
/// keyword-safe aliases that otherwise only exist in the language bindings.
/// One row per spelling: `(spelling, canonical key, builder)`.
static EXTRA_CONTROLS: &[(&str, &str, ControlBuilder)] = &[
    ("s", "s", |p| s(p)),
    ("sound", "s", |p| sound(p)),
    ("mode", "mode", |p| mode(p)),
    ("distort", "distort", |p| distort(p)),
    ("shape", "shape", |p| shape(p)),
    ("transient", "transient", |p| transient(p)),
    ("vib", "vib", |p| vib(p)),
    ("vibrato", "vib", |p| vib(p)),
    ("v", "vib", |p| vib(p)),
    ("label", "label", |p| label(p)),
    ("bendRange", "bendRange", |p| bend_range(p)),
    ("wavetablePosition", "wt", |p| wt(p)),
    ("wavetableWarp", "warp", |p| warp(p)),
    ("wavetableWarpMode", "warpmode", |p| warpmode(p)),
    ("wavetablePhaseRand", "wtphaserand", |p| wtphaserand(p)),
    ("fadeOutTime", "fadeTime", |p| fade_time(p)),
    ("FXrel", "FXrelease", |p| fx_release(p)),
    ("FXr", "FXrelease", |p| fx_release(p)),
    ("loopb", "loopBegin", |p| loop_begin(p)),
    ("loope", "loopEnd", |p| loop_end(p)),
];

/// Every `(name, builder)` control pair: plain controls, aliases,
/// literal-key controls, and binding-layer spellings. Each builder wraps a
/// value pattern into the control's map; the language bindings use this to
/// expose every control as a pattern method without hand-listing names.
pub fn control_builders() -> impl Iterator<Item = ControlBuilderEntry> {
    PLAIN_CONTROL_BUILDERS
        .iter()
        .chain(ALIAS_CONTROL_BUILDERS)
        .chain(NAMED_CONTROL_BUILDERS)
        .copied()
        .chain(
            EXTRA_CONTROLS
                .iter()
                .map(|&(name, _, builder)| (name, builder)),
        )
}

fn builder_key(name: &'static str) -> &'static str {
    match name {
        "byte_beat_expression" => "byteBeatExpression",
        "byte_beat_start_time" => "byteBeatStartTime",
        "fx_release" => "FXrelease",
        _ => name,
    }
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
    if PLAIN_CONTROL_BUILDERS.iter().any(|(n, _)| *n == name) {
        return name.to_string();
    }
    if let Some((_, key)) = ALIAS_CONTROL_KEYS.iter().find(|(n, _)| *n == name) {
        return builder_key(key).to_string();
    }
    if let Some((key, _)) = NAMED_CONTROL_BUILDERS.iter().find(|(n, _)| *n == name) {
        return (*key).to_string();
    }
    if let Some((_, key, _)) = EXTRA_CONTROLS.iter().find(|(n, _, _)| *n == name) {
        return (*key).to_string();
    }
    if let Some((_, key)) = numbered_control_names()
        .into_iter()
        .find(|(n, _)| n == name)
    {
        return key;
    }
    name.to_string()
}
