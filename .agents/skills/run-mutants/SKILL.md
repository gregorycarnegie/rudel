---
name: run-mutants
description: Run cargo-mutants on rudel — a whole-workspace baseline, one package, or a re-check of specific files after writing tests — and read the results. Use when asked to run mutation testing, mutants, to measure or re-measure test quality, to check whether new tests killed the surviving mutants, or to find where the tests are weak.
---

# Mutation testing rudel

Use the existing repository scripts below from the repo root.

```powershell
pwsh -File scripts/full-run.ps1                              # all 8 packages, ~10h30m
pwsh -File scripts/full-run.ps1 -Packages rudel-core,rudel-app   # resume / subset
pwsh -File scripts/check-file.ps1 rudel-core euclid.rs choice.rs # re-check files, minutes
pwsh -File scripts/verify-missed.ps1 rudel-dsp                   # re-test only last run's misses
```

After a full run, `verify-missed.ps1` is the cheap way to check new tests: it
re-tests just the mutants `mutants-full/<pkg>/mutants.out/missed.txt` lists,
by name. dsp's 139 misses take minutes; re-checking their files is ~4800
mutants and hours.

Use `exec_command` with a short `yield_time_ms`, then monitor its returned
session with `write_stdin`; even `check-file.ps1` pays a tree copy and baseline
build. If monitoring stops, report the process/session and log path rather than
claiming the run finished. A full run takes hours; choose the scope requested.

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

## Hung builds

A full run is ~10.5 hours now (2026-10-01: lang 4h30m, dsp 2h20m). Five times
in that run a mutant's `rustc` sat idle — 0 CPU, empty working set — for hours,
holding its slot; cargo-mutants has no build timeout of its own. Both scripts
now pass `--build-timeout 900`. In a run started without it, kill the idle
`rustc` *and* its parent `cargo test --no-run`; killing only the `rustc` leaves
the `cargo` hung too. The mutant is then recorded as a failed build.

## Csound must be required where it is installed

Csound's tests skip when the library does not load, so a mutant that breaks
starting it turns every test into a skip and survives. Both scripts set
`RUDEL_CSOUND_REQUIRED=1` when `csound64.dll` is in its default location;
before that, every `csound.rs` mutant on the start path read as missed.

## While a run is in flight

**Do not edit the source or start concurrent builds.** Each package is a fresh invocation that copies the source at
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
failed. Investigate the baseline failure before rerunning; it is not a mutant result.

Compare **percentages**, never raw mutant counts, across dates: the count moves
with the tree. Counts on `app/panels.rs` wobble ±1–2 between runs because its
tests spawn blocked threads, so a mutant can be caught *flakily*.

## What survives here

A large equivalent-mutant population is normal and is not worth chasing:
comparisons inside continuous piecewise envelopes where `<` and `<=` agree at
the boundary, `if b.len() < n { resize }` guards, fast-path predicates whose two
paths agree by contract, `|` vs `^` on disjoint bit flags, and clamps nothing
reaches.

Devices are not a reason to leave a mutant. Each has a fake or a real
stand-in already:

- **Audio:** `Engine::with_fake_output(sr)` returns a `FakeOutput` whose
  `pull(frames)` renders on demand; the playhead moves only when it is pulled.
- **GPU:** `ui_tests::gpu_app_at(code, pixels_per_point)` renders the shader,
  hydra and spiral widgets on this machine's adapter. Run at 2.0 so `* ppp`
  cannot pass as `/ ppp`. Hydra reads other outputs from the previous frame, so
  render twice. A widget's id carries its source span, so an edit that keeps
  the length keeps the id and its caches.
- **MIDI:** the Windows "Microsoft GS Wavetable Synth" is a real output port on
  every install. A `MidiSink` recorder covers the scheduler. This machine has
  no input ports, so an input list that comes back empty is the truth here.
- **File dialogs:** `RudelApp::dialogs` answers Open, Save and confirm in
  tests. `rfd::FileDialog` is `Debug`, so its filters can be asserted.

What genuinely needs other hardware is the non-Windows `say`/`spd-say` speech
backend.

After writing tests, re-check with `check-file.ps1`, one invocation per package
listing every file at once, and expect two or three rounds before a file
settles.
