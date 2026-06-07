use crate::bindings::{KPattern, arg_to_f64, arg0};
use koto::prelude::*;
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
}

/// Convert a Koto value into a `serde_json::Value` for an inline sample map.
/// Handles the shapes a sample map uses: strings, numbers, lists, and nested
/// (note-keyed) maps with string keys.
fn koto_to_json(value: &KValue) -> Option<serde_json::Value> {
    use serde_json::Value as Json;
    Some(match value {
        KValue::Str(s) => Json::String(s.to_string()),
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
    prelude.add_fn("samples", move |ctx| {
        let mut eff = sample_effects.lock().unwrap();
        let args = ctx.args();
        match args.first() {
            // Inline map form: samples({ bd: "...", ... }, base?)
            Some(KValue::Map(_)) => {
                if let Some(json) = koto_to_json(&args[0]) {
                    let base = match args.get(1) {
                        Some(KValue::Str(s)) => s.to_string(),
                        _ => String::new(),
                    };
                    eff.maps.push((json.to_string(), base));
                }
            }
            // String source form: samples("github:...", "https://...", ...)
            _ => {
                for arg in args {
                    if let KValue::Str(s) = arg {
                        eff.sources.push(s.to_string());
                    }
                }
            }
        }
        Ok(KPattern(rudel_core::silence()).into())
    });

    // aliasBank(canonical, alias, ...): each extra string is an alias.
    prelude.add_fn("aliasBank", move |ctx| {
        let strs: Vec<String> = ctx
            .args()
            .iter()
            .filter_map(|a| match a {
                KValue::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect();
        if let Some((canonical, aliases)) = strs.split_first() {
            let mut eff = effects.lock().unwrap();
            for alias in aliases {
                eff.bank_aliases.push((canonical.clone(), alias.clone()));
            }
        }
        Ok(KPattern(rudel_core::silence()).into())
    });

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
