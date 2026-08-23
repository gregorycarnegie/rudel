---
name: run-mutants
description: Run cargo-mutants on rudel — a whole-workspace baseline, one package, or a re-check of specific files after writing tests — and read the results. Use when asked to run mutation testing, mutants, to measure or re-measure test quality, to check whether new tests killed the surviving mutants, or to find where the tests are weak.
---

# Mutation testing rudel

Two scripts do the work. Both must run from the repo root.

```powershell
pwsh -File scripts/full-run.ps1                              # all 8 packages, ~6h30m
pwsh -File scripts/full-run.ps1 -Packages rudel-core,rudel-app   # resume / subset
pwsh -File scripts/check-file.ps1 rudel-core euclid.rs choice.rs # re-check files, minutes
```

Run them with `run_in_background: true` — even `check-file.ps1` pays a tree copy
and a baseline build.

## The flags, and why

- **`--gitignore=true` is mandatory** (both scripts pass it). The vendored
  `strudel/` is a nested git repo; without it the tree copy tries to recreate
  npm symlinks and dies on Windows with `os error 1314` at "0 mutants tested".
- **rudel-core needs `--test-package rudel-core --test-package rudel-mini`.**
  Its parity tests (`tonal_parity`, `transform_parity`, `tune_table_parity`)
  live in rudel-mini's test dir because they need the mini parser. Without mini
  the miss count reads about a fifth too high. Both scripts add this for core.
- `-j8` is fine; avoid `--in-place`, which forces `-j1` and corrupts the shared
  source when sharded.
- `--file` matches on the **basename**, so `--file lib.rs` hits every `lib.rs`
  in the package.

## While a run is in flight

**Do not compile.** Each package is a fresh invocation that copies the source at
*its* start, so an edit lands in every package not yet started — a broken build
fails that package's baseline and wastes hours. Reading and drafting are free;
`cargo check` is not, and a concurrent build at `-j8` inflates the timeout count.

**A dsp run looks stalled but is not.** The tee'd log prints a line per
*non-caught* mutant only, and dsp is by far the largest package (~4000 mutants).
Progress shows in `mutants-full/rudel-dsp/mutants.out/*.txt`.

## Reading the results

`full-run.ps1` prints a per-package `caught / missed / timeout = %` summary into
`mutants-full/run.log` at the end. Per package the detail is in
`mutants-full/<pkg>/mutants.out/`: `missed.txt`, `caught.txt`, `timeout.txt`,
`unviable.txt`. Rank the missed by file to decide where to go next:

```powershell
Get-Content mutants-full/*/mutants.out/missed.txt |
    ForEach-Object { ($_ -split ':')[0] } | Group-Object | Sort-Object Count -Desc |
    Select-Object -First 15 Count, Name
```

"failed in an unmutated tree" in `mutants-full/<pkg>.log` means the **baseline**
failed — a real bug under parallel load, not flake. Fix it before rerunning.

Compare **percentages**, never raw mutant counts, across dates: the count moves
with the tree. Counts on `app/panels.rs` wobble ±1–2 between runs because its
tests spawn blocked threads, so a mutant can be caught *flakily*.

## What survives here

A large equivalent-mutant population is normal and is not worth chasing:
comparisons inside continuous piecewise envelopes where `<` and `<=` agree at
the boundary, `if b.len() < n { resize }` guards, fast-path predicates whose two
paths agree by contract, and clamps nothing reaches. What is left in
`audio/mixer/engine.rs` needs a live cpal device and `midi/output.rs` needs a
real port; leave both.

After writing tests, re-check with `check-file.ps1`, one invocation per package
listing every file at once, and expect two or three rounds before a file
settles.
