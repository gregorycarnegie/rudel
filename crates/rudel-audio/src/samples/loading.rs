//! Sample-map resolution, file/network loading, and download caching.

use super::{SampleBank, decoding::decode_sample_bytes};
use crate::sample_map;
use rudel_dsp::{Sample, WaveTable};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// A sample parsed and loaded, but not yet merged into a [`SampleBank`].
pub(crate) struct LoadedSample {
    name: String,
    note: Option<i32>,
    sample: Arc<Sample>,
}

impl SampleBank {
    /// Load a single audio file and register it under `name`.
    pub fn load_file(&mut self, name: &str, path: &Path) -> Result<(), String> {
        let sample = load_sample(path)?;
        self.register(name, Arc::new(sample));
        Ok(())
    }

    /// Load a directory of samples: each immediate subdirectory becomes a sound
    /// name, and the audio files within (sorted) become its indices. Returns the
    /// number of samples loaded.
    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, String> {
        let loaded = Self::load_dir_entries(dir)?;
        Ok(self.extend_loaded(loaded))
    }

    /// Scans a directory and returns loaded sample data from immediate subdirectories.
    pub(crate) fn load_dir_entries(dir: &Path) -> Result<Vec<LoadedSample>, String> {
        let mut sample_dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("read_dir {dir:?}: {e}"))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        sample_dirs.sort();

        let mut jobs = Vec::new();
        for path in sample_dirs {
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let mut files: Vec<_> = std::fs::read_dir(&path)
                .map_err(|e| format!("read_dir {path:?}: {e}"))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| is_audio_file(p))
                .collect();
            files.sort();
            jobs.extend(files.into_iter().map(|file| (name.clone(), file)));
        }

        // Decode in parallel (CPU-bound), one worker per core.
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
        let decoded = parallel_map(jobs, workers, |(_, file)| load_sample(file));
        Ok(decoded
            .into_iter()
            .filter_map(|((name, _), sample)| {
                sample.ok().map(|sample| LoadedSample {
                    name,
                    note: None,
                    sample: Arc::new(sample),
                })
            })
            .collect())
    }

    /// Merges loaded samples into this bank, returning the count of added samples.
    pub(crate) fn extend_loaded(&mut self, loaded: Vec<LoadedSample>) -> usize {
        let count = loaded.len();
        for LoadedSample { name, note, sample } in loaded {
            match note {
                Some(midi) => self.register_note(&name, midi, sample),
                None => self.register(&name, sample),
            }
        }
        count
    }
}

impl SampleBank {
    /// The `samples(...)` loader. `source` may be:
    /// - a `github:user/repo[/branch]` or `bubo:pack` pseudo-URL,
    /// - an http(s) URL to a `strudel.json` sample map,
    /// - a local path to a `.json` sample map, or
    /// - a local directory of sample folders (delegates to [`load_dir`]).
    ///
    /// Returns the number of samples registered.
    ///
    /// [`load_dir`]: SampleBank::load_dir
    pub fn load_samples_source(&mut self, source: &str) -> Result<usize, String> {
        let loaded = Self::load_samples_source_entries(source)?;
        Ok(self.extend_loaded(loaded))
    }

    /// Resolves the sample source (JSON, URL, directory) into loaded sample records.
    pub(crate) fn load_samples_source_entries(source: &str) -> Result<Vec<LoadedSample>, String> {
        let resolved = sample_map::resolve_special_paths(source.trim());
        // github: bases point at the repo's strudel.json.
        let url = if resolved.starts_with("github:") {
            sample_map::github_path(&resolved, "strudel.json")?
        } else {
            resolved
        };

        if is_http(&url) {
            let json = fetch_text(&url)?;
            let base = sample_map::base_url_of(&url);
            return Self::load_sample_map_entries(&json, &base);
        }

        // Local path: expand a leading `~` to the user's home directory.
        let url = expand_home(&url);
        let path = Path::new(&url);
        if path.is_dir() {
            return Self::load_dir_entries(path);
        }
        if path.is_file() {
            let json = std::fs::read_to_string(path).map_err(|e| format!("read {url}: {e}"))?;
            let base = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            return Self::load_sample_map_entries(&json, &base);
        }
        Err(format!(
            "samples: not a URL, .json file, or directory: {url}"
        ))
    }

