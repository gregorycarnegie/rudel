// query.rs - scheduler-agnostic extraction of timed control events from a
// pattern. Shared by the audio, MIDI and OSC back-ends so they all see the same
// onsets, timing and control maps.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    fraction::Frac,
    pattern::Pattern,
    state::State,
    timespan::TimeSpan,
    value::{Value, ValueMap},
};

/// A discrete onset with its control map and timing, in seconds on the caller's
/// clock (derived from `cps`).
#[derive(Clone, Debug)]
pub struct ControlEvent {
    /// Onset time in seconds (`onset_cycle / cps`).
    pub onset_seconds: f64,
    /// Event duration in seconds (`whole_duration / cps`).
    pub duration_seconds: f64,
    /// Onset time in cycles.
    pub onset_cycle: f64,
    /// The resolved control map (`note`, `s`, `gain`, ...).
    pub controls: ValueMap,
    /// Widget tags from the hap context (`pat.tag(id)`); the audio engine uses
    /// them to route a voice's output to per-widget analyzer taps.
    pub tags: Vec<String>,
}

/// Normalize a hap value into a control map: maps pass through; bare strings
/// become `{s}`; `name:index` lists become `{s, n}`; bare numbers become
/// `{note}`.
pub fn to_control_map(value: &Value) -> ValueMap {
    match value {
        Value::Map(m) => m.clone(),
        Value::Str(_) => ValueMap::from([("s".to_string(), value.clone())]),
        Value::List(items) if !items.is_empty() => {
            let mut m = ValueMap::from([("s".to_string(), items[0].clone())]);
            if let Some(n) = items.get(1) {
                m.insert("n".to_string(), n.clone());
            }
            m
        }
        other => ValueMap::from([("note".to_string(), other.clone())]),
    }
}

/// The control `log`/`logValues` tag their haps with. Its value is either one
/// of the two built-in formats or, when the user passed a formatting callback,
/// the message that callback already produced.
pub const LOG_KEY: &str = "_log";

/// The control `onTriggerTime` tags its haps with (the id of the callback the
/// host should fire). Defined here because this is where it is stripped; the
/// registry itself lives in the language layer, which owns the Koto VM.
pub const TRIGGER_KEY: &str = "_ontrigger";

/// Build the line a `_log`-tagged event writes. `"hap"` is `log()`'s default
/// (`[hap] ${hap.showWhole(true)}`), `"values"` is `logValues()`'s
/// (`[hap] ${stringifyValues(value, true)}`); anything else is a message a
/// user callback already formatted, and is logged as-is.
fn log_message(mode: &Value, whole: &TimeSpan, controls: &ValueMap) -> String {
    let values = crate::host::stringify_values(&Value::Map(controls.clone()));
    match mode.as_str() {
        Some("hap") => format!("[hap] {} → {}: {values}", whole.begin, whole.end),
        Some("values") => format!("[hap] {values}"),
        Some(message) => message.to_string(),
        None => format!("[hap] {values}"),
    }
}

