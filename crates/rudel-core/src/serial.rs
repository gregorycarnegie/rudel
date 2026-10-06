// serial.rs - `pat.serial(...)`, ported from strudel/packages/serial/serial.mjs.
//
// Upstream's `onTrigger` formats the hap and writes it to a Web Serial port 0.1 s
// after the hap's time. Here the formatted bytes travel as a control, as
// `speak` does, and the host writes them from its trigger loop.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::Pattern,
    value::{Value, ValueMap},
};

/// The control a serial hap carries: `{ bytes, baud, name }`.
pub const SERIAL: &str = "_serial";

/// What a serial hap asks the host to write.
#[derive(Clone, Debug, PartialEq)]
pub struct SerialWrite {
    /// The port key the script named (`'default'` unless given).
    pub name: String,
    pub baud: u32,
    pub bytes: Vec<u8>,
}

/// JS template-literal stringification of a hap value.
fn js_string(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => x.to_string(),
        Value::Frac(f) => f.to_f64().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::List(l) => l.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Map(_) => "[object Object]".to_string(),
        _ => "undefined".to_string(),
    }
}

/// CRC-16/CCITT-FALSE over the message's UTF-16 code units, as upstream's
/// `charCodeAt` loop computes it.
fn crc16(message: &str) -> u16 {
    let mut crc: u32 = 0xffff;
    for unit in message.encode_utf16() {
        crc ^= u32::from(unit) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    (crc & 0xffff) as u16
}

/// The bytes upstream's `onTrigger` writes for one hap value.
fn encode(value: &Value, crc: bool, single_char_ids: bool) -> Vec<u8> {
    let first = |s: &str| s.chars().next().map(String::from).unwrap_or_default();
    let (message, chk) = match value {
        Value::Map(m) => match m.get("action") {
            Some(action) => {
                let action = js_string(action);
                let action = if single_char_ids {
                    first(&action)
                } else {
                    action
                };
                let args: Vec<String> = m
                    .iter()
                    .filter(|(k, _)| *k != "action")
                    .map(|(k, v)| {
                        let k = if single_char_ids { first(k) } else { k.clone() };
                        format!("{k}:{}", js_string(v))
                    })
                    .collect();
                let message = format!("{action}({})", args.join(","));
                let chk = if crc { crc16(&message) } else { 0 };
                (message, chk)
            }
            None => (
                m.iter()
                    .map(|(k, v)| format!("{k}:{}", js_string(v)))
                    .collect(),
                0,
            ),
        },
        other => (js_string(other), 0),
    };
    let mut bytes = message.into_bytes();
    // A checksum of 0 is falsy upstream, so it is not sent.
    if chk != 0 {
        bytes.extend([b'|', (chk >> 8) as u8, chk as u8, b';']);
    }
    bytes
}

impl Pattern {
    /// `serial(br, sendcrc, singlecharids, name)`: write each hap to a serial
    /// port instead of playing it (upstream's trigger is dominant).
    pub fn serial(&self, baud: u32, crc: bool, single_char_ids: bool, name: &str) -> Pattern {
        let name = name.to_string();
        self.fmap(move |v| {
            let bytes = encode(&v, crc, single_char_ids);
            let mut map = match v {
                Value::Map(m) => m,
                other => ValueMap::from([("value".to_string(), other)]),
            };
            let request = ValueMap::from([
                ("name".to_string(), Value::Str(name.clone())),
                ("baud".to_string(), Value::Int(i64::from(baud))),
                (
                    "bytes".to_string(),
                    Value::List(bytes.into_iter().map(|b| Value::Int(b.into())).collect()),
                ),
            ]);
            map.insert(SERIAL.to_string(), Value::Map(request));
            Value::Map(map)
        })
    }
}

/// Whether this hap is a serial write rather than a sound.
pub fn is_serial(value: &Value) -> bool {
    matches!(value, Value::Map(m) if m.contains_key(SERIAL))
}

/// The write a hap asks for, if it is a serial hap.
pub fn request(value: &Value) -> Option<SerialWrite> {
    let Value::Map(m) = value else { return None };
    let Value::Map(r) = m.get(SERIAL)? else {
        return None;
    };
    Some(SerialWrite {
        name: r.get("name")?.as_str()?.to_string(),
        baud: r.get("baud")?.as_f64()? as u32,
        bytes: match r.get("bytes")? {
            Value::List(l) => l.iter().filter_map(|b| Some(b.as_f64()? as u8)).collect(),
            _ => return None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Value)]) -> Value {
        Value::Map(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn formats_as_upstream() {
        let plain = map(&[("x", Value::Int(1)), ("y", Value::F64(0.5))]);
        assert_eq!(
            encode(&plain, true, false),
            b"x:1y:0.5",
            "no separator, no crc"
        );
        let action = map(&[
            ("action", Value::Str("move".into())),
            ("speed", Value::Int(3)),
            ("dir", Value::Str("left".into())),
        ]);
        assert_eq!(encode(&action, false, false), b"move(speed:3,dir:left)");
        assert_eq!(encode(&action, false, true), b"m(s:3,d:left)");
        assert_eq!(encode(&Value::Str("hi".into()), true, false), b"hi");
    }

    #[test]
    fn appends_the_ccitt_false_checksum() {
        // The standard check value for CRC-16/CCITT-FALSE.
        assert_eq!(crc16("123456789"), 0x29b1);
        let action = map(&[("action", Value::Str("a".into()))]);
        let chk = crc16("a()");
        let mut want = b"a()".to_vec();
        want.extend([b'|', (chk >> 8) as u8, chk as u8, b';']);
        assert_eq!(encode(&action, true, false), want);
    }

    #[test]
    fn tags_haps_with_the_write() {
        let p = crate::pure(Value::Str("go".into())).serial(9600, false, false, "default");
        let hap = p
            .query_arc(crate::Frac::zero(), crate::Frac::one())
            .remove(0);
        assert!(is_serial(&hap.value));
        assert_eq!(
            request(&hap.value),
            Some(SerialWrite {
                name: "default".into(),
                baud: 9600,
                bytes: b"go".to_vec()
            })
        );
    }
}