    /// Load a Strudel-format sample map (the contents of a `strudel.json`).
    /// `base` resolves relative file paths (a `_base` key in the JSON overrides
    /// it). Each referenced file is fetched (http(s)) or read from disk,
    /// decoded, and registered under its sound name. Files that fail to load are
    /// logged and skipped. Returns the number of samples registered.
    pub fn load_sample_map(&mut self, json: &str, base: &str) -> Result<usize, String> {
        let loaded = Self::load_sample_map_entries(json, base)?;
        Ok(self.extend_loaded(loaded))
    }

    /// Parses and downloads/reads all files in a sample map JSON content.
    pub(crate) fn load_sample_map_entries(
        json: &str,
        base: &str,
    ) -> Result<Vec<LoadedSample>, String> {
        use sample_map::SoundFiles;

        // A fetch job: sound name, optional MIDI tuning (pitched maps), and URL.
        type Job = (String, Option<i32>, String);

        // Flatten the map into jobs in declaration order so `n` indices stay
        // stable after the parallel fetch.
        let mut jobs: Vec<Job> = Vec::new();
        for (name, files) in sample_map::parse_sample_map(json, base)? {
            match files {
                SoundFiles::Flat(urls) => {
                    jobs.extend(urls.into_iter().map(|u| (name.clone(), None, u)));
                }
                SoundFiles::Pitched(groups) => {
                    for (midi, urls) in groups {
                        jobs.extend(urls.into_iter().map(|u| (name.clone(), Some(midi), u)));
                    }
                }
            }
        }

        // Fetch + decode in parallel; downloads are I/O-bound so the pool is
        // wider than the CPU count.
        let decoded = parallel_map(jobs, 16, |job| fetch_and_decode(&job.2));

        let mut loaded = Vec::new();
        for ((name, note, _), sample) in decoded {
            match sample {
                Ok(s) => loaded.push(LoadedSample {
                    name,
                    note,
                    sample: Arc::new(s),
                }),
                Err(e) => eprintln!("[rudel-audio] sample {name:?}: {e}"),
            }
        }
        Ok(loaded)
    }

    /// Register a sample source's *names* without downloading any audio, which
    /// is what Strudel's `prebake` does: the map is a list of URLs, and the
    /// browser only fetches a file the first time something plays it.
    ///
    /// Fetching eagerly instead means a startup that downloads every bank in
    /// full — measured at 3.1 GB and about nine minutes for the seven maps the
    /// REPL preloads, nearly all of it audio nobody asked to hear. Returns the
    /// number of sounds registered.
    pub fn register_samples_source(&mut self, source: &str) -> Result<usize, String> {
        use sample_map::SoundFiles;

        let (json, base) = Self::resolve_map_source(source)?;
        let mut count = 0;
        for (name, files) in sample_map::parse_sample_map(&json, &base)? {
            let files: Vec<(Option<i32>, String)> = match files {
                SoundFiles::Flat(urls) => urls.into_iter().map(|u| (None, u)).collect(),
                SoundFiles::Pitched(groups) => groups
                    .into_iter()
                    .flat_map(|(midi, urls)| {
                        urls.into_iter()
                            .map(move |u| (Some(midi), u))
                            .collect::<Vec<_>>()
                    })
                    .collect(),
            };
            if files.is_empty() {
                continue;
            }
            self.register_pending(&name, files);
            count += 1;
        }
        Ok(count)
    }

    /// Download and decode the files of one pending sound. Takes the list
    /// rather than reading it from a bank so the caller can hold its lock for
    /// as little as possible — the audio thread reads that bank continuously.
    pub(crate) fn fetch_pending_entries(
        name: &str,
        files: Vec<(Option<i32>, String)>,
    ) -> Vec<LoadedSample> {
        let decoded = parallel_map(files, 16, |(_, url)| fetch_and_decode(url));
        let mut loaded = Vec::new();
        for ((note, url), sample) in decoded {
            match sample {
                Ok(s) => loaded.push(LoadedSample {
                    name: name.to_string(),
                    note,
                    sample: Arc::new(s),
                }),
                Err(e) => eprintln!("[rudel-audio] sample {name:?} ({url}): {e}"),
            }
        }
        loaded
    }

