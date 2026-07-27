// input.rs - realtime input buses feeding query-time signals.
// External MIDI control-change messages are written into a global bus by the
// MIDI back-end (`rudel-midi`); patterns read the latest value at query time via
// the `cc_in` signal. This is the input counterpart to the output controls and
// mirrors Strudel's `MidiInput` CC refs (packages/midi/input.mjs), which expose
// the latest CC value as a `ref()` signal.
//
// The same shape serves the pointer and keyboard: the app writes the latest
// state each frame, and `mousex`/`mousey`/`key_down` read it at query time.
// Strudel gets these from `document` event listeners (core/signal.mjs,
// core/util.mjs `getCurrentKeyboardState`); Rudel's egui app is the event
// source instead.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{pattern::Pattern, signal::signal, value::Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, RwLock},
};

/// How the CC bus is keyed: device name, MIDI channel, controller number.
type CcKey = (String, u8, u8);

/// Global MIDI-input CC bus: the latest value (0..1) keyed by
/// `(device, channel, cc)`. An empty device name means "any device" and channel
/// `0` means "any channel" — the most recent value seen anywhere — so a reader
/// that pins neither still tracks the one selected input. Long-lived for the
/// process, like Strudel's singleton `midiInputs`.
static CC_BUS: LazyLock<RwLock<HashMap<CcKey, f64>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Record an incoming MIDI CC from `device` (value already scaled to 0..1).
/// Writes the device/channel-specific entry plus the wildcard entries, so
/// readers that don't pin a device or channel see the latest value from any.
/// Called by the MIDI input thread.
pub fn set_cc_from(device: &str, channel: u8, cc: u8, value: f64) {
    let mut bus = CC_BUS.write().unwrap();
    for key in [
        (device.to_string(), channel, cc),
        (device.to_string(), 0, cc),
        (String::new(), channel, cc),
        (String::new(), 0, cc),
    ] {
        bus.insert(key, value);
    }
}

/// Record an incoming MIDI CC without attributing it to a named device.
pub fn set_cc(channel: u8, cc: u8, value: f64) {
    set_cc_from("", channel, cc, value);
}

/// Read the latest value of CC `cc` on `channel` (0 = any) from `device`
/// (`""` = any), defaulting to `0.0`.
pub fn get_cc_from(device: &str, channel: u8, cc: u8) -> f64 {
    CC_BUS
        .read()
        .unwrap()
        .get(&(device.to_string(), channel, cc))
        .copied()
        .unwrap_or(0.0)
}

/// Read the latest value of CC `cc` on `channel` (0 = any) from any device.
pub fn get_cc(channel: u8, cc: u8) -> f64 {
    get_cc_from("", channel, cc)
}

/// Clear all recorded CC state (device reset / tests).
pub fn clear_cc() {
    CC_BUS.write().unwrap().clear();
}

/// A continuous 0..1 signal of the latest value of MIDI CC `cc`. `channel` is
/// `1..=16`, or `None` for any channel (`ccin` in Koto). Reads the live bus at
/// query time, so the value tracks incoming controllers in real time.
pub fn cc_in(cc: u8, channel: Option<u8>) -> Pattern {
    cc_in_from("", cc, channel)
}

/// Like [`cc_in`] but reading only CCs that arrived from the named device —
/// the signal `midin(device)`'s factory returns. `device` is matched exactly
/// against the name the input connection was opened under; `""` means any.
pub fn cc_in_from(device: &str, cc: u8, channel: Option<u8>) -> Pattern {
    let chan = channel.unwrap_or(0);
    let device = device.to_string();
    signal(move |_t| Value::F64(get_cc_from(&device, chan, cc)))
}

// ---------------------------------------------------------------------------
// MIDI keyboard

/// How many unplayed note-ons are kept per device. Upstream's `kHaps` grows
/// without bound until the scheduler drains it; bounding it means a keyboard
/// hammered while the transport is stopped can't grow forever.
const NOTE_QUEUE_CAPACITY: usize = 64;

/// A buffered note-on: MIDI note number and velocity scaled to 0..1.
type QueuedNote = (i64, f64);

