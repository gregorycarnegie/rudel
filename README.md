# rudel

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue)](https://www.gnu.org/licenses/agpl-3.0.en.html)
[![Rust edition: 2024](https://img.shields.io/badge/rust%20edition-2024-orange)](Cargo.toml)
[![MSRV: 1.92](https://img.shields.io/badge/MSRV-1.92-orange)](Cargo.toml)
[![Workspace: 8 crates](https://img.shields.io/badge/workspace-8%20crates-informational)](#workspace)
[![Checks: test + clippy](https://img.shields.io/badge/checks-test%20%2B%20clippy-brightgreen)](#tests)

Rudel is a native Rust fork of [Strudel](https://codeberg.org/uzu/strudel)
(itself the JS port of [TidalCycles](https://tidalcycles.org/)): live-coded,
algorithmic music patterns with a Koto scripting layer, native audio, MIDI out,
and SuperDirt-compatible OSC out.

> Licensed under **AGPL-3.0-or-later**, the same as Strudel. Sound bank licensing
> follows the source samples you load.

## Workspace

| Crate                               | Role                                                                                                                                                                                            |
|-------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| [`rudel-core`](crates/rudel-core)   | Pure pattern engine: `Pattern = State -> Vec<Hap>`, exact rational time, combinators, controls, signals, sample transforms, tonal helpers, and scheduler-neutral event extraction.              |
| [`rudel-mini`](crates/rudel-mini)   | `pest` mini-notation parser ported from Strudel's `krill.pegjs`: sequences, groups, rests, alternation, stacks, choices, Euclidean rhythms, ranges, polymeter, degradation, and sample indices. |
| [`rudel-dsp`](crates/rudel-dsp)     | Offline-testable voices: synth oscillators, noise, built-in drums, sampler playback, filters, envelopes, panning, and per-voice post effects.                                                   |
| [`rudel-audio`](crates/rudel-audio) | Real-time audio engine: lookahead scheduler, `cpal` output, sample bank loading, mixer, delay, and `fundsp` reverb.                                                                             |
| [`rudel-lang`](crates/rudel-lang)   | [Koto](https://koto.dev) bindings for Rudel patterns, controls, signals, factories, higher-order callbacks, sample transforms, and tonal operations.                                            |
| [`rudel-midi`](crates/rudel-midi)   | MIDI output: control-map to note/CC/program messages, timed windows, port wrapper, and real-time scheduler.                                                                                     |
| [`rudel-osc`](crates/rudel-osc)     | SuperDirt OSC output: hand-rolled OSC 1.0 encoding, `/dirt/play` messages, UDP sender, and real-time scheduler.                                                                                 |
| [`rudel-app`](crates/rudel-app)     | Native `egui` editor with Koto live evaluation, audio/MIDI/OSC output selection, sample loading, and a one-cycle visualizer grouped by orbit.                                                   |

## Run the app

```bash
cargo run --release -p rudel-app
```

Type a pattern in the editor, press **Ctrl+Enter** to evaluate, then press
**Play**:

```koto
stack(
  s("bd ~ bd bd").gain(0.9),
  s("~ sd ~ sd"),
  s("hh*8").gain(0.5),
  note("c4 e4 g4 b4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").lpf("400 1600").gain(0.6).delay(0.3)
)
```

The app starts with native audio. Use the output selector for MIDI or OSC; OSC
defaults to `127.0.0.1:57120` for local SuperDirt.

## Examples

```bash
cargo run -p rudel-audio --example play
cargo run -p rudel-audio --example live -- 'note("c e g").fast(2).room(0.4)'
cargo run -p rudel-audio --example samples -- path/to/samples
```

For sample folders, immediate subdirectories become sound names and files inside
them become sample indices.

## Using the library

```rust
use rudel_core::*;

let pat = note(seq([60, 64, 67, 71]))
    .s("triangle")
    .jux(|p| p.rev())
    .every(4, |p| p.fast(2))
    .gain(0.8);
```

Install mini-notation when you want strings to parse like Strudel patterns:

```rust
rudel_mini::install();

let pat = note("c3 [e3 g3] <c4 e4>")
    .s("saw")
    .chop(4)
    .room(0.4);
```

## Current Status

Rudel has a usable native live-coding path today: pattern engine, mini-notation,
synth/drum/sample audio, effects, Koto live evaluation, an `egui` app, MIDI out,
and SuperDirt-compatible OSC out. The core, mini parser, transforms, audio event
scheduling, MIDI, OSC, and Koto bindings are covered by unit, integration, and
Strudel parity tests.

Still evolving: richer synth families, more Strudel sample-bank loading modes,
MIDI input/clock-in, per-pattern routing helpers, deeper editor ergonomics, and
the long tail of Strudel/Tidal compatibility.

Some Strudel packages bridge to browser-only platform APIs or provide alternative
language front-ends and are intentionally not ported. See
[`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md) for the authoritative list of
unsupported and intentionally different features.

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Parity oracle notes live in [`tools/oracle/README.md`](tools/oracle/README.md).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, check commands, parity
oracle guidance, and contribution conventions.
