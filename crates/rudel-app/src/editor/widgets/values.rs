use rudel_core::Value;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_values_drop_only_trailing_zeros() {
        assert_eq!(value_short(&Value::Str("bd".into())), "bd");
        assert_eq!(value_short(&Value::Int(3)), "3");
        assert_eq!(value_short(&Value::F64(0.5)), "0.5");
        assert_eq!(value_short(&Value::F64(2.0)), "2");
        assert_eq!(value_short(&Value::F64(1.0 / 3.0)), "0.333");
        // 10 would lose its zero to a bare `trim_end_matches('0')`.
        assert_eq!(value_short(&Value::Int(10)), "10");
    }
}
