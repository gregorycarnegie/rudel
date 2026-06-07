// rudel-midi - MIDI output for Rudel.
// Maps a pattern's control events to MIDI note-on/off, control-change and clock
// messages, and drives them in real time over a `midir` connection.
// SPDX-License-Identifier: AGPL-3.0-or-later

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use rudel_core::{Pattern, Value, freq_to_midi, note_to_midi, query_controls};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// MIDI status bytes (channel goes in the low nibble).
const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;
const CONTROL_CHANGE: u8 = 0xB0;
const PROGRAM_CHANGE: u8 = 0xC0;
const PITCH_BEND: u8 = 0xE0;
const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;
const MPE_MASTER_CHANNEL: u8 = 0;
const MPE_FIRST_MEMBER: u8 = 1;
const MPE_LAST_MEMBER: u8 = 15;
const DEFAULT_BEND_RANGE: f64 = 2.0;

/// Clamp a float to a 0..=127 MIDI data byte.
fn clamp7(x: f64) -> u8 {
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

fn bend_value(pitch: f64, note: u8, range: f64) -> u16 {
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

fn pitch_bend_bytes(channel: u8, bend: u16) -> [u8; 3] {
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

/// A MIDI message stamped with the time (in seconds, on the engine clock) at
/// which it should be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedMidi {
    pub at_seconds: f64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
struct MpeState {
    configured_range: Option<(u8, u8)>,
    active_until: [f64; 16],
}

impl MpeState {
    fn new() -> Self {
        Self {
            configured_range: None,
            active_until: [0.0; 16],
        }
    }

    fn free_expired(&mut self, now: f64) {
        for ch in MPE_FIRST_MEMBER..=MPE_LAST_MEMBER {
            let slot = &mut self.active_until[ch as usize];
            if *slot <= now {
                *slot = 0.0;
            }
        }
    }

    fn allocate(&mut self, on: f64, off: f64) -> Option<u8> {
        self.free_expired(on);
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

fn bend_range_key(bend_range: f64) -> (u8, u8) {
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

fn schedule_window_with_state(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
    mpe_state: &mut MpeState,
) -> Vec<TimedMidi> {
    let mut out = Vec::new();
    for ev in query_controls(pattern, cps, begin_cycle, end_cycle) {
        let Some(mut note) = control_to_midi(&ev.controls) else {
            continue;
        };
        let on = ev.onset_seconds;
        // Hold for the event duration, minus a tiny gap to retrigger cleanly.
        let off = on + (ev.duration_seconds - 0.001).max(0.0);
        if note.mpe {
            out.extend(mpe_state.setup_messages(on, note.bend_range));
            if let Some(channel) = mpe_state.allocate(on, off) {
                note.channel = channel;
            } else {
                note.channel = MPE_MASTER_CHANNEL;
                note.bend = None;
            }
        }
        if let Some(p) = note.program {
            out.push(TimedMidi {
                at_seconds: on,
                data: note.program_bytes(p).to_vec(),
            });
        }
        for &(c, v) in &note.ccs {
            out.push(TimedMidi {
                at_seconds: on,
                data: note.cc_bytes(c, v).to_vec(),
            });
        }
        if let Some(bytes) = note.pitch_bend_bytes() {
            out.push(TimedMidi {
                at_seconds: on,
                data: bytes.to_vec(),
            });
        }
        out.push(TimedMidi {
            at_seconds: on,
            data: note.note_on_bytes().to_vec(),
        });
        out.push(TimedMidi {
            at_seconds: off,
            data: note.note_off_bytes().to_vec(),
        });
    }
    out.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    out
}

/// Anything that can receive raw MIDI bytes. Implemented by [`MidiOut`]; a
/// recording sink is used in tests.
pub trait MidiSink: Send {
    fn send(&mut self, bytes: &[u8]);
}

/// A connection to a MIDI output port.
pub struct MidiOut {
    conn: MidiOutputConnection,
}

impl MidiOut {
    /// List the names of the available MIDI output ports.
    pub fn list_ports() -> Result<Vec<String>, String> {
        let out = MidiOutput::new("rudel").map_err(|e| e.to_string())?;
        Ok(out
            .ports()
            .iter()
            .filter_map(|p| out.port_name(p).ok())
            .collect())
    }

    /// Connect to an output port whose name contains `name_substr` (case
    /// insensitive), or the first available port when `None`.
    pub fn connect(name_substr: Option<&str>) -> Result<MidiOut, String> {
        let out = MidiOutput::new("rudel").map_err(|e| e.to_string())?;
        let ports = out.ports();
        if ports.is_empty() {
            return Err("no MIDI output ports available".to_string());
        }
        let port = match name_substr {
            Some(needle) => {
                let needle = needle.to_lowercase();
                ports
                    .iter()
                    .find(|p| {
                        out.port_name(p)
                            .map(|n| n.to_lowercase().contains(&needle))
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| format!("no MIDI port matching {needle:?}"))?
            }
            None => &ports[0],
        };
        let conn = out
            .connect(port, "rudel-out")
            .map_err(|e| format!("MIDI connect failed: {e}"))?;
        Ok(MidiOut { conn })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.conn.send(bytes).map_err(|e| e.to_string())
    }

    /// Send a MIDI clock tick (`0xF8`); 24 per quarter note by convention.
    pub fn clock(&mut self) {
        let _ = self.conn.send(&[CLOCK]);
    }
    pub fn transport_start(&mut self) {
        let _ = self.conn.send(&[START]);
    }
    pub fn transport_continue(&mut self) {
        let _ = self.conn.send(&[CONTINUE]);
    }
    pub fn transport_stop(&mut self) {
        let _ = self.conn.send(&[STOP]);
    }
}

impl MidiSink for MidiOut {
    fn send(&mut self, bytes: &[u8]) {
        let _ = self.send(bytes);
    }
}

/// A running MIDI scheduler: a background thread queries the pattern ahead of a
/// real-time clock and sends note messages through a [`MidiSink`].
pub struct MidiEngine {
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<Mutex<f64>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MidiEngine {
    /// Start scheduling `pattern` to `sink` at `cps` cycles per second.
    pub fn start<S: MidiSink + 'static>(sink: S, pattern: Pattern, cps: f64) -> MidiEngine {
        let pattern = Arc::new(RwLock::new(pattern));
        let cps = Arc::new(Mutex::new(cps));
        let running = Arc::new(AtomicBool::new(true));
        let handle = {
            let pattern = pattern.clone();
            let cps = cps.clone();
            let running = running.clone();
            std::thread::spawn(move || run_scheduler(sink, pattern, cps, running))
        };
        MidiEngine {
            pattern,
            cps,
            running,
            handle: Some(handle),
        }
    }

    pub fn set_pattern(&self, pat: Pattern) {
        *self.pattern.write().unwrap() = pat;
    }
    pub fn set_cps(&self, cps: f64) {
        *self.cps.lock().unwrap() = cps;
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for MidiEngine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

const LOOKAHEAD: f64 = 0.1;

fn run_scheduler<S: MidiSink>(
    mut sink: S,
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<Mutex<f64>>,
    running: Arc<AtomicBool>,
) {
    let start = Instant::now();
    let mut scheduled_cycle = 0.0_f64;
    let mut pending: Vec<TimedMidi> = Vec::new();
    let mut mpe_state = MpeState::new();
    while running.load(Ordering::Relaxed) {
        let cps_now = *cps.lock().unwrap();
        let now = start.elapsed().as_secs_f64();
        let target_cycle = (now + LOOKAHEAD) * cps_now;
        if target_cycle > scheduled_cycle {
            let pat = pattern.read().unwrap().clone();
            pending.extend(schedule_window_with_state(
                &pat,
                cps_now,
                scheduled_cycle,
                target_cycle,
                &mut mpe_state,
            ));
            pending.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
            scheduled_cycle = target_cycle;
        }
        let now = start.elapsed().as_secs_f64();
        while pending.first().is_some_and(|m| m.at_seconds <= now) {
            let m = pending.remove(0);
            sink.send(&m.data);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    for message in reset_messages() {
        sink.send(&message);
    }
}

// ---------------------------------------------------------------------------
// MIDI input: incoming CC -> the rudel-core input bus, plus clock-in tempo.

/// Estimates tempo from incoming MIDI clock pulses (24 per quarter note),
/// smoothing the inter-pulse interval with an EWMA so the BPM is stable.
#[derive(Default)]
pub struct ClockDetector {
    last: Option<f64>,
    /// EWMA of the inter-pulse interval, in seconds.
    interval: Option<f64>,
}

impl ClockDetector {
    pub fn new() -> ClockDetector {
        ClockDetector::default()
    }

    /// Forget timing state (on transport start/stop, or device change).
    pub fn reset(&mut self) {
        self.last = None;
        self.interval = None;
    }

    /// Feed a clock pulse at `now` (seconds). Returns the current BPM estimate
    /// once at least two pulses have been seen.
    pub fn pulse(&mut self, now: f64) -> Option<f64> {
        if let Some(last) = self.last {
            let dt = now - last;
            if dt > 0.0 {
                self.interval = Some(match self.interval {
                    Some(prev) => prev * 0.8 + dt * 0.2,
                    None => dt,
                });
            }
        }
        self.last = Some(now);
        self.bpm()
    }

    /// The current BPM estimate (24 pulses per quarter note), if known.
    pub fn bpm(&self) -> Option<f64> {
        self.interval.map(|i| 60.0 / (i * 24.0))
    }
}

/// Convert a BPM to cycles-per-second, given how many beats fill one cycle
/// (Strudel's default cycle is one bar of `beats_per_cycle` beats).
pub fn bpm_to_cps(bpm: f64, beats_per_cycle: f64) -> f64 {
    bpm / 60.0 / beats_per_cycle.max(1.0)
}

/// The decoded effect of one incoming MIDI message (the testable core of the
/// input callback).
#[derive(Clone, Debug, PartialEq)]
pub enum InputAction {
    /// A control-change: channel (1..=16), controller, value scaled to 0..1.
    Cc { channel: u8, cc: u8, value: f64 },
    /// A new tempo estimate (BPM) from the clock.
    Bpm(f64),
    /// Transport start/stop/continue (resets the clock estimate).
    Transport,
    /// Nothing actionable.
    None,
}

/// Decode one incoming MIDI message, advancing the clock detector. Pure so the
/// routing can be unit-tested without a device.
pub fn process_input(bytes: &[u8], clock: &mut ClockDetector, now: f64) -> InputAction {
    let Some(&status) = bytes.first() else {
        return InputAction::None;
    };
    if status & 0xF0 == CONTROL_CHANGE && bytes.len() >= 3 {
        return InputAction::Cc {
            channel: (status & 0x0F) + 1,
            cc: bytes[1],
            value: bytes[2] as f64 / 127.0,
        };
    }
    match status {
        CLOCK => match clock.pulse(now) {
            Some(bpm) => InputAction::Bpm(bpm),
            None => InputAction::None,
        },
        START | CONTINUE | STOP => {
            clock.reset();
            InputAction::Transport
        }
        _ => InputAction::None,
    }
}

/// A live MIDI input connection. Incoming CC messages are written to the
/// `rudel-core` input bus (readable via `cc_in`/`ccin`); MIDI clock updates a
/// shared BPM estimate (`bpm`/`cps`) for clock-in tempo sync.
pub struct MidiIn {
    _conn: MidiInputConnection<()>,
    bpm: Arc<Mutex<Option<f64>>>,
}

impl MidiIn {
    /// List the names of the available MIDI input ports.
    pub fn list_ports() -> Result<Vec<String>, String> {
        let input = MidiInput::new("rudel-in-list").map_err(|e| e.to_string())?;
        Ok(input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .collect())
    }

    /// Connect to an input port whose name contains `name_substr` (case
    /// insensitive), or the first available port when `None`. Incoming CCs flow
    /// to the global input bus; clock pulses update the BPM estimate.
    pub fn connect(name_substr: Option<&str>) -> Result<MidiIn, String> {
        let mut input = MidiInput::new("rudel-in").map_err(|e| e.to_string())?;
        // Receive timing (clock) messages too, which midir ignores by default.
        input.ignore(Ignore::None);
        let ports = input.ports();
        if ports.is_empty() {
            return Err("no MIDI input ports available".to_string());
        }
        let port = match name_substr {
            Some(needle) => {
                let needle = needle.to_lowercase();
                ports
                    .iter()
                    .find(|p| {
                        input
                            .port_name(p)
                            .map(|n| n.to_lowercase().contains(&needle))
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| format!("no MIDI input port matching {needle:?}"))?
                    .clone()
            }
            None => ports[0].clone(),
        };
        let bpm = Arc::new(Mutex::new(None));
        let bpm_cb = bpm.clone();
        let mut clock = ClockDetector::new();
        let start = Instant::now();
        let conn = input
            .connect(
                &port,
                "rudel-in",
                move |_stamp, message, _| {
                    let now = start.elapsed().as_secs_f64();
                    match process_input(message, &mut clock, now) {
                        InputAction::Cc { channel, cc, value } => {
                            rudel_core::set_cc(channel, cc, value);
                        }
                        InputAction::Bpm(b) => *bpm_cb.lock().unwrap() = Some(b),
                        InputAction::Transport | InputAction::None => {}
                    }
                },
                (),
            )
            .map_err(|e| format!("MIDI input connect failed: {e}"))?;
        Ok(MidiIn { _conn: conn, bpm })
    }

    /// The latest BPM estimate from incoming MIDI clock, if any.
    pub fn bpm(&self) -> Option<f64> {
        *self.bpm.lock().unwrap()
    }

    /// The latest clock-in tempo as cycles-per-second (`beats_per_cycle` beats
    /// to a cycle), if a clock has been detected.
    pub fn cps(&self, beats_per_cycle: f64) -> Option<f64> {
        self.bpm().map(|b| bpm_to_cps(b, beats_per_cycle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Frac, note, pure, sequence, silence};

    fn map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
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
    fn overlapping_mpe_notes_use_different_member_channels() {
        let pat =
            rudel_core::stack(&[note(pure(Value::F64(60.25))), note(pure(Value::F64(64.25)))]);
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
}
