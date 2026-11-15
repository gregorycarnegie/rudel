// Compile the vendored dough engine (vendor/dough.h) into this crate.
// SPDX-License-Identifier: AGPL-3.0-or-later

fn main() {
    println!("cargo:rerun-if-changed=vendor/dough.h");
    println!("cargo:rerun-if-changed=src/dough.c");
    cc::Build::new()
        .file("src/dough.c")
        .include("vendor")
        // `M_PI`, which the Windows CRT's <math.h> only defines on request.
        .define("_USE_MATH_DEFINES", None)
        // Upstream builds it as GNU C99; MSVC ignores the flag.
        .flag_if_supported("-std=gnu99")
        // No fused multiply-adds: WebAssembly has none, and a compiler that
        // fuses `a * b + c` (clang on ARM does by default) rounds once where
        // the browser rounds twice, so the output would drift from it.
        .flag_if_supported("-ffp-contract=off")
        // Third-party code: its warnings are not ours to act on.
        .warnings(false)
        .compile("dough");
}
