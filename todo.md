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

## Tonal / scales (`rudel-core/src/tonal.rs`)

- [x] Note-name → MIDI (`note_to_midi`, default octave 3, sharps/flats); the
      DSP `note_name_to_midi` now delegates to it. ~24 scale types inlined
      (church modes, pentatonics, blues, bebop, diminished, …).
- [x] `scale("C:major")` (root:type syntax, patternifiable) + scale-degree
      mapping with octave wrapping and `#`/`b` step accidentals; note names are
      quantised to the scale. Scale is tagged on the hap context.
- [x] `transpose` (semitones), `scale_transpose` (within the tagged scale), and
      `chord()` (chord-symbol → stacked notes, ~20 chord qualities). All bound
      in Koto.

> Not yet ported: enharmonic-correct interval-string transpose (e.g. `"3M"`),
> `@tonaljs` voicing dictionaries (`renderVoicing`), and `anchor`-based scale
> stepping. Numeric/semitone paths cover the common cases.

## I/O (Phase 7, optional)

- [ ] `rudel-midi` — MIDI output via `midir` (note on/off, CC, clock).
- [ ] OSC output (SuperDirt-compatible) for external synths.

## App (rudel-app) polish

- [ ] UI button to load a sample folder into the `SampleBank` at runtime.
- [ ] Live cycle playhead / position indicator on the visualizer.
- [ ] Per-orbit / multi-pattern display.
- [ ] Surface available sound names + controls (autocomplete or a reference pane).

## Engine parity

- [x] `perlin` noise signal (`signal::perlin`, quintic smootherstep, reads
      `randSeed` from controls). Bound in Koto.
- [x] Bit-for-bit parity oracle, golden values dumped from Strudel's real
      engine (`tools/oracle/`, `tools/gen_parity_oracle.mjs`):
      - RNG + analytic signals (`crates/rudel-core/tests/parity_oracle.rs`):
        `rand`, `perlin`, `degradeBy`, `saw`/`isaw`/`sine`/`cosine`/`square`
        match to 1e-12.
      - mini-notation (`crates/rudel-mini/tests/mini_parity.rs`): 29 patterns
        covering sequences, sub-groups, `*`/`/`, `!`/`@`, `[,]` stacks, `~`,
        `<>` alternation, `.` groups, euclid `(p,s,r)`, `..` ranges, polymeter
        `{}%`, `?` degrade, `:` sample-index.
      - core transforms (`crates/rudel-mini/tests/transform_parity.rs`):
        18 cases (`rev`/`fast`/`slow`/`ply`/`iter`/`palindrome`/`every`/`off`/
        `chop`/`striate`/`chunk`/`within`/`struct`/`mask`/`jux`/`add`/`degrade`/
        `superimpose`).
      Caught and fixed a real bug: euclidean rotation was rotating the wrong
      direction (Strudel rotates right by `rotation`).
- [x] Alignment matrix (`.add.out` / `.set.squeeze` / …). Engine primitives
      `op_in`/`op_out`/`op_mix`/`op_squeeze`/`op_squeeze_out`/`op_reset`/
      `op_restart` (+ `reset_join`/`restart_join`) plus an `Align` enum and
      `op_align`. Exposed `<op>_<align>` methods for add/sub/mul/div/set/keep
      (a macro generates out/mix/squeeze/squeezeout/reset/restart; `in` stays the
      plain method). Bound a curated set in Koto; parity-checked against Strudel
      in `transform_parity.rs` (add.out/mix/squeeze/squeezeout/reset/restart,
      mul.out, set.out/mix/squeeze, keep.out). `poly` not yet ported (needs the
      steps-based `extend`).
