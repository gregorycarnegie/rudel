use crate::{
    CLOCK, CONTINUE, NOTE_OFF, NOTE_ON, START, STOP,
    note::reset_messages,
    schedule::{MpeState, TimedMidi, schedule_window_with_state},
};
use midir::{MidiOutput, MidiOutputConnection};
use rudel_core::{Clock, Pattern};
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Anything that can receive raw MIDI bytes. Implemented by [`MidiOut`]; a
/// recording sink is used in tests.
pub trait MidiSink: Send {
    fn send(&mut self, bytes: &[u8]);
}

/// A connection to a MIDI output port.
pub struct MidiOut {
    /// `None` only while dropping; see [`crate::port_api_lock`].
    conn: Option<MidiOutputConnection>,
}

impl MidiOut {
    /// List the names of the available MIDI output ports.
    pub fn list_ports() -> Result<Vec<String>, String> {
        let _guard = crate::port_api_lock();
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
        let _guard = crate::port_api_lock();
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
        Ok(MidiOut { conn: Some(conn) })
    }

    /// Present for the whole life of the value; taken only by `Drop`.
    fn conn(&mut self) -> &mut MidiOutputConnection {
        self.conn.as_mut().expect("connection open")
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.conn().send(bytes).map_err(|e| e.to_string())
    }

    /// Send a MIDI clock tick (`0xF8`); 24 per quarter note by convention.
    pub fn clock(&mut self) {
        let _ = self.conn().send(&[CLOCK]);
    }

    pub fn transport_start(&mut self) {
        let _ = self.conn().send(&[START]);
    }

    pub fn transport_continue(&mut self) {
        let _ = self.conn().send(&[CONTINUE]);
    }

    pub fn transport_stop(&mut self) {
        let _ = self.conn().send(&[STOP]);
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

/// Whether `m` is a channel message of kind `status` (`NOTE_ON`, `NOTE_OFF`).
fn status_is(m: &TimedMidi, status: u8) -> bool {
    m.data.first().is_some_and(|s| s & 0xF0 == status)
}

/// The (channel, note) a note message is about, which is what pairs an off
/// with the on it ends.
fn voice(m: &TimedMidi) -> Option<[u8; 2]> {
    Some([m.data.first()? & 0x0F, *m.data.get(1)?])
}

fn run_scheduler<S: MidiSink>(
    mut sink: S,
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<Mutex<f64>>,
    running: Arc<AtomicBool>,
) {
    let start = Instant::now();
    let mut clock = Clock::new(*cps.lock().unwrap());
    let mut scheduled_cycle = 0.0_f64;
    let mut pending: Vec<TimedMidi> = Vec::new();
    let mut mpe_state = MpeState::new();
    while running.load(Ordering::Relaxed) {
        let cps_set = *cps.lock().unwrap();
        let now = start.elapsed().as_secs_f64();
        if cps_set != clock.cps() {
            // Re-anchor rather than rescale from the origin: without this a
            // slower cps pushes the target cycle *behind* what is already
            // scheduled (silence until the clock catches up) and a faster one
            // jumps it forward (a burst of events at once).
            clock.set_cps(now, cps_set);
            // Everything still queued was timed at the old rate, so it is
            // dropped and re-queried — except the note-offs, which belong to
            // notes that are already sounding. Dropping those leaves them
            // stuck on, and nothing re-queries them: the notes they end were
            // scheduled before the change.
            // ponytail: `mpe_state`'s channel reservations keep their old-rate
            // end times for one window; give it a rebase if that ever audibly
            // steals a channel.
            // ...but only the ones whose note actually started: a note-off
            // still paired with a queued note-on would land on whatever the
            // new schedule puts at that pitch instead, cutting it short. The
            // note it was for never sounded, so nothing is left hanging.
            let unplayed: Vec<[u8; 2]> = pending
                .iter()
                .filter(|m| m.at_seconds > now && status_is(m, NOTE_ON))
                .filter_map(voice)
                .collect();
            pending.retain(|m| {
                m.at_seconds <= now
                    || (status_is(m, NOTE_OFF) && !voice(m).is_some_and(|v| unplayed.contains(&v)))
            });
            scheduled_cycle = clock.cycle_at(now);
        }
        let cps_now = clock.cps();
        let target_cycle = clock.cycle_at(now + LOOKAHEAD);
        if target_cycle > scheduled_cycle {
            let pat = pattern.read().unwrap().clone();
            // `schedule_window_with_state` times events as `cycle / cps`, i.e.
            // from the origin; shifting by the anchor's origin time puts them
            // back on this thread's `start` clock.
            let shift = clock.seconds_at(0.0);
            pending.extend(
                schedule_window_with_state(
                    &pat,
                    cps_now,
                    scheduled_cycle,
                    target_cycle,
                    &mut mpe_state,
                )
                .into_iter()
                .map(|mut m| {
                    m.at_seconds += shift;
                    m
                }),
            );
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

impl Drop for MidiOut {
    fn drop(&mut self) {
        // Closing has to hold the lock too — dropping the field after this
        // function returns would close it outside the lock.
        let _guard = crate::port_api_lock();
        drop(self.conn.take());
    }
}