    /// Download one pending sound into this bank. The in-process convenience
    /// form of [`fetch_pending_entries`](Self::fetch_pending_entries); the
    /// engine splits the two around its lock.
    pub fn load_pending(&mut self, name: &str) -> Result<usize, String> {
        let Some(files) = self.pending_files(name) else {
            return Ok(0);
        };
        let loaded = Self::fetch_pending_entries(name, files);
        let count = self.extend_loaded(loaded);
        // Cleared either way: a sound whose files all 404 must not be retried
        // on every event for the rest of the session.
        self.clear_pending(name);
        Ok(count)
    }

    /// The `strudel.json` text and base URL behind a sample source.
    fn resolve_map_source(source: &str) -> Result<(String, String), String> {
        let resolved = sample_map::resolve_special_paths(source.trim());
        let url = if resolved.starts_with("github:") {
            sample_map::github_path(&resolved, "strudel.json")?
        } else {
            resolved
        };
        if is_http(&url) {
            let json = fetch_text(&url)?;
            let base = sample_map::base_url_of(&url);
            return Ok((json, base));
        }
        let url = expand_home(&url);
        let path = Path::new(&url);
        if path.is_file() {
            let json = std::fs::read_to_string(path).map_err(|e| format!("read {url}: {e}"))?;
            let base = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            return Ok((json, base));
        }
        Err(format!("samples: not a URL or .json file: {url}"))
    }

    /// Load a wavetable collection (`tables(source, frameLen)`), porting
    /// `wavetable.mjs`'s `tables`/`_processTables`: the source is resolved the
    /// same way `samples()` resolves one, each entry's files are fetched and
    /// decoded, and each buffer is sliced into `frame_len`-sample single-cycle
    /// frames. Non-`.wav` entries are skipped with a note, as upstream does.
    pub(crate) fn load_tables_entries(
        source: &str,
        frame_len: usize,
    ) -> Result<Vec<(String, WaveTable)>, String> {
        use sample_map::SoundFiles;

        let resolved = sample_map::resolve_special_paths(source.trim());
        let (json, base) = if resolved.starts_with("github:") {
            let url = sample_map::github_path(&resolved, "strudel.json")?;
            let base = sample_map::base_url_of(&url);
            (fetch_text(&url)?, base)
        } else if is_http(&resolved) {
            let base = sample_map::base_url_of(&resolved);
            (fetch_text(&resolved)?, base)
        } else {
            let path = expand_home(&resolved);
            let path = Path::new(&path);
            let json = std::fs::read_to_string(path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            let base = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            (json, base)
        };

        let mut jobs: Vec<(String, String)> = Vec::new();
        for (name, files) in sample_map::parse_sample_map(&json, &base)? {
            let urls = match files {
                SoundFiles::Flat(urls) => urls,
                // Wavetables are a flat list per name; a note-keyed map has no
                // meaning here, so its groups are flattened in order.
                SoundFiles::Pitched(groups) => {
                    groups.into_iter().flat_map(|(_, urls)| urls).collect()
                }
            };
            for url in urls {
                if !url.to_lowercase().ends_with(".wav") {
                    eprintln!("[rudel-audio] wavetable {url:?}: must be .wav, skipping");
                    continue;
                }
                jobs.push((name.clone(), url));
            }
        }

        let decoded = parallel_map(jobs, 16, |(_, url)| fetch_and_decode(url));
        let mut tables = Vec::new();
        for ((name, _), sample) in decoded {
            match sample {
                Ok(s) => tables.push((name, WaveTable::from_samples(&s.data, frame_len))),
                Err(e) => eprintln!("[rudel-audio] wavetable {name:?}: {e}"),
            }
        }
        Ok(tables)
    }
}

/// Helper to determine if a URL scheme represents HTTP or HTTPS.
pub(super) fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Run `work` over `jobs` on a small worker pool, returning `(job, result)`
/// pairs in job order.
fn parallel_map<J: Send + Sync, R: Send>(
    jobs: Vec<J>,
    workers: usize,
    work: impl Fn(&J) -> R + Sync,
) -> Vec<(J, R)> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = workers.clamp(1, jobs.len().max(1));
    let (tx, rx) = mpsc::channel();
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (next, jobs, work) = (&next, &jobs, &work);
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let _ = tx.send((i, work(job)));
                }
            });
        }
    });
    drop(tx);
    let mut results: Vec<(usize, R)> = rx.into_iter().collect();
    results.sort_unstable_by_key(|(i, _)| *i);
    jobs.into_iter()
        .zip(results.into_iter().map(|(_, result)| result))
        .collect()
}

