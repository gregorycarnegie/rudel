# Vendored: dough

`dough.h` is the dough synth engine by Felix Roos and contributors,
<https://codeberg.org/uzu/dough>, licensed AGPL-3.0-or-later (the same licence
as Rudel; see the repository's `LICENSE`).

- Taken from the npm package `dough-synth` version **0.4.1**, the version
  Strudel's `packages/dough` pins.
- SHA-256 of the file as published:
  `02492d266f5eb804f2bf86b0566af762459e07349ae323cbe25aee8bf4222313`

## What it is for

Rudel plays `.dough()` through a Rust port of this engine (`../src/dsp.rs`,
`engine.rs`, `event.rs`); no C is built for the app. This file is the reference
the port is held to. With the `c-reference` feature, `../build.rs` compiles it
through `../src/dough.c`, with no audio back end (neither `DOUGH_MINIAUDIO` nor
`DOUGH_PORTAUDIO`), for:

- `tests/c_reference.rs`, which renders random scenarios on both engines and
  requires every sample to match to the bit;
- `benches/engine.rs`, which times the two.

## Modification

This copy differs from the published file in one way: two comment lines near
the top (a URL and a marker string, lines 5 and 6 as published) were removed on
2026-11-15. No code was changed.

## Updating

Replace `dough.h` with the new version's file, remove the same two comment
lines, and update the version and hash above. Then port the engine's changes to
the Rust, regenerate `tools/oracle/dough_golden.json` from the new
`dough.wasm`, and run `cargo test -p rudel-dough --features c-reference` until
both comparisons pass.
