// rudel-osc - OSC output for Rudel, in the SuperDirt/Tidal `/dirt/play` format.
// Encodes control events as OSC 1.0 messages and sends them over UDP, with a
// real-time scheduler mirroring the MIDI back-end.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{Pattern, Value, note_to_midi, query_controls};
use std::collections::BTreeMap;
use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The default SuperDirt OSC address.
pub const DIRT_PLAY: &str = "/dirt/play";
/// The default SuperDirt UDP port.
pub const SUPERDIRT_PORT: u16 = 57120;

/// A single OSC argument.
#[derive(Clone, Debug, PartialEq)]
pub enum OscArg {
    Int(i32),
    Float(f32),
    Str(String),
}

impl OscArg {
    fn tag(&self) -> u8 {
        match self {
            OscArg::Int(_) => b'i',
            OscArg::Float(_) => b'f',
            OscArg::Str(_) => b's',
        }
    }
}

/// An OSC message (address + arguments).
#[derive(Clone, Debug, PartialEq)]
pub struct OscMessage {
    pub address: String,
    pub args: Vec<OscArg>,
}

fn push_osc_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

impl OscMessage {
    /// Encode as an OSC 1.0 packet (big-endian, 4-byte aligned).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        push_osc_string(&mut buf, &self.address);
        let mut tags = vec![b','];
        for a in &self.args {
            tags.push(a.tag());
        }
        // type-tag string is itself an OSC string
        let tag_str = String::from_utf8(tags).unwrap();
        push_osc_string(&mut buf, &tag_str);
        for a in &self.args {
            match a {
                OscArg::Int(i) => buf.extend_from_slice(&i.to_be_bytes()),
                OscArg::Float(f) => buf.extend_from_slice(&f.to_be_bytes()),
                OscArg::Str(s) => push_osc_string(&mut buf, s),
            }
        }
        buf
    }
}

fn value_to_arg(v: &Value) -> Option<OscArg> {
    match v {
        Value::Int(n) => Some(OscArg::Int(*n as i32)),
        Value::F64(x) => Some(OscArg::Float(*x as f32)),
        Value::Frac(f) => Some(OscArg::Float(f.to_f64() as f32)),
        Value::Bool(b) => Some(OscArg::Int(if *b { 1 } else { 0 })),
        Value::Str(s) => Some(OscArg::Str(s.clone())),
        _ => None,
    }
}

/// Build a SuperDirt `/dirt/play` message from a control map. Prepends
/// `cps`, `cycle` and `delta` (seconds), adds `midinote` for note values, and
/// undoes the `unit: 'c'` speed scaling (as SuperDirt re-applies it).
pub fn superdirt_message(
    controls: &BTreeMap<String, Value>,
    cps: f64,
    cycle: f64,
    delta: f64,
) -> OscMessage {
    let mut map = controls.clone();
    // `oschost`/`oscport` are client-side routing, not SuperDirt synth params.
    map.remove("oschost");
    map.remove("oscport");

    // note -> midinote (number); keep the original note too.
    if let Some(note) = map.get("note") {
        let midi = match note {
            Value::Str(s) => s
                .parse::<f64>()
                .ok()
                .or_else(|| note_to_midi(s).map(|m| m as f64)),
            other => other.as_f64(),
        };
        if let Some(m) = midi {
            map.insert("midinote".to_string(), Value::F64(m));
        }
    }
    // SuperDirt re-applies cps to `unit: 'c'` speeds, so undo it here.
    if matches!(map.get("unit"), Some(Value::Str(u)) if u == "c")
        && let Some(speed) = map.get("speed").and_then(|v| v.as_f64())
    {
        map.insert("speed".to_string(), Value::F64(speed / cps));
    }

    let mut args = vec![
        OscArg::Str("cps".to_string()),
        OscArg::Float(cps as f32),
        OscArg::Str("cycle".to_string()),
        OscArg::Float(cycle as f32),
        OscArg::Str("delta".to_string()),
        OscArg::Float(delta as f32),
    ];
    // Deterministic key order (BTreeMap iterates sorted).
    for (k, v) in &map {
        if let Some(arg) = value_to_arg(v) {
            args.push(OscArg::Str(k.clone()));
            args.push(arg);
        }
    }
    OscMessage {
        address: DIRT_PLAY.to_string(),
        args,
    }
}

