//! Hydra chains, compiled to WGSL.
//!
//! [Hydra](https://hydra.ojack.xyz/) builds a visual by chaining sources and
//! transforms — `osc(10).rotate(0.5).modulate(noise())` — and compiles the
//! chain into one fragment shader. Strudel's `@strudel/hydra` is a ~50-line
//! loader that fetches hydra-synth from a CDN at runtime, so there is no hydra
//! source in the vendored Strudel tree; the reference here is hydra-synth
//! 1.3.29, pinned into `tools/oracle/hydra_golden.json` by
//! `tools/oracle/gen_hydra_oracle.mjs`.
//!
//! Two pieces make up the port: [`table`] holds every function's WGSL body and
//! signature, and [`compile`] folds a chain into a shader the way hydra's own
//! `generate-glsl.js` does. The fold is the whole trick, and it turns on the
//! function's *type*:
//!
//! | type | shape | composes |
//! | --- | --- | --- |
//! | `src` | `vec4 f(vec2 st, …)` | starts a chain |
//! | `coord` | `vec2 f(vec2 st, …)` | inward, wrapping the coordinate |
//! | `color` | `vec4 f(vec4 c, …)` | outward, wrapping the colour |
//! | `combine` | `vec4 f(vec4 a, vec4 b, …)` | joins two chains |
//! | `combineCoord` | `vec2 f(vec2 st, vec4 c, …)` | one chain warps another's coords |
//!
//! So `osc(10).rotate(0.5).color(1,0,0)` folds to
//! `h_color(h_osc(h_rotate(st, 0.5, 0.0), 10.0, 0.1, 0.0), 1.0, 0.0, 0.0, 1.0)`.

mod table;

use std::fmt::Write as _;

/// How a function composes into a chain. Hydra's own five types, verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnType {
    Src,
    Coord,
    Color,
    Combine,
    CombineCoord,
}

impl FnType {
    /// The spelling hydra uses in its table, and the oracle checks against.
    pub fn as_str(self) -> &'static str {
        match self {
            FnType::Src => "src",
            FnType::Coord => "coord",
            FnType::Color => "color",
            FnType::Combine => "combine",
            FnType::CombineCoord => "combineCoord",
        }
    }
}

/// A shared WGSL helper a function's body calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Helper {
    Noise,
    Luminance,
    RgbToHsv,
    HsvToRgb,
}

impl Helper {
    fn wgsl(self) -> &'static str {
        match self {
            Helper::Noise => table::NOISE,
            Helper::Luminance => table::LUMINANCE,
            Helper::RgbToHsv => table::RGB_TO_HSV,
            Helper::HsvToRgb => table::HSV_TO_RGB,
        }
    }
}

/// One of a function's parameters, with the default hydra gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub name: &'static str,
    pub default: f64,
}

/// One hydra function: what it is called, how it composes, what it takes, and
/// the WGSL body that implements it.
#[derive(Debug)]
pub struct HydraFn {
    pub name: &'static str,
    pub ty: FnType,
    pub inputs: &'static [Input],
    pub helpers: &'static [Helper],
    pub wgsl: &'static str,
}

/// Hydra functions Rudel does not implement, and why. Checked by the parity
/// test, so this list cannot quietly rot.
pub const UNIMPLEMENTED: &[(&str, &str)] = &[
    (
        "sum",
        "its GLSL body closes the function and opens a second overload, and it returns a float \
         where the composer expects a vec4",
    ),
];

/// Every implemented function.
pub fn functions() -> &'static [HydraFn] {
    table::FUNCTIONS
}

/// Look one up by the name a script uses.
pub fn lookup(name: &str) -> Option<&'static HydraFn> {
    table::FUNCTIONS.iter().find(|f| f.name == name)
}

/// An argument to one link in a chain: a plain number, or a whole nested chain
/// (which is what `modulate(osc())` passes).
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Number(f64),
    Chain(Chain),
}

/// One link: a function and the arguments the script gave it.
#[derive(Clone, Debug)]
pub struct Transform {
    pub func: &'static HydraFn,
    pub args: Vec<Arg>,
}

// Table entries are unique statics, so identity is the equality that matters
// and `HydraFn` itself needs no `PartialEq`.
impl PartialEq for Transform {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.func, other.func) && self.args == other.args
    }
}

/// A chain of transforms, in the order they were written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chain {
    pub transforms: Vec<Transform>,
}

impl Chain {
    /// Start a chain from a source function.
    pub fn source(func: &'static HydraFn, args: Vec<Arg>) -> Self {
        Self {
            transforms: vec![Transform { func, args }],
        }
    }

