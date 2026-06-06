# rudel — TODO

Remaining / deferred work. Phases 0–6 (engine → mini-notation → voices →
scheduler/audio → samples/effects → Koto live-eval → egui app) are complete.

---

# Gap audit vs Strudel learn pages

Function-by-function audit against the Strudel learn pages
(<https://patterns.slab.org/>) and the `strudel/` source. Legend:

- `[x]` usable from Koto now
- `[~]` implemented in the engine (`rudel-core`) but **not yet bound in Koto** —
  usually a one-line addition to the `kpattern_methods!` lists / prelude
- `[ ]` not implemented (needs engine and/or DSP work)

## Biggest quick wins (engine done, just bind in Koto)

- [x] **Signals**: `sine` `cosine` `saw` `isaw` `tri` `square` `rand` `rand2`
      `perlin` `time` exposed as Koto *values* (Strudel-style, no parens), plus
      bipolar `sine2`/`cosine2`/`saw2`/`isaw2`/`tri2`/`square2`, and `irand(n)`/
      `run(n)` as fns.
- [x] **Factories**: `slowcat` `fastcat` `randcat` `chooseCycles` `pure` `gap`
      bound in the prelude (alongside `stack`/`cat`/`seq`/`silence`).
- [x] **Transforms** newly bound: `hurry` `focus` `press_by` `euclid_rot`
      (`range2` was already bound). Still unbound: `layer` (needs callback-array
      marshaling), `apply`, `timecat` (weighted pairs).

## learn/notes & learn/tonal

- [x] `note` / `n`, note names + octaves, MIDI numbers
- [x] `scale("C:major")`, scale-degree numbers, `#`/`b` step accidentals
- [x] `transpose`/`trans`, `scaleTranspose`/`strans`, `chord()` (chord symbols)
- [x] `voicing()` (default `legacy` dict), `voicings("name")` (lefthand/triads/
      guidetones/legacy), `rootNotes`/`root_notes(octave)` — ports `renderVoicing`
      + interval-semitones math + the curated dictionaries from `voicings.mjs`.
      Reads `dict`/`anchor`/`mode`/`offset`/`octaves`/`n` from map values; symbol
      normalisation (`maj7`→`^7`, `min7`→`m7`, …) since mini can't spell `^`.
      Not ported: deprecated `voicings()` voice-leading (external package) + the
      523-line iReal dictionary.
- [x] `arp` (index pattern selects chord notes), `arpeggiate` (play chord in
      sequence), `arp_with(|chord| …)` (per-chord callback; chord presented as a
      note sequence). Built on a new `collect` (group simultaneous haps).
      `arp_with` is bound via an eager probe-and-bake: the Koto VM isn't `Send`
      so the callback can't run in the query path, so distinct chords over the
      first 16 cycles are memoised at construction (chords appearing only later
      fall back to silence).
- [ ] enharmonic interval-string transpose (`"3M"`), `mode`/`anchor` stepping
- [ ] `mtranspose` / `ctranspose` / `degreeToNote`, `toScale` (custom scales)

## learn/sounds & learn/samples

- [x] `s`/`sound`, sample index via `:`/`n`, `gain`, `pan`
- [x] synthesized drums (`bd sd rim cp hh oh lt mt ht rd cr`) — rudel extension
- [x] `chop` `striate` `slice` `splice` `loopAt` `fit` `begin` `end` `speed` `unit`
- [x] sample-folder loading (app button; `Engine::load_samples`)
- [ ] `samples(url/json)` loader (remote/JSON sample maps, `bank`, aliases)
- [ ] `cut` (cut groups / choke), `loop` / `loopBegin` / `loopEnd`
- [ ] `bank` control (drum-machine name prefix)

## learn/synths

- [x] waveforms `sine` `sawtooth` `square` `triangle`
- [x] ADSR: `attack`/`att` `decay`/`dec` `sustain`/`sus` `release`/`rel`
- [x] `ad` / `ar` / `adsr` shortcut controls (`:`-lists) + `hold`
- [x] noise sources `white` `pink` `brown` (`s("white")`; stateful white/pink/
      brown generators in the synth voice)
- [x] `supersaw` (`unison`/`detune`/`spread`) — N detuned saws summed
- [x] single-operator FM (`fm`/`fmi` index, `fmh` ratio): carrier freq
      modulated by `fmi·modfreq·sin`
- [ ] `supersaw` (`unison` `spread` `detune`)
- [x] single-operator FM (`fm`/`fmi`/`fmh`); [ ] multi-operator FM matrix,
      additive, `zzfx`, wavetables
- [x] vibrato (`vib` rate + `vibmod` depth, LFO on pitch) and pitch envelope
      (`penv` semitones + `p{attack,decay,sustain,release}`/`panchor`)
- [ ] `pcurve` (env curve shapes), `pw` pulse-width, `noise` mix amount

## learn/effects

- [x] low-pass `cutoff`/`lpf` + `resonance`/`lpq`
- [x] high-pass `hcutoff`/`hpf` + `hresonance`/`hpq`; band-pass `bandf`/`bpf` + `bandq`/`bpq`
- [x] reverb `room`/`size`; delay `delay`/`delaytime`/`delayfeedback`
- [x] `pan`, `jux`/`juxBy`, `speed`, `orbit`, `gain`, `postgain`
- [x] waveshaping/decimation: `crush` (bitcrush), `shape` (hyperbolic),
      `distort` (+`distortvol`/`shapevol`), `coarse` (sample-rate reduction) —
      per-voice `PostFx` matching superdough's worklet formulas
- [x] `vowel` formant filter (a/e/i/o/u; 5 parallel band-pass + makeup gain,
      per-channel, in `PostFx`)
- [x] filter envelopes `lpenv`/`lpattack`/`lpdecay`/`lpsustain`/`lprelease`
      (+ `hp*`/`bp*` and `fanchor`): per-sample cutoff sweep `min..max` =
      `2^-offset·f .. 2^(|env|-offset)·f` driven by the filter's own ADSR
- [x] `tremolo` (+`tremolodepth`) amplitude LFO; `phaser`/`phaserrate`
      (+`phaserdepth`/`phasercenter`/`phasersweep`) swept-notch — per-voice in
      `PostFx` (notch detune-sweep matching superdough's `getPhaser`)
- [ ] `compressor*` (needs orbit/bus routing), `dry` (wet/dry of room/delay),
      `squiz` (sampler harmonic repeats), `fshift` (frequency shifter worklet)
- [ ] `djf`, `leslie`, `ftype`/`fanchor`, IR reverb (`ir`)

## functions/value-modifiers

- [x] `add` `sub` `mul` `div` `mod`(`modulo`) `pow` `set` `keep`
- [x] `round` `floor` `ceil` `range` `range2` `rangex` `ratio` `toBipolar` `fromBipolar`
- [x] alignment matrix (`.add.out`/`.set.squeeze`/… in/out/mix/squeeze/squeezeout/reset/restart/poly)

## learn/time-modifiers

- [x] `fast` `slow` `rev` `iter` `iterBack` `ply` `palindrome` `off` `early` `late`
- [x] `compress` `zoom` `fastGap` `inside` `outside` `swingBy`/`swing` `repeatCycles`
      `press`/`pressBy` `brak` `hurry` `focus`
- [x] `pace` (stretch to a target step count, preserving step metadata)
- [ ] `ribbon`/`rib`, `compressSpan`/`focusSpan`/`zoomArc`, `flux`, `seg`

## learn/signals

- [x] `sine` `cosine` `saw` `isaw` `tri` `square` `rand` `rand2` `irand` `run`
      `time` `perlin` — bound as Koto values/fns; `segment`/`range` on signals
- [x] bipolar variants `saw2`/`square2`/`tri2`/`isaw2`/`sine2`/`cosine2`
- [ ] `envL`/`envLR`/`envEq`…, `mousex`/`mousey` (n/a native)

## learn/conditional-modifiers

- [x] `every`/`firstOf`/`lastOf`, `when`, `chunk`/`chunkBack`
- [x] `sometimes`/`sometimesBy`/`often`/`rarely`/`almostAlways`/`almostNever`/`always`/`never`
- [x] `someCycles`/`someCyclesBy`, `degrade`/`degradeBy`/`undegrade`, `mask`, `struct`
- [x] `euclid`, `euclidRot`/`euclid_rot` (3-arg rotation now bound)
- [ ] `euclidLegato`, `whenKey`/`keyDown` (keyboard), `ifp`

## learn/accumulation

- [x] `stack`, `superimpose`, `off`, `echo`/`stut`, `jux`/`juxBy`
- [x] `layer` (bound: `pat.layer([f, g, …])` stacks each callback's result)
- [x] `overlay` (method), `arrange` (factory)
- [x] `wchoose` (continuous), `wchooseCycles`/`wrandcat` (per-cycle) — weighted
      `[pattern, weight]` pairs; `scan(n)` (growing runs, one per cycle)

## learn/factories

- [x] `stack` `cat`(slowcat) `seq`(fastcat) `fastcat` `slowcat` `randcat`
      `chooseCycles` `pure` `gap` `silence`
- [x] `timecat`/`stepcat` (weighted pairs: bare patterns weight by step count,
      or pass `[weight, pat]` pairs), `arrange` (`[cycles, pat]` sections),
      `polymeter`/`pm` (`pace`-align to LCM steps)
- [x] `run` factory (already bound), `stepalt` (alternate groups stepwise),
      `take`/`drop` (keep/discard the first N steps; negative counts from the end)
- [ ] `ncat`, stepwise `expand`/`contract`/`shrink`/`grow` step-counters

## learn/mini-notation (parser — parity-tested)

- [x] sequences, `[ ]` sub-groups, `*`/`/`, `!`/`@`/`_` (elongate), `[,]` stacks,
      `~`, `<>` alternation, `.` groups, euclid `(p,s,r)`, `..` ranges,
      `{}%` polymeter, `?` degrade, `:` sample index, `|` random choice
- [ ] chord names in mini-notation (`c:maj7`), `:` with non-numeric tails

## learn/input-output

- [x] MIDI out (`rudel-midi`), OSC/SuperDirt out (`rudel-osc`), app output selector
- [ ] `.midi(...)` / `.osc(...)` as Koto pattern methods (route per-pattern)
- [ ] MIDI input / clock-in, MIDI CC mapping helpers
- [ ] `osc` custom address/host/port from controls (`oschost`/`oscport`)

## learn/code (REPL ergonomics)

- [x] live eval + hot-swap, error surfacing, cps slider, reference pane
- [ ] autocomplete / sound+control hints in the editor
- [ ] per-pattern naming (`$:` / `p()` style multi-pattern), comments-as-mute

---

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
