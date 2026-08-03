// sf2.rs - SoundFont 2 file reading, for `loadSoundfont` / `.soundfont()`.
//
// Strudel gets this from the `sfumato` library; this is a direct reader for the
// parts that matter to playing a note: the sample headers, the preset ->
// instrument -> sample zone hierarchy, and the handful of generators that set
// a zone's key range, tuning and loop.
//
// The result is a [`Preset`] of the same [`Zone`]s the WebAudioFont path
// produces, so both soundfont formats share one playback path — the tuning
// math, zone selection and looping are identical once the recordings are
// decoded.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::soundfont::{Preset, Zone};
use rudel_dsp::Sample;
use std::sync::Arc;

/// The SF2 generator operators this reader understands. A SoundFont defines 60;
/// the rest shape envelopes and filters that Rudel drives from its own controls.
mod op {
    pub const START_ADDRS_OFFSET: u16 = 0;
    pub const END_ADDRS_OFFSET: u16 = 1;
    pub const STARTLOOP_ADDRS_OFFSET: u16 = 2;
    pub const ENDLOOP_ADDRS_OFFSET: u16 = 3;
    pub const START_ADDRS_COARSE_OFFSET: u16 = 4;
    pub const END_ADDRS_COARSE_OFFSET: u16 = 12;
    pub const STARTLOOP_ADDRS_COARSE_OFFSET: u16 = 45;
    pub const KEY_RANGE: u16 = 43;
    pub const ENDLOOP_ADDRS_COARSE_OFFSET: u16 = 50;
    pub const COARSE_TUNE: u16 = 51;
    pub const FINE_TUNE: u16 = 52;
    pub const SAMPLE_ID: u16 = 53;
    pub const SAMPLE_MODES: u16 = 54;
    pub const OVERRIDING_ROOT_KEY: u16 = 58;
    pub const INSTRUMENT: u16 = 41;
}

/// A little-endian byte reader over a RIFF chunk body.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(out)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    /// A fixed-width, NUL-padded name field.
    fn name(&mut self, n: usize) -> Option<String> {
        let raw = self.take(n)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some(String::from_utf8_lossy(&raw[..end]).trim().to_string())
    }
}

/// Walk the sub-chunks of a RIFF body, calling `visit` with each `(id, body)`.
/// LIST chunks are descended into, so `pdta`'s records are reached directly.
fn walk_chunks(body: &[u8], visit: &mut impl FnMut(&[u8; 4], &[u8])) {
    let mut r = Reader::new(body);
    while r.remaining() >= 8 {
        let Some(id) = r.take(4) else { return };
        let id: [u8; 4] = id.try_into().unwrap();
        let Some(size) = r.u32() else { return };
        let Some(data) = r.take(size as usize) else {
            return;
        };
        if &id == b"LIST" && data.len() >= 4 {
            walk_chunks(&data[4..], visit);
        } else {
            visit(&id, data);
        }
        // Chunks are word-aligned.
        if size % 2 == 1 {
            r.pos += 1;
        }
    }
}

/// One record of a `phdr`/`inst` table: a name and the index its zones start at.
struct Header {
    name: String,
    bag_index: usize,
}

/// A generator list, read as `(operator, amount)` pairs.
type Generators = Vec<(u16, u16)>;

/// The raw tables an SF2 file is built from.
#[derive(Default)]
struct Tables {
    /// 16-bit PCM sample data for the whole file.
    smpl: Vec<i16>,
    presets: Vec<Header>,
    preset_bags: Vec<usize>,
    preset_gens: Generators,
    instruments: Vec<Header>,
    inst_bags: Vec<usize>,
    inst_gens: Generators,
    samples: Vec<SampleHeader>,
}

/// One `shdr` record: where a recording lives in `smpl` and how it is tuned.
struct SampleHeader {
    start: u32,
    end: u32,
    start_loop: u32,
    end_loop: u32,
    sample_rate: u32,
    original_pitch: u8,
    pitch_correction: i8,
}

/// A generator amount read as a signed value (tuning amounts are signed).
fn signed(amount: u16) -> i32 {
    amount as i16 as i32
}

/// The last value set for `op` in a generator list, if any.
fn gen_value(gens: &[(u16, u16)], op: u16) -> Option<u16> {
    gens.iter().rev().find(|(o, _)| *o == op).map(|(_, a)| *a)
}

