use crate::fraction::Frac;
use crate::pattern::{Pattern, pure};
use crate::value::Value;

/// Anything that can be lifted into a pattern argument.
pub trait IntoPattern {
    fn into_pattern(self) -> Pattern;
}

impl IntoPattern for Pattern {
    fn into_pattern(self) -> Pattern {
        self
    }
}

impl IntoPattern for &Pattern {
    fn into_pattern(self) -> Pattern {
        self.clone()
    }
}

impl IntoPattern for Value {
    fn into_pattern(self) -> Pattern {
        crate::pattern::value_to_pattern(self)
    }
}

macro_rules! into_pattern_via {
    ($($t:ty => $variant:expr),* $(,)?) => {
        $(impl IntoPattern for $t {
            fn into_pattern(self) -> Pattern { pure($variant(self)) }
        })*
    };
}

into_pattern_via!(i64 => Value::Int, f64 => Value::F64, bool => Value::Bool, Frac => Value::Frac);

impl IntoPattern for i32 {
    fn into_pattern(self) -> Pattern {
        pure(Value::Int(self as i64))
    }
}

impl IntoPattern for &str {
    fn into_pattern(self) -> Pattern {
        crate::pattern::parse_string(self)
    }
}

impl IntoPattern for String {
    fn into_pattern(self) -> Pattern {
        crate::pattern::parse_string(&self)
    }
}
