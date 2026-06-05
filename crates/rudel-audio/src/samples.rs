// samples.rs - a bank of decoded audio samples, keyed by sound name and index.
// Decoding uses fundsp's Wave (Symphonia under the hood).
// SPDX-License-Identifier: AGPL-3.0-or-later

use fundsp::wave::Wave;
use rudel_dsp::Sample;
use std::collections::HashMap;
use std::path::Path;
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
        let mut count = 0;
        let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
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
            for file in files {
                if self.load_file(&name, &file).is_ok() {
                    count += 1;
                }
            }
        }
        Ok(count)
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
    Ok(Sample {
        data,
        sample_rate: wave.sample_rate() as f32,
    })
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
}
