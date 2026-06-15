mod pattern;
mod prelude;
mod routing;

use koto::prelude::*;

pub use pattern::KPattern;
pub(crate) use pattern::{
    arg_to_f64, arg_to_raw_str, arg0, collected_stack, method_names, reset_slots,
};
pub(crate) use prelude::register;
pub use routing::{filter_output, output_targets};

/// The names of the top-level functions/values registered in `prelude`,
/// sorted. Used to build the generated reference surface from the live runtime
/// rather than a hand-maintained list.
pub(crate) fn function_names(prelude: &KMap) -> Vec<String> {
    let mut names: Vec<String> = prelude
        .data()
        .iter()
        .filter_map(|(key, _)| match key.value() {
            KValue::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
