// rudel-midi - MIDI output for Rudel.
// Maps a pattern's control events to MIDI note-on/off, control-change and clock
// messages, and drives them in real time over a `midir` connection.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod input;
mod note;
mod output;
mod schedule;

pub use input::{ClockDetector, InputAction, MidiIn, bpm_to_cps, process_input};
pub use note::{MidiNote, control_to_midi, reset_messages};
pub use output::{MidiEngine, MidiOut, MidiSink};
pub use schedule::{TimedMidi, schedule_window};

// MIDI status bytes (channel goes in the low nibble).
pub(crate) const NOTE_ON: u8 = 0x90;
pub(crate) const NOTE_OFF: u8 = 0x80;
pub(crate) const CONTROL_CHANGE: u8 = 0xB0;
pub(crate) const PROGRAM_CHANGE: u8 = 0xC0;
pub(crate) const PITCH_BEND: u8 = 0xE0;
pub(crate) const CLOCK: u8 = 0xF8;
pub(crate) const START: u8 = 0xFA;
pub(crate) const CONTINUE: u8 = 0xFB;
pub(crate) const STOP: u8 = 0xFC;
pub(crate) const MPE_MASTER_CHANNEL: u8 = 0;
pub(crate) const MPE_FIRST_MEMBER: u8 = 1;
pub(crate) const MPE_LAST_MEMBER: u8 = 15;
pub(crate) const DEFAULT_BEND_RANGE: f64 = 2.0;

#[cfg(test)]
mod tests;
