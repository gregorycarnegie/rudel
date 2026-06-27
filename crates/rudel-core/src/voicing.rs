// voicing.rs - chord-symbol voicings. Ported from strudel/packages/tonal/
// {voicings,tonleiter,ireal}.mjs. The recommended `voicing` path
// (`renderVoicing`) lives entirely in tonleiter.mjs with no external dependency,
// so it ports cleanly here; the curated dictionaries from voicings.mjs
// (lefthand / triads / guidetones / legacy) plus the default iReal dictionaries
// (`ireal` = `simple`, `ireal-ext` = `complex`) are inlined. The deprecated
// `voicings()` voice-leading (external `chord-voicings` package) is the one
// intentional gap: rudel's `voicings(dict)` instead aliases `voicing` with a
// named dictionary (no smoothest-voice-leading state).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::{Pattern, pure, silence, stack},
    tonal::{chord_symbol, interval_to_semitones, letter_semitone, note_to_midi_with_octave},
    value::{Value, ValueMap},
};

type VoicingTable = phf::OrderedMap<&'static str, &'static [&'static str]>;

/// How a voicing is aligned to the anchor note.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Top note <= anchor.
    Below,
    /// Top note <= anchor, with notes equal to the anchor dropped.
    Duck,
    /// Bottom note >= anchor.
    Above,
    /// Bottom note used as the target; always picks the first voicing.
    Root,
}

impl Mode {
    fn from_str(s: &str) -> Mode {
        match s {
            "above" => Mode::Above,
            "duck" => Mode::Duck,
            "root" => Mode::Root,
            _ => Mode::Below,
        }
    }

    /// The note in a voicing the anchor is compared against.
    fn target(self, voicing: &[i32]) -> i32 {
        match self {
            Mode::Above | Mode::Root => voicing[0],
            Mode::Below | Mode::Duck => *voicing.last().unwrap(),
        }
    }
}

/// A named voicing dictionary plus its default alignment.
struct Dictionary {
    /// chord symbol -> list of voicings (each a list of interval strings).
    table: &'static VoicingTable,
    mode: Mode,
    /// Anchor note name (parsed with default octave 4).
    anchor: &'static str,
}

static LEFTHAND: VoicingTable = phf::phf_ordered_map! {
    "m7" => &["3m 5P 7m 9M", "7m 9M 10m 12P"],
    "7" => &["3M 6M 7m 9M", "7m 9M 10M 13M"],
    "^7" => &["3M 5P 7M 9M", "7M 9M 10M 12P"],
    "69" => &["3M 5P 6A 9M"],
    "m7b5" => &["3m 5d 7m 8P", "7m 8P 10m 12d"],
    "7b9" => &["3M 6m 7m 9m", "7m 9m 10M 13m"],
    "7b13" => &["3M 6m 7m 9m", "7m 9m 10M 13m"],
    "o7" => &["1P 3m 5d 6M", "5d 6M 8P 10m"],
    "7#11" => &["7m 9M 11A 13A"],
    "7#9" => &["3M 7m 9A"],
    "mM7" => &["3m 5P 7M 9M", "7M 9M 10m 12P"],
    "m6" => &["3m 5P 6M 9M", "6M 9M 10m 12P"],
};

static TRIADS: VoicingTable = phf::phf_ordered_map! {
    "" => &["1P 3M 5P", "3M 5P 8P", "5P 8P 10M"],
    "M" => &["1P 3M 5P", "3M 5P 8P", "5P 8P 10M"],
    "m" => &["1P 3m 5P", "3m 5P 8P", "5P 8P 10m"],
    "o" => &["1P 3m 5d", "3m 5d 8P", "5d 8P 10m"],
    "aug" => &["1P 3m 5A", "3m 5A 8P", "5A 8P 10m"],
};

static GUIDETONES: VoicingTable = phf::phf_ordered_map! {
    "m7" => &["3m 7m", "7m 10m"],
    "m9" => &["3m 7m", "7m 10m"],
    "7" => &["3M 7m", "7m 10M"],
    "^7" => &["3M 7M", "7M 10M"],
    "^9" => &["3M 7M", "7M 10M"],
    "69" => &["3M 6M"],
    "6" => &["3M 6M", "6M 10M"],
    "m7b5" => &["3m 7m", "7m 10m"],
    "7b9" => &["3M 7m", "7m 10M"],
    "7b13" => &["3M 7m", "7m 10M"],
    "o7" => &["3m 6M", "6M 10m"],
    "7#11" => &["3M 7m", "7m 10M"],
    "7#9" => &["3M 7m", "7m 10M"],
    "mM7" => &["3m 7M", "7M 10m"],
    "m6" => &["3m 6M", "6M 10m"],
};

static LEGACY: VoicingTable = phf::phf_ordered_map! {
    "" => &["1P 3M 5P", "3M 5P 8P", "5P 8P 10M"],
    "M" => &["1P 3M 5P", "3M 5P 8P", "5P 8P 10M"],
    "m" => &["1P 3m 5P", "3m 5P 8P", "5P 8P 10m"],
    "o" => &["1P 3m 5d", "3m 5d 8P", "5d 8P 10m"],
    "aug" => &["1P 3m 5A", "3m 5A 8P", "5A 8P 10m"],
    "m7" => &["3m 5P 7m 9M", "7m 9M 10m 12P"],
    "7" => &["3M 6M 7m 9M", "7m 9M 10M 13M"],
    "^7" => &["3M 5P 7M 9M", "7M 9M 10M 12P"],
    "69" => &["3M 5P 6A 9M"],
    "m7b5" => &["3m 5d 7m 8P", "7m 8P 10m 12d"],
    "7b9" => &["3M 6m 7m 9m", "7m 9m 10M 13m"],
    "7b13" => &["3M 6m 7m 9m", "7m 9m 10M 13m"],
    "o7" => &["1P 3m 5d 6M", "5d 6M 8P 10m"],
    "7#11" => &["7m 9M 11A 13A"],
    "7#9" => &["3M 7m 9A"],
    "mM7" => &["3m 5P 7M 9M", "7M 9M 10m 12P"],
    "m6" => &["3m 5P 6M 9M", "6M 9M 10m 12P"],
};

