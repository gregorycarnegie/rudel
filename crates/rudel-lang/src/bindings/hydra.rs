//! The hydra chain DSL, as Koto values.
//!
//! `osc(10).rotate(0.5).modulate(noise())` builds a [`Chain`]; the chain
//! compiles to WGSL when it reaches a widget option (see
//! `widgets::option_from_koto`), so the shader is generated once per evaluation
//! rather than per frame.
//!
//! Every function in [`crate::hydra`] is exposed as a method. The `src` ones
//! start a chain and so need to be callable on their own, but three of them --
//! `osc`, `noise` and `shape` -- are names Strudel already uses, for the OSC
//! output and for two core functions. Hydra takes those globals for itself
//! upstream, which is why `clearHydra` puts `shape` and `speed` back
//! afterwards; here they live on a `Hydra` map instead, so a chain reads
//! `Hydra.osc(10).kaleid(4)` and `.shape(0.5)` still means waveshaping.
//!
//! Capitalised, like the `Math` and `Object` namespaces next to it — and
//! because the lowercase `hydra` is already taken by the widget method that
//! renders a chain, which rudel also exposes as a top-level function.

use crate::hydra::{self, Arg, Chain, FnType, HydraFn};
use koto::{derive::*, prelude::*, runtime::Result as KotoResult};

/// A Koto handle on a hydra chain.
#[derive(Clone, KotoCopy, KotoType)]
pub struct KHydra(pub Chain);

impl KotoObject for KHydra {
    fn display(&self, ctx: &mut DisplayContext) -> KotoResult<()> {
        let names: Vec<&str> = self.0.transforms.iter().map(|t| t.func.name).collect();
        ctx.append(format!("hydra({})", names.join(".")));
        Ok(())
    }
}

impl From<KHydra> for KValue {
    fn from(h: KHydra) -> KValue {
        KObject::from(h).into()
    }
}

/// Read one call argument: a number, or another chain for the `combine` and
/// `modulate` families. Anything else is ignored, so the function's default
/// stands rather than the evaluation failing — a half-written chain should
/// still draw.
fn arg(value: &KValue) -> Option<Arg> {
    match value {
        KValue::Number(n) => Some(Arg::Number(f64::from(n))),
        KValue::Object(o) => o.cast::<KHydra>().ok().map(|h| Arg::Chain(h.0.clone())),
        _ => None,
    }
}

fn args(values: &[KValue]) -> Vec<Arg> {
    // `None` in the middle would shift later arguments onto the wrong
    // parameter, so an unreadable one becomes an explicit gap the compiler
    // fills with hydra's default.
    values
        .iter()
        .map(|v| arg(v).unwrap_or(Arg::Number(f64::NAN)))
        .collect()
}

impl KHydra {
    /// Continue the chain with one more function.
    fn extend(&self, name: &str, values: &[KValue]) -> KotoResult<KValue> {
        let Some(func) = hydra::lookup(name) else {
            return runtime_error!("hydra: no function named '{name}'");
        };
        Ok(KHydra(self.0.clone().then(func, args(values))).into())
    }
}

/// Generate the Koto method for every function in the table — one per hydra
/// function, `src` included so `osc().add(osc())` reads as it does upstream.
///
/// The names are spelled out because `#[koto_method]` needs them at compile
/// time, while the table itself is a runtime static; [`crate::hydra`]'s parity
/// test is what keeps the two lists honest, since a name here that hydra does
/// not have fails `nothing_is_implemented_that_hydra_does_not_have`.
macro_rules! hydra_methods {
    ($($name:ident),* $(,)?) => {
        // hydra's own spellings, which are camelCase.
        #[allow(non_snake_case)]
        #[koto_impl]
        impl KHydra {
            $(
                #[koto_method]
                fn $name(&self, args: &[KValue]) -> KotoResult<KValue> {
                    self.extend(stringify!($name), args)
                }
            )*
        }
    };
}

hydra_methods! {
    // src
    noise, voronoi, osc, shape, gradient, solid,
    // coord
    rotate, scale, pixelate, repeat, repeatX, repeatY, kaleid, scroll, scrollX, scrollY,
    // color
    posterize, shift, invert, contrast, brightness, luma, thresh, color, saturate, hue,
    colorama, r, g, b, a,
    // combine
    add, sub, layer, blend, mult, diff, mask,
    // combineCoord
    modulateRepeat, modulateRepeatX, modulateRepeatY, modulateKaleid, modulateScrollX,
    modulateScrollY, modulate, modulateScale, modulatePixelate, modulateRotate, modulateHue,
}

/// Register the source functions under a `Hydra` map.
///
/// Only `src`-typed functions go here: those are the ones that start a chain.
/// `rotate` on its own is not a hydra expression, so it exists only as a
/// method.
pub(crate) fn register(prelude: &KMap) {
    let hydra_map = KMap::new();
    for func in hydra::functions().iter().filter(|f| f.ty == FnType::Src) {
        let start: &'static HydraFn = func;
        hydra_map.add_fn(func.name, move |ctx| {
            Ok(KHydra(Chain::source(start, args(ctx.args()))).into())
        });
    }
    prelude.insert("Hydra", hydra_map);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_argument_does_not_shift_the_ones_after_it() {
        // `NaN` is the gap marker; the compiler turns it back into the
        // function's own default rather than emitting `NaN` into the shader.
        let got = args(&[KValue::Null, KValue::Number(3.0.into())]);
        assert!(matches!(got[0], Arg::Number(n) if n.is_nan()));
        assert_eq!(got[1], Arg::Number(3.0));
    }

    #[test]
    fn the_source_names_do_not_shadow_the_ones_strudel_already_has() {
        // `osc` is Strudel's OSC output, `noise` and `shape` are core. Hydra
        // claims all three as globals upstream; taking them here would break
        // patterns that never mentioned hydra, so they stay behind `hydra.`.
        let prelude = KMap::new();
        register(&prelude);
        for taken in ["osc", "noise", "shape", "gradient", "solid", "voronoi"] {
            assert!(
                prelude.get(taken).is_none(),
                "`{taken}` was registered as a bare global"
            );
        }
        let Some(KValue::Map(map)) = prelude.get("Hydra") else {
            panic!("no `Hydra` map");
        };
        for source in ["osc", "noise", "shape", "gradient", "solid", "voronoi"] {
            assert!(map.get(source).is_some(), "Hydra.{source} is missing");
        }
        // Lowercase `hydra` is the widget method's top-level form, so the
        // namespace must not be spelled that way or one silently wins.
        assert!(prelude.get("hydra").is_none(), "`hydra` would be shadowed");
    }

    #[test]
    fn every_generated_method_names_a_real_hydra_function() {
        // The macro list is hand-written; this is the guard that it matches the
        // table. `src`/`prev`/`sum` are the documented gaps.
        let expected = hydra::functions().len();
        assert_eq!(expected, 49, "table size changed; update hydra_methods!");
    }
}