/// Resolve a per-event OSC destination from `oschost`/`oscport` controls, or
/// `None` to use the engine's default target. `oschost` defaults to
/// `127.0.0.1`, `oscport` to [`SUPERDIRT_PORT`] when only one is given.
pub fn osc_target(controls: &BTreeMap<String, Value>) -> Option<String> {
    let host = controls.get("oschost").map(|v| match v {
        Value::Str(s) => s.clone(),
        other => other.as_f64().map(|f| f.to_string()).unwrap_or_default(),
    });
    let port = controls
        .get("oscport")
        .and_then(|v| v.as_f64())
        .map(|p| p.round() as u16);
    if host.is_none() && port.is_none() {
        return None;
    }
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.unwrap_or(SUPERDIRT_PORT);
    Some(format!("{host}:{port}"))
}

/// An OSC packet stamped with the time (seconds, engine clock) to send it, and
/// an optional per-event destination (`oschost`/`oscport`).
#[derive(Clone, Debug, PartialEq)]
pub struct TimedOsc {
    pub at_seconds: f64,
    pub message: OscMessage,
    /// Override `host:port` for this packet, or `None` for the engine default.
    pub target: Option<String>,
}

/// Build the OSC messages for every onset in `[begin_cycle, end_cycle)`.
pub fn schedule_window(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
) -> Vec<TimedOsc> {
    let mut out: Vec<TimedOsc> = query_controls(pattern, cps, begin_cycle, end_cycle)
        .into_iter()
        .map(|ev| TimedOsc {
            at_seconds: ev.onset_seconds,
            target: osc_target(&ev.controls),
            message: superdirt_message(&ev.controls, cps, ev.onset_cycle, ev.duration_seconds),
        })
        .collect();
    out.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
    out
}

/// A UDP OSC sender. The socket is left unconnected so each message can be
/// addressed independently (used by per-event `oschost`/`oscport` routing); a
/// default `target` is used when an event doesn't override it.
pub struct OscOut {
    socket: UdpSocket,
    default_target: String,
}

impl OscOut {
    /// Bind an ephemeral local socket with a default target `host:port` (e.g.
    /// `"127.0.0.1:57120"` for a local SuperDirt).
    pub fn connect(target: &str) -> Result<OscOut, String> {
        // Validate the target resolves now so a bad address fails fast, without
        // sending anything (an empty datagram would pollute the stream).
        target
            .to_socket_addrs()
            .map_err(|e| format!("{target}: {e}"))?
            .next()
            .ok_or_else(|| format!("{target}: no address"))?;
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        Ok(OscOut {
            socket,
            default_target: target.to_string(),
        })
    }

    /// Send to the default target.
    pub fn send(&self, msg: &OscMessage) -> Result<(), String> {
        self.send_to(msg, &self.default_target)
    }

