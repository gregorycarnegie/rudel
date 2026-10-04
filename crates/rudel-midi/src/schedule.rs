use crate::{
    CONTROL_CHANGE, DEFAULT_BEND_RANGE, MPE_FIRST_MEMBER, MPE_LAST_MEMBER, MPE_MASTER_CHANNEL,
    Port,
    note::{
        aux_messages, control_to_midi, is_controller, latency_seconds, note_offset_seconds, port_of,
    },
};
use rudel_core::{Pattern, query_controls};

/// A MIDI message stamped with the time (in seconds, on the engine clock) at
/// which it should be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedMidi {
    pub at_seconds: f64,
    pub data: Vec<u8>,
    /// The device it goes to, when not the output the app opened.
    pub port: Option<Port>,
}

#[derive(Clone, Debug)]
pub(crate) struct MpeState {
    configured_range: Option<(u8, u8)>,
    active_until: [f64; 16],
}

impl MpeState {
    pub(crate) fn new() -> Self {
        Self {
            configured_range: None,
            active_until: [0.0; 16],
        }
    }

    fn allocate(&mut self, on: f64, off: f64) -> Option<u8> {
        // Expired slots need no sweep: the scan below already takes the first
        // channel whose note has ended by `on`.
        for ch in MPE_FIRST_MEMBER..=MPE_LAST_MEMBER {
            let slot = &mut self.active_until[ch as usize];
            if *slot <= on {
                *slot = off;
                return Some(ch);
            }
        }
        None
    }

    fn setup_messages(&mut self, at_seconds: f64, bend_range: f64) -> Vec<TimedMidi> {
        let key = bend_range_key(bend_range);
        if self.configured_range == Some(key) {
            return Vec::new();
        }
        self.configured_range = Some(key);

        let mut out = Vec::new();
        // Lower-zone setup: master channel 1, member channels 2-16.
        push_rpn(
            &mut out,
            at_seconds,
            MPE_MASTER_CHANNEL,
            0,
            6,
            MPE_LAST_MEMBER,
            0,
        );
        for ch in MPE_FIRST_MEMBER..=MPE_LAST_MEMBER {
            push_rpn(&mut out, at_seconds, ch, 0, 0, key.0, key.1);
        }
        out
    }
}

pub(crate) fn bend_range_key(bend_range: f64) -> (u8, u8) {
    let range = if bend_range > 0.0 {
        bend_range
    } else {
        DEFAULT_BEND_RANGE
    }
    .clamp(0.0, 96.0);
    let semis = range.floor().clamp(0.0, 96.0) as u8;
    let cents = ((range - semis as f64) * 100.0).round().clamp(0.0, 99.0) as u8;
    (semis, cents)
}

fn push_cc(out: &mut Vec<TimedMidi>, at_seconds: f64, channel: u8, cc: u8, value: u8) {
    out.push(TimedMidi {
        at_seconds,
        data: vec![CONTROL_CHANGE | (channel & 0x0F), cc, value],
        port: None,
    });
}

fn push_rpn(
    out: &mut Vec<TimedMidi>,
    at_seconds: f64,
    channel: u8,
    rpn_msb: u8,
    rpn_lsb: u8,
    data_msb: u8,
    data_lsb: u8,
) {
    push_cc(out, at_seconds, channel, 101, rpn_msb);
    push_cc(out, at_seconds, channel, 100, rpn_lsb);
    push_cc(out, at_seconds, channel, 6, data_msb);
    push_cc(out, at_seconds, channel, 38, data_lsb);
    push_cc(out, at_seconds, channel, 101, 127);
    push_cc(out, at_seconds, channel, 100, 127);
}

/// Produce the time-stamped MIDI messages for every onset in the cycle window
/// `[begin_cycle, end_cycle)`: a note-on (plus any CC/program) at the onset and
/// a matching note-off at the end of the event.
pub fn schedule_window(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
) -> Vec<TimedMidi> {
    let mut mpe = MpeState::new();
    schedule_window_with_state(pattern, cps, begin_cycle, end_cycle, &mut mpe)
}

pub(crate) fn schedule_window_with_state(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
    mpe_state: &mut MpeState,
) -> Vec<TimedMidi> {
    let mut out = Vec::new();
    for ev in query_controls(pattern, cps, begin_cycle, end_cycle) {
        let first = out.len();
        let on = ev.onset_seconds;
        let at = |data: Vec<u8>, at_seconds: f64| TimedMidi {
            at_seconds,
            data,
            port: None,
        };

        // Note-independent messages (CC, program, sysex, NRPN, aftertouch, raw
        // bend) fire at the onset whether or not the hap carries a note, like
        // midi.mjs.
        out.extend(
            aux_messages(&ev.controls)
                .into_iter()
                .map(|data| at(data, on)),
        );

        // A controller (`isController`) is sent everything but notes.
        let note = (!is_controller(&ev.controls))
            .then(|| control_to_midi(&ev.controls))
            .flatten();
        if let Some(mut note) = note {
            // Hold for the event, less `noteOffsetMs`, so the note-off does not
            // land on the next note-on.
            let off = on + (ev.duration_seconds - note_offset_seconds(&ev.controls)).max(0.0);
            if note.mpe {
                out.extend(mpe_state.setup_messages(on, note.bend_range));
                if let Some(channel) = mpe_state.allocate(on, off) {
                    note.channel = channel;
                } else {
                    note.channel = MPE_MASTER_CHANNEL;
                    note.bend = None;
                }
            }
            if let Some(bytes) = note.pitch_bend_bytes() {
                out.push(at(bytes.to_vec(), on));
            }
            out.push(at(note.note_on_bytes().to_vec(), on));
            out.push(at(note.note_off_bytes().to_vec(), off));
        }

        let port = port_of(&ev.controls);
        let latency = latency_seconds(&ev.controls);
        for m in &mut out[first..] {
            m.at_seconds += latency;
            m.port = port.clone();
        }
    }
    out.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    out
}
