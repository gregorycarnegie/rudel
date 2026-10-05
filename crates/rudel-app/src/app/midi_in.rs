// midi_in.rs - what upstream's `MidiInput` (midi/input.mjs) does around a
// `midin` device besides reading it: keep its CC values between sessions
// (`localStorage` there, eframe's storage here), notice it unplugged and reopen
// it when it is back, and send its values back on connect so motorised faders
// and LED rings show where they were left. midir reports no hot-plug events, so
// the port list is polled while a `midin` device is open or waited for.
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::RudelApp;
use eframe::egui;
use std::time::{Duration, Instant};

pub(super) const SAVED_MIDI_CC_KEY: &str = "midin_cc";

/// How often the port list is checked, and how long a device is given to
/// settle before its values are sent back (upstream waits 2 s too).
const PORT_POLL: Duration = Duration::from_secs(2);

/// The CC bus's named-device values as `device\tchannel\tcc\tvalue` lines.
pub(super) fn saved_cc_state() -> String {
    let mut lines = Vec::new();
    for device in rudel_core::cc_devices() {
        for (channel, cc, value) in rudel_core::cc_values_from(&device) {
            lines.push(format!("{device}\t{channel}\t{cc}\t{value}"));
        }
    }
    lines.join("\n")
}

/// Put [`saved_cc_state`]'s values back on the bus; a line that does not read
/// is skipped.
pub(super) fn restore_cc_state(text: &str) {
    for line in text.lines() {
        let mut fields = line.split('\t');
        let (Some(device), Some(channel), Some(cc), Some(value)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if let (Ok(channel), Ok(cc), Ok(value)) = (channel.parse(), cc.parse(), value.parse()) {
            rudel_core::restore_cc(device, channel, cc, value);
        }
    }
}

/// Control changes that put `values` back on a controller, as `_sendAllStates`
/// does: per-channel values only, as the any-channel ones have no channel.
fn feedback_messages(values: &[(u8, u8, f64)]) -> Vec<[u8; 3]> {
    values
        .iter()
        .filter(|&&(channel, ..)| (1..=16).contains(&channel))
        .map(|&(channel, cc, value)| {
            [
                0xB0 | (channel - 1),
                cc & 0x7F,
                (value * 127.0).round().clamp(0.0, 127.0) as u8,
            ]
        })
        .collect()
}

/// Once the device has had time to settle, send `device`'s values to the
/// output port named `port`, if there is one. Failures are only logged, as
/// upstream carries on without them.
fn send_states_later(device: String, port: String) {
    std::thread::spawn(move || {
        std::thread::sleep(PORT_POLL);
        let messages = feedback_messages(&rudel_core::cc_values_from(&device));
        if messages.is_empty()
            || !rudel_midi::MidiOut::list_ports().is_ok_and(|ports| ports.contains(&port))
        {
            return;
        }
        let sent = rudel_midi::MidiOut::connect(Some(&port))
            .and_then(|mut out| messages.iter().try_for_each(|m| out.send(m)));
        if let Err(e) = sent {
            rudel_core::log_line(format!("[midi] could not restore {device}: {e}"));
        }
    });
}

/// Which open devices lost their port, and which waiting ones have one again.
/// `open` is `(device, port name)`; a waiting device matches a port the way
/// `MidiIn::connect` picks one, by case-insensitive substring.
fn port_changes(
    open: &[(&str, &str)],
    waiting: &[String],
    ports: &[String],
) -> (Vec<String>, Vec<String>) {
    let gone = open
        .iter()
        .filter(|(_, port)| !ports.iter().any(|p| p == port))
        .map(|(device, _)| device.to_string())
        .collect();
    let back = waiting
        .iter()
        .filter(|device| {
            let needle = device.trim().to_lowercase();
            ports.iter().any(|p| p.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect();
    (gone, back)
}

impl RudelApp {
    /// A `midin` device opened: send its values back to it shortly.
    pub(super) fn midi_input_opened(&self, device: &str, port: &str) {
        send_states_later(device.to_string(), port.to_string());
    }

    /// Every couple of seconds while a `midin` device is open or waited for,
    /// drop the ones whose port went away and reopen the ones that are back.
    pub(super) fn watch_midi_inputs(&mut self, ctx: &egui::Context) {
        if self.script_midi_ins.is_empty() && self.midi_in_waiting.is_empty() {
            return;
        }
        ctx.request_repaint_after(PORT_POLL);
        if self
            .midi_ports_checked
            .is_some_and(|at| at.elapsed() < PORT_POLL)
        {
            return;
        }
        self.midi_ports_checked = Some(Instant::now());
        let Ok(ports) = rudel_midi::MidiIn::list_ports() else {
            return;
        };
        let open: Vec<(&str, &str)> = self
            .script_midi_ins
            .iter()
            .map(|(device, input)| (device.as_str(), input.port_name()))
            .collect();
        let (gone, back) = port_changes(&open, &self.midi_in_waiting, &ports);
        for device in gone {
            self.script_midi_ins.remove(&device);
            self.log_lines
                .push(format!("[midi] device disconnected: {device}"));
            self.midi_in_waiting.push(device);
        }
        for device in back {
            self.midi_in_waiting.retain(|d| *d != device);
            self.log_lines
                .push(format!("[midi] device reconnected: {device}"));
            self.queue_midi_input(device);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc_state_survives_a_save_and_restore() {
        rudel_core::set_cc_from("midi-in-test", 3, 21, 0.5);
        let saved = saved_cc_state();
        let mine: Vec<&str> = saved
            .lines()
            .filter(|l| l.starts_with("midi-in-test\t"))
            .collect();
        assert_eq!(
            mine,
            ["midi-in-test\t0\t21\t0.5", "midi-in-test\t3\t21\t0.5"]
        );

        restore_cc_state("midi-in-restored\t3\t21\t0.75\nnot a line\nx\ty\tz\tw");
        assert_eq!(rudel_core::get_cc_from("midi-in-restored", 3, 21), 0.75);
    }

    #[test]
    fn values_go_back_as_control_changes_on_their_channel() {
        assert_eq!(
            feedback_messages(&[(0, 7, 1.0), (1, 7, 1.0), (16, 74, 0.5), (17, 1, 1.0)]),
            [[0xB0, 7, 127], [0xBF, 74, 64]],
            "any-channel and out-of-range entries have no channel to send on"
        );
    }

    #[test]
    fn a_vanished_port_is_waited_for_and_a_returning_one_reopened() {
        let ports = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let open = [("nano", "nanoKONTROL2 1"), ("fader", "FaderPort 0")];
        let waiting = ["launch".to_string(), "absent".to_string()];
        let (gone, back) = port_changes(
            &open,
            &waiting,
            &ports(&["nanoKONTROL2 1", "Launchpad X 2"]),
        );
        assert_eq!(gone, ["fader"]);
        assert_eq!(back, ["launch"], "matched like connect, by substring");
    }
}
