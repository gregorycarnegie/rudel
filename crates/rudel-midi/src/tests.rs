use super::*;
use crate::note::{aux_messages, bend_value, clamp7, pitch_bend_bytes};
use rudel_core::ValueMap;
use rudel_core::{Frac, Pattern, Value, note, pure, sequence, silence};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn map(pairs: &[(&str, Value)]) -> ValueMap {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn maps_note_velocity_channel() {
    let n = control_to_midi(&map(&[
        ("note", Value::Int(60)),
        ("gain", Value::F64(1.0)),
        ("midichan", Value::Int(2)),
    ]))
    .unwrap();
    assert_eq!(n.note, 60);
    assert_eq!(n.velocity, 127);
    assert_eq!(n.channel, 1); // 1-based -> 0-based
    assert_eq!(n.note_on_bytes(), [0x91, 60, 127]);
    assert_eq!(n.note_off_bytes(), [0x81, 60, 0]);
}

#[test]
fn note_name_resolves_to_midi() {
    let n = control_to_midi(&map(&[("note", Value::Str("a4".into()))])).unwrap();
    assert_eq!(n.note, 69);
    // default velocity 0.9 -> 114
    assert_eq!(n.velocity, clamp7(0.9 * 127.0));
}

#[test]
fn cc_and_default_channel() {
    let n = control_to_midi(&map(&[
        ("note", Value::Int(64)),
        ("ccn", Value::Int(74)),
        ("ccv", Value::F64(0.5)),
    ]))
    .unwrap();
    assert_eq!(n.channel, 0);
    assert_eq!(n.ccs, vec![(74, clamp7(0.5 * 127.0))]);
}

#[test]
fn no_pitch_yields_none() {
    assert!(control_to_midi(&map(&[("s", Value::Str("bd".into()))])).is_none());
}

#[test]
fn schedule_emits_on_then_off() {
    // note(60) over one cycle at cps=1 -> on at 0, off near 1
    let pat = note(pure(Value::Int(60)));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].at_seconds, 0.0);
    assert_eq!(msgs[0].data, vec![0x90, 60, clamp7(0.9 * 127.0)]);
    assert_eq!(msgs[1].data, vec![0x80, 60, 0]);
    assert!(msgs[1].at_seconds > 0.9 && msgs[1].at_seconds <= 1.0);
}

#[test]
fn schedule_orders_two_notes() {
    // "60 67" at cps=1 -> on@0, off@~0.5, on@0.5, off@~1
    let pat = note(sequence(&[pure(Value::Int(60)), pure(Value::Int(67))]));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    assert_eq!(msgs.len(), 4);
    // sorted by time and first message is the first note-on
    assert_eq!(msgs[0].data[0] & 0xF0, NOTE_ON);
    assert!(msgs.windows(2).all(|w| w[0].at_seconds <= w[1].at_seconds));
}

#[test]
fn freq_uses_mpe_with_centered_bend() {
    let pat = rudel_core::freq(pure(Value::F64(440.0)));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let data: Vec<Vec<u8>> = msgs.into_iter().map(|m| m.data).collect();
    assert!(data.contains(&vec![0xB0, 101, 0])); // MPE setup starts on master
    assert!(data.contains(&vec![0xB1, 6, 2])); // default member bend range
    assert!(data.contains(&vec![0xE1, 0, 64])); // centered bend on member ch 2
    assert!(data.contains(&vec![0x91, 69, clamp7(0.9 * 127.0)]));
    assert!(data.contains(&vec![0x81, 69, 0]));
}

#[test]
fn fractional_pitch_emits_bend_before_note_on() {
    let pat = note(pure(Value::F64(60.25)));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let data: Vec<Vec<u8>> = msgs.into_iter().map(|m| m.data).collect();
    let bend = pitch_bend_bytes(1, bend_value(60.25, 60, DEFAULT_BEND_RANGE)).to_vec();
    let bend_idx = data.iter().position(|m| *m == bend).unwrap();
    let note_idx = data
        .iter()
        .position(|m| *m == vec![0x91, 60, clamp7(0.9 * 127.0)])
        .unwrap();
    assert!(bend_idx < note_idx);
    assert!(data.contains(&vec![0x81, 60, 0]));
}

#[test]
fn channel_aftertouch_scales_to_7bit() {
    // miditouch 0.5 -> round(0.5 * 127) = 64 on channel 0 (status 0xD0).
    let msgs = aux_messages(&map(&[("miditouch", Value::F64(0.5))]));
    assert_eq!(msgs, vec![vec![0xD0, 64]]);
    // on channel 3 (1-based -> nibble 2)
    let msgs = aux_messages(&map(&[
        ("miditouch", Value::F64(1.0)),
        ("midichan", Value::Int(3)),
    ]));
    assert_eq!(msgs, vec![vec![0xD2, 127]]);
}

