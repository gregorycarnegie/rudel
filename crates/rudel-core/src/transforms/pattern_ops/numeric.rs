use super::helpers::frac;
use crate::{
    fraction::Frac,
    pattern::{Pattern, pure},
    transforms::IntoPattern,
    value::{Value, ValueMap},
};

impl Pattern {
    // -- Numeric value transforms ------------------------------------------

    /// Round each numeric value (`round`).
    pub fn round(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).round()))
    }
    /// Floor each numeric value (`floor`).
    pub fn floor(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).floor()))
    }
    /// Ceil each numeric value (`ceil`).
    pub fn ceil(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).ceil()))
    }
    /// Base-2 logarithm of each numeric value (`log2`).
    pub fn log2(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).log2()))
    }
    /// Scale a unipolar (0..1) value to bipolar (-1..1) (`toBipolar`).
    pub fn to_bipolar(&self) -> Pattern {
        self.fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0) * 2.0 - 1.0))
    }
    /// Scale a bipolar (-1..1) value to unipolar (0..1) (`fromBipolar`).
    pub fn from_bipolar(&self) -> Pattern {
        self.fmap(|v| Value::F64((v.as_f64().unwrap_or(0.0) + 1.0) / 2.0))
    }
    /// Scale a bipolar signal into `min..max` (`range2`).
    pub fn range2(&self, min: f64, max: f64) -> Pattern {
        self.from_bipolar().range(min, max)
    }
    /// Exponential variant of [`range`](Self::range) (`rangex`).
    pub fn rangex(&self, min: f64, max: f64) -> Pattern {
        self.range(min.ln(), max.ln())
            .fmap(|v| Value::F64(v.as_f64().unwrap_or(0.0).exp()))
    }

    /// Both speed up the pattern and the sample playback (`hurry`).
    pub fn hurry(&self, r: impl Into<Frac>) -> Pattern {
        let r = frac(r);
        let mut m = ValueMap::new();
        m.insert("speed".to_string(), Value::Frac(r));
        self._fast(r).mul(pure(Value::Map(m)))
    }

    // -- more math ops -----------------------------------------------------

    /// Modulo each value by `other` (`mod`).
    pub fn modulo(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), super::super::core::num_mod)
    }
    /// Raise each value to the power `other` (`pow`).
    pub fn pow(&self, other: impl IntoPattern) -> Pattern {
        self.op_in(other.into_pattern(), super::super::core::num_pow)
    }

    /// Reduce `":"`-list values to a single divided number (`ratio`).
    pub fn ratio(&self) -> Pattern {
        self.fmap(|v| ratio_value(&v))
    }
}

/// Reduce `":"`-separated list values to a single number (`ratio`).
pub fn ratio_value(v: &Value) -> Value {
    match v {
        Value::List(items) if !items.is_empty() => {
            let mut acc = items[0].as_f64().unwrap_or(0.0);
            for item in &items[1..] {
                acc /= item.as_f64().unwrap_or(1.0);
            }
            Value::F64(acc)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pat: &Pattern) -> Vec<f64> {
        pat.query_arc(Frac::zero(), Frac::one())
            .into_iter()
            .filter_map(|h| h.value.as_f64())
            .collect()
    }

    #[test]
    fn bipolar_conversions_are_inverses_at_both_ends_and_the_middle() {
        let unipolar = crate::pattern::fastcat(&[
            pure(Value::F64(0.0)),
            pure(Value::F64(0.5)),
            pure(Value::F64(1.0)),
        ]);
        assert_eq!(values(&unipolar.to_bipolar()), vec![-1.0, 0.0, 1.0]);
        assert_eq!(
            values(&unipolar.to_bipolar().from_bipolar()),
            vec![0.0, 0.5, 1.0]
        );
    }

    #[test]
    fn a_ratio_list_divides_left_to_right() {
        // `"3:2"` is 1.5, and a third element keeps dividing.
        let ratio = |items: Vec<Value>| ratio_value(&Value::List(items));
        assert_eq!(ratio(vec![Value::Int(3), Value::Int(2)]), Value::F64(1.5));
        assert_eq!(
            ratio(vec![Value::Int(8), Value::Int(2), Value::Int(2)]),
            Value::F64(2.0)
        );
        // Nothing to reduce: the value passes through untouched, and an empty
        // list has no first element to read.
        assert_eq!(ratio_value(&Value::Int(3)), Value::Int(3));
        assert_eq!(ratio(vec![]), Value::List(vec![]));
        assert_eq!(ratio(vec![Value::Int(3)]), Value::F64(3.0));
    }
}
