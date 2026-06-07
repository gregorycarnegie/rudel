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
      (`range2` was already bound), plus `apply`/`always`/`never` and a broad set
      of Strudel-style camelCase aliases (`iterBack`, `fastGap`, `repeatCycles`,
      `chunkBack`, `firstOf`/`lastOf`, `juxBy`, `sometimesBy`, `someCycles`/
      `someCyclesBy`, `almostAlways`/`almostNever`, `pressBy`, `swingBy`,
      `euclidRot`, `scaleTranspose`/`scaleTrans`, `rootNotes`, `loopAt`,
      `toBipolar`/`fromBipolar`). `layer`/`timecat` are bound too.

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
- [x] enharmonic interval-string transpose: `transpose("3M")`, `transpose("5P")`,
      descending `"-2M"`, and patterns like `"<5P -2M>"`. Canonical interval
      parser in `tonal.rs` (`interval_to_semitones`, both note orders + sign);
      mini-notation gained an `interval` token so quality suffixes survive.
- [x] `anchor` scale stepping (`stepInNamedScale`): an `anchor` control on a
      `scale(...)` realigns scale-degree zero to that note (e.g.
      `n("0 .. 7").anchor("c5").scale("C:major")` starts at C5).
- [x] `mtranspose` / `ctranspose` — folded into `note` at event extraction
      (`tonal::apply_transpose_controls`, shared by audio/MIDI/OSC), matching
      SuperDirt: `mtranspose` steps within the tagged scale (default `C:major`),
      `ctranspose` adds semitones. The controls are consumed once applied (so an
      external SuperDirt doesn't double-apply); left in place when there's no
      `note`. `mode("below:G4")` sets both `mode` and `anchor`. Plus the voicing
      controls `chord`/`dictionary`(`dict`)/`anchor`/`offset`/`octaves` read by
      `.voicing()`.
- [x] Xenharmonic functions (`i`, `freq`, `getFreq`, `tune`, `xen`, `withBase`,
      `ftrans`/`fTrans`/`ftranspose`/`fTranspose`): full Tune.js scale archive
      generated into a Rust static table, named scales normalized to ratios with
      the octave endpoint dropped, `xen("31edo")` tagging EDO size for later
      `ftrans`, ratio arrays for `xen([...])`, frequency arrays for `tune([...])`.
- [ ] `degreeToNote`, `toScale` (custom interval-list scales) — not in the local
      Strudel clone, so no reference source to port from

## learn/sounds & learn/samples

- [x] `s`/`sound`, sample index via `:`/`n`, `gain`, `pan`
- [x] synthesized drums (`bd sd rim cp hh oh lt mt ht rd cr`) — rudel extension
- [x] `chop` `striate` `slice` `splice` `loopAt` `fit` `begin` `end` `speed` `unit`
- [x] sample-folder loading (app button; `Engine::load_samples`)
- [x] `samples(url/json)` loader: `Engine::samples(source)` / `SampleBank::
      load_samples_source` accept a local sample folder, a local `strudel.json`,
      an http(s) URL, or a `github:user/repo[/branch]` / `bubo:pack` pseudo-URL.
      `sample_map.rs` ports the pure parts of superdough's `processSampleMap`/
      `githubPath`/`resolveSpecialPaths` (string/array/note-keyed value forms,
      `_base` override, URL joining); files are fetched (ureq) or read locally,
      decoded from bytes (`Wave::load_slice`) in parallel, and registered.
      Wired into the app's sample field. Note-keyed (pitched) maps select the
      closest-tuned sample and repitch it onto the requested `note` (ports
      `getCommonSampleInfo`/`valueToMidi`): `SampleBank` stores note-grouped
      samples, `resolve(name, n, midi)` returns the sample + semitone transpose,
      and `events.rs` applies `speed *= 2^(semis/12)`. Flat packs repitch
      relative to C3 (MIDI 36) only when `note` is set (drums are untouched).
      Koto `samples("github:…")` / `aliasBank(canonical, alias…)` are exposed as
      side effects: `eval_with_samples` returns the resulting pattern plus a
      `SampleEffects` (sources + bank aliases) that the app applies against the
      engine's bank (deduped across re-evals, so live-coding doesn't re-fetch).
      `bank` aliases resolve via `SampleBank::alias_bank`/`canonical_bank`.
      Inline-map form `samples({bd: "…", sd: […]}, base)` works too: the Koto map
      is serialized to strudel.json (`koto_to_json`) and carried as a
      `SampleEffects.maps` `(json, base)` entry the app loads via
      `Engine::load_sample_map`. Local sources expand a leading `~`/`~/` to the
      home dir (`expand_home`). Not ported: the callback form of
      `registerSamplesPrefix` (arbitrary prefix → resolver fn doesn't fit the
      collect-effects-then-apply model).
