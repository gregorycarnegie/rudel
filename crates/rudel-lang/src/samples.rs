use crate::{
    bindings::{arg_to_f64, arg_to_raw_str, arg0},
    js::{Arg, Res, Scope},
};
use rudel_core::CcMapping;
use std::sync::{Arc, Mutex};

/// Side effects collected while evaluating a script: sample-pack loads and bank
/// aliases that the host applies against its own sample bank after eval.
#[derive(Default, Debug, PartialEq)]
pub struct SampleEffects {
    /// `samples(src, ...)` string sources to load (URL / github: / path).
    pub sources: Vec<String>,
    /// Inline `samples({...}, base)` maps as `(strudel.json text, base)`.
    pub maps: Vec<(String, String)>,
    /// `aliasBank(canonical, alias, ...)` pairs to register.
    pub bank_aliases: Vec<(String, String)>,
    /// `aliasBank(url)` sources: JSON files of `{ canonical: alias | [alias] }`.
    pub bank_alias_sources: Vec<String>,
    /// Optional global tempo requested by `setCps`/`setcps`/`setCpm`/`setcpm`.
    pub cps: Option<f64>,
    /// Where soundfont presets are fetched from (`setSoundfontUrl`).
    pub soundfont_url: Option<String>,
    /// Local SoundFont (`.sf2`) files to load (`loadSoundfont`), as
    /// `(path, sound name)`.
    pub soundfonts: Vec<(String, String)>,
    /// MIDI input ports `midin`/`midikeys` asked for, by the name the script
    /// used. The host opens each (matching the name against its ports) and
    /// tags what arrives with that same name.
    pub midi_inputs: Vec<String>,
    /// Wavetable collections to load (`tables(url, frameLen)`), as
    /// `(source, frame length)`.
    pub tables: Vec<(String, usize)>,
    /// `midimaps(src)` string sources to fetch (URL / `github:` / path). The
    /// inline `midimaps({...})` form needs no I/O and is applied during eval.
    pub midimaps: Vec<String>,
    /// Csound orchestras the script asked for, in the order it asked, as
    /// `(is_url, text)` — `loadCsound(code)` is the code itself, `loadOrc(url)`
    /// a URL to fetch it from. The host starts Csound on the first of these.
    pub csound_orcs: Vec<(bool, String)>,
}

/// Convert a script value into a `serde_json::Value` for an inline sample map.
/// Handles the shapes a sample map uses: strings, numbers, arrays, and nested
/// (note-keyed) objects.
fn arg_to_json(value: &Arg) -> Option<serde_json::Value> {
    use serde_json::Value as Json;
    if let Some(s) = arg_to_raw_str(value) {
        return Some(Json::String(s));
    }
    Some(match value {
        Arg::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => Json::Number((*n as i64).into()),
        Arg::Num(n) => serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number),
        Arg::List(l) => Json::Array(l.iter().filter_map(arg_to_json).collect()),
        Arg::Map(m) => Json::Object(
            m.iter()
                .filter_map(|(key, v)| Some((key.clone(), arg_to_json(v)?)))
                .collect(),
        ),
        _ => return None,
    })
}

/// The pattern a side-effecting call hands back, so it can sit on a line of
/// its own.
fn done() -> Res {
    Ok(rudel_core::silence().into())
}

