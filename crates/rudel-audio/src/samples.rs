// samples.rs - a bank of decoded audio samples, keyed by sound name and index.
// Decoding uses fundsp's Wave (Symphonia under the hood).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::sample_map;
use fundsp::wave::Wave;
use rayon::prelude::*;
use rudel_dsp::Sample;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Maps a sound name (e.g. `"bd"`) to one or more samples (indexed by `n`).
#[derive(Default)]
pub struct SampleBank {
    map: HashMap<String, Vec<Arc<Sample>>>,
}

impl SampleBank {
    pub fn new() -> SampleBank {
        SampleBank::default()
    }

    /// Add a sample under `name` (appended as the next index).
    pub fn register(&mut self, name: &str, sample: Arc<Sample>) {
        self.map.entry(name.to_string()).or_default().push(sample);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// All registered sound names, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.map.keys().cloned().collect();
        names.sort();
        names
    }

    /// Fetch the `index`-th sample for `name` (wrapping if out of range).
    pub fn get(&self, name: &str, index: usize) -> Option<Arc<Sample>> {
        let v = self.map.get(name)?;
        if v.is_empty() {
            return None;
        }
        Some(v[index % v.len()].clone())
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

        let mut loaded: Vec<_> = jobs
            .into_par_iter()
            .enumerate()
            .filter_map(|(index, (name, file))| {
                load_sample(&file)
                    .ok()
                    .map(|sample| (index, name, Arc::new(sample)))
            })
            .collect();
        loaded.sort_by_key(|(index, _, _)| *index);

        let count = loaded.len();
        for (_, name, sample) in loaded {
            self.register(&name, sample);
        }
        Ok(count)
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
            return self.load_sample_map(&json, &base);
        }

        let path = Path::new(&url);
        if path.is_dir() {
            return self.load_dir(path);
        }
        if path.is_file() {
            let json = std::fs::read_to_string(path).map_err(|e| format!("read {url}: {e}"))?;
            let base = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            return self.load_sample_map(&json, &base);
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
        let entries = sample_map::parse_sample_map(json, base)?;
        // Fetch + decode in parallel; samples are registered in declaration
        // order so `n` indices stay stable.
        let loaded: Vec<(String, Vec<Result<Sample, String>>)> = entries
            .into_par_iter()
            .map(|(name, urls)| {
                let samples = urls.into_iter().map(|u| fetch_and_decode(&u)).collect();
                (name, samples)
            })
            .collect();

        let mut count = 0;
        for (name, samples) in loaded {
            for sample in samples {
                match sample {
                    Ok(s) => {
                        self.register(&name, Arc::new(s));
                        count += 1;
                    }
                    Err(e) => eprintln!("[rudel-audio] sample {name:?}: {e}"),
                }
            }
        }
        Ok(count)
    }
}

fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
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
        decode_sample_bytes(bytes)
    } else {
        load_sample(Path::new(url))
    }
}

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
    let wave = Wave::load(path).map_err(|e| format!("load {path:?}: {e}"))?;
    Ok(wave_to_sample(&wave))
}

/// Decode in-memory audio bytes into a mono [`Sample`] (for remote samples).
fn decode_sample_bytes(bytes: Vec<u8>) -> Result<Sample, String> {
    let wave = Wave::load_slice(bytes).map_err(|e| format!("decode audio: {e}"))?;
    Ok(wave_to_sample(&wave))
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
    use std::f32::consts::PI;

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
            .map(|i| (2.0 * PI * 220.0 * i as f32 / 44100.0).sin())
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

        let (_, urls) = entries
            .iter()
            .find(|(name, urls)| name == "bd" && !urls.is_empty())
            .expect("a `bd` sound with files");
        let sample = fetch_and_decode(&urls[0]).expect("fetch + decode one sample");
        assert!(!sample.data.is_empty(), "decoded sample should have audio");
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
        assert!(bank.get("missing", 0).is_none());
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
