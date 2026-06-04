# rudel

A Rust fork of [Strudel](https://codeberg.org/uzu/strudel) (itself the JS port of
[TidalCycles](https://tidalcycles.org/)): live-coding algorithmic music patterns,
natively, with a Koto scripting layer and native audio.

> Licensed under **AGPL-3.0-or-later**, the same as Strudel. Sound bank licensing
> follows the source samples you load.

## Workspace

| Crate | Role |
|-------|------|
| [`rudel-core`](crates/rudel-core) | The pattern engine: `Pattern = State -> Vec<Hap>`, functor/applicative/monad combinators, ~90 transforms, signals (with Strudel's legacy RNG), controls, Euclidean rhythms. Time is exact rational (`Ratio<i128>`). |
| [`rudel-mini`](crates/rudel-mini) | Mini-notation parser (a `pest` grammar ported from Strudel's `krill.pegjs`): `"bd [hh hh] <sd cp>*2"`. |
| [`rudel-dsp`](crates/rudel-dsp) | Synthesis voices: oscillators + ADSR + pan + biquad low-pass, and a sample-playback voice. |
| [`rudel-audio`](crates/rudel-audio) | Lookahead scheduler (cycle↔sample clock) + `cpal` output, a `SampleBank`, and `fundsp` reverb / delay effects. |
| [`rudel-lang`](crates/rudel-lang) | [Koto](https://koto.dev) bindings exposing the builder API for live evaluation. |
| [`rudel-app`](crates/rudel-app) | Native `egui` editor: type Koto, Ctrl+Enter to hot-swap the pattern, with a one-cycle visualizer. |

## Run the app

```bash
cargo run -p rudel    # the rudel-app binary
```

Type a pattern in the editor and press **Ctrl+Enter** (then **Play**):

```koto
stack(
  note("c4 e4 g4 b4").s("triangle").room(0.5),
  note("c2 ~ g2 ~").s("saw").cutoff("400 1600").gain(0.6).delay(0.3)
)
```

## Examples

```bash
cargo run -p rudel-audio --example play      # synth demo
cargo run -p rudel-audio --example live -- 'note("c e g").fast(2).room(0.4)'
cargo run -p rudel-audio --example samples -- path/to/samples   # subfolders = sound names
```

## Using the library

```rust
use rudel_core::*;
rudel_mini::install();                  // make &str parse as mini-notation

let pat = note("c3 [e3 g3] <c4 e4>")
    .s("piano")
    .jux(|p| p.rev())
    .every(4, |p| p.fast(2))
    .gain(0.8);
```

## Status

Phases 0–6 complete: engine → mini-notation → voices → scheduler/audio →
samples/effects → Koto live-eval → egui app. ~67 tests, clippy-clean.

Not yet ported: higher-order Koto combinators (`every`/`jux` from script),
sample-manipulation transforms (`chop`/`striate`/`loopAt`), the `tonal`/scale
family, and MIDI/OSC I/O.

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
