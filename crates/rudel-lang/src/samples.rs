use crate::bindings::{KPattern, arg_to_f64, arg_to_raw_str, arg0};
use koto::prelude::*;
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

/// Convert a Koto value into a `serde_json::Value` for an inline sample map.
/// Handles the shapes a sample map uses: strings, numbers, lists, and nested
/// (note-keyed) maps with string keys.
fn koto_to_json(value: &KValue) -> Option<serde_json::Value> {
    use serde_json::Value as Json;
    if let Some(s) = arg_to_raw_str(value) {
        return Some(Json::String(s));
    }
    Some(match value {
        KValue::Number(n) => {
            if n.is_i64() {
                Json::Number(i64::from(n).into())
            } else {
                serde_json::Number::from_f64(f64::from(n)).map_or(Json::Null, Json::Number)
            }
        }
        KValue::List(l) => Json::Array(l.data().iter().filter_map(koto_to_json).collect()),
        KValue::Tuple(t) => Json::Array(t.data().iter().filter_map(koto_to_json).collect()),
        KValue::Map(m) => {
            let obj = m
                .data()
                .iter()
                .filter_map(|(k, v)| match k.value() {
                    KValue::Str(key) => Some((key.to_string(), koto_to_json(v)?)),
                    _ => None,
                })
                .collect();
            Json::Object(obj)
        }
        _ => return None,
    })
}

