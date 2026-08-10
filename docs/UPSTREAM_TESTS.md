# Upstream Strudel tests and their Rudel counterparts

Strudel's test suite is `vitest` over the vendored packages. Rudel is Rust, so
those files cannot be executed against it directly; what this page records is,
per upstream test file, **how its behaviour is covered on the Rudel side** —
either by a generated parity oracle (the strongest form: the assertions come
from running the real Strudel engine under Node), by a ported equivalent test,
or not at all, with the reason.

The pinned upstream revision is in [`STRUDEL_SOURCE.md`](STRUDEL_SOURCE.md).
Regeneration instructions for every oracle are in
[`tools/oracle/README.md`](../tools/oracle/README.md).

## Why the suite is not run directly

Two reasons, both structural rather than incidental:

1. **The assertions are JavaScript.** Upstream tests call `Pattern` methods and
   compare against JS objects and inline snapshots. Running them would test
   Strudel, not Rudel.
2. **The vendored checkout is not installable in CI.** `strudel/` is
   git-ignored (it is a reference copy, not a dependency), and its
   `node_modules` carries only what the oracle generators need — `vitest` is not
   installed. CI therefore runs against *committed goldens* produced by the
   oracle generators, which is what lets the parity tests run with no Node and
   no Strudel checkout present.

The oracle route is stronger than porting assertions by hand: a generated golden
is the real engine's output, so it catches disagreements the upstream suite
never asserted on. Where a generated golden exists, that is the counterpart of
record.

## Coverage map

| Upstream file | Tests | Rudel counterpart | Form |
| --- | ---: | --- | --- |
| `packages/core/test/pattern.test.mjs` | 162 | `rudel-mini/tests/transform_parity.rs` (core transforms + the alignment matrix, hap-for-hap vs the real engine), plus the per-module unit tests in `rudel-core/src/{pattern,transforms,euclid,signal,samples,impure}.rs` | oracle + ported |
| `packages/mini/test/mini.test.mjs` | 36 | `rudel-mini/tests/mini_parity.rs` — **every deterministic case from this file** is golden-tested hap-for-hap, `_steps` included (`gen_mini_oracle.mjs`) | oracle |
| `packages/core/test/signal.test.mjs` | 6 | `rudel-core/tests/parity_oracle.rs` — `rand`/`perlin`/`degradeBy` and the analytic signals to 1e-12 (`gen_core_oracle.mjs`); upstream's statistical PRNG checks are superseded by exact goldens | oracle |
| `packages/core/test/euclid.test.js` | 5 | `rudel-core/src/euclid.rs` unit tests + proptests (tresillo/cinquillo goldens, length/pulse-count/inversion invariants) and the `euclid*` oracle cases | ported |
| `packages/core/test/fraction.test.mjs` | 1 | `rudel-core/src/fraction.rs` unit tests + proptests | ported |
| `packages/core/test/value.test.mjs` | 2 | `rudel-core/src/value.rs` unit tests, including the issue #1026 control-vs-scalar guard verified against the real engine | ported |
| `packages/core/test/controls.test.mjs` | 8 | `rudel-lang/src/tests/controls.rs` + `rudel-core/src/controls/tests.rs` (alias canonicalisation vs `getControlName`), and `reference_parity.rs` for the full control surface | ported |
| `packages/core/test/util.test.mjs` | 39 | `rudel-lang/src/tests/util.rs` for the REPL-reachable helpers; the rest of `util.mjs` is registration/curry/hashing plumbing realised natively in Rust (see the `core/util.mjs` item in `FULL_STRUDEL.md`) | ported (partial by design) |
| `packages/tonal/test/tonal.test.mjs` | 14 | `rudel-mini/tests/tonal_parity.rs` — 59 labelled cases from the real `@strudel/tonal` (`gen_tonal_oracle.mjs`) | oracle |
| `packages/tonal/test/tonleiter.test.mjs` | 14 | `rudel-mini/tests/tonal_parity.rs` (scale steps, voicings, `rootNotes`) + `rudel-core/src/{tonal,voicing}.rs` unit tests | oracle + ported |
| `packages/xen/test/xen.test.mjs` | 1 | `rudel-mini/tests/tonal_parity.rs` xen cases + `rudel-mini/tests/tune_table_parity.rs`, which checks **all 3304** tune.js scales against the real engine | oracle |
| `packages/edo/test/{edo,edoscale,ratios}.test.mjs` | 3 | `rudel-mini/tests/tonal_parity.rs` `edoScale` cases + `rudel-core/src/edo.rs` unit tests pinning the full metadata | oracle + ported |
| `packages/transpiler/test/transpiler.test.mjs` | 11 | `rudel-lang/src/tests/preprocess.rs` — Rudel emits Koto, not JS, so the assertions are against the expected rewrite and against behavioural equivalence of the two callback spellings | ported (different target) |
| `test/examples.test.mjs` | 1 (×509 examples) | `rudel-lang/tests/doc_examples.rs` — the same corpus, extracted by `gen_examples_oracle.mjs`, executed against Rudel with an exact-match allowlist for what cannot run | ported |
| `test/metadata.test.mjs` | 28 | `rudel-lang/tests/{reference_parity,api_inventory,reference_snapshot}.rs` — asserts every documented Strudel name is exposed or allowlisted, and that the classification and exposed surface cannot drift | ported |
| `packages/core/test/drawLine.test.mjs` | 6 | **Not covered.** `drawLine` renders an ASCII pattern diagram for the browser console; Rudel's visualisers are the native inline widgets. See `docs/UNSUPPORTED.md`. | not ported |
| `packages/core/test/solmization.test.js` | 6 | **Not covered.** `sol2note` and the solmization tables are unused scaffolding in Strudel itself ("not used yet") and are not exposed by Rudel. | not ported |
| `packages/mondo/test/mondo.test.mjs` | 178 | `rudel-lang/src/preprocess/mondo.rs` — the tokenizer/parser/sugar cases, which pin the language itself, plus `rudel-lang/src/tests/mondo.rs` comparing each mondo spelling against the Koto one it compiles to. The remaining ~150 cases drive upstream's runner as a general Lisp (SICP arithmetic, `let`/`match`/recursion/lists) through an evaluator the pattern language never uses; that half is not ported. See `docs/UNSUPPORTED.md`. | ported (partial by design) |
| `test/tunes.test.mjs` | 1 (×N tunes) | **Not covered.** Snapshots the full example tunes from `testtunes.mjs`, several of which use unsupported features; the doc-example corpus is the equivalent breadth check. | not ported |

## Summary

Of 21 upstream test files, 18 have a Rudel counterpart — 10 of them backed by a
generated oracle that compares against the real Strudel engine rather than
against re-typed expectations. The 3 that do not are `drawLine`, `solmization`
and `tunes`: two cover surfaces Rudel deliberately does not expose, and one is a
snapshot suite the doc-example corpus supersedes.

Rudel additionally runs parity tests upstream has no equivalent for, because
they compare a Rust reimplementation against JS the browser executes directly:
the stepwise corpus (`stepwise_parity.rs`, 45 cases from
<https://strudel.cc/learn/stepwise/> and the `@example`s of the functions it
documents, evaluated by the real engine — the doc-example corpus only asks that
a snippet runs, and a wrong *step count* is silent rather than loud);
the DSP goldens (`zzfx`, `lfo`, `modenv`, `adsr`, `distortion`, `warp`,
`bytebeat`, the `djf`/`ladder`/`transient` worklets) and the Web Audio goldens
rendered through `node-web-audio-api` (`biquad`, `vowel`, `phaser`).
