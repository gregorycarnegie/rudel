# rudel — TODO

Remaining / deferred work. Phases 0–6 (engine → mini-notation → voices →
scheduler/audio → samples/effects → Koto live-eval → egui app) are complete.

## Live-eval (rudel-lang)

- [x] Higher-order Koto combinators with function args — `every(n, f)`, `jux(f)`,
      `sometimes(f)`, `off(t, f)`, `superimpose(f)`, `within(a, b, f)`. Done via a
      `Callback` marshaler (spawns a shared VM and drives the `KValue` function
      eagerly, surfacing the first error). Also bound: `first_of`/`last_of`,
      `chunk`/`chunk_back`, `inside`/`outside`, `jux_by`, `sometimes_by`/`often`/
      `rarely`/`almost_always`/`almost_never`, `some_cycles`/`some_cycles_by`, `when`.
- [x] Expose remaining transforms already in the engine but not yet bound in Koto:
      `chunk`, `inside`/`outside`, `echo`/`stut`, `swing`/`swing_by`, `range`/
      `range2`/`rangex`, `compress`, `zoom`, plus a broad set of patternified
      controls and value ops (`div`, `modulo`, `pow`, `set`, `mask`, `struct_pat`,
      `early`/`late`, `iter_back`, `repeat_cycles`, `rev`/`revv`, `press`, `brak`,
      `round`/`floor`/`ceil`, …) and all the named sample controls.

## Sample manipulation (rudel-core / rudel-dsp / rudel-audio)

- [x] `chop(n)` — slice a sample into n equal pieces across the event.
- [x] `striate(n)` — interleave n slices across the cycle.
- [x] `slice(n, ip)` / `splice` — index into n slices of a sample (n may be a
      list of split points). `splice` sets `speed`/`unit` per slice.
- [x] `loopAt(cycles)` (`loop_at`) — stretch a sample to span N cycles
      (sets speed/unit; reads `_cps` from query state).
- [x] `fit` — stretch each sample to fill its own event duration.
- [x] `begin` / `end` controls — play a sub-range of a sample (already in the
      engine + DSP; now also bound in Koto). `unit: 'c'` handling added to the
      DSP `SamplerVoice` so `loopAt`/`fit`/`splice` time-stretch correctly.

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