// The default `ireal` (`simple`) and `ireal-ext` (`complex`) dictionaries from
// `ireal.mjs`, generated from the real package (with `voicingAlias` side-effects
// applied, so `^`/`-`/`+`/`M`/`m`/`aug` spellings are all present) via
// `tools/oracle` — see the voicing oracle. Unlike the curated dicts above,
// `registerVoicings` registers these with no mode/anchor, so they fall back to
// `renderVoicing`'s defaults: mode `below`, anchor `c5`.
static IREAL: VoicingTable = phf::phf_ordered_map! {
    "" => &[
        "1P 5P 8P 10M",
        "1P 5P 8P 10M 12P",
        "3M 5P 8P 10M 12P",
        "3M 8P 10M 12P 15P",
        "5P 8P 10M 12P 15P",
    ],
    "+" => &[
        "1P 3M 6m 8P 10M",
        "1P 6m 8P 10M 13m",
        "3M 6m 8P 10M 13m",
        "3M 8P 10M 13m 15P",
        "6m 8P 10M 13m 15P",
        "6m 10M 13m 15P 17M",
    ],
    "-" => &[
        "1P 3m 5P 8P 10m",
        "1P 5P 8P 10m 12P",
        "3m 5P 8P 10m 12P",
        "5P 8P 10m 12P 15P",
    ],
    "-#5" => &["1P 6m 8P 10m 13m", "3m 6m 8P 10m 13m", "6m 8P 10m 13m 15P"],
    "-11" => &[
        "1P 3m 7m 9M 11P",
        "3m 7m 8P 9M 11P",
        "1P 4P 7m 10m 12P",
        "5P 8P 11P 14m",
        "3m 7m 9M 11P 15P",
        "5P 8P 11P 14m 16M",
        "7m 10m 12P 15P 18P",
    ],
    "-6" => &[
        "1P 3m 5P 6M 8P",
        "1P 5P 6M 8P 10m",
        "3m 5P 6M 8P 10m",
        "1P 5P 8P 10m 13M",
        "3m 5P 8P 10m 13M",
        "5P 8P 10m 12P 13M",
        "5P 8P 10m 13M 15P",
    ],
    "-69" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "3m 6M 9M 10m 12P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 9M 10m 13M",
        "5P 8P 10m 13M 16M",
    ],
    "-7" => &[
        "1P 3m 5P 7m 10m",
        "1P 5P 7m 10m 12P",
        "3m 7m 8P 10m 12P",
        "3m 7m 8P 10m 14m",
        "5P 7m 8P 10m 14m",
        "7m 10m 12P 14m 15P",
        "5P 8P 10m 14m 17m",
        "7m 10m 12P 15P 17m",
    ],
    "-7b5" => &[
        "3m 5d 7m 8P 10m",
        "1P 7m 10m 12d",
        "1P 5d 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 7m 8P 10m 14m",
        "5d 8P 10m 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "-9" => &[
        "1P 3m 5P 7m 9M",
        "3m 5P 7m 8P 9M",
        "3m 7m 8P 9M 12P",
        "5P 8P 9M 10m 14m",
        "3m 7m 9M 12P 15P",
        "7m 10m 12P 15P 16M",
    ],
    "-M7" => &[
        "1P 3m 5P 7M 10m",
        "1P 5P 7M 10m 12P",
        "3m 7M 8P 10m 12P",
        "5P 7M 8P 10m 14M",
        "5P 8P 10m 14M 17m",
    ],
    "-M9" => &[
        "1P 3m 5P 7M 9M",
        "1P 7M 9M 10m 12P",
        "3m 7M 8P 9M 12P",
        "5P 8P 9M 10m 14M",
    ],
    "-^7" => &[
        "1P 3m 5P 7M 10m",
        "1P 5P 7M 10m 12P",
        "3m 7M 8P 10m 12P",
        "5P 7M 8P 10m 14M",
        "5P 8P 10m 14M 17m",
    ],
    "-^9" => &[
        "1P 3m 5P 7M 9M",
        "1P 7M 9M 10m 12P",
        "3m 7M 8P 9M 12P",
        "5P 8P 9M 10m 14M",
    ],
    "-add9" => &[
        "1P 2M 3m 5P 8P",
        "1P 3m 5P 9M",
        "3m 5P 8P 9M 12P",
        "5P 8P 9M 10m 12P",
    ],
    "-b6" => &[
        "1P 5P 6m 8P 10m",
        "1P 5P 8P 10m 13m",
        "3m 5P 8P 10m 13m",
        "5P 8P 10m 13m",
        "5P 8P 10m 13m 15P",
    ],
    "11" => &[
        "1P 5P 7m 9M 11P",
        "5P 7m 8P 9M 11P",
        "7m 8P 9M 11P 12P",
        "7m 8P 11P 12P 16M",
    ],
    "13" => &[
        "1P 6M 7m 9M 10M",
        "1P 7m 9M 10M 13M",
        "3M 7m 8P 9M 13M",
        "7m 8P 9M 10M 13M",
        "7m 9M 10M 13M 15P",
    ],
    "13#11" => &["1P 6M 7m 10M 12d", "3M 7m 9M 12d 13M", "7m 10M 12d 13M 16M"],
    "13#9" => &["1P 3M 6M 7m 10m", "3M 7m 8P 10m 13M", "7m 10M 13M 14m 17m"],
    "13b9" => &[
        "1P 3M 6M 7m 9m",
        "1P 6M 7m 9m 10M",
        "3M 7m 9m 10M 13M",
        "3M 7m 10M 13M 16m",
        "7m 10M 13M 16m 17M",
    ],
    "13sus" => &[
        "1P 4P 6M 7m 9M",
        "1P 7m 9M 11P 13M",
        "5P 7m 9M 11P 13M",
        "7m 9M 11P 13M 15P",
    ],
    "2" => &["1P 5P 8P 9M", "1P 5P 8P 9M 12P", "5P 8P 9M 12P"],
    "5" => &["1P 5P 8P 12P", "5P 8P 12P 15P"],
    "6" => &[
        "1P 5P 6M 8P 10M",
        "1P 5P 8P 10M 13M",
        "3M 5P 8P 10M 13M",
        "5P 8P 10M 12P 13M",
    ],
    "69" => &[
        "1P 5P 6M 9M 10M",
        "1P 5P 9M 10M 13M",
        "3M 5P 8P 9M 13M",
        "5P 8P 9M 10M 13M",
    ],
    "7" => &[
        "1P 5P 7m 8P 10M",
        "1P 7m 8P 10M 12P",
        "3M 7m 8P 10M 12P",
        "3M 7m 8P 10M 14m",
        "3M 7m 10M 12P 15P",
        "7m 10M 12P 14m 15P",
        "7m 10M 12P 15P 17M",
    ],
    "7#11" => &["1P 3M 7m 10M 12d", "3M 7m 8P 10M 12d", "7m 10M 12d 14m 15P"],
    "7#5" => &[
        "1P 3M 7m 10M 13m",
        "3M 7m 8P 10M 13m",
        "3M 7m 8P 13m 14m",
        "7m 10M 13m 14m 15P",
    ],
    "7#9" => &["1P 3M 7m 10m", "3M 7m 8P 10m 14m", "7m 10m 10M 14m 15P"],
    "7#9#11" => &[
        "1P 3M 7m 10m 12d",
        "3M 7m 10m 12d 15P",
        "7m 10M 12d 15P 17m",
    ],
    "7#9#5" => &[
        "1P 3M 7m 10m 13m",
        "3M 7m 10m 13m 15P",
        "7m 10M 13m 15P 17m",
    ],
    "7#9b5" => &[
        "1P 3M 7m 10m 12d",
        "3M 7m 10m 12d 15P",
        "7m 10M 12d 15P 17m",
    ],
    "7alt" => &[
        "3M 7m 8P 9m 12d",
        "1P 7m 10m 10M 13m",
        "3M 7m 8P 10m 13m",
        "3M 7m 9m 12d 15P",
        "3M 7m 10m 13m 15P",
        "7m 10M 12d 15P 17m",
        "7m 10M 13m 15P 17m",
    ],
    "7b13" => &[
        "1P 3M 7m 10M 13m",
        "3M 7m 8P 10M 13m",
        "3M 7m 8P 13m 14m",
        "7m 10M 13m 14m 15P",
    ],
    "7b13sus" => &["1P 5P 7m 11P 13m", "5P 7m 8P 11P 13m", "7m 11P 13m 14m 15P"],
    "7b5" => &["1P 3M 7m 10M 12d", "3M 7m 8P 10M 12d", "7m 10M 12d 14m 15P"],
    "7b9" => &[
        "1P 3M 7m 9m 10M",
        "3M 7m 8P 9m 10M",
        "3M 7m 8P 9m 14m",
        "7m 9m 10M 14m 15P",
    ],
    "7b9#11" => &["1P 7m 9m 10M 12d", "3M 7m 8P 9m 12d", "7m 8P 10M 12d 16m"],
    "7b9#5" => &["1P 7m 9m 10M 13m", "3M 7m 8P 9m 13m", "7m 9m 10M 13m 15P"],
    "7b9#9" => &["1P 3M 7m 9m 10m", "3M 7m 8P 9m 10m", "7m 8P 10M 16m 17m"],
    "7b9b13" => &["1P 7m 9m 10M 13m", "3M 7m 8P 9m 13m", "7m 9m 10M 13m 15P"],
    "7b9b5" => &["1P 7m 9m 10M 12d", "3M 7m 8P 9m 12d", "7m 8P 10M 12d 16m"],
    "7b9sus" => &["1P 5P 7m 9m 11P", "5P 7m 8P 9m 11P", "7m 8P 11P 14m 16m"],
    "7sus" => &[
        "1P 5P 7m 8P 11P",
        "5P 8P 11P 12P 14m",
        "7m 8P 11P 12P 14m",
        "7m 11P 12P 14m 18P",
    ],
    "7susadd3" => &["1P 4P 5P 7m 10M", "5P 8P 10M 11P 14m", "7m 11P 12P 15P 17M"],
    "9" => &[
        "1P 5P 7m 9M 10M",
        "1P 7m 9M 10M 12P",
        "3M 7m 8P 9M 12P",
        "7m 9M 10M 14m 15P",
        "3M 7m 8P 12P 16M",
        "7m 10M 12P 15P 16M",
    ],
    "9#11" => &["1P 7m 9M 10M 12d", "3M 7m 8P 9M 12d", "7m 10M 12d 15P 16M"],
    "9#5" => &[
        "1P 7m 9M 10M 13m",
        "3M 7m 9M 10M 13m",
        "3M 7m 9M 13m 14m",
        "7m 10M 13m 14m 16M",
        "7m 10M 13m 16M 17M",
    ],
    "9b5" => &["1P 7m 9M 10M 12d", "3M 7m 8P 9M 12d", "7m 10M 12d 15P 16M"],
    "9sus" => &[
        "1P 5P 7m 9M 11P",
        "5P 7m 8P 9M 11P",
        "7m 8P 9M 11P 12P",
        "7m 8P 11P 12P 16M",
    ],
    "M" => &[
        "1P 5P 8P 10M",
        "1P 5P 8P 10M 12P",
        "3M 5P 8P 10M 12P",
        "3M 8P 10M 12P 15P",
        "5P 8P 10M 12P 15P",
    ],
    "M13" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 8P 13M 16M",
        "7M 8P 10M 13M 16M",
    ],
    "M7" => &[
        "1P 5P 7M 10M 12P",
        "1P 10M 12P 14M",
        "3M 8P 10M 12P 14M",
        "5P 8P 10M 12P 14M",
        "5P 8P 10M 14M 17M",
    ],
    "M7#11" => &[
        "1P 5P 7M 10M 12d",
        "3M 7M 8P 10M 12d",
        "1P 7M 10M 12d 14M",
        "3M 7M 8P 12d 14M",
        "5P 8P 10M 12d 14M",
    ],
    "M7#5" => &["1P 6m 7M 10M 13m", "3M 7M 8P 10M 13m", "6m 7M 8P 10M 13m"],
    "M9" => &[
        "1P 5P 7M 9M 10M",
        "1P 7M 9M 10M 12P",
        "3M 7M 8P 9M 12P",
        "3M 7M 8P 12P 16M",
        "5P 8P 10M 14M 16M",
        "7M 8P 10M 12P 16M",
    ],
    "M9#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 8P 9M 12d",
        "3M 8P 9M 12d 14M",
    ],
    "^" => &[
        "1P 5P 8P 10M",
        "1P 5P 8P 10M 12P",
        "3M 5P 8P 10M 12P",
        "3M 8P 10M 12P 15P",
        "5P 8P 10M 12P 15P",
    ],
    "^13" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 8P 13M 16M",
        "7M 8P 10M 13M 16M",
    ],
    "^7" => &[
        "1P 5P 7M 10M 12P",
        "1P 10M 12P 14M",
        "3M 8P 10M 12P 14M",
        "5P 8P 10M 12P 14M",
        "5P 8P 10M 14M 17M",
    ],
    "^7#11" => &[
        "1P 5P 7M 10M 12d",
        "3M 7M 8P 10M 12d",
        "1P 7M 10M 12d 14M",
        "3M 7M 8P 12d 14M",
        "5P 8P 10M 12d 14M",
    ],
    "^7#5" => &["1P 6m 7M 10M 13m", "3M 7M 8P 10M 13m", "6m 7M 8P 10M 13m"],
    "^9" => &[
        "1P 5P 7M 9M 10M",
        "1P 7M 9M 10M 12P",
        "3M 7M 8P 9M 12P",
        "3M 7M 8P 12P 16M",
        "5P 8P 10M 14M 16M",
        "7M 8P 10M 12P 16M",
    ],
    "^9#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 8P 9M 12d",
        "3M 8P 9M 12d 14M",
    ],
    "add9" => &[
        "1P 5P 8P 9M 10M",
        "1P 5P 9M 10M 12P",
        "3M 8P 9M 10M 12P",
        "3M 8P 9M 12P 15P",
        "5P 8P 9M 12P 17M",
    ],
    "aug" => &[
        "1P 3M 6m 8P 10M",
        "1P 6m 8P 10M 13m",
        "3M 6m 8P 10M 13m",
        "3M 8P 10M 13m 15P",
        "6m 8P 10M 13m 15P",
        "6m 10M 13m 15P 17M",
    ],
    "h" => &[
        "3m 5d 7m 8P 10m",
        "1P 5d 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 7m 8P 10m 14m",
        "5d 8P 10m 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "h7" => &[
        "3m 5d 7m 8P 10m",
        "1P 5d 7m 10m 12d",
        "1P 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 7m 8P 10m 14m",
        "5d 8P 10m 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "h9" => &[
        "1P 7m 9M 10m 12d",
        "3m 7m 8P 9M 12d",
        "5d 8P 9M 10m 14m",
        "7m 10m 12d 15P 16M",
    ],
    "m" => &[
        "1P 3m 5P 8P 10m",
        "1P 5P 8P 10m 12P",
        "3m 5P 8P 10m 12P",
        "5P 8P 10m 12P 15P",
    ],
    "m#5" => &["1P 6m 8P 10m 13m", "3m 6m 8P 10m 13m", "6m 8P 10m 13m 15P"],
    "m11" => &[
        "1P 3m 7m 9M 11P",
        "3m 7m 8P 9M 11P",
        "1P 4P 7m 10m 12P",
        "5P 8P 11P 14m",
        "3m 7m 9M 11P 15P",
        "5P 8P 11P 14m 16M",
        "7m 10m 12P 15P 18P",
    ],
    "m6" => &[
        "1P 3m 5P 6M 8P",
        "1P 5P 6M 8P 10m",
        "3m 5P 6M 8P 10m",
        "1P 5P 8P 10m 13M",
        "3m 5P 8P 10m 13M",
        "5P 8P 10m 12P 13M",
        "5P 8P 10m 13M 15P",
    ],
    "m69" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "3m 6M 9M 10m 12P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 9M 10m 13M",
        "5P 8P 10m 13M 16M",
    ],
    "m7" => &[
        "1P 3m 5P 7m 10m",
        "1P 5P 7m 10m 12P",
        "3m 7m 8P 10m 12P",
        "3m 7m 8P 10m 14m",
        "5P 7m 8P 10m 14m",
        "7m 10m 12P 14m 15P",
        "5P 8P 10m 14m 17m",
        "7m 10m 12P 15P 17m",
    ],
    "m7b5" => &[
        "3m 5d 7m 8P 10m",
        "1P 7m 10m 12d",
        "1P 5d 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 7m 8P 10m 14m",
        "5d 8P 10m 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "m9" => &[
        "1P 3m 5P 7m 9M",
        "3m 5P 7m 8P 9M",
        "3m 7m 8P 9M 12P",
        "5P 8P 9M 10m 14m",
        "3m 7m 9M 12P 15P",
        "7m 10m 12P 15P 16M",
    ],
    "m^7" => &[
        "1P 3m 5P 7M 10m",
        "1P 5P 7M 10m 12P",
        "3m 7M 8P 10m 12P",
        "5P 7M 8P 10m 14M",
        "5P 8P 10m 14M 17m",
    ],
    "m^9" => &[
        "1P 3m 5P 7M 9M",
        "1P 7M 9M 10m 12P",
        "3m 7M 8P 9M 12P",
        "5P 8P 9M 10m 14M",
    ],
    "madd9" => &[
        "1P 2M 3m 5P 8P",
        "1P 3m 5P 9M",
        "3m 5P 8P 9M 12P",
        "5P 8P 9M 10m 12P",
    ],
    "mb6" => &[
        "1P 5P 6m 8P 10m",
        "1P 5P 8P 10m 13m",
        "3m 5P 8P 10m 13m",
        "5P 8P 10m 13m",
        "5P 8P 10m 13m 15P",
    ],
    "o" => &["1P 5d 8P 10m 12d", "3m 8P 10m 12d 15P", "5d 8P 10m 12d 15P"],
    "o7" => &[
        "1P 6M 8P 10m 12d",
        "1P 6M 10m 12d 13M",
        "3m 8P 10m 12d 13M",
        "3m 8P 12d 13M 15P",
        "5d 10m 12d 13M 15P",
        "5d 10m 13M 15P 17m",
        "6M 12d 13M 15P 17m",
        "6M 12d 15P 17m 19d",
    ],
    "sus" => &[
        "1P 4P 5P 8P",
        "1P 4P 5P 8P 11P",
        "5P 8P 11P 12P",
        "5P 8P 11P 12P 15P",
    ],
};

