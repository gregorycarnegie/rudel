# Full Strudel Parity

This is the canonical parity checklist for making Rudel fully compatible with the local Strudel checkout in `strudel/`.

Completing this file should mean that Rudel can run Strudel code, mini-notation, controls, transforms, editor workflows, outputs, examples, and tests with matching behavior unless a difference is explicitly documented as intentional.

## Implementation Guidance

- Prefer adding names through existing alias groups and generated binding lists before writing a new implementation from scratch.
- Add a unique `KPattern` implementation only when the Strudel behavior cannot be represented by an existing alias, control wrapper, generated method group, or shared helper.
- When adding a new unique `KPattern` method, include parity tests that show why it needs bespoke behavior.

## Definition of Done

- [ ] Pin the exact Strudel source of truth used for parity: package versions, git commit if available, and any local patches under `strudel/`.
- [ ] Build an automated API inventory for every Strudel package under `strudel/packages`.
- [ ] For every exported function, registered pattern function, `Pattern.prototype` method, control, alias, top-level REPL helper, and editor command, mark one of: implemented, intentionally different, not applicable, or missing.
- [ ] For every implemented item, add parity tests against Strudel behavior or a documented golden output.
- [ ] For every intentionally different item, document the reason and user-visible behavior.
- [ ] Run all upstream Strudel tests that can be run locally, or port equivalent tests into Rudel where direct execution is not possible.
- [ ] Add a docs/example corpus from Strudel docs and examples, and verify Rudel parses/evaluates/renders/plays them as expected.
- [ ] Add CI checks so new Rudel changes cannot silently regress the parity surface.

## Source Packages to Audit

- [ ] `strudel/packages/core`
- [ ] `strudel/packages/mini`
- [ ] `strudel/packages/transpiler`
- [ ] `strudel/packages/codemirror`
- [ ] `strudel/packages/repl`
- [ ] `strudel/packages/reference`
- [ ] `strudel/packages/xen`
- [ ] `strudel/packages/tonal`
- [ ] `strudel/packages/edo`
- [ ] `strudel/packages/webaudio`
- [ ] `strudel/packages/superdough`
- [ ] `strudel/packages/supradough`
- [ ] `strudel/packages/sampler`
- [ ] `strudel/packages/soundfonts`
- [ ] `strudel/packages/midi`
- [ ] `strudel/packages/osc`
- [ ] `strudel/packages/desktopbridge`
- [ ] `strudel/packages/draw`
- [ ] `strudel/packages/motion`
- [ ] `strudel/packages/gamepad`
- [ ] `strudel/packages/serial`
- [ ] `strudel/packages/mqtt`
- [ ] `strudel/packages/csound`
- [ ] `strudel/packages/hydra`
- [ ] `strudel/packages/tidal`
- [ ] `strudel/packages/mondo`
- [ ] `strudel/packages/mondough`
- [ ] `strudel/packages/web`
- [ ] `strudel/packages/embed`

## Core Pattern Engine

