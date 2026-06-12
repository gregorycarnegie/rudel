# rudel-mini

Mini-notation parser for Rudel.

`rudel-mini` ports Strudel's `krill.pegjs` grammar to `pest` and turns compact
pattern strings into `rudel-core::Pattern` values, mirroring the
`mini.mjs` builder hap-for-hap (including `_steps` metadata and the
per-occurrence PRNG seeds used by `?` and `|`).

## Supported Syntax

- Sequences: `bd sd hh`
- Rests: `~` and `-`
- Sub-cycles: `bd [hh hh]`
- Alternation: `<bd sd cp>`
- Stacks and choices: `bd, hh*4` and `bd | sd`
- Feet/dot groups: `a b . c d`
- Speed and slow operators: `*` and `/`, with patterned factors (`a*<3 5>`)
- Replication and weighting: `!`, `@`, `_` (also bare repeats: `a ! !`)
- Euclidean rhythms: `x(3,8)`, `x(3,8,1)`, patterned args `x(<3 5>,<8 16>)`
- Ranges: `0 .. 3`, patterned bounds `<0 1> .. <2 4>`
- Sample indices and lists: `bd:2`, `a:b:c`
- Polymeter: `{bd sd, hh*3}%4`, patterned steps `{a b c}%<2 3>`
- Degradation: `?`, `?0.8` (seeded per occurrence like Strudel)
- Steps marker: `^` (`a [^b c]` has 4 steps, for `pace` and friends)
- Tokens are classified with JavaScript `Number()` semantics: `1e3`, `0x10`,
  `.5`, and `-3` are numbers; `3M`, `-x`, `bd.cp`, `a~b` stay strings

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
against goldens generated from Strudel's real parser by
`tools/oracle/gen_mini_oracle.mjs` (93 patterns, haps + `_steps`, cycles 0..3).
