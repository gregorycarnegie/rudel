// controls.rs - control parameters (note, s, gain, pan, ...).
// Mirrors strudel/packages/core/controls.mjs: each control wraps values into a
// single-key map; as a method it merges that key into the pattern.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::Value;
use crate::xen::freq_to_midi;
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

        /// `(name, builder)` pairs for the plain controls above; used by
        /// [`control_name`] to resolve canonical control keys.
        static PLAIN_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( (stringify!($name), |p: Pattern| $name(p)) ),*
        ];
    };
}

controls!(
    i,
    freq,
    mpe,
    note,
    n,
    gain,
    postgain,
    pan,
    speed,
    room,
    roomlp,
    roomdim,
    roomfade,
    size,
    shape,
    crush,
    cutoff,
    resonance,
    hcutoff,
    hresonance,
    bandf,
    bandq,
    ftype,
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
    dry,
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
    pw,
    noise,
    pcurve,
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
    // OSC routing (read by the OSC back-end to pick a destination)
    oschost,
    oscport,
    // tonal / voicing controls
    mtranspose,
    ctranspose,
    dictionary,
    anchor,
    offset,
    octaves,
    // wavetable position + envelope
    wt,
    wtenv,
    wtattack,
    wtdecay,
    wtsustain,
    wtrelease,
    wtrate,
    wtsync,
    wtdepth,
    wtshape,
    wtdc,
    wtskew,
    wtphaserand,
    // wavetable warp + envelope
    warp,
    warpenv,
    warpattack,
    warpdecay,
    warpsustain,
    warprelease,
    warprate,
    warpsync,
    warpdepth,
    warpshape,
    warpdc,
    warpskew,
    warpmode,
    // sound / amplitude / sample-window extras
    source,
    amp,
    stretch,
    duration,
    gate,
    // filter LFO modulation
    lprate,
    lpsync,
    lpdepth,
    lpdepthfrequency,
    lpshape,
    lpdc,
    lpskew,
    bprate,
    bpsync,
    bpdepth,
    bpdepthfrequency,
    bpshape,
    bpdc,
    bpskew,
    hprate,
    hpsync,
    hpdepth,
    hpdepthfrequency,
    hpshape,
    hpdc,
    hpskew,
    // delay extras + DJ filter
    delayspeed,
    delaysync,
    djf,
    lock,
    // tremolo extras
    tremolosync,
    tremoloskew,
    tremolophase,
    tremoloshape,
    // fx: chorus / drive / ducking / channels / pulse-width LFO / leslie
    chorus,
    drive,
    duckorbit,
    duckdepth,
    duckonset,
    duckattack,
    channels,
    channel,
    pwrate,
    pwsweep,
    leslie,
    lrate,
    lsize,
    // tonal / spatial extras
    degree,
    harmonic,
    nudge,
    octave,
    bus,
    busgain,
    overgain,
    overshape,
    panspan,
    pansplay,
    panwidth,
    panorient,
    slide,
    semitone,
    voice,
    // impulse-response reverb + distortion + compressor
    ir,
    irspeed,
    irbegin,
    distort,
    distortvol,
    distorttype,
    compressor,
    // SuperDirt / SuperDough misc
    analyze,
    fft,
    squiz,
    waveloss,
    density,
    expression,
    sustainpedal,
    fshift,
    fshiftnote,
    fshiftphase,
    triode,
    krush,
    kcutoff,
    octer,
    octersub,
    octersubsub,
    ring,
    ringf,
    ringdf,
    freeze,
    xsdelay,
    tsdelay,
    real,
    imag,
    enhance,
    comb,
    smear,
    scram,
    binshift,
    hbrick,
    lbrick,
    frames,
    hours,
    minutes,
    seconds,
    uid,
    val,
    // ZZFX
    zrand,
    curve,
    znoise,
    zmod,
    zcrush,
    zdelay,
    zzfx,
    // visuals / event metadata
    color,
    transient,
    // FM envelope ramp type
    fmenv,
    // MIDI controls
    midichan,
    midimap,
    midiport,
    midicmd,
    ccn,
    ccv,
    nrpnn,
    nrpv,
    sysexid,
    sysexdata,
    midibend,
    miditouch,
);

/// The `bendRange` control. The Rust function is snake_case while the emitted
/// control key matches Strudel's camelCase spelling.
pub fn bend_range(pat: impl IntoPattern) -> Pattern {
    control("bendRange", pat.into_pattern())
}