- [ ] Match Strudel's `Pattern`, `Hap`, `State`, `TimeSpan`, `Fraction`, and `Value` data model semantics.
- [ ] Match query semantics including `query`, `queryArc`, whole spans, part spans, event splitting, event clipping, and source locations.
- [ ] Audit and implement every export and `Pattern.prototype` method in `core/pattern.mjs`.
- [ ] Audit and implement every export in `core/euclid.mjs`, including aliases such as `euclidRot`, `euclidLegato`, `euclidLegatoRot`, `euclidish`, and `eish`.
- [ ] Audit and implement every export in `core/pick.mjs`, including `pick`, `pickmod`, `pickF`, `pickmodF`, `pickOut`, `pickmodOut`, `pickRestart`, `pickmodRestart`, `pickReset`, `pickmodReset`, `inhabit`, `pickSqueeze`, `inhabitmod`, and `pickmodSqueeze`.
- [ ] Audit and implement every export in `core/signal.mjs`, including continuous signals, random signals, seed behavior, `choose`, weighted choice, `shuffle`, `scramble`, conditional transforms, keyboard signals, and `per`/`cyclesPer`.
- [ ] Audit and implement every export in `core/value.mjs`.
- [ ] Audit and implement every public utility from `core/util.mjs` that Strudel users can reach from the REPL.
- [ ] Match Strudel's currying, registration, alias, and method-chaining behavior.
- [ ] Match Strudel's higher-order function behavior for callbacks, pattern-of-functions, and pattern-valued functions.
- [ ] Match Strudel's pattern alignment behavior across all in/out/mix/squeeze/reset/restart/poly variants.
- [ ] Match Strudel's stepwise functions: `take`, `drop`, `expand`, `extend`, `contract`, `shrink`, `grow`, `tour`, `zip`, `pace`, `stepcat`, `timecat`, `stepalt`, and aliases.
- [ ] Match Strudel's sample/time transforms: `chop`, `striate`, `loopAt`, `loopAtCps`, `slice`, `splice`, `fit`, `scrub`, and related helpers.
- [ ] Match Strudel's distortion/worklet/effects pattern helpers such as `soft`, `hard`, `cubic`, `diode`, `asym`, `fold`, `sinefold`, `chebyshev`, `partials`, `phases`, `FX`, and `worklet`.
- [ ] Match Strudel REPL pattern slots and aliases from `core/repl.mjs`, including `p`, `q`, `d1`-style slots, `p1`-style slots, `q1`-style silence helpers, `cpm`, stack behavior, and hush/update behavior.
- [ ] Match Strudel scheduler behavior: CPS, latency, trigger timing, pattern replacement, and event deadlines.
- [ ] Match reset/timeline/impure behavior from `core/impure.mjs` where it is user-visible.
- [ ] Match speech support from `core/speak.mjs` or document it as intentionally unsupported.

## Mini-Notation and Transpilation

- [ ] Match `mini/krill.pegjs` grammar behavior.
- [ ] Match `mini/krill-parser.js` output for all upstream mini tests.
- [ ] Match `mini/mini.mjs` APIs: `mini2ast`, `getLeaves`, `getLeafLocation`, `getLeafLocations`, `mini`, `m`, `h`, `minify`, and `miniAllStrings`.
- [ ] Preserve source locations from mini-notation leaves through Rudel patterns.
- [ ] Match mini-notation parser edge cases: nesting, alternation, Euclidean syntax, polymeter syntax, ratios, weights, rests, holds, lists, subdivisions, repetition, degradation, randomness, and source offsets.
- [ ] Match `transpiler/transpiler.mjs` behavior.
- [ ] Match `transpiler/plugin-mini.mjs` double-quoted mini-notation transformation.
- [ ] Match `transpiler/plugin-widgets.mjs` inline widget transformation.
- [ ] Match `transpiler/plugin-sample.mjs` sample shorthand behavior.
- [ ] Match `transpiler/plugin-kabelsalat.mjs` behavior or document it as intentionally unsupported.
- [ ] Support Strudel-style JavaScript user-code conveniences in Rudel's language layer or provide a compatibility translation path.
- [ ] Add differential tests that compare Rudel and Strudel AST/transpiled output for representative code snippets.

## Controls Parity

Source: `strudel/packages/core/controls.mjs`.

Checked items mean the Strudel-style chained control name is exposed in Rudel's public Koto/Rust surface. Unchecked items are missing as dedicated Strudel-compatible control methods, even if a value could be set manually with `.ctrl("name", value)`.

Ranges such as `fmh3`-`fmh8` mean every control name in that range is still unchecked.

### Sound, Pitch, and Amplitude

- [x] `s`
- [x] `sound`
- [x] `source`
- [x] `src`
- [x] `n`
- [x] `i`
- [x] `note`
- [x] `freq`
- [x] `accelerate`
- [x] `velocity`
- [x] `vel`
- [x] `gain`
- [x] `postgain`
- [x] `amp`
- [x] `bank`
- [x] `pan`
- [x] `speed`
- [x] `stretch`
- [x] `unit`
- [x] `clip`
- [x] `legato`
- [x] `duration`
- [x] `dur`

