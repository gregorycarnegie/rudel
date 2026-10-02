//! The hydra chain DSL, as script values.
//!
//! `osc(10).rotate(0.5).modulate(noise())` builds a [`Chain`]; the chain
//! compiles to WGSL when it reaches a widget option (see
//! `widgets::option_from_arg`), so the shader is generated once per evaluation
//! rather than per frame.
//!
//! Every function in [`crate::hydra`] is exposed as a method. The `src` ones
//! start a chain and so need to be callable on their own, but three of them --
//! `osc`, `noise` and `shape` -- are names Strudel already uses, for the OSC
//! output and for two core functions. Hydra takes those globals for itself
//! upstream, which is why `clearHydra` puts `shape` and `speed` back
//! afterwards; here they live on a `Hydra` object instead, so a chain reads
//! `Hydra.osc(10).kaleid(4)` and `.shape(0.5)` still means waveshaping.
//!
//! Capitalised, like the `Math` and `Object` namespaces next to it — and
//! because the lowercase `hydra` is already taken by the widget method that
//! renders a chain, which rudel also exposes as a top-level function.

use crate::{
    hydra::{self, Chain, FnType, HydraFn},
    js::{Arg, Scope},
};

/// Read one call argument: a number, or another chain for the `combine` and
/// `modulate` families. Anything else is ignored, so the function's default
/// stands rather than the evaluation failing — a half-written chain should
/// still draw.
fn arg(value: &Arg) -> Option<hydra::Arg> {
    match value {
        Arg::Num(n) => Some(hydra::Arg::Number(*n)),
        Arg::Hydra(chain) => Some(hydra::Arg::Chain(chain.clone())),
        _ => None,
    }
}

fn args(values: &[Arg]) -> Vec<hydra::Arg> {
    // `None` in the middle would shift later arguments onto the wrong
    // parameter, so an unreadable one becomes an explicit gap the compiler
    // fills with hydra's default.
    values
        .iter()
        .map(|v| arg(v).unwrap_or(hydra::Arg::Number(f64::NAN)))
        .collect()
}

/// Register the source functions under a `Hydra` object, and every hydra
/// function as a method of a chain.
///
/// Only `src`-typed functions start a chain: `rotate` on its own is not a
/// hydra expression, so it exists only as a method. The methods include the
/// sources, so `osc().add(osc())` reads as it does upstream.
pub(crate) fn register(prelude: &Scope) {
    let methods = Scope::hydra();
    for func in hydra::functions() {
        let func: &'static HydraFn = func;
        methods.method(func.name, move |this, a| match this {
            Arg::Hydra(chain) => Ok(Arg::Hydra(chain.clone().then(func, args(a)))),
            _ => Err(format!("hydra: {} is not called on a chain", func.name)),
        });
    }
    let namespace = prelude.namespace("Hydra");
    for func in hydra::functions().iter().filter(|f| f.ty == FnType::Src) {
        let start: &'static HydraFn = func;
        namespace.func(func.name, move |a| {
            Ok(Arg::Hydra(Chain::source(start, args(a))))
        });
    }
    // The output buffers, as plain indices: `Hydra.src(Hydra.o1)` reads the
    // chain bound to the widget's `o1` option, as it stood last frame.
    for (name, index) in [("o0", 0.0), ("o1", 1.0), ("o2", 2.0), ("o3", 3.0)] {
        namespace.value(name, index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_argument_does_not_shift_the_ones_after_it() {
        // `NaN` is the gap marker; the compiler turns it back into the
        // function's own default rather than emitting `NaN` into the shader.
        let got = args(&[Arg::Null, Arg::Num(3.0)]);
        assert!(matches!(got[0], hydra::Arg::Number(n) if n.is_nan()));
        assert_eq!(got[1], hydra::Arg::Number(3.0));
    }

    #[test]
    fn a_chain_argument_is_kept_as_a_chain() {
        let chain = Chain::source(hydra::lookup("noise").expect("noise"), Vec::new());
        assert_eq!(
            args(&[Arg::Hydra(chain.clone())]),
            [hydra::Arg::Chain(chain)]
        );
    }

    #[test]
    fn only_sources_start_a_chain_and_every_function_continues_one() {
        crate::eval("const c = Hydra.osc().rotate(1)\ns(\"bd\")")
            .expect("osc starts, rotate continues");
        let err = crate::eval("const c = Hydra.rotate(1)\ns(\"bd\")")
            .err()
            .expect("rotate is not a source");
        assert!(err.contains("not a callable function"), "{err}");
    }

    #[test]
    fn every_hydra_function_is_in_the_table() {
        // The methods are generated from the table, so its size is the one
        // thing to pin: `src`/`prev`/`sum` are the documented gaps.
        assert_eq!(hydra::functions().len(), 51, "table size changed");
    }
}
