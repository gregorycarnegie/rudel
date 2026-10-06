// mqtt.rs - `pat.mqtt(...)`, ported from strudel/packages/mqtt/mqtt.mjs.
//
// Upstream's `onTrigger` publishes each hap to an MQTT broker over WebSockets,
// `latency` seconds after the hap's time. Here the options travel as a control,
// as `speak` and `serial` do, and the host builds the message as each hap
// triggers (it needs the tempo for the `duration`/`cps` metadata).
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::Pattern,
    value::{Value, ValueMap},
};

/// The control an MQTT hap carries: its `.mqtt(...)` options.
pub const MQTT: &str = "_mqtt";

/// `.mqtt(username, password, topic, host, client, latency, add_meta)`.
#[derive(Clone, Debug, PartialEq)]
pub struct MqttOptions {
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic: Option<String>,
    pub host: String,
    pub client: Option<String>,
    pub latency: f64,
    pub add_meta: bool,
}

impl Default for MqttOptions {
    fn default() -> MqttOptions {
        MqttOptions {
            username: None,
            password: None,
            topic: None,
            host: "wss://localhost:8883/".to_string(),
            client: None,
            latency: 0.0,
            add_meta: true,
        }
    }
}

/// One message to publish.
#[derive(Clone, Debug, PartialEq)]
pub struct MqttPublish {
    pub host: String,
    /// The client id the script gave, if any (upstream makes up
    /// `strudel-<n>` otherwise).
    pub client: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topic: String,
    pub payload: Vec<u8>,
    /// Seconds after the hap's time to send it.
    pub latency: f64,
}

fn opt_str(s: &Option<String>) -> Value {
    s.as_ref().map_or(Value::Null, |s| Value::Str(s.clone()))
}

fn get_str(m: &ValueMap, key: &str) -> Option<String> {
    m.get(key).and_then(Value::as_str).map(str::to_string)
}

impl Pattern {
    /// Publish each hap to an MQTT broker instead of playing it (upstream's
    /// trigger is dominant).
    pub fn mqtt(&self, options: MqttOptions) -> Pattern {
        let tag = ValueMap::from([
            ("username".to_string(), opt_str(&options.username)),
            ("password".to_string(), opt_str(&options.password)),
            ("topic".to_string(), opt_str(&options.topic)),
            ("host".to_string(), Value::Str(options.host)),
            ("client".to_string(), opt_str(&options.client)),
            ("latency".to_string(), Value::F64(options.latency)),
            ("add_meta".to_string(), Value::Bool(options.add_meta)),
        ]);
        self.fmap(move |v| {
            let mut tag = tag.clone();
            let mut map = match v {
                Value::Map(m) => m,
                // A hap that is not an object is sent as it is.
                other => {
                    tag.insert("scalar".to_string(), other);
                    ValueMap::new()
                }
            };
            map.insert(MQTT.to_string(), Value::Map(tag));
            Value::Map(map)
        })
    }
}

/// Whether this hap is an MQTT message rather than a sound.
pub fn is_mqtt(value: &Value) -> bool {
    matches!(value, Value::Map(m) if m.contains_key(MQTT))
}

