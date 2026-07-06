// samples.rs - a bank of decoded audio samples, keyed by sound name and index.
// Decoding uses fundsp's Wave (Symphonia under the hood).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::sample_map;
use fundsp::wave::Wave;
use rudel_dsp::Sample;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

/// A group of samples sharing one tuning. Flat (drum-machine) sounds use a
/// single group with `note: None`; pitched (note-keyed) maps have one group per
/// note name, used to pick the closest sample and repitch it.
struct SampleGroup {
    /// MIDI note this group is tuned to, or `None` for an un-pitched sound.
    note: Option<i32>,
    /// The decoded audio samples in this group.
    samples: Vec<Arc<Sample>>,
}

/// Maps a sound name (e.g. `"bd"`) to its sample group(s).
#[derive(Default)]
pub struct SampleBank {
    map: HashMap<String, Vec<SampleGroup>>,
    /// Bank aliases (`alias -> canonical`), so `s("bd").bank("tr909")` can find
    /// a pack registered as `RolandTR909_bd`. See [`alias_bank`](Self::alias_bank).
    bank_aliases: HashMap<String, String>,
}

/// A sample structure parsed and loaded, but not yet merged into a SampleBank.
pub(crate) struct LoadedSample {
    /// Name of the sound trigger (e.g. "bd").
    name: String,
    /// MIDI note tuning of the sample, if note-keyed.
    note: Option<i32>,
    /// Decoded audio sample data.
    sample: Arc<Sample>,
}

impl SampleBank {
    /// Create a new empty `SampleBank`.
    pub fn new() -> SampleBank {
        SampleBank::default()
    }

    /// Add an un-pitched sample under `name` (appended as the next `n` index).
    pub fn register(&mut self, name: &str, sample: Arc<Sample>) {
        self.push_into(name, None, sample);
    }

    /// Add a sample tuned to `note` (a MIDI number) under `name`, for pitched
    /// note-keyed sample maps.
    pub fn register_note(&mut self, name: &str, note: i32, sample: Arc<Sample>) {
        self.push_into(name, Some(note), sample);
    }

    /// Internal helper to push a sample into the corresponding group.
    fn push_into(&mut self, name: &str, note: Option<i32>, sample: Arc<Sample>) {
        let groups = self.map.entry(name.to_string()).or_default();
        match groups.iter_mut().find(|g| g.note == note) {
            Some(g) => g.samples.push(sample),
            None => groups.push(SampleGroup {
                note,
                samples: vec![sample],
            }),
        }
    }

    /// Check if the bank contains any samples for the given sound name.
    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// Register a bank alias: a sound pack loaded as `<canonical>_<sound>` also
    /// becomes reachable via `<alias>_<sound>`. Mirrors Strudel's `aliasBank`
    /// (e.g. `alias_bank("RolandTR909", "tr909")`). Case-insensitive on `alias`.
    pub fn alias_bank(&mut self, canonical: &str, alias: &str) {
        self.bank_aliases
            .insert(alias.to_string(), canonical.to_string());
        self.bank_aliases
            .insert(alias.to_lowercase(), canonical.to_string());
    }

    /// Resolve a bank name through the alias map (returns the input unchanged if
    /// it isn't an alias).
    pub fn canonical_bank<'a>(&'a self, bank: &'a str) -> &'a str {
        self.bank_aliases
            .get(bank)
            .or_else(|| self.bank_aliases.get(&bank.to_lowercase()))
            .map(String::as_str)
            .unwrap_or(bank)
    }

