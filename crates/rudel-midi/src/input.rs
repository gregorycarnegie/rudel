use crate::{CLOCK, CONTINUE, CONTROL_CHANGE, START, STOP};
use midir::{Ignore, MidiInput, MidiInputConnection};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

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