static IREAL_EXT: VoicingTable = phf::phf_ordered_map! {
    "" => &[
        "1P 3M 5P 6M 9M",
        "1P 5P 8P 10M 12P",
        "3M 5P 9M 10M 12P",
        "1P 5P 8P 10M 13M",
        "3M 8P 10M 13M 15P",
        "5P 9M 10M 12P 15P",
    ],
    "+" => &[
        "1P 6m 8P 9M 10M",
        "1P 6m 8P 10M 13m",
        "3M 8P 9M 10M 13m",
        "3M 8P 10M 13m 15P",
        "6m 10M 13m 15P 16M",
        "6m 10M 13m 15P 17M",
    ],
    "-" => &[
        "1P 3m 5P 8P 10m",
        "1P 3m 5P 9M 11P",
        "3m 5P 8P 9M 11P",
        "5P 8P 9M 10m 11P",
        "1P 5P 9M 10m 12P",
        "3m 5P 8P 10m 12P",
        "5P 8P 10m 12P 15P",
    ],
    "-#5" => &["1P 6m 8P 10m 13m", "3m 6m 8P 11P 13m", "6m 8P 10m 13m 15P"],
    "-11" => &[
        "3m 5P 7m 9M 11P",
        "7m 9M 10m 11P",
        "1P 4P 7m 10m 12P",
        "3m 7m 9M 11P 12P",
        "7m 9M 10m 11P 12P",
        "3m 7m 9M 11P 14m",
        "4P 10m 12P 14m",
        "5P 8P 11P 14m",
        "5P 8P 11P 14m 16M",
        "7m 10m 12P 16M 18P",
        "7m 10m 11P 16M 21m",
    ],
    "-6" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "1P 5P 6M 10m 11P",
        "3m 5P 6M 8P 11P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 10m 11P 13M",
        "5P 8P 10m 13M 16M",
    ],
    "-69" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "3m 6M 9M 10m 12P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 9M 10m 13M",
        "5P 8P 10m 13M 16M",
    ],
    "-7" => &[
        "1P 3m 5P 7m 9M",
        "1P 3m 5P 7m 10m",
        "1P 5P 7m 10m 11P",
        "3m 7m 8P 10m 11P",
        "1P 5P 7m 10m 12P",
        "3m 7m 9M 10m 12P",
        "3m 7m 8P 10m 14m",
        "5P 7m 9M 10m 14m",
        "7m 10m 11P 14m 15P",
        "7m 10m 12P 15P 16M",
        "5P 8P 11P 14m 17m",
        "7m 10m 12P 15P 17m",
    ],
    "-7b5" => &[
        "1P 5d 7m 10m 11P",
        "3m 5d 7m 8P 11P",
        "5d 7m 8P 10m 11P",
        "1P 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 8P 10m 11P 14m",
        "7m 10m 11P 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "-9" => &[
        "1P 3m 5P 7m 9M",
        "1P 3m 7m 9M 11P",
        "3m 7m 9M 10m 11P",
        "3m 7m 9M 10m 12P",
        "3m 7m 9M 10m 14m",
        "3m 7m 9M 12P 15P",
        "7m 10m 11P 14m 16M",
        "7m 10m 12P 16M 18P",
    ],
    "-M7" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "-M9" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "-^7" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "-^9" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "-add9" => &[
        "1P 2M 3m 5P 8P",
        "1P 3m 5P 9M",
        "3m 5P 8P 9M 12P",
        "5P 8P 9M 10m 12P",
    ],
    "-b6" => &["1P 3m 5P 6m 8P", "3m 5P 8P 11P 13m", "5P 8P 10m 11P 13m"],
    "11" => &[
        "1P 4P 6M 7m 9M",
        "1P 5P 7m 9M 11P",
        "4P 6M 7m 9M 11P",
        "5P 8P 9M 11P 14m",
        "7m 9M 11P 13M 15P",
        "7m 11P 12P 14m 18P",
    ],
    "13" => &[
        "3M 7m 9M 10M 13M",
        "3M 7m 9M 13M 15P",
        "3M 7m 10M 13M 16M",
        "7m 10M 12P 13M 16M",
        "7m 10M 13M 16M 17M",
        "7m 10M 13M 16M 19P",
    ],
    "13#11" => &["3M 7m 9M 12d 13M", "7m 10M 12d 13M 16M"],
    "13#9" => &["3M 7m 10m 10M 13M", "7m 10M 13M 14m 17m"],
    "13b9" => &[
        "3M 7m 9m 10M 13M",
        "3M 7m 10M 13M 16m",
        "7m 10M 13M 16m 17M",
    ],
    "13sus" => &[
        "1P 4P 6M 7m 9M",
        "1P 7m 9M 11P 13M",
        "4P 7m 9M 11P 13M",
        "7m 9M 11P 13M 15P",
        "7m 11P 13M 14m 16M",
        "7m 11P 13M 16M 18P",
    ],
    "2" => &[
        "1P 5P 6M 8P 9M",
        "1P 5P 8P 9M 12P",
        "5P 8P 9M 12P 13M",
        "5P 8P 9M 12P 15P",
    ],
    "5" => &[
        "1P 5P 8P 12P",
        "1P 5P 8P 9M 12P",
        "5P 8P 12P 15P",
        "5P 8P 12P 15P 16M",
    ],
    "6" => &[
        "1P 5P 6M 9M 10M",
        "1P 5P 9M 10M 13M",
        "3M 5P 9M 10M 13M",
        "5P 8P 9M 10M 13M",
        "3M 6M 9M 12P 15P",
    ],
    "69" => &[
        "1P 5P 6M 9M 10M",
        "1P 5P 9M 10M 13M",
        "3M 5P 9M 10M 13M",
        "5P 8P 9M 10M 13M",
        "3M 6M 9M 12P 15P",
    ],
    "7" => &[
        "1P 5P 7m 8P 10M",
        "1P 7m 8P 10M 12P",
        "3M 7m 8P 10M 12P",
        "3M 7m 8P 10M 14m",
        "3M 7m 10M 12P 15P",
        "7m 10M 12P 14m 15P",
        "7m 10M 12P 15P 17M",
        "7m 10M 14m 17M 19P",
    ],
    "7#11" => &["1P 3M 7m 9M 12d", "3M 7m 9M 12d 13M", "7m 10M 12d 13M 16M"],
    "7#5" => &[
        "1P 3M 7m 10M 13m",
        "3M 7m 8P 10M 13m",
        "3M 7m 8P 13m 14m",
        "7m 10M 13m 14m 15P",
        "7m 10M 13m 14m 17M",
    ],
    "7#9" => &[
        "1P 3M 7m 10m",
        "3M 7m 10m 10M 12P",
        "3M 7m 10m 12P 14m",
        "7m 10M 12P 14m 17m",
    ],
    "7#9#11" => &[
        "3M 7m 10m 10M 12d",
        "3M 7m 10m 12d 14m",
        "7m 10M 12d 14m 17m",
    ],
    "7#9#5" => &[
        "3M 7m 10m 10M 13m",
        "3M 7m 10m 13m 14m",
        "7m 10M 13m 14m 17m",
    ],
    "7#9b5" => &[
        "3M 7m 10m 10M 12d",
        "3M 7m 10m 12d 14m",
        "7m 10M 12d 14m 17m",
    ],
    "7alt" => &[
        "3M 7m 8P 10m 13m",
        "3M 7m 9m 12d 13m",
        "3M 7m 9m 10m 13m",
        "3M 7m 10m 13m 14m",
        "3M 7m 9m 12d 14m",
        "3M 7m 10m 13m 15P",
        "3M 7m 10m 13m 16m",
        "7m 10M 12d 14m 16m",
        "7m 10M 12d 13m 16m",
        "7m 10M 13m 15P 17m",
        "7m 10M 13m 16m 17m",
        "7m 10M 13m 16m 19d",
    ],
    "7b13" => &[
        "1P 3M 7m 10M 13m",
        "3M 7m 8P 10M 13m",
        "3M 7m 8P 13m 14m",
        "7m 10M 13m 14m 15P",
        "7m 10M 13m 14m 17M",
    ],
    "7b13sus" => &["1P 5P 7m 11P 13m", "5P 7m 8P 11P 13m", "7m 11P 13m 14m 15P"],
    "7b5" => &["1P 3M 7m 9M 12d", "3M 7m 9M 12d 13M", "7m 10M 12d 13M 16M"],
    "7b9" => &[
        "1P 3M 7m 9m 10M",
        "3M 7m 8P 9m 10M",
        "3M 7m 8P 9m 14m",
        "7m 9m 10M 14m 15P",
    ],
    "7b9#11" => &[
        "3M 7m 9m 10M 12d",
        "3M 7m 9m 12d 14m",
        "7m 8P 10M 12d 16m",
        "7m 10M 12d 14m 16m",
    ],
    "7b9#5" => &[
        "1P 7m 9m 10M 13m",
        "3M 7m 9m 10M 13m",
        "3M 7m 10M 13m 16m",
        "7m 10M 13m 14m 16m",
        "7m 10M 13m 16m 17M",
    ],
    "7b9#9" => &["1P 3M 7m 9m 10m", "3M 7m 10m 13m 16m", "7m 10M 13m 16m 17m"],
    "7b9b13" => &[
        "1P 7m 9m 10M 13m",
        "3M 7m 9m 10M 13m",
        "3M 7m 10M 13m 16m",
        "7m 10M 13m 14m 16m",
        "7m 10M 13m 16m 17M",
    ],
    "7b9b5" => &[
        "3M 7m 9m 10M 12d",
        "3M 7m 9m 12d 14m",
        "7m 8P 10M 12d 16m",
        "7m 10M 12d 14m 16m",
    ],
    "7b9sus" => &["1P 5P 7m 9m 11P", "5P 7m 8P 9m 11P", "7m 8P 11P 14m 16m"],
    "7sus" => &[
        "1P 4P 6M 7m 9M",
        "1P 5P 7m 9M 11P",
        "4P 6M 7m 9M 11P",
        "5P 8P 9M 11P 14m",
        "7m 9M 11P 13M 15P",
        "7m 11P 12P 14m 18P",
    ],
    "7susadd3" => &["1P 4P 5P 7m 10M", "5P 8P 10M 11P 14m", "7m 11P 12P 15P 17M"],
    "9" => &[
        "1P 6M 7m 9M 10M",
        "3M 7m 9M 10M 12P",
        "1P 7m 9M 10M 13M",
        "3M 7m 9M 10M 13M",
        "3M 7m 9M 12P 15P",
        "7m 10M 12P 13M 16M",
        "7m 10M 13M 16M 17M",
        "7m 10M 13M 16M 19P",
    ],
    "9#11" => &["1P 7m 9M 10M 12d", "3M 7m 8P 9M 12d", "7m 10M 12d 15P 16M"],
    "9#5" => &[
        "1P 7m 9M 10M 13m",
        "3M 7m 9M 10M 13m",
        "3M 7m 9M 13m 14m",
        "7m 10M 13m 14m 16M",
        "7m 10M 13m 16M 17M",
    ],
    "9b5" => &["1P 7m 9M 10M 12d", "3M 7m 8P 9M 12d", "7m 10M 12d 15P 16M"],
    "9sus" => &[
        "1P 4P 6M 7m 9M",
        "1P 5P 7m 9M 11P",
        "4P 6M 7m 9M 11P",
        "5P 8P 9M 11P 14m",
        "7m 9M 11P 13M 15P",
        "7m 11P 12P 14m 18P",
    ],
    "M" => &[
        "1P 3M 5P 6M 9M",
        "1P 5P 8P 10M 12P",
        "3M 5P 9M 10M 12P",
        "1P 5P 8P 10M 13M",
        "3M 8P 10M 13M 15P",
        "5P 9M 10M 12P 15P",
    ],
    "M13" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "M7" => &[
        "1P 6M 7M 9M 10M",
        "3M 7M 9M 10M 12P",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "M7#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 9M 10M 12d",
        "3M 7M 9M 12d 13M",
        "3M 7M 10M 12d 14M",
        "7M 10M 12d 13M 14M",
        "7M 10M 12d 13M 16M",
        "7M 10M 12d 14M 17M",
    ],
    "M7#5" => &[
        "1P 6m 7M 10M 13m",
        "3M 7M 9M 10M 13m",
        "3M 7M 10M 13m 14M",
        "7M 10M 13m 14M 16M",
        "7M 10M 13m 14M 17M",
    ],
    "M9" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "M9#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 9M 10M 12d",
        "3M 7M 9M 12d 13M",
        "3M 7M 9M 12d 14M",
        "7M 10M 12d 14M 16M",
        "7M 10M 12d 13M 16M",
    ],
    "^" => &[
        "1P 3M 5P 6M 9M",
        "1P 5P 8P 10M 12P",
        "3M 5P 9M 10M 12P",
        "1P 5P 8P 10M 13M",
        "3M 8P 10M 13M 15P",
        "5P 9M 10M 12P 15P",
    ],
    "^13" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "^7" => &[
        "1P 6M 7M 9M 10M",
        "3M 7M 9M 10M 12P",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "^7#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 9M 10M 12d",
        "3M 7M 9M 12d 13M",
        "3M 7M 10M 12d 14M",
        "7M 10M 12d 13M 14M",
        "7M 10M 12d 13M 16M",
        "7M 10M 12d 14M 17M",
    ],
    "^7#5" => &[
        "1P 6m 7M 10M 13m",
        "3M 7M 9M 10M 13m",
        "3M 7M 10M 13m 14M",
        "7M 10M 13m 14M 16M",
        "7M 10M 13m 14M 17M",
    ],
    "^9" => &[
        "1P 6M 7M 9M 10M",
        "1P 7M 9M 10M 13M",
        "3M 7M 9M 10M 13M",
        "3M 7M 9M 12P 13M",
        "3M 7M 8P 9M 13M",
        "3M 7M 9M 13M 14M",
        "3M 7M 10M 13M 16M",
        "7M 10M 13M 14M 16M",
        "7M 10M 13M 16M 17M",
        "7M 10M 13M 16M 19P",
    ],
    "^9#11" => &[
        "1P 3M 5d 7M 9M",
        "1P 7M 9M 10M 12d",
        "3M 7M 9M 10M 12d",
        "3M 7M 9M 12d 13M",
        "3M 7M 9M 12d 14M",
        "7M 10M 12d 14M 16M",
        "7M 10M 12d 13M 16M",
    ],
    "add9" => &[
        "1P 5P 8P 9M 10M",
        "1P 5P 9M 10M 12P",
        "3M 8P 9M 10M 12P",
        "3M 8P 9M 12P 15P",
        "5P 8P 9M 10M 15P",
        "5P 8P 9M 12P 17M",
    ],
    "aug" => &[
        "1P 6m 8P 9M 10M",
        "1P 6m 8P 10M 13m",
        "3M 8P 9M 10M 13m",
        "3M 8P 10M 13m 15P",
        "6m 10M 13m 15P 16M",
        "6m 10M 13m 15P 17M",
    ],
    "h" => &[
        "1P 5d 7m 10m 11P",
        "3m 5d 7m 8P 11P",
        "5d 7m 8P 10m 11P",
        "1P 7m 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 8P 10m 11P 14m",
        "7m 10m 11P 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "h7" => &[
        "1P 5d 7m 10m 11P",
        "3m 5d 7m 8P 11P",
        "5d 7m 8P 10m 11P",
        "1P 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 8P 10m 11P 14m",
        "7m 10m 11P 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "h9" => &[
        "3m 5d 7m 9M 11P",
        "1P 7m 9M 10m 12d",
        "3m 7m 9M 12d 14m",
        "5d 8P 9M 10m 14m",
        "7m 10m 11P 12d 14m",
        "7m 10m 12d 14m 16M",
    ],
    "m" => &[
        "1P 3m 5P 8P 10m",
        "1P 3m 5P 9M 11P",
        "3m 5P 8P 9M 11P",
        "5P 8P 9M 10m 11P",
        "1P 5P 9M 10m 12P",
        "3m 5P 8P 10m 12P",
        "5P 8P 10m 12P 15P",
    ],
    "m#5" => &["1P 6m 8P 10m 13m", "3m 6m 8P 11P 13m", "6m 8P 10m 13m 15P"],
    "m11" => &[
        "3m 5P 7m 9M 11P",
        "7m 9M 10m 11P",
        "1P 4P 7m 10m 12P",
        "3m 7m 9M 11P 12P",
        "7m 9M 10m 11P 12P",
        "3m 7m 9M 11P 14m",
        "4P 10m 12P 14m",
        "5P 8P 11P 14m",
        "5P 8P 11P 14m 16M",
        "7m 10m 12P 16M 18P",
        "7m 10m 11P 16M 21m",
    ],
    "m6" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "1P 5P 6M 10m 11P",
        "3m 5P 6M 8P 11P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 10m 11P 13M",
        "5P 8P 10m 13M 16M",
    ],
    "m69" => &[
        "1P 3m 5P 6M 9M",
        "3m 5P 6M 8P 9M",
        "3m 6M 9M 10m 12P",
        "1P 5P 9M 10m 13M",
        "3m 5P 8P 9M 13M",
        "5P 8P 9M 10m 13M",
        "5P 8P 10m 13M 16M",
    ],
    "m7" => &[
        "1P 3m 5P 7m 9M",
        "1P 3m 5P 7m 10m",
        "1P 5P 7m 10m 11P",
        "3m 7m 8P 10m 11P",
        "1P 5P 7m 10m 12P",
        "3m 7m 9M 10m 12P",
        "3m 7m 8P 10m 14m",
        "5P 7m 9M 10m 14m",
        "7m 10m 11P 14m 15P",
        "7m 10m 12P 15P 16M",
        "5P 8P 11P 14m 17m",
        "7m 10m 12P 15P 17m",
    ],
    "m7b5" => &[
        "1P 5d 7m 10m 11P",
        "3m 5d 7m 8P 11P",
        "5d 7m 8P 10m 11P",
        "1P 7m 10m 12d",
        "3m 7m 8P 10m 12d",
        "3m 7m 8P 12d 14m",
        "5d 8P 10m 11P 14m",
        "7m 10m 11P 12d 14m",
        "7m 10m 12d 14m 15P",
        "5d 8P 10m 14m 17m",
    ],
    "m9" => &[
        "1P 3m 5P 7m 9M",
        "1P 3m 7m 9M 11P",
        "3m 7m 9M 10m 11P",
        "3m 7m 9M 10m 12P",
        "3m 7m 9M 10m 14m",
        "3m 7m 9M 12P 15P",
        "7m 10m 11P 14m 16M",
        "7m 10m 12P 16M 18P",
    ],
    "m^7" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "m^9" => &[
        "1P 3m 5P 7M 9M",
        "1P 5P 7M 10m 11P",
        "3m 7M 9M 10m 11P",
        "3m 7M 9M 10m 12P",
        "3m 7M 9M 12P 14M",
        "7M 10m 11P 12P 14M",
        "7M 10m 12P 14M 16M",
    ],
    "madd9" => &[
        "1P 2M 3m 5P 8P",
        "1P 3m 5P 9M",
        "3m 5P 8P 9M 12P",
        "5P 8P 9M 10m 12P",
    ],
    "mb6" => &["1P 3m 5P 6m 8P", "3m 5P 8P 11P 13m", "5P 8P 10m 11P 13m"],
    "o" => &[
        "1P 6M 8P 10m 12d",
        "1P 6M 10m 12d 13M",
        "3m 8P 10m 12d 13M",
        "3m 8P 12d 13M 15P",
        "5d 10m 12d 13M 15P",
        "5d 10m 13M 15P 17m",
        "6M 12d 13M 15P 17m",
        "6M 12d 15P 17m 19d",
    ],
    "o7" => &[
        "1P 6M 8P 10m 12d",
        "1P 6M 10m 12d 13M",
        "3m 8P 10m 12d 13M",
        "3m 8P 12d 13M 15P",
        "5d 10m 12d 13M 15P",
        "5d 10m 13M 15P 17m",
        "6M 12d 13M 15P 17m",
        "6M 12d 15P 17m 19d",
    ],
    "sus" => &[
        "1P 4P 5P 8P 9M",
        "1P 4P 5P 8P 11P",
        "1P 5P 8P 9M 11P",
        "5P 8P 9M 11P 12P",
        "5P 8P 11P 12P 13M",
        "5P 8P 11P 13M 15P",
    ],
};

