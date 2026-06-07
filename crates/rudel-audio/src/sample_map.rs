// sample_map.rs - parsing Strudel-format sample maps (`strudel.json`) and
// resolving their special URL schemes. Mirrors the pure parts of
// strudel/packages/superdough/sampler.mjs (processSampleMap / githubPath /
// resolveSpecialPaths). Network fetching and decoding live in samples.rs.
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value as Json;

/// Resolve Strudel's shorthand bases to a concrete pseudo/real URL.
/// `bubo:foo` expands to `github:Bubobubobubobubo/dough-foo`.
pub(crate) fn resolve_special_paths(base: &str) -> String {
    if let Some(repo) = base.strip_prefix("bubo:") {
        return format!("github:Bubobubobubobubo/dough-{repo}");
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
/// slash), e.g. `.../packs/strudel.json` -> `.../packs`.
pub(crate) fn base_url_of(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..i].to_string(),
        None => String::new(),
    }
}

/// Join a base URL/dir with a (possibly relative) file path, inserting exactly
/// one separator. Absolute http(s) values are returned unchanged.
pub(crate) fn join_url(base: &str, v: &str) -> String {
    if v.starts_with("http://") || v.starts_with("https://") {
        return v.to_string();
    }
    if base.is_empty() {
        return v.to_string();
    }
    let sep = if base.ends_with('/') || v.starts_with('/') {
        ""
    } else {
        "/"
    };
    format!("{base}{sep}{v}")
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

/// Pull the file paths out of a sample-map value (string / array / note-keyed
/// object), returning them in declaration order. Non-string leaves are skipped.
fn collect_files(value: &Json) -> Vec<String> {
    match value {
        Json::String(s) => vec![s.clone()],
        Json::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        // Note-keyed object (pitched sample map): flatten every note's files
        // into a flat index list. Pitch-based selection is not yet applied.
        Json::Object(map) => map
            .iter()
            .filter(|(k, _)| *k != "_base")
            .flat_map(|(_, v)| collect_files(v))
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse a Strudel sample-map JSON into `(sound_name, resolved_urls)` entries.
/// `base` resolves relative file paths; a top-level or per-entry `_base` key
/// overrides it. Handles the string, array, and note-keyed-object value forms.
pub(crate) fn parse_sample_map(
    json: &str,
    base: &str,
) -> Result<Vec<(String, Vec<String>)>, String> {
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

        let urls = collect_files(value)
            .iter()
            .map(|f| join_url(&entry_base, f))
            .collect();
        entries.push((key.clone(), urls));
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
    fn join_url_inserts_one_separator() {
        assert_eq!(join_url("base/", "a.wav"), "base/a.wav");
        assert_eq!(join_url("base", "a.wav"), "base/a.wav");
        assert_eq!(join_url("base", "/a.wav"), "base/a.wav");
        assert_eq!(join_url("", "a.wav"), "a.wav");
        assert_eq!(join_url("base", "https://x/a.wav"), "https://x/a.wav");
    }

    #[test]
    fn parses_array_and_string_forms_with_base() {
        let json = r#"{ "_base": "https://x.com/s/", "bd": ["808bd/a.wav", "808bd/b.wav"], "sd": "808sd/c.wav" }"#;
        let mut entries = parse_sample_map(json, "ignored").unwrap();
        entries.sort();
        assert_eq!(
            entries,
            vec![
                (
                    "bd".to_string(),
                    vec![
                        "https://x.com/s/808bd/a.wav".to_string(),
                        "https://x.com/s/808bd/b.wav".to_string(),
                    ]
                ),
                (
                    "sd".to_string(),
                    vec!["https://x.com/s/808sd/c.wav".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn flattens_note_keyed_objects() {
        let json = r#"{ "piano": { "c4": "c4.wav", "e4": ["e4a.wav", "e4b.wav"] } }"#;
        let entries = parse_sample_map(json, "base").unwrap();
        assert_eq!(entries.len(), 1);
        let (name, mut urls) = entries.into_iter().next().unwrap();
        assert_eq!(name, "piano");
        urls.sort();
        assert_eq!(urls, vec!["base/c4.wav", "base/e4a.wav", "base/e4b.wav"]);
    }

    #[test]
    fn github_base_in_json_is_expanded() {
        let json = r#"{ "_base": "github:me/pack", "bd": "bd.wav" }"#;
        let entries = parse_sample_map(json, "").unwrap();
        assert_eq!(
            entries[0].1[0],
            "https://raw.githubusercontent.com/me/pack/main/bd.wav"
        );
    }
}
