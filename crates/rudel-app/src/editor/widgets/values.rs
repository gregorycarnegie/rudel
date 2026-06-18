use rudel_core::Value;

pub(super) fn value_to_midi(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| rudel_core::note_to_midi(s).map(|m| m as f64)),
        other => other.as_f64(),
    }
}

pub(super) fn value_short(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::F64(x) => format!("{x:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string(),
        other => format!("{other:?}"),
    }
}
