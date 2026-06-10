// controls.rs - control parameters (note, s, gain, pan, ...).
// Mirrors strudel/packages/core/controls.mjs: each control wraps values into a
// single-key map; as a method it merges that key into the pattern.
// SPDX-License-Identifier: AGPL-3.0-or-later

mod aliases;
mod base;
mod convenience;
mod multi;
mod named;
mod plain;
mod registry;
mod special;

pub use aliases::*;
pub use base::{control_dyn, wrap_control_dyn};
pub use multi::{ad, adsr, ar, ds};
pub use named::*;
pub use plain::*;
pub use registry::{control_builders, control_name, numbered_control_names};
pub use special::{mode, s, sound};

#[cfg(test)]
mod tests;
