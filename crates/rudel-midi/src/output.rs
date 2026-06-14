use crate::note::reset_messages;
use crate::schedule::{MpeState, TimedMidi, schedule_window_with_state};
use crate::{CLOCK, CONTINUE, START, STOP};
use midir::{MidiOutput, MidiOutputConnection};
use rudel_core::Pattern;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