/// Look up a voicing dictionary by name (default: `ireal`).
///
/// Every dictionary uses mode `below` and anchor `c5`. In Strudel's `voicing`,
/// the per-value controls (`anchor`/`mode`, both `undefined` unless explicitly
/// set) are spread *after* the registry entry, so they override the registry's
/// `mode`/`anchor` with `undefined` — which then falls back to `renderVoicing`'s
/// defaults (`mode='below'`, `anchor='c5'`). So the curated dicts' registry
/// `a4`/`above` settings are dead for the `voicing` path; only an explicit
/// `.anchor(...)` / `.mode(...)` control changes them (handled in [`VoicingOpts`]).
fn dictionary(name: &str) -> Dictionary {
    let table = match name {
        "lefthand" => &LEFTHAND,
        "triads" => &TRIADS,
        "guidetones" => &GUIDETONES,
        "legacy" => &LEGACY,
        "ireal-ext" => &IREAL_EXT,
        // ireal (the default) and any unknown name fall back to ireal/simple.
        _ => &IREAL,
    };
    Dictionary {
        table,
        mode: Mode::Below,
        anchor: "c5",
    }
}

/// Pitch class (letter + accidentals) to a 0..11 chroma.
fn pc_to_chroma(pc: &str) -> Option<i32> {
    let mut chars = pc.chars();
    let mut chroma = letter_semitone(chars.next()?)?;
    for c in chars {
        match c {
            '#' | 's' => chroma += 1,
            'b' | 'f' => chroma -= 1,
            _ => return None,
        }
    }
    Some(chroma.rem_euclid(12))
}