    /// Append a transform.
    pub fn then(mut self, func: &'static HydraFn, args: Vec<Arg>) -> Self {
        self.transforms.push(Transform { func, args });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
}

/// Render a float the way WGSL wants it: always with a decimal point, so `1`
/// does not arrive as an integer literal where an `f32` is expected.
fn wgsl_f32(value: f64) -> String {
    if value.is_finite() {
        let rendered = format!("{value:?}");
        if rendered.contains('.') {
            rendered
        } else {
            format!("{rendered}.0")
        }
    } else {
        // A chain should never carry these, but a NaN reaching the shader as
        // the literal `NaN` would be a compile error rather than a bad picture.
        "0.0".to_string()
    }
}

/// The arguments for one call, padded out with hydra's defaults and rendered as
/// WGSL. A nested chain argument compiles as its own expression from `st`.
///
/// `arg_offset` is 1 for `combine`/`combineCoord` and 0 otherwise. Hydra
/// `unshift`s a `color` input onto those two at registration time
/// (`generator-factory.js`, "add extra input to beginning for backward
/// combatibility"), so upstream's input list is one longer than the table's and
/// its indices line up with the user's arguments. The table here is the raw
/// one, so the *arguments* are offset instead: `add(other, amount)` puts the
/// other chain at `args[0]` and `amount` at `args[1]`, against `inputs[0]`.
fn call_args(transform: &Transform, arg_offset: usize, ctx: &mut Ctx) -> String {
    let mut out = String::new();
    for (index, input) in transform.func.inputs.iter().enumerate() {
        out.push_str(", ");
        match transform.args.get(index + arg_offset) {
            Some(Arg::Number(value)) if value.is_finite() => out.push_str(&wgsl_f32(*value)),
            // hydra lets a generator stand in for a numeric input; it compiles
            // from `st` rather than from the enclosing coordinate.
            Some(Arg::Chain(chain)) => out.push_str(&fold(chain, "st", ctx)),
            // Absent, or a non-finite gap marker standing in for an argument
            // the binding could not read: hydra's own default takes over, which
            // keeps later arguments on the right parameters.
            _ => out.push_str(&wgsl_f32(input.default)),
        }
    }
    out
}

/// What the fold carries: the functions it has emitted so far, and which output
/// buffer this chain is being compiled for (which is what `prev()` means).
struct Ctx {
    used: Vec<&'static HydraFn>,
    output: usize,
}

impl Ctx {
    fn use_fn(&mut self, func: &'static HydraFn) {
        if !self.used.iter().any(|f| f.name == func.name) {
            self.used.push(func);
        }
    }
}

/// The expression for a chain evaluated at coordinate `uv`.
///
/// This is hydra's `generateGlsl`: a fold that carries a "what does the colour
/// at `uv` come out as" closure, and rewrites it per transform type.
fn fold(chain: &Chain, uv: &str, ctx: &mut Ctx) -> String {
    let mut frag = String::new();
    for transform in &chain.transforms {
        // `prev()` is "the buffer this chain draws into, last frame", which is
        // `src` of the output being compiled — something a fixed WGSL body
        // cannot express, since it depends on where the chain is bound.
        if transform.func.name == "prev" {
            let src = lookup("src").expect("src is in the table");
            ctx.use_fn(src);
            frag = format!("h_src({uv}, {}.0)", ctx.output);
            continue;
        }
        ctx.use_fn(transform.func);
        let name = transform.func.name;
        frag = match transform.func.ty {
            // A source ignores whatever came before it, exactly as upstream.
            FnType::Src => format!("h_{name}({uv}{})", call_args(transform, 0, ctx)),
            // Coordinate transforms wrap inward: the chain so far is evaluated
            // at the *transformed* coordinate.
            FnType::Coord => {
                let inner = format!("h_{name}({uv}{})", call_args(transform, 0, ctx));
                fold(&chain_before(chain, transform), &inner, ctx)
            }
            // Colour transforms wrap outward, around the colour so far.
            FnType::Color => format!("h_{name}({frag}{})", call_args(transform, 0, ctx)),
            FnType::Combine => {
                let other = combine_operand(transform, uv, ctx);
                format!(
                    "h_{name}({frag}, {other}{})",
                    call_args(transform, 1, ctx)
                )
            }
            FnType::CombineCoord => {
                let other = combine_operand(transform, uv, ctx);
                let inner = format!(
                    "h_{name}({uv}, {other}{})",
                    call_args(transform, 1, ctx)
                );
                fold(&chain_before(chain, transform), &inner, ctx)
            }
        };
    }
    frag
}

/// The first argument of a `combine`/`combineCoord`: the other chain, or a bare
/// number if the script passed one.
fn combine_operand(transform: &Transform, uv: &str, ctx: &mut Ctx) -> String {
    match transform.args.first() {
        Some(Arg::Chain(chain)) => fold(chain, uv, ctx),
        Some(Arg::Number(value)) => format!("vec4<f32>({})", wgsl_f32(*value)),
        None => "vec4<f32>(0.0)".to_string(),
    }
}

/// The part of `chain` up to (not including) `transform`.
///
/// `coord` and `combineCoord` re-evaluate everything before them at a new
/// coordinate, which upstream gets by having already built that closure. The
/// fold here is iterative, so it slices instead.
fn chain_before(chain: &Chain, transform: &Transform) -> Chain {
    let end = chain
        .transforms
        .iter()
        .position(|t| std::ptr::eq(t, transform))
        .unwrap_or(0);
    Chain {
        transforms: chain.transforms[..end].to_vec(),
    }
}

/// The WGSL signature for a function of this type.
fn signature(func: &HydraFn) -> String {
    let (head, returns) = match func.ty {
        FnType::Src => ("_st: vec2<f32>", "vec4<f32>"),
        FnType::Coord => ("_st: vec2<f32>", "vec2<f32>"),
        FnType::Color => ("_c0: vec4<f32>", "vec4<f32>"),
        FnType::Combine => ("_c0: vec4<f32>, _c1: vec4<f32>", "vec4<f32>"),
        FnType::CombineCoord => ("_st: vec2<f32>, _c0: vec4<f32>", "vec2<f32>"),
    };
    let mut params = head.to_string();
    for input in func.inputs {
        let _ = write!(params, ", {}: f32", input.name);
    }
    format!("fn h_{}({params}) -> {returns}", func.name)
}

/// Compile a chain into a complete WGSL module, bound to output buffer
/// `output` (0-3) — which is what `prev()` in the chain reads.
///
/// The module is self-contained — its own vertex and fragment entry points and
/// its own uniform block — because a hydra chain needs module-scope helper
/// functions, which cannot be spliced into the `_shader` widget's body slot.
/// The uniform layout is deliberately the same one `_shader` uses, so the app
/// renders both through the same pipeline cache and uniform buffer.
pub fn compile(chain: &Chain, output: usize) -> String {
    let mut ctx = Ctx {
        used: Vec::new(),
        output: output.min(3),
    };
    let frag = if chain.is_empty() {
        // An empty chain is black rather than a compile error: a half-typed
        // chain should not blank the widget with a diagnostic.
        "vec4<f32>(0.0, 0.0, 0.0, 1.0)".to_string()
    } else {
        fold(chain, "st", &mut ctx)
    };

    let mut helpers: Vec<Helper> = ctx.used.iter().flat_map(|f| f.helpers.iter().copied()).collect();
    helpers.sort();
    helpers.dedup();

    let mut out = String::with_capacity(4096);
    out.push_str(PREAMBLE);
    // `_mod` is unconditional: the noise helper calls it, and so do the repeat
    // and kaleid families.
    out.push_str(table::MOD_HELPERS);
    for helper in helpers {
        out.push_str(helper.wgsl());
    }
    for func in &ctx.used {
        let _ = write!(out, "\n{} {{{}\n}}\n", signature(func), func.wgsl);
    }
    let _ = write!(out, "{ENTRY_HEAD}    return {frag};\n}}\n");
    out
}

/// Uniforms and the full-screen triangle. Kept byte-compatible with the
/// `_shader` widget's block so one painter can feed either.
const PREAMBLE: &str = r#"
struct HydraUniforms {
    res: vec2<f32>,
    time: f32,
    gain: f32,
    note: f32,
    voices: f32,
};
@group(0) @binding(0) var<uniform> hu: HydraUniforms;
// The four output buffers, as they stood at the end of the previous frame.
// Always bound, so one pipeline layout serves every chain whether or not it
// reads a buffer.
@group(0) @binding(1) var hBuf0: texture_2d<f32>;
@group(0) @binding(2) var hBuf1: texture_2d<f32>;
@group(0) @binding(3) var hBuf2: texture_2d<f32>;
@group(0) @binding(4) var hBuf3: texture_2d<f32>;
@group(0) @binding(5) var hSamp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;
    var out: VsOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}
