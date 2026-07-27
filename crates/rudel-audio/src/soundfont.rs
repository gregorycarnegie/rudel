// soundfont.rs - General MIDI soundfont playback via WebAudioFont presets.
//
// Ports strudel/packages/soundfonts: `gm.mjs`'s name -> preset-file table
// (generated into `gm_table.json`) and `fontloader.mjs`'s preset loading, zone
// selection and repitching. A WebAudioFont preset is a JavaScript file holding
// one `zones` array; each zone covers a MIDI key range and carries a base64
// audio file plus the tuning and loop points to play it back with.
//
// Every playback primitive this needs already exists: `SamplerParams` does
// repitching and looping, and the voice ADSR / pitch envelope / vibrato are
// shared with every other sound. Only the asset format is new.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_dsp::Sample;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};

/// The General MIDI table: `gm_*` sound name -> the WebAudioFont preset files
/// that can voice it, in Strudel's order (so `n` picks the same one).
static GM: LazyLock<HashMap<String, Vec<String>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../tools/oracle/gm_table.json"))
        .expect("gm_table.json")
});

/// Every General MIDI sound name (`gm_piano`, `gm_epiano1`, …), sorted.
pub fn gm_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = GM.keys().map(String::as_str).collect();
    names.sort_unstable();
    names
}

/// The preset file backing `name` at sample index `n`, wrapping like
/// superdough's `getSoundIndex`. `None` when `name` is not a GM sound.
pub fn gm_preset(name: &str, n: i64) -> Option<&'static str> {
    let fonts = GM.get(name)?;
    let i = n.rem_euclid(fonts.len() as i64) as usize;
    Some(fonts[i].as_str())
}

/// How many preset variants a GM name has (`n` selects among them).
pub fn gm_variants(name: &str) -> usize {
    GM.get(name).map_or(0, Vec::len)
}

// ---------------------------------------------------------------------------
// Preset source

/// Where preset files are fetched from. Strudel defaults to the WebAudioFont
/// data mirror and lets `setSoundfontUrl` repoint it.
const DEFAULT_URL: &str = "https://felixroos.github.io/webaudiofontdata/sound";

static SOUNDFONT_URL: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new(DEFAULT_URL.to_string()));

/// Repoint preset loading at another mirror or a local directory
/// (`setSoundfontUrl`).
pub fn set_soundfont_url(url: &str) {
    *SOUNDFONT_URL.lock().unwrap() = url.trim_end_matches('/').to_string();
}

/// The URL a preset file is loaded from.
pub fn preset_url(preset: &str) -> String {
    format!("{}/{preset}.js", SOUNDFONT_URL.lock().unwrap())
}

// ---------------------------------------------------------------------------
// Lazy load requests
//
// A soundfont is fetched the first time a pattern asks for it, as upstream does
// inside its async `registerSound` handler. The scheduler cannot block or spawn
// threads, so a miss records the request here and the app drains it, exactly
// like the sample-loading jobs it already runs.

static REQUESTS: LazyLock<Mutex<HashSet<(String, i64)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Note that `(name, n)` was asked for but is not loaded yet.
pub fn request_font(name: &str, n: i64) {
    REQUESTS.lock().unwrap().insert((name.to_string(), n));
}

/// Take everything requested since the last call.
pub fn take_font_requests() -> Vec<(String, i64)> {
    REQUESTS.lock().unwrap().drain().collect()
}

// ---------------------------------------------------------------------------
// Zones

/// One zone of a WebAudioFont preset: the decoded audio for a MIDI key range,
/// plus how to tune and loop it.
#[derive(Clone)]
pub struct Zone {
    /// The decoded recording this zone plays.
    pub sample: Arc<Sample>,
    /// Lowest MIDI key this zone covers (inclusive).
    pub key_low: i32,
    /// Highest MIDI key this zone covers (inclusive).
    pub key_high: i32,
    /// The pitch the recording sounds at, in cents (2400 = MIDI 24).
    pub original_pitch: f64,
    /// Tuning offset in semitones.
    pub coarse_tune: f64,
    /// Tuning offset in cents.
    pub fine_tune: f64,
    /// The *original* sample rate, which the loop points are expressed in.
    pub sample_rate: f64,
    /// Loop start, in frames at [`sample_rate`](Self::sample_rate).
    pub loop_start: f64,
    /// Loop end, in frames at [`sample_rate`](Self::sample_rate).
    pub loop_end: f64,
}

