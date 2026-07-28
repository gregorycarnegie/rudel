// samples.rs - a bank of decoded audio samples, keyed by sound name and index.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod decoding;
mod loading;
#[cfg(test)]
mod tests;

pub(crate) use loading::{decode_bytes, fetch_cached_bytes, fetch_cached_text};

use crate::soundfont::Preset;
use rudel_dsp::{Sample, WaveTable};
use std::{collections::HashMap, sync::Arc};
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
    /// Loaded soundfont presets, keyed by sound name and `n` variant. Kept
    /// apart from `map` because a preset picks its recording by MIDI key range
    /// and detunes in cents, where a sample group picks the nearest tuning and
    /// repitches in semitones.
    fonts: HashMap<(String, i64), Arc<Preset>>,
    /// Bank aliases (`alias -> canonical`), so `s("bd").bank("tr909")` can find
    /// a pack registered as `RolandTR909_bd`. See [`alias_bank`](Self::alias_bank).
    bank_aliases: HashMap<String, String>,
    /// Wavetable collections loaded by `tables(...)`, keyed by sound name; `n`
    /// indexes into the list, as it does for samples.
    tables: HashMap<String, Vec<WaveTable>>,
}

/// A soundfont zone resolved for one note: what to play, how fast, and where
/// it loops.
pub struct FontVoice {
    /// The zone's recording.
    pub sample: Arc<Sample>,
    /// Playback rate that puts the recording at the requested pitch.
    pub rate: f32,
    /// Loop region as fractions of the buffer, when the zone sustains.
    pub loop_region: Option<(f32, f32)>,
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
        // Publish the length so `getDuration(name, n)` can read it back at eval
        // time; `n` counts across the sound's groups, as `resolve` indexes it.
        let seconds = sample.data.len() as f64 / f64::from(sample.sample_rate).max(1.0);
        let groups = self.map.entry(name.to_string()).or_default();
        let index = groups.iter().map(|g| g.samples.len()).sum::<usize>() as i64;
        rudel_core::set_sample_duration(name, index, seconds);
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

    /// Register a wavetable under `name` (appended as the next `n` index).
    pub fn register_table(&mut self, name: &str, table: WaveTable) {
        self.tables.entry(name.to_string()).or_default().push(table);
    }

    /// Resolve the `n`-th wavetable registered under `name`, wrapping `n` into
    /// range like [`resolve`](Self::resolve) does for samples.
    pub fn resolve_table(&self, name: &str, index: i64) -> Option<WaveTable> {
        let tables = self.tables.get(name)?;
        if tables.is_empty() {
            return None;
        }
        Some(tables[wrap_index(index, tables.len())].clone())
    }

    /// Register a loaded soundfont preset for `name` at variant `n`.
    pub fn register_font(&mut self, name: &str, n: i64, preset: Preset) {
        self.fonts.insert((name.to_string(), n), Arc::new(preset));
    }

    /// Whether a preset is already loaded for `(name, n)`.
    pub fn has_font(&self, name: &str, n: i64) -> bool {
        self.fonts.contains_key(&(name.to_string(), n))
    }

    /// Resolve a soundfont voice: the zone covering `midi`, the playback rate
    /// that puts it at that pitch, and its loop region if it sustains on one.
    pub fn resolve_font(&self, name: &str, n: i64, midi: f64) -> Option<FontVoice> {
        let zone = self.fonts.get(&(name.to_string(), n))?.zone_for(midi)?;
        Some(FontVoice {
            sample: zone.sample.clone(),
            rate: zone.playback_rate(midi) as f32,
            loop_region: zone.loop_fractions(),
        })
    }
}

/// Wrap a signed sample index into `0..len`, so negative indices count from the end.
fn wrap_index(index: i64, len: usize) -> usize {
    index.rem_euclid(len as i64) as usize
}
