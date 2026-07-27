// host.rs - process-global tables the host publishes for scripts to read back.
//
// The input buses in `input.rs` carry realtime *input* (MIDI CC, pointer,
// keyboard) into query-time signals. This module is the other direction: state
// the audio engine and the app own, which the language layer needs at
// evaluation time but cannot reach directly (`rudel-lang` depends only on
// `rudel-core`).
//
// - Sample durations back `getDuration`/`getDur` (superdough's `sampler.mjs`),
//   which upstream reads off the decoded `AudioBuffer`. The bank publishes each
//   sample's length as it registers it.
// - The log ring backs `log`/`logValues` (`core/pattern.mjs`), whose messages
//   upstream go to `logger()` and appear in the REPL's side menu. Rudel's
//   scheduler pushes a line here as each tagged event is turned into a note,
//   and the app drains it into its console panel.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    collections::{HashMap, VecDeque},
    sync::{LazyLock, RwLock},
};

// ---------------------------------------------------------------------------
// Sample durations

/// Sample length in seconds, keyed by `(sound name, n index)`.
static SAMPLE_DURATIONS: LazyLock<RwLock<HashMap<(String, i64), f64>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Publish the length (seconds) of the `n`-th sample registered under `name`.
pub fn set_sample_duration(name: &str, n: i64, seconds: f64) {
    SAMPLE_DURATIONS
        .write()
        .unwrap()
        .insert((name.to_string(), n), seconds);
}

/// The length in seconds of `name`'s `n`-th sample, if it is loaded.
pub fn sample_duration(name: &str, n: i64) -> Option<f64> {
    SAMPLE_DURATIONS
        .read()
        .unwrap()
        .get(&(name.to_string(), n))
        .copied()
}

/// Forget every published duration (bank reset / tests).
pub fn clear_sample_durations() {
    SAMPLE_DURATIONS.write().unwrap().clear();
}

// ---------------------------------------------------------------------------
// Log lines

/// How many log lines are kept before the oldest are dropped. Bounded because
/// a pattern logging every event fills this faster than a UI drains it.
const LOG_CAPACITY: usize = 512;

static LOG: LazyLock<RwLock<VecDeque<String>>> =
    LazyLock::new(|| RwLock::new(VecDeque::with_capacity(LOG_CAPACITY)));

/// Record one log line (from a `log`/`logValues`-tagged event being played).
pub fn log_line(line: String) {
    let mut log = LOG.write().unwrap();
    if log.len() == LOG_CAPACITY {
        log.pop_front();
    }
    log.push_back(line);
}

/// Take every buffered log line, emptying the ring.
pub fn drain_log() -> Vec<String> {
    LOG.write().unwrap().drain(..).collect()
}

/// Render a hap value the way `util.mjs`'s `stringifyValues(value, true)` does:
/// a control map prints as `key:value` pairs separated by spaces, anything else
/// prints as itself.
pub fn stringify_values(value: &crate::value::Value) -> String {
    use crate::value::Value;
    match value {
        Value::Map(m) => m
            .iter()
            .map(|(k, v)| format!("{k}:{}", stringify_values(v)))
            .collect::<Vec<_>>()
            .join(" "),
        Value::List(items) => items
            .iter()
            .map(stringify_values)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => format!("{x}"),
        Value::Frac(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_round_trip_per_name_and_index() {
        set_sample_duration("sax", 0, 1.5);
        set_sample_duration("sax", 1, 2.0);
        assert_eq!(sample_duration("sax", 0), Some(1.5));
        assert_eq!(sample_duration("sax", 1), Some(2.0));
        assert_eq!(sample_duration("sax", 2), None);
        assert_eq!(sample_duration("nope", 0), None);
    }

    #[test]
    fn log_drains_in_order_and_is_bounded() {
        // The ring is process-global, so drain first to start from empty.
        drain_log();
        for i in 0..LOG_CAPACITY + 10 {
            log_line(i.to_string());
        }
        let lines = drain_log();
        assert_eq!(lines.len(), LOG_CAPACITY, "oldest lines are dropped");
        assert_eq!(lines.first().map(String::as_str), Some("10"));
        assert_eq!(lines.last().map(String::as_str), Some("521"));
        assert!(drain_log().is_empty(), "draining empties the ring");
    }
}
