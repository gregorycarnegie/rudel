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
//!
//! Each function is its own test: `accounted/`, `signature/`, `known/` and
//! `wgsl/` followed by its name.

use libtest_mimic::{Arguments, Failed, Trial};
use rudel_lang::hydra::{self, Arg, Chain, FnType, HydraFn};
use serde_json::Value;
use std::sync::Arc;

fn golden() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/oracle/hydra_golden.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("hydra_golden.json is valid JSON")
}

fn main() {
    let g = golden();
    let entries = g["functions"].as_array().expect("functions array").clone();
    let known: Arc<Vec<String>> = Arc::new(
        entries
            .iter()
            .map(|e| e["name"].as_str().expect("name").to_string())
            .collect(),
    );
    let version = g["version"].as_str().map(str::to_string);
    let mut trials = vec![
        Trial::test("the_pinned_version_is_recorded", move || {
            the_pinned_version_is_recorded(version.as_deref())
        }),
        Trial::test("a_long_mixed_chain_compiles", a_long_mixed_chain_compiles),
        Trial::test("prev_clamps_an_output_out_of_range", || {
            // Out-of-range clamps rather than emitting a read of a buffer that
            // is not bound, which would be a compile error in the module.
            let prev = hydra::lookup("prev").expect("prev");
            assert!(hydra::compile(&Chain::source(prev, vec![]), 9).contains("h_src(st, 3.0)"));
            Ok(())
        }),
    ];
    for output in 0..4 {
        trials.push(Trial::test(
            format!("prev_reads_output/{output}"),
            move || prev_reads_the_buffer_its_chain_is_bound_to(output),
        ));
    }
    for entry in entries {
        let name = entry["name"].as_str().expect("name").to_string();
        let accounted = name.clone();
        trials.push(Trial::test(format!("accounted/{name}"), move || {
            implemented_or_accounted_for(&accounted)
        }));
        if hydra::lookup(&name).is_some() {
            trials.push(Trial::test(format!("signature/{name}"), move || {
                implemented_function_matches_the_pinned_signature(&name, &entry)
            }));
        }
    }
    let ours = hydra::functions()
        .iter()
        .map(|f| f.name)
        .chain(hydra::UNIMPLEMENTED.iter().map(|(n, _)| *n));
    for name in ours {
        let known = known.clone();
        trials.push(Trial::test(format!("known/{name}"), move || {
            // An invented function would compile and run, and would silently
            // not exist in hydra — the worst kind of divergence, because it
            // only shows up when someone moves a patch the other way.
            assert!(
                known.iter().any(|k| k == name),
                "`{name}` is not a hydra function"
            );
            Ok(())
        }));
    }
    for func in hydra::functions() {
        trials.push(Trial::test(format!("wgsl/{}", func.name), move || {
            check_wgsl(&hydra::compile(&exercising(func), 0)).map_err(Failed::from)
        }));
    }
    libtest_mimic::run(&Arguments::from_args(), trials).exit();
}

fn implemented_or_accounted_for(name: &str) -> Result<(), Failed> {
    if hydra::lookup(name).is_some() || hydra::UNIMPLEMENTED.iter().any(|(n, _)| *n == name) {
        return Ok(());
    }
    Err("neither implemented nor listed in `hydra::UNIMPLEMENTED`".into())
}

fn implemented_function_matches_the_pinned_signature(
    name: &str,
    entry: &Value,
) -> Result<(), Failed> {
    let ours = hydra::lookup(name).expect("implemented");
    assert_eq!(
        ours.ty.as_str(),
        entry["type"].as_str().expect("type"),
        "composition type differs, so it would fold into the wrong shader"
    );
    let theirs = entry["inputs"].as_array().expect("inputs");
    assert_eq!(
        ours.inputs.len(),
        theirs.len(),
        "takes {} inputs upstream, {} here",
        theirs.len(),
        ours.inputs.len()
    );
    for (ours, theirs) in ours.inputs.iter().zip(theirs) {
        assert_eq!(
            ours.name,
            theirs["name"].as_str().expect("input name"),
            "input name differs"
        );
        // `src` is the one non-float input that is ported: upstream types it
        // `sampler2D` and passes an output object, where an output here is its
        // index. Everything else has to be a float, or the port is quietly
        // reinterpreting a parameter.
        if name == "src" {
            assert_eq!(theirs["type"].as_str(), Some("sampler2D"));
            continue;
        }
        assert_eq!(
            theirs["type"].as_str(),
            Some("float"),
            "only float inputs are ported; {} is not one",
            ours.name
        );
        let default = theirs["default"]
            .as_f64()
            .unwrap_or_else(|| panic!("{} has a non-numeric default", ours.name));
        assert_eq!(
            ours.default, default,
            "{} defaults to {default} upstream",
            ours.name
        );
    }
    Ok(())
}

/// A chain that exercises `func`, whatever its type.
fn exercising(func: &'static HydraFn) -> Chain {
    let osc = hydra::lookup("osc").expect("osc");
    let shape = hydra::lookup("shape").expect("shape");
    match func.ty {
        FnType::Src => Chain::source(func, vec![]),
        FnType::Coord | FnType::Color => Chain::source(osc, vec![]).then(func, vec![]),
        // The first argument of a combine is the other chain.
        FnType::Combine | FnType::CombineCoord => {
            Chain::source(osc, vec![]).then(func, vec![Arg::Chain(Chain::source(shape, vec![]))])
        }
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

/// Every composition type at once, nested two deep, which is where the fold is
/// most likely to emit something that parses but does not type-check.
fn a_long_mixed_chain_compiles() -> Result<(), Failed> {
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
    check_wgsl(&source).map_err(|e| format!("mixed chain does not compile:\n{e}\n{source}").into())
}

/// `prev()` means "this output, last frame", so the same chain compiled for a
/// different output has to read a different buffer. Nothing else in the module
/// depends on which output it is.
fn prev_reads_the_buffer_its_chain_is_bound_to(output: usize) -> Result<(), Failed> {
    let prev = hydra::lookup("prev").expect("prev");
    let source = hydra::compile(&Chain::source(prev, vec![]), output);
    assert!(
        source.contains(&format!("h_src(st, {output}.0)")),
        "did not bind to its own buffer:\n{source}"
    );
    check_wgsl(&source).map_err(Failed::from)
}

/// Strudel loads `https://unpkg.com/hydra-synth` with no version in the URL, so
/// what a user actually runs is whatever is latest that day — the reference is
/// a moving target by construction. Pinning it here at least makes the
/// movement a deliberate, reviewable change: 1.4.0 altered `shift`'s body, and
/// porting 1.3.29's would have been wrong for anyone running Strudel today.
fn the_pinned_version_is_recorded(version: Option<&str>) -> Result<(), Failed> {
    assert_eq!(
        version,
        Some("1.4.0"),
        "hydra_golden.json was regenerated against a different hydra-synth"
    );
    Ok(())
}
