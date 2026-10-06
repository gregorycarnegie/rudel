// mqtt.rs - the broker `.mqtt()` publishes to (`@strudel/mqtt`).
//
// Upstream uses Paho's MQTT-over-WebSockets client. This is the part of MQTT
// 3.1.1 that needs: CONNECT, QoS 0 PUBLISH and PINGREQ, in binary WebSocket
// frames with the `mqtt` subprotocol, so `ws://` and `wss://` brokers work as
// they do in the browser.
// SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::mqtt::MqttPublish;
use std::{
    collections::HashMap,
    net::{TcpStream, ToSocketAddrs},
    sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    time::{Duration, Instant, SystemTime},
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, http::HeaderValue, stream::MaybeTlsStream,
};

/// Paho's default keep-alive, in seconds.
const KEEP_ALIVE: u16 = 60;
/// How long a broker that failed is left before trying again.
const RETRY: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(5);

/// The publisher thread, started on the first message.
#[derive(Default)]
pub(crate) struct MqttOut {
    tx: Option<Sender<(Instant, MqttPublish)>>,
}

impl MqttOut {
    pub(crate) fn send(&mut self, msg: MqttPublish) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = channel();
            let _ = std::thread::Builder::new()
                .name("mqtt".into())
                .spawn(move || run(rx));
            tx
        });
        let at = Instant::now() + Duration::from_secs_f64(msg.latency.max(0.0));
        let _ = tx.send((at, msg));
    }
}

/// A UTF-8 string or binary field: a big-endian length, then the bytes.
fn field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend((bytes.len() as u16).to_be_bytes());
    buf.extend(bytes);
}

/// A control packet: its type byte, the remaining length (a varint), the body.
fn packet(kind: u8, body: Vec<u8>) -> Vec<u8> {
    let mut out = vec![kind];
    let mut n = body.len();
    loop {
        let byte = (n % 128) as u8;
        n /= 128;
        out.push(if n > 0 { byte | 0x80 } else { byte });
        if n == 0 {
            break;
        }
    }
    out.extend(body);
    out
}

fn connect_packet(client: &str, username: Option<&str>, password: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
    field(&mut body, b"MQTT");
    body.push(4); // 3.1.1
    let mut flags = 0x02; // clean session
    if username.is_some() {
        flags |= 0x80;
        if password.is_some() {
            flags |= 0x40;
        }
    }
    body.push(flags);
    body.extend(KEEP_ALIVE.to_be_bytes());
    field(&mut body, client.as_bytes());
    if let Some(user) = username {
        field(&mut body, user.as_bytes());
        if let Some(pass) = password {
            field(&mut body, pass.as_bytes());
        }
    }
    packet(0x10, body)
}

fn publish_packet(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    field(&mut body, topic.as_bytes());
    body.extend(payload);
    packet(0x30, body)
}

const PINGREQ: [u8; 2] = [0xc0, 0];

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn open(msg: &MqttPublish) -> Result<Socket, String> {
    let mut request = msg
        .host
        .as_str()
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("mqtt"));
    let uri = request.uri();
    let host = uri.host().ok_or("no host in the broker URL")?;
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("the broker's host did not resolve")?;
    let tcp = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let (mut ws, _) = tungstenite::client_tls(request, tcp).map_err(|e| e.to_string())?;
    let client = msg.client.clone().unwrap_or_else(|| {
        let nanos = SystemTime::UNIX_EPOCH
            .elapsed()
            .map_or(0, |d| d.subsec_nanos());
        format!("strudel-{}", nanos % 1_000_000)
    });
    let hello = connect_packet(&client, msg.username.as_deref(), msg.password.as_deref());
    ws.send(Message::Binary(hello.into()))
        .map_err(|e| e.to_string())?;
    match ws.read().map_err(|e| e.to_string())? {
        Message::Binary(ack) if ack.len() >= 4 && ack[0] == 0x20 => match ack[3] {
            0 => Ok(ws),
            code => Err(format!("the broker refused the connection (code {code})")),
        },
        _ => Err("the broker did not acknowledge the connection".to_string()),
    }
}