/// The message a hap publishes when it triggers, lasting `duration` cycles at
/// `cps`. `None` when it is not an MQTT hap, or has no topic (upstream's
/// `send` throws then).
pub fn publish(value: &Value, duration: f64, cps: f64) -> Option<MqttPublish> {
    let Value::Map(m) = value else { return None };
    let Value::Map(opts) = m.get(MQTT)? else {
        return None;
    };
    let mut topic = get_str(opts, "topic");
    let payload = match opts.get("scalar") {
        Some(Value::Str(s)) => s.clone(),
        Some(other) => json(other),
        None => {
            let mut value = m.clone();
            value.shift_remove(MQTT);
            if topic.is_none()
                && let Some(t) = value.get("topic")
            {
                let t = match t {
                    Value::List(parts) => parts.iter().map(json_bare).collect::<Vec<_>>().join("/"),
                    t => json_bare(t),
                };
                topic = Some(format!("/{t}"));
            }
            if matches!(opts.get("add_meta"), Some(Value::Bool(true))) {
                value.insert("duration".to_string(), Value::F64(duration / cps));
                value.insert("cps".to_string(), Value::F64(cps));
            }
            json(&Value::Map(value))
        }
    };
    Some(MqttPublish {
        host: get_str(opts, "host")?,
        client: get_str(opts, "client"),
        username: get_str(opts, "username"),
        password: get_str(opts, "password"),
        topic: topic?,
        payload: payload.into_bytes(),
        latency: opts.get("latency").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

/// A value as JS's `String(v)` has it (a string unquoted).
fn json_bare(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        v => json(v),
    }
}

fn js_number(x: f64) -> String {
    if !x.is_finite() {
        "null".to_string()
    } else if x == x.trunc() && x.abs() < 1e21 {
        format!("{}", x as i128)
    } else {
        x.to_string()
    }
}

/// `JSON.stringify`, keys in insertion order.
pub fn json(v: &Value) -> String {
    match v {
        Value::Null | Value::Func(_) | Value::Pat(_) => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => js_number(*x),
        Value::Frac(f) => js_number(f.to_f64()),
        Value::Str(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::List(l) => format!("[{}]", l.iter().map(json).collect::<Vec<_>>().join(",")),
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", json(&Value::Str(k.clone())), json(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frac;

    fn first(p: &Pattern) -> Value {
        p.query_arc(Frac::zero(), Frac::one()).remove(0).value
    }

    fn map(pairs: &[(&str, Value)]) -> Value {
        Value::Map(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn publishes_the_hap_as_json_with_its_timing() {
        let opts = MqttOptions {
            topic: Some("strudel".into()),
            ..Default::default()
        };
        let hap = first(
            &crate::pure(map(&[
                ("s", Value::Str("bd".into())),
                ("gain", Value::F64(0.5)),
            ]))
            .mqtt(opts),
        );
        assert!(is_mqtt(&hap));
        let msg = publish(&hap, 0.5, 2.0).unwrap();
        assert_eq!(msg.topic, "strudel");
        assert_eq!(msg.host, "wss://localhost:8883/");
        assert_eq!(
            String::from_utf8(msg.payload).unwrap(),
            r#"{"s":"bd","gain":0.5,"duration":0.25,"cps":2}"#
        );
    }

    #[test]
    fn takes_the_topic_from_the_hap_when_none_is_given() {
        let opts = MqttOptions {
            add_meta: false,
            ..Default::default()
        };
        let topic = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]);
        let hap =
            first(&crate::pure(map(&[("topic", topic), ("x", Value::Int(1))])).mqtt(opts.clone()));
        let msg = publish(&hap, 1.0, 1.0).unwrap();
        assert_eq!(msg.topic, "/a/b");
        assert_eq!(
            String::from_utf8(msg.payload).unwrap(),
            r#"{"topic":["a","b"],"x":1}"#
        );
        // No topic at all: nothing to send to.
        let hap = first(&crate::pure(map(&[("x", Value::Int(1))])).mqtt(opts));
        assert_eq!(publish(&hap, 1.0, 1.0), None);
    }

    #[test]
    fn a_plain_value_is_sent_as_it_is() {
        let opts = MqttOptions {
            topic: Some("t".into()),
            ..Default::default()
        };
        let hap = first(&crate::pure(Value::Str("on".into())).mqtt(opts));
        assert_eq!(publish(&hap, 1.0, 1.0).unwrap().payload, b"on");
    }

    #[test]
    fn json_is_javascripts() {
        assert_eq!(json(&Value::F64(1.0)), "1");
        assert_eq!(json(&Value::F64(f64::NAN)), "null");
        assert_eq!(json(&Value::Str("a\"\n\u{1}".into())), r#""a\"\n\u0001""#);
    }
}
