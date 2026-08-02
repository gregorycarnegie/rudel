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
- [x] `degreeToNote`, `toScale` (custom interval-list scales) — **unsupported**:
      neither is defined anywhere in the pinned Strudel checkout, so there is no
      reference behaviour to port. Documented in `docs/UNSUPPORTED.md`;
      `reference_parity.rs` will flag them if a Strudel bump introduces them.

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
- [x] `zzfx` — ported (`rudel-dsp/zzfx.rs`, golden-tested against superdough's
      `zzfx.mjs`); `s("zzfx")` and the `z_<wave>` family resolve to it.
- [x] `s("bytebeat")` — ported (`rudel-dsp/bytebeat.rs`). `bb`/`byteBeatExpression`
      chooses the formula and `n` picks one of the 15 built-in beats. Note that
      only double-quoted strings are mini-notation, so write the expression in
      single quotes: `s("bytebeat").bb('t&t>>8')`.
- [x] wavetable oscillator: `tables(url[, frameLen])` loads a collection of
      `.wav` wavetables (same source forms as `samples`), each sliced into
      `frameLen`-sample single-cycle frames (default 2048), and `s("name")`
      plays them. `rudel-dsp/wavetable.rs` ports the
      `WavetableOscillatorProcessor` worklet: all 22 `warpmode` phase
      distortions (golden-tested against the worklet in
      `rudel-dsp/tests/warp_golden.rs` over a 64×7 phase/amount grid), frame
      interpolation by `wt` position, and the `unison`/`detune`/`spread`/
      `wtphaserand` unison stack. `wt`/`warp` are swept per sample by their own
      linear ADSR + LFO (`ParamMod`, porting `applyParameterModulators`:
      `wtenv`/`wt{adsr}`/`wtrate`/`wtsync`/`wtdepth`/`wtshape`/`wtskew`/`wtdc`
      and the `warp*` twins). The table is attached in `events.rs` after loaded
      samples win, and `fm` applies to the wavetable frequency like upstream.