/// Register the side-effecting sample helpers (`samples` / `aliasBank`). They
/// record their string arguments into `effects` (applied by the host against
/// its sample bank) and return an empty pattern.
pub(crate) fn register_samples(prelude: &KMap, effects: Arc<Mutex<SampleEffects>>) {
    let sample_effects = effects.clone();
    let tempo_effects = effects.clone();
    register_midimaps(prelude, effects.clone());
    prelude.add_fn("samples", move |ctx| {
        let mut eff = sample_effects.lock().unwrap();
        let args = ctx.args();
        match args.first() {
            // Inline map form: samples({ bd: "...", ... }, base?)
            Some(KValue::Map(_)) => {
                if let Some(json) = koto_to_json(&args[0]) {
                    let base = args.get(1).and_then(arg_to_raw_str).unwrap_or_default();
                    eff.maps.push((json.to_string(), base));
                }
            }
            // String source form: samples("github:...", "https://...", ...)
            _ => {
                for arg in args {
                    if let Some(s) = arg_to_raw_str(arg) {
                        eff.sources.push(s);
                    }
                }
            }
        }
        Ok(KPattern(rudel_core::silence()).into())
    });

    let effects = tempo_effects.clone();
    // `setSoundfontUrl(url)`: repoint General MIDI preset loading at another
    // mirror or a local directory.
    prelude.add_fn("setSoundfontUrl", move |ctx| {
        if let Some(url) = ctx.args().first().and_then(arg_to_raw_str) {
            effects.lock().unwrap().soundfont_url = Some(url);
        }
        Ok(KPattern(rudel_core::silence()).into())
    });

    // `registerSoundfonts()`: upstream registers the `gm_*` names with lazy
    // loaders at prebake. Rudel knows them from its built-in General MIDI
    // table and fetches on first use, so this exists for parity and to make
    // the intent explicit in a script.
    prelude.add_fn("registerSoundfonts", |_ctx| {
        Ok(KPattern(rudel_core::silence()).into())
    });

    let effects = tempo_effects.clone();
    // `loadSoundfont(path, name?)`: load a local `.sf2` file, exposing its
    // presets under `name` (defaulting to the file stem).
    prelude.add_fn("loadSoundfont", move |ctx| {
        let args = ctx.args();
        if let Some(path) = args.first().and_then(arg_to_raw_str) {
            let name = args
                .get(1)
                .and_then(arg_to_raw_str)
                .unwrap_or_else(|| soundfont_stem(&path));
            effects
                .lock()
                .unwrap()
                .soundfonts
                .push((path, name.clone()));
            return Ok(KValue::Str(name.into()));
        }
        Ok(KValue::Null)
    });

    // tables(url, frameLen): load a collection of wavetables to play with `s`.
    // Recorded as a host effect, like `samples(...)`; the default frame length
    // is superdough's 2048.
    let effects = tempo_effects.clone();
    prelude.add_fn("tables", move |ctx| {
        let args = ctx.args();
        if let Some(source) = args.first().and_then(arg_to_raw_str) {
            let frame_len = args
                .get(1)
                .map(arg_to_f64)
                .filter(|n| *n >= 1.0)
                .map_or(2048, |n| n as usize);
            effects.lock().unwrap().tables.push((source, frame_len));
        }
        Ok(KPattern(rudel_core::silence()).into())
    });

    // midin(device): open a named MIDI input port and return a
    // `(cc[, channel]) -> pattern` factory reading only that device's control
    // changes. Upstream returns a promise (WebMidi is async); Rudel records the
    // port as a host effect and returns the factory straight away, so the
    // signals read 0 until the app has the port open.
    let effects = tempo_effects.clone();
    prelude.add_fn("midin", move |ctx| {
        let device = arg_to_raw_str(&arg0(ctx)).unwrap_or_default();
        effects.lock().unwrap().midi_inputs.push(device.clone());
        Ok(KValue::NativeFunction(KNativeFunction::new(move |ctx| {
            let a = ctx.args();
            let cc = a.first().map(arg_to_f64).unwrap_or(0.0) as u8;
            let chan = a
                .get(1)
                .map(arg_to_f64)
                .map(|c| c as u8)
                .filter(|c| *c >= 1);
            Ok(KPattern(rudel_core::cc_in_from(&device, cc, chan)).into())
        })))
    });

    // midikeys(device): open a named MIDI input port and return a
    // `(noteLength?) -> pattern` factory of the notes played on it. `noteLength`
    // is in cycles and defaults to 0.5, as upstream.
    let effects = tempo_effects.clone();
    prelude.add_fn("midikeys", move |ctx| {
        let device = arg_to_raw_str(&arg0(ctx)).unwrap_or_default();
        effects.lock().unwrap().midi_inputs.push(device.clone());
        Ok(KValue::NativeFunction(KNativeFunction::new(move |ctx| {
            let length = match ctx.args().first() {
                None | Some(KValue::Null) => rudel_core::pure(rudel_core::Value::F64(0.5)),
                Some(arg) => crate::bindings::arg_to_pattern(arg),
            };
            Ok(KPattern(rudel_core::midi_keys(&device, length)).into())
        })))
    });

    // aliasBank(canonical, alias, ...): each extra string is an alias.
    let effects = tempo_effects.clone();
    prelude.add_fn("aliasBank", move |ctx| {
        let strs: Vec<String> = ctx.args().iter().filter_map(arg_to_raw_str).collect();
        if let Some((canonical, aliases)) = strs.split_first() {
            let mut eff = effects.lock().unwrap();
            for alias in aliases {
                eff.bank_aliases.push((canonical.clone(), alias.clone()));
            }
        }
        Ok(KPattern(rudel_core::silence()).into())
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
        let effects = tempo_effects.clone();
        prelude.add_fn(name, move |ctx| {
            let text = ctx.args().first().and_then(arg_to_raw_str);
            if is_url && text.is_none() {
                return koto::runtime::runtime_error!("loadOrc: expected a url string");
            }
            effects
                .lock()
                .unwrap()
                .csound_orcs
                .push((is_url, text.unwrap_or_default()));
            Ok(KPattern(rudel_core::silence()).into())
        });
    }

    for (name, scale) in [
        ("setCps", 1.0),
        ("setcps", 1.0),
        ("setCpm", 1.0 / 60.0),
        ("setcpm", 1.0 / 60.0),
    ] {
        let effects = tempo_effects.clone();
        prelude.add_fn(name, move |ctx| {
            effects.lock().unwrap().cps = Some(arg_to_f64(&arg0(ctx)) * scale);
            Ok(KPattern(rudel_core::silence()).into())
        });
    }
}

/// Read one midimap entry: a bare CC number (`{ lpf: 74 }`) or a table
/// (`{ lpf: { ccn: 74, min: 0, max: 20000, exp: 0.5 } }`), matching
/// `unifyMapping`'s two accepted value shapes.
fn cc_mapping_from(value: &KValue) -> Option<CcMapping> {
    let ccn = |x: f64| x.round().clamp(0.0, 127.0) as u8;
    match value {
        KValue::Map(m) => {
            let field = |k: &str, fallback| {
                m.get(k)
                    .map(|v| arg_to_f64(&v))
                    .filter(|x| x.is_finite())
                    .unwrap_or(fallback)
            };
            Some(CcMapping {
                ccn: ccn(m.get("ccn").map(|v| arg_to_f64(&v))?),
                min: field("min", 0.0),
                max: field("max", 1.0),
                exp: field("exp", 1.0),
            })
        }
        KValue::Number(n) => Some(CcMapping::new(ccn(f64::from(n)))),
        _ => None,
    }
}

/// Collect a `{ control: ccn | { ccn, min, max, exp } }` Koto map into the
/// entries [`rudel_core::set_midimap`] takes.
fn midimap_entries(value: &KValue) -> Vec<(String, CcMapping)> {
    let KValue::Map(m) = value else {
        return Vec::new();
    };
    m.data()
        .iter()
        .filter_map(|(k, v)| match k.value() {
            KValue::Str(key) => Some((key.to_string(), cc_mapping_from(v)?)),
            _ => None,
        })
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
fn register_midimaps(prelude: &KMap, effects: Arc<Mutex<SampleEffects>>) {
    prelude.add_fn("midimaps", move |ctx| {
        match ctx.args().first() {
            Some(KValue::Map(maps)) => {
                for (name, table) in maps.data().iter() {
                    if let KValue::Str(name) = name.value() {
                        rudel_core::set_midimap(name, midimap_entries(table));
                    }
                }
            }
            Some(arg) => {
                if let Some(source) = arg_to_raw_str(arg) {
                    effects.lock().unwrap().midimaps.push(source);
                }
            }
            None => {}
        }
        Ok(KPattern(rudel_core::silence()).into())
    });
    prelude.add_fn("defaultmidimap", |ctx| {
        if let Some(table) = ctx.args().first() {
            rudel_core::set_midimap("default", midimap_entries(table));
        }
        Ok(KPattern(rudel_core::silence()).into())
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
