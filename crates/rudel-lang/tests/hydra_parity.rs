//! The hydra port, checked against the pinned reference and against WGSL.
//!
//! Two independent guards, because the port can go wrong in two ways:
//!
//! - **Signature drift.** `tools/oracle/hydra_golden.json` is hydra-synth
//!   1.3.29's own function table. A chain written for hydra has to mean the
//!   same thing here, so names, composition types, input names and input
//!   defaults are compared entry for entry. Regenerate with
//!   `cd tools/oracle && node gen_hydra_oracle.mjs`.
//! - **Mistranslation.** The GLSL bodies are transliterated by hand, and WGSL
//!   is stricter than GLSL about scalar/vector mixing. Every function is
//!   compiled inside a real chain and put through `naga`, so a body that would
//!   not compile fails here rather than on someone's GPU mid-set.

use rudel_lang::hydra::{self, Arg, Chain, FnType, HydraFn};
use serde_json::Value;

fn golden() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/oracle/hydra_golden.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("hydra_golden.json is valid JSON")
}

fn golden_functions(g: &Value) -> &Vec<Value> {
    g["functions"].as_array().expect("functions array")
}

#[test]
fn every_hydra_function_is_implemented_or_accounted_for() {
    let g = golden();
    let mut unaccounted = Vec::new();
    for entry in golden_functions(&g) {
        let name = entry["name"].as_str().expect("name");
        if hydra::lookup(name).is_some() {
            continue;
        }
        if hydra::UNIMPLEMENTED.iter().any(|(n, _)| *n == name) {
            continue;
        }
        unaccounted.push(name.to_string());
    }
    assert!(
        unaccounted.is_empty(),
        "hydra functions neither implemented nor listed in `hydra::UNIMPLEMENTED`: {unaccounted:?}"
    );
}

#[test]
fn implemented_functions_match_the_pinned_signature() {
    let g = golden();
    for entry in golden_functions(&g) {
        let name = entry["name"].as_str().expect("name");
        let Some(ours) = hydra::lookup(name) else {
            continue;
        };

        assert_eq!(
            ours.ty.as_str(),
            entry["type"].as_str().expect("type"),
            "{name}: composition type differs, so it would fold into the wrong shader"
        );

        let theirs = entry["inputs"].as_array().expect("inputs");
        assert_eq!(
            ours.inputs.len(),
            theirs.len(),
            "{name}: takes {} inputs upstream, {} here",
            theirs.len(),
            ours.inputs.len()
        );
        for (ours, theirs) in ours.inputs.iter().zip(theirs) {
            assert_eq!(
                ours.name,
                theirs["name"].as_str().expect("input name"),
                "{name}: input name differs"
            );
            // `src` is the one non-float input that is ported: upstream types
            // it `sampler2D` and passes an output object, where an output here
            // is its index. Everything else has to be a float, or the port is
            // quietly reinterpreting a parameter.
            if name == "src" {
                assert_eq!(theirs["type"].as_str(), Some("sampler2D"));
                continue;
            }
            assert_eq!(
                theirs["type"].as_str(),
                Some("float"),
                "{name}: only float inputs are ported; {} is not one",
                ours.name
            );
            let default = theirs["default"].as_f64().unwrap_or_else(|| {
                panic!("{name}: {} has a non-numeric default", ours.name)
            });
            assert_eq!(
                ours.default, default,
                "{name}: {} defaults to {default} upstream",
                ours.name
            );
        }
    }
}

#[test]
fn nothing_is_implemented_that_hydra_does_not_have() {
    // An invented function would compile and run, and would silently not exist
    // in hydra — the worst kind of divergence, because it only shows up when
    // someone moves a patch the other way.
    let g = golden();
    let known: Vec<&str> = golden_functions(&g)
        .iter()
        .map(|e| e["name"].as_str().expect("name"))
        .collect();
    for ours in hydra::functions() {
        assert!(
            known.contains(&ours.name),
            "`{}` is not a hydra function",
            ours.name
        );
    }
    for (name, _) in hydra::UNIMPLEMENTED {
        assert!(
            known.contains(name),
            "`{name}` is listed as unimplemented but hydra has no such function"
        );
    }
}

