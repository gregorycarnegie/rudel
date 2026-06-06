# rudel-core

Pure pattern engine for Rudel.

`rudel-core` owns the scheduler-neutral model: `Pattern`, `State`, `Hap`,
`TimeSpan`, `Value`, exact rational cycle time, pattern combinators, transforms,
controls, signals, Euclidean rhythms, tonal helpers, sample-manipulation
transforms, and event extraction shared by audio, MIDI, and OSC.

## Highlights

- `Pattern = State -> Vec<Hap>` with functor, applicative, and monadic-style
  composition.
- Exact time via `Frac` (`Ratio<i128>` underneath).
- Controls such as `note`, `n`, `s`, `gain`, `pan`, filters, envelopes,
  effects, sample range controls, MIDI controls, and aliases.
- Time/value transforms including `fast`, `slow`, `rev`, `every`, `jux`,
  `within`, `chunk`, `mask`, `struct_pat`, `range`, `echo`, `swing`, and the
  alignment matrix.
- Signals and deterministic Strudel-style randomness: `rand`, `perlin`, `sine`,
  `saw`, `square`, `irand`, `run`, and bipolar variants.
- Tonal helpers: note names, scales, transposition, scale transposition, and
  chord expansion.
- Sample transforms: `chop`, `striate`, `slice`, `splice`, `loop_at`, and `fit`.

## Example

```rust
use rudel_core::*;

let pat = note(seq([60, 64, 67, 71]))
    .s("triangle")
    .gain(0.8)
    .jux(|p| p.rev())
    .every(4, |p| p.fast(2));

let haps = pat.query_arc(Frac::zero(), Frac::one());
```

For mini-notation strings such as `"bd [hh hh]"`, install `rudel-mini`'s string
parser in the host crate before building string patterns.

## Tests

```bash
cargo test -p rudel-core
```

This crate also includes Strudel parity checks for randomness and analytic
signals in `tests/parity_oracle.rs`.