### Wavetable and Warp Controls

- [x] `wt`
- [x] `wavetablePosition`
- [x] `wtenv`
- [x] `wtattack`
- [x] `wtatt`
- [x] `wtdecay`
- [x] `wtdec`
- [x] `wtsustain`
- [x] `wtsus`
- [x] `wtrelease`
- [x] `wtrel`
- [x] `wtrate`
- [x] `wtsync`
- [x] `wtdepth`
- [x] `wtshape`
- [x] `wtdc`
- [x] `wtskew`
- [x] `warp`
- [x] `wavetableWarp`
- [x] `warpattack`
- [x] `warpatt`
- [x] `warpdecay`
- [x] `warpdec`
- [x] `warpsustain`
- [x] `warpsus`
- [x] `warprelease`
- [x] `warprel`
- [x] `warprate`
- [x] `warpdepth`
- [x] `warpshape`
- [x] `warpdc`
- [x] `warpskew`
- [x] `warpmode`
- [x] `wavetableWarpMode`
- [x] `wtphaserand`
- [x] `wavetablePhaseRand`
- [x] `warpenv`
- [x] `warpsync`

### FM and Supersaw Controls

- [x] `fmh`
- [ ] `fmh1`
- [x] `fmh2`
- [ ] `fmh3`-`fmh8`
- [x] `fmi`
- [ ] `fmi1`
- [x] `fmi2`
- [ ] `fmi3`-`fmi8`
- [x] `fm`
- [ ] `fm1`-`fm8`
- [x] `fmenv`
- [ ] `fmenv1`-`fmenv8`
- [x] `fme`
- [x] `fmattack`
- [ ] `fmattack1`
- [x] `fmattack2`
- [ ] `fmattack3`-`fmattack8`
- [x] `fmatt`
- [ ] `fmatt1`-`fmatt8`
- [x] `fmwave`
- [ ] `fmwave1`
- [x] `fmwave2`
- [ ] `fmwave3`-`fmwave8`
- [x] `fmdecay`
- [ ] `fmdecay1`
- [x] `fmdecay2`
- [ ] `fmdecay3`-`fmdecay8`
- [x] `fmdec`
- [ ] `fmdec1`-`fmdec8`
- [x] `fmsustain`
- [ ] `fmsustain1`
- [x] `fmsustain2`
- [ ] `fmsustain3`-`fmsustain8`
- [x] `fmsus`
- [ ] `fmsus1`-`fmsus8`
- [x] `fmrelease`
- [ ] `fmrelease1`
- [x] `fmrelease2`
- [ ] `fmrelease3`-`fmrelease8`
- [x] `fmrel`
- [ ] `fmrel1`-`fmrel8`
- [ ] `fmi11`-`fmi88`
- [ ] `fm11`-`fm88`
- [x] `unison`
- [x] `detune`
- [x] `det`
- [x] `spread`

### Envelopes and Sample Windows

- [x] `attack`
- [x] `att`
- [x] `decay`
- [x] `dec`
- [x] `sustain`
- [x] `sus`
- [x] `release`
- [x] `rel`
- [x] `hold`
- [x] `begin`
- [x] `end`
- [x] `loop`
- [x] `loopBegin`
- [x] `loopb`
- [x] `loopEnd`
- [x] `loope`
- [x] `pattack`
- [x] `patt`
- [x] `pdecay`
- [x] `pdec`
- [x] `psustain`
- [x] `psus`
- [x] `prelease`
- [x] `prel`
- [x] `penv`
- [x] `pcurve`
- [x] `panchor`
- [x] `gate`
- [x] `gat`

### Filters and Filter Envelopes