/// On-disk cache location for a downloaded sample, keyed by URL hash — the
/// native analogue of the browser HTTP cache that makes Strudel's repeat
/// sample loads instant. Raw bytes are cached (not decoded audio) so format
/// sniffing in `decode_sample_bytes` still applies. The sample-map JSON is
/// deliberately *not* cached, so updated remote maps are always picked up.
fn cache_path(url: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    // ponytail: DefaultHasher isn't stable across Rust releases; a toolchain
    // bump just re-downloads the cache once.
    let mut hasher = std::hash::DefaultHasher::new();
    url.hash(&mut hasher);
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?
        .join("rudel")
        .join("sample-cache");
    Some(dir.join(format!("{:016x}", hasher.finish())))
}

/// Expand a leading `~` (or `~/`) in a local path to the user's home directory.
/// Returns the input unchanged if there's no home directory or no `~` prefix.
pub(super) fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    match home {
        Some(home) => {
            let rest = rest.strip_prefix(['/', '\\']).unwrap_or(rest);
            if rest.is_empty() {
                home
            } else {
                format!("{home}/{rest}")
            }
        }
        None => path.to_string(),
    }
}

/// Fetch a text resource (a sample-map JSON) over http(s).
pub(super) fn fetch_text(url: &str) -> Result<String, String> {
    let mut resp = ureq::get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    resp.body_mut()
        .read_to_string()
        .map_err(|e| format!("read body {url}: {e}"))
}

/// Fetch a text file (http(s) URL or local path), caching HTTP responses on
/// disk like the sample downloads. Preset files are ~1MB each, so a font is
/// fetched once per machine rather than once per session.
pub(crate) fn fetch_cached_text(url: &str) -> Result<String, String> {
    if !is_http(url) {
        return std::fs::read_to_string(url).map_err(|e| format!("read {url}: {e}"));
    }
    let cache = cache_path(url);
    if let Some(path) = &cache
        && let Ok(text) = std::fs::read_to_string(path)
    {
        return Ok(text);
    }
    let text = fetch_text(url)?;
    // Best-effort cache write; a failed write just re-downloads next time.
    if let Some(path) = &cache
        && let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::write(path, &text);
    }
    Ok(text)
}

/// Decode in-memory audio bytes (a soundfont zone's payload, say).
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<Sample, String> {
    decode_sample_bytes(bytes.to_vec())
}

/// Fetch a binary file (http(s) URL or local path), caching HTTP responses on
/// disk. Used for sample files and for `.sf2` SoundFonts, which are large
/// enough to be worth fetching once per machine.
pub(crate) fn fetch_cached_bytes(url: &str) -> Result<Vec<u8>, String> {
    if !is_http(url) {
        let path = expand_home(url);
        return std::fs::read(&path).map_err(|e| format!("read {path}: {e}"));
    }
    let cache = cache_path(url);
    if let Some(path) = &cache
        && let Ok(bytes) = std::fs::read(path)
    {
        return Ok(bytes);
    }
    use std::io::Read;
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    // `into_reader()` streams without the 10MB cap that `read_to_vec()` has.
    let mut bytes = Vec::new();
    resp.into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read {url}: {e}"))?;
    // Best-effort cache write; a failed write just re-downloads next time.
    if let Some(path) = &cache
        && let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::write(path, &bytes);
    }
    Ok(bytes)
}

/// Fetch a single sample file (http(s) URL or local path) and decode it.
pub(super) fn fetch_and_decode(url: &str) -> Result<Sample, String> {
    if is_http(url) {
        decode_sample_bytes(fetch_cached_bytes(url)?)
    } else {
        load_sample(Path::new(url))
    }
}

/// Helper to check if a file extension represents a supported audio format.
fn is_audio_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("wav" | "flac" | "ogg" | "mp3" | "aiff" | "aif")
    )
}

/// Decode an audio file into a mono [`Sample`] (channels are averaged).
pub(super) fn load_sample(path: &Path) -> Result<Sample, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
    decode_sample_bytes(bytes).map_err(|e| format!("load {path:?}: {e}"))
}
