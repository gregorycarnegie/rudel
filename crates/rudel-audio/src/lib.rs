//! rudel-audio - real-time clock, lookahead scheduler and cpal output.
//! The scheduler maps cycle time to the audio sample clock and feeds timed
//! note events to a mixer running in the audio callback.
//! Clock approach mirrors strudel/packages/core/{zyklus,cyclist}.mjs.
//! SPDX-License-Identifier: AGPL-3.0-or-later

#![warn(missing_docs)]

/// Cycle/seconds clock with cyclist-style cps re-anchoring.
pub mod clock;
/// Note event creation and scheduling logic.
pub mod events;
mod midimap_source;
mod mixer;
mod sample_map;
/// In-memory audio sample bank and decoding utilities.
pub mod samples;
mod scope;
/// SoundFont 2 (`.sf2`) file reading.
pub mod sf2;
/// General MIDI soundfont playback (WebAudioFont presets).
pub mod soundfont;
mod sync;

pub use clock::Clock;
pub use events::{NoteEvent, collect_events, collect_events_at, to_control_map};
pub use midimap_source::{load_midimaps, spawn_midimaps};
pub use mixer::{Engine, OfflineMixer};
pub use samples::{DEFAULT_SAMPLE_BANKS, SampleBank, take_sample_requests};
pub use scope::{ScopeTap, ScopeTaps};
pub use soundfont::{gm_names, set_soundfont_url, take_font_requests};