/// Note-ons received since the last query, keyed by the device they came in on
/// (plus a `""` entry for "any device"). Mirrors Strudel's `kHaps`, which
/// buffers incoming notes until the pattern that reads them is next queried.
static NOTE_QUEUE: LazyLock<RwLock<HashMap<String, Vec<QueuedNote>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Buffer an incoming note-on from `device`: MIDI note number and velocity
/// scaled to 0..1. Note-offs are not queued — like upstream, a `midikeys` hap's
/// length comes from the pattern, not from when the key is released.
pub fn push_midi_note(device: &str, note: i64, velocity: f64) {
    let mut queue = NOTE_QUEUE.write().unwrap();
    for key in [device.to_string(), String::new()] {
        let notes = queue.entry(key).or_default();
        if notes.len() == NOTE_QUEUE_CAPACITY {
            notes.remove(0);
        }
        notes.push((note, velocity));
    }
}

/// Take every buffered note from `device` (`""` = any), emptying its queue.
pub fn take_midi_notes(device: &str) -> Vec<QueuedNote> {
    let mut queue = NOTE_QUEUE.write().unwrap();
    let taken = queue
        .get_mut(device)
        .map(std::mem::take)
        .unwrap_or_default();
    // A note is delivered once, so draining one view must drop it from the
    // others (the device-specific queue and the "any device" queue hold the
    // same notes).
    if !taken.is_empty() {
        for (key, notes) in queue.iter_mut() {
            if key != device {
                notes.retain(|n| !taken.contains(n));
            }
        }
    }
    taken
}

/// Forget every buffered note (transport stop / tests).
pub fn clear_midi_notes() {
    NOTE_QUEUE.write().unwrap().clear();
}