- [x] `cutoff`
- [x] `ctf`
- [x] `lpf`
- [x] `lp`
- [x] `resonance`
- [x] `lpq`
- [x] `hcutoff`
- [x] `hpf`
- [x] `hp`
- [x] `hresonance`
- [x] `hpq`
- [x] `bandf`
- [x] `bpf`
- [x] `bp`
- [x] `bandq`
- [x] `bpq`
- [x] `lpenv`
- [x] `lpe`
- [x] `hpenv`
- [x] `hpe`
- [x] `bpenv`
- [x] `bpe`
- [x] `lpattack`
- [x] `lpa`
- [x] `hpattack`
- [x] `hpa`
- [x] `bpattack`
- [x] `bpa`
- [x] `lpdecay`
- [x] `lpd`
- [x] `hpdecay`
- [x] `hpd`
- [x] `bpdecay`
- [x] `bpd`
- [x] `lpsustain`
- [x] `lps`
- [x] `hpsustain`
- [x] `hps`
- [x] `bpsustain`
- [x] `bps`
- [x] `lprelease`
- [x] `lpr`
- [x] `hprelease`
- [x] `hpr`
- [x] `bprelease`
- [x] `bpr`
- [x] `ftype`
- [x] `fanchor`
- [x] `lprate`
- [x] `lpsync`
- [x] `lpdepth`
- [x] `lpdepthfrequency`
- [x] `lpdepthfreq`
- [x] `lpshape`
- [x] `lpdc`
- [x] `lpskew`
- [x] `bprate`
- [x] `bpsync`
- [x] `bpdepth`
- [x] `bpdepthfrequency`
- [x] `bpdepthfreq`
- [x] `bpshape`
- [x] `bpdc`
- [x] `bpskew`
- [x] `hprate`
- [x] `hpsync`
- [x] `hpdepth`
- [x] `hpdepthfrequency`
- [x] `hpdepthfreq`
- [x] `hpshape`
- [x] `hpdc`
- [x] `hpskew`

### Modulation, Delay, and Effects

- [x] `vib`
- [x] `vibrato`
- [x] `v`
- [x] `vibmod`
- [x] `vmod`
- [x] `noise`
- [x] `delay`
- [x] `delayfeedback`
- [x] `delayfb`
- [x] `dfb`
- [x] `delayspeed`
- [x] `delaytime`
- [x] `delayt`
- [x] `dt`
- [x] `delaysync`
- [x] `delays`
- [x] `ds`
- [x] `dry`
- [x] `crush`
- [x] `coarse`
- [x] `tremolo`
- [x] `trem`
- [x] `tremolosync`
- [x] `tremolodepth`
- [x] `tremdepth`
- [x] `tremoloskew`
- [x] `tremskew`
- [x] `tremolophase`
- [x] `tremphase`
- [x] `tremoloshape`
- [x] `tremshape`
- [x] `phaser`
- [x] `phaserrate`
- [x] `ph`
- [x] `phasersweep`
- [x] `phs`
- [x] `phasercenter`
- [x] `phc`
- [x] `phaserdepth`
- [x] `phd`
- [x] `phasdp`
- [x] `chorus`
- [x] `drive`
- [x] `duck`
- [x] `duckdepth`
- [x] `duckonset`
- [x] `duckons`
- [x] `duckattack`
- [x] `duckatt`
- [x] `datt`
- [x] `byteBeatExpression`
- [x] `bbexpr`
- [x] `bb`
- [x] `byteBeatStartTime`
- [x] `bbst`
- [x] `channels`
- [x] `ch`
- [x] `pw`
- [x] `pwrate`
- [x] `pwr`
- [x] `pwsweep`
- [x] `pws`
- [x] `channel`
- [x] `cut`
- [x] `djf`
- [x] `lock`
- [x] `fadeTime`
- [x] `fadeOutTime`
- [x] `fadeInTime`
- [x] `leslie`
- [x] `lrate`
- [x] `lsize`

### Tonal, Voicing, and Spatial Controls

