# rudel-lang

Koto scripting bindings for live-coding Rudel patterns.

`rudel-lang` evaluates Koto scripts into `rudel-core::Pattern` values. It is the
live layer used by `rudel-app` and the `rudel-audio` live example.

## What Is Bound

- Constructors: `note`, `n`, `s`, `sound`, `stack`, `cat`, `seq`, `fastcat`,
  `slowcat`, `randcat`, `chooseCycles`, `pure`, `gap`, and `silence`.
- Signals as Strudel-style values: `sine`, `cosine`, `saw`, `tri`, `square`,
  `rand`, `perlin`, `time`, and bipolar variants; `irand(n)` and `run(n)` as
  functions.
- Pattern transforms including timing, value math, alignment variants,
  conditionals, accumulation, Euclidean rhythms, sample manipulation, and tonal
  transforms.
- Higher-order callbacks such as `every`, `jux`, `sometimes`, `off`, `within`,
  `superimpose`, `chunk`, `inside`, and `when`.
- Controls for audio, samples, filters, effects, MIDI, and OSC-facing event
  data.

## Example

```rust
let pat = rudel_lang::eval(r#"
stack(
  s("bd ~ sd ~"),
  note("c4 e4 g4").s("triangle").every(4, |x| x.fast(2))
)
"#)?;
```

Koto strings are parsed as mini-notation when converted into patterns, while
`pure("text")` keeps a literal string value.

## Tests

```bash
cargo test -p rudel-lang
```