/// A chain that exercises `func`, whatever its type.
fn exercising(func: &'static HydraFn) -> Chain {
    let osc = hydra::lookup("osc").expect("osc");
    let shape = hydra::lookup("shape").expect("shape");
    match func.ty {
        FnType::Src => Chain::source(func, vec![]),
        FnType::Coord | FnType::Color => Chain::source(osc, vec![]).then(func, vec![]),
        // The first argument of a combine is the other chain.
        FnType::Combine | FnType::CombineCoord => Chain::source(osc, vec![]).then(
            func,
            vec![Arg::Chain(Chain::source(shape, vec![]))],
        ),
    }
}

fn check_wgsl(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map(|_| ())
    .map_err(|e| e.emit_to_string(source))
}

#[test]
fn every_function_compiles_as_wgsl() {
    let mut broken = Vec::new();
    for func in hydra::functions() {
        let source = hydra::compile(&exercising(func), 0);
        if let Err(error) = check_wgsl(&source) {
            broken.push(format!("--- {} ---\n{error}", func.name));
        }
    }
    assert!(
        broken.is_empty(),
        "{} hydra function(s) do not compile:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

#[test]
fn a_long_mixed_chain_compiles() {
    // Every composition type at once, nested two deep, which is where the fold
    // is most likely to emit something that parses but does not type-check.
    let f = |name: &str| hydra::lookup(name).unwrap_or_else(|| panic!("{name}"));
    let chain = Chain::source(f("osc"), vec![Arg::Number(20.0), Arg::Number(0.1)])
        .then(f("rotate"), vec![Arg::Number(0.4)])
        .then(
            f("modulate"),
            vec![
                Arg::Chain(
                    Chain::source(f("noise"), vec![Arg::Number(3.0)]).then(f("kaleid"), vec![]),
                ),
                Arg::Number(0.3),
            ],
        )
        .then(f("colorama"), vec![Arg::Number(0.02)])
        .then(f("luma"), vec![])
        .then(
            f("add"),
            vec![
                Arg::Chain(Chain::source(f("voronoi"), vec![]).then(f("thresh"), vec![])),
                Arg::Number(0.6),
            ],
        );
    let source = hydra::compile(&chain, 0);
    check_wgsl(&source).unwrap_or_else(|e| panic!("mixed chain does not compile:\n{e}\n{source}"));
}

#[test]
fn prev_reads_the_buffer_its_chain_is_bound_to() {
    // `prev()` means "this output, last frame", so the same chain compiled for
    // a different output has to read a different buffer. Nothing else in the
    // module depends on which output it is.
    let prev = hydra::lookup("prev").expect("prev");
    for output in 0..4 {
        let source = hydra::compile(&Chain::source(prev, vec![]), output);
        assert!(
            source.contains(&format!("h_src(st, {output}.0)")),
            "output {output} did not bind to its own buffer:
{source}"
        );
        check_wgsl(&source).unwrap_or_else(|e| panic!("output {output}:
{e}"));
    }
    // Out-of-range clamps rather than emitting a read of a buffer that is not
    // bound, which would be a compile error in the generated module.
    assert!(hydra::compile(&Chain::source(prev, vec![]), 9).contains("h_src(st, 3.0)"));
}

#[test]
fn the_pinned_version_is_recorded() {
    // Strudel loads `https://unpkg.com/hydra-synth` with no version in the URL,
    // so what a user actually runs is whatever is latest that day — the
    // reference is a moving target by construction. Pinning it here at least
    // makes the movement a deliberate, reviewable change: 1.4.0 altered
    // `shift`'s body, and porting 1.3.29's would have been wrong for anyone
    // running Strudel today.
    let g = golden();
    assert_eq!(
        g["version"].as_str(),
        Some("1.4.0"),
        "hydra_golden.json was regenerated against a different hydra-synth"
    );
}
