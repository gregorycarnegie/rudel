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

/// Global MIDI-input CC bus: the latest value (0..1) keyed by `(channel, cc)`,
/// where channel `0` means "any channel" (the most recent value seen on any
/// channel). Long-lived for the process, like Strudel's singleton inputs.
static CC_BUS: LazyLock<RwLock<HashMap<(u8, u8), f64>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Record an incoming MIDI CC (value already scaled to 0..1). Writes both the
/// channel-specific entry and the channel-agnostic (`0`) entry, so `cc_in`
/// readers that don't pin a channel see the latest value on any channel. Called
/// by the MIDI input thread.
pub fn set_cc(channel: u8, cc: u8, value: f64) {
    let mut bus = CC_BUS.write().unwrap();
    bus.insert((channel, cc), value);
    bus.insert((0, cc), value);
}

/// Read the latest value of CC `cc` on `channel` (0 = any), defaulting to `0.0`.
pub fn get_cc(channel: u8, cc: u8) -> f64 {
    CC_BUS
        .read()
        .unwrap()
        .get(&(channel, cc))
        .copied()
        .unwrap_or(0.0)
}

/// Clear all recorded CC state (device reset / tests).
pub fn clear_cc() {
    CC_BUS.write().unwrap().clear();
}

/// A continuous 0..1 signal of the latest value of MIDI CC `cc`. `channel` is
/// `1..=16`, or `None` for any channel (`ccin` in Koto). Reads the live bus at
/// query time, so the value tracks incoming controllers in real time.
pub fn cc_in(cc: u8, channel: Option<u8>) -> Pattern {
    let chan = channel.unwrap_or(0);
    signal(move |_t| Value::F64(get_cc(chan, cc)))
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