/// Register the side-effecting sample helpers (`samples` / `aliasBank`). They
/// record their string arguments into `effects` (applied by the host against
/// its sample bank) and return an empty pattern.
pub(crate) fn register_samples(prelude: &Scope, effects: Arc<Mutex<SampleEffects>>) {
    register_midimaps(prelude, effects.clone());
    let eff = effects.clone();
    prelude.func("samples", move |args| {
        let mut eff = eff.lock().unwrap();
        match args.first() {
            // Inline map form: samples({ bd: "...", ... }, base?)
            Some(map @ Arg::Map(_)) => {
                if let Some(json) = arg_to_json(map) {
                    let base = args.get(1).and_then(arg_to_raw_str).unwrap_or_default();
                    eff.maps.push((json.to_string(), base));
                }
            }
            // String source form: samples("github:...", "https://...", ...)
            _ => eff.sources.extend(args.iter().filter_map(arg_to_raw_str)),
        }
        done()
    });

    let eff = effects.clone();
    // `setSoundfontUrl(url)`: repoint General MIDI preset loading at another
    // mirror or a local directory.
    prelude.func("setSoundfontUrl", move |args| {
        if let Some(url) = args.first().and_then(arg_to_raw_str) {
            eff.lock().unwrap().soundfont_url = Some(url);
        }
        done()
    });

    // `registerSoundfonts()`: upstream registers the `gm_*` names with lazy
    // loaders at prebake. Rudel knows them from its built-in General MIDI
    // table and fetches on first use, so this exists for parity and to make
    // the intent explicit in a script.
    prelude.func("registerSoundfonts", |_| done());

    let eff = effects.clone();
    // `loadSoundfont(path, name?)`: load a local `.sf2` file, exposing its
    // presets under `name` (defaulting to the file stem).
    prelude.func("loadSoundfont", move |args| {
        let Some(path) = args.first().and_then(arg_to_raw_str) else {
            return Ok(Arg::Null);
        };
        let name = args
            .get(1)
            .and_then(arg_to_raw_str)
            .unwrap_or_else(|| soundfont_stem(&path));
        eff.lock().unwrap().soundfonts.push((path, name.clone()));
        Ok(name.into())
    });

    // tables(url, frameLen): load a collection of wavetables to play with `s`.
    // Recorded as a host effect, like `samples(...)`; the default frame length
    // is superdough's 2048.
    let eff = effects.clone();
    prelude.func("tables", move |args| {
        if let Some(source) = args.first().and_then(arg_to_raw_str) {
            let frame_len = args
                .get(1)
                .map(arg_to_f64)
                .filter(|n| *n >= 1.0)
                .map_or(2048, |n| n as usize);
            eff.lock().unwrap().tables.push((source, frame_len));
        }
        done()
    });

    // midin(device): open a named MIDI input port and return a
    // `(cc[, channel]) -> pattern` factory reading only that device's control
    // changes. Upstream returns a promise (WebMidi is async); Rudel records the
    // port as a host effect and returns the factory straight away, so the
    // signals read 0 until the app has the port open.
    let eff = effects.clone();
    prelude.func("midin", move |args| {
        let device = arg_to_raw_str(arg0(args)).unwrap_or_default();
        eff.lock().unwrap().midi_inputs.push(device.clone());
        Ok(Arg::native(move |a| {
            let cc = a.first().map(arg_to_f64).unwrap_or(0.0) as u8;
            let chan = a.get(1).map(|c| arg_to_f64(c) as u8).filter(|c| *c >= 1);
            Ok(rudel_core::cc_in_from(&device, cc, chan).into())
        }))
    });

    // midikeys(device): open a named MIDI input port and return a
    // `(noteLength?) -> pattern` factory of the notes played on it. `noteLength`
    // is in cycles and defaults to 0.5, as upstream.
    let eff = effects.clone();
    prelude.func("midikeys", move |args| {
        let device = arg_to_raw_str(arg0(args)).unwrap_or_default();
        eff.lock().unwrap().midi_inputs.push(device.clone());
        Ok(Arg::native(move |a| {
            let length = match a.first() {
                None | Some(Arg::Null) => rudel_core::pure(rudel_core::Value::F64(0.5)),
                Some(arg) => crate::bindings::arg_to_pattern(arg),
            };
            Ok(rudel_core::midi_keys(&device, length).into())
        }))
    });

    // aliasBank(canonical, alias | [alias, ...], ...), aliasBank({ canonical:
    // alias | [alias, ...] }), or aliasBank(url) of a JSON file of that map.
    let eff = effects.clone();
    prelude.func("aliasBank", move |args| {
        let mut eff = eff.lock().unwrap();
        match args {
            [Arg::Map(map)] => {
                for (canonical, aliases) in map {
                    for alias in alias_strs(std::slice::from_ref(aliases)) {
                        eff.bank_aliases.push((canonical.clone(), alias));
                    }
                }
            }
            [source] => eff.bank_alias_sources.extend(arg_to_raw_str(source)),
            [canonical, aliases @ ..] => {
                if let Some(canonical) = arg_to_raw_str(canonical) {
                    for alias in alias_strs(aliases) {
                        eff.bank_aliases.push((canonical.clone(), alias));
                    }
                }
            }
            [] => {}
        }
        done()
    });

    // `loadCsound(code)` / `loadOrc(url)` (@strudel/csound). Both start Csound
    // on first use; the difference is only where the orchestra text comes from.
    // `loadCsound()` with no argument is how upstream starts it bare, so an
    // empty string is recorded rather than skipped.
    for (name, is_url) in [
        ("loadCsound", false),
        ("loadCSound", false),
        ("loadcsound", false),
        ("loadOrc", true),
        ("loadorc", true),
    ] {
        let eff = effects.clone();
        prelude.func(name, move |args| {
            let text = args.first().and_then(arg_to_raw_str);
            if is_url && text.is_none() {
                return Err("loadOrc: expected a url string".to_string());
            }
            eff.lock()
                .unwrap()
                .csound_orcs
                .push((is_url, text.unwrap_or_default()));
            done()
        });
    }

    for (name, scale) in [
        ("setCps", 1.0),
        ("setcps", 1.0),
        ("setCpm", 1.0 / 60.0),
        ("setcpm", 1.0 / 60.0),
    ] {
        let eff = effects.clone();
        prelude.func(name, move |args| {
            eff.lock().unwrap().cps = Some(arg_to_f64(arg0(args)) * scale);
            done()
        });
    }
}