impl Zone {
    /// The playback rate for a MIDI note, from `fontloader.mjs`:
    /// `baseDetune = originalPitch - 100*coarseTune - fineTune`, then
    /// `2^((100*midi - baseDetune) / 1200)`.
    pub fn playback_rate(&self, midi: f64) -> f64 {
        let base_detune = self.original_pitch - 100.0 * self.coarse_tune - self.fine_tune;
        2f64.powf((100.0 * midi - base_detune) / 1200.0)
    }

    /// Whether this zone sustains on a loop rather than playing once through
    /// (upstream's `loopStart > 1 && loopStart < loopEnd`).
    pub fn loops(&self) -> bool {
        self.loop_start > 1.0 && self.loop_start < self.loop_end
    }

    /// The loop region as fractions of the decoded buffer. Upstream converts
    /// the loop points to seconds against the zone's original sample rate and
    /// hands those to the buffer source, which is the same thing once the
    /// decoded buffer's own duration is taken into account.
    pub fn loop_fractions(&self) -> Option<(f32, f32)> {
        if !self.loops() {
            return None;
        }
        let duration = self.sample.data.len() as f64 / self.sample.sample_rate as f64;
        if duration <= 0.0 {
            return None;
        }
        let to_frac = |frames: f64| ((frames / self.sample_rate) / duration).clamp(0.0, 1.0) as f32;
        Some((to_frac(self.loop_start), to_frac(self.loop_end)))
    }
}

/// A loaded preset: its zones, in file order.
#[derive(Clone, Default)]
pub struct Preset {
    /// The preset's zones, in file order.
    pub zones: Vec<Zone>,
}

impl Preset {
    /// The zone covering `midi`, matching `fontloader.mjs`'s `findZone`
    /// (`keyRangeLow <= pitch && keyRangeHigh + 1 >= pitch`, first match wins).
    /// The `+ 1` makes adjacent ranges overlap by a semitone, so a note exactly
    /// on a boundary takes the lower zone; that is upstream's behaviour and is
    /// kept rather than corrected. Falls back to the nearest zone so a note
    /// outside every range still sounds.
    pub fn zone_for(&self, midi: f64) -> Option<&Zone> {
        let pitch = midi.round() as i32;
        self.zones
            .iter()
            .find(|z| z.key_low <= pitch && z.key_high + 1 >= pitch)
            .or_else(|| {
                self.zones.iter().min_by_key(|z| {
                    let mid = (z.key_low + z.key_high) / 2;
                    (mid - pitch).abs()
                })
            })
    }
}

// ---------------------------------------------------------------------------
// Preset file parsing
//
// A preset file is a JavaScript assignment whose right-hand side is an object
// literal: unquoted keys, numbers, booleans, and single-quoted base64 strings.
// Upstream splits on `={` and `eval`s the remainder; here the fields are read
// directly, which needs no general JS parser and cannot execute anything.

/// Split a preset file into its zone bodies (the text between each zone's
/// braces), tracking single-quoted strings so a brace inside base64 data — or
/// any other punctuation — cannot end a zone early.
fn zone_bodies(src: &str) -> Vec<&str> {
    let Some(start) = src.find("zones:") else {
        return Vec::new();
    };
    let bytes = src.as_bytes();
    let mut bodies = Vec::new();
    let (mut i, mut depth, mut open, mut in_str) = (start, 0usize, 0usize, false);
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str => i += 1, // skip the escaped character
            b'\'' => in_str = !in_str,
            b'{' if !in_str => {
                if depth == 0 {
                    open = i + 1;
                }
                depth += 1;
            }
            b'}' if !in_str => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    bodies.push(&src[open..i]);
                }
            }
            b']' if !in_str && depth == 0 => break, // end of the zones array
            _ => {}
        }
        i += 1;
    }
    bodies
}

