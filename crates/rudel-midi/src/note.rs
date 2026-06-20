use crate::{
    CHANNEL_AFTERTOUCH, CONTROL_CHANGE, DEFAULT_BEND_RANGE, NOTE_OFF, NOTE_ON, PITCH_BEND,
    PROGRAM_CHANGE, SYSEX_END, SYSEX_START,
};
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
    let channel = channel_of(controls);

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

/// 0-based MIDI channel from `midichan`/`channel` (1-based), defaulting to 1.
pub(crate) fn channel_of(controls: &BTreeMap<String, Value>) -> u8 {
    let chan = get_f64(controls, "midichan")
        .or_else(|| get_f64(controls, "channel"))
        .unwrap_or(1.0);
    ((chan as i64 - 1).clamp(0, 15)) as u8
}

/// Collect a value or list of values into MIDI bytes (0..=255).
fn bytes_from_value(v: &Value) -> Vec<u8> {
    let to_byte = |x: f64| x.round().clamp(0.0, 255.0) as u8;
    match v {
        Value::List(items) => items.iter().filter_map(Value::as_f64).map(to_byte).collect(),
        other => other.as_f64().map(to_byte).into_iter().collect(),
    }
}

/// Split a value (single 14-bit number, or `[msb, lsb]` list) into 7-bit MSB/LSB.
fn split14(v: &Value) -> (u8, u8) {
    match v {
        Value::List(items) => {
            let msb = items.first().and_then(Value::as_f64).map(clamp7).unwrap_or(0);
            let lsb = items.get(1).and_then(Value::as_f64).map(clamp7).unwrap_or(0);
            (msb, lsb)
        }
        other => {
            let n = other.as_f64().unwrap_or(0.0).round().clamp(0.0, 16383.0) as u16;
            (((n >> 7) & 0x7F) as u8, (n & 0x7F) as u8)
        }
    }
}

/// Canonical NRPN sequence on `channel`: parameter (CC 99/98), data (CC 6/38),
/// then the null-select (CC 101/100 = 127) that deactivates further data entry.
/// midi.mjs delegates to WebMidi.js's `sendNRPN`; rudel emits the standard
/// conformant byte stream (the upstream wrapper's exact arg shape is ambiguous).
fn nrpn_messages(channel: u8, param: &Value, value: &Value) -> Vec<Vec<u8>> {
    let (p_msb, p_lsb) = split14(param);
    let (v_msb, v_lsb) = split14(value);
    let cc = |controller, val| vec![CONTROL_CHANGE | channel, controller, val];
    vec![
        cc(99, p_msb),
        cc(98, p_lsb),
        cc(6, v_msb),
        cc(38, v_lsb),
        cc(101, 127),
        cc(100, 127),
    ]
}

/// Note-independent MIDI messages from a control map, in midi.mjs's handler
/// order: channel aftertouch (`miditouch`), system exclusive (`sysexid` +
/// `sysexdata`), NRPN (`nrpnn` + `nrpv`), and raw pitch bend (`midibend`). These
/// fire whether or not the hap carries a note, matching `Pattern.prototype.midi`.
pub(crate) fn aux_messages(controls: &BTreeMap<String, Value>) -> Vec<Vec<u8>> {
    let channel = channel_of(controls);
    let mut out = Vec::new();

    // System exclusive: F0, <manufacturer id bytes>, <data bytes>, F7.
    if let (Some(id), Some(data)) = (controls.get("sysexid"), controls.get("sysexdata")) {
        let mut msg = vec![SYSEX_START];
        msg.extend(bytes_from_value(id));
        msg.extend(bytes_from_value(data));
        msg.push(SYSEX_END);
        out.push(msg);
    }

    // NRPN non-registered parameter (nrpnn) + value (nrpv).
    if let (Some(n), Some(v)) = (controls.get("nrpnn"), controls.get("nrpv")) {
        out.extend(nrpn_messages(channel, n, v));
    }

    // Raw pitch bend: midibend in -1..1 -> 14-bit centered at 8192, matching
    // WebMidi.js `sendPitchBend` (round((v + 1) / 2 * 16383)).
    if let Some(b) = get_f64(controls, "midibend") {
        let level = (((b + 1.0) / 2.0) * 16383.0).round().clamp(0.0, 16383.0) as u16;
        out.push(pitch_bend_bytes(channel, level).to_vec());
    }

    // Channel aftertouch: miditouch in 0..1 -> 7-bit pressure.
    if let Some(t) = get_f64(controls, "miditouch") {
        out.push(vec![CHANNEL_AFTERTOUCH | channel, clamp7(t * 127.0)]);
    }

    out
}