/// The generator lists of the zones of one `bag` range, in order.
fn zones_of(
    bags: &[usize],
    gens: &Generators,
    first: usize,
    next: Option<usize>,
) -> Vec<Generators> {
    let last = next.unwrap_or(bags.len().saturating_sub(1));
    (first..last.min(bags.len().saturating_sub(1)))
        .map(|i| {
            let (from, to) = (bags[i], bags[i + 1].min(gens.len()));
            gens.get(from..to).map(<[_]>::to_vec).unwrap_or_default()
        })
        .collect()
}

impl Tables {
    fn parse(bytes: &[u8]) -> Result<Tables, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"sfbk" {
            return Err("not a SoundFont 2 file (expected a RIFF sfbk header)".to_string());
        }
        let mut t = Tables::default();
        walk_chunks(&bytes[12..], &mut |id, data| match id {
            b"smpl" => {
                t.smpl = data
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
            }
            b"phdr" => t.presets = read_headers(data, 38, 24),
            b"inst" => t.instruments = read_headers(data, 22, 20),
            b"pbag" => t.preset_bags = read_bags(data),
            b"ibag" => t.inst_bags = read_bags(data),
            b"pgen" => t.preset_gens = read_gens(data),
            b"igen" => t.inst_gens = read_gens(data),
            b"shdr" => t.samples = read_sample_headers(data),
            _ => {}
        });
        if t.smpl.is_empty() {
            return Err("SoundFont has no sample data".to_string());
        }
        if t.presets.is_empty() {
            return Err("SoundFont has no presets".to_string());
        }
        Ok(t)
    }
}

/// Read a `phdr`/`inst` table: fixed-size records of a 20-byte name, then
/// (for presets) preset/bank numbers, then the bag index.
fn read_headers(data: &[u8], record: usize, bag_offset: usize) -> Vec<Header> {
    data.chunks_exact(record)
        .filter_map(|rec| {
            let mut r = Reader::new(rec);
            let name = r.name(20)?;
            r.pos = bag_offset;
            Some(Header {
                name,
                bag_index: r.u16()? as usize,
            })
        })
        .collect()
}

/// Read a `pbag`/`ibag` table: each record's generator start index.
fn read_bags(data: &[u8]) -> Vec<usize> {
    data.chunks_exact(4)
        .filter_map(|rec| Some(u16::from_le_bytes(rec[0..2].try_into().ok()?) as usize))
        .collect()
}

/// Read a `pgen`/`igen` table of `(operator, amount)` pairs.
fn read_gens(data: &[u8]) -> Generators {
    data.chunks_exact(4)
        .filter_map(|rec| {
            Some((
                u16::from_le_bytes(rec[0..2].try_into().ok()?),
                u16::from_le_bytes(rec[2..4].try_into().ok()?),
            ))
        })
        .collect()
}

/// Read the `shdr` sample-header table.
fn read_sample_headers(data: &[u8]) -> Vec<SampleHeader> {
    data.chunks_exact(46)
        .filter_map(|rec| {
            let mut r = Reader::new(rec);
            r.name(20)?;
            Some(SampleHeader {
                start: r.u32()?,
                end: r.u32()?,
                start_loop: r.u32()?,
                end_loop: r.u32()?,
                sample_rate: r.u32()?,
                original_pitch: r.u8()?,
                pitch_correction: r.u8()? as i8,
            })
        })
        .collect()
}

/// A parsed SoundFont: its presets, ready to voice notes.
pub struct SoundFont {
    presets: Vec<Preset>,
    names: Vec<String>,
}

impl SoundFont {
    /// The preset names, in file order (`n` selects among them).
    pub fn preset_names(&self) -> &[String] {
        &self.names
    }

    /// The preset at index `n`, wrapping like `getSoundIndex`.
    pub fn preset(&self, n: i64) -> Option<&Preset> {
        if self.presets.is_empty() {
            return None;
        }
        self.presets
            .get(n.rem_euclid(self.presets.len() as i64) as usize)
    }

    /// Every preset, paired with its name.
    pub fn into_presets(self) -> Vec<(String, Preset)> {
        self.names.into_iter().zip(self.presets).collect()
    }
}