/// Read one `key:value` field out of a zone body. Values are numbers, booleans
/// or single-quoted strings, and fields are comma-separated.
fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(at) = body[from..].find(key) {
        let at = from + at;
        // Only a whole key, not a suffix of a longer one (`file` vs `sfile`).
        let standalone = body[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after = body[at + key.len()..].trim_start();
        if standalone && let Some(value) = after.strip_prefix(':') {
            let value = value.trim_start();
            const QUOTE: char = '\u{27}';
            return Some(match value.strip_prefix(QUOTE) {
                // A quoted value runs to the closing quote.
                Some(rest) => &rest[..rest.find(QUOTE).unwrap_or(rest.len())],
                // A bare value runs to the next separator.
                None => value[..value
                    .find([',', '}', '\u{a}', '\u{d}'])
                    .unwrap_or(value.len())]
                    .trim(),
            });
        }
        from = at + key.len();
    }
    None
}

/// Parse a WebAudioFont preset file, decoding each zone's audio.
///
/// `decode` turns a zone's raw (base64-decoded) audio bytes into a [`Sample`];
/// zones whose audio fails to decode are skipped rather than failing the whole
/// preset, so one bad zone does not silence an instrument.
pub fn parse_preset(
    src: &str,
    decode: impl Fn(&[u8]) -> Result<Sample, String>,
) -> Result<Preset, String> {
    let bodies = zone_bodies(src);
    if bodies.is_empty() {
        return Err("no zones found in preset".to_string());
    }
    let mut zones = Vec::new();
    for body in bodies {
        let num = |key: &str, default: f64| {
            field(body, key)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(default)
        };
        let Some(encoded) = field(body, "file") else {
            // `zone.sample` (raw PCM) is the other upstream branch, which its
            // own code marks untested; the mirrors all ship `file`.
            continue;
        };
        let Ok(bytes) = base64_decode(encoded) else {
            continue;
        };
        let Ok(sample) = decode(&bytes) else {
            continue;
        };
        zones.push(Zone {
            sample: Arc::new(sample),
            key_low: num("keyRangeLow", 0.0) as i32,
            key_high: num("keyRangeHigh", 127.0) as i32,
            original_pitch: num("originalPitch", 6000.0),
            coarse_tune: num("coarseTune", 0.0),
            fine_tune: num("fineTune", 0.0),
            sample_rate: num("sampleRate", 44100.0).max(1.0),
            loop_start: num("loopStart", 0.0),
            loop_end: num("loopEnd", 0.0),
        });
    }
    if zones.is_empty() {
        return Err("no playable zones in preset".to_string());
    }
    Ok(Preset { zones })
}

