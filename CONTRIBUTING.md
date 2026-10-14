# Contributing to Rudel

Thanks for helping move Rudel along. This is a Rust workspace, and most changes
should stay scoped to the crate that owns the behavior.

## Setup

Rudel uses Rust edition 2024 and the workspace `rust-version` is `1.99`.

```bash
cargo test --workspace
```

No CMake, and nothing to install for the codecs: LAME and libvorbis build from
vendored C with `cc` alone (autotools on Linux, which the runner images carry),
and FLAC and Opus are pure Rust.

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
- `crates/rudel-lang`: JavaScript bindings (Boa) for live evaluation.
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

CI runs the suite under [`cargo nextest`][nextest], which gives each test its own
process. The engine keeps real global state — `set_string_parser`, the soundfont
statics, the scope registry — so a test that only passes because a neighbour set
something up fails there rather than hiding behind a shared process. Worth
running locally before a push if you touched any of that:

```bash
cargo nextest run --workspace
```

The workspace has no doctests, so nextest not running them costs nothing.

[nextest]: https://nexte.st/

## Coverage and assertions

Linux CI runs the workspace with `cargo-llvm-cov` and nextest and uploads a
`coverage` artifact containing LCOV and a browsable HTML report. To reproduce
locally (or replace `--workspace` with `-p rudel-dsp` for a smaller report):

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
cargo llvm-cov nextest --workspace --profile coverage --html
```

Open `target/llvm-cov/html/index.html`. Coverage identifies lines to investigate;
mutation tests check whether the assertions detect wrong behavior. There is no
arbitrary percentage gate: hardware and platform paths differ across runners.
The nextest `coverage` profile allows extra time for instrumented DSP tests.
Stable LLVM coverage reports lines and regions; branch coverage needs nightly
(`cargo +nightly llvm-cov nextest --workspace --branch --html`).

Use [`googletest`](https://docs.rs/googletest/) where collection diagnostics or
floating-point matchers help, as in `rudel-dsp/src/fft.rs` and `bus.rs`.
`assert_that!` works in normal tests, including `rstest`; use `#[gtest]` with
`expect_that!` when independent assertions should all report their failures.
Keep explicit tolerances for DSP comparisons. Ordinary `assert_eq!` remains
appropriate for simple exact values.

## Parser fuzzing

`fuzz/` is a separate workspace so libFuzzer and nightly are not dependencies of
the app. Its `mini` target checks parsing, editor offsets and UTF-8 leaf spans;
`bytebeat` parses and evaluates expressions, including non-finite sample times.
The mini target deliberately stops at pattern construction: arbitrary speed and
replication counts can make querying produce unbounded numbers of events.

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked
mkdir -p fuzz/corpus/mini fuzz/corpus/bytebeat
cargo +nightly fuzz run mini fuzz/corpus/mini fuzz/seeds/mini -- -max_total_time=60 -max_len=4096
cargo +nightly fuzz run bytebeat fuzz/corpus/bytebeat fuzz/seeds/bytebeat -- -max_total_time=60 -max_len=4096
```

These commands use Bash (Linux/WSL is the simplest setup). The first corpus
directory receives generated inputs; the committed seeds are read-only inputs.
Windows needs a compatible AddressSanitizer runtime on PATH; if its runtime or
linker cannot launch the targets, use Linux/WSL rather than disabling sanitizers.
The Parser fuzzing workflow runs on relevant pull requests, weekly and manually,
and uploads corpora and crash reproducers. Reproduce a crash with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<crash-file>`, then add
a small regression test to the owning crate before fixing it. A one-minute run
is a smoke check, not evidence that the parser is crash-free.

Other test tools are intentionally conditional:

| Tool | Fit here |
| --- | --- |
| `insta` | Existing committed Strudel oracle goldens already cover structured output; avoid duplicating them with self-generated snapshots. |
| Miri | Most unsafe code calls FFmpeg, Csound or OS APIs, which Miri cannot execute. Use it for a future isolated Rust unsafe helper rather than claiming FFI coverage. |
| `loom` | The scope ring permits a reader to observe a partial update; it is not a consistent snapshot. Add a model if that synchronization contract changes, without replacing production atomics just to add a tool. |
| `trybuild` | Add when compile-time API restrictions or macros need negative tests; current tests exercise runtime APIs. |

## Mutation Testing

Test suites here are graded with [`cargo-mutants`][mutants]: it edits the code
and expects a test to fail. It is how the "asserts a signal came out" tests were
found — every arithmetic operator swap satisfied them.