/// Split a chord symbol like `"C^7"`, `"Am7"`, `"G7/B"` into `(root, symbol)`,
/// dropping any slash-bass.
fn tokenize_chord(chord: &str) -> Option<(String, String)> {
    let chord = chord.split('/').next().unwrap_or(chord);
    let mut chars = chord.chars().peekable();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || !"abcdefg".contains(first.to_ascii_lowercase()) {
        return None;
    }
    let mut root = String::new();
    root.push(first);
    while let Some(&c) = chars.peek() {
        if c == '#' || c == 'b' {
            root.push(c);
            chars.next();
        } else {
            break;
        }
    }
    Some((root, chars.collect()))
}

/// Normalise alternate chord-symbol spellings to the dictionary's canonical
/// keys. The canonical keys use `^` (major 7th) which mini-notation can't spell,
/// so accept the `maj`/`min`/`dim`/`+` spellings too.
fn normalize_symbol(s: &str) -> &str {
    match s {
        "maj7" | "M7" => "^7",
        "maj9" | "M9" => "^9",
        "min7" | "-7" => "m7",
        "min9" | "-9" => "m9",
        "minor" | "min" | "-" => "m",
        "major" | "maj" => "",
        "dim" => "o",
        "dim7" => "o7",
        "min7b5" | "m7-5" | "hdim" => "m7b5",
        "+" => "aug",
        "minMaj7" | "mmaj7" => "mM7",
        other => other,
    }
}