impl Pattern {
    /// Set the `bendRange` control, keeping this pattern's structure.
    pub fn bend_range(&self, x: impl IntoPattern) -> Pattern {
        self.set(bend_range(x))
    }

    /// Wrap this pattern's current values into a control map.
    pub fn wrap_control(&self, name: impl Into<String>) -> Pattern {
        wrap_control_dyn(name, self.clone())
    }
}

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

        /// `(alias, builder)` pairs for the aliases above; used by
        /// [`control_name`] to resolve canonical control keys.
        static ALIAS_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( (stringify!($alias), |p: Pattern| $target(p)) ),*
        ];
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
    rlp => roomlp,
    rdim => roomdim,
    rfade => roomfade,
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
    // sound / amplitude / sample-window aliases
    src => source,
    dur => duration,
    gat => gate,
    // synth aliases
    det => detune,
    fme => fmenv,
    fmatt => fmattack,
    fmdec => fmdecay,
    fmsus => fmsustain,
    fmrel => fmrelease,
    // wavetable / warp envelope aliases
    wtatt => wtattack,
    wtdec => wtdecay,
    wtsus => wtsustain,
    wtrel => wtrelease,
    warpatt => warpattack,
    warpdec => warpdecay,
    warpsus => warpsustain,
    warprel => warprelease,
    // delay aliases
    dfb => delayfeedback,
    dt => delaytime,
    delays => delaysync,
    // tremolo aliases
    trem => tremolo,
    tremdepth => tremolodepth,
    tremskew => tremoloskew,
    tremphase => tremolophase,
    tremshape => tremoloshape,
    // phaser aliases
    ph => phaserrate,
    phs => phasersweep,
    phc => phasercenter,
    phd => phaserdepth,
    phasdp => phaserdepth,
    // ducking aliases (Strudel's canonical key is `duckorbit`)
    duck => duckorbit,
    duckons => duckonset,
    duckatt => duckattack,
    datt => duckattack,
    // channel / pulse-width aliases
    ch => channels,
    pwr => pwrate,
    pws => pwsweep,
    // tonal / spatial aliases
    oct => octave,
    bgain => busgain,
    // reverb / distortion aliases (Rudel's canonical reverb-size key is `size`)
    iresponse => ir,
    roomsize => size,
    sz => size,
    rsize => size,
    dist => distort,
    distvol => distortvol,
    disttype => distorttype,
    // filter LFO aliases
    lpdepthfreq => lpdepthfrequency,
    bpdepthfreq => bpdepthfrequency,
    hpdepthfreq => hpdepthfrequency,
    // vibrato / color aliases
    v => vib,
    colour => color,
    // byte-beat / FX-release aliases
    bbexpr => byte_beat_expression,
    bb => byte_beat_expression,
    bbst => byte_beat_start_time,
    fxr => fx_release,
);

// Controls whose Strudel key can't be a Rust fn name (keywords like `loop`,
// camelCase keys like `loopBegin`). The builder fn is snake_case while still
// writing the Strudel control key.
macro_rules! named_controls {
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

        /// `(key, builder)` pairs for the controls above; used by
        /// [`control_name`] to resolve canonical control keys.
        static NAMED_CONTROL_BUILDERS: &[(&str, fn(Pattern) -> Pattern)] = &[
            $( ($key, |p: Pattern| $fn(p)) ),*
        ];
    };
}

