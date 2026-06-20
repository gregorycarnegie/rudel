use rudel_core::{Pattern, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

static SLIDER_VALUES: LazyLock<RwLock<HashMap<String, Value>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub(crate) fn slider_with_id(id: String, value: Value) -> Pattern {
    sync_slider_value(&id, value);
    rudel_core::signal::signal(move |_| slider_value(&id).unwrap_or(Value::Null))
}

fn sync_slider_value(id: &str, value: Value) {
    SLIDER_VALUES.write().unwrap().insert(id.to_string(), value);
}

/// Update a registered slider value from the editor UI. Returns `false` when
/// the id is unknown, matching Strudel's "only update registered sliders" rule.
pub fn set_slider_value(id: &str, value: f64) -> bool {
    let mut values = SLIDER_VALUES.write().unwrap();
    let Some(slot) = values.get_mut(id) else {
        return false;
    };
    *slot = Value::F64(value);
    true
}

/// Read the current value for a registered slider.
pub fn slider_value(id: &str) -> Option<Value> {
    SLIDER_VALUES.read().unwrap().get(id).cloned()
}
