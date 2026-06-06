use rudel_core::Value;

pub fn mtof(note: f64) -> f32 {
    (440.0 * 2f64.powf((note - 69.0) / 12.0)) as f32
}

/// Convert a note value (number, or a name like `c4`/`eb3`/`f#5`) to a
/// frequency. Note names follow the convention a4 = 69 = 440 Hz.
pub fn note_to_freq(value: &Value) -> Option<f32> {
    if let Some(n) = value.as_f64() {
        // numeric: treat as a MIDI note number
        if matches!(value, Value::Int(_) | Value::F64(_)) {
            return Some(mtof(n));
        }
    }
    let s = value.as_str()?;
    note_name_to_midi(s).map(|m| mtof(m as f64))
}

/// Parse a note name like `c`, `cs4`, `c#4`, `eb3`, `Gb2` to a MIDI number.
/// Delegates to the canonical implementation in `rudel_core::tonal`.
pub fn note_name_to_midi(s: &str) -> Option<i32> {
    rudel_core::note_to_midi(s)
}