- [x] `mtranspose`
- [x] `ctranspose`
- [x] `degree`
- [x] `harmonic`
- [x] `stepsPerOctave`
- [x] `octaveR`
- [x] `nudge`
- [x] `octave`
- [x] `oct`
- [x] `orbit`
- [x] `o`
- [x] `bus`
- [x] `busgain`
- [x] `bgain`
- [x] `overgain`
- [x] `overshape`
- [x] `panspan`
- [x] `pansplay`
- [x] `panwidth`
- [x] `panorient`
- [x] `slide`
- [x] `semitone`
- [x] `voice`
- [x] `chord`
- [x] `dictionary`
- [x] `dict`
- [x] `anchor`
- [x] `offset`
- [x] `octaves`
- [x] `mode`

### Reverb, Room, IR, and Distortion

- [x] `room`
- [x] `roomlp`
- [x] `rlp`
- [x] `roomdim`
- [x] `rdim`
- [x] `roomfade`
- [x] `rfade`
- [x] `ir`
- [x] `iresponse`
- [x] `irspeed`
- [x] `irbegin`
- [x] `roomsize`
- [x] `size`
- [x] `sz`
- [x] `rsize`
- [x] `shape`
- [x] `distort`
- [x] `dist`
- [x] `distortvol`
- [x] `distvol`
- [x] `distorttype`
- [x] `disttype`
- [x] `compressor`
- [x] `compressorKnee`
- [x] `compressorRatio`
- [x] `compressorAttack`
- [x] `compressorRelease`

### SuperDirt, SuperDough, ZZFX, and Miscellaneous Controls

- [x] `analyze`
- [x] `fft`
- [x] `squiz`
- [x] `vowel`
- [x] `waveloss`
- [x] `density`
- [x] `expression`
- [x] `sustainpedal`
- [x] `fshift`
- [x] `fshiftnote`
- [x] `fshiftphase`
- [x] `triode`
- [x] `krush`
- [x] `kcutoff`
- [x] `octer`
- [x] `octersub`
- [x] `octersubsub`
- [x] `ring`
- [x] `ringf`
- [x] `ringdf`
- [x] `freeze`
- [x] `xsdelay`
- [x] `tsdelay`
- [x] `real`
- [x] `imag`
- [x] `enhance`
- [x] `comb`
- [x] `smear`
- [x] `scram`
- [x] `binshift`
- [x] `hbrick`
- [x] `lbrick`
- [x] `frameRate`
- [x] `frames`
- [x] `hours`
- [x] `minutes`
- [x] `seconds`
- [x] `songPtr`
- [x] `uid`
- [x] `val`
- [ ] `cps`
- [x] `zrand`
- [x] `curve`
- [x] `deltaSlide`
- [x] `pitchJump`
- [x] `pitchJumpTime`
- [x] `znoise`
- [x] `zmod`
- [x] `zcrush`
- [x] `zdelay`
- [x] `zzfx`
- [x] `color`
- [x] `colour`
- [x] `transient`
- [x] `FXrelease`
- [x] `FXrel`
- [x] `FXr`
- [x] `fxr`

### MIDI and OSC Controls

- [x] `midichan`
- [x] `midimap`
- [x] `midiport`
- [x] `midicmd`
- [x] `ccn`
- [x] `ccv`
- [x] `ctlNum`
- [x] `nrpnn`
- [x] `nrpv`
- [x] `progNum`
- [x] `sysexid`
- [x] `sysexdata`
- [x] `midibend`
- [x] `miditouch`
- [x] `polyTouch`
- [x] `oschost`
- [x] `oscport`

### Other APIs in `controls.mjs`

