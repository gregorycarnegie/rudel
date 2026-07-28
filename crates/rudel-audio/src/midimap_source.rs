// midimap_source.rs - loading `midimaps(url)` control->CC tables.
// Ports the remote half of midi.mjs's `midimaps`: a `github:user/repo` pseudo-URL
// resolves to that repo's `midimap.json`, any other string is fetched (or read
// from disk) as JSON, and each `{ name: { control: ccn | {ccn,...} } }` entry is
// registered into rudel-core's midimap registry. The inline map form is handled
// in the language layer, which needs no I/O.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{sample_map::github_path, samples::fetch_cached_bytes};
use rudel_core::{CcMapping, set_midimap};
use serde_json::Value as Json;
use std::thread::JoinHandle;

/// Read one midimap entry: a bare CC number or a `{ccn, min, max, exp}` object,
/// matching the two value shapes `unifyMapping` accepts.
fn cc_mapping_from(value: &Json) -> Option<CcMapping> {
    let ccn = |x: f64| x.round().clamp(0.0, 127.0) as u8;
    match value {
        Json::Number(n) => Some(CcMapping::new(ccn(n.as_f64()?))),
        Json::Object(o) => {
            let field = |k: &str, fallback| o.get(k).and_then(Json::as_f64).unwrap_or(fallback);
            Some(CcMapping {
                ccn: ccn(o.get("ccn").and_then(Json::as_f64)?),
                min: field("min", 0.0),
                max: field("max", 1.0),
                exp: field("exp", 1.0),
            })
        }
        _ => None,
    }
}

/// Resolve a midimap source to the URL (or path) its JSON lives at. A
/// `github:` pseudo-URL points at that repo's `midimap.json`; anything else is
/// taken as-is, so an http(s) URL or a local file both work.
fn midimap_url(source: &str) -> Result<String, String> {
    if source.starts_with("github:") {
        github_path(source, "midimap.json")
    } else {
        Ok(source.to_string())
    }
}

/// Fetch `source` and register every midimap it declares, returning how many
/// were registered. Blocking; call it off the UI thread.
pub fn load_midimaps(source: &str) -> Result<usize, String> {
    let url = midimap_url(source)?;
    let bytes = fetch_cached_bytes(&url)?;
    let json: Json =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse {url} as JSON: {e}"))?;
    let Json::Object(maps) = json else {
        return Err(format!("{url}: expected a JSON object of midimaps"));
    };
    let mut count = 0;
    for (name, table) in maps {
        let Json::Object(table) = table else { continue };
        let entries: Vec<(String, CcMapping)> = table
            .iter()
            .filter_map(|(control, v)| Some((control.clone(), cc_mapping_from(v)?)))
            .collect();
        set_midimap(&name, entries);
        count += 1;
    }
    Ok(count)
}

/// Load midimaps on a background thread, for the host's job queue.
pub fn spawn_midimaps(source: String) -> JoinHandle<Result<usize, String>> {
    std::thread::spawn(move || load_midimaps(&source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Value, ValueMap, midimap_ccs};

    #[test]
    fn github_source_points_at_midimap_json() {
        assert_eq!(
            midimap_url("github:user/repo").unwrap(),
            "https://raw.githubusercontent.com/user/repo/main/midimap.json"
        );
        // A plain URL or path is used unchanged.
        assert_eq!(
            midimap_url("https://example.com/my.json").unwrap(),
            "https://example.com/my.json"
        );
    }

    #[test]
    fn a_local_json_file_registers_its_maps() {
        // `fetch_cached_bytes` reads non-http sources off disk, so a local
        // midimap.json loads without a network round trip.
        let dir = std::env::temp_dir().join("rudel_midimap_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("midimap.json");
        std::fs::write(
            &path,
            r#"{ "audio_test_map": { "lpf": { "ccn": 74, "min": 0, "max": 20000, "exp": 0.5 },
                                     "gain": 7 } }"#,
        )
        .unwrap();

        assert_eq!(load_midimaps(path.to_str().unwrap()).unwrap(), 1);
        let controls: ValueMap = [
            ("cutoff".to_string(), Value::F64(5000.0)),
            ("gain".to_string(), Value::F64(1.0)),
        ]
        .into_iter()
        .collect();
        // gain -> CC 7 full; cutoff -> CC 74, (5000/20000)^0.5 = 0.5.
        assert_eq!(
            midimap_ccs("audio_test_map", &controls),
            [(7, 1.0), (74, 0.5)]
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_non_object_payload_is_an_error() {
        let dir = std::env::temp_dir().join("rudel_midimap_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "[1, 2, 3]").unwrap();
        assert!(load_midimaps(path.to_str().unwrap()).is_err());
        std::fs::remove_file(&path).ok();
    }
}