/// `scaleStep`: index into `notes` like a scale, octaving overshoots.
fn scale_step_in(notes: &[i32], offset: i32, octaves: i32) -> i32 {
    let len = notes.len() as i32;
    let oct_offset = offset.div_euclid(len) * octaves * 12;
    notes[offset.rem_euclid(len) as usize] + oct_offset
}

/// Options for [`render_voicing`].
struct VoicingOpts {
    dict: String,
    offset: i32,
    n: Option<i32>,
    mode: Option<Mode>,
    anchor: Option<i32>,
    octaves: i32,
}

impl Default for VoicingOpts {
    fn default() -> Self {
        VoicingOpts {
            dict: "ireal".to_string(),
            offset: 0,
            n: None,
            mode: None,
            anchor: None,
            octaves: 1,
        }
    }
}

/// Render a chord symbol into a list of MIDI notes (port of `renderVoicing`).
fn render_voicing(chord: &str, opts: &VoicingOpts) -> Option<Vec<i32>> {
    let dict = dictionary(&opts.dict);
    let mode = opts.mode.unwrap_or(dict.mode);
    let anchor = opts
        .anchor
        .or_else(|| note_to_midi_with_octave(dict.anchor, 4))?;

    let (root, symbol) = tokenize_chord(chord)?;
    let root_chroma = pc_to_chroma(&root)?;
    let anchor_chroma = anchor.rem_euclid(12);

    let normalized = normalize_symbol(&symbol);
    let voicing_defs = dict
        .table
        .get(symbol.as_str())
        .or_else(|| dict.table.get(normalized))
        .copied()?;
    let voicings: Vec<Vec<i32>> = voicing_defs
        .iter()
        .map(|v| {
            v.split_whitespace()
                .filter_map(interval_to_semitones)
                .collect()
        })
        .collect();
    if voicings.iter().any(|v| v.is_empty()) {
        return None;
    }

    // Pick the voicing whose top/bottom note sits closest below the anchor.
    let mut min_distance: Option<i32> = None;
    let mut best_index = 0;
    let chroma_diffs: Vec<i32> = voicings
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let diff = (anchor_chroma - mode.target(v) - root_chroma).rem_euclid(12);
            if min_distance.is_none_or(|m| diff < m) {
                min_distance = Some(diff);
                best_index = i;
            }
            diff
        })
        .collect();
    if mode == Mode::Root {
        best_index = 0;
    }

    let len = voicings.len() as i32;
    let oct_diff = (opts.offset as f64 / len as f64).ceil() as i32 * 12;
    let index = (best_index as i32 + opts.offset).rem_euclid(len) as usize;
    let voicing = &voicings[index];
    let target_step = mode.target(voicing);
    let anchor_midi = anchor - chroma_diffs[index] + oct_diff;
    let voicing_midi: Vec<i32> = voicing
        .iter()
        .map(|v| anchor_midi - target_step + v)
        .collect();

    let notes: Vec<i32> = if mode == Mode::Duck {
        voicing_midi.into_iter().filter(|&m| m != anchor).collect()
    } else {
        voicing_midi
    };

    match opts.n {
        Some(n) => Some(vec![scale_step_in(&notes, n, opts.octaves)]),
        None => Some(notes),
    }
}

