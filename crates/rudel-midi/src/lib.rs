// rudel-midi - MIDI output for Rudel.
// Maps a pattern's control events to MIDI note-on/off, control-change and clock
// messages, and drives them in real time over a `midir` connection.
// SPDX-License-Identifier: AGPL-3.0-or-later

use midir::{MidiOutput, MidiOutputConnection};
use rudel_core::{Pattern, Value, note_to_midi, query_controls};
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
const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;

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
    pub note: u8,
    pub velocity: u8,
    /// `(controller, value)` pairs to send at the note onset.
    pub ccs: Vec<(u8, u8)>,
    /// Program change to send at the note onset, if any.
    pub program: Option<u8>,
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
}

fn get_f64(m: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    m.get(key).and_then(|v| v.as_f64())
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

/// Map a control map to a [`MidiNote`], or `None` if it carries no pitch.
///
/// - pitch from `note`/`n` (number or note name)
/// - velocity from `velocity` (0..1), else `gain` (0..1), else 0.9
/// - channel from `midichan`/`channel` (1-based), else 1
/// - control-change from `ccn` + `ccv` (value 0..1)
/// - program change from `progNum`
pub fn control_to_midi(controls: &BTreeMap<String, Value>) -> Option<MidiNote> {
    let note = controls
        .get("note")
        .or_else(|| controls.get("n"))
        .and_then(value_to_note)?;
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

    Some(MidiNote {
        channel,
        note: clamp7(note),
        velocity: clamp7(velocity * 127.0),
        ccs,
        program,
    })
}

/// A MIDI message stamped with the time (in seconds, on the engine clock) at
/// which it should be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedMidi {
    pub at_seconds: f64,
    pub data: Vec<u8>,
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
    let mut out = Vec::new();
    for ev in query_controls(pattern, cps, begin_cycle, end_cycle) {
        let Some(note) = control_to_midi(&ev.controls) else {
            continue;
        };
        let on = ev.onset_seconds;
        // Hold for the event duration, minus a tiny gap to retrigger cleanly.
        let off = on + (ev.duration_seconds - 0.001).max(0.0);
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
    while running.load(Ordering::Relaxed) {
        let cps_now = *cps.lock().unwrap();
        let now = start.elapsed().as_secs_f64();
        let target_cycle = (now + LOOKAHEAD) * cps_now;
        if target_cycle > scheduled_cycle {
            let pat = pattern.read().unwrap().clone();
            pending.extend(schedule_window(&pat, cps_now, scheduled_cycle, target_cycle));
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
    // All-notes-off on the channels we touched would be ideal; send a coarse
    // reset on channel 0 so a held note doesn't hang.
    sink.send(&[CONTROL_CHANGE, 123, 0]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::{Frac, note, pure, sequence, silence};

    fn map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
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
            got.iter().any(|m| m.first().map(|b| b & 0xF0) == Some(NOTE_ON)),
            "expected at least one note-on, got {got:?}"
        );
        let _ = (Frac::zero(), silence()); // keep imports tidy across cfgs
    }
}
