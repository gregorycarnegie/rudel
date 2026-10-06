// serial.rs - the port `.serial()` writes to (`@strudel/serial`).
//
// Upstream asks the browser for a port (`navigator.serial.requestPort()`, a
// picker) and writes each message 0.1 s after its hap's time. There is no
// picker here: a `name` that is a port (`'COM3'`, `'/dev/ttyACM0'`) opens that
// port, and any other name the first USB serial port, else the first port.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::serial::SerialWrite;
use serialport::{SerialPort, SerialPortInfo, SerialPortType};
use std::{
    collections::HashMap,
    io::Write,
    sync::mpsc::{Receiver, Sender, channel},
    time::{Duration, Instant},
};

/// upstream's `latency`: how long after its hap a message is written.
const LATENCY: Duration = Duration::from_millis(100);
/// How long a port that failed to open is left before trying again.
const RETRY: Duration = Duration::from_secs(2);

/// The writer thread, started on the first write.
#[derive(Default)]
pub(crate) struct SerialOut {
    tx: Option<Sender<(Instant, SerialWrite)>>,
}

impl SerialOut {
    pub(crate) fn send(&mut self, write: SerialWrite) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = channel();
            let _ = std::thread::Builder::new()
                .name("serial".into())
                .spawn(move || run(rx));
            tx
        });
        let _ = tx.send((Instant::now() + LATENCY, write));
    }
}

/// The port a name picks among those present.
fn pick<'a>(name: &str, ports: &'a [SerialPortInfo]) -> Option<&'a SerialPortInfo> {
    ports
        .iter()
        .find(|p| p.port_name.eq_ignore_ascii_case(name))
        .or_else(|| {
            ports
                .iter()
                .find(|p| matches!(p.port_type, SerialPortType::UsbPort(_)))
        })
        .or(ports.first())
}

fn open(write: &SerialWrite) -> Result<Box<dyn SerialPort>, String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    let info = pick(&write.name, &ports).ok_or("no serial port found")?;
    let port = serialport::new(&info.port_name, write.baud)
        .open()
        .map_err(|e| format!("{}: {e}", info.port_name))?;
    rudel_core::log_line(format!(
        "serial: opened {} at {} baud",
        info.port_name, write.baud
    ));
    Ok(port)
}

fn run(rx: Receiver<(Instant, SerialWrite)>) {
    // Each name's port, or when it last failed to open.
    let mut ports: HashMap<String, Result<Box<dyn SerialPort>, Instant>> = HashMap::new();
    for (at, write) in rx {
        std::thread::sleep(at.saturating_duration_since(Instant::now()));
        let slot = ports
            .entry(write.name.clone())
            .or_insert_with(|| Err(Instant::now() - RETRY));
        if let Err(failed) = slot
            && failed.elapsed() >= RETRY
        {
            *slot = open(&write).map_err(|e| {
                rudel_core::log_line(format!("serial: {e}"));
                Instant::now()
            });
        }
        if let Ok(port) = slot
            && let Err(e) = port.write_all(&write.bytes)
        {
            rudel_core::log_line(format!("serial: {e}"));
            *slot = Err(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, usb: bool) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.to_string(),
            port_type: if usb {
                SerialPortType::UsbPort(serialport::UsbPortInfo {
                    vid: 0x2341,
                    pid: 0x0043,
                    serial_number: None,
                    manufacturer: None,
                    product: None,
                })
            } else {
                SerialPortType::Unknown
            },
        }
    }

    #[test]
    fn a_name_picks_its_port_else_the_first_usb_one() {
        let ports = [port("COM1", false), port("COM3", true), port("COM4", true)];
        assert_eq!(pick("com4", &ports).unwrap().port_name, "COM4");
        assert_eq!(pick("default", &ports).unwrap().port_name, "COM3");
        assert_eq!(pick("default", &ports[..1]).unwrap().port_name, "COM1");
        assert!(pick("default", &[]).is_none());
    }
}
