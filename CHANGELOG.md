# Changelog

Notable changes to Rudel. Format follows [Keep a Changelog][kac]; versioning is
[semantic][semver], with the pre-1.0 convention that the minor number carries
breaking changes.

This file starts at 0.7.0. Earlier history is in the git log.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- **A right-click menu in the editor.** Every action it offers already had a
  keyboard shortcut and no other way in: evaluate, evaluate block, hush, panic,
  cut/copy/paste/select-all, toggle comment, indent and outdent. Each entry
  routes through the same code path as its shortcut rather than repeating the
  edit, and shows the shortcut beside it. Paste needs the clipboard *read* egui
  does not expose, so the platform clipboard is used directly; entries that
  cannot act — cut and copy with no selection, paste with an empty clipboard —
  are greyed rather than silently doing nothing.

### Fixed

- **Songs written in real Strudel evaluate.** Measured against
  [eefano/strudel-songs-collection](https://github.com/eefano/strudel-songs-collection),
  88 complete songs rather than doc snippets: **16 of them evaluated, now 86**.
  `crates/rudel-lang/examples/songs.rs` is the harness
  (`cargo run --release -p rudel-lang --example songs -- <dir> [cycles]`); a
  single file argument prints the whole Koto error with its line and column.

  Most of what it found was Rudel's, not JavaScript's:

  - `register(name, fn)` — the way a script defines its own pattern method, and
    the first line of about a quarter of the corpus — was not bound at all. It
    now inserts into the same method map the controls use, with the pattern
    last and the arguments patternified, as upstream registers them. A built-in
    is never replaced: scripts carry polyfills for names Rudel already has, and
    since the method map outlives an evaluation, one such script used to hand
    its polyfill to every later script in the session.
  - The `$:` label rewriter counted brackets a line at a time. A template
    literal spans lines, so scanning one of its lines alone read the closing
    backtick as an opening one and lost every bracket after it; the label then
    ran to the end of the file. It scans the whole source now.
  - A `.` continuation joined onto the line above stopped a *second* one from
    joining, which left it stranded on its own line — where Koto ends the
    argument list. This is the shape a tune writes whenever a chain gets long,
    and it holds whether the line above *ended* with the closing bracket or
    *began* with it.
  - `innerJoin` / `outerJoin` / `squeezeJoin` are reachable after all. A Koto
    callback returning a pattern already converts to a pattern-valued hap, so
    the joins need no Koto in the query path — only the binding was missing.
  - `let x = …` parsed as Koto's *typed* binding, so the value that followed
    was read as a type annotation. `let` and `var` are stripped like `const`.
  - A declaration is moved above the code that reads it. JavaScript resolves a
    name inside a function when the function runs, so a helper is often written
    above the data it uses; Koto captures at definition and reported the name as
    missing.
  - `setDefaultVoicings`, `withValue`, `filterValues`.

  And the JavaScript the preprocessor did not read: the conditional operator,
  `typeof`, brace-bodied arrow, `function` and `if`/`else` bodies, `&&`/`||`/`!`,
  block comments, object spread, numeric object keys, `.length`/`.value`/`.n`,
  comma-separated declarations, a name Koto reserves used as a variable, and the
  line breaks JavaScript allows anywhere and Koto does not — after `=`, `=>` or
  `:`, before a call's `(`, and inside a nested argument list.

  The two songs that still do not run need the Koto VM in the query path, which
  is the one architectural line Rudel does not cross (`docs/UNSUPPORTED.md`):
  one patches `Pattern.prototype` and builds `new Pattern(state => …)`, the
  other passes a pattern whose *values* are transform functions.

- Two allowlisted documentation examples (`seed`, `onTriggerTime`) run now —
  both were only ever blocked by syntax the above adds — and are no longer
  allowlisted.

## [0.10.1] — 2026-08-10

A release about a test that was asking the wrong question. Every example on
[the stepwise page](https://strudel.cc/learn/stepwise/) produced no events — all
25 of them — and the documented-example corpus passed them all, because it pins
that a snippet *evaluates and queries*, and these did. A step count is metadata:
the page itself points out that `expand(2)` and `expand(4)` sound identical on
their own, so getting one wrong is silent until a `stepcat` or a `pace` reads it
back and the pattern collapses.

The fix is small in both places it lands. What was missing was an oracle that
asks about the events rather than the reach, and there is now one.

### Fixed

- **The stepwise page works.** Every example on
  [strudel.cc/learn/stepwise](https://strudel.cc/learn/stepwise/) produced no
  events at all — `fastcat("bd hh hh", "bd hh hh cp hh").sound()`,
  `stepalt(["bd cp", "mt"], "bd").sound()`,
  `sound("tha dhi thom nam").bank("mridangam").expand("3 2 1 1 2 3").pace(8)`
  and the rest. Two causes:

  - `sound` had a hand-written binding that shadowed the control registry, so a
    bare `.sound()` set the control from a *missing* argument, i.e. from
    silence. `.s()` was fine, because it went through the registry. Deleting the
    shadow hands `sound` back to the one path.
  - The stepwise counts were not patternified: `expand("3 2 1 1 2 3")` read a
    whole pattern as `0`, and the `pace(8)` after it turned a zero step count
    into silence. Upstream registers these with `stepJoin` instead of the usual
    `innerJoin`, which is what makes a patterned count *stepcat* its variants
    rather than sample one per cycle — the page's own "will be treated in a
    stepwise fashion". `Pattern::step_join` now ports that, and `expand`,
    `extend`, `contract`, `shrink`, `grow`, `take`, `drop` and `replicate` route
    through it, as methods and as standalone pattern-last forms.

  This went unnoticed because the documented-example corpus pins *reach* — every
  snippet evaluates and queries — and a wrong step count is silent, not loud. A
  new oracle (`tools/oracle/gen_stepwise_oracle.mjs`,
  `crates/rudel-lang/tests/stepwise_parity.rs`) now pins the events themselves:
  45 cases, hap for hap against the real Strudel engine.
- **`FULL_STRUDEL.md` is UTF-8 again.** A 0.10.0 edit wrote one em dash as
  CP-1252.

## [0.10.0] — 2026-08-10

A release about a second way to write a pattern. Mondo Notation is Strudel's
Lisp-like alternative to the mini-notation-and-method-chains spelling, and it
turns out to cost almost nothing to support: it is a *source language* over the
pattern engine Rudel already has, so the work is a parser and a code generator,
not a second evaluator.

Building it was also the best bug-finder this repo has had in a while. Every
form in mondo lands on a Rudel function, so compiling upstream's own example
page walked straight into three things Rudel got wrong — a control that dropped
the value it was given, a `:`-list the method form would not split, and a
degenerate euclid that panicked. Each is fixed where all the callers route
through, and the euclid family gained the patterned counts it should have had
all along.

### Added

- **Mondo Notation.** [Strudel's Lisp-like pattern notation](https://strudel.cc/learn/mondo-notation/)
  now works in Rudel, two ways: `// mondo` on the first line reads the whole
  script as mondo — which is how the examples on that page are written — and
  `` mondo`s hh*8` `` (plus `mondolang` and `mondi`) embeds one pattern in a Koto
  script, the surface upstream's library exposes. Function calls, `#` chaining
  and `#`-lambdas, all four bracket kinds, the infix operators
  (`* / ! @ % ? & : ..`), `,`/`$` stacks, `|` choices, strings, comments and
  `def` are supported. A script that is mondo but has no marker gets an error
  saying so, rather than Koto's `unexpected token` at the first `$`.

  It is a compiler, not a second engine: `crates/rudel-lang/src/preprocess/mondo.rs`
  ports upstream's parser and emits Koto, which is why every control, transform
  and signal in the prelude is reachable from mondo with no dispatch table to
  keep in sync. Upstream's own parser and desugaring tests are ported alongside
  it, and each mondo spelling is checked to produce the same haps as the Koto
  one it compiles to. Two limits, both in `docs/UNSUPPORTED.md`: `:` and `..`
  take literal operands, and `def` binds values rather than functions.
- **`setSteps` on a pattern**, the stepwise metadata setter Strudel exposes and
  Rudel had only internally.
- **The euclid family takes patterned counts.** `s("bd").euclid("<3 5>", 8)` now
  alternates rhythm by cycle, as `euclid`, `euclidRot`, `euclidLegato`,
  `euclidLegatoRot` and their standalone forms all do upstream — previously only
  mini-notation's `bd(<3 5>,8)` could, and the binding silently produced nothing
  for a patterned count. The patternification moved out of `rudel-mini` into
  `rudel-core` so the operator and the bindings cannot drift apart, and literal
  counts still take the direct path.

### Fixed

- **A control set from a pattern that already carried one no longer drops it.**
  `"cp".delay(0.6)` is `{value: "cp", delay: 0.6}`, and upstream's `createParam`
  promotes that unnamed `value` into the control's own key on *every* path.
  Rudel applied the rule for single-key controls built by `control`, but not for
  `s`/`mode` (which read `:`-lists) or the multi-key spread controls, so
  `s(seq("bd", "cp".delay(0.6)))` emitted an inert `value` beside a `delay` that
  played nothing where Strudel emits `{s: "cp", delay: 0.6}`.
- **A bare control method spreads a `:`-list like its factory does.** `.s()` with
  no argument wrapped the pattern's values by name, so `"bd:3".s()` produced
  `{s: ["bd", 3]}` where `s("bd:3")` produced `{s: "bd", n: 3}`. Both now go
  through the control's own builder.
- **A degenerate euclid no longer takes the app down.** `euclid(0, 0)` produces
  an empty rhythm, which reached `slowcat` with nothing to concatenate and
  panicked on `pats[n % 0]`. An empty `slowcat`/`cat` is now silence, as an
  empty `stack` or `timecat` already was.

## [0.9.1] — 2026-08-09

A release about listening again. 0.9.0 got the example tunes sounding like
instruments; this is the five that still did not sound *right* next to Strudel,
and every one turned out to be a different mechanism rather than a matter of
taste. Three of them shared a single cause.

### Fixed

- **`speed` below zero plays the sample backwards instead of blowing up.** A
  negative speed made the read step negative, so the voice walked off the front
  of the buffer: the position saturated at frame 0 while the interpolation
  fraction kept growing, and linear interpolation between two fixed neighbours
  became linear *extrapolation* — an unbounded DC ramp, tens of times full scale
  by the end of the note. It was audible as popping and as everything else
  seeming to duck around it. superdough reverses the buffer and plays it at
  `Math.abs(speed)`, so `begin`/`end` and looping index the reversed copy; the
  voice now flips the frame lookup and leaves the position walking forwards,
  which gets the same result without copying the sample per note. This is what
  the "Delay", "Orbit" and "Amensister" tunes were reaching for with
  `sometimes(x => x.speed("-1"))` — the only three of the website tunes that ask
  for a negative speed.

- **A sample's default release is 10 ms, not 50 ms.** superdough's
  `getADSRValues` returns a `0.01` release when a hap sets no envelope controls.
  The five-times-longer default only showed when `clip` or `loop` cut a sample
  short of its own end, and there it smeared each note into the next — "Wavy
  kalimba" clips to as little as 0.1 s a note.

- **An MP3's encoder delay is trimmed, as the browser trims it.** Sample
  decoding went through `fundsp`'s `Wave::load_slice`, which hard-codes
  symphonia's gapless support off; the decode is now symphonia 0.6 directly,
  with it on (and `fundsp`, which nothing else used, is gone). A LAME-encoded
  MP3 carries ~1100 frames of encoder delay at the head that its own Xing/LAME
  header says to drop — `decodeAudioData` drops it, so upstream never hears it.
  Keeping it started every MP3 sample ~25 ms late, which is nothing on a drum
  hit at `speed(1)`. But the offset is in *source* frames, so it stretches with
  the playback rate: "Wavy kalimba" plays one MP3 as both a melody and a bass
  line three octaves below it, and the two layers came out 100 ms apart.

- **A voice only configures the orbit effects it actually sends to.** Every
  event applied its `delaytime`/`roomsize` to the orbit it landed on, including
  events sending nothing to either — and those carry the defaults, so they
  overwrote whatever the layer sharing that orbit had set. superdough reads
  those controls inside its `if (room > 0)` and `if (delay > 0 && …)` branches
  for exactly this reason. In "Amensister" a chord line on `delaytime(.125)`
  shares orbit 1 with a bass line of eight notes a cycle that sets no delay at
  all, and each of those notes yanked the delay's read head across the echo
  still ringing in the buffer — heard as the mix dipping.

- **Csound diagnostics are readable again.** Csound's message callback is a
  stream, not a line writer: a syntax error echoes the offending source *one
  character per call* to mark the fault position. Recording each call as its own
  line turned the whole diagnostic into a column of single letters, and the
  eight-line cap then truncated it after eight of them. Text is now buffered and
  cut on newlines, with the unterminated tail flushed when the messages are read.

## [0.9.0] — 2026-08-09

A release about sounding right. 0.8.0 got the example tunes to *evaluate*; this
one is what happened when they were listened to, and then compared against
Strudel's own hap snapshot event by event. Two things turned out to be wrong at
once: rudel started with no sample banks registered, so every tune written for
strudel.cc came out as beeps rather than instruments — and underneath that, a
dozen parity bugs in how controls, arithmetic and voicings resolve. **All 31
tunes on <https://strudel.cc/examples> now run**, up from 22, and 28 of them
reproduce Strudel's events exactly, up from 11.

Csound is supported as of this release — the one feature with a dependency
outside the workspace, and an optional one. **Several fixes change what existing
patterns sound like**; they are corrections toward Strudel, and are listed under
Migrating.

### Added

- **The default sample banks load at startup.** Rudel began with nothing
  registered, so a pattern naming `piano`, `ocarina_vib` or a drum machine fell
  through to a synth voice — which is why tunes written against strudel.cc came
  out as beeps rather than instruments. The seven maps the Strudel REPL
  prebakes (piano, VCSL, tidal-drum-machines, uzu-drumkit, uzu-wavetables,
  mridangam, and the Dirt-Samples subset) are now registered on launch.

  Only the *maps* are read: seven small JSON files, ~2.5s for the 857 sounds
  they describe. A sound's audio is downloaded the first time something plays
  it and cached on disk from then on, which is how the browser serves Strudel.
  Fetching every file up front — the obvious reading of "preload" — measures
  3.1 GB and about nine minutes, nearly all of it audio nobody asked to hear.
  The bank records a miss from the audio thread (which can neither block nor
  spawn) and the host turns it into a background job, the same path soundfonts
  already used. Failures are logged rather than raised: rudel has to start
  offline, and a bank nobody asked for should not open with a red error.

- **Csound works, against the Csound installed on the machine.** `loadCsound`,
  `loadOrc` and the `csound` / `csoundm` outputs all run; the last two tunes on
  strudel.cc/examples that did not, **CSound demo** and **Lounge sponge**, now
  do, so all 31 run.

  Upstream loads Csound's WebAssembly build in the browser. There is no
  pure-Rust Csound and that build is Emscripten output, which needs a browser
  or Node, so Rudel opens the *native* `libcsound` by name at run time — the
  first time a script asks for it, and never otherwise. Nothing links against
  it: a Rudel built or run without Csound behaves exactly as before, and a
  script that wants it and cannot find it gets an error naming what to install
  while the rest of the pattern keeps playing. Set `RUDEL_CSOUND_LIB` to point
  at a specific library. See [docs/UNSUPPORTED.md](docs/UNSUPPORTED.md#csound-strudelcsound--supported-with-csound-installed-separately).

  Csound renders inside the audio callback with host-implemented audio IO, so
  it is a signal in Rudel's mixer rather than a second output stream: one
  device, one clock, and note onsets sample-accurate against every other layer.
  Orchestra errors come back with Csound's own text — the line number and the
  offending source — because `csoundCompileOrc` returns `-1` for everything and
  a return code alone is not a diagnostic.

- **The tune corpus is now compared against Strudel's haps, not just run.**
  Upstream's `tunes.test.mjs` snapshots `queryCode(tune, testCycles[key])` for
  every website tune — each hap's spans and every control that reaches the synth
  — and commits the result. `gen_tunes_oracle.mjs` carries that into the corpus
  (parsing the committed snapshot rather than standing up a Strudel runtime, so
  the expectation is upstream's own), and `tunes.rs` compares hap for hap. 11 of
  the 27 website tunes that evaluated reproduced it exactly when the comparison
  was first turned on; 28 of 31 do now, and the three that do not are named with
  the difference in `tunes_parity_allowlist.json`, which fails in both
  directions like every other allowlist here.

  Two representation differences are normalised rather than reported, because
  they are spellings and not events: `note` names against MIDI numbers (Strudel
  converts at playback, Rudel when the control is set), and float formatting,
  compared to nine decimals.

### Fixed

- **`.add.out(x)` and the rest of the alignment getters are now accepted.**
  Strudel reaches an alignment through a second property access — `pat.add` is
  an object whose properties are the aligned variants — while Koto has no
  property getters, so Rudel binds the matrix flat as `add_out`, `set_squeeze`
  and so on. Every tune written the upstream way stopped at a parse error. The
  preprocessor now flattens `.op.align(` to the bound name, collapsing `.in` (the
  default alignment *is* the plain method) and normalising the `squeezeIn` /
  `squeezeout` spellings on the way. **Arpoon** runs as a result, and reproduces
  Strudel's haps exactly.

- **A tagged template is a call, not a pattern.** `loadCsound`` instr … endin ``
  is JavaScript's tagged-template call form, and the mini pass was wrapping the
  backtick body in `m(...)` — parsing an orchestra as mini-notation, and gluing
  its `m` onto the tag to make the undefined `loadCsoundm`. Tagged templates are
  now rewritten to an ordinary call and left out of mini-notation, which is the
  line upstream's `plugin-mini` draws too (`TemplateLiteral && parent !==
  TaggedTemplateExpression`). Untagged multi-line templates are still patterns.

- **A method chain can continue after a multi-line call.** Koto will not carry a
  chain onto a new line after a call whose arguments spanned lines, however far
  the `.` is indented — so `n("0 7".off(…)\n.slow(2))\n.clip(.25)` was a syntax
  error no amount of indentation fixed. It does accept the chain written on the
  line that closes the call, so that is where the continuation now goes.

- **`setVoicingRange` is accepted instead of aborting the script.** It narrows a
  voicing dictionary's register, which upstream only reaches the deprecated
  `.voicings(dict)` path — `.voicing()` aligns by `mode`/`anchor` and never reads
  `range` — and which Rudel does not model on either path, so it is a no-op.
  **Dinofunk** runs as a result, and matches Strudel's own haps with the call
  ignored.

  With Csound above, **all 31** tunes on strudel.cc/examples now run. 28 match
  Strudel's own haps exactly; the three that do not are listed with the
  difference in `tunes_parity_allowlist.json`, and none of the three differs in
  a way that is audible.

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
- **A control could not carry a whole event.** `withVal`'s fall-through is
  `return { [name]: xs }` — a map with no unnamed `value` is nested *under* the
  control's key, which is how `.anchor(melody)` stores `{anchor: {note: …}}` for
  `renderVoicing` to read back as `anchor?.note || anchor`. Rudel merged such a
  map into the hap instead, overwriting the very controls the voicing was about
  to read, so `.anchor(pat).mode('duck')` fell back to the dictionary's own
  anchor: the comping layer voiced an octave low and kept the tone it was
  supposed to duck out of the melody's way.
- **`degradeBy` did not patternify its amount.** Upstream registers it with
  patternify, so a signal or mini pattern is sampled per cycle; Rudel took a
  single `f64` and collapsed the argument to one arbitrary value, keeping events
  upstream drops. Tunes reach for `degradeBy(sine.range(0,.5).slow(32))` to make
  the density breathe. `undegradeBy` had the same shape and the same fix, via a
  new `patternify_f64` — the existing `Frac` variant would round a probability
  onto a bounded rational, which is right for a time and not for this.

### Internal

- `wrap_control_dyn` is now `control_dyn`: the distinction only existed because
  the bag rule was applied on one path.
- Tune parity went from 11 exact to **28 of 31** on the back of the above. None
  of the three left is a bug to fix, and none differs in a way a listener could
  hear:
  - **Jux und tollerei** — Strudel's `every`/`firstOf`, and so `palindrome()`
    (which is `every(2, rev)`), returns nothing for negative cycles, while
    `when`, `rev` and `fast` behave normally there. The tune's `off` copy
    therefore pulls nothing in from cycle -1 upstream; Rudel carries the
    previous cycle's note across, which is the correct reading.
  - **CSound demo** — `.csound(name)` is an `onTrigger` upstream, which hangs
    off the hap's *context*, so the instrument never appears in the snapshot;
    Rudel has no onTrigger and carries it as a control. Spans and notes match
    one for one.
  - **Lounge sponge** — the same, plus `n` after `.scale(...)`: Strudel leaves
    the note name it resolved to (`n:E5`), Rudel the MIDI number (`n:76`). Both
    are 659.26 Hz, and the test only normalises names for the `note` key.
- A guard that every registered control is read by something that makes sound
  (`crates/rudel-audio/tests/control_coverage.rs`). `clip` and `velocity` both
  got past every other check the same way — registered, riding along on the hap
  so the tune oracle was satisfied, then dropped silently at the boundary
  between the control map and the voice. It scans the crates that turn a control
  into output for each canonical key, and inventories the ~120 with no reader,
  grouped with the reason. It says a control is *wired up*, not that it is
  correct; that is what the DSP goldens and the tune oracle are for.

### Migrating

Every entry below is a correction toward Strudel, so the "to restore" column is
mostly empty on purpose — the old behaviour was the bug. The changes worth
knowing about before you re-evaluate an existing set:

| pattern | was | now | to restore |
| --- | --- | --- | --- |
| `s("piano")`, `s("ocarina_vib")`, any bank name | a synth beep, the name being unregistered | the sample, from the banks loaded at launch | name a synth: `s("triangle")` |
| `"c2".add(12)` | 12 — the note name coerced to zero | 48, `parseNumeral` resolving the name first | write the number: `"36".add(12)` |
| `cat('C3 dorian', 'Bb2 major')` | four values, the single-quoted strings parsed as mini | two plain strings | use double quotes for mini-notation |
| `.velocity(0.5)` | ignored | multiplies `gain`, before any voice | drop the call |
| `.clip(0.5).piano()` | kept your `clip` | `piano()` sets `clip` to 1 | `.piano().clip(0.5)` |
| `v("8:.125")` | the whole list under `vib` | `vib` 8, `vibmod` 0.125 | — |
| `rootNotes(4)` | a MIDI number | a note name (`"C4"`) | — |
| `.superimpose(f, g)` | only `f` applied | both | — |
| `.voicing()` | bare numbers | a `note` control | — |

A fractional scale degree is now ceiled rather than rounded, matching
`scaleStep`. That only shows up where a degree is computed rather than written —
`n("0").add(n(rand.range(0,12))).scale(…)` lands on a fraction every time — and
moves roughly half of those up a degree.

Sample playback follows `sampler.mjs`'s rule: a sample plays its whole slice
unless `clip`, `loop` or `release` asks for it to be cut to the hap. A sustained
instrument left uncut rings past its note, so if a layer of yours now overlaps
into noise, `.clip(1)` is the control that was always meant to bound it.

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