fn run(rx: Receiver<(Instant, MqttPublish)>) {
    // Each `host-client` key's connection (upstream's key), or when it last
    // failed.
    // ponytail: nothing is read after the CONNACK, so PINGRESPs pile up unread
    // (two bytes a ping); read them if a broker ever minds.
    let mut conns: HashMap<String, Result<(Socket, Instant), Instant>> = HashMap::new();
    let ping_every = Duration::from_secs(u64::from(KEEP_ALIVE) / 2);
    loop {
        match rx.recv_timeout(ping_every) {
            Ok((at, msg)) => {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                // Upstream drops what triggers before it has connected; don't
                // send a burst of stale messages after a slow connect.
                if at.elapsed() > Duration::from_secs(1) {
                    continue;
                }
                let key = format!(
                    "{}-{}",
                    msg.host,
                    msg.client.as_deref().unwrap_or("undefined")
                );
                let slot = conns
                    .entry(key)
                    .or_insert_with(|| Err(Instant::now() - RETRY));
                if let Err(failed) = slot
                    && failed.elapsed() >= RETRY
                {
                    *slot = match open(&msg) {
                        Ok(ws) => {
                            rudel_core::log_line(format!("mqtt: connected to {}", msg.host));
                            Ok((ws, Instant::now()))
                        }
                        Err(e) => {
                            rudel_core::log_line(format!("mqtt: {}: {e}", msg.host));
                            Err(Instant::now())
                        }
                    };
                }
                if let Ok((ws, last)) = slot {
                    let publish = publish_packet(&msg.topic, &msg.payload);
                    match ws.send(Message::Binary(publish.into())) {
                        Ok(()) => *last = Instant::now(),
                        Err(e) => {
                            rudel_core::log_line(format!("mqtt connection lost: {e}"));
                            *slot = Err(Instant::now());
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for slot in conns.values_mut() {
            if let Ok((ws, last)) = slot
                && last.elapsed() >= ping_every
            {
                match ws.send(Message::Binary(PINGREQ.to_vec().into())) {
                    Ok(()) => *last = Instant::now(),
                    Err(_) => *slot = Err(Instant::now()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::handshake::server::{Request, Response};

    #[test]
    fn packets_are_mqtt_3_1_1() {
        assert_eq!(
            connect_packet("c", Some("u"), Some("p")),
            [
                0x10, 19, 0, 4, b'M', b'Q', b'T', b'T', 4, 0xc2, 0, 60, 0, 1, b'c', 0, 1, b'u', 0,
                1, b'p'
            ]
        );
        assert_eq!(
            publish_packet("t", b"hi"),
            [0x30, 5, 0, 1, b't', b'h', b'i']
        );
        // The remaining length is a varint past 127.
        let long = publish_packet("t", &[0; 200]);
        assert_eq!(long[..3], [0x30, 0xcb, 0x01]);
        assert_eq!(long.len(), 3 + 203);
    }

    /// A broker on loopback that checks the CONNECT and records a PUBLISH: the
    /// path the app takes, over a plain `ws://` socket.
    #[test]
    // tungstenite's handshake callback signature returns the large error.
    #[allow(clippy::result_large_err)]
    fn publishes_to_a_websocket_broker() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let broker = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut protocol = None;
            let callback = |req: &Request, mut res: Response| {
                protocol = req.headers().get("Sec-WebSocket-Protocol").cloned();
                res.headers_mut()
                    .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("mqtt"));
                Ok(res)
            };
            let mut ws = tungstenite::accept_hdr(tcp, callback).unwrap();
            let connect = ws.read().unwrap().into_data();
            ws.send(Message::Binary(vec![0x20, 2, 0, 0].into()))
                .unwrap();
            let publish = ws.read().unwrap().into_data();
            (protocol, connect[0], publish.to_vec())
        });
        let msg = MqttPublish {
            host: format!("ws://127.0.0.1:{port}/"),
            client: Some("me".into()),
            username: None,
            password: None,
            topic: "t".into(),
            payload: b"{}".to_vec(),
            latency: 0.0,
        };
        let mut ws = open(&msg).unwrap();
        ws.send(Message::Binary(
            publish_packet(&msg.topic, &msg.payload).into(),
        ))
        .unwrap();
        let (protocol, connect, publish) = broker.join().unwrap();
        assert_eq!(protocol.as_ref().map(|p| p.to_str().unwrap()), Some("mqtt"));
        assert_eq!(connect, 0x10);
        assert_eq!(publish, publish_packet("t", b"{}"));
    }
}
