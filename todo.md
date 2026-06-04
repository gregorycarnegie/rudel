# rudel — TODO

Remaining / deferred work. Phases 0–6 (engine → mini-notation → voices →
scheduler/audio → samples/effects → Koto live-eval → egui app) are complete.

## Live-eval (rudel-lang)

- [ ] Higher-order Koto combinators with function args — `every(n, f)`, `jux(f)`,
      `sometimes(f)`, `off(t, f)`, `superimpose(f)`, `within(a, b, f)`. Needs
      Koto-callback marshaling (wrap a `KValue` function as a Rust
      `Fn(Pattern) -> Pattern`). The engine already implements all of these.
- [ ] Expose remaining transforms already in the engine but not yet bound in Koto
      (e.g. `chunk`, `inside`/`outside`, `echo`/`stut`, `swing`, `range`).

## Sample manipulation (rudel-core / rudel-dsp / rudel-audio)

- [ ] `chop(n)` — slice a sample into n equal pieces across the event.
- [ ] `striate(n)` — interleave n slices across the cycle.
- [ ] `slice(n, ip)` / `splice` — index into n slices of a sample.
- [ ] `loopAt(cycles)` — stretch a sample to span N cycles (sets speed/unit).
- [ ] `fit` — map sample indices across the events in a cycle.
- [ ] `begin` / `end` controls — play a sub-range of a sample.

## Tonal / scales (new module in rudel-core)

- [ ] Note-name → MIDI for the full set (port Strudel's `tonal` package).
- [ ] `scale("C:major")` + `scale` control and scale-degree mapping.
- [ ] `transpose`, `note`-by-scale-degree, chord helpers.

## I/O (Phase 7, optional)

- [ ] `rudel-midi` — MIDI output via `midir` (note on/off, CC, clock).
- [ ] OSC output (SuperDirt-compatible) for external synths.

## App (rudel-app) polish

- [ ] UI button to load a sample folder into the `SampleBank` at runtime.
- [ ] Live cycle playhead / position indicator on the visualizer.
- [ ] Per-orbit / multi-pattern display.
- [ ] Surface available sound names + controls (autocomplete or a reference pane).

## Engine parity

- [ ] Port remaining `core/test` + `mini/test` snapshots from Strudel as a
      bit-for-bit parity oracle (especially RNG-driven: `rand`/`degrade`/`perlin`).
- [ ] Full `.add.out` / `.set.squeeze` alignment matrix.
- [ ] `perlin` noise signal.