named_controls!(
    loop_play => "loop",
    loop_begin => "loopBegin",
    loop_end => "loopEnd",
    steps_per_octave => "stepsPerOctave",
    octave_r => "octaveR",
    ctl_num => "ctlNum",
    prog_num => "progNum",
    poly_touch => "polyTouch",
    compressor_knee => "compressorKnee",
    compressor_ratio => "compressorRatio",
    compressor_attack => "compressorAttack",
    compressor_release => "compressorRelease",
    frame_rate => "frameRate",
    song_ptr => "songPtr",
    delta_slide => "deltaSlide",
    pitch_jump => "pitchJump",
    pitch_jump_time => "pitchJumpTime",
    fade_time => "fadeTime",
    fade_in_time => "fadeInTime",
    byte_beat_expression => "byteBeatExpression",
    byte_beat_start_time => "byteBeatStartTime",
    fx_release => "FXrelease",
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
    /// escape hatch for controls without a dedicated method.
    pub fn ctrl(&self, name: impl Into<String>, x: impl IntoPattern) -> Pattern {
        self.set(control_dyn(name, x))
    }

    /// Strudel's `piano()` convenience: select the piano sample bank, set a
    /// short release and default clip, then spread notes gently by pitch.
    pub fn piano(&self) -> Pattern {
        self.s("piano").release(0.1).fmap(|v| match v {
            Value::Map(mut m) => {
                let pan = piano_pan(&m);
                m.entry("clip".to_string()).or_insert(Value::Int(1));
                if let Some(pan) = pan {
                    let existing = m.get("pan").and_then(Value::as_f64).unwrap_or(1.0);
                    m.insert("pan".to_string(), Value::F64(existing * pan));
                }
                Value::Map(m)
            }
            other => other,
        })
    }
}

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

/// View a value as positional parts: a list yields its items, anything else
/// is a single part. Mini-notation `a:b:c` values arrive as lists.
fn value_parts(v: &Value) -> Vec<Value> {
    match v {
        Value::List(items) => items.clone(),
        other => vec![other.clone()],
    }
}

