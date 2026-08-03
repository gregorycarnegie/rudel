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

    // --- preset-file scanning and zone selection ----------------------------
    //
    // 36 of soundfont.rs's mutants survived, 14 of them in `zone_bodies`. That
    // scanner is the reason this module needs no JS evaluator: it finds each
    // zone's braces while tracking single-quoted strings, so a brace inside
    // base64 data cannot end a zone early. Nothing was feeding it data that
    // contained one.

    fn zone(low: i32, high: i32) -> Zone {
        Zone {
            sample: Arc::new(Sample {
                data: vec![0.0; 100],
                sample_rate: 100.0,
            }),
            key_low: low,
            key_high: high,
            original_pitch: 6000.0,
            coarse_tune: 0.0,
            fine_tune: 0.0,
            sample_rate: 100.0,
            loop_start: 0.0,
            loop_end: 0.0,
        }
    }

    #[test]
    fn zone_bodies_are_split_on_braces_outside_strings() {
        // The plain shape.
        let bodies = zone_bodies("x={zones:[{a:1},{b:2}]}");
        assert_eq!(bodies, vec!["a:1", "b:2"]);

        // A brace inside a quoted value is data, not structure — this is the
        // case real preset files hit, since base64 contains every punctuation
        // character there is.
        let bodies = zone_bodies("x={zones:[{file:'AA}BB{CC',a:1},{b:2}]}");
        assert_eq!(bodies, vec!["file:'AA}BB{CC',a:1", "b:2"]);
        // ...including a bracket, which would otherwise end the array.
        let bodies = zone_bodies("x={zones:[{file:'A]B'},{b:2}]}");
        assert_eq!(bodies, vec!["file:'A]B'", "b:2"]);

        // An escaped quote does not close the string.
        let bodies = zone_bodies("x={zones:[{file:'A\\'}B',a:1}]}");
        assert_eq!(bodies, vec!["file:'A\\'}B',a:1"]);

        // Nested braces belong to their zone rather than splitting it.
        let bodies = zone_bodies("x={zones:[{a:{b:1}},{c:2}]}");
        assert_eq!(bodies, vec!["a:{b:1}", "c:2"]);

        // Everything after the zones array is ignored.
        let bodies = zone_bodies("x={zones:[{a:1}],other:{b:2}}");
        assert_eq!(bodies, vec!["a:1"]);

        // No zones key, and an empty array, both give nothing.
        assert!(zone_bodies("x={other:[{a:1}]}").is_empty());
        assert!(zone_bodies("x={zones:[]}").is_empty());
        assert!(zone_bodies("").is_empty());
    }

    #[test]
    fn a_field_is_read_only_as_a_whole_key() {
        let body = "file:'AAA',midi:60,loop:true,sfile:'BBB'";
        assert_eq!(field(body, "midi"), Some("60"));
        assert_eq!(field(body, "loop"), Some("true"));
        assert_eq!(field(body, "file"), Some("AAA"));
        // `file` must not match the tail of `sfile`, and `sfile` is its own key.
        assert_eq!(field(body, "sfile"), Some("BBB"));
        // A key that is not there.
        assert_eq!(field(body, "nope"), None);
        // A key appearing only as part of a longer name is not a match.
        assert_eq!(field("sfile:'BBB'", "file"), None);

        // Whitespace either side of the colon is allowed.
        assert_eq!(field("midi : 60 , x:1", "midi"), Some("60"));
        // A value runs to the next separator, not past it.
        assert_eq!(field("a:1}", "a"), Some("1"));
        assert_eq!(field("a:1\nb:2", "a"), Some("1"));
        // A quoted value keeps its punctuation.
        assert_eq!(field("a:'1,2}3'", "a"), Some("1,2}3"));
        // A key with no value is not a field.
        assert_eq!(field("midi", "midi"), None);
    }

    #[test]
    fn a_key_picks_the_zone_covering_it_or_the_nearest_one() {
        let preset = Preset {
            zones: vec![zone(0, 47), zone(48, 71), zone(72, 127)],
        };
        let picked = |midi: f64| {
            let z = preset.zone_for(midi).expect("a zone");
            (z.key_low, z.key_high)
        };

        // Inside a range.
        assert_eq!(picked(60.0), (48, 71));
        assert_eq!(picked(49.0), (48, 71));
        assert_eq!(picked(47.0), (0, 47));
        assert_eq!(picked(0.0), (0, 47));
        assert_eq!(picked(127.0), (72, 127));

        // On a boundary the *lower* zone wins, because upstream's test is
        // `keyRangeHigh + 1 >= pitch` — adjacent ranges overlap by a semitone
        // and the first match is taken. Kept rather than corrected, so this
        // pins the quirk rather than the tidier answer.
        assert_eq!(picked(48.0), (0, 47), "the +1 overlap takes the lower zone");
        assert_eq!(picked(72.0), (48, 71));

        // A fractional key rounds to the nearest MIDI note first.
        assert_eq!(picked(60.4), (48, 71));
        assert_eq!(picked(47.6), (0, 47), "rounds to 48, which zone 0 covers");
        assert_eq!(picked(49.4), (48, 71));

        // Outside every range, the nearest zone by midpoint is used rather than
        // nothing at all — a note above the top of the font still sounds.
        assert_eq!(picked(200.0), (72, 127));
        assert_eq!(picked(-40.0), (0, 47));

        // A preset with no zones has nothing to pick.
        assert!(Preset::default().zone_for(60.0).is_none());
    }

    #[test]
    fn a_zone_loops_only_when_its_points_make_a_region() {
        let with = |start: f64, end: f64| {
            let mut z = zone(0, 127);
            z.loop_start = start;
            z.loop_end = end;
            z
        };
        // A real region.
        assert!(with(10.0, 50.0).loops());
        // Points at the origin mark "no loop" rather than a zero-length one.
        assert!(!with(0.0, 0.0).loops());
        assert!(!with(0.0, 50.0).loops());
        // So does a start of exactly 1, which upstream uses the same way.
        assert!(!with(1.0, 50.0).loops());
        assert!(with(1.5, 50.0).loops());
        // A backwards or empty region is not a loop.
        assert!(!with(50.0, 10.0).loops());
        assert!(!with(50.0, 50.0).loops());
    }

    #[test]
    fn loop_points_become_fractions_of_the_decoded_buffer() {
        // The sample here is 100 frames at 100Hz, so one second long, and the
        // zone states its points against the same rate — a point at frame 25 is
        // a quarter of the way in.
        let mut z = zone(0, 127);
        z.loop_start = 25.0;
        z.loop_end = 75.0;
        let (a, b) = z.loop_fractions().expect("a loop");
        assert!((a - 0.25).abs() < 1e-6, "start fraction {a}");
        assert!((b - 0.75).abs() < 1e-6, "end fraction {b}");

        // A zone that does not loop has no fractions.
        let mut z = zone(0, 127);
        z.loop_start = 0.0;
        z.loop_end = 0.0;
        assert!(z.loop_fractions().is_none());

        // Points past the end of the buffer clamp into range rather than
        // producing a fraction above 1, which would read off the end.
        let mut z = zone(0, 127);
        z.loop_start = 25.0;
        z.loop_end = 500.0;
        let (a, b) = z.loop_fractions().expect("a loop");
        assert!((a - 0.25).abs() < 1e-6);
        assert_eq!(b, 1.0, "clamped to the end of the buffer");

        // An empty buffer has no duration to take a fraction of.
        let mut z = zone(0, 127);
        z.sample = Arc::new(Sample {
            data: Vec::new(),
            sample_rate: 100.0,
        });
        z.loop_start = 25.0;
        z.loop_end = 75.0;
        assert!(z.loop_fractions().is_none());
    }

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

    #[test]
    fn loop_fractions_divide_by_the_buffers_duration_not_just_its_rate() {
        // The earlier case used a one-second buffer, where dividing by the
        // duration and not dividing by it are the same thing. This one is two
        // seconds long, so both divisions have to be right.
        let mut z = zone(0, 127);
        z.sample = Arc::new(Sample {
            data: vec![0.0; 200],
            sample_rate: 100.0,
        });
        z.sample_rate = 100.0;
        z.loop_start = 50.0;
        z.loop_end = 150.0;
        let (a, b) = z.loop_fractions().expect("a loop");
        assert!(
            (a - 0.25).abs() < 1e-6,
            "50 frames into 2s is a quarter: {a}"
        );
        assert!((b - 0.75).abs() < 1e-6, "150 frames into 2s is 3/4: {b}");

        // And the zone's own rate is what the points are counted in, which is
        // not always the decoded buffer's.
        let mut z = zone(0, 127);
        z.sample = Arc::new(Sample {
            data: vec![0.0; 200],
            sample_rate: 100.0,
        });
        z.sample_rate = 50.0; // points stated at half the decoded rate
        z.loop_start = 25.0;
        z.loop_end = 75.0;
        let (a, b) = z.loop_fractions().expect("a loop");
        assert!(
            (a - 0.25).abs() < 1e-6,
            "25 frames at 50Hz is 0.5s of 2s: {a}"
        );
        assert!((b - 0.75).abs() < 1e-6);
    }

    #[test]
    fn base64_round_trips_and_rejects_what_is_not_base64() {
        // The zone payloads are base64, so a wrong bit here is a wrong sample.
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
        assert_eq!(base64_decode("QQ==").unwrap(), b"A");
        assert_eq!(base64_decode("QUI=").unwrap(), b"AB");
        assert_eq!(base64_decode("QUJD").unwrap(), b"ABC");
        assert_eq!(base64_decode("QUJDRA==").unwrap(), b"ABCD");
        // Padding is optional, as the doc comment says.
        assert_eq!(base64_decode("QQ").unwrap(), b"A");
        assert_eq!(base64_decode("QUI").unwrap(), b"AB");
        // Whitespace between groups is skipped, including newlines.
        assert_eq!(base64_decode("QU\n JD").unwrap(), b"ABC");

        // Every alphabet position has to map to its own value, so a byte with
        // all the bits set round-trips exactly.
        assert_eq!(base64_decode("////").unwrap(), vec![0xff, 0xff, 0xff]);
        assert_eq!(base64_decode("AAAA").unwrap(), vec![0, 0, 0]);
        // The two non-alphanumeric characters are distinct.
        assert_eq!(base64_decode("+/+/").unwrap(), vec![0xfb, 0xff, 0xbf]);

        // Anything outside the alphabet is an error rather than silent data.
        assert!(base64_decode("QU*D").is_err());
        assert!(base64_decode("QU-D").is_err());
        assert!(base64_decode("QU_D").is_err());
    }

    #[test]
    fn a_preset_url_joins_the_base_to_the_preset_name() {
        // The base is shared mutable state, so put it back afterwards.
        let original = preset_url("x");
        let base = original.strip_suffix("/x.js").expect("a base").to_string();

        set_soundfont_url("https://example.test/fonts");
        assert_eq!(
            preset_url("_tone_0000"),
            "https://example.test/fonts/_tone_0000.js"
        );
        // A trailing slash on the base does not become a double slash.
        set_soundfont_url("https://example.test/fonts/");
        assert_eq!(
            preset_url("_tone_0000"),
            "https://example.test/fonts/_tone_0000.js"
        );
        // Several trailing slashes go too.
        set_soundfont_url("https://example.test/fonts///");
        assert_eq!(preset_url("a"), "https://example.test/fonts/a.js");

        set_soundfont_url(&base);
        assert_eq!(preset_url("x"), original, "the base is restored");
    }
}
