// sample_map.rs - parsing Strudel-format sample maps (`strudel.json`) and
// resolving their special URL schemes. Mirrors the pure parts of
// strudel/packages/superdough/sampler.mjs (processSampleMap / githubPath /
// resolveSpecialPaths). Network fetching and decoding live in samples.rs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value as Json;

/// Resolve Strudel's shorthand sample-source schemes to a concrete pseudo/real
/// URL, mirroring superdough's `resolveSpecialPaths` (`bubo:`) plus the extra
/// rewrites in `fetchSampleMap`:
/// - `bubo:foo` -> `github:Bubobubobubobubo/dough-foo`
/// - `local:`   -> `http://localhost:5432` (the `@strudel/sampler` dev server)
/// - `shabda:words` -> `https://shabda.ndre.gr/words.json?strudel=1`
/// - `shabda/speech[/<lang>/<gender>]:words` -> the shabda speech endpoint
///
/// (`github:` is left for [`github_path`] to expand, since it needs a subpath.)
pub(crate) fn resolve_special_paths(base: &str) -> String {
    if let Some(repo) = base.strip_prefix("bubo:") {
        return format!("github:Bubobubobubobubo/dough-{repo}");
    }
    if base.starts_with("local:") {
        return "http://localhost:5432".to_string();
    }
    // `shabda/speech` is checked before `shabda:` (it has no `:` after the
    // scheme word, so the bare-`shabda:` branch would not match it anyway).
    if let Some(rest) = base.strip_prefix("shabda/speech") {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let (params, words) = rest.split_once(':').unwrap_or(("", rest));
        // default voice (matches superdough's `gender='f'`, `language='en-GB'`);
        // a `<lang>/<gender>` params segment overrides them.
        let (language, gender) = if params.is_empty() {
            ("en-GB", "f")
        } else {
            params.split_once('/').unwrap_or((params, "f"))
        };
        return format!(
            "https://shabda.ndre.gr/speech/{words}.json?gender={gender}&language={language}&strudel=1"
        );
    }
    if let Some(path) = base.strip_prefix("shabda:") {
        return format!("https://shabda.ndre.gr/{path}.json?strudel=1");
    }
    base.to_string()
}

/// Expand a `github:user/repo[/branch][/sub...]` pseudo-URL into a
/// raw.githubusercontent.com URL. `subpath` is appended (e.g. `strudel.json`);
/// pass `""` to get the base directory (with a trailing slash).
pub(crate) fn github_path(base: &str, subpath: &str) -> Result<String, String> {
    let path = base
        .strip_prefix("github:")
        .ok_or_else(|| format!("expected \"github:\" at the start of {base:?}"))?;
    let path = path.strip_suffix('/').unwrap_or(path);

    let comps: Vec<&str> = path.split('/').collect();
    let user = comps.first().copied().unwrap_or("");
    let repo = comps.get(1).copied().unwrap_or("samples");
    let branch = comps.get(2).copied().unwrap_or("main");
    let mut other: Vec<&str> = comps.iter().skip(3).copied().collect();
    other.push(subpath);
    let other = other.join("/");

    Ok(format!(
        "https://raw.githubusercontent.com/{user}/{repo}/{branch}/{other}"
    ))
}

/// Strip the last path segment of a URL to get its base directory (no trailing
/// slash), e.g. `.../packs/strudel.json` -> `.../packs`. Mirrors superdough's
/// `getBaseURL` (`new URL('.', url)` without the trailing slash): an http(s)
/// URL whose authority carries *no* path component (e.g. `http://localhost:5432`,
/// the `@strudel/sampler` dev server) is its own base, so the leading `//` of
/// the scheme is never mistaken for a path separator.
pub(crate) fn base_url_of(url: &str) -> String {
    if let Some(scheme) = url.find("://") {
        let after_authority = scheme + 3;
        if !url[after_authority..].contains('/') {
            // authority only, no path — the URL is already the base directory.
            return url.to_string();
        }
    }
    match url.rfind('/') {
        Some(i) => url[..i].to_string(),
        None => String::new(),
    }
}

/// Join a base URL/dir with a (possibly relative) file path, inserting exactly
/// one separator. HTTP(S) paths are percent-encoded for transport.
pub(crate) fn join_url(base: &str, v: &str) -> String {
    if v.starts_with("http://") || v.starts_with("https://") {
        return encode_http_url_path(v);
    }
    if base.is_empty() {
        return v.to_string();
    }
    let sep = if base.ends_with('/') || v.starts_with('/') {
        ""
    } else {
        "/"
    };
    let path = if base.starts_with("http://") || base.starts_with("https://") {
        percent_encode_path(v)
    } else {
        v.to_string()
    };
    format!("{base}{sep}{path}")
}