/// Wrap positional values into the given control keys: `[x, y]` becomes
/// `{ names[0]: x, names[1]: y }`. Extra parts are dropped, missing parts
/// leave their key unset. Powers Strudel's multi-control helpers.
fn spread_control(names: &'static [&'static str], pat: Pattern) -> Pattern {
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
            let mut m = BTreeMap::new();
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
            let mut m = BTreeMap::new();
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
            let mut m = BTreeMap::new();
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
            let mut m = BTreeMap::new();
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

fn piano_pan(m: &BTreeMap<String, Value>) -> Option<f64> {
    let midi = m
        .get("note")
        .and_then(value_to_midi)
        .or_else(|| m.get("freq").and_then(|v| v.as_f64().map(freq_to_midi)))?;
    let max_pan = crate::tonal::note_to_midi("C8")? as f64;
    let pitch_pan = (midi.round() / max_pan).clamp(0.0, 1.0);
    Some(pitch_pan * 0.5 + 0.25)
}

fn value_to_midi(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => crate::tonal::note_to_midi(s).map(|m| m as f64),
        other => other.as_f64(),
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
    fn alias_controls_write_canonical_keys() {
        // Aliases canonicalize like Strudel's `getControlName`: `ph` writes
        // `phaserrate`, `duck` writes `duckorbit`, `v` writes `vib`.
        let pat = note(seq([0])).ph(2).duck(0.5).v(4);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("phaserrate"), Some(&Value::Int(2)));
                assert_eq!(m.get("duckorbit"), Some(&Value::F64(0.5)));
                assert_eq!(m.get("vib"), Some(&Value::Int(4)));
                assert!(!m.contains_key("ph"));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn named_controls_write_literal_keys() {
        // Snake-case builder fns write Strudel's camelCase keys.
        let pat = note(seq([0])).compressor_knee(30).fx_release(0.2);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("compressorKnee"), Some(&Value::Int(30)));
                assert_eq!(m.get("FXrelease"), Some(&Value::F64(0.2)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn roomsize_aliases_map_to_size() {
        // Rudel's canonical reverb-size key is `size`; Strudel's `roomsize`,
        // `sz`, and `rsize` all land there.
        let pat = note(seq([0])).roomsize(0.8);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => assert_eq!(m.get("size"), Some(&Value::F64(0.8))),
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn adsr_expands_into_envelope_keys() {
        // `adsr(".1:.2:.5:.3")` (a `:`-list) expands into the four envelope
        // controls, like Strudel's multi-control helper.
        let pat = adsr(Value::List(vec![
            Value::F64(0.1),
            Value::F64(0.2),
            Value::F64(0.5),
            Value::F64(0.3),
        ]));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("attack"), Some(&Value::F64(0.1)));
                assert_eq!(m.get("decay"), Some(&Value::F64(0.2)));
                assert_eq!(m.get("sustain"), Some(&Value::F64(0.5)));
                assert_eq!(m.get("release"), Some(&Value::F64(0.3)));
            }
            other => panic!("expected map, got {other:?}"),
        }
        // a scalar only sets `attack`
        let pat = adsr(Value::F64(0.1));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("attack"), Some(&Value::F64(0.1)));
                assert!(!m.contains_key("decay"));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn envelope_helper_defaults_match_strudel() {
        // `ad(x)`: decay defaults to attack; `ar(x)`: release defaults to
        // attack; `ds(x)`: sustain defaults to 0.
        let first = &ad(Value::F64(0.2)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("attack"), Some(&Value::F64(0.2)));
                assert_eq!(m.get("decay"), Some(&Value::F64(0.2)));
            }
            other => panic!("expected map, got {other:?}"),
        }
        let first = &ds(Value::F64(0.3)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("decay"), Some(&Value::F64(0.3)));
                assert_eq!(m.get("sustain"), Some(&Value::Int(0)));
            }
            other => panic!("expected map, got {other:?}"),
        }
        let first = &ar(Value::F64(0.4)).query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("attack"), Some(&Value::F64(0.4)));
                assert_eq!(m.get("release"), Some(&Value::F64(0.4)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn control_and_sysex_spread_pairs() {
        let pat = note(seq([0])).control(Value::List(vec![Value::Int(74), Value::Int(64)]));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("ccn"), Some(&Value::Int(74)));
                assert_eq!(m.get("ccv"), Some(&Value::Int(64)));
            }
            other => panic!("expected map, got {other:?}"),
        }
        let pat = note(seq([0])).sysex(Value::List(vec![Value::Int(7), Value::Int(1)]));
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("sysexid"), Some(&Value::Int(7)));
                assert_eq!(m.get("sysexdata"), Some(&Value::Int(1)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn control_name_resolves_aliases() {
        // mirrors Strudel's getControlName: aliases resolve to the canonical
        // key they write, unknown names resolve to themselves.
        assert_eq!(control_name("lpf"), "cutoff");
        assert_eq!(control_name("bb"), "byteBeatExpression");
        assert_eq!(control_name("fm23"), "fmi23");
        assert_eq!(control_name("vel"), "velocity");
        assert_eq!(control_name("sound"), "s");
        assert_eq!(control_name("loopb"), "loopBegin");
        assert_eq!(control_name("note"), "note");
        assert_eq!(control_name("not_a_control"), "not_a_control");
    }

    #[test]
    fn as_controls_maps_positional_values() {
        // `"c:.5".as("note:clip")`: list values map positionally, with alias
        // names canonicalized (vel -> velocity).
        let pat = crate::pure(Value::List(vec![Value::Str("c".into()), Value::F64(0.5)]))
            .as_controls(&["note", "vel"]);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("note"), Some(&Value::Str("c".into())));
                assert_eq!(m.get("velocity"), Some(&Value::F64(0.5)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn scrub_sets_begin_speed_and_clip() {
        // scrub("0.5:2"): structure from the positions pattern; begin set,
        // speed multiplied, clip forced to 1.
        let positions = crate::pure(Value::List(vec![Value::F64(0.5), Value::Int(2)]));
        let pat = s("amen".into_pattern()).speed(0.5).scrub(positions);
        let first = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
        match &first.value {
            Value::Map(m) => {
                assert_eq!(m.get("begin"), Some(&Value::F64(0.5)));
                assert_eq!(m.get("speed"), Some(&Value::F64(1.0)));
                assert_eq!(m.get("clip"), Some(&Value::Int(1)));
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn numbered_fm_names_resolve_to_canonical_keys() {
        // 8 families * 8 ops + 5 short spellings * 8 + fm1-fm8 + the 9x9
        // matrix under both spellings.
        let names = numbered_control_names();
        assert_eq!(names.len(), 8 * 8 + 5 * 8 + 8 + 9 * 9 * 2);
        let key = |name: &str| {
            names
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, k)| k.as_str())
                .unwrap_or_else(|| panic!("{name} missing"))
        };
        // `{name}1` is the bare control; `fmN` aliases the chain `fmiN`;
        // `fm{i}{j}` aliases the matrix edge `fmi{i}{j}`.
        assert_eq!(key("fmh1"), "fmh");
        assert_eq!(key("fm1"), "fm");
        assert_eq!(key("fm3"), "fmi3");
        assert_eq!(key("fmatt5"), "fmattack5");
        assert_eq!(key("fme1"), "fmenv");
        assert_eq!(key("fm23"), "fmi23");
        assert_eq!(key("fmi20"), "fmi20");
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