- [x] `cut` (cut groups / choke): a `cut` control tags each voice with a group;
      when a new voice in the same group starts, any still-playing voice in that
      group is choked with a 10ms fade (matches Strudel). Applies to all voice
      types, not just samplers. Choke ramp lives in the `Mixer` (`ActiveVoice`).
- [x] `loop` / `loopBegin` / `loopEnd`: a `loop` control makes a sampler loop
      between `loopBegin`/`loopEnd` (0..1 of the buffer) for the hap's duration
      instead of playing once to its natural end (matches superdough). Forward
      playback only; the read position wraps in `SamplerVoice::tick`. Koto
      methods `loop`/`loopBegin`/`loopEnd` (+ `loopb`/`loope`) — `loop` is a Koto
      keyword but is allowed after `.`, so it binds directly.
- [x] `bank` control (drum-machine name prefix): `s("bd").bank("RolandTR909")`
      resolves the banked sample `RolandTR909_bd`, falling back to the bare
      name (so the built-in drum synth still works when no pack is loaded).

## learn/synths

- [x] waveforms `sine` `sawtooth` `square` `triangle`
- [x] ADSR: `attack`/`att` `decay`/`dec` `sustain`/`sus` `release`/`rel`
- [x] `ad` / `ar` / `adsr` shortcut controls (`:`-lists) + `hold`
- [x] noise sources `white` `pink` `brown` (`s("white")`; stateful white/pink/
      brown generators in the synth voice)
- [x] `supersaw` (`unison`/`detune`/`spread`) — N detuned saws summed
- [x] single-operator FM (`fm`/`fmi` index, `fmh` ratio): carrier freq
      modulated by `fmi·modfreq·sin`