    /// All registered sound names, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.map.keys().cloned().collect();
        names.sort();
        names
    }

    /// Fetch the `index`-th sample for `name` (wrapping if out of range),
    /// ignoring pitch. Equivalent to [`resolve`](Self::resolve) with no note.
    pub fn get(&self, name: &str, index: i64) -> Option<Arc<Sample>> {
        self.resolve(name, index, None).map(|(s, _)| s)
    }

    /// Resolve a sample for playback. `index` is the `n` sample index; `midi` is
    /// the requested MIDI note (from `note`/`freq`), or `None` if unset.
    ///
    /// Returns the chosen sample and the repitch in semitones to apply:
    /// - un-pitched sounds repitch relative to C3 (MIDI 36) only when a note is
    ///   requested (so drums without `note` are untouched);
    /// - note-keyed maps pick the group whose tuning is closest to `midi` and
    ///   repitch that sample onto the requested note.
    ///
    /// Mirrors superdough's `getCommonSampleInfo`. `index` is the (already
    /// rounded) `n` sample index and wraps euclidean-modulo over the chosen
    /// group's length, so a negative `n` selects from the end — matching
    /// superdough's `getSoundIndex` (`_mod(Math.round(n), numSounds)`).
    pub fn resolve(&self, name: &str, index: i64, midi: Option<f64>) -> Option<(Arc<Sample>, f64)> {
        let groups = self.map.get(name)?;
        if groups.iter().any(|g| g.note.is_some()) {
            // Pitched map: pick the closest tuned group (fallback target C3=36).
            let target = midi.unwrap_or(36.0);
            let group = groups
                .iter()
                .filter(|g| g.note.is_some() && !g.samples.is_empty())
                .min_by(|a, b| {
                    let da = (a.note.unwrap() as f64 - target).abs();
                    let db = (b.note.unwrap() as f64 - target).abs();
                    da.total_cmp(&db)
                })?;
            let sample = group.samples[wrap_index(index, group.samples.len())].clone();
            Some((sample, target - group.note.unwrap() as f64))
        } else {
            // Flat: index into the un-pitched group; repitch vs C3 if note set.
            let group = groups.iter().find(|g| !g.samples.is_empty())?;
            let sample = group.samples[wrap_index(index, group.samples.len())].clone();
            Some((sample, midi.map(|m| m - 36.0).unwrap_or(0.0)))
        }
    }

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
}