/// Extract a voicing's controls from a hap value (chord string, or a map with a
/// `chord` key plus optional `dict`/`anchor`/`mode`/`offset`/`octaves`/`n`).
/// Returns `(chord, opts, extra_controls)`.
fn opts_from_value(value: &Value) -> Option<(String, VoicingOpts, ValueMap)> {
    match value {
        Value::Map(m) => {
            let chord = chord_symbol(m.get("chord")?)?;
            let mut opts = VoicingOpts::default();
            if let Some(d) = m
                .get("dictionary")
                .or_else(|| m.get("dict"))
                .and_then(|v| v.as_str())
            {
                opts.dict = d.to_string();
            }
            if let Some(a) = m.get("anchor") {
                opts.anchor = match a {
                    Value::Str(s) => note_to_midi_with_octave(s, 4),
                    other => other.as_f64().map(|f| f.round() as i32),
                };
            }
            if let Some(mode) = m.get("mode").and_then(|v| v.as_str()) {
                opts.mode = Some(Mode::from_str(mode));
            }
            if let Some(o) = m.get("offset").and_then(|v| v.as_f64()) {
                opts.offset = o.round() as i32;
            }
            if let Some(o) = m.get("octaves").and_then(|v| v.as_f64()) {
                opts.octaves = o.round() as i32;
            }
            if let Some(n) = m.get("n").and_then(|v| v.as_f64()) {
                opts.n = Some(n.round() as i32);
            }
            // Everything except the voicing controls is merged onto the output.
            let mut extra = m.clone();
            for k in [
                "chord",
                "dictionary",
                "dict",
                "anchor",
                "mode",
                "offset",
                "octaves",
                "n",
            ] {
                extra.shift_remove(k);
            }
            Some((chord, opts, extra))
        }
        other => Some((
            chord_symbol(other)?,
            VoicingOpts::default(),
            ValueMap::new(),
        )),
    }
}