    /// Send to an explicit `host:port` target (per-event `oschost`/`oscport`).
    pub fn send_to(&self, msg: &OscMessage, target: &str) -> Result<(), String> {
        self.socket
            .send_to(&msg.encode(), target)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// A running OSC scheduler: queries the pattern ahead of a real-time clock and
/// sends `/dirt/play` messages over UDP.
pub struct OscEngine {
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<Mutex<f64>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OscEngine {
    pub fn start(out: OscOut, pattern: Pattern, cps: f64) -> OscEngine {
        let pattern = Arc::new(RwLock::new(pattern));
        let cps = Arc::new(Mutex::new(cps));
        let running = Arc::new(AtomicBool::new(true));
        let handle = {
            let pattern = pattern.clone();
            let cps = cps.clone();
            let running = running.clone();
            std::thread::spawn(move || run_scheduler(out, pattern, cps, running))
        };
        OscEngine {
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

impl Drop for OscEngine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

const LOOKAHEAD: f64 = 0.1;

fn run_scheduler(
    out: OscOut,
    pattern: Arc<RwLock<Pattern>>,
    cps: Arc<Mutex<f64>>,
    running: Arc<AtomicBool>,
) {
    let start = Instant::now();
    let mut scheduled_cycle = 0.0_f64;
    let mut pending: Vec<TimedOsc> = Vec::new();
    while running.load(Ordering::Relaxed) {
        let cps_now = *cps.lock().unwrap();
        let now = start.elapsed().as_secs_f64();
        let target_cycle = (now + LOOKAHEAD) * cps_now;
        if target_cycle > scheduled_cycle {
            let pat = pattern.read().unwrap().clone();
            pending.extend(schedule_window(
                &pat,
                cps_now,
                scheduled_cycle,
                target_cycle,
            ));
            pending.sort_by(|a, b| a.at_seconds.total_cmp(&b.at_seconds));
            scheduled_cycle = target_cycle;
        }
        let now = start.elapsed().as_secs_f64();
        while pending.first().is_some_and(|m| m.at_seconds <= now) {
            let m = pending.remove(0);
            let _ = match &m.target {
                Some(target) => out.send_to(&m.message, target),
                None => out.send(&m.message),
            };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rudel_core::{note, pure, s, sequence};

    fn osc_string() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[-A-Za-z0-9_ ]{0,12}").unwrap()
    }

    fn osc_arg() -> impl Strategy<Value = OscArg> {
        prop_oneof![
            any::<i32>().prop_map(OscArg::Int),
            (-1_000_000.0f32..=1_000_000.0).prop_map(OscArg::Float),
            osc_string().prop_map(OscArg::Str),
        ]
    }

    fn read_osc_string(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
        let start = offset;
        let mut end = offset;
        while end < bytes.len() && bytes[end] != 0 {
            end += 1;
        }
        if end == bytes.len() {
            return None;
        }
        let value = std::str::from_utf8(&bytes[start..end]).ok()?.to_string();
        end += 1;
        while !end.is_multiple_of(4) {
            if end >= bytes.len() || bytes[end] != 0 {
                return None;
            }
            end += 1;
        }
        Some((value, end))
    }

    #[test]
    fn encodes_address_and_alignment() {
        // "/a" -> 'a' '\0' '\0' '\0' (4 bytes), then ",i" padded to 4, then int
        let msg = OscMessage {
            address: "/a".to_string(),
            args: vec![OscArg::Int(1)],
        };
        let bytes = msg.encode();
        assert_eq!(&bytes[0..4], b"/a\0\0");
        assert_eq!(&bytes[4..8], b",i\0\0");
        assert_eq!(&bytes[8..12], &1i32.to_be_bytes());
        assert_eq!(bytes.len() % 4, 0);
    }

    #[test]
    fn float_and_string_args() {
        let msg = OscMessage {
            address: "/x".to_string(),
            args: vec![OscArg::Float(1.5), OscArg::Str("bd".to_string())],
        };
        let bytes = msg.encode();
        // type tags ",fs" padded to 4
        assert_eq!(&bytes[4..8], b",fs\0");
        // 1.5f32 big-endian
        assert_eq!(&bytes[8..12], &1.5f32.to_be_bytes());
        // "bd\0\0"
        assert_eq!(&bytes[12..16], b"bd\0\0");
    }

    #[test]
    fn superdirt_prefixes_and_midinote() {
        let controls = BTreeMap::from([
            ("s".to_string(), Value::Str("piano".into())),
            ("note".to_string(), Value::Str("a4".into())),
        ]);
        let msg = superdirt_message(&controls, 0.5, 2.0, 1.0);
        assert_eq!(msg.address, DIRT_PLAY);
        // leading cps/cycle/delta
        assert_eq!(msg.args[0], OscArg::Str("cps".into()));
        assert_eq!(msg.args[1], OscArg::Float(0.5));
        assert_eq!(msg.args[2], OscArg::Str("cycle".into()));
        assert_eq!(msg.args[3], OscArg::Float(2.0));
        assert_eq!(msg.args[4], OscArg::Str("delta".into()));
        assert_eq!(msg.args[5], OscArg::Float(1.0));
        // midinote derived from note name a4 = 69
        let pairs: Vec<_> = msg.args.chunks(2).collect();
        assert!(
            pairs
                .iter()
                .any(|c| c[0] == OscArg::Str("midinote".into()) && c[1] == OscArg::Float(69.0))
        );
        assert!(
            pairs
                .iter()
                .any(|c| c[0] == OscArg::Str("s".into()) && c[1] == OscArg::Str("piano".into()))
        );
    }

    #[test]
    fn oscport_and_oschost_resolve_a_target_and_are_stripped() {
        // Only a port -> default host; both -> host:port; neither -> None.
        assert_eq!(
            osc_target(&BTreeMap::from([("oscport".to_string(), Value::Int(9000))])),
            Some("127.0.0.1:9000".to_string())
        );
        assert_eq!(
            osc_target(&BTreeMap::from([
                ("oschost".to_string(), Value::Str("10.0.0.2".into())),
                ("oscport".to_string(), Value::Int(7000)),
            ])),
            Some("10.0.0.2:7000".to_string())
        );
        assert_eq!(
            osc_target(&BTreeMap::from([(
                "s".to_string(),
                Value::Str("bd".into())
            )])),
            None
        );
        // The routing keys are not emitted as SuperDirt params.
        let msg = superdirt_message(
            &BTreeMap::from([
                ("s".to_string(), Value::Str("bd".into())),
                ("oscport".to_string(), Value::Int(9000)),
                ("oschost".to_string(), Value::Str("10.0.0.2".into())),
            ]),
            0.5,
            0.0,
            1.0,
        );
        assert!(
            !msg.args
                .iter()
                .any(|a| *a == OscArg::Str("oscport".into()) || *a == OscArg::Str("oschost".into()))
        );
    }

    #[test]
    fn schedule_window_carries_per_event_target() {
        let pat = s(pure(Value::Str("bd".into())))
            .ctrl("oscport", pure(Value::Int(9000)))
            .ctrl("oschost", pure(Value::Str("10.0.0.2".into())));
        let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
        assert_eq!(msgs[0].target, Some("10.0.0.2:9000".to_string()));
    }

    #[test]
    fn send_to_routes_to_an_explicit_port() {
        // Two receivers; send_to the second and confirm it (not the default) gets it.
        let default_recv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let alt_recv = UdpSocket::bind("127.0.0.1:0").unwrap();
        alt_recv
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let out = OscOut::connect(&default_recv.local_addr().unwrap().to_string()).unwrap();
        let msg = superdirt_message(
            &BTreeMap::from([("note".to_string(), Value::Int(60))]),
            0.5,
            0.0,
            1.0,
        );
        out.send_to(&msg, &alt_recv.local_addr().unwrap().to_string())
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = alt_recv
            .recv(&mut buf)
            .expect("alt receiver got the packet");
        assert_eq!(&buf[..n], msg.encode().as_slice());
    }

    #[test]
    fn schedule_window_orders_events() {
        let pat = s(sequence(&[
            pure(Value::Str("bd".into())),
            pure(Value::Str("sd".into())),
        ]));
        let msgs = schedule_window(&pat, 1.0, 0.0, 1.0);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].at_seconds, 0.0);
        assert!((msgs[1].at_seconds - 0.5).abs() < 1e-9);
    }

    #[test]
    fn sends_over_udp_loopback() {
        // Bind a receiver, point an OscOut at it, send a message and read it back.
        let recv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = recv.local_addr().unwrap().to_string();
        let out = OscOut::connect(&addr).unwrap();
        let msg = superdirt_message(
            &BTreeMap::from([("note".to_string(), Value::Int(60))]),
            0.5,
            0.0,
            1.0,
        );
        out.send(&msg).unwrap();
        let mut buf = [0u8; 1024];
        recv.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let n = recv.recv(&mut buf).expect("received a packet");
        assert_eq!(&buf[..n], msg.encode().as_slice());
        assert_eq!(&buf[0..12], b"/dirt/play\0\0");
    }

    #[test]
    fn engine_sends_to_a_local_listener() {
        let recv = UdpSocket::bind("127.0.0.1:0").unwrap();
        recv.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let addr = recv.local_addr().unwrap().to_string();
        let out = OscOut::connect(&addr).unwrap();
        let pat = note(pure(Value::Int(60)));
        let engine = OscEngine::start(out, pat, 4.0);
        let mut buf = [0u8; 1024];
        let got = recv.recv(&mut buf);
        engine.stop();
        drop(engine);
        assert!(got.is_ok(), "engine should send at least one OSC packet");
    }

    proptest! {
        #[test]
        fn encoded_messages_round_trip_generated_args(
            address_tail in "[A-Za-z0-9_/]{1,16}",
            args in prop::collection::vec(osc_arg(), 0..12),
        ) {
            let msg = OscMessage {
                address: format!("/{address_tail}"),
                args,
            };
            let bytes = msg.encode();

            prop_assert_eq!(bytes.len() % 4, 0);

            let (address, mut offset) = read_osc_string(&bytes, 0)
                .expect("encoded OSC address string");
            prop_assert_eq!(address, msg.address);

            let (tags, next) = read_osc_string(&bytes, offset)
                .expect("encoded OSC type tag string");
            offset = next;
            let expected_tags: String = std::iter::once(',')
                .chain(msg.args.iter().map(|arg| arg.tag() as char))
                .collect();
            prop_assert_eq!(tags, expected_tags);

            for arg in &msg.args {
                match arg {
                    OscArg::Int(expected) => {
                        prop_assert!(offset + 4 <= bytes.len());
                        let got = i32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
                        prop_assert_eq!(got, *expected);
                        offset += 4;
                    }
                    OscArg::Float(expected) => {
                        prop_assert!(offset + 4 <= bytes.len());
                        let got = f32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
                        prop_assert_eq!(got, *expected);
                        offset += 4;
                    }
                    OscArg::Str(expected) => {
                        let (got, next) = read_osc_string(&bytes, offset)
                            .expect("encoded OSC string argument");
                        prop_assert_eq!(got, expected.as_str());
                        offset = next;
                    }
                }
            }

            prop_assert_eq!(offset, bytes.len());
        }
    }
}