/// Helper to determine if a URL scheme represents HTTP or HTTPS.
fn is_http(url: &str) -> bool {
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

/// Wrap a (signed) sample index into `0..len`, euclidean-modulo, so negative
/// indices count from the end. Mirrors `_mod` in superdough's `getSoundIndex`.
/// `len` is assumed non-zero (callers only pass non-empty groups).
fn wrap_index(index: i64, len: usize) -> usize {
    index.rem_euclid(len as i64) as usize
}

/// Expand a leading `~` (or `~/`) in a local path to the user's home directory.
/// Returns the input unchanged if there's no home directory or no `~` prefix.
fn expand_home(path: &str) -> String {
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
fn fetch_text(url: &str) -> Result<String, String> {
    let mut resp = ureq::get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    resp.body_mut()
        .read_to_string()
        .map_err(|e| format!("read body {url}: {e}"))
}

/// Fetch a single sample file (http(s) URL or local path) and decode it.
fn fetch_and_decode(url: &str) -> Result<Sample, String> {
    if is_http(url) {
        let cache = cache_path(url);
        if let Some(path) = &cache
            && let Ok(bytes) = std::fs::read(path)
        {
            return decode_sample_bytes(bytes);
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
        decode_sample_bytes(bytes)
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
fn load_sample(path: &Path) -> Result<Sample, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
    decode_sample_bytes(bytes).map_err(|e| format!("load {path:?}: {e}"))
}

/// Decode in-memory audio bytes into a mono [`Sample`]. Symphonia (via fundsp)
/// handles all formats; WAVs it rejects fall back to our lenient in-house
/// reader, since old sample packs (e.g. dirt-samples' `mute`/`pluck`) have
/// nonstandard 20-byte PCM fmt chunks symphonia refuses to parse.
fn decode_sample_bytes(bytes: Vec<u8>) -> Result<Sample, String> {
    let is_wav = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WAVE"[..]);
    let bytes: std::sync::Arc<[u8]> = bytes.into();
    match Wave::load_slice(bytes.clone()) {
        Ok(wave) => Ok(wave_to_sample(&wave)),
        Err(e) if is_wav => {
            decode_wav_lenient(&bytes).map_err(|e2| format!("decode audio: {e}; lenient wav: {e2}"))
        }
        Err(e) => Err(format!("decode audio: {e}")),
    }
}

/// Fallback lenient WAV decode (replaces the archived `wavers` crate): skips
/// unknown chunks, tolerates oversized fmt chunks and truncated data, and
/// handles 8/16/24/32-bit PCM plus 32/64-bit IEEE float.
fn decode_wav_lenient(bytes: &[u8]) -> Result<Sample, String> {
    let mut fmt: Option<(u16, usize, f32, u16)> = None; // (tag, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let u16_at = |o: usize| u16::from_le_bytes(body[o..o + 2].try_into().unwrap());
                let mut tag = u16_at(0);
                // WAVE_FORMAT_EXTENSIBLE: real format is the first word of the sub-format GUID
                if tag == 0xFFFE && body.len() >= 26 {
                    tag = u16_at(24);
                }
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap()) as f32;
                fmt = Some((tag, u16_at(2).max(1) as usize, rate, u16_at(14)));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + size + (size & 1); // chunks are word-aligned
    }
    let (tag, channels, sample_rate, bits) = fmt.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    let samples: Vec<f32> = match (tag, bits) {
        (1, 8) => data.iter().map(|&v| (v as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .chunks_exact(3)
            .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
            .collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect(),
        (3, 64) => data
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()) as f32)
            .collect(),
        _ => return Err(format!("unsupported wav format: tag {tag}, {bits}-bit")),
    };
    let data = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(Sample { data, sample_rate })
}

/// Average a decoded [`Wave`]'s channels down to a mono [`Sample`].
fn wave_to_sample(wave: &Wave) -> Sample {
    let channels = wave.channels().max(1);
    let len = wave.len();
    let mut data = Vec::with_capacity(len);
    for i in 0..len {
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += wave.at(c, i);
        }
        data.push(sum / channels as f32);
    }
    Sample {
        data,
        sample_rate: wave.sample_rate() as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Write a minimal 16-bit mono PCM WAV so we can exercise the real decoder.
    fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        let data_len = (samples.len() * 2) as u32;
        let byte_rate = sample_rate * 2;
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
        f.write_all(&16u16.to_le_bytes()).unwrap(); // bits
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            f.write_all(&v.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn loads_a_wav_file() {
        let dir = std::env::temp_dir().join("rudel_sample_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tone.wav");
        let samples: Vec<f32> = (0..4410)
            .map(|i| (TAU * 220.0 * i as f32 / 44100.0).sin())
            .collect();
        write_wav(&path, &samples, 44100);

        let mut bank = SampleBank::new();
        bank.load_file("tone", &path).expect("load wav");
        let s = bank.get("tone", 0).expect("sample present");
        assert_eq!(s.sample_rate, 44100.0);
        assert!(s.data.len() > 4000);
        assert!(s.data.iter().any(|&x| x.abs() > 0.1));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn decodes_wav_with_nonstandard_fmt_chunk() {
        // 20-byte PCM fmt chunk (16 + 4 junk bytes), as found in dirt-samples'
        // "mute"/"pluck" banks; symphonia rejects it, the lenient fallback must not.
        let samples: Vec<f32> = (0..64).map(|i| (TAU * i as f32 / 64.0).sin()).collect();
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((40 + data_len).to_le_bytes());
        b.extend(b"WAVE");
        b.extend(b"fmt ");
        b.extend(20u32.to_le_bytes());
        b.extend(1u16.to_le_bytes()); // PCM
        b.extend(1u16.to_le_bytes()); // mono
        b.extend(44100u32.to_le_bytes());
        b.extend((44100u32 * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes()); // block align
        b.extend(16u16.to_le_bytes()); // bits
        b.extend([0u8; 4]); // the nonstandard trailing bytes
        b.extend(b"data");
        b.extend(data_len.to_le_bytes());
        for &s in &samples {
            b.extend(((s * 32767.0) as i16).to_le_bytes());
        }

        let s = decode_sample_bytes(b).expect("lenient wav fallback decodes");
        assert_eq!(s.sample_rate, 44100.0);
        assert_eq!(s.data.len(), 64);
        assert!((s.data[16] - 1.0).abs() < 1e-2); // sin peak survives roundtrip
    }

    #[test]
    fn lenient_decoder_handles_stereo_float32() {
        // Exercises the IEEE-float branch and channel averaging directly.
        let frames: Vec<(f32, f32)> = (0..32)
            .map(|i| (i as f32 / 32.0, -(i as f32) / 32.0))
            .collect();
        let data_len = (frames.len() * 8) as u32;
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + data_len).to_le_bytes());
        b.extend(b"WAVE");
        b.extend(b"fmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(3u16.to_le_bytes()); // IEEE float
        b.extend(2u16.to_le_bytes()); // stereo
        b.extend(48000u32.to_le_bytes());
        b.extend((48000u32 * 8).to_le_bytes());
        b.extend(8u16.to_le_bytes()); // block align
        b.extend(32u16.to_le_bytes()); // bits
        b.extend(b"data");
        b.extend(data_len.to_le_bytes());
        for &(l, r) in &frames {
            b.extend(l.to_le_bytes());
            b.extend(r.to_le_bytes());
        }

        let s = decode_wav_lenient(&b).expect("float32 stereo decodes");
        assert_eq!(s.sample_rate, 48000.0);
        assert_eq!(s.data.len(), 32);
        // L and R are mirrored, so the mono average is ~0 everywhere.
        assert!(s.data.iter().all(|&x| x.abs() < 1e-6));
    }

    #[test]
    fn load_sample_map_reads_local_files() {
        // A strudel.json-style map whose files live in a local base directory.
        let root = std::env::temp_dir().join(format!("rudel_map_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_wav(&root.join("a.wav"), &[0.1; 32], 44100);
        write_wav(&root.join("b.wav"), &[0.2; 32], 44100);
        write_wav(&root.join("c.wav"), &[0.3; 32], 44100);

        let json = r#"{ "bd": ["a.wav", "b.wav"], "sd": "c.wav" }"#;
        let base = root.to_str().unwrap();

        let mut bank = SampleBank::new();
        let count = bank.load_sample_map(json, base).expect("load map");
        assert_eq!(count, 3);
        assert_eq!(bank.get("bd", 0).unwrap().data.len(), 32);
        assert!((bank.get("bd", 1).unwrap().data[0] - 0.2).abs() < 1e-3);
        assert!((bank.get("sd", 0).unwrap().data[0] - 0.3).abs() < 1e-3);
        assert!(bank.get("bd", 2).is_some()); // index wraps over the 2 bd samples

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_samples_source_loads_a_local_json_file() {
        let root = std::env::temp_dir().join(format!("rudel_src_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_wav(&root.join("kick.wav"), &[0.4; 32], 44100);
        let json_path = root.join("strudel.json");
        std::fs::write(&json_path, r#"{ "bd": "kick.wav" }"#).unwrap();

        let mut bank = SampleBank::new();
        let count = bank
            .load_samples_source(json_path.to_str().unwrap())
            .expect("load source");
        assert_eq!(count, 1);
        assert!((bank.get("bd", 0).unwrap().data[0] - 0.4).abs() < 1e-3);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "hits the network (github.com)"]
    fn fetches_parses_and_decodes_a_real_github_pack() {
        // End-to-end smoke test of the ureq fetch + JSON parse + remote decode
        // path against a real repo. Run with `--ignored`. Decodes exactly one
        // file (not the whole pack) to keep it light.
        let url =
            sample_map::github_path("github:tidalcycles/dirt-samples", "strudel.json").unwrap();
        let json = fetch_text(&url).expect("fetch strudel.json");
        let base = sample_map::base_url_of(&url);
        let entries = sample_map::parse_sample_map(&json, &base).expect("parse map");
        assert!(entries.len() > 10, "expected many sounds in the pack");

        let (_, files) = entries
            .iter()
            .find(|(name, _)| name == "bd")
            .expect("a `bd` sound");
        let url = match files {
            sample_map::SoundFiles::Flat(urls) => urls.first().expect("bd files"),
            sample_map::SoundFiles::Pitched(groups) => &groups.first().expect("bd groups").1[0],
        };
        let sample = fetch_and_decode(url).expect("fetch + decode one sample");
        assert!(!sample.data.is_empty(), "decoded sample should have audio");
    }

    #[test]
    fn expand_home_replaces_leading_tilde() {
        // SAFETY: single-threaded test; we set HOME for the duration of the call.
        unsafe { std::env::set_var("HOME", "/home/me") };
        assert_eq!(expand_home("~/samples"), "/home/me/samples");
        assert_eq!(expand_home("~"), "/home/me");
        assert_eq!(expand_home("/abs/path"), "/abs/path");
        assert_eq!(expand_home("relative/path"), "relative/path");
    }

    #[test]
    fn index_wraps() {
        let mut bank = SampleBank::new();
        let mk = |v: f32| {
            Arc::new(Sample {
                data: vec![v],
                sample_rate: 44100.0,
            })
        };
        bank.register("bd", mk(0.1));
        bank.register("bd", mk(0.2));
        assert_eq!(bank.get("bd", 0).unwrap().data[0], 0.1);
        assert_eq!(bank.get("bd", 1).unwrap().data[0], 0.2);
        assert_eq!(bank.get("bd", 2).unwrap().data[0], 0.1); // wraps
        // negative indices count from the end (superdough's `_mod`).
        assert_eq!(bank.get("bd", -1).unwrap().data[0], 0.2);
        assert_eq!(bank.get("bd", -2).unwrap().data[0], 0.1);
        assert!(bank.get("missing", 0).is_none());
    }

    fn mk(v: f32) -> Arc<Sample> {
        Arc::new(Sample {
            data: vec![v],
            sample_rate: 44100.0,
        })
    }

    #[test]
    fn resolve_picks_the_closest_pitched_group() {
        let mut bank = SampleBank::new();
        bank.register_note("piano", 60, mk(0.60)); // c4
        bank.register_note("piano", 64, mk(0.64)); // e4

        // midi 63 -> e4 is closest (dist 1), repitch down one semitone
        let (s, t) = bank.resolve("piano", 0, Some(63.0)).unwrap();
        assert_eq!(s.data[0], 0.64);
        assert_eq!(t, -1.0);

        // midi 61 -> c4 is closest, repitch up one semitone
        let (s, t) = bank.resolve("piano", 0, Some(61.0)).unwrap();
        assert_eq!(s.data[0], 0.60);
        assert_eq!(t, 1.0);

        // no note -> fall back to C3 (36) target -> nearest is c4 (60)
        let (s, t) = bank.resolve("piano", 0, None).unwrap();
        assert_eq!(s.data[0], 0.60);
        assert_eq!(t, 36.0 - 60.0);
    }

    #[test]
    fn flat_sound_repitches_only_when_a_note_is_set() {
        let mut bank = SampleBank::new();
        bank.register("bd", mk(0.5));
        // no note -> no repitch
        assert_eq!(bank.resolve("bd", 0, None).unwrap().1, 0.0);
        // baseline is MIDI 36 (C2); note 36 -> 0, note 48 (C3) -> +12 semitones
        assert_eq!(bank.resolve("bd", 0, Some(36.0)).unwrap().1, 0.0);
        assert_eq!(bank.resolve("bd", 0, Some(48.0)).unwrap().1, 12.0);
    }

    #[test]
    fn load_dir_keeps_sorted_sample_indices() {
        let root =
            std::env::temp_dir().join(format!("rudel_sample_dir_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sound_dir = root.join("tone");
        std::fs::create_dir_all(&sound_dir).unwrap();
        write_wav(&sound_dir.join("02.wav"), &[0.2; 16], 44100);
        write_wav(&sound_dir.join("01.wav"), &[0.1; 16], 44100);

        let mut bank = SampleBank::new();
        let count = bank.load_dir(&root).expect("load sample dir");
        assert_eq!(count, 2);
        assert!((bank.get("tone", 0).unwrap().data[0] - 0.1).abs() < 1e-4);
        assert!((bank.get("tone", 1).unwrap().data[0] - 0.2).abs() < 1e-4);

        let _ = std::fs::remove_dir_all(&root);
    }
}