/// Build a stacked note pattern for one chord, merging any extra controls.
fn voicing_pattern(chord: &str, opts: &VoicingOpts, extra: &ValueMap) -> Pattern {
    match render_voicing(chord, opts) {
        Some(notes) => {
            let pats: Vec<Pattern> = notes
                .into_iter()
                .map(|midi| {
                    if extra.is_empty() {
                        pure(Value::F64(midi as f64))
                    } else {
                        let mut map = extra.clone();
                        map.insert("note".to_string(), Value::F64(midi as f64));
                        pure(Value::Map(map))
                    }
                })
                .collect();
            stack(&pats)
        }
        None => silence(),
    }
}

impl Pattern {
    /// Turn chord symbols into voicings (`voicing`). Values may be chord strings
    /// (e.g. `"C^7"`) or maps with a `chord` key plus optional
    /// `dict`/`anchor`/`mode`/`offset`/`octaves`/`n` controls. Uses the `ireal`
    /// dictionary by default (matching Strudel's `defaultDict`).
    pub fn voicing(&self) -> Pattern {
        self.outer_bind(|value| match opts_from_value(&value) {
            Some((chord, opts, extra)) => voicing_pattern(&chord, &opts, &extra),
            None => silence(),
        })
    }

    /// Like [`voicing`](Self::voicing) but with an explicit dictionary name
    /// (`ireal`, `ireal-ext`, `lefthand`, `triads`, `guidetones`, or `legacy`).
    pub fn voicings(&self, dict: impl Into<String>) -> Pattern {
        let dict = dict.into();
        self.outer_bind(move |value| match opts_from_value(&value) {
            Some((chord, mut opts, extra)) => {
                opts.dict = dict.clone();
                voicing_pattern(&chord, &opts, &extra)
            }
            None => silence(),
        })
    }

    /// Map chord symbols to their root note in the given octave (`rootNotes`).
    pub fn root_notes(&self, octave: i64) -> Pattern {
        let octave = octave as i32;
        self.with_value(move |value| {
            let chord = match &value {
                Value::Map(m) => m.get("chord").and_then(chord_symbol),
                other => chord_symbol(other),
            };
            let Some(chord) = chord else { return value };
            let Some((root, _)) = tokenize_chord(&chord) else {
                return value;
            };
            let Some(midi) = note_to_midi_with_octave(&format!("{root}{octave}"), octave) else {
                return value;
            };
            match value {
                Value::Map(mut m) => {
                    m.insert("note".to_string(), Value::F64(midi as f64));
                    Value::Map(m)
                }
                _ => Value::F64(midi as f64),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frac;

    fn notes(pat: &Pattern) -> Vec<i32> {
        let mut v: Vec<i32> = pat
            .query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .map(|h| h.value.as_f64().unwrap() as i32)
            .collect();
        v.sort();
        v
    }

    #[test]
    fn lefthand_cmaj7() {
        // C^7 lefthand, anchor a4 -> rootless voicing B3 D4 E4 G4
        let opts = VoicingOpts {
            dict: "lefthand".to_string(),
            ..Default::default()
        };
        assert_eq!(render_voicing("C^7", &opts), Some(vec![59, 62, 64, 67]));
    }

    #[test]
    fn triad_c_major() {
        let opts = VoicingOpts {
            dict: "triads".to_string(),
            ..Default::default()
        };
        // C major triad below the default c5 anchor -> E4 G4 C5.
        assert_eq!(render_voicing("C", &opts), Some(vec![64, 67, 72]));
    }

    #[test]
    fn voicing_pattern_stacks_notes() {
        // default dictionary is now `ireal`: C -> E3 C4 E4 G4 C5.
        let pat = pure(Value::Str("C".into())).voicing();
        assert_eq!(notes(&pat), vec![52, 60, 64, 67, 72]);
    }

    #[test]
    fn voicings_named_dictionary() {
        let pat = pure(Value::Str("C^7".into())).voicings("lefthand");
        assert_eq!(notes(&pat), vec![59, 62, 64, 67]);
    }

    #[test]
    fn voicing_reads_list_backed_chord_symbol() {
        // mini spells `c:maj7` as ["c", "maj7"]; voicing joins it to "Cmaj7".
        let pat = pure(Value::List(vec![
            Value::Str("C".into()),
            Value::Str("maj7".into()),
        ]))
        .voicings("lefthand");
        let from_symbol = pure(Value::Str("C^7".into())).voicings("lefthand");
        assert_eq!(notes(&pat), notes(&from_symbol));
    }

    #[test]
    fn voicing_reads_dictionary_control_key() {
        // a map carrying chord + the `dictionary` control key (from `dict()`).
        let mut m = ValueMap::new();
        m.insert("chord".to_string(), Value::Str("C^7".into()));
        m.insert("dictionary".to_string(), Value::Str("lefthand".into()));
        let pat = pure(Value::Map(m)).voicing();
        assert_eq!(notes(&pat), vec![59, 62, 64, 67]);
    }

    #[test]
    fn root_notes_reads_list_backed_chord() {
        let pat = pure(Value::List(vec![
            Value::Str("A".into()),
            Value::Str("m7".into()),
        ]))
        .root_notes(3);
        assert_eq!(notes(&pat), vec![57]); // A3
    }

    #[test]
    fn root_notes_maps_to_octave() {
        let pat = pure(Value::Str("C^7".into())).root_notes(2);
        assert_eq!(notes(&pat), vec![36]); // C2
        let pat = pure(Value::Str("Am7".into())).root_notes(3);
        assert_eq!(notes(&pat), vec![57]); // A3
    }

    #[test]
    fn voicing_with_n_plays_like_scale() {
        // n selects a single note from the voicing, octaving overshoots.
        // triads C below the c5 anchor is [E4 G4 C5] = [64, 67, 72].
        let opts = VoicingOpts {
            dict: "triads".to_string(),
            n: Some(0),
            ..Default::default()
        };
        assert_eq!(render_voicing("C", &opts), Some(vec![64]));
        let opts = VoicingOpts {
            dict: "triads".to_string(),
            n: Some(3), // wraps to the next octave of note 0
            ..Default::default()
        };
        assert_eq!(render_voicing("C", &opts), Some(vec![76]));
    }

    #[test]
    fn unknown_chord_is_silent() {
        let pat = pure(Value::Str("Zwurble".into())).voicing();
        assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
    }
}