/// Percent-encode the path component of an HTTP(S) URL, leaving the scheme,
/// authority, and query parameters untouched.
fn encode_http_url_path(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return percent_encode_path(url);
    };
    let after_authority = scheme_end + 3;
    let Some(path_start) = url[after_authority..]
        .find('/')
        .map(|i| after_authority + i)
    else {
        return url.to_string();
    };
    let query_start = url[path_start..]
        .find('?')
        .map(|i| path_start + i)
        .unwrap_or(url.len());
    format!(
        "{}{}{}",
        &url[..path_start],
        percent_encode_path(&url[path_start..query_start]),
        &url[query_start..]
    )
}

/// Percent-encode arbitrary string paths by replacing unsafe characters with
/// `%HEX` sequences. Already percent-encoded sequences are preserved.
fn percent_encode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(path.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
        } else if is_path_safe(b) {
            out.push(b as char);
            i += 1;
        } else {
            out.push_str(&format!("%{b:02X}"));
            i += 1;
        }
    }
    out
}

/// Check if a byte represents an ASCII hex digit.
fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Determine if a byte is safe to include in a path segment without percent-encoding.
fn is_path_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'/'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

/// Resolve a base string through the `bubo:`/`github:` schemes into a usable
/// URL prefix (github bases become a raw.githubusercontent directory URL).
fn resolve_base(base: &str) -> String {
    let resolved = resolve_special_paths(base);
    if resolved.starts_with("github:") {
        github_path(&resolved, "").unwrap_or(resolved)
    } else {
        resolved
    }
}

/// The files for one sound: either a flat index list (drum-machine packs) or a
/// note-keyed (pitched) map of `(midi_note, urls)` groups.
#[derive(Debug, PartialEq)]
pub(crate) enum SoundFiles {
    Flat(Vec<String>),
    Pitched(Vec<(i32, Vec<String>)>),
}