/// The pattern `midikeys(device)`'s factory returns: every note received since
/// the last scheduler query, each sounding for `note_length` cycles from the
/// moment it is picked up.
///
/// Ports upstream's `kb(noteLength)`. Two differences, both forced by Rudel
/// having no wall-clock-to-cycle map outside the scheduler: a note is placed at
/// the start of the query window that picks it up (upstream stamps it with the
/// cyclist time at which the message arrived, so it lands within a scheduler
/// block either way), and there is no immediate out-of-band trigger — the note
/// sounds on the next scheduler block rather than being dispatched straight to
/// the audio engine. Like upstream, the queue is only drained on a scheduler
/// (`cyclist`) query, so a visualiser querying the same pattern doesn't eat the
/// notes before they are played.
pub fn midi_keys(device: &str, note_length: Pattern) -> Pattern {
    let device = device.to_string();
    Pattern::new(move |state| {
        let scheduler_query = state.controls.contains_key("cyclist");
        let notes = if scheduler_query {
            take_midi_notes(&device)
        } else {
            Vec::new()
        };
        let length = note_length
            .query(&state.set_span(crate::timespan::TimeSpan::new(
                state.span.begin,
                state.span.begin,
            )))
            .first()
            .and_then(|h| h.value.as_f64())
            .unwrap_or(0.5);
        let span = crate::timespan::TimeSpan::new(
            state.span.begin,
            state.span.begin + crate::fraction::Frac::from_f64(length),
        );
        notes
            .into_iter()
            .map(|(note, velocity)| {
                let value = Value::Map(crate::value::ValueMap::from([
                    ("note".to_string(), Value::Int(note)),
                    ("velocity".to_string(), Value::F64(velocity)),
                ]));
                crate::hap::Hap::new(
                    Some(span),
                    span.intersection(&state.span).unwrap_or(span),
                    value,
                )
            })
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Pointer

/// Latest pointer position, each axis normalised to 0..1 across the window.
/// Strudel divides `clientX`/`clientY` by the body size; the app does the same
/// against its own window.
static POINTER: LazyLock<RwLock<(f64, f64)>> = LazyLock::new(|| RwLock::new((0.0, 0.0)));

/// Record the pointer position (already normalised to 0..1). Called by the app
/// once per frame.
pub fn set_pointer(x: f64, y: f64) {
    *POINTER.write().unwrap() = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
}

/// The latest pointer position as `(x, y)` in 0..1.
pub fn get_pointer() -> (f64, f64) {
    *POINTER.read().unwrap()
}

/// A continuous 0..1 signal of the pointer's x position (`mousex`).
pub fn mousex() -> Pattern {
    signal(|_t| Value::F64(get_pointer().0))
}

/// A continuous 0..1 signal of the pointer's y position (`mousey`).
pub fn mousey() -> Pattern {
    signal(|_t| Value::F64(get_pointer().1))
}

// ---------------------------------------------------------------------------
// Keyboard

/// The names of the keys currently held. Keys are named as in Strudel — that
/// is, the browser's [`KeyboardEvent.key`] values (`"a"`, `"Control"`,
/// `"ArrowUp"`, …) — so patterns written for either engine name them alike.
///
/// [`KeyboardEvent.key`]: https://developer.mozilla.org/docs/Web/API/UI_Events/Keyboard_event_key_values
static KEYS: LazyLock<RwLock<HashSet<String>>> = LazyLock::new(|| RwLock::new(HashSet::new()));

/// Replace the set of held keys. The app polls its window each frame, so it
/// reports the whole set rather than individual up/down edges.
pub fn set_keys_held<S: Into<String>>(names: impl IntoIterator<Item = S>) {
    *KEYS.write().unwrap() = names.into_iter().map(Into::into).collect();
}

/// Forget every held key (window focus loss / tests).
pub fn clear_keys() {
    KEYS.write().unwrap().clear();
}

/// Resolve Strudel's shorthand key names (`util.mjs`'s `keyAlias`). Anything
/// else passes through as written.
fn key_alias(name: &str) -> &str {
    match name {
        "control" | "ctrl" => "Control",
        "alt" => "Alt",
        "shift" => "Shift",
        "down" => "ArrowDown",
        "up" => "ArrowUp",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        other => other,
    }
}

/// True when **every** named key is held, matching `_keyDown`'s `every`. A
/// combination is written as a `:`-list (`"Control:j"`), which reaches here as
/// several names.
pub fn keys_down<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    let keys = KEYS.read().unwrap();
    let mut any = false;
    for name in names {
        any = true;
        if !keys.contains(key_alias(name)) {
            return false;
        }
    }
    // `[].every(...)` is true in JS, but an empty key list is a user error
    // rather than "always on", so it reads as not-held.
    any
}

/// A boolean signal that is true while every key in `names` is held
/// (`keyDown`). Reads the live state at query time, so a pattern responds to
/// the keyboard without re-evaluating.
pub fn key_down(names: Vec<String>) -> Pattern {
    signal(move |_t| Value::Bool(keys_down(names.iter().map(String::as_str))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fraction::Frac;

    fn sample(pat: &Pattern) -> f64 {
        pat.query_arc(Frac::zero(), Frac::one())[0]
            .value
            .as_f64()
            .unwrap()
    }

    // The bus is process-global, so these tests use disjoint CC numbers rather
    // than `clear_cc` (which would race other tests in the same binary).

    #[test]
    fn cc_in_reads_the_latest_value() {
        // unseen CC defaults to 0
        let sig = cc_in(74, None);
        assert_eq!(sample(&sig), 0.0);
        // a write is visible to the signal at query time
        set_cc(1, 74, 0.5);
        assert_eq!(sample(&sig), 0.5);
        set_cc(1, 74, 0.9);
        assert_eq!(sample(&sig), 0.9);
    }

    #[test]
    fn cc_in_respects_channel() {
        set_cc(1, 20, 0.25);
        set_cc(2, 20, 0.75);
        // channel-pinned readers see their own channel
        assert_eq!(sample(&cc_in(20, Some(1))), 0.25);
        assert_eq!(sample(&cc_in(20, Some(2))), 0.75);
        // the any-channel reader sees the most recent write (channel 2)
        assert_eq!(sample(&cc_in(20, None)), 0.75);
    }

    #[test]
    fn pointer_signals_track_the_latest_position() {
        set_pointer(0.25, 0.75);
        assert_eq!(sample(&mousex()), 0.25);
        assert_eq!(sample(&mousey()), 0.75);
        // Out-of-window positions clamp rather than escaping 0..1.
        set_pointer(-1.0, 2.0);
        assert_eq!(sample(&mousex()), 0.0);
        assert_eq!(sample(&mousey()), 1.0);
    }

    #[test]
    fn key_down_needs_every_key_of_a_combination() {
        clear_keys();
        // Strudel's aliases: "ctrl" and "control" both mean "Control".
        let combo = key_down(vec!["ctrl".into(), "j".into()]);
        let truthy = |p: &Pattern| p.query_arc(Frac::zero(), Frac::one())[0].value.truthy();

        assert!(!truthy(&combo), "nothing held");
        set_keys_held(["Control"]);
        assert!(!truthy(&combo), "only one of the pair held");
        set_keys_held(["Control", "j"]);
        assert!(truthy(&combo), "both held");
        set_keys_held(["j"]);
        assert!(!truthy(&combo), "released again");
        clear_keys();
    }

    #[test]
    fn cc_in_is_continuous_and_segmentable() {
        set_cc(0, 30, 1.0);
        // sampling at 8 points across a cycle all read the same live value
        let seg = cc_in(30, None).segment(Frac::int(8));
        let haps = seg.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 8);
        assert!(haps.iter().all(|h| h.value.as_f64() == Some(1.0)));
    }
}
