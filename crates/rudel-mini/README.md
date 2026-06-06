# rudel-mini

Mini-notation parser for Rudel.

`rudel-mini` ports Strudel's `krill.pegjs` grammar to `pest` and turns compact
pattern strings into `rudel-core::Pattern` values.

## Supported Syntax

- Sequences: `bd sd hh`
- Rests: `~`
- Sub-cycles: `bd [hh hh]`
- Alternation: `<bd sd cp>`
- Stacks and choices: `bd, hh*4` and `bd | sd`
- Feet/dot groups: `a b . c d`
- Speed and slow operators: `*` and `/`
- Replication and weighting: `!`, `@`, `_`
- Euclidean rhythms: `x(3,8)` and `x(3,8,1)`
- Ranges and sample indices: `0..3`, `bd:2`
- Polymeter: `{bd sd, hh*3}%4`
- Degradation: `?`

## Example

```rust
let pat = rudel_mini::parse("bd [hh hh] <sd cp>*2")?;
```

Install the parser to make `&str` pattern arguments in `rudel-core` behave like
mini-notation:

```rust
rudel_mini::install();

let pat = rudel_core::s("bd:2 [hh hh] ~");
```

## Tests

```bash
cargo test -p rudel-mini
```

The integration tests compare mini-notation and selected transform output
against goldens generated from Strudel.