#[test]
fn raw_pitch_bend_centers_at_zero() {
    // midibend in -1..1 -> 14-bit, matching WebMidi.js round((v+1)/2*16383).
    // 0.0 -> 8192 -> lsb 0, msb 64
    assert_eq!(
        aux_messages(&map(&[("midibend", Value::F64(0.0))])),
        vec![vec![0xE0, 0, 64]]
    );
    // 1.0 -> 16383 -> lsb 127, msb 127; -1.0 -> 0
    assert_eq!(
        aux_messages(&map(&[("midibend", Value::F64(1.0))])),
        vec![vec![0xE0, 127, 127]]
    );
    assert_eq!(
        aux_messages(&map(&[("midibend", Value::F64(-1.0))])),
        vec![vec![0xE0, 0, 0]]
    );
}

#[test]
fn sysex_frames_id_and_data() {
    // F0, <id bytes>, <data bytes>, F7. id is a single number, data a list.
    let msgs = aux_messages(&map(&[
        ("sysexid", Value::Int(0x7E)),
        (
            "sysexdata",
            Value::List(vec![Value::Int(0x7F), Value::Int(0x00), Value::Int(0x01)]),
        ),
    ]));
    assert_eq!(msgs, vec![vec![0xF0, 0x7E, 0x7F, 0x00, 0x01, 0xF7]]);
    // a 3-byte manufacturer id (array) frames just the same.
    let msgs = aux_messages(&map(&[
        (
            "sysexid",
            Value::List(vec![Value::Int(0x00), Value::Int(0x21), Value::Int(0x09)]),
        ),
        ("sysexdata", Value::List(vec![Value::Int(0x40)])),
    ]));
    assert_eq!(msgs, vec![vec![0xF0, 0x00, 0x21, 0x09, 0x40, 0xF7]]);
}

#[test]
fn nrpn_emits_canonical_cc_sequence() {
    // nrpnn=1000 -> param MSB 7, LSB 104; nrpv=500 -> data MSB 3, LSB 116;
    // then the null-select (101/100 = 127). All on channel 0.
    let msgs = aux_messages(&map(&[
        ("nrpnn", Value::Int(1000)),
        ("nrpv", Value::Int(500)),
    ]));
    assert_eq!(
        msgs,
        vec![
            vec![0xB0, 99, 7],
            vec![0xB0, 98, 104],
            vec![0xB0, 6, 3],
            vec![0xB0, 38, 116],
            vec![0xB0, 101, 127],
            vec![0xB0, 100, 127],
        ]
    );
}

#[test]
fn aux_messages_fire_without_a_note() {
    // A hap carrying only sysex (no pitch) still emits the sysex message, and no
    // note-on/off, matching midi.mjs's note-independent handlers.
    let controls = map(&[
        ("sysexid", Value::Int(0x7E)),
        ("sysexdata", Value::List(vec![Value::Int(0x01)])),
    ]);
    let pat = pure(Value::Map(controls));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let data: Vec<Vec<u8>> = msgs.iter().map(|m| m.data.clone()).collect();
    assert_eq!(data, vec![vec![0xF0, 0x7E, 0x01, 0xF7]]);
    assert!(
        !data
            .iter()
            .any(|m| m.first().map(|b| b & 0xF0) == Some(NOTE_ON))
    );
}

#[test]
fn aftertouch_accompanies_a_note_at_the_onset() {
    // note + miditouch: both fire at the onset (aftertouch before the note-on).
    let controls = map(&[("note", Value::Int(60)), ("miditouch", Value::F64(1.0))]);
    let pat = pure(Value::Map(controls));
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let data: Vec<Vec<u8>> = msgs.iter().map(|m| m.data.clone()).collect();
    assert!(data.contains(&vec![0xD0, 127]));
    assert!(data.contains(&vec![0x90, 60, clamp7(0.9 * 127.0)]));
}

#[test]
fn overlapping_mpe_notes_use_different_member_channels() {
    let pat = rudel_core::stack(&[note(pure(Value::F64(60.25))), note(pure(Value::F64(64.25)))]);
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let mut channels: Vec<u8> = msgs
        .iter()
        .filter(|m| m.data.first().map(|b| b & 0xF0) == Some(NOTE_ON))
        .map(|m| m.data[0] & 0x0F)
        .collect();
    channels.sort();
    assert_eq!(channels, vec![1, 2]);
}

#[test]
fn bend_range_changes_mpe_scaling() {
    let pat = note(pure(Value::F64(60.25))).bend_range(12.0);
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let data: Vec<Vec<u8>> = msgs.into_iter().map(|m| m.data).collect();
    assert!(data.contains(&vec![0xB1, 6, 12]));
    assert!(data.contains(&pitch_bend_bytes(1, bend_value(60.25, 60, 12.0)).to_vec()));
}

#[test]
fn exhausted_mpe_channels_fall_back_to_master_unbent() {
    let pats: Vec<Pattern> = (0..16)
        .map(|n| note(pure(Value::F64(60.25 + n as f64))))
        .collect();
    let pat = rudel_core::stack(&pats);
    let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
    let note_on_channels: Vec<u8> = msgs
        .iter()
        .filter(|m| m.data.first().map(|b| b & 0xF0) == Some(NOTE_ON))
        .map(|m| m.data[0] & 0x0F)
        .collect();
    assert_eq!(note_on_channels.len(), 16);
    assert!(note_on_channels.contains(&MPE_MASTER_CHANNEL));
    assert!(!msgs.iter().any(|m| m.data[0] == PITCH_BEND)); // no master bend
}

