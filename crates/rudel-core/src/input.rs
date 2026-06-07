// input.rs - realtime MIDI-input bus feeding query-time signals.
// External MIDI control-change messages are written into a global bus by the
// MIDI back-end (`rudel-midi`); patterns read the latest value at query time via
// the `cc_in` signal. This is the input counterpart to the output controls and
// mirrors Strudel's `MidiInput` CC refs (packages/midi/input.mjs), which expose
// the latest CC value as a `ref()` signal.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::pattern::Pattern;
use crate::signal::signal;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

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
    fn cc_in_is_continuous_and_segmentable() {
        set_cc(0, 30, 1.0);
        // sampling at 8 points across a cycle all read the same live value
        let seg = cc_in(30, None).segment(Frac::int(8));
        let haps = seg.query_arc(Frac::zero(), Frac::one());
        assert_eq!(haps.len(), 8);
        assert!(haps.iter().all(|h| h.value.as_f64() == Some(1.0)));
    }
}
