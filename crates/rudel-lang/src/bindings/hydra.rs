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
    WidgetConfig, WidgetOption,
    hydra::{self, Chain, FnType, HydraFn},
    js::{Arg, Scope},
};
use std::cell::RefCell;

/// What a script sent to hydra's own outputs: `chain.out(o1)` and `render(…)`.
/// Upstream that is the full-screen canvas behind the code.
#[derive(Default)]
struct Scene {
    outputs: [Option<Chain>; 4],
    /// `None` until `render` is called, which shows `o0`; `Some(None)` is
    /// `render()`, all four.
    render: Option<Option<usize>>,
}

thread_local! {
    // Evaluations run one at a time on the JS thread.
    static SCENE: RefCell<Scene> = RefCell::new(Scene::default());
}

/// Forget the previous evaluation's outputs.
pub(crate) fn reset_scene() {
    SCENE.with(|s| *s.borrow_mut() = Scene::default());
}

/// This evaluation's hydra outputs as a `_hydra` widget config, if it sent a
/// chain to any. Its options are the ones the inline `_hydra` widget takes.
pub(crate) fn take_scene() -> Option<WidgetConfig> {
    let scene = SCENE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if scene.outputs.iter().all(Option::is_none) {
        return None;
    }
    let mut options: std::collections::BTreeMap<_, _> = scene
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(i, chain)| {
            let wgsl = hydra::compile(chain.as_ref()?, i);
            Some((format!("o{i}"), WidgetOption::String(wgsl)))
        })
        .collect();
    let render = match scene.render {
        None => WidgetOption::Number(0.0),
        Some(None) => WidgetOption::String("all".to_string()),
        Some(Some(i)) => WidgetOption::Number(i as f64),
    };
    options.insert("render".to_string(), render);
    Some(WidgetConfig {
        widget_type: "_hydra".to_string(),
        id: "hydra-background".to_string(),
        options,
        ..WidgetConfig::default()
    })
}

/// An output index from `o0`..`o3` (which are plain numbers here).
fn output_index(value: Option<&Arg>) -> Option<usize> {
    match value {
        Some(Arg::Num(n)) if n.is_finite() => Some((*n as usize).min(3)),
        _ => None,
    }
}

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
    // `chain.out(o1)`: show this chain on hydra's own canvas, in that output
    // (`o0` when none is named).
    methods.method("out", |this, a| match this {
        Arg::Hydra(chain) => {
            let index = output_index(a.first()).unwrap_or(0);
            SCENE.with(|s| s.borrow_mut().outputs[index] = Some(chain.clone()));
            Ok(Arg::Null)
        }
        _ => Err("hydra: out is not called on a chain".to_string()),
    });
    let namespace = prelude.namespace("Hydra");
    // `render(o2)` shows one output; `render()` all four.
    namespace.func("render", |a| {
        SCENE.with(|s| s.borrow_mut().render = Some(output_index(a.first())));
        Ok(Arg::Null)
    });
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

    fn scene(src: &str) -> Option<WidgetConfig> {
        crate::eval_result(src).expect("eval").meta.hydra
    }

    #[test]
    fn out_sends_a_chain_to_the_scene_behind_the_code() {
        assert_eq!(scene("s(\"bd\")"), None);
        let one = scene("await initHydra()
osc(10).out()
s(\"bd\")").expect("a scene");
        assert_eq!(one.widget_type, "_hydra");
        assert!(matches!(one.options.get("o0"), Some(WidgetOption::String(w)) if w.contains("h_osc")));
        assert_eq!(one.options.get("render"), Some(&WidgetOption::Number(0.0)));

        // `render(o1)` shows one output, `render()` all of them.
        let two = scene("await initHydra()
noise().out(o1)
render(o1)").expect("a scene");
        assert!(two.options.contains_key("o1") && !two.options.contains_key("o0"));
        assert_eq!(two.options.get("render"), Some(&WidgetOption::Number(1.0)));
        let all = scene("await initHydra()
osc().out()
render()").expect("a scene");
        assert_eq!(all.options.get("render"), Some(&WidgetOption::String("all".into())));
    }

    #[test]
    fn hydra_globals_wait_for_init_hydra() {
        // Until then `osc` is still Strudel's OSC output and `noise` its signal.
        assert_eq!(scene("Hydra.osc().out()"), scene("await initHydra()
osc().out()"));
        assert!(crate::eval("osc().out()").is_err());
    }

    #[test]
    fn upstream_hydra_idioms_evaluate() {
        // External sources read as empty, arrays take hydra's sequencing
        // methods, and `a` exists with `detectAudio`.
        let src = "await initHydra({detectAudio: true})
s0.initCam()
                   src(s0).modulate(osc([1, 2].fast(2).smooth(), 0.1)).out()
                   a.setBins(3)
n(a.fft.length)";
        let result = crate::eval_result(src).expect("eval");
        assert!(result.meta.hydra.is_some());
    }

    #[test]
    fn every_hydra_function_is_in_the_table() {
        // The methods are generated from the table, so its size is the one
        // thing to pin: `src`/`prev`/`sum` are the documented gaps.
        assert_eq!(hydra::functions().len(), 51, "table size changed");
    }
}
