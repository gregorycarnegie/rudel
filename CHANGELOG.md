# Changelog

Notable changes to Rudel. Format follows [Keep a Changelog][kac]; versioning is
[semantic][semver], with the pre-1.0 convention that the minor number carries
breaking changes.

This file starts at 0.7.0. Earlier history is in the git log.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- **The tune corpus is now compared against Strudel's haps, not just run.**
  Upstream's `tunes.test.mjs` snapshots `queryCode(tune, testCycles[key])` for
  every website tune — each hap's spans and every control that reaches the synth
  — and commits the result. `gen_tunes_oracle.mjs` carries that into the corpus
  (parsing the committed snapshot rather than standing up a Strudel runtime, so
  the expectation is upstream's own), and `tunes.rs` compares hap for hap. 11 of
  the 27 website tunes that evaluate reproduce it exactly; the other 16 are
  named with the difference in `tunes_parity_allowlist.json`, which fails in
  both directions like every other allowlist here.

  Two representation differences are normalised rather than reported, because
  they are spellings and not events: `note` names against MIDI numbers (Strudel
  converts at playback, Rudel when the control is set), and float formatting,
  compared to nine decimals.

### Fixed

- **An unnamed `value` was not promoted into the control that followed it.**
  Strudel's `withVal` moves it — `bag = {...xs}; xs = xs.value; delete bag.value`
  — so `"A5".color('#54C571').note()` is `{note: "A5", color: …}`. Rudel left
  maps untouched, emitting `{value: "A5", color: …}` with the control never set,
  which reaches the voices as silence; tunes routinely colour or label a layer
  before naming its sound. Fixed in `controls/base.rs`, where every control
  spelling goes through one helper, so it holds for all three of `createParam`'s
  paths — bare method, standalone function, and argument. Swimming, Wavy
  kalimba, Zelda's Rescue, Bass fuge, Barry Harris and the sample-drum demo went
  from wrong to exact on the back of it.

- **Arithmetic on a note name treated it as zero.** Every composer upstream is
  wrapped in `numeralArgs`, so `parseNumeral` resolves `"c2"` to 36 before the
  op runs. Rudel's coercion gave up on any non-numeric string and fell back to
  zero, so `"<c2 c3 f2>".add("0,.02")` produced `0` and `0.02` — a bass line
  played at the bottom of the keyboard instead of transposed. The bitwise ops
  already did this correctly, with the reasoning written out; the arithmetic
  ones never got it.
- **`voicing()` emitted bare numbers instead of a `note` control.** Upstream
  ends with `stack(...notes).note().set(rest)`. A bare number sounds on its own
  but composes wrongly: the `.add(note("0,.1"))` a tune uses to detune a voicing
  unioned `{note: 0}` onto `{value: 58}` and left the voiced note in `value`
  with the control unset.
- **`piano()` defaulted `clip` instead of setting it.** Upstream opens with
  `this.clip(1)`, which overwrites whatever the chain had already put there, so
  an echo that shortens each repeat with `.clip(1/(i+1))` still arrives at 1.
- **`vib` was a single-key control.** Upstream registers
  `registerControl(['vib', 'vibmod'], 'vibrato', 'v')`, so `v("8:.125")` spreads
  across rate and depth; Rudel put the whole list under `vib`.
- **`f64` time values were rounded onto a fixed 1/1,000,000 grid.** The bound
  exists for a real reason — the exact rational behind an `f64` has a
  denominator near 2^52, and pattern arithmetic multiplies denominators until
  they overflow — but rounding destroys the simple fractions a tune is made of:
  `1/6` became `166667/1000000`, and `.fast(2/3)` put every span on a
  denominator of 666667. `Frac::from_f64` now walks the continued-fraction
  convergents and stops at the same bound, which is what Fraction.js does, so
  `1/6` is `1/6` and an irrational still lands on a small denominator.

- **A plain string argument was parsed as mini-notation.** Upstream only treats
  a string as mini when the transpiler wrapped it — double quotes and backticks,
  never single quotes — and `reify` leaves the rest alone, because the
  string-parser hook is installed by `miniAllStrings()`, which nothing calls.
  Rudel parsed every string, so `cat('C3 dorian', 'Bb2 major')` became a
  sequence of four words and `.scale(...)` was handed `"C3"` and `"dorian"` as
  scale names on alternating cycles, producing notes belonging to no scale.
- **A fractional scale degree was rounded, not ceiled.** `scaleStep` opens with
  `step = Math.ceil(step)`. It shows up wherever a degree is computed rather
  than written — `n("0").add(n(rand.range(0,12))).scale(...)` lands on a
  fraction every time — and put roughly half of those a degree low.
- **`rootNotes` returned a MIDI number instead of a note name.** Upstream builds
  `root + octave` (`"C4"`), and the difference matters downstream: a number is
  indistinguishable from a scale degree, so `rootNotes(4).scale('C minor')` read
  60 as *degree* 60 and landed eight octaves up.
- **`superimpose` applied only its first function.** It is variadic upstream —
  `stack(this, ...funcs.map(f => f(this)))`, the same shape as `layer` — so a
  tune superimposing two voices silently lost the second.

### Internal

- `wrap_control_dyn` is now `control_dyn`: the distinction only existed because
  the bag rule was applied on one path.
- Tune parity went from 11 exact to 23 of 27 on the back of the above. The four
  left are named in `tunes_parity_allowlist.json`; all four are now extra or
  missing haps around a wrapped copy (`off`, `jux` + `late`), not wrong values.

## [0.8.0] — 2026-08-09

A release about whole tunes. The example corpus had only ever been *snippets* —
one documented line at a time — and running the complete songs behind
<https://strudel.cc/examples> for the first time found that 8 of 46 evaluated.
Most of the rest failed on how tunes are *written* rather than on anything they
ask the engine to do: a method chain indented inside a `stack(...)`, a comma
alone on its own line, a melody in a backtick template. **One fix changes what
existing patterns do** — see Migrating below.

### Added

- **A whole-tune corpus test** (`crates/rudel-lang/tests/tunes.rs`, corpus from
  `tools/oracle/gen_tunes_oracle.mjs`). 46 complete scripts: the 31 tunes behind
  strudel.cc/examples, plus upstream's own community-song fixtures. Where
  `doc_examples.rs` pins reach across thousands of one-liners, this pins the
  shapes only a real song has — tens of lines, comments, `const` bindings, arrow
  callbacks, multi-statement bodies. Each tune has to evaluate *and* produce
  events over four cycles: a tune that parses into silence fails like one that
  fails to parse, which is what caught two of the fixes below. Anything Rudel
  genuinely cannot run is named with a reason in `tunes_allowlist.json`, and the
  test fails both ways. 8 of 46 ran when it was added; 37 do now, the 9
  allowlisted being Csound, Tone.js, `addVoicings`, the `add.out` getter form,
  and three pre-2022 fixtures using bare note-name globals.
- `useRNG(mode)`. Rudel ports Strudel's default `legacy` generator bit-exactly,
  so `useRNG('legacy')` is a no-op; `'precise'` says it is not ported rather than
  quietly returning different random numbers. Nine tunes open with this line.

### Fixed

- **A method chain indented inside a call ended the argument list.**
  `indent_dot_continuations` only recognised a continuation when the `.` sat in
  column 0, but tunes write their chains as arguments, at the argument's own
  indent, where Koto reads the next line as the next argument. Indenting the
  line is not enough on its own either — whatever the line's own brackets open
  has to move with it, or a chained call's arguments end up level with the call.
  The pass now tracks the column each continuation was pushed to, keyed by
  bracket depth, and holds everything nested inside it further in. It only ever
  adds indentation, so a hand-written Koto block keeps its shape.
- **A `,` alone on a line ended the argument list.** Tunes separate the layers of
  a long `stack(...)` that way so each can be commented and reordered. The comma
  is hoisted onto the end of the line above, which keeps the line count — and so
  the line numbers in error messages — unlike joining the two.
- **Controls called with no argument returned silence.** `"0 2 4".note()`,
  `"bd sd".s()` and `.piano()` set the control from the *missing* argument, i.e.
  from silence. Strudel's `createParam` has an explicit branch for this
  (`if (typeof value === 'undefined') return pat.fmap(withVal)`): the pattern's
  own values become the control. Whole tunes are written this way, and evaluated
  to nothing.
- **A patterned scale returned silence.** `.scale("<C:major C:mixolydian>")` read
  its argument back as raw text, handing `parse_scale` the mini source instead of
  the pattern, so it matched no scale and dropped every hap. The argument now
  follows Strudel's quoting rule, which the preprocessor already applies
  elsewhere: a double-quoted argument went through the mini parser and stays a
  pattern, a single-quoted one is a literal name that may contain spaces
  (`scale('C bebop major')`).
- **`samples({` with its keys at column 0 was parsed as labels.** A multi-line
  sample map writes `bd: [...]` exactly like a `$:`-style label, and the label
  rewriter took it, splitting the literal open. Label detection now only runs
  outside open brackets.
- **Backtick template literals were a parse error.** Tunes spell a whole melody
  as one multi-line mini string. The scanner reads them as strings — keeping
  their brackets and quotes out of every pass that counts them — and they are
  rewritten to Koto raw strings. The editor colours them like any other string,
  with mini highlighting inside.

### Changed

- **`.chord()` with no argument sets the chord control instead of expanding to
  notes.** Strudel registers `chord` as a plain control and nothing else, so a
  bare `.chord()` promotes the pattern's own values into it for `.dict(...)` and
  `.voicing()` to read. Expanding the names straight to note stacks left
  `.voicing()` nothing to voice: Giant Steps, which spells its chords
  `seq(...).chord().dict('lefthand').voicing()`, lost that entire layer — 58 haps
  where Strudel produces 147. See Migrating.

### Internal

- Measured against Strudel's own checked-in tune snapshot
  (`test/__snapshots__/tunes.test.mjs.snap`, at its per-tune cycle counts): of
  the 27 example tunes that evaluate, **21 now produce Strudel's exact hap
  count**, including every long one — Swimming 553, Wavy kalimba 896, Caverave
  751, Underground plumber 402, sml1 328. The six that differ are small
  (amensister +4, festivalOfFingers3 +16, giantSteps +9, holyflute −1,
  juxUndTollerei +4, sampleDemo +2) and are the next thing to work down.
- Unit tests pin the preprocessor rules the tunes exercised — a chain held
  together inside a call, a second `.` line aligning with the first rather than
  stepping right, arguments written hard against the call's own column, a comma
  hoisted over blank lines but not onto a line that cannot take one, and a map
  key inside an open bracket staying a map key.
- `docs/API_INVENTORY.md` and the reference surface regenerated for `useRNG`,
  which leaves both allowlists one entry shorter.

### Migrating

`.chord()` with no argument no longer expands chord names to note stacks. If you
were relying on that, `chord_notes` is still what it always was in `rudel-core`;
in a script, spell the expansion out:

| pattern | was | now | to restore |
| --- | --- | --- | --- |
| `"C".chord()` | notes 48, 52, 55 | `{chord: "C"}` | `"C".chord().voicing()` |

Patterns that already went on to `.dict(...)`/`.voicing()` — which is how every
tune spells it — were producing silence and now play.

## [0.7.2] — 2026-08-09

A performance release. Profiling the running app for the first time showed the
cost was not where the DSP benchmarks had been looking.

### Fixed

- **Inline widgets re-queried the whole pattern on every repaint.** A pianoroll,
  spiral or pitchwheel draws a window around the playhead — `DrawWindow::around`
  spans four cycles — and because that window slides continuously, nothing was
  reusable between frames: each widget re-ran `Pattern::query_arc` over four
  cycles at the repaint rate, and `_scope`/`_spectrum` did it twice (once more
  for the active hap's colour). Patterns are lazily-evaluated closures with no
  memoisation, so a redraw cost the same as a first evaluation. Widgets now
  query whole cycles — which change once per cycle, not once per frame — cache
  that in egui's temp store, and slice the visible window out of it.
- **The active-event highlight did the same.** `active_source_spans` already
  queried a whole cycle, so it only needed splitting into the expensive
  `cycle_flashes` (cached) and the per-frame `spans_at`.

Both caches are keyed on a pattern generation counter bumped wherever the
current pattern is replaced, so a re-eval drops them; the widget cache is also
keyed on the widget's source range, which slides as you type without an eval.

Measured with `samply` on real sessions: the UI thread went from 86.5% of
process CPU to 65.6%, and `Pattern::query_arc` from 72.9% of that thread to
3.3% — nearly all of what remains being the once-per-cycle cache miss that has
to happen. The audio thread was never the bottleneck: it sat at ~6% of process
CPU throughout, two thirds of it parked in `NtWaitForMultipleObjects`, matching
what the mixer benchmarks predicted.

### Added

- A `profiling` cargo profile (release codegen plus line tables), so
  `samply record ./target/profiling/rudel.exe` resolves frames to source without
  putting debug info in normal release builds.

### Internal

- Tests pin what the caches are allowed to get wrong: that a cached whole-cycle
  query returns exactly what querying the sliding window directly would, that a
  generation bump drops the previous pattern's haps (and that *not* bumping
  keeps them), and that a cached cycle still tracks the playhead rather than
  freezing the highlight on one event.
- `paint_pattern_widget` takes the `WidgetPaintInput` it was being handed field
  by field.

## [0.7.1] — 2026-08-03

### Fixed

- **The transport buttons stopped taking clicks when an editor overlay scrolled
  behind them.** Widget surfaces (pianoroll, punchcard, …) and inline sliders
  are foreground areas anchored to their code line, clipped to the editor so
  they never paint over the panels around it — but only their *painting* was
  clipped. A surface scrolled up behind the transport bar kept its full interact
  rect there, and egui's hit-test drops the layers below a foreground area that
  covers the pointer, so Play/Eval/Hush/Panic silently swallowed the click even
  though the surfaces are marked non-interactable (an `Area` always registers a
  hover-sensing widget). The overlays are now bounded to the editor's visible
  area, which egui intersects the interact rect with, without re-enabling the
  position clamping that used to make scrolled sliders jump.

## [0.7.0] — 2026-08-01

A parity and testing release. Several long-standing divergences from Strudel
turned up once the DSP voices were compared against superdough sample-for-sample
rather than merely asserted to make a sound. **Three of the fixes change what
existing patterns sound like** — see Migrating below.

### Fixed

- **Mini-notation no longer crashes the process on deeply nested input.** Every
  level of `[]`/`<>`/`{}`/`()` recursed through the parser, and every chained
  operator through the pattern builder, so a few hundred levels overflowed the
  stack. That is an abort, not a panic: no `Result` could catch it and the
  editor buffer went with it. Nesting deeper than 64 is now rejected before the
  grammar runs, which reads as silence like any other invalid pattern. Reachable
  by paste, or by a script that generates mini-notation.
- **A one-voice super-saw played ~9 cents flat.** `s("supersaw").unison(1)` kept
  subtracting the per-voice centering offset even though superdough's
  `getDetuner` returns a flat `0` below two voices, detuning the whole voice by
  half the spread.
- **The pitch envelope released ten times too fast.** `getADSRValues` was never
  ported: naming any one of `pattack`/`pdecay`/`psustain`/`prelease` sends the
  rest to a clamped branch where release floors at 10ms, but Rudel filled each
  stage in from its own defaults, so an unset release stayed at 1ms.
- **`.shape(1)` put NaN into the mix.** The waveshaper backs its divisor off
  from 1 with upstream's `1.0 - 4e-10`, which is fine in JS doubles but rounds
  straight back to 1.0 in `f32` — so `1 - shape` was zero, the shaping
  coefficient infinite, and the result `inf/inf`. NaN does not stay local; it
  propagates through everything downstream of the voice. Now backs off one
  `f32` ulp, giving the hard clipper the bound was always meant to produce.

### Changed

- **The gain envelope now resolves through `getADSRValues` as well.** Its stages
  are decided together rather than filled in field-wise, so naming one changes
  what the others default to — matching upstream. Audible on any pattern that
  sets some but not all of attack/decay/sustain/release.
- **`cp` (clap) re-attacks.** Its second and third envelope stages sat at
  `exp(0) = 1` before their onsets rather than at 0, so all three fired at once
  and the result decayed smoothly — a noise blip, not a clap. They are now gated
  on, giving bursts at 0, 10ms and 20ms like the 808 circuit and like the
  recorded clap Strudel plays here. Peak level is held roughly constant (0.606 →
  0.546); average energy drops, the sound now being three bursts with gaps.
- `adsr`/`ad`/`ds`/`ar` are handled only in `rudel-core`, where they expand into
  plain `attack`/`decay`/`sustain`/`release` controls as they do upstream. The
  duplicate handling in the DSP layer was unreachable and had drifted (it forced
  `ad`'s sustain to 0 by hand); it is gone. No user-visible change.

### Added

- `cargo run -p rudel-dsp --example envelope_ab` renders the gain envelope
  before and after the change to WAV, for A/B listening.
- Audio parity oracles for the super-saw oscillator (`gen_supersaw_oracle.mjs`,
  including a moving pitch driven by `getPitchEnvelope` + `getVibratoOscillator`)
  and for the additive wavetable builder and pink/brown noise filters
  (`gen_oscillator_oracle.mjs`).
- The native app has its first tests that render a frame, via `egui_kittest` —
  transport buttons, eval/hush shortcuts, and the inline widget paint path.

### Internal

- CI runs `cargo nextest`, one process per test, so the engine's global
  registries cannot let a test pass on a neighbour's setup.
- `getParamADSR` and the Web Audio parameter mock are shared from
  `tools/oracle/lib.mjs` instead of living in one generator.
- Mutation survivors in the voices that were compared sample-for-sample:
  `next_supersaw` 0 of 71, `oscillator.rs` 2 of 226, `drum.rs` 2 of 161 — all
  four equivalent mutants (`<` against a time boundary that accumulated `f32`
  never lands on, and a peak-normalisation guard).
- The preprocessor tokenises once. Eight rewriters each carried their own copy
  of the same skip-strings-and-comments guard — the mutation run put survivors
  on the comment-detection line of every one — and now share
  `scanner::classify`/`scanner::chunks`. The scanner is byte-indexed throughout,
  dropping the `Vec<(usize, char)>` every pass used to build: ~20% faster over
  the doc-example corpus, with byte-identical output on all 509 examples. The
  preprocessor's mutation score went from 72.8% to 89.4% over 155 fewer mutants
  — code that no longer exists to get wrong — and `scanner.rs`, which absorbed
  every guard, reads 98.0% with its three survivors equivalent.

### Migrating

Patterns that name only *some* envelope stages will sound different, because the
unnamed ones now re-default the way Strudel's do. Spell out the stage you were
relying on to get the old shape back:

| pattern | was | now | to restore |
| --- | --- | --- | --- |
| `.attack(0.1)` | decay 0.05, sustain 0.6 | decay 0.001, sustain 1.0 | `.attack(0.1).decay(0.05).sustain(0.6)` |
| `.decay(0.15)` | sustain 0.6 (held) | sustain 0.001 (percussive) | `.decay(0.15).sustain(0.6)` |
| `.release(0.001)` | release 0.001 | release 0.01 (upstream's floor) | not restorable; the floor is upstream's |

Patterns naming all four stages are unaffected, as are `s("cp")` users who want
the old sound only in that they cannot have it — the previous envelope was a bug
against its own stated intent.