#[test]
fn reset_clears_all_channels_and_centers_bends() {
    let reset = reset_messages();
    assert_eq!(reset.len(), 32);
    for ch in 0..16 {
        assert!(reset.contains(&vec![CONTROL_CHANGE | ch, 123, 0]));
        assert!(reset.contains(&vec![PITCH_BEND | ch, 0, 64]));
    }
}

#[test]
fn input_cc_decodes_channel_and_scales_value() {
    let mut clock = ClockDetector::new();
    // CC #74 = 127 on channel 1 (status 0xB0) -> value 1.0, channel 1.
    let action = process_input(&[0xB0, 74, 127], &mut clock, 0.0);
    assert_eq!(
        action,
        InputAction::Cc {
            channel: 1,
            cc: 74,
            value: 1.0
        }
    );
    // channel nibble 2 (status 0xB2), half value
    let action = process_input(&[0xB2, 10, 64], &mut clock, 0.0);
    assert_eq!(
        action,
        InputAction::Cc {
            channel: 3,
            cc: 10,
            value: 64.0 / 127.0
        }
    );
}

#[test]
fn clock_detector_estimates_bpm() {
    // 120 BPM = 2 beats/sec = 48 clock pulses/sec -> interval 1/48 s.
    let mut clock = ClockDetector::new();
    let dt = 1.0 / 48.0;
    let mut now = 0.0;
    for _ in 0..96 {
        process_input(&[CLOCK], &mut clock, now);
        now += dt;
    }
    let bpm = clock.bpm().expect("a bpm estimate after many pulses");
    assert!((bpm - 120.0).abs() < 1.0, "expected ~120 BPM, got {bpm}");
    // 120 BPM over 4 beats/cycle -> cps 0.5.
    assert!((bpm_to_cps(bpm, 4.0) - 0.5).abs() < 0.01);
}

#[test]
fn transport_resets_the_clock() {
    let mut clock = ClockDetector::new();
    process_input(&[CLOCK], &mut clock, 0.0);
    process_input(&[CLOCK], &mut clock, 0.02);
    assert!(clock.bpm().is_some());
    assert_eq!(
        process_input(&[START], &mut clock, 0.03),
        InputAction::Transport
    );
    assert!(clock.bpm().is_none(), "transport should reset the estimate");
}

#[test]
fn input_cc_reaches_the_core_bus() {
    // The side-effecting path the connection callback runs.
    rudel_core::clear_cc();
    if let InputAction::Cc { channel, cc, value } =
        process_input(&[0xB0, 20, 100], &mut ClockDetector::new(), 0.0)
    {
        rudel_core::set_cc(channel, cc, value);
    }
    assert!((rudel_core::get_cc(1, 20) - 100.0 / 127.0).abs() < 1e-9);
}

#[test]
fn engine_sends_through_a_sink() {
    // Drive the engine with a recording sink and confirm a note-on arrives.
    #[derive(Clone)]
    struct Rec(Arc<Mutex<Vec<Vec<u8>>>>);
    impl MidiSink for Rec {
        fn send(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().push(bytes.to_vec());
        }
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Rec(log.clone());
    let pat = note(pure(Value::Int(60)));
    let engine = MidiEngine::start(sink, pat, 4.0); // fast cps for a quick test
    std::thread::sleep(Duration::from_millis(120));
    engine.stop();
    drop(engine);
    let got = log.lock().unwrap();
    assert!(
        got.iter()
            .any(|m| m.first().map(|b| b & 0xF0) == Some(NOTE_ON)),
        "expected at least one note-on, got {got:?}"
    );
    let _ = (Frac::zero(), silence()); // keep imports tidy across cfgs
}

#[test]
fn engine_emits_sysex_and_note_through_the_sink() {
    // End-to-end: a hap carrying both a note and sysex flows through the real
    // scheduler thread and the note-independent aux path to a fake device.
    #[derive(Clone)]
    struct Rec(Arc<Mutex<Vec<Vec<u8>>>>);
    impl MidiSink for Rec {
        fn send(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().push(bytes.to_vec());
        }
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Rec(log.clone());
    let controls = map(&[
        ("note", Value::Int(60)),
        ("sysexid", Value::Int(0x7E)),
        ("sysexdata", Value::List(vec![Value::Int(0x01)])),
    ]);
    let pat = pure(Value::Map(controls));
    let engine = MidiEngine::start(sink, pat, 4.0);
    std::thread::sleep(Duration::from_millis(120));
    engine.stop();
    drop(engine);
    let got = log.lock().unwrap();
    assert!(
        got.iter().any(|m| *m == vec![0xF0, 0x7E, 0x01, 0xF7]),
        "expected a sysex frame, got {got:?}"
    );
    assert!(
        got.iter()
            .any(|m| m.first().map(|b| b & 0xF0) == Some(NOTE_ON)),
        "expected a note-on alongside the sysex, got {got:?}"
    );
}
