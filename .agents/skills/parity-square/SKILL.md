---
name: parity-square
description: Measure Rudel against real Strudel over a corpus of patterns and cross-tabulate the result into a works/fails square, then rank the rudel-only failures by cause. Use when asked how many patterns work, to run a differential or sweep against Strudel, to find or triage parity gaps, to check a preprocessor or binding change for regressions across the whole corpus, or when a pass rate over user-written patterns is wanted.
---

# The parity square

A pass rate over patterns people actually wrote means nothing on its own: a
large share of what is shared on strudel.cc does not work **in Strudel**
either — half-finished sketches, scratch pads, browser-only APIs. Running the
same corpus through both engines and crossing the results is the only way to
separate those from a Rudel gap.

```
                    rudel works  rudel fails
strudel works              4937          111   <- ours to fix
strudel fails              1918         1038   <- broken for everyone
```

The top-right cell identifies Rudel-only evaluation failures. Investigate
`PANIC` results in any cell; the bottom row alone does not establish a parity gap.

Paths below are relative to the repo root.

## 1. Get a corpus

Any directory of `.js` files works — `~/Downloads/strudel-tunes`, a clone of a
song collection, a handful of files you are debugging. The interesting one is
everything shared on strudel.cc:

```bash
node .agents/skills/parity-square/corpus.mjs /path/to/corpus
```

That reads the REPL's own Supabase table with the anon key committed in
`strudel/website/src/repl/util.mjs`. It takes about a minute and writes ~8000
files. **Keep it out of the repo** — use a task-specific directory under the system temp directory.

## 2. Run the Strudel side

Once per corpus, and again only when the vendored `strudel/` checkout moves.
Setup — every `@strudel/*` package plus `superdough` linked into
`strudel/node_modules` — is in the "Differential run" section of
[`tools/oracle/README.md`](../../../tools/oracle/README.md).

The examples below use Bash environment syntax. In PowerShell set
`$env:DIFF_CORPUS` and `$env:DIFF_OUT` before invoking Node; restore their previous
values afterward. Fetch once and keep the same corpus and query length for comparisons.

```bash
DIFF_CORPUS=/path/to/corpus DIFF_OUT=/path/to/strudel.tsv \
  node strudel/node_modules/vitest/vitest.mjs run \
    --config tools/oracle/strudel_diff.config.mjs
```

Twenty minutes for 8000 patterns. `DIFF_FROM`/`DIFF_TO` shard it.

## 3. Run the Rudel side, and cross it

```bash
cargo build --release -p rudel-lang --example sweep

./target/release/examples/sweep.exe /path/to/corpus \
  --out /path/to/rudel.tsv \
  --strudel /path/to/strudel.tsv \
  --gaps /path/to/gaps.txt
```

Both sides write the same shape — `<OK|EMPTY|ERR|PANIC>\t<id>\t<haps|message>`
— so either can play either role. `EMPTY` counts as working: a pattern that
legitimately evaluates to silence is not a failure.

The square and a ranked cause list print to stdout. **Read the `srcs` column,
not `pats`**: one tune re-shared fifty times is one thing to fix, not fifty.
The largest bucket by pattern count is regularly a single duplicated file.

Two minutes for 8000 patterns, release build. Debug is ~15× slower — don't.

## 4. Triage

`--errors` prints each gap's whole error, with the *preprocessed* source line
and a caret, which is what actually tells you the cause:

```bash
./target/release/examples/sweep.exe /path/to/gapdir --strudel /path/to/strudel.tsv --errors
```

Copy the gap files into their own directory first (`gaps.txt` has the ids) so
this stays fast and the output stays readable.

What the buckets look like, and what to do with them:

- **The same `SyntaxError` on many sources** is nearly always *one*
  preprocessor shape, not many: the engine reads JavaScript itself, so a
  syntax error in a script Strudel runs means one of the Strudel passes (labels,
  mini-notation, widgets, `await`) rewrote it into something else. Run the
  preprocessor over the original `.js` and read the output at the reported
  line and column.
- **`ReferenceError: name is not defined`** is a missing binding: cheap, and
  usually real Strudel API worth adding. `not a callable function` is the same
  for a missing method, though the engine does not say which one.
- **A message naming a browser global** (`document`, `window`) or a
  browser-only engine may be an intentional limitation. Check
  [docs/UNSUPPORTED.md](../../../docs/UNSUPPORTED.md) before classifying it.
- Fixing one bucket routinely moves patterns *into* another, so the totals move
  less than the bucket does. Judge by the square, not by one row.

## 5. Check for regressions — always

Every preprocessor change is a rewrite of everyone's source. Compare the set of
ids that are **not** ok, before and against after; anything new is a pattern
that used to work:

```bash
comm -13 <(grep -E '^(ERR|PANIC)' before.tsv | cut -f2 | sort) \
         <(grep -E '^(ERR|PANIC)' after.tsv  | cut -f2 | sort)
```

An empty result means no new evaluation failures. `EMPTY` counts as working
in this comparison, just as it does in the square. With PowerShell, read the TSV
rows, select ids whose first field is `ERR` or `PANIC`, and use `Compare-Object`
to find additions; do not compare counts alone. Keep every `sweep*.tsv` from the
session so you can compare against the start of the day, not just the last run.

`PANIC` in the summary is a crash, not a failed pattern, and takes priority
over everything else on the list. `--errors` gives the message; re-run that one
file under `RUST_BACKTRACE=1` with a plain `eval_result` call to get a
backtrace, since the sweep deliberately catches panics to keep going.

## Notes

- The corpus is a moving target — patterns are shared continuously — so a
  number is only comparable against another run of the *same* files. Fetch
  once per campaign, not per measurement.
- `--cycles` sets the query length, 8 by default. Longer finds more, slower.
- A pattern that works in both engines can still sound different. This measures
  *evaluates and produces haps*, nothing more. Hap-for-hap parity is what
  `tools/oracle/`'s golden tests are for.