/// Pull the file paths out of a string/array sample-map value, in declaration
/// order. Non-string leaves are skipped.
fn collect_files(value: &Json) -> Vec<String> {
    match value {
        Json::String(s) => vec![s.clone()],
        Json::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse a Strudel sample-map JSON into `(sound_name, files)` entries. `base`
/// resolves relative file paths; a top-level or per-entry `_base` key overrides
/// it. String/array values become [`SoundFiles::Flat`]; note-keyed objects
/// become [`SoundFiles::Pitched`] (keys parsed to MIDI via `note_to_midi`).
pub(crate) fn parse_sample_map(
    json: &str,
    base: &str,
) -> Result<Vec<(String, SoundFiles)>, String> {
    let parsed: Json = serde_json::from_str(json).map_err(|e| format!("parse sample map: {e}"))?;
    let Json::Object(map) = parsed else {
        return Err("sample map must be a JSON object".to_string());
    };

    // A top-level `_base` overrides the caller-supplied base.
    let map_base = map
        .get("_base")
        .and_then(Json::as_str)
        .map(resolve_base)
        .unwrap_or_else(|| resolve_base(base));

    let mut entries = Vec::new();
    for (key, value) in &map {
        if key == "_base" {
            continue;
        }
        // A per-entry `_base` (on object values) overrides the map base.
        let entry_base = value
            .get("_base")
            .and_then(Json::as_str)
            .map(resolve_base)
            .unwrap_or_else(|| map_base.clone());
        let join = |f: &String| join_url(&entry_base, f);

        let files = match value {
            // Note-keyed object (pitched sample map): one group per note name.
            Json::Object(notes) => {
                let groups = notes
                    .iter()
                    .filter(|(note, _)| *note != "_base")
                    .filter_map(|(note, files)| {
                        let midi = rudel_core::note_to_midi(note)?;
                        Some((midi, collect_files(files).iter().map(join).collect()))
                    })
                    .collect();
                SoundFiles::Pitched(groups)
            }
            _ => SoundFiles::Flat(collect_files(value).iter().map(join).collect()),
        };
        entries.push((key.clone(), files));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_path_expands_defaults() {
        assert_eq!(
            github_path("github:tidalcycles/dirt-samples", "strudel.json").unwrap(),
            "https://raw.githubusercontent.com/tidalcycles/dirt-samples/main/strudel.json"
        );
        // explicit branch + base directory (empty subpath -> trailing slash)
        assert_eq!(
            github_path("github:user/repo/dev", "").unwrap(),
            "https://raw.githubusercontent.com/user/repo/dev/"
        );
    }

    #[test]
    fn bubo_resolves_to_github() {
        assert_eq!(
            resolve_special_paths("bubo:drum"),
            "github:Bubobubobubobubo/dough-drum"
        );
    }

    #[test]
    fn base_url_of_matches_getbaseurl() {
        // file under a path -> directory of that file
        assert_eq!(
            base_url_of("https://host/a/b/strudel.json"),
            "https://host/a/b"
        );
        // root-level file -> the authority
        assert_eq!(base_url_of("https://host/strudel.json"), "https://host");
        // authority only (the `local:` sampler server) -> the URL itself, NOT
        // the scheme's `http:/` truncation the naive rfind would produce.
        assert_eq!(
            base_url_of("http://localhost:5432"),
            "http://localhost:5432"
        );
        // trailing slash -> authority without the slash
        assert_eq!(
            base_url_of("http://localhost:5432/"),
            "http://localhost:5432"
        );
        // pseudo/local fallback unchanged
        assert_eq!(base_url_of("packs/strudel.json"), "packs");
    }

    #[test]
    fn local_scheme_resolves_to_the_sampler_dev_server() {
        // `@strudel/sampler` serves its banks JSON at http://localhost:5432, and
        // superdough's `local:` shorthand maps the whole url there regardless of
        // any suffix.
        assert_eq!(resolve_special_paths("local:"), "http://localhost:5432");
        assert_eq!(resolve_special_paths("local:foo"), "http://localhost:5432");
    }

    #[test]
    fn shabda_schemes_resolve_to_the_shabda_service() {
        assert_eq!(
            resolve_special_paths("shabda:cat dog"),
            "https://shabda.ndre.gr/cat dog.json?strudel=1"
        );
        // speech with defaults
        assert_eq!(
            resolve_special_paths("shabda/speech:hello"),
            "https://shabda.ndre.gr/speech/hello.json?gender=f&language=en-GB&strudel=1"
        );
        // speech with an explicit <lang>/<gender> segment
        assert_eq!(
            resolve_special_paths("shabda/speech/de-DE/m:hallo"),
            "https://shabda.ndre.gr/speech/hallo.json?gender=m&language=de-DE&strudel=1"
        );
    }

    #[test]
    fn join_url_inserts_one_separator() {
        assert_eq!(join_url("base/", "a.wav"), "base/a.wav");
        assert_eq!(join_url("base", "a.wav"), "base/a.wav");
        assert_eq!(join_url("base", "/a.wav"), "base/a.wav");
        assert_eq!(join_url("", "a.wav"), "a.wav");
        assert_eq!(join_url("base", "https://x/a.wav"), "https://x/a.wav");
    }

    #[test]
    fn join_url_escapes_http_sample_paths() {
        assert_eq!(
            join_url(
                "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/dr55",
                "000_DR55 hi hat.wav"
            ),
            "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/dr55/000_DR55%20hi%20hat.wav"
        );
        assert_eq!(
            join_url(
                "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/h",
                "001_0_da0-200%_1000_0_R.wav"
            ),
            "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/h/001_0_da0-200%25_1000_0_R.wav"
        );
        assert_eq!(
            join_url(
                "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/mute",
                "000_FH A#2 SCF.wav"
            ),
            "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/mute/000_FH%20A%232%20SCF.wav"
        );
        assert_eq!(
            join_url(
                "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/mute",
                "000_FH%20A%232%20SCF.wav"
            ),
            "https://raw.githubusercontent.com/tidalcycles/Dirt-Samples/master/mute/000_FH%20A%232%20SCF.wav"
        );
    }

    #[test]
    fn absolute_http_values_are_escaped_without_touching_query() {
        assert_eq!(
            join_url("", "https://example.com/samples/a hat.wav?raw=1"),
            "https://example.com/samples/a%20hat.wav?raw=1"
        );
    }

    #[test]
    fn parses_array_and_string_forms_with_base() {
        let json = r#"{ "_base": "https://x.com/s/", "bd": ["808bd/a.wav", "808bd/b.wav"], "sd": "808sd/c.wav" }"#;
        let mut entries = parse_sample_map(json, "ignored").unwrap();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            entries,
            vec![
                (
                    "bd".to_string(),
                    SoundFiles::Flat(vec![
                        "https://x.com/s/808bd/a.wav".to_string(),
                        "https://x.com/s/808bd/b.wav".to_string(),
                    ])
                ),
                (
                    "sd".to_string(),
                    SoundFiles::Flat(vec!["https://x.com/s/808sd/c.wav".to_string()])
                ),
            ]
        );
    }

    #[test]
    fn note_keyed_objects_become_pitched_groups() {
        // c4 -> MIDI 60, e4 -> 64 (note_to_midi default octave 3 => c4 = 60).
        let json = r#"{ "piano": { "c4": "c4.wav", "e4": ["e4a.wav", "e4b.wav"] } }"#;
        let entries = parse_sample_map(json, "base").unwrap();
        assert_eq!(entries.len(), 1);
        let (name, files) = entries.into_iter().next().unwrap();
        assert_eq!(name, "piano");
        let SoundFiles::Pitched(mut groups) = files else {
            panic!("expected a pitched map, got {files:?}");
        };
        groups.sort_by_key(|(midi, _)| *midi);
        assert_eq!(
            groups,
            vec![
                (60, vec!["base/c4.wav".to_string()]),
                (
                    64,
                    vec!["base/e4a.wav".to_string(), "base/e4b.wav".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn github_base_in_json_is_expanded() {
        let json = r#"{ "_base": "github:me/pack", "bd": "bd.wav" }"#;
        let entries = parse_sample_map(json, "").unwrap();
        assert_eq!(
            entries[0].1,
            SoundFiles::Flat(vec![
                "https://raw.githubusercontent.com/me/pack/main/bd.wav".to_string()
            ])
        );
    }
}
