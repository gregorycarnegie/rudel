use super::helpers::frac;
use crate::fraction::Frac;
use crate::pattern::{Pattern, pure};
use crate::transforms::IntoPattern;
use crate::value::{Value, ValueMap};

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