/// Read one midimap entry: a bare CC number (`{ lpf: 74 }`) or a table
/// (`{ lpf: { ccn: 74, min: 0, max: 20000, exp: 0.5 } }`), matching
/// `unifyMapping`'s two accepted value shapes.
fn cc_mapping_from(value: &Arg) -> Option<CcMapping> {
    let ccn = |x: f64| x.round().clamp(0.0, 127.0) as u8;
    match value {
        Arg::Map(_) => {
            let field = |k: &str, fallback| {
                value
                    .get(k)
                    .map(arg_to_f64)
                    .filter(|x| x.is_finite())
                    .unwrap_or(fallback)
            };
            Some(CcMapping {
                ccn: ccn(value.get("ccn").map(arg_to_f64)?),
                min: field("min", 0.0),
                max: field("max", 1.0),
                exp: field("exp", 1.0),
            })
        }
        Arg::Num(n) => Some(CcMapping::new(ccn(*n))),
        _ => None,
    }
}

/// The alias names in `aliasBank` arguments: strings, or lists of them.
fn alias_strs(args: &[Arg]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| match arg {
            Arg::List(items) => items.iter().filter_map(arg_to_raw_str).collect::<Vec<_>>(),
            arg => arg_to_raw_str(arg).into_iter().collect(),
        })
        .collect()
}

/// Collect a `{ control: ccn | { ccn, min, max, exp } }` object into the
/// entries [`rudel_core::set_midimap`] takes.
fn midimap_entries(value: &Arg) -> Vec<(String, CcMapping)> {
    let Arg::Map(m) = value else {
        return Vec::new();
    };
    m.iter()
        .filter_map(|(key, v)| Some((key.clone(), cc_mapping_from(v)?)))
        .collect()
}

/// The control-to-CC tables `Pattern.prototype.midi` consults, keyed by the
/// hap's `midimap` control (`default` when it sets none).
///
/// `midimaps({ name: { control: ccn } })` and `defaultmidimap({ control: ccn })`
/// write the process-global registry in `rudel-core` directly — they need no
/// I/O. `midimaps("github:user/repo")` (or any URL / path) instead records the
/// source for the host to fetch, since the JSON lives behind a network call;
/// upstream `await`s a `fetch`, rudel collects the request like `samples(...)`.
fn register_midimaps(prelude: &Scope, effects: Arc<Mutex<SampleEffects>>) {
    prelude.func("midimaps", move |args| {
        match args.first() {
            Some(Arg::Map(maps)) => {
                for (name, table) in maps {
                    rudel_core::set_midimap(name, midimap_entries(table));
                }
            }
            Some(arg) => {
                if let Some(source) = arg_to_raw_str(arg) {
                    effects.lock().unwrap().midimaps.push(source);
                }
            }
            None => {}
        }
        done()
    });
    prelude.func("defaultmidimap", |args| {
        if let Some(table) = args.first() {
            rudel_core::set_midimap("default", midimap_entries(table));
        }
        done()
    });
}

/// The default sound name for a `.sf2` file: its stem, lowercased, with
/// separators normalised so it is typeable in a pattern.
fn soundfont_stem(path: &str) -> String {
    let stem = path
        .rsplit(['/', '\u{5c}'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".sf2")
        .trim_end_matches(".SF2");
    stem.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