/// Parse a `.sf2` file into presets of playable zones.
///
/// Each preset zone points at an instrument, whose zones point at recordings;
/// the two levels of generators are merged (instrument first, preset last) the
/// way the SoundFont spec layers them, and the result is one [`Zone`] per
/// playable region.
pub fn parse(bytes: &[u8]) -> Result<SoundFont, String> {
    let t = Tables::parse(bytes)?;
    let mut presets = Vec::new();
    let mut names = Vec::new();

    // The final `phdr` record is the terminal "EOP" entry, which only marks
    // where the last preset's zones end.
    for (i, header) in t
        .presets
        .iter()
        .enumerate()
        .take(t.presets.len().max(1) - 1)
    {
        let next = t.presets.get(i + 1).map(|h| h.bag_index);
        let mut zones = Vec::new();
        for pzone in zones_of(&t.preset_bags, &t.preset_gens, header.bag_index, next) {
            let Some(inst_idx) = gen_value(&pzone, op::INSTRUMENT) else {
                continue; // a global preset zone, no instrument of its own
            };
            let Some(inst) = t.instruments.get(inst_idx as usize) else {
                continue;
            };
            let inst_next = t
                .instruments
                .get(inst_idx as usize + 1)
                .map(|h| h.bag_index);
            for izone in zones_of(&t.inst_bags, &t.inst_gens, inst.bag_index, inst_next) {
                if let Some(zone) = build_zone(&t, &izone, &pzone) {
                    zones.push(zone);
                }
            }
        }
        if !zones.is_empty() {
            names.push(header.name.clone());
            presets.push(Preset { zones });
        }
    }
    if presets.is_empty() {
        return Err("SoundFont has no playable presets".to_string());
    }
    Ok(SoundFont { presets, names })
}

