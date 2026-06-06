# Contributing to Rudel

Thanks for helping move Rudel along. This is a Rust workspace, and most changes
should stay scoped to the crate that owns the behavior.

## Setup

Rudel uses Rust edition 2024 and the workspace `rust-version` is `1.92`.

```bash
cargo test --workspace
```

For the native app, release mode is recommended because it runs the real-time
audio path:

```bash
cargo run --release -p rudel-app
```

## Workspace Map

- `crates/rudel-core`: pure pattern engine, transforms, controls, signals,
  tonal helpers, sample transforms, and event extraction.
- `crates/rudel-mini`: mini-notation parser and Strudel parity tests.
- `crates/rudel-dsp`: synth, drum, sampler, filter, and post-effect voices.
- `crates/rudel-audio`: `cpal` audio engine, scheduler, mixer, and sample bank.
- `crates/rudel-lang`: Koto bindings for live evaluation.
- `crates/rudel-midi`: MIDI event mapping and real-time output.
- `crates/rudel-osc`: SuperDirt-compatible OSC output.
- `crates/rudel-app`: native `egui` live-coding app.

## Checks

Run the narrowest useful test while iterating, then the workspace checks before
wrapping up:

```bash
cargo test -p rudel-core
cargo test -p rudel-mini
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Use `cargo fmt` after code changes:

```bash
cargo fmt --all
```

## Parity Tests

Rudel keeps Strudel parity goldens for RNG/signals, mini-notation, and selected
core transforms. The committed goldens are used by:

- `crates/rudel-core/tests/parity_oracle.rs`
- `crates/rudel-mini/tests/mini_parity.rs`
- `crates/rudel-mini/tests/transform_parity.rs`

If a change intentionally alters parity behavior, regenerate or update the
relevant goldens and explain why. The oracle workflow is documented in
[`tools/oracle/README.md`](tools/oracle/README.md).

## Coding Style

- Prefer the existing pattern APIs and crate boundaries over new abstractions.
- Keep pure pattern behavior in `rudel-core`; keep rendering and device I/O out
  of the core crate.
- Put scheduler-neutral event extraction in `rudel-core::query` so audio, MIDI,
  and OSC see the same events.
- Add focused tests for new transforms, controls, parser behavior, voices, and
  output mappings.
- Keep real-time audio code allocation-light in the callback path.
- Use concise comments where they clarify timing, parity, or DSP decisions.

## Documentation

Update the relevant crate README when a crate's public role, examples, or
supported controls change. Update the root README when workspace-level behavior,
app usage, or support status changes.

## Samples and Licensing

Rudel is AGPL-3.0-or-later. Loaded sample banks keep their own source licenses;
do not add sample assets unless their license is clear and compatible with the
intended use.