"#;

const ENTRY_HEAD: &str = r#"
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let st = in.uv;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str) -> &'static HydraFn {
        lookup(name).unwrap_or_else(|| panic!("{name} is in the table"))
    }

    #[test]
    fn a_source_alone_folds_to_one_call_with_its_defaults() {
        let chain = Chain::source(f("osc"), vec![]);
        let out = compile(&chain, 0);
        assert!(
            out.contains("return h_osc(st, 60.0, 0.1, 0.0);"),
            "unexpected fold:\n{out}"
        );
        // Only the function actually used is emitted.
        assert!(out.contains("fn h_osc("));
        assert!(!out.contains("fn h_noise("));
    }

    #[test]
    fn given_arguments_win_over_defaults_and_short_calls_are_padded() {
        let chain = Chain::source(f("osc"), vec![Arg::Number(10.0)]);
        assert!(compile(&chain, 0).contains("h_osc(st, 10.0, 0.1, 0.0)"));
    }

    #[test]
    fn a_coord_transform_wraps_inward_and_a_color_transform_wraps_outward() {
        // This is the whole composition rule: `rotate` rewrites the coordinate
        // `osc` is sampled at, while `color` wraps the colour `osc` returned.
        let chain = Chain::source(f("osc"), vec![Arg::Number(10.0)])
            .then(f("rotate"), vec![Arg::Number(0.5)])
            .then(
                f("color"),
                vec![Arg::Number(1.0), Arg::Number(0.0), Arg::Number(0.0)],
            );
        let out = compile(&chain, 0);
        assert!(
            out.contains(
                "return h_color(h_osc(h_rotate(st, 0.5, 0.0), 10.0, 0.1, 0.0), 1.0, 0.0, 0.0, 1.0);"
            ),
            "unexpected fold:\n{out}"
        );
    }

    #[test]
    fn a_combine_takes_the_other_chain_as_its_first_argument() {
        let chain = Chain::source(f("osc"), vec![]).then(
            f("add"),
            vec![Arg::Chain(Chain::source(f("noise"), vec![]))],
        );
        let out = compile(&chain, 0);
        assert!(
            out.contains("return h_add(h_osc(st, 60.0, 0.1, 0.0), h_noise(st, 10.0, 0.1), 1.0);"),
            "unexpected fold:\n{out}"
        );
        // The nested chain pulls its own helper in.
        assert!(out.contains("fn _noise("));
    }

    #[test]
    fn a_modulate_warps_the_coordinate_with_another_chain() {
        // `combineCoord` is the awkward one: the modulating chain is evaluated
        // at `st`, its result warps the coordinate, and *then* everything
        // before the modulate is evaluated at that warped coordinate.
        let chain = Chain::source(f("osc"), vec![]).then(
            f("modulate"),
            vec![Arg::Chain(Chain::source(f("noise"), vec![])), Arg::Number(0.2)],
        );
        let out = compile(&chain, 0);
        assert!(
            out.contains(
                "return h_osc(h_modulate(st, h_noise(st, 10.0, 0.1), 0.2), 60.0, 0.1, 0.0);"
            ),
            "unexpected fold:\n{out}"
        );
    }

    #[test]
    fn helpers_are_emitted_once_and_only_when_used() {
        // `luma` and `mask` both call `_luminance`.
        let chain = Chain::source(f("osc"), vec![])
            .then(f("luma"), vec![])
            .then(
                f("mask"),
                vec![Arg::Chain(Chain::source(f("shape"), vec![]))],
            );
        let out = compile(&chain, 0);
        assert_eq!(out.matches("fn _luminance(").count(), 1);
        assert!(!out.contains("fn _rgbToHsv("), "unused helper emitted");
        // A function used twice is only defined once either way.
        let twice = compile(
            &Chain::source(f("osc"), vec![]).then(
                f("add"),
                vec![Arg::Chain(Chain::source(f("osc"), vec![]))],
            ),
            0,
        );
        assert_eq!(twice.matches("fn h_osc(").count(), 1);
    }

    #[test]
    fn an_empty_chain_compiles_to_black_rather_than_an_error() {
        // Half-typed chains reach this on every keystroke.
        let out = compile(&Chain::default(), 0);
        assert!(out.contains("return vec4<f32>(0.0, 0.0, 0.0, 1.0);"));
    }

    #[test]
    fn whole_numbers_reach_wgsl_as_floats() {
        // `h_osc(st, 10, ...)` is a type error in WGSL, not a rounding question.
        assert_eq!(wgsl_f32(10.0), "10.0");
        assert_eq!(wgsl_f32(0.5), "0.5");
        assert_eq!(wgsl_f32(-3.0), "-3.0");
        assert_eq!(wgsl_f32(f64::NAN), "0.0");
    }
}