/// Build one playable zone from an instrument zone (and the preset zone whose
/// generators refine it).
fn build_zone(t: &Tables, izone: &[(u16, u16)], pzone: &[(u16, u16)]) -> Option<Zone> {
    let sample_id = gen_value(izone, op::SAMPLE_ID)? as usize;
    let sh = t.samples.get(sample_id)?;

    // Sample addresses may be nudged by coarse (32768-frame) and fine offsets.
    let offset = |fine: u16, coarse: u16| {
        signed(gen_value(izone, fine).unwrap_or(0))
            + signed(gen_value(izone, coarse).unwrap_or(0)) * 32768
    };
    let start = (sh.start as i64
        + offset(op::START_ADDRS_OFFSET, op::START_ADDRS_COARSE_OFFSET) as i64)
        .max(0) as usize;
    let end = (sh.end as i64 + offset(op::END_ADDRS_OFFSET, op::END_ADDRS_COARSE_OFFSET) as i64)
        .max(0)
        .min(t.smpl.len() as i64) as usize;
    if end <= start {
        return None;
    }

    let data: Vec<f32> = t.smpl[start..end]
        .iter()
        .map(|&s| s as f32 / 32768.0)
        .collect();

    // The key range comes from the instrument zone, narrowed by the preset
    // zone's own range when it sets one.
    let range = |gens: &[(u16, u16)]| {
        gen_value(gens, op::KEY_RANGE).map(|r| ((r & 0xff) as i32, (r >> 8) as i32))
    };
    let (mut low, mut high) = range(izone).unwrap_or((0, 127));
    if let Some((plow, phigh)) = range(pzone) {
        low = low.max(plow);
        high = high.min(phigh);
    }
    if low > high {
        return None;
    }

    // Tuning: preset-level offsets add to the instrument's, and the sample's
    // own `pitchCorrection` is a further cents trim.
    let tune = |op: u16| {
        signed(gen_value(izone, op).unwrap_or(0)) + signed(gen_value(pzone, op).unwrap_or(0))
    };
    let root = gen_value(izone, op::OVERRIDING_ROOT_KEY)
        .filter(|&k| k < 128)
        .map_or(sh.original_pitch as i32, |k| k as i32);

    // `sampleModes` bit 0 selects a looping zone.
    let loops = gen_value(izone, op::SAMPLE_MODES).is_some_and(|m| m & 1 == 1);
    let loop_offset = |fine: u16, coarse: u16| {
        signed(gen_value(izone, fine).unwrap_or(0))
            + signed(gen_value(izone, coarse).unwrap_or(0)) * 32768
    };
    let (loop_start, loop_end) = if loops {
        (
            (sh.start_loop as i64 - start as i64
                + loop_offset(
                    op::STARTLOOP_ADDRS_OFFSET,
                    op::STARTLOOP_ADDRS_COARSE_OFFSET,
                ) as i64)
                .max(0) as f64,
            (sh.end_loop as i64 - start as i64
                + loop_offset(op::ENDLOOP_ADDRS_OFFSET, op::ENDLOOP_ADDRS_COARSE_OFFSET) as i64)
                .max(0) as f64,
        )
    } else {
        (0.0, 0.0)
    };

    let sample_rate = sh.sample_rate.max(1) as f32;
    Some(Zone {
        sample: Arc::new(Sample { data, sample_rate }),
        key_low: low,
        key_high: high,
        // WebAudioFont states the root pitch in cents; a SoundFont states it as
        // a MIDI key, so the shared playback math needs it scaled.
        original_pitch: root as f64 * 100.0 - sh.pitch_correction as f64,
        coarse_tune: tune(op::COARSE_TUNE) as f64,
        fine_tune: tune(op::FINE_TUNE) as f64,
        sample_rate: sample_rate as f64,
        loop_start,
        loop_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- zone construction ---------------------------------------------------
    //
    // `build_zone` turns a pair of generator lists into a playable zone, and 31
    // of sf2.rs's 44 surviving mutants sat inside it. The round-trip test below
    // loads a whole file, which reaches the function but only along one path:
    // every offset is zero, the key range is the default, nothing loops. The
    // arithmetic joining those is what went unchecked.
    //
    // Driving it directly is much cheaper than assembling bytes for each case —
    // it is a pure function of two generator lists and the sample tables.

    /// Tables holding one 64-frame sample, addressed 0..64, with a loop marked
    /// at 16..48 and a deliberately odd tuning so every field is distinguishable.
    fn one_sample_tables() -> Tables {
        Tables {
            smpl: (0..64i16).map(|i| i * 100).collect(),
            presets: Vec::new(),
            preset_bags: Vec::new(),
            preset_gens: Generators::new(),
            instruments: Vec::new(),
            inst_bags: Vec::new(),
            inst_gens: Generators::new(),
            samples: vec![SampleHeader {
                start: 0,
                end: 64,
                start_loop: 16,
                end_loop: 48,
                sample_rate: 22050,
                original_pitch: 60,
                pitch_correction: 7,
            }],
        }
    }

    /// The instrument-zone generator list for sample 0, plus whatever else.
    fn izone(extra: &[(u16, u16)]) -> Vec<(u16, u16)> {
        let mut v = vec![(op::SAMPLE_ID, 0)];
        v.extend_from_slice(extra);
        v
    }

    fn zone(izone: &[(u16, u16)], pzone: &[(u16, u16)]) -> Option<Zone> {
        build_zone(&one_sample_tables(), izone, pzone)
    }

    #[test]
    fn a_zone_needs_a_sample_that_exists() {
        // No `sampleID` generator at all, and one pointing past the table.
        assert!(zone(&[], &[]).is_none(), "no sample id");
        assert!(zone(&[(op::SAMPLE_ID, 1)], &[]).is_none(), "out of range");
        assert!(zone(&izone(&[]), &[]).is_some(), "sample 0 exists");
    }

    #[test]
    fn the_sample_window_is_nudged_by_the_fine_and_coarse_offsets() {
        // The default window is the whole sample, scaled to -1..1.
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert_eq!(z.sample.data.len(), 64);
        assert!((z.sample.data[1] - 100.0 / 32768.0).abs() < 1e-9);

        // A fine start offset moves the start forward.
        let z = zone(&izone(&[(op::START_ADDRS_OFFSET, 8)]), &[]).expect("a zone");
        assert_eq!(z.sample.data.len(), 56, "start moved 8 frames in");
        assert!((z.sample.data[0] - 800.0 / 32768.0).abs() < 1e-9);

        // A fine end offset is signed, so pulling the end back is negative.
        let z = zone(&izone(&[(op::END_ADDRS_OFFSET, (-16i16) as u16)]), &[]).expect("a zone");
        assert_eq!(z.sample.data.len(), 48, "end pulled back 16 frames");

        // The coarse offsets are in 32768-frame units, so one is far past the
        // end of this sample and the window clamps to it rather than panicking.
        let z = zone(&izone(&[(op::END_ADDRS_COARSE_OFFSET, 1)]), &[]).expect("a zone");
        assert_eq!(z.sample.data.len(), 64, "clamped to the sample data");

        // A window with nothing in it is not a zone.
        assert!(
            zone(&izone(&[(op::START_ADDRS_OFFSET, 64)]), &[]).is_none(),
            "an empty window is not a zone"
        );
        assert!(
            zone(
                &izone(&[
                    (op::START_ADDRS_OFFSET, 40),
                    (op::END_ADDRS_OFFSET, (-40i16) as u16)
                ]),
                &[]
            )
            .is_none(),
            "an inverted window is not a zone"
        );
    }

    #[test]
    fn the_key_range_is_the_instruments_narrowed_by_the_presets() {
        // Packed low in the low byte, high in the high byte.
        let range = |low: i32, high: i32| ((high as u16) << 8) | low as u16;

        // No range anywhere is the whole keyboard.
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert_eq!((z.key_low, z.key_high), (0, 127));

        // The instrument zone alone.
        let z = zone(&izone(&[(op::KEY_RANGE, range(36, 72))]), &[]).expect("a zone");
        assert_eq!((z.key_low, z.key_high), (36, 72));

        // A preset range narrows it from both sides, never widens it.
        let z = zone(
            &izone(&[(op::KEY_RANGE, range(36, 72))]),
            &[(op::KEY_RANGE, range(48, 60))],
        )
        .expect("a zone");
        assert_eq!((z.key_low, z.key_high), (48, 60), "narrowed");

        let z = zone(
            &izone(&[(op::KEY_RANGE, range(48, 60))]),
            &[(op::KEY_RANGE, range(0, 127))],
        )
        .expect("a zone");
        assert_eq!((z.key_low, z.key_high), (48, 60), "not widened");

        // Ranges that do not overlap leave no keys, so there is no zone.
        assert!(
            zone(
                &izone(&[(op::KEY_RANGE, range(36, 47))]),
                &[(op::KEY_RANGE, range(60, 72))]
            )
            .is_none(),
            "disjoint ranges make no zone"
        );
    }

    #[test]
    fn the_root_pitch_comes_from_the_sample_unless_the_zone_overrides_it() {
        // In cents, with the sample's own correction trimmed off.
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert!(
            (z.original_pitch - (60.0 * 100.0 - 7.0)).abs() < 1e-9,
            "sample root 60 less 7 cents, got {}",
            z.original_pitch
        );

        // An overriding root key replaces the sample's.
        let z = zone(&izone(&[(op::OVERRIDING_ROOT_KEY, 69)]), &[]).expect("a zone");
        assert!((z.original_pitch - (69.0 * 100.0 - 7.0)).abs() < 1e-9);

        // The override only counts as a MIDI key; 255 is the "unset" marker
        // and has to fall back rather than become key 255.
        let z = zone(&izone(&[(op::OVERRIDING_ROOT_KEY, 255)]), &[]).expect("a zone");
        assert!(
            (z.original_pitch - (60.0 * 100.0 - 7.0)).abs() < 1e-9,
            "an out-of-range override falls back to the sample"
        );
    }

    #[test]
    fn tuning_adds_the_preset_offsets_to_the_instruments() {
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert_eq!((z.coarse_tune, z.fine_tune), (0.0, 0.0));

        let z = zone(
            &izone(&[(op::COARSE_TUNE, 2), (op::FINE_TUNE, 30)]),
            &[(op::COARSE_TUNE, 3), (op::FINE_TUNE, 5)],
        )
        .expect("a zone");
        assert_eq!(z.coarse_tune, 5.0, "coarse tunes add");
        assert_eq!(z.fine_tune, 35.0, "fine tunes add");

        // Both are signed, so a negative preset offset pulls the pitch down.
        let z = zone(
            &izone(&[(op::COARSE_TUNE, 2)]),
            &[(op::COARSE_TUNE, (-5i16) as u16)],
        )
        .expect("a zone");
        assert_eq!(z.coarse_tune, -3.0, "signed amounts");
    }

    #[test]
    fn only_a_zone_with_the_loop_bit_set_carries_loop_points() {
        // No `sampleModes` at all means no loop.
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert_eq!((z.loop_start, z.loop_end), (0.0, 0.0));

        // Bit 0 clear means no loop either, even though the sample has points.
        let z = zone(&izone(&[(op::SAMPLE_MODES, 2)]), &[]).expect("a zone");
        assert_eq!((z.loop_start, z.loop_end), (0.0, 0.0), "bit 0 is the flag");

        // Bit 0 set takes the sample header's points, relative to the window.
        let z = zone(&izone(&[(op::SAMPLE_MODES, 1)]), &[]).expect("a zone");
        assert_eq!((z.loop_start, z.loop_end), (16.0, 48.0));
        let z = zone(&izone(&[(op::SAMPLE_MODES, 3)]), &[]).expect("a zone");
        assert_eq!(
            (z.loop_start, z.loop_end),
            (16.0, 48.0),
            "other bits ignored"
        );

        // The points move with the window, so a shifted start shifts them back.
        let z = zone(
            &izone(&[(op::SAMPLE_MODES, 1), (op::START_ADDRS_OFFSET, 8)]),
            &[],
        )
        .expect("a zone");
        assert_eq!((z.loop_start, z.loop_end), (8.0, 40.0), "relative to start");

        // ...and the loop generators nudge them further.
        let z = zone(
            &izone(&[
                (op::SAMPLE_MODES, 1),
                (op::STARTLOOP_ADDRS_OFFSET, 4),
                (op::ENDLOOP_ADDRS_OFFSET, (-8i16) as u16),
            ]),
            &[],
        )
        .expect("a zone");
        assert_eq!((z.loop_start, z.loop_end), (20.0, 40.0));

        // A loop point pushed below zero clamps there rather than going
        // negative, which the player would read as a backwards loop.
        let z = zone(
            &izone(&[
                (op::SAMPLE_MODES, 1),
                (op::STARTLOOP_ADDRS_OFFSET, (-100i16) as u16),
            ]),
            &[],
        )
        .expect("a zone");
        assert_eq!(z.loop_start, 0.0, "clamped at the window start");
    }

    #[test]
    fn the_zone_keeps_the_samples_own_rate() {
        let z = zone(&izone(&[]), &[]).expect("a zone");
        assert_eq!(z.sample.sample_rate, 22050.0);
        assert_eq!(z.sample_rate, 22050.0);

        // A zero rate would divide by zero in the player, so it floors at 1.
        let mut t = one_sample_tables();
        t.samples[0].sample_rate = 0;
        let z = build_zone(&t, &izone(&[]), &[]).expect("a zone");
        assert_eq!(z.sample.sample_rate, 1.0);
    }

    #[test]
    fn a_repeated_generator_takes_the_last_value() {
        // The SoundFont spec says a later generator of the same op wins, which
        // is how preset-level edits override an instrument's defaults.
        assert_eq!(gen_value(&[(5, 1), (5, 2), (5, 3)], 5), Some(3));
        assert_eq!(gen_value(&[(5, 1), (6, 2)], 6), Some(2));
        assert_eq!(gen_value(&[(5, 1)], 6), None);
        assert_eq!(gen_value(&[], 5), None);
    }

    #[test]
    fn generator_amounts_read_as_signed_sixteen_bit() {
        assert_eq!(signed(0), 0);
        assert_eq!(signed(1), 1);
        assert_eq!(signed(32767), 32767);
        assert_eq!(signed(32768), -32768);
        assert_eq!(signed(65535), -1);
        assert_eq!(signed((-100i16) as u16), -100);
    }

    /// Build a minimal but structurally valid SoundFont: one preset, one
    /// instrument, one sample.
    fn tiny_sf2() -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
            if body.len() % 2 == 1 {
                out.push(0);
            }
            out
        }
        fn name(s: &str, len: usize) -> Vec<u8> {
            let mut v = s.as_bytes().to_vec();
            v.resize(len, 0);
            v
        }

        // 64 frames of PCM.
        let smpl: Vec<u8> = (0..64i16).flat_map(|i| (i * 400).to_le_bytes()).collect();

        // phdr: "Tiny" then the terminal EOP record.
        let mut phdr = name("Tiny", 20);
        phdr.extend(0u16.to_le_bytes()); // preset
        phdr.extend(0u16.to_le_bytes()); // bank
        phdr.extend(0u16.to_le_bytes()); // bag index
        phdr.extend([0u8; 12]); // library/genre/morphology
        phdr.extend(name("EOP", 20));
        phdr.extend(0u16.to_le_bytes());
        phdr.extend(0u16.to_le_bytes());
        phdr.extend(1u16.to_le_bytes()); // one preset zone
        phdr.extend([0u8; 12]);

        // pbag: zone 0 starts at generator 0; terminal record at 1.
        let mut pbag = Vec::new();
        pbag.extend(0u16.to_le_bytes());
        pbag.extend(0u16.to_le_bytes());
        pbag.extend(1u16.to_le_bytes());
        pbag.extend(0u16.to_le_bytes());

        // pgen: the preset zone points at instrument 0.
        let mut pgen = Vec::new();
        pgen.extend(op::INSTRUMENT.to_le_bytes());
        pgen.extend(0u16.to_le_bytes());
        pgen.extend([0u8; 4]); // terminal

        // inst: "TinyInst" then terminal.
        let mut inst = name("TinyInst", 20);
        inst.extend(0u16.to_le_bytes());
        inst.extend(name("EOI", 20));
        inst.extend(1u16.to_le_bytes());

        let mut ibag = Vec::new();
        ibag.extend(0u16.to_le_bytes());
        ibag.extend(0u16.to_le_bytes());
        ibag.extend(3u16.to_le_bytes());
        ibag.extend(0u16.to_le_bytes());

        // igen: keyRange 48..72, root key 60, sampleID 0.
        let mut igen = Vec::new();
        igen.extend(op::KEY_RANGE.to_le_bytes());
        igen.extend((48u16 | (72u16 << 8)).to_le_bytes());
        igen.extend(op::OVERRIDING_ROOT_KEY.to_le_bytes());
        igen.extend(60u16.to_le_bytes());
        igen.extend(op::SAMPLE_ID.to_le_bytes());
        igen.extend(0u16.to_le_bytes());
        igen.extend([0u8; 4]); // terminal

        // shdr: the whole buffer, 22050Hz, root 60.
        let mut shdr = name("TinySample", 20);
        shdr.extend(0u32.to_le_bytes()); // start
        shdr.extend(64u32.to_le_bytes()); // end
        shdr.extend(16u32.to_le_bytes()); // start loop
        shdr.extend(48u32.to_le_bytes()); // end loop
        shdr.extend(22050u32.to_le_bytes());
        shdr.push(60); // original pitch
        shdr.push(0); // pitch correction
        shdr.extend(0u16.to_le_bytes()); // sample link
        shdr.extend(1u16.to_le_bytes()); // sample type
        shdr.extend(name("EOS", 20));
        shdr.extend([0u8; 26]);

        let mut sdta = b"sdta".to_vec();
        sdta.extend(chunk(b"smpl", &smpl));
        let mut pdta = b"pdta".to_vec();
        for (id, body) in [
            (b"phdr", &phdr),
            (b"pbag", &pbag),
            (b"pgen", &pgen),
            (b"inst", &inst),
            (b"ibag", &ibag),
            (b"igen", &igen),
            (b"shdr", &shdr),
        ] {
            pdta.extend(chunk(id, body));
        }

        let mut body = b"sfbk".to_vec();
        body.extend(chunk(b"LIST", &sdta));
        body.extend(chunk(b"LIST", &pdta));
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn rejects_files_that_are_not_soundfonts() {
        assert!(parse(b"not a soundfont at all").is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn reads_a_preset_with_its_zone() {
        let sf = parse(&tiny_sf2()).expect("parse");
        assert_eq!(sf.preset_names(), ["Tiny"]);
        let preset = sf.preset(0).expect("preset 0");
        assert_eq!(preset.zones.len(), 1);
        let zone = &preset.zones[0];
        assert_eq!((zone.key_low, zone.key_high), (48, 72));
        assert_eq!(zone.sample.data.len(), 64);
        assert_eq!(zone.sample.sample_rate, 22050.0);
        // Root key 60 becomes 6000 cents, so MIDI 60 plays at its native rate.
        assert_eq!(zone.original_pitch, 6000.0);
        assert!((zone.playback_rate(60.0) - 1.0).abs() < 1e-12);
        assert!((zone.playback_rate(72.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn preset_index_wraps_like_a_sample_index() {
        let sf = parse(&tiny_sf2()).expect("parse");
        assert!(sf.preset(0).is_some());
        assert!(sf.preset(5).is_some()); // wraps back to the only preset
        assert!(sf.preset(-1).is_some());
    }

    #[test]
    fn zones_without_sample_modes_do_not_loop() {
        let sf = parse(&tiny_sf2()).expect("parse");
        assert!(!sf.preset(0).unwrap().zones[0].loops());
    }
}
