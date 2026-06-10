// Several Koto methods are deliberately named in camelCase to match Strudel's
// public API exactly (e.g. `iterBack`, `euclidLegato`); the koto derive macro
// also generates `__koto_<name>` shims that inherit those names.
#![allow(non_snake_case)]

mod args;
mod callback;
mod convert;
mod generated;
mod methods;
mod pick;

use koto::derive::*;
use koto::prelude::*;
use koto::runtime::KotoObject;
use rudel_core::Pattern;

pub(crate) use convert::{arg_to_f64, arg0};
pub(super) use convert::{
    arg_to_group, arg_to_pattern, arg_to_pattern_weight, arg_to_value, arg_to_weighted_pair,
    koto_to_value,
};
pub(super) use pick::pick_args;

/// A Koto wrapper around a rudel [`Pattern`].
#[derive(Clone, KotoCopy, KotoType)]
pub struct KPattern(pub Pattern);

impl KotoObject for KPattern {}

impl From<KPattern> for KValue {
    fn from(p: KPattern) -> KValue {
        KObject::from(p).into()
    }
}

impl KPattern {
    fn wrap(pat: Pattern) -> KValue {
        KPattern(pat).into()
    }
}