- [x] vibrato (`vib` rate + `vibmod` depth, LFO on pitch) and pitch envelope
      (`penv` semitones + `p{attack,decay,sustain,release}`/`panchor`) — on
      **samplers and soundfonts too**, not just the oscillator synth, matching
      superdough (which wires both onto every source node's `detune`). Shared
      `PitchMod` in `rudel-dsp/pitch.rs`.
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
- [x] `stretch` (phase-vocoder pitch shift) — `rudel-dsp/vocoder.rs`, ported from
      superdough's `phase-vocoder-processor` + `ola-processor.js`. Runs at the
      head of the post-fx chain like upstream; `stretch(1)` is an octave up.
- [x] `tremolo` (+`tremolodepth`) amplitude LFO; `phaser`/`phaserrate`
      (+`phaserdepth`/`phasercenter`/`phasersweep`) swept-notch — per-voice in
      `PostFx` (notch detune-sweep matching superdough's `getPhaser`)
- [x] `dry` (wet/dry of room/delay): per-voice `dry` scales the direct signal in
      the mixer (default full); reverb/delay sends are taken pre-dry, so `dry(0)`
      leaves only the wet signal.
- [x] `ftype` (filter model), all three of superdough's `['12db','ladder','24db']`:
      `0`/`"12db"` = single biquad (default), `2`/`"24db"` = the biquad cascaded
      twice, `1`/`"ladder"` = the Moog-style nonlinear ladder lowpass ported from
      superdough's `ladder-processor` worklet (`filter.rs` `Ladder`, driven by
      `drive`). Applies to `lpf`/`hpf`/`bpf` (synth) and the sampler lowpass,
      which now shares the voice filter slot. Golden-tested
      (`rudel-dsp/tests/worklet_golden.rs`).
- [x] **per-orbit effect buses** (`rudel-audio` `OrbitBus`, mirroring
      superdough's `Orbit` in `superdoughoutput.mjs`): each `orbit` gets its own
      reverb, feedback delay and DJ filter, created on demand and configured by
      the most recent event to hit it. The routing controls moved off the voice
      params onto `NoteEvent.send` (`rudel-dsp` `OrbitSend`), so `VoiceLike` no
      longer carries `room`/`delay_send`/`dry`. This made these live for the
      first time (they were parsed and forwarded over OSC but had no effect on
      the native audio): `orbit`, `delaytime`/`delaysync`/`delayfeedback`,
      `size`/`roomsize`, `roomlp`, `roomdim`. Idle orbits stop running their
      reverb/delay once the tail has decayed.
- [x] `djf` (DJ filter): ported from superdough's `djf-processor` worklet
      (`rudel-dsp/bus.rs`), applied on the orbit bus so it colours that orbit's
      dry signal and its reverb/delay returns alike. Golden-tested.
- [x] `transient`/`transsustain` (transient shaper): ported from superdough's
      `transient-processor` worklet (`rudel-dsp/postfx.rs` `TransientShaper`).
      Golden-tested. Note `transient` is a *multi*-control upstream
      (`registerControl(['transient','transsustain'])`), as is `shape`
      (`['shape','shapevol']`) — both now spread their `:`-lists rather than
      treating the second name as an alias of the first.
- [x] `roomfade` and IR reverb (`ir`/`iresponse`/`irspeed`/`irbegin`): the FDN
      reverb was replaced by a real convolution reverb, so both now work.
      `rudel-dsp/convolver.rs` ports `reverbGen.generateReverb` +
      `applyGradualLowpass` (seeded noise, exponential decay, fade-in ramp, the
      `roomlp`→`roomdim` lowpass sweep) and `adjustLength` (fitting a loaded
      sample to `size` via `irspeed`/`irbegin`), and convolves with a
      uniform-partitioned overlap-save FFT convolver sharing `fft.rs` with the
      phase vocoder. `ReverbConfig` carries the resolved IR (`events.rs` looks
      `ir`/`iresponse` up in the bank). The impulse response is normalized like a `ConvolverNode` (whose `normalize` defaults to `true`), so the wet level tracks upstream's. Deliberate differences: the noise is
      seeded (upstream re-randomises on every rebuild) and the wet return is one
      partition late (~23ms), inherent to uniform partitioning. Covered by
      `convolver.rs` unit tests and the engine-level
      `roomfade_delays_the_onset_of_the_reverb_tail` /
      `ir_uses_a_loaded_sample_as_the_impulse_response`.
- [x] `leslie`, `squiz` (sampler harmonic repeats), `fshift` (frequency shifter)
      — **unsupported, matching upstream**: control-only in Strudel too
      (superdough has no DSP for them; they exist to be forwarded to SuperDirt
      over OSC, which Rudel already does). A native implementation would be
      original DSP, not a port. Documented in `docs/UNSUPPORTED.md`.
- [x] `stretch` (phase vocoder): `rudel-dsp/vocoder.rs` ports the
      `phase-vocoder-processor` worklet and the `ola-processor.js` overlap-add
      framework it sits on — Hann-windowed 2048-sample frames at a 128-sample
      hop, spectral peak finding, region-of-influence shifting with phase
      correction. Wired in as the first insert of `PostFx`, matching superdough's
      chain order, via a per-sample `StretchStage` adapter.
- [x] `bytebeat`: `rudel-dsp/bytebeat.rs` ports the `byte-beat-processor`
      worklet plus the `registerSound('bytebeat')` wrapper (the 15 `defaultBeats`
      selected by `n`, the ADSR gain stage). Upstream compiles the expression
      with `new Function`, so the port carries its own parser/evaluator for the
      integer-expression subset: JS operator precedence, `ToInt32` coercion on
      the bitwise ops, 5-bit shift masking, ternaries, short-circuiting and the
      `Math` calls. Pinned against V8 by `tests/bytebeat_golden.rs` (every
      built-in beat plus 26 operator-surface cases, over 63 `t` values). Not
      ported: the exotic `chyx` helpers (unused by the built-in beats) and
      anything needing a real JS runtime. Finding this mismatch also surfaced
      that serde_json's default float parser is off by one ulp, now fixed
      workspace-wide with its `float_roundtrip` feature.
- [x] generic modulators (`lfo`/`env` + `modulate`): the LFO and envelope
      *sources* are ported from superdough's worklets and golden-tested, and
      routing is now wired, so they are audible. A modulator is an additive
      offset on its target control (matching Web Audio, where connecting a node
      to an `AudioParam` sums with the param's own value); `ModSpecs` resolves
      each descriptor entry against the built voice (`depthabs ?? depth *
      currentValue`, the 20Hz..24kHz clamp on frequency params, `sync*cps` vs
      `rate`, cycle-locked vs `retrig` phase) into a sample-rate-free spec that
      the mixer instantiates at the device rate. Targets are limited to the
      parameters Rudel's scalar DSP already varies per sample — oscillator
      frequency, `gain`, the three filters' cutoff/resonance, and the post-fx
      amounts; see `docs/UNSUPPORTED.md` for the table.
- [x] `bmod` (bus modulation): `bus`/`busgain` now route a voice's post-effect
      output into a numbered signal bus in the mixer, on top of its orbit
      routing (so `dry(0)` makes a pattern a pure modulation source), and
      `bmod` reads it back as `connectBusModulator` does —
      `(signal + dc) * depth / 0.3`, with the same frequency-param clamp the
      waveshaper applies upstream. Buses are stereo (`getStereoNode`) and sum to
      mono on the `bmod` read, which is what Web Audio does on the way into an
      `AudioParam`; the mixer renders sending voices before reading ones so a
      reader sees the same block its sender wrote. `s("bus")` reads a bus back
      as a source (`registerSound('bus')`: the bus through a linear ADSR gain),
      so a second pattern can run it through its own effects. Still unhandled: `subControl` (modulating a
      modulator) and `fxi` (which link of an `FX` chain to target), for the same
      reason `FX` is — there is no explicit effect graph. Documented in
      `docs/UNSUPPORTED.md`.
- [x] per-voice filters on the fixed-recipe voices: `lpf`/`hpf`/`bpf` (and their
      envelopes, `fanchor`, `ftype`, `drive`) used to reach only the oscillator
      and sampler voices, so `s("bd").lpf(500)` was silently inert — upstream
      these are samples and get filtered like anything else. The chain the
      oscillator voice ran inline is now `FilterSet` + `VoiceFilters`
      (`rudel-dsp/filter.rs`), one parser and one per-sample stage shared by the
      synth, drum, ZZFX, bytebeat and bus voices. The four fixed-recipe voices
      also take modulators now, though only their filter targets are read.
- [x] `duckorbit`/`duckonset`/`duckattack`/`duckdepth` (sidechain ducking of one
      orbit by another), ported from superdough's `Orbit.duck`: a voice's
      `duckorbit` dips the *target* orbit's output gain to `1 - sqrt(depth)`
      over `duckonset`, then recovers to unity over `duckattack` (min 0.002s).
      Both segments are exponential ramps, which are geometric, so `DuckEnv`
      (`rudel-dsp/bus.rs`) runs them as a constant per-sample multiplier rather
      than a `powf` per sample. Re-triggering ramps from the *current* gain, not
      from unity, matching upstream's `cancelScheduledValues` +
      `setValueAtTime(currVal)` pair. `duckorbit("2:3")` targets several orbits
      and `duckonset`/`duckattack`/`duckdepth` may be `:`-lists read per target
      (falling back to the first entry). A target orbit that does not exist yet
      is created rather than logged as an error, so the duck still lands once
      that orbit's own pattern starts.

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
- [x] `flux`/`fluxBy` (Strudel's aliases for `juxFlip`/`juxFlipBy`), bound as
      both methods and standalone callback combinators
- [x] `compressSpan`/`focusSpan`/`zoomArc` — **unsupported by design**: upstream
      these take a `TimeSpan` *object* and throw on a plain array, so they are
      internal helpers, not user API. The user-facing two-arg
      `compress`/`focus`/`zoom` are the equivalent surface.

## learn/signals

- [x] `sine` `cosine` `saw` `isaw` `tri` `square` `rand` `rand2` `irand` `run`
      `time` `perlin` — bound as Koto values/fns; `segment`/`range` on signals
- [x] bipolar variants `saw2`/`square2`/`tri2`/`isaw2`/`sine2`/`cosine2`
- [x] `mousex`/`mousey` (+ `mouseX`/`mouseY`): pointer position 0..1 across the
      app window, published to the input bus each frame by the egui app.
- [x] `envL`/`envLR`/`envEq`… (envelope signals) — **unsupported**: not defined
      in the pinned Strudel checkout. `lfo`/`env` modulators, the per-effect
      envelopes (`lpenv`/`penv`/`fmenv`/`wtenv`) and `range` over a signal cover
      the same ground.

## learn/conditional-modifiers

- [x] `every`/`firstOf`/`lastOf`, `when`, `chunk`/`chunkBack`
- [x] `sometimes`/`sometimesBy`/`often`/`rarely`/`almostAlways`/`almostNever`/`always`/`never`
- [x] `someCycles`/`someCyclesBy`, `degrade`/`degradeBy`/`undegrade`, `mask`, `struct`
- [x] `euclid`, `euclidRot`/`euclid_rot` (3-arg rotation now bound)
- [x] `euclidLegato`/`euclidLegatoRot` (gapless held pulses; rotation as a late
      offset, matching superdough's `_euclidLegato`)
- [x] `shuffle(n)` / `scramble(n)` / `randrun(n)` — slice-rearranging
      randomizers from signal.mjs (`_rearrangeWith`), parity-tested against the
      oracle. Fixing these also fixed `time_to_rands` to match Strudel's legacy
      RNG (signed values for `n > 1`, initial `xorwise` applied).
- [x] `whenKey`/`keyDown`: the held-key set is published to the input bus each
      frame, so both read the live keyboard at query time (no re-eval needed).
      Key names match the browser's `KeyboardEvent.key` values, including
      Strudel's `ctrl`/`alt`/`shift`/`up`/`down`/`left`/`right` shorthands; a
      `:`-list is a combination that must be held in full.
- [x] `filter`/`filterWhen`/`tag`: probe-and-bake like `fmap` — 16 cycles are
      queried, the predicate applied per hap, and the survivors emitted as a
      static pattern repeating with that period (the Koto VM can't run in the
      query path). `filter` gets the hap as a map
      (`{value, begin, end, wholeBegin, wholeEnd, tags}`), `filterWhen` the
      whole's begin in cycles. Needed the transpiler fix below.
- [x] transpiler quote rule: only *double*-quoted strings are mini-notation,
      matching `plugin-mini`'s `isStringWithDoubleQuotes`. Single quotes are
      plain strings, which is what upstream examples like
      `.filter(hap => hap.value.s === 'hh')` rely on. Rudel used to rewrite
      both styles.
- [x] soundfonts, both paths. **General MIDI**: `gm.mjs`'s table is generated
      into `tools/oracle/gm_table.json` (125 names, 869 preset files) and
      embedded; `rudel-audio/src/soundfont.rs` reads a WebAudioFont preset's
      zones directly (no JS eval), base64-decodes each zone's audio through the
      existing sample decoder, and ports `findZone` (its `keyRangeHigh + 1`
      off-by-one included) plus the `baseDetune`/playback-rate math and loop
      handling. Presets are fetched lazily on first use — the scheduler records
      the miss, the app drains it into the background job queue — and cached on
      disk, so the first note of a fresh instrument is silent like upstream's
      async loader. **`.sf2`**: `rudel-audio/src/sf2.rs` replaces `sfumato` with
      a direct RIFF reader (phdr/pbag/pgen -> inst/ibag/igen -> shdr, layered
      generators) producing the same `Zone`/`Preset` types, so both formats
      share one playback path. `loadSoundfont`/`setSoundfontUrl`/
      `registerSoundfonts`/`.soundfont(name, n)` are bound.
- [x] soundfont envelopes and remote `.sf2`. Reading `fontloader.mjs` settled
      the first half: upstream *also* drives a soundfont's envelope from the
      user's `value.attack`/`decay`/… plus `getVibratoOscillator` /
      `getPitchEnvelope`, not from the font's own SF2 generators — so Rudel's
      behaviour was already parity, and the real gap was that its **sampler
      voices had no vibrato or pitch envelope at all** (superdough applies both
      to every sampler's `detune`, `sampler.mjs`). Extracted the synth's vibrato
      + pitch-envelope logic into a shared `PitchMod` (`rudel-dsp/pitch.rs`) and
      gave `SamplerVoice` the same, so `vib`/`vibmod`/`penv`/`p{adsr}`/`panchor`/
      `pcurve` now shape samples, soundfonts and wavetables as they do synths.
      `loadSoundfont` now also accepts an http(s) URL (`fetch_cached_bytes`,
      factored out of the sample fetch path so both share the disk cache).
- [x] `log`/`logValues`: the message is decided at build time (one of the two
      built-in formats, or a probe-and-baked string when a formatting callback
      is given) and carried as a `_log` control; the scheduler's shared event
      extraction consumes it and writes the line as the event plays, which the
      app drains into a console panel. `onTriggerTime` keeps the evaluation's
      Koto VM alive past `eval` (`rudel-lang/src/triggers.rs`) and the app fires
      the hooks from its frame loop as each onset passes — frame-accurate, like
      upstream's `setTimeout`.
- [x] `midin(device)`/`midikeys(device)`: the input bus is keyed by device, so
      `midin` returns a `(cc[, chan]) -> pattern` factory reading one port and
      `midikeys` a `(noteLength?) -> pattern` factory of its note-ons. The port
      open is a host effect (like `samples`), so no `await` is needed. Notes
      only drain on a scheduler (`cyclist`) query, so a visualiser querying the
      same pattern doesn't eat them.
- [x] `label`/`activeLabel` (multi-control; the pianoroll swaps to `activeLabel`
      while an event sounds), `markcss` (a colour is parsed out of the CSS and
      used for the editor's flash, falling back to the `color` control),
      `getDuration`/`getDur` (the bank publishes each sample's length as it
      registers it, so the call is synchronous), `clearScope` (a no-op returning
      silence: Rudel evaluates in a fresh VM, so no user scope accumulates), and
      the `dracula` editor theme.
- [x] `ifp` — **unsupported**: not defined in the pinned Strudel checkout.

## learn/accumulation

- [x] `stack`, `superimpose`, `off`, `echo`/`stut`, `jux`/`juxBy`
- [x] `layer` (bound: `pat.layer([f, g, …])` stacks each callback's result)
- [x] `overlay` (method), `arrange` (factory)
- [x] `wchoose` (continuous), `wchooseCycles`/`wrandcat` (per-cycle) — weighted
      `[pattern, weight]` pairs; `scan(n)` (growing runs, one per cycle)

## pick family (core/pick.mjs — parity-tested)

- [x] `pick`/`pickmod` (inner join; clamp vs wrap for list lookups),
      `pickOut`/`pickmodOut` (outer join), `pickReset`/`pickmodReset` and
      `pickRestart`/`pickmodRestart` (retriggering joins),
      `inhabit`/`pickSqueeze` + `inhabitmod`/`pickmodSqueeze` (squeeze join),
      and standalone `squeeze` (== `inhabitmod` for lists). Core logic in
      `rudel-core/transforms/pick.rs` (`pick_list`/`pick_map` + `PickJoin`);
      bound as KPattern methods and prelude factories with flexible
      (lookup, selector) arg order. List lookups index by rounded number;
      map lookups go by name (missing keys → silence).
- [x] `pickF`/`pickmodF` — a pattern of indices/names picks which function
      transforms the pattern. Functions are applied eagerly via the Callback
      marshaler (the Koto VM can't run in the query path), then picked among
      with an inner join — equivalent to Strudel's
      `pat.apply(pick(lookup, pickPattern))` composition.

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
- [x] `tour` (insert a pattern into a list stepwise, rotating backwards one
      slot per repetition) and `zip` (interleave the steps of several patterns
      into one dense run), both parity-tested. Bound with the current aliases
      (`timeCat`, `steps` = `pace`) and the deprecated `s_*` family (`s_cat`,
      `s_alt`, `s_polymeter`, `s_taper`, `s_add`, `s_sub`, `s_expand`,
      `s_extend`, `s_contract`, `s_tour`, `s_zip`).
- [x] `ncat` — **unsupported**: not defined in the pinned Strudel checkout.
      `timecat`/`stepcat` cover weighted concatenation.

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
- [x] autocomplete / sound+control hints in the editor
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
        `superimpose`), plus `randrun`/`shuffle`/`scramble`/`tour`/`zip`.
      Caught and fixed real bugs: euclidean rotation was rotating the wrong
      direction (Strudel rotates right by `rotation`), and a mini-notation
      sequence with a single multi-step element reported the inner step count
      (`"[c g]"` had 2 steps; Strudel says 1 — caught by the `tour` case).
- [x] Alignment matrix (`.add.out` / `.set.squeeze` / …). Engine primitives
      `op_in`/`op_out`/`op_mix`/`op_squeeze`/`op_squeeze_out`/`op_reset`/
      `op_restart`/`op_poly` (+ `reset_join`/`restart_join`/`poly_join` and the
      steps-based `expand`/`extend`) plus an `Align` enum and `op_align`. Exposed
      `<op>_<align>` methods for add/sub/mul/div/set/keep (a macro generates
      out/mix/squeeze/squeezeout/reset/restart/poly; `in` stays the plain
      method). Bound a curated set in Koto; parity-checked against Strudel in
      `transform_parity.rs` (add.out/mix/squeeze/squeezeout/reset/restart/poly,
      mul.out, set.out/mix/squeeze/poly, keep.out).
- [x] `cp` (clap) now actually claps. Its later two exponentials were
      `(t - offset).max(0.0)`, so they sat at `exp(0) = 1` until their onset
      rather than at 0 — all three stages fired at once and the envelope decayed
      monotonically, despite the comment promising "three quick bursts". Gated
      them on instead, so the amplitude climbs again at 10ms and 20ms like the
      808 circuit (and like the recorded clap Strudel plays here — it has no
      clap synth of its own, only two commented-out SuperDirt control names).
      The peak envelope drops from 3.0 to ~1.62 with the stages no longer
      stacked, so the scale went 0.4 -> 0.74 to hold the level: rendered peak
      moves 0.606 -> 0.546, and RMS 0.293 -> 0.174 because the energy is now in
      three bursts with gaps rather than one continuous blob.
- [x] Ported `getPitchEnvelope` + `getVibratoOscillator` into the super-saw
      oracle, so the goldens drive a voice with a moving pitch rather than a
      static one. `getParamADSR` and the Web Audio param mock moved from
      `gen_adsr_oracle.mjs` into `lib.mjs` and are shared (adsr_golden.json
      regenerated byte-identical). `next_supersaw` is now 71/71 on
      cargo-mutants, up from 2 survivors.

      This found a parity bug: **`getADSRValues` was never ported.** It is not a
      field-wise "fill in the missing ones" — the defaults array is used only
      when *all four* stages are unset, and naming any one of them sends the
      rest to a clamped branch (attack/decay floor at 1ms, release floors at
      **10ms**, sustain caps at 1, and an unset sustain becomes 1 or 0.001
      depending on whether decay was given). `PitchMod::new` used `unwrap_or`
      per field, so `penv(7).pattack(0.05)` released the pitch envelope ten
      times too fast. Now shared as `envelope::adsr_values`.
- [x] Routed the **gain** ADSR through `envelope::adsr_values` too. The four
      stages now stay `Option` until they are resolved together, so
      `s("saw").attack(0.1)` gives decay 0.001 / sustain 1.0 (a held note) where
      it used to keep the defaults' decay 0.05 / sustain 0.6 (a note dropping to
      60%). The `adsr`/`ad`/`ar` list shortcuts feed the same four controls, as
      they do upstream, instead of resolving envelopes of their own — which also
      drops the hand-forced `sustain = 0.0` in `ad` (upstream reaches 0.001
      there via `getADSRValues`, and `ad(t)` is `[attack, decay = attack]`, not
      "attack/decay with no sustain").

      `cargo run -p rudel-dsp --example envelope_ab` renders the before/after
      for listening.
- [x] `ds(t)` was already implemented and correct — `rudel_core::controls::
      multi::ds`, bound in Koto, covered by a core test and an end-to-end lang
      test. The earlier note here was wrong: it inferred a gap from the missing
      `list("ds")` arm in `VoiceParams::from_controls`, but no such arm is
      needed. Like upstream, `adsr`/`ad`/`ds`/`ar` are control *setters* that
      expand into plain attack/decay/sustain/release in core, so the raw keys
      never reach the DSP layer at all — which made the `list("adsr")`/`ad`/`ar`
      arms there dead code, now deleted.
- [ ] `rewrite_arrow_functions` converts block-bodied arrows, which its own doc
      comment says it does not: "block bodies (`x => { ... }`) are *not*
      converted — Koto would read `{ ... }` as a map literal". It converts them,
      and Koto then rejects `|x| { ... }` with "expected '}' at end of map
      declaration" — pointing at a map the user never wrote. Both forms are
      errors either way, so this is about which message the user gets, not
      correctness. Leaving the arrow alone would at least fail on the `=>` the
      user actually typed. Pinned as-is by
      `rewrite_arrow_functions_maps_expression_bodies_only`.