/// Query `pattern` over the cycle window `[begin_cycle, end_cycle)` and return a
/// [`ControlEvent`] for every onset, with times converted to seconds via `cps`.
/// Exposes `_cps` to cps-dependent transforms (`loopAt`/`fit`/`splice`).
pub fn query_controls(
    pattern: &Pattern,
    cps: f64,
    begin_cycle: f64,
    end_cycle: f64,
) -> Vec<ControlEvent> {
    if cps <= 0.0 || end_cycle <= begin_cycle {
        return Vec::new();
    }
    let begin = Frac::from_f64(begin_cycle);
    let end = Frac::from_f64(end_cycle);
    // `cyclist` marks this as a scheduler/trigger query (Strudel's cyclist sets
    // it too), so impure transforms like `timeline` know to advance their
    // persistent state here rather than on visualiser queries.
    let controls = ValueMap::from([
        ("_cps".to_string(), Value::F64(cps)),
        ("cyclist".to_string(), Value::Str("cyclist".to_string())),
    ]);
    let state = State::with_controls(TimeSpan::new(begin, end), controls);

    let mut out = Vec::new();
    for hap in pattern.query(&state) {
        let Some(whole) = hap.whole else {
            continue; // skip continuous haps
        };
        if !hap.has_onset() {
            continue;
        }
        let onset_cycle = whole.begin.to_f64();
        if onset_cycle < begin_cycle || onset_cycle >= end_cycle {
            continue; // avoid duplicates across adjacent windows
        }
        let mut controls = to_control_map(&hap.value);
        // `log`/`logValues` tag their haps with `_log`; this is the trigger-time
        // hook upstream gets from `Pattern.onTrigger`. The key is consumed here
        // so it never reaches a voice or an OSC message.
        if let Some(mode) = controls.shift_remove(LOG_KEY) {
            crate::host::log_line(log_message(&mode, &whole, &controls));
        }
        // `onTriggerTime`'s hook id is fired by the host (which owns the Koto
        // VM), not here; strip it so it never reaches a voice or an OSC message.
        controls.shift_remove(TRIGGER_KEY);
        // Fold mtranspose/ctranspose into `note` using the hap's tagged scale,
        // matching SuperDirt's external-synth pitch handling.
        crate::tonal::apply_transpose_controls(&mut controls, hap.context.scale.as_deref());
        out.push(ControlEvent {
            onset_seconds: onset_cycle / cps,
            duration_seconds: hap.clipped_duration().to_f64() / cps,
            onset_cycle,
            controls,
            tags: hap.context.tags.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{n, note, pure, sequence, silence};

    fn note_of(ev: &ControlEvent) -> f64 {
        ev.controls.get("note").and_then(|v| v.as_f64()).unwrap()
    }

    #[test]
    fn ctranspose_adds_semitones_to_note() {
        // note(60).ctranspose(7) -> 67, and the control is consumed.
        let pat = note(pure(Value::Int(60))).ctranspose(7);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert_eq!(note_of(&evs[0]), 67.0);
        assert!(!evs[0].controls.contains_key("ctranspose"));
    }

    #[test]
    fn mtranspose_steps_within_tagged_scale() {
        // n(0).scale("C:major") = C3 (48); mtranspose(2) -> degree 2 = E3 (52).
        let pat = n(pure(Value::Int(0))).scale("C:major").mtranspose(2);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert_eq!(note_of(&evs[0]), 52.0);
        assert!(!evs[0].controls.contains_key("mtranspose"));
    }

    #[test]
    fn mtranspose_defaults_to_major_without_a_scale() {
        // No tagged scale -> C:major: note 60 (C4) up 1 step -> D4 (62).
        let mut controls = ValueMap::from([
            ("note".to_string(), Value::Int(60)),
            ("mtranspose".to_string(), Value::Int(1)),
        ]);
        crate::tonal::apply_transpose_controls(&mut controls, None);
        assert_eq!(controls.get("note").and_then(|v| v.as_f64()), Some(62.0));
    }

    #[test]
    fn transpose_controls_left_when_no_note() {
        // Without a note (e.g. a bare sample), the controls are forwarded as-is.
        let mut controls = ValueMap::from([("ctranspose".to_string(), Value::Int(7))]);
        crate::tonal::apply_transpose_controls(&mut controls, None);
        assert_eq!(controls.get("ctranspose"), Some(&Value::Int(7)));
    }

    #[test]
    fn onsets_and_timing() {
        // "bd ~ sd" at cps=1: onsets at 0 and 2/3
        let pat = sequence(&[
            pure(Value::Str("bd".into())),
            silence(),
            pure(Value::Str("sd".into())),
        ]);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert_eq!(evs.len(), 2);
        assert!((evs[0].onset_seconds - 0.0).abs() < 1e-9);
        assert!((evs[1].onset_seconds - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(evs[0].controls.get("s"), Some(&Value::Str("bd".into())));
    }

    #[test]
    fn note_value_becomes_control() {
        let evs = query_controls(&note(pure(Value::Int(60))), 2.0, 0.0, 1.0);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].controls.get("note"), Some(&Value::Int(60)));
        // duration in seconds = 1 cycle / cps
        assert!((evs[0].duration_seconds - 0.5).abs() < 1e-9);
    }

    #[test]
    fn clip_shortens_event_duration_seconds() {
        // note(60).clip(0.5) at cps=1: the sounding duration is halved even
        // though the structural whole still spans a full cycle.
        let pat = note(pure(Value::Int(60))).clip(0.5);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert!((evs[0].duration_seconds - 0.5).abs() < 1e-9);
    }

    #[test]
    fn legato_aliases_clip_for_event_duration() {
        // `.legato(x)` is an alias of `.clip(x)`: it writes the canonical `clip`
        // key and clips the event, matching Strudel's registerControl aliasing.
        let pat = note(pure(Value::Int(60))).legato(0.25);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert!((evs[0].duration_seconds - 0.25).abs() < 1e-9);
        assert!(evs[0].controls.contains_key("clip"));
        assert!(!evs[0].controls.contains_key("legato"));
    }

    #[test]
    fn duration_control_sets_event_length() {
        // A 2-step seq has a 1/2-cycle whole, but `.duration(1)` overrides it so
        // the first event sounds for a full cycle.
        let pat = note(sequence(&[pure(Value::Int(60)), pure(Value::Int(62))])).duration(1.0);
        let evs = query_controls(&pat, 1.0, 0.0, 1.0);
        assert!((evs[0].duration_seconds - 1.0).abs() < 1e-9);
        // Structural onsets are unchanged: still two events, at 0 and 1/2.
        assert_eq!(evs.len(), 2);
        assert!((evs[1].onset_seconds - 0.5).abs() < 1e-9);
    }

    #[test]
    fn windows_do_not_duplicate() {
        let pat = sequence(&[pure(Value::Int(0)), pure(Value::Int(1))]);
        let a = query_controls(&pat, 1.0, 0.0, 0.5);
        let b = query_controls(&pat, 1.0, 0.5, 1.0);
        assert_eq!(a.len() + b.len(), 2);
    }

    #[test]
    fn hap_values_normalise_into_control_maps() {
        let s = |m: &ValueMap, k: &str| m.get(k).cloned();
        assert_eq!(
            to_control_map(&Value::Str("bd".into())),
            ValueMap::from([("s".to_string(), Value::Str("bd".into()))])
        );
        // `[name, index]` is `s`/`n`, and a one-element list is just `s`.
        let m = to_control_map(&Value::List(vec![Value::Str("bd".into()), Value::Int(3)]));
        assert_eq!(s(&m, "s"), Some(Value::Str("bd".into())));
        assert_eq!(s(&m, "n"), Some(Value::Int(3)));
        let m = to_control_map(&Value::List(vec![Value::Str("bd".into())]));
        assert_eq!(s(&m, "s"), Some(Value::Str("bd".into())));
        assert_eq!(s(&m, "n"), None);
        // An empty list has no name to read, so it falls through to `note`
        // rather than indexing into nothing.
        assert_eq!(
            s(&to_control_map(&Value::List(vec![])), "note"),
            Some(Value::List(vec![]))
        );
        assert_eq!(
            s(&to_control_map(&Value::Int(60)), "note"),
            Some(Value::Int(60))
        );
    }

    #[test]
    fn log_lines_name_their_format() {
        let controls = ValueMap::from([("s".to_string(), Value::Str("bd".into()))]);
        let whole = TimeSpan::new(Frac::zero(), Frac::one());
        let line = |mode: Value| log_message(&mode, &whole, &controls);
        assert_eq!(line(Value::Str("hap".into())), "[hap] 0/1 → 1/1: s:bd");
        assert_eq!(line(Value::Str("values".into())), "[hap] s:bd");
        // A user callback's message is logged as it was written.
        assert_eq!(line(Value::Str("mine".into())), "mine");
        assert_eq!(line(Value::Null), "[hap] s:bd");
    }

    #[test]
    fn onsets_convert_to_seconds_by_cps_and_a_stopped_clock_says_nothing() {
        let pat = note(sequence(&[pure(Value::Int(60)), pure(Value::Int(62))]));
        // cps 2: the second of two steps is at cycle 0.5, i.e. a quarter second.
        let evs = query_controls(&pat, 2.0, 0.0, 1.0);
        assert_eq!(evs[1].onset_cycle, 0.5);
        assert_eq!(evs[1].onset_seconds, 0.25);
        assert_eq!(evs[0].duration_seconds, 0.25);
        // A stopped or reversed clock has no events, and no division by zero.
        assert!(query_controls(&pat, 0.0, 0.0, 1.0).is_empty());
        assert!(query_controls(&pat, 1.0, 1.0, 1.0).is_empty());
    }
}
