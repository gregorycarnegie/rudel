# rudel

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue)](https://www.gnu.org/licenses/agpl-3.0.en.html)
[![CI](https://github.com/gregorycarnegie/rudel/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/gregorycarnegie/rudel/actions/workflows/ci.yml)
[![Rust edition: 2024](https://img.shields.io/badge/rust%20edition-2024-orange)](Cargo.toml)
[![MSRV: 1.96](https://img.shields.io/badge/MSRV-1.96-orange)](Cargo.toml)
[![Koto: 0.16.1](https://img.shields.io/badge/Koto-0.16.1-blue)](https://koto.dev)
[![Csound: optional at runtime](https://img.shields.io/badge/Csound-optional%20at%20runtime-blue)](#csound)
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

## Mondo Notation

[Mondo Notation](https://strudel.cc/learn/mondo-notation/) is Strudel's
Lisp-like way of writing the same patterns, and Rudel speaks it. Put `// mondo`
on the first line and the whole script is read as mondo:

```
// mondo
$ s [bd rim [~ bd] rim] # bank tr707
$ n <0 2 4 [3 1] -1>*4 # scale C4:minor # jux rev # dec .2 # delay .5
```

Round parens call a function, `#` chains one call onto the last, and `$`
separates patterns into a stack. That is this, in Koto:

```koto
stack(
  s("bd rim [~ bd] rim").bank("tr707"),
  n("<0 2 4 [3 1] -1>*4").scale("C4:minor").jux(rev).dec(0.2).delay(0.5)
)
```

To reach for mondo inside an otherwise-Koto script, tag a single pattern with
it instead: `` mondo`s hh*8` ``.

Mondo is compiled to Koto rather than interpreted, so every control, transform
and signal Rudel exposes is reachable from it. Its two limits — `:`/`..` want
literal operands, and `def` binds values rather than functions — are in
[`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md#mondo-strudelmondo-strudelmondough--supported).

## Csound

Rudel can play a pattern on a [Csound](https://csound.com/) instrument instead of
one of its own voices, the way `@strudel/csound` does — `loadCsound`, `loadOrc`,
and the `csound` / `csoundm` outputs.

This is the one feature with an outside dependency. There is no pure-Rust
Csound, and the WebAssembly build Strudel loads in the browser needs a browser to
run, so Rudel uses the native library. **Nothing links against it**: Rudel builds
and runs exactly as before without Csound, the library is opened only when a
script first asks for it, and a script that asks when it is not there gets an
error saying so while the rest of the pattern keeps playing.

To get started, install Csound ([downloads](https://csound.com/download.html);
the 64-bit build, which is what the installers give you), then paste this in and
press **Play**:

```koto
loadCsound`
instr Beep
    asig = vco2(p5, p4)
    asig *= linsegr:a(0, .01, 1, p3, 1, .1, 0)
    out(asig, asig)
endin`

note("c3 e3 g3 c4").csound('Beep')
```

`loadOrc('github:user/repo/branch/some.orc')` loads an orchestra from a URL
instead. An orchestra that will not compile reports Csound's own message — the
line number and the offending source — in the app's error panel.

Rudel looks for `csound64.dll` / `libcsound64.so` / `libcsound64.dylib` on the
usual library path, then in the default install locations. Set
`RUDEL_CSOUND_LIB` to a full path to override that, which is also all an
unpacked build needs. Details, including how Csound is mixed in, are in
[`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md#csound-strudelcsound--supported-with-csound-installed-separately).

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

Install mini-notation when you want strings to parse like Strudel patterns.
As in Strudel, **double-quoted** strings are mini-notation and single-quoted
strings are plain strings:

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

Some Strudel packages bridge to browser-only platform APIs, and one — the
experimental TidalCycles interpreter — is an alternative source language rather
than a notation over the one Rudel has; these are intentionally not ported. See
[`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md) for the authoritative list of
unsupported and intentionally different features.

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p rudel-lang   # performance benchmarks over representative patterns
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the test +
clippy checks on every push and PR; the suite includes the parity drift guards
(reference surface, per-package API inventory, and mini/tonal goldens).

Parity oracle notes live in [`tools/oracle/README.md`](tools/oracle/README.md).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, check commands, parity
oracle guidance, and contribution conventions.
