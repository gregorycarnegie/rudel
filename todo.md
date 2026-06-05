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

## I/O (Phase 7)

- [x] `rudel-midi` — MIDI output via `midir`. Pure control-map → `MidiNote`
      mapping (note/name, velocity from velocity|gain, channel from
      midichan|channel, `ccn`/`ccv`, `progNum`), `schedule_window` emitting
      timed note-on/off, a `MidiOut` port wrapper, and a real-time `MidiEngine`
      thread driving a `MidiSink`. Shared event extraction with the audio engine
      via `rudel_core::query_controls`.
- [x] OSC output (SuperDirt-compatible) — `rudel-osc`. Hand-rolled OSC 1.0
      encoder (no extra deps), `/dirt/play` message builder (prepends
      `cps`/`cycle`/`delta`, adds `midinote`, undoes `unit:'c'` speed), UDP
      `OscOut`, and an `OscEngine` scheduler. Tested over UDP loopback.

> Both back-ends are standalone crates depending only on `rudel-core`; wiring
> them into `rudel-app` (output selector) is left as app-polish follow-up.

## App (rudel-app) polish

- [x] UI to load a sample folder into the `SampleBank` at runtime (path field +
      "Load folder"; reports count and refreshes the sound list).
- [x] Live cycle playhead on the visualizer (`Engine::position_cycles`, repaints
      while playing).
- [x] Per-orbit / multi-pattern display (haps grouped into labelled bands by
      their `orbit` control).
- [x] Reference pane listing available sounds (synth waveforms + loaded sample
      names) and control names.
- [x] Bonus: output selector (Audio / MIDI / OSC) wiring in `rudel-midi` /
      `rudel-osc`, with lazy connection and graceful fallback to audio.

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
      `op_restart`/`op_poly` (+ `reset_join`/`restart_join`/`poly_join` and the
      steps-based `expand`/`extend`) plus an `Align` enum and `op_align`. Exposed
      `<op>_<align>` methods for add/sub/mul/div/set/keep (a macro generates
      out/mix/squeeze/squeezeout/reset/restart/poly; `in` stays the plain
      method). Bound a curated set in Koto; parity-checked against Strudel in
      `transform_parity.rs` (add.out/mix/squeeze/squeezeout/reset/restart/poly,
      mul.out, set.out/mix/squeeze/poly, keep.out).