- [ ] Match Strudel behavior for `adsr`, `ad`, `ds`, and `ar` envelope helpers. Rudel currently exposes some of these names as simple controls, not all of Strudel's multi-control expansion behavior.
- [ ] Implement `control([ccn, ccv])` MIDI helper.
- [ ] Implement `sysex([id, data])` MIDI helper.
- [ ] Implement `as(mapping)` batch control mapper.
- [ ] Implement `scrub(begin)` sample scrub helper.
- [ ] Implement `createParams(...)` / custom control parameter creation.
- [ ] Implement `modulate(type, config, id)`, `lfo(config, id)`, `env(config, id)`, and `bmod(config, id)` behavior.
- [ ] Verify alias canonicalization matches Strudel's `getControlName` for every alias above.

## Xenharmonic, Tonal, and EDO

- [ ] Match every export in `xen/xen.mjs`, including `edo`, `xen`, `withBase`, `ftrans`, `fTrans`, `ftranspose`, `fTranspose`, and `tuning`.
- [ ] Match every export in `xen/tune.mjs`, including named scale lookup behavior and `tunejs` scale compatibility.
- [ ] Verify Rudel's generated tune table against `xen/tunejs.js` and upstream scale data.
- [ ] Match all examples from Strudel's xenharmonic docs.
- [ ] Match every export in `tonal/tonal.mjs`, including `transpose`, `trans`, `scaleTranspose`, `scaleTrans`, `strans`, and `scale`.
- [ ] Match every export in `tonal/tonleiter.mjs`, including chord tokenization, pitch-class conversion, scale-step behavior, named scales, voicing rendering, and note transposition.
- [ ] Match every export in `tonal/voicings.mjs`, including dictionaries, aliases, ranges, `voicings`, `rootNotes`, and `voicing`.
- [ ] Match `tonal/ireal.mjs` simple and complex dictionaries.
- [ ] Match every export and test in `edo`, including intervals, ratios, pitch naming, and EDO scale behavior.

## Audio and Output Backends

- [ ] Match `webaudio/webaudio.mjs` output behavior, including oscillator fallback, sample/synth controls, `webaudioRepl`, and `Pattern.prototype.dough`.
- [ ] Match `webaudio/scope.mjs`, `webaudio/spectrum.mjs`, and `webaudio/supradough.mjs`, including `tscope`, `fscope`, `scope`, `spectrum`, and `supradough`.
- [ ] Match `superdough` synthesis and sampler behavior, including wavetable, vowel, feedback delay, reverb generation, modulators, ZZFX, node pools, and worklet behavior.
- [ ] Match `supradough` behavior or document any intentional difference.
- [ ] Match `sampler` package behavior, including sample server expectations and remote/local sample resolution.
- [ ] Match `soundfonts` behavior, including font loading, GM map, and SoundFont playback.
- [ ] Match sample naming, bank resolution, sample indexes, sample URL schemes, and sample caching behavior.
- [ ] Match Strudel's gain, clipping, pan, envelope, effect, reverb, distortion, compressor, modulation, and bus semantics audibly enough for representative examples.
- [ ] Add audio golden tests where deterministic output is possible, and smoke tests for real-time outputs where exact samples are not stable.

## MIDI, OSC, and Bridges

- [ ] Match `midi/midi.mjs` output behavior, including `Pattern.prototype.midi`, device selection, default maps, channel behavior, note-on/off timing, CC, NRPN, pitch bend, aftertouch, sysex, and MIDI clock commands.
- [ ] Match `midi/input.mjs`, including incoming MIDI signals and `ccin`-style behavior.
- [ ] Match `osc/osc.mjs` and `osc/superdirtoutput.js`, including control parsing, event timing, SuperDirt OSC address/value behavior, and target routing.
- [ ] Match `desktopbridge` MIDI, OSC, and logger bridges where Rudel exposes equivalent desktop behavior.
- [ ] Add integration tests with fake MIDI/OSC devices or loopback ports.

## Editor, REPL, and Live Coding UX

