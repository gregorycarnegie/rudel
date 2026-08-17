use super::*;
use crate::note::{aux_messages, bend_value, clamp7, pitch_bend_bytes};
use crate::schedule::bend_range_key;
use rudel_core::{Frac, Pattern, Value, ValueMap, note, pure, sequence, silence};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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
fn midicmd_sends_transport_and_clock() {
    let cmd = |s: &str| aux_messages(&map(&[("midicmd", Value::Str(s.to_string()))]));
    assert_eq!(cmd("clock"), vec![vec![CLOCK]]);
    assert_eq!(cmd("midiClock"), vec![vec![CLOCK]]);
    assert_eq!(cmd("start"), vec![vec![START]]);
    assert_eq!(cmd("stop"), vec![vec![STOP]]);
    assert_eq!(cmd("continue"), vec![vec![CONTINUE]]);
    // An unrecognised command is dropped rather than emitting a stray byte.
    assert!(cmd("wat").is_empty());
}

#[test]
fn midicmd_array_forms_send_channel_messages() {
    let cmd = |items: Vec<Value>, chan: i64| {
        aux_messages(&map(&[
            ("midicmd", Value::List(items)),
            ("midichan", Value::Int(chan)),
        ]))
    };
    let s = |x: &str| Value::Str(x.to_string());
    // ['progNum', n] -> program change on the hap's channel.
    assert_eq!(
        cmd(vec![s("progNum"), Value::Int(5)], 3),
        vec![vec![0xC2, 5]]
    );
    // ['cc', ccn, ccv] with ccv in 0..1 -> control change scaled to 7 bits.
    assert_eq!(
        cmd(vec![s("cc"), Value::Int(74), Value::F64(0.5)], 1),
        vec![vec![0xB0, 74, 64]]
    );
    // ['sysex', id, data] frames like the sysexid/sysexdata pair.
    assert_eq!(
        cmd(
            vec![
                s("sysex"),
                Value::Int(0x7E),
                Value::List(vec![Value::Int(0x01)])
            ],
            1
        ),
        vec![vec![0xF0, 0x7E, 0x01, 0xF7]]
    );
    // Wrong arity is ignored rather than emitting a truncated message.
    assert!(cmd(vec![s("cc"), Value::Int(74)], 1).is_empty());
    assert!(cmd(vec![s("progNum")], 1).is_empty());
}