```bash
cargo mutants -j8 --file "crates/rudel-dsp/src/synth.rs"
```

Two settings are not optional on this tree, and `.cargo/mutants.toml` sets both:

- `gitignore = true`, or the tree copy fails. The vendored `strudel/` is a nested
  git repo, so without it cargo-mutants copies `node_modules` and Windows refuses
  the npm symlinks (`os error 1314`, at "0 mutants tested").
- `test_tool = "nextest"` to match CI.

Work one file at a time (`--file`) while iterating; that is minutes rather than
the ~10 hours a whole-workspace run takes. For a full run, shard it:
`--shard 0/4 .. 3/4` with a separate `--output` per shard.

Reading the results: a whole-workspace run tests each mutant against only its own
package's tests, which overstates the gap wherever coverage lives downstream —
rudel-core's parity suites are in rudel-mini's test directory, and re-testing
with `--test-package rudel-core --test-package rudel-mini` moved a sample of its
"missed" mutants to caught. Treat `missed.txt` as a list to triage, not a verdict.

[mutants]: https://mutants.rs/

## Parity Tests

Rudel keeps committed goldens dumped from Strudel's real engine, so a change that
silently alters behaviour fails CI. They need no `strudel/` checkout to run — the
JSON is in the repo — and the generators live in
[`tools/oracle/`](tools/oracle/README.md).

- **Engine**: `rudel-core/tests/parity_oracle.rs` (RNG, signals),
  `rudel-mini/tests/{mini,transform,tonal,tune_table}_parity.rs`. The last three
  sit in rudel-mini because they need the parser, even though they cover
  rudel-core.
- **Audio**: `rudel-dsp/tests/*_golden.rs` (adsr, bytebeat, distortion, lfo,
  modenv, warp, worklet, zzfx) plus `rudel-dsp/src/tests/{supersaw,oscillator,
  postfx,filters}.rs`, which live inside the crate because they drive private
  per-sample functions directly rather than a whole voice.
- **Surface**: `rudel-lang/tests/{reference_parity,api_inventory,
  reference_snapshot,doc_examples,examples}.rs` guard the exposed names.

If a change intentionally alters parity behaviour, regenerate the relevant golden
and say why in the commit.

One golden is *not* a parity golden: `rudel-dsp/src/tests/drum_snapshot.json`.
The synthesized drums are a Rudel extension — Strudel plays samples there, so
there is nothing upstream to compare against — and it is generated from this
crate. It can only catch an unintended change to the drum voicing, never tell you
the voicing is right; the assertions next to it in `tests/drums.rs` are what say
that. Regenerate deliberately, after listening:

```bash
cargo test -p rudel-dsp --lib regenerate_drum_snapshot -- --ignored
```

## Testing the App

`rudel-app` is a binary crate, so its tests live in `#[cfg(test)]` modules rather
than a `tests/` directory. Whole-app tests that actually render a frame use
[`egui_kittest`][kittest] — see `src/app/ui_tests.rs`, which clicks the real
transport buttons and presses the real shortcuts against a headless harness.
Build the app for a test with `RudelApp::headless()`, which is `new()` without an
audio device.

[kittest]: https://docs.rs/egui_kittest/

## Coding Style

- Prefer the existing pattern APIs and crate boundaries over new abstractions.
- Keep pure pattern behavior in `rudel-core`; keep rendering and device I/O out
  of the core crate.
- Put scheduler-neutral event extraction in `rudel-core::query` so audio, MIDI,
  and OSC see the same events.
- Add focused tests for new transforms, controls, parser behavior, voices, and
  output mappings. For a voice, prefer comparing against an oracle over asserting
  that a signal came out: "it made a sound" is satisfied by almost any wrong
  arithmetic, which is what the mutation runs kept finding.
- Keep real-time audio code allocation-light in the callback path.
- Use concise comments where they clarify timing, parity, or DSP decisions.

## Documentation

Update the relevant crate README when a crate's public role, examples, or
supported controls change. Update the root README when workspace-level behavior,
app usage, or support status changes.

Add a [`CHANGELOG.md`](CHANGELOG.md) entry for anything a user would notice —
particularly a parity fix that changes how existing patterns sound, which wants a
migration note saying how to get the old behaviour back.

## Samples and Licensing

Rudel is AGPL-3.0-or-later. Loaded sample banks keep their own source licenses;
do not add sample assets unless their license is clear and compatible with the
intended use.