/// Decode standard base64. Hand-rolled rather than pulling a crate in for one
/// call site; whitespace is skipped and padding is optional.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const INVALID: i8 = -1;
    static TABLE: LazyLock<[i8; 256]> = LazyLock::new(|| {
        let mut t = [INVALID; 256];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for (i, c) in alphabet.iter().enumerate() {
            t[*c as usize] = i as i8;
        }
        t
    });
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in input.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = TABLE[c as usize];
        if v == INVALID {
            return Err(format!("invalid base64 byte {c:#x}"));
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Loading

/// Fetch and parse the preset backing `(name, n)`, decoding every zone.
///
/// `fetch` reads the preset file's text (HTTP or local, cached by the caller)
/// and `decode` turns a zone's audio bytes into a [`Sample`], so this stays
/// free of both the network and the audio decoder.
pub fn load_gm_preset(
    name: &str,
    n: i64,
    fetch: impl Fn(&str) -> Result<String, String>,
    decode: impl Fn(&[u8]) -> Result<Sample, String>,
) -> Result<Preset, String> {
    let preset = gm_preset(name, n).ok_or_else(|| format!("unknown soundfont {name:?}"))?;
    let src = fetch(&preset_url(preset))?;
    parse_preset(&src, decode).map_err(|e| format!("{preset}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature preset in the real file's shape: a `var` assignment holding
    /// one `zones` array of unquoted-key objects with base64 audio.
    const PRESET: &str = concat!(
        "console.log('load _tone_x');
",
        "var _tone_x={
",
        "	zones:[
",
        "		{
",
        "			midi:0
",
        "			,originalPitch:2400
",
        "			,keyRangeLow:0
",
        "			,keyRangeHigh:27
",
        "			,loopStart:100
",
        "			,loopEnd:200
",
        "			,coarseTune:0
",
        "			,fineTune:0
",
        "			,sampleRate:44100
",
        "			,ahdsr:true
",
        "			,file:'QUJD'
",
        "		},
",
        "		{
",
        "			midi:0
",
        "			,originalPitch:6000
",
        "			,keyRangeLow:28
",
        "			,keyRangeHigh:127
",
        "			,loopStart:0
",
        "			,loopEnd:0
",
        "			,coarseTune:-1
",
        "			,fineTune:50
",
        "			,sampleRate:22050
",
        "			,file:'REVG'
",
        "		}]}
",
    );

    fn stub(bytes: &[u8]) -> Result<Sample, String> {
        Ok(Sample {
            data: vec![0.0; bytes.len().max(1)],
            sample_rate: 44100.0,
        })
    }

    #[test]
    fn base64_round_trips_known_values() {
        assert_eq!(base64_decode("QUJD").unwrap(), b"ABC");
        // Padding is optional and whitespace is skipped.
        assert_eq!(base64_decode("QQ==").unwrap(), b"A");
        assert_eq!(base64_decode("Q U J D").unwrap(), b"ABC");
        assert!(base64_decode("!!").is_err());
    }

    #[test]
    fn preset_zones_parse_with_their_tuning() {
        let preset = parse_preset(PRESET, stub).expect("parse");
        assert_eq!(preset.zones.len(), 2);
        let low = &preset.zones[0];
        assert_eq!((low.key_low, low.key_high), (0, 27));
        assert_eq!(low.original_pitch, 2400.0);
        assert_eq!(low.sample.data.len(), 3); // "ABC"
        let high = &preset.zones[1];
        assert_eq!(high.coarse_tune, -1.0);
        assert_eq!(high.fine_tune, 50.0);
    }

    #[test]
    fn zone_selection_follows_the_key_ranges() {
        let preset = parse_preset(PRESET, stub).expect("parse");
        assert_eq!(preset.zone_for(10.0).unwrap().key_high, 27);
        assert_eq!(preset.zone_for(27.0).unwrap().key_high, 27);
        // Upstream's test is `keyRangeHigh + 1 >= pitch`, so ranges overlap by
        // one and the note on a boundary takes the *lower* zone. Kept as-is for
        // parity rather than "fixed".
        assert_eq!(preset.zone_for(28.0).unwrap().key_high, 27);
        assert_eq!(preset.zone_for(29.0).unwrap().key_low, 28);
        assert_eq!(preset.zone_for(64.0).unwrap().key_low, 28);
    }

    #[test]
    fn playback_rate_matches_the_upstream_formula() {
        let preset = parse_preset(PRESET, stub).expect("parse");
        // originalPitch 2400 cents is MIDI 24, so that note plays at rate 1.
        let low = &preset.zones[0];
        assert!((low.playback_rate(24.0) - 1.0).abs() < 1e-12);
        // An octave up doubles the rate.
        assert!((low.playback_rate(36.0) - 2.0).abs() < 1e-12);
        // baseDetune = 6000 - 100*(-1) - 50 = 6050, so MIDI 60 plays slightly flat.
        let high = &preset.zones[1];
        let want = 2f64.powf((6000.0 - 6050.0) / 1200.0);
        assert!((high.playback_rate(60.0) - want).abs() < 1e-12);
    }

    #[test]
    fn loop_region_is_only_reported_for_looping_zones() {
        let preset = parse_preset(PRESET, stub).expect("parse");
        assert!(preset.zones[0].loops());
        assert!(preset.zones[0].loop_fractions().is_some());
        // loopStart == loopEnd == 0 is a one-shot.
        assert!(!preset.zones[1].loops());
        assert!(preset.zones[1].loop_fractions().is_none());
    }

    #[test]
    fn gm_table_maps_names_to_presets() {
        assert!(gm_names().contains(&"gm_piano"));
        assert_eq!(gm_names().len(), 125);
        let first = gm_preset("gm_piano", 0).expect("gm_piano");
        // `n` wraps over the variants, like superdough's getSoundIndex.
        let count = gm_variants("gm_piano") as i64;
        assert_eq!(gm_preset("gm_piano", count), Some(first));
        assert_eq!(gm_preset("gm_piano", -count), Some(first));
        assert_eq!(gm_preset("not_a_font", 0), None);
    }
}
