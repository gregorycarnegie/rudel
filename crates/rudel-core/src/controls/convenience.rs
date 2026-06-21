use crate::pattern::Pattern;
use crate::value::{Value, ValueMap};
use crate::xen::freq_to_midi;

impl Pattern {
    /// Strudel's `piano()` convenience: select the piano sample bank, set a
    /// short release and default clip, then spread notes gently by pitch.
    pub fn piano(&self) -> Pattern {
        self.s("piano").release(0.1).fmap(|v| match v {
            Value::Map(mut m) => {
                let pan = piano_pan(&m);
                m.entry("clip".to_string()).or_insert(Value::Int(1));
                if let Some(pan) = pan {
                    let existing = m.get("pan").and_then(Value::as_f64).unwrap_or(1.0);
                    m.insert("pan".to_string(), Value::F64(existing * pan));
                }
                Value::Map(m)
            }
            other => other,
        })
    }
}

fn piano_pan(m: &ValueMap) -> Option<f64> {
    let midi = m
        .get("note")
        .and_then(value_to_midi)
        .or_else(|| m.get("freq").and_then(|v| v.as_f64().map(freq_to_midi)))?;
    let max_pan = crate::tonal::note_to_midi("C8")? as f64;
    let pitch_pan = (midi.round() / max_pan).clamp(0.0, 1.0);
    Some(pitch_pan * 0.5 + 0.25)
}

fn value_to_midi(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => crate::tonal::note_to_midi(s).map(|m| m as f64),
        other => other.as_f64(),
    }
}