- [x] FM modulator waveform (`fmwave`: sine/saw/square/triangle) and FM
      modulation-index envelope (`fmattack`/`fmdecay`/`fmsustain`/`fmrelease`,
      scaling the index 0..1 via a linear ADSR; sustain defaults to full when
      only attack/decay are set, like superdough's `getADSRValues`).
- [x] multi-operator FM matrix (ports superdough's `applyFM`): 8 operators
      tuned by per-op `fmh{n}` ratio + `fmwave{n}` + index envelope `fm{adsr}{n}`,
      routed by an `fmiIJ` matrix (chain `fmi`/`fmi2`/… plus arbitrary edges)
      into each other and the carrier (target 0). Lives in `rudel-dsp/fm.rs`
      (`FmSpec`/`FmOp`); the synth advances all operator phases per sample with a
      one-sample cross-modulation delay. Koto binds operator 1 + operator 2 as
      named controls; higher operators / arbitrary `fmiIJ` edges use the generic
      `ctrl("name", value)` method. Not ported: per-op `fmenv` exp curve.
- [x] additive synthesis (`partials`/`phases`): builds a peak-normalized
      one-cycle wavetable from harmonic magnitudes over the base series named by
      `s` (sawtooth/square/triangle/user), ports `waveformN` + Web Audio's
      PeriodicWave normalization. `partials` is a list of magnitudes or a count
      (= N equal harmonics); `phases` rotates each harmonic. Built in
      `oscillator.rs` (`build_additive`/`sample_table`), stored on `VoiceParams`,
      sampled with linear interpolation. Koto `partials`/`phases` take a list.
- [ ] `zzfx`, wavetables — no DSP reference in the local Strudel clone
      (worklet-based); would need original DSP, not a port
- [x] vibrato (`vib` rate + `vibmod` depth, LFO on pitch) and pitch envelope
      (`penv` semitones + `p{attack,decay,sustain,release}`/`panchor`)
- [x] `pw` pulse-width (`s("pulse")` + `pw` duty cycle; 0.5 == square),
      `noise` mix amount (pink-noise blended into the oscillator via
      superdough's `wetfade` dry/wet crossfade), and `pcurve` (pitch-envelope
      ramp shape: 0 = linear, nonzero = exponential/geometric segments).

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
- [x] `dry` (wet/dry of room/delay): per-voice `dry` scales the direct signal in
      the mixer (default full); reverb/delay sends are taken pre-dry, so `dry(0)`
      leaves only the wet signal.
- [x] `ftype` (filter slope): `0`/`"12db"` = single biquad (default), `2`/`"24db"`
      = cascade the biquad twice for a steeper slope; applies to `lpf`/`hpf`/`bpf`
      (synth) and the sampler lowpass. `1`/`"ladder"` (Moog worklet) not ported.
- [ ] `djf`, `leslie`, IR reverb (`ir`), `squiz` (sampler harmonic repeats),
      `compressor*` (needs orbit/bus routing), `fshift` (frequency shifter) — no
      DSP reference in the local Strudel clone (control-only / worklet-only)

## functions/value-modifiers

- [x] `add` `sub` `mul` `div` `mod`(`modulo`) `pow` `set` `keep`
- [x] `round` `floor` `ceil` `range` `range2` `rangex` `ratio` `toBipolar` `fromBipolar`
- [x] alignment matrix (`.add.out`/`.set.squeeze`/… in/out/mix/squeeze/squeezeout/reset/restart/poly)

## learn/time-modifiers

- [x] `fast` `slow` `rev` `iter` `iterBack` `ply` `palindrome` `off` `early` `late`
- [x] `compress` `zoom` `fastGap` `inside` `outside` `swingBy`/`swing` `repeatCycles`
      `press`/`pressBy` `brak` `hurry` `focus`
- [x] `pace` (stretch to a target step count, preserving step metadata)
- [x] `ribbon`/`rib` (cut a `cycles`-long window at `offset` and loop it;
      `early` + `keep_restart`), `seg` (alias for `segment`)
- [ ] `compressSpan`/`focusSpan`/`zoomArc` (would just duplicate the two-arg
      `compress`/`focus`/`zoom` — no Koto span type), `flux`

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
- [x] `euclidLegato`/`euclidLegatoRot` (gapless held pulses; rotation as a late
      offset, matching superdough's `_euclidLegato`)
- [ ] `whenKey`/`keyDown` (keyboard), `ifp`

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
- [x] stepwise `expand`/`contract`/`shrink`/`grow` step-counters (fixed numeric
      amounts; pattern-varying step metadata still out of scope)
- [ ] `ncat`

## learn/mini-notation (parser — parity-tested)

- [x] sequences, `[ ]` sub-groups, `*`/`/`, `!`/`@`/`_` (elongate), `[,]` stacks,
      `~`, `<>` alternation, `.` groups, euclid `(p,s,r)`, `..` ranges,
      `{}%` polymeter, `?` degrade, `:` sample index, `|` random choice
- [x] chord names in mini-notation (`c:maj7`), `:` with non-numeric tails
      (`:` tails stay list values; `s("name:tail")` keeps non-numeric `n`;
      tonal/voicing/root-note code reads list symbols like `["C","maj7"]`)

## learn/input-output

- [x] MIDI out (`rudel-midi`), OSC/SuperDirt out (`rudel-osc`), app output selector
- [x] True microtonal MIDI via lower-zone MPE: `freq` has pitch priority and
      `freq`/fractional pitches use MPE by default; channel 1 is master, channels
      2-16 are allocated per active note, pitch bend is sent before note-on,
      `bendRange` controls bend scaling, exhausted member channels fall back to
      nearest unbent master-channel notes, and stop/reset sends all-notes-off +
      pitch-bend center on all 16 channels.
- [x] `.midi(...)` / `.osc(...)` as Koto pattern methods (route per-pattern): tag
      haps with an `_io` control; the app runs all back-ends at once and splits
      the pattern (`rudel_lang::filter_output`/`output_targets`), with untagged
      events going to the selected default output. `.osc("host:port")` also sets
      `oschost`/`oscport`; `.midi("dev")` records a `_midiport` hint.
- [x] `osc` custom address/host/port from controls (`oschost`/`oscport`): the OSC
      back-end resolves a per-event `host:port` (`osc_target`) and `send_to`s it,
      stripping the routing keys from the `/dirt/play` message.
- [x] MIDI input / clock-in, MIDI CC mapping helpers: a process-global input bus
      in `rudel-core` (`set_cc`/`get_cc`/`cc_in`) feeds the `ccin(cc[, chan])`
      query-time 0..1 signal; `rudel-midi`'s `MidiIn` connects an input port
      (`Ignore::None` to receive clock), routes incoming CC to the bus, and a
      `ClockDetector` estimates BPM from clock pulses (`bpm`/`cps`,
      `bpm_to_cps`). The app adds a MIDI-input device field + a `clock→cps`
      toggle. `process_input` decodes messages (unit-tested without a device).

## learn/code (REPL ergonomics)

- [x] live eval + hot-swap, error surfacing, cps slider, reference pane
- [ ] autocomplete / sound+control hints in the editor
- [x] per-pattern naming: `$:` anonymous labels, `name:` labels, and the `.p(name)`
      method all tag patterns with an `id` and stack into the result;
      comments-as-mute works (a commented label line drops out of the stack).

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
