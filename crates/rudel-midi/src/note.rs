use crate::{CONTROL_CHANGE, DEFAULT_BEND_RANGE, NOTE_OFF, NOTE_ON, PITCH_BEND, PROGRAM_CHANGE};
use rudel_core::{Value, freq_to_midi, note_to_midi};
use std::collections::BTreeMap;

/// Clamp a float to a 0..=127 MIDI data byte.
pub(crate) fn clamp7(x: f64) -> u8 {
    x.round().clamp(0.0, 127.0) as u8
}

/// A resolved MIDI note plus any control-change / program-change messages that
/// accompany it, derived from a control map.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiNote {
    /// Channel, 0..=15.
    pub channel: u8,
    /// Fractional MIDI pitch before rounding/clamping.
    pub pitch: f64,
    pub note: u8,
    pub velocity: u8,
    /// `(controller, value)` pairs to send at the note onset.
    pub ccs: Vec<(u8, u8)>,
    /// Program change to send at the note onset, if any.
    pub program: Option<u8>,
    /// Use lower-zone MPE for this note.
    pub mpe: bool,
    /// Pitch-bend range in semitones for MPE member channels.
    pub bend_range: f64,
    /// 14-bit pitch bend value, centered at 8192.
    pub bend: Option<u16>,
}

impl MidiNote {
    pub fn note_on_bytes(&self) -> [u8; 3] {
        [NOTE_ON | (self.channel & 0x0F), self.note, self.velocity]
    }

    pub fn note_off_bytes(&self) -> [u8; 3] {
        [NOTE_OFF | (self.channel & 0x0F), self.note, 0]
    }

    pub fn cc_bytes(&self, controller: u8, value: u8) -> [u8; 3] {
        [CONTROL_CHANGE | (self.channel & 0x0F), controller, value]
    }

    pub fn program_bytes(&self, program: u8) -> [u8; 2] {
        [PROGRAM_CHANGE | (self.channel & 0x0F), program]
    }

    pub fn pitch_bend_bytes(&self) -> Option<[u8; 3]> {
        self.bend.map(|bend| pitch_bend_bytes(self.channel, bend))
    }
}

fn get_f64(m: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    m.get(key).and_then(|v| v.as_f64())
}

fn get_bool(m: &BTreeMap<String, Value>, key: &str) -> Option<bool> {
    m.get(key).map(Value::truthy)
}

/// Resolve a note value (number or note name) to a MIDI number.
fn value_to_note(v: &Value) -> Option<f64> {
    match v {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| note_to_midi(s).map(|m| m as f64)),
        other => other.as_f64(),
    }
}

pub(crate) fn bend_value(pitch: f64, note: u8, range: f64) -> u16 {
    let range = if range > 0.0 {
        range
    } else {
        DEFAULT_BEND_RANGE
    };
    let semis = pitch - note as f64;
    (8192.0 + (semis / range) * 8192.0)
        .round()
        .clamp(0.0, 16383.0) as u16
}

pub(crate) fn pitch_bend_bytes(channel: u8, bend: u16) -> [u8; 3] {
    [
        PITCH_BEND | (channel & 0x0F),
        (bend & 0x7F) as u8,
        ((bend >> 7) & 0x7F) as u8,
    ]
}

/// Reset messages sent on MIDI engine shutdown.
pub fn reset_messages() -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(32);
    for ch in 0..16 {
        out.push(vec![CONTROL_CHANGE | ch, 123, 0]);
        out.push(pitch_bend_bytes(ch, 8192).to_vec());
    }
    out
}

/// Map a control map to a [`MidiNote`], or `None` if it carries no pitch.
///
/// - pitch from `freq` first, then `note`/`n` (number or note name)
/// - velocity from `velocity` (0..1), else `gain` (0..1), else 0.9
/// - channel from `midichan`/`channel` (1-based), else 1
/// - control-change from `ccn` + `ccv` (value 0..1)
/// - program change from `progNum`
pub fn control_to_midi(controls: &BTreeMap<String, Value>) -> Option<MidiNote> {
    let freq_pitch = controls
        .get("freq")
        .and_then(Value::as_f64)
        .filter(|f| *f > 0.0)
        .map(freq_to_midi);
    let pitch = freq_pitch.or_else(|| {
        controls
            .get("note")
            .or_else(|| controls.get("n"))
            .and_then(value_to_note)
    })?;
    if !pitch.is_finite() {
        return None;
    }

    let velocity = get_f64(controls, "velocity")
        .or_else(|| get_f64(controls, "gain"))
        .unwrap_or(0.9);
    let chan = get_f64(controls, "midichan")
        .or_else(|| get_f64(controls, "channel"))
        .unwrap_or(1.0);
    let channel = ((chan as i64 - 1).clamp(0, 15)) as u8;

    let mut ccs = Vec::new();
    if let (Some(n), Some(v)) = (get_f64(controls, "ccn"), get_f64(controls, "ccv")) {
        ccs.push((clamp7(n), clamp7(v * 127.0)));
    }
    let program = get_f64(controls, "progNum").map(clamp7);
    let bend_range = get_f64(controls, "bendRange")
        .filter(|r| *r > 0.0)
        .unwrap_or(DEFAULT_BEND_RANGE);
    let fractional = (pitch - pitch.round()).abs() > 1e-9;
    let mpe = get_bool(controls, "mpe").unwrap_or(freq_pitch.is_some() || fractional);
    let note = clamp7(pitch);
    let bend = mpe.then(|| bend_value(pitch, note, bend_range));

    Some(MidiNote {
        channel,
        pitch,
        note,
        velocity: clamp7(velocity * 127.0),
        ccs,
        program,
        mpe,
        bend_range,
        bend,
    })
}