- [ ] Add inline UI controls as code inputs in the editor, matching Strudel-style live widgets such as sliders/knobs/toggles embedded in pattern code.
- [ ] Support Strudel-style inline UI values as live pattern inputs, not just visual editor widgets.
- [ ] Add inline animations/visuals in the editor so code can create or drive visual feedback directly.
- [ ] Support Strudel-style inline animation/visual outputs as first-class runtime effects.
- [ ] Add `Ctrl+\` comment/uncomment for the current line or current selection.
- [x] Add basic syntax highlighting for Rudel/Strudel-like code.
- [ ] Upgrade syntax highlighting to Strudel/CodeMirror-grade highlighting, including richer token categories and mini-notation awareness.
- [ ] Add active-event highlighting for mini-notation and code spans while playback runs.
- [ ] Preserve source locations through preprocessing/evaluation so live playback can point back to the exact code that produced each hap.
- [ ] Add editor conveniences expected from Strudel's CodeMirror-based REPL, such as bracket matching, selection-aware commands, and completion/reference help.
- [ ] Add a reference/autocomplete surface generated from this parity data and Strudel's `reference` package.
- [ ] Audit keyboard shortcuts against Strudel's REPL and document the supported subset.
- [ ] Match `codemirror` package behavior: autocomplete, highlight, flash, widgets, sliders, labels, block utilities, tooltips, keybindings, themes, and HTML helpers.
- [ ] Match `repl/repl-component.mjs`, `repl/prebake.mjs`, and `repl/index.mjs` user-visible behavior.
- [ ] Match code evaluation semantics: update while playing, hush, multiple outputs, output routing, error reporting, user-defined state, and reset behavior.
- [ ] Add tests or scripted UI checks for editor shortcuts, inline controls, inline visual feedback, active-event highlighting, and live update behavior.

## Visuals, Draw, Motion, and External Inputs

- [ ] Match `draw` package behavior: pianoroll, pitchwheel, spiral, color, draw, and animate.
- [ ] Match Hydra integration or document it as intentionally unsupported.
- [ ] Match motion/device-motion input package behavior or document it as intentionally unsupported.
- [ ] Match gamepad input package behavior or document it as intentionally unsupported.
- [ ] Match serial and MQTT packages or document them as intentionally unsupported.
- [ ] Match `csound` package behavior or document it as intentionally unsupported.
- [ ] Match `tidal`, `mondo`, and `mondough` packages or document them as intentionally unsupported.
- [ ] Match `web` and `embed` package user-visible embedding behavior where Rudel has an equivalent surface.

## Reference, Docs, and Examples

- [ ] Generate a complete reference table from Strudel's `reference` package and compare it with Rudel's exposed names.
- [ ] Port or execute every runnable example from Strudel docs against Rudel.
- [ ] Add a parity example suite covering first sounds, notes, effects, pattern effects, mini-notation, tonal, xen, MIDI, OSC, samples, synths, and visual feedback.
- [ ] Keep `FULL_STRUDEL.md` in sync with generated API inventories so manual drift is obvious.
- [ ] Document every unsupported or intentionally different feature in user-facing docs.

## Test Strategy

- [ ] Add a Strudel differential harness that can query local Strudel patterns through Node and compare Rudel haps for deterministic examples.
- [ ] Add golden tests for mini-notation ASTs, event spans, values, controls, and source locations.
- [ ] Add property tests for core time transforms where exact Strudel behavior can be expressed generically.
- [ ] Add snapshot tests for reference docs/autocomplete output.
- [ ] Add audio smoke tests for WebAudio/superdough-equivalent paths.
- [ ] Add MIDI/OSC loopback tests.
- [ ] Add editor automation tests for syntax highlighting, widgets, shortcuts, active highlights, and live-code updates.
- [ ] Add performance benchmarks against representative Strudel patterns.
- [ ] Add regression tests for every bug found while working through this checklist.

## Migration and Maintenance

- [ ] Add a script to regenerate the controls/API checklist from `strudel/packages` so `FULL_STRUDEL.md` can be audited after Strudel updates.
- [ ] Add a documented process for updating Rudel when the vendored Strudel version changes.
- [ ] Check licensing requirements for any code, data, samples, or generated tables ported from Strudel.
