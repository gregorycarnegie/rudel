// transforms/core.rs - patternified, argument-lifting transforms.
// These wrap the raw `_`-prefixed ops in pattern.rs the way Strudel's
// `register` mechanism does: arguments can themselves be patterns.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod align;
mod higher_order;
mod into_pattern;
mod patternify;
mod random;
mod stepwise;
mod structure;
mod time;
mod value_methods;
mod value_ops;

#[cfg(test)]
mod tests;

pub use align::Align;
pub use into_pattern::IntoPattern;

pub(crate) use value_ops::{num_mod, num_pow};