#[test]
fn midimap_turns_mapped_controls_into_ccs() {
    use rudel_core::{CcMapping, set_midimap};
    set_midimap(
        "midi_test_map",
        [
            (
                "lpf".to_string(),
                CcMapping {
                    ccn: 74,
                    min: 0.0,
                    max: 20000.0,
                    exp: 1.0,
                },
            ),
            ("gain".to_string(), CcMapping::new(7)),
        ],
    );
    let msgs = aux_messages(&map(&[
        ("midimap", Value::Str("midi_test_map".to_string())),
        ("cutoff", Value::F64(10000.0)),
        ("gain", Value::F64(1.0)),
        // not in the map, so it produces no CC
        ("pan", Value::F64(0.25)),
    ]));
    // Sorted by controller: gain -> CC 7 full, cutoff -> CC 74 at half range.
    assert_eq!(msgs, vec![vec![0xB0, 7, 127], vec![0xB0, 74, 64]]);

    // A hap naming no midimap uses `default`, which is unset here.
    assert!(aux_messages(&map(&[("gain", Value::F64(1.0))])).is_empty());
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
fn input_decodes_note_on_and_ignores_note_off() {
    let mut clock = ClockDetector::new();
    assert_eq!(
        process_input(&[0x90, 60, 127], &mut clock, 0.0),
        InputAction::NoteOn {
            note: 60,
            velocity: 1.0
        }
    );
    // note-on with velocity 0 is a note-off on many devices
    assert_eq!(
        process_input(&[0x90, 60, 0], &mut clock, 0.0),
        InputAction::None
    );
    assert_eq!(
        process_input(&[0x80, 60, 64], &mut clock, 0.0),
        InputAction::None
    );
}

#[test]
fn note_ons_reach_the_core_queue_per_device() {
    rudel_core::clear_midi_notes();
    if let InputAction::NoteOn { note, velocity } =
        process_input(&[0x90, 64, 127], &mut ClockDetector::new(), 0.0)
    {
        rudel_core::push_midi_note("keystep", note, velocity);
    }
    // A device-pinned reader takes the note...
    assert_eq!(rudel_core::take_midi_notes("keystep"), vec![(64, 1.0)]);
    // ...and it is only delivered once, so the wildcard view no longer has it.
    assert!(rudel_core::take_midi_notes("").is_empty());
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

// --- the wire bytes ---------------------------------------------------------
//
// Every message is a status nibble OR'd with a channel, and the channel is
// masked to 4 bits. Getting either wrong sends a valid-looking message to the
// wrong place, or corrupts the status byte into a different message type — both
// silent, because nothing on this side reads it back.

fn note_at(channel: u8) -> MidiNote {
    MidiNote {
        channel,
        pitch: 60.0,
        note: 60,
        velocity: 100,
        ccs: Vec::new(),
        program: None,
        mpe: false,
        bend_range: 2.0,
        bend: None,
    }
}

#[test]
fn status_bytes_carry_the_status_nibble_and_the_channel() {
    for ch in 0u8..16 {
        let n = note_at(ch);
        assert_eq!(
            n.note_on_bytes(),
            [0x90 | ch, 60, 100],
            "note on, channel {ch}"
        );
        assert_eq!(
            n.note_off_bytes(),
            [0x80 | ch, 60, 0],
            "note off, channel {ch}"
        );
        assert_eq!(
            n.cc_bytes(74, 42),
            [0xB0 | ch, 74, 42],
            "control change, channel {ch}"
        );
        assert_eq!(
            n.program_bytes(7),
            [0xC0 | ch, 7],
            "program change, channel {ch}"
        );
    }
    // A note-off is always velocity 0, whatever the note's velocity was.
    assert_eq!(note_at(0).note_off_bytes()[2], 0);
}

#[test]
fn the_channel_is_masked_to_four_bits() {
    // A channel past 15 wraps into the nibble rather than corrupting the status
    // byte above it — 16 has to read as channel 0, not as a different message.
    let over = note_at(16);
    assert_eq!(over.note_on_bytes()[0], 0x90, "channel 16 wraps to 0");
    assert_eq!(note_at(17).note_on_bytes()[0], 0x91, "and 17 to 1");
    assert_eq!(note_at(255).note_on_bytes()[0], 0x9F, "and 255 to 15");
    // The status nibble survives the mask in every case.
    for ch in [0u8, 15, 16, 200, 255] {
        assert_eq!(
            note_at(ch).note_on_bytes()[0] & 0xF0,
            0x90,
            "channel {ch} must not disturb the status nibble"
        );
    }
}

#[test]
fn a_pitch_bend_splits_into_two_seven_bit_halves() {
    // 14-bit value, low 7 bits first then the high 7 — swapping them or masking
    // wrong sends a wildly different pitch.
    let bytes = pitch_bend_bytes(3, 8192);
    assert_eq!(bytes[0], 0xE0 | 3, "status and channel");
    assert_eq!(bytes[1], 0, "centre has a zero LSB");
    assert_eq!(bytes[2], 64, "and 64 as its MSB");

    // Both data bytes always stay inside 7 bits.
    for bend in [0u16, 1, 127, 128, 8191, 8192, 16383] {
        let b = pitch_bend_bytes(0, bend);
        assert!(b[1] < 128 && b[2] < 128, "data bytes are 7-bit for {bend}");
        // ...and reassemble to the original value.
        let round = (b[1] as u16) | ((b[2] as u16) << 7);
        assert_eq!(
            round, bend,
            "{bend} should round-trip through the two halves"
        );
    }
}

#[test]
fn bend_value_maps_semitones_across_the_range() {
    // Centre is 8192; a semitone offset of the full range reaches an extreme.
    assert_eq!(bend_value(60.0, 60, 2.0), 8192, "no offset is centre");
    assert_eq!(
        bend_value(62.0, 60, 2.0),
        16383,
        "+2 semitones over a 2-semitone range is the top"
    );
    assert_eq!(bend_value(58.0, 60, 2.0), 0, "-2 semitones is the bottom");
    // Half the range is half way up from centre.
    assert_eq!(bend_value(61.0, 60, 2.0), 12288);

    // A wider range makes the same offset a smaller bend.
    assert!(
        bend_value(61.0, 60, 12.0) < bend_value(61.0, 60, 2.0),
        "a wider bend range should need less bend for the same interval"
    );

    // A non-positive range falls back to the default rather than dividing by
    // zero.
    assert_eq!(bend_value(61.0, 60, 0.0), bend_value(61.0, 60, 2.0));
    assert_eq!(bend_value(61.0, 60, -5.0), bend_value(61.0, 60, 2.0));

    // Offsets beyond the range clamp into the 14-bit span instead of wrapping.
    assert_eq!(bend_value(90.0, 60, 2.0), 16383);
    assert_eq!(bend_value(20.0, 60, 2.0), 0);
}

#[test]
fn the_mpe_bend_range_splits_into_semitones_and_cents() {
    // The RPN pair is (semitones, cents), so 2.5 semitones is (2, 50).
    assert_eq!(bend_range_key(2.0), (2, 0));
    assert_eq!(bend_range_key(2.5), (2, 50));
    assert_eq!(bend_range_key(12.0), (12, 0));
    assert_eq!(bend_range_key(48.25), (48, 25));

    // Non-positive falls back to the default.
    assert_eq!(bend_range_key(0.0), bend_range_key(2.0));
    assert_eq!(bend_range_key(-1.0), bend_range_key(2.0));

    // Both halves stay in range: semitones cap at 96, cents at 99.
    let (s, c) = bend_range_key(200.0);
    assert!(s <= 96 && c <= 99, "clamped to ({s}, {c})");
    let (_, c) = bend_range_key(1.999);
    assert!(c <= 99, "rounding must not push cents to 100, got {c}");
}

// --- control mapping --------------------------------------------------------

#[test]
fn a_note_needs_a_pitch_and_rejects_a_non_finite_one() {
    let map = |pairs: Vec<(&str, Value)>| -> ValueMap {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    };
    // No note, n or freq at all: nothing to play.
    assert!(control_to_midi(&map(vec![("gain", Value::F64(1.0))])).is_none());
    // A non-finite pitch is rejected rather than clamped into a wrong note.
    assert!(control_to_midi(&map(vec![("note", Value::F64(f64::NAN))])).is_none());
    assert!(control_to_midi(&map(vec![("note", Value::F64(f64::INFINITY))])).is_none());

    // `freq` wins over `note`, and only when positive — a zero or negative
    // frequency has no MIDI number.
    let from_freq = control_to_midi(&map(vec![
        ("freq", Value::F64(440.0)),
        ("note", Value::F64(0.0)),
    ]))
    .expect("freq should give a note");
    assert_eq!(from_freq.note, 69, "440Hz is A4 = 69");
    let zero_freq = control_to_midi(&map(vec![
        ("freq", Value::F64(0.0)),
        ("note", Value::F64(60.0)),
    ]))
    .expect("a zero freq should fall back to note");
    assert_eq!(zero_freq.note, 60);

    // `n` is the fallback when `note` is absent.
    assert_eq!(
        control_to_midi(&map(vec![("n", Value::F64(64.0))]))
            .unwrap()
            .note,
        64
    );
}

#[test]
fn mpe_engages_for_pitches_between_the_keys() {
    let map = |pairs: Vec<(&str, Value)>| -> ValueMap {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    };
    // A whole-number note needs no bend.
    let plain = control_to_midi(&map(vec![("note", Value::F64(60.0))])).unwrap();
    assert!(!plain.mpe, "a whole note should not need MPE");
    assert_eq!(plain.bend, None);

    // A fractional one does, and carries a bend off centre.
    let quarter = control_to_midi(&map(vec![("note", Value::F64(60.5))])).unwrap();
    assert!(quarter.mpe, "a fractional note should engage MPE");
    assert!(
        quarter.bend.is_some_and(|b| b != 8192),
        "and bend away from centre, got {:?}",
        quarter.bend
    );

    // `freq` always goes out as a bend, since it rarely lands on a key.
    assert!(
        control_to_midi(&map(vec![("freq", Value::F64(445.0))]))
            .unwrap()
            .mpe
    );

    // An explicit `mpe` control overrides the guess in both directions.
    assert!(
        !control_to_midi(&map(vec![
            ("note", Value::F64(60.5)),
            ("mpe", Value::Bool(false)),
        ]))
        .unwrap()
        .mpe
    );
    assert!(
        control_to_midi(&map(vec![
            ("note", Value::F64(60.0)),
            ("mpe", Value::Bool(true)),
        ]))
        .unwrap()
        .mpe
    );
}

#[test]
fn cc_and_program_controls_reach_the_note() {
    let map = |pairs: Vec<(&str, Value)>| -> ValueMap {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    };
    // `ccv` is 0..1 scaled to 0..127; both are needed or neither is sent.
    let n = control_to_midi(&map(vec![
        ("note", Value::F64(60.0)),
        ("ccn", Value::F64(74.0)),
        ("ccv", Value::F64(1.0)),
    ]))
    .unwrap();
    assert_eq!(n.ccs, vec![(74, 127)], "ccv 1.0 should be full scale");

    let half = control_to_midi(&map(vec![
        ("note", Value::F64(60.0)),
        ("ccn", Value::F64(1.0)),
        ("ccv", Value::F64(0.5)),
    ]))
    .unwrap();
    assert_eq!(half.ccs, vec![(1, 64)], "ccv 0.5 is about half of 127");

    // `ccn` on its own sends nothing.
    let lone = control_to_midi(&map(vec![
        ("note", Value::F64(60.0)),
        ("ccn", Value::F64(74.0)),
    ]))
    .unwrap();
    assert!(lone.ccs.is_empty(), "ccn without ccv should send no CC");

    // Program change comes from `progNum`.
    assert_eq!(
        control_to_midi(&map(vec![
            ("note", Value::F64(60.0)),
            ("progNum", Value::F64(9.0)),
        ]))
        .unwrap()
        .program,
        Some(9)
    );
    assert_eq!(
        control_to_midi(&map(vec![("note", Value::F64(60.0))]))
            .unwrap()
            .program,
        None
    );
}

/// Opening and closing ports from several threads at once used to corrupt the
/// heap about one run in ten — winmm is not thread-safe across `midiOutClose`
/// and midir does not serialise it. Four tests rather than four spawned
/// threads, so the harness itself provides the concurrency.
///
/// A regression shows up as the *binary* dying, not as a failed assertion.
mod concurrent_port_use {
    fn open_and_close() {
        for _ in 0..8 {
            if let Ok(out) = crate::MidiOut::connect(None) {
                drop(out);
            }
        }
    }

    #[test]
    fn a() {
        open_and_close();
    }
    #[test]
    fn b() {
        open_and_close();
    }
    #[test]
    fn c() {
        open_and_close();
    }
    #[test]
    fn d() {
        open_and_close();
    }
}
