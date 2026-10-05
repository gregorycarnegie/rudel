# Changelog

Notable changes to Rudel. Format follows [Keep a Changelog][kac]; versioning is
[semantic][semver], with the pre-1.0 convention that the minor number carries
breaking changes.

This file starts at 0.7.0. Earlier history is in the git log.

[flacenc]: https://github.com/yotarok/flacenc-rs
[`opus-rs`]: https://github.com/restsend/opus-rs
[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- **Drum machines answer to their model names**, as on strudel.cc:
  `bank("TR909")`, `bank("RZ1")`, `bank("LM1")` and the rest of the 66
  aliases Strudel registers at startup. `aliasBank` takes upstream's other
  forms too: a map (`aliasBank({RolandTR808: "808"})`), a list of aliases,
  and the URL of a JSON map.
- Pianoroll's deprecated `timeframe` option (draws from 0 to its value).
- **Strudel's editor fonts.** The font picker and the `fontFamily` pattern
  setting offer the website's whole list (PressStart, mode7, teletext,
  Monocraft, FiraCode, JetBrains, …). Each is downloaded from strudel.cc the
  first time it is chosen and cached; the editor stays in monospace until it
  arrives.

### Changed

- **The script prelude and the draw canvas are native.** What was left in
  JavaScript (`bind` and kin, `createParam`, `ref`, `onTrigger`, the pattern
  settings, the hydra glue and the whole of `.draw`/`.onPaint`/`animate`/
  `document`) is Rust now, with no bundled JavaScript left. Scripts see the
  same behaviour: every hap of the 90-song collection is unchanged. Building a
  script engine takes about half as long (3–4 ms, was 7–8).

### Fixed

- The REPL's wavetable bank plays as wavetables: `s("wt_digital")` and the
  other `wt_` sounds Strudel preloads reach the wavetable oscillator, as any
  `wt_` name in a `samples(...)` map does upstream. Before they played as
  one-shot samples.
- Two loads of the same file at once download it once, and a half-written
  cache file can no longer be read as a whole one.
- Chained `onTrigger` hooks all fire, as upstream's compose: before only the
  last ran, so `.fontFamily("mode7").fontSize(28)` set the size and dropped the
  font.

## [0.23.0-beta.1] — 2026-10-04

A beta: the changes below have not been through a mutation-testing run yet,
and part of the script prelude is still JavaScript.

### Added

- **Vim, Emacs, VS Code and Helix keymaps,** as Strudel's `keybindings`
  setting has: pick one under `keys` in the editor settings. Vim and Helix
  show their mode at the editor's corner, and `:w` evaluates and `:q` hushes
  as in Strudel. Each is the everyday core of its editor; the app README lists
  what is in.

- **Themes are TOML files, and you can add your own** without rebuilding:
  put a `.toml` in rudel's themes folder (the `folder` button beside the theme
  picker opens it) and it is read at launch. Strudel's 39 are built in as the
  same files (`crates/rudel-app/themes/`). See the app README for the format.

- **Editor settings are kept between sessions:** theme, font, size and the
  editor switches come back on the next launch, where they used to reset.

- **`.midi(port, options)` picks the device and takes Strudel's options:**
  `isController`, `noteOffsetMs`, `latencyMs`, and default `midichannel`,
  `velocity`, `gain` and `midimap`. The port (a name part or an index) and a
  hap's `midiport` route to that device, opened on first use; before, the name
  was ignored and everything went to the port picked in the app.

- **Evaluating one block keeps the others playing,** as Strudel's block-based
  evaluation does: labelled patterns from the last full evaluation and the
  blocks since stay in the mix, a block replaces only the labels it defines (or
  drops the ones it no longer has), and top-level `const`/`let`/`function`
  declarations carry over to later blocks. `clearScope()` forgets them, and `$:`
  in a block is refused with upstream's message.

- **All 39 of Strudel's editor themes,** generated from its CodeMirror theme
  files (`tools/generate_themes.mjs`), where rudel had three.
- **`theme`, `fontFamily` and `fontSize` work as pattern settings,** as on the
  Strudel website: `s("bd").theme("<githubDark nord>")` switches the editor's
  theme as the events play. `fontFamily` maps only the generic families.
- **`onTrigger(fn)` calls `fn(hap, time)` as each event plays.** Upstream it
  also silences the event's own sound by default; here the sound plays.
- **`ref(() => value)`, `document` events and standalone `scrub`.** Tunes that
  steer themselves with `document.onmousemove` or `onkeydown` and read the
  result through `ref` now run, and so does hydra's `() => mouse.x`.
- **Strudel's draw canvas works: `.draw(fn)`, `.onPaint`, `getDrawContext()`,
  `requestAnimationFrame` and `animate()`.** Scripts draw with a
  `CanvasRenderingContext2D` stand-in (paths, arcs, curves, transforms, text)
  on a canvas behind the code that keeps its pixels until cleared, as a
  browser's does, so trails and `animate`'s smear work.
  `window.innerWidth`/`innerHeight` report its size. These used to evaluate
  and draw nothing.
- **Hydra draws behind the code, as in Strudel.** `await initHydra()` puts
  hydra's functions in global scope and `osc(10).out()` / `render(o1)` draw
  the scene behind the editor; "Pop out hydra" in the right-click menu shows
  it in its own window, fullscreen on a double-click. hydra's array methods
  (`[1, 2].fast(2)`), `s0`-`s3`, `a` and `width`/`height` exist, so 331 of the
  390 shared strudel.cc patterns that failed on hydra now evaluate (7,422 of
  8,004 overall).
- **`initHydra({feedStrudel: 1})` feeds the visuals into hydra.** A
  `.scope()`, `.pianoroll()` or `.spiral()` (the public spellings, which
  upstream draws on its full-screen canvas) becomes hydra's `s0` instead of an
  inline widget, so `src(s0).kaleid(4)` folds the scope into a kaleidoscope.
  84 of the shared strudel.cc patterns use it.
- **`s0.initCam()` streams a webcam into hydra,** as upstream: `initCam(1)`
  picks the second camera.
- **`s0.initVideo(url)` plays a video into hydra,** looping and muted as
  upstream, from http(s) or this computer. With FFmpeg installed, any format
  it reads (WebM, MOV, GIF, …); without it, MP4 with H.264, which is most
  shared patterns' videos.
- **`s0.initScreen()` captures the primary monitor into hydra,** about 15
  times a second. Not on Linux.
- **`s0.initImage(url)` loads a picture into a hydra source,** PNG or JPEG,
  over http(s) (cached on disk like samples) or from this computer (a path,
  `~/…`, `file:///…`, or relative to the open file's folder), so `src(s0)`
  shows it.
- **Hydra arguments change every frame, as upstream:** `H(pattern)` follows
  the pattern (`shape(H("3 4 5"))` changes shape with the notes), arrays step
  through their values with hydra's `.fast`/`.smooth`/`.ease`/`.offset`, and
  a function argument (`({time}) => Math.sin(time)`) is called every frame.
  In an inline `_hydra` widget too.
- **Hydra reacts to the music.** With `initHydra({detectAudio: true})`,
  `a.fft` follows what rudel is playing (upstream it listens to the
  microphone), through hydra's own bins, smoothing and cutoff, so
  `shape(4, () => a.fft[0])` pulses with the kick.

- **Double-click an inline visual to pop it out into its own window.**
  Double-click that window for borderless fullscreen on whichever screen it
  is on (drag it to a second monitor first, as for a presentation); Esc
  leaves fullscreen, and closing the window puts the visual back inline. The
  spiral scales up with the window; the others fill it.

- **`window` is the global object,** as in the browser REPL, so tunes that
  share values through it (`window.spag = …`) evaluate. 232 of the 8,004
  patterns shared on strudel.cc failed on this alone. `requestAnimationFrame`
  exists but never calls back, since there is nothing to paint.
- **`createParam` and `createParams`** make new controls at runtime, as in
  Strudel: `const {movement} = createParams('movement')`.
- **The `animate` shape params `x`, `y`, `w`, `h`, `angle`, `r` and `fill` are
  controls.** They were missing, so any pattern naming them failed to evaluate.

### Fixed

- **Pianoroll labels match Strudel's:** a sound with a number reads `bd:3`,
  labels are sized by the note, drawn black on filled notes and in the note's
  colour on empty ones, are no longer hidden on short notes, and `fontFamily`
  is read.

- **A re-evaluation no longer redraws what already played:** pianoroll,
  punchcard, wordfall and spiral keep the notes that had started as they were
  drawn and show the new pattern only from the playhead on, as Strudel's
  drawer does; the old notes scroll away as usual.
- **`_pitchwheel` matches Strudel's modes:** `mode: 'polygon'` outlines in the
  last note's colour and alpha rather than the theme's, centre lines are
  `hapRadius` wide, and only the default `flake` mode draws them (`dots` drew
  them too).

- **MIDI CC and program change are sent without a note,** so
  `ccn(74).ccv(sine.segment(16)).midi()` reaches the device. They used to need
  a note on the same event.
- **MIDI velocity is `velocity` × `gain`,** as in Strudel, so `.gain(0.5)`
  sends 0.45, not 0.5. Note-offs come 10 ms before the event's end, Strudel's
  `noteOffsetMs`, where rudel left 1 ms.

- **`echo` and `stut` take patterns for every argument,** as upstream's
  `register` makes them. A count given as a pattern (`echo(reify(3), …)`)
  made no echoes at all, and a patterned time stacked every copy at once.
- **Downloads say they come from rudel.** Samples, pictures and videos were
  fetched with the HTTP library's generic user agent, which some hosts
  (Wikimedia) refuse.
- **Pasting over a buffer with an inline slider no longer turns it into the
  slider's number.** A select-all paste stretched the slider's range over the
  whole new text, the slider stayed live on its remembered value, and the
  first nudge egui gave it wrote that number over everything.

### Changed

- **`.pianoroll()`, `.punchcard()`, `.wordfall()`, `.spiral()`,
  `.pitchwheel()`, `.scope()` and `.spectrum()` draw full-size behind the
  code,** as Strudel's do on its draw canvas; the `_`-prefixed spellings
  (`._pianoroll()`) stay inline. They used to be inline too.

- **A label used twice plays its last pattern,** as upstream's
  `pPatterns[id] = this` does, where both used to stack.
- **`d1`-`d9` and `p1`-`p9` are properties, as in Strudel:** write
  `note("c").d1`, not `note("c").d1()`. `q1`-`q9` are `silence`. The call form
  now fails with "not a callable function".

- **The draw canvas is about 50 times faster.** A tune drawing 200 arcs a
  frame through `.draw` took 137 ms a frame, so animations crawled; it now
  takes 2.5 ms. Paths, transforms and what is drawn are kept natively rather
  than built point by point in script, and a script reads queried haps about
  15% faster. The numbered slots, the alignment getters (`pat.add.out`) and the
  `_name` twins are native too; they behave as before.

## [0.22.2] — 2026-10-03

### Fixed

- **Line numbers stay level with their lines.** The gutter summed the font's
  row height per line, but the editor snaps each row to a whole pixel, so the
  numbers slid further off the text with every line — over a pixel by line 40,
  and more with inline widgets. They are now placed from the editor's own laid
  out rows, which also numbers a soft-wrapped line once instead of drifting.

## [0.22.1] — 2026-10-03

### Fixed

- **`withValue` (and `fmap`) map values as the pattern plays,** as upstream
  does. They used to map the first 16 cycles at evaluation and repeat them, so
  any pattern longer than that replayed its opening from cycle 16 on. One song
  in the eefano collection now gives Strudel's exact 1,001 events over 128
  cycles rather than 855, and evaluates in 5.5 ms rather than 27. A callback
  that throws leaves the value as it was and logs `withValue: <message>` to
  the console; it used to fail the evaluation.
- **`filter`, `filterValues`, `filterWhen`, `arpWith`, `plyWith`/`plyForEach`,
  `into`/`chunkInto`, and `log`/`logValues` with a callback work past cycle
  16,** and so do a patterned leading argument (`chunk("<2 4>", f)`,
  `inside`, `within`) and a patterned argument to a `register`ed function.
  Each worked out its result over the first 16 cycles at evaluation. Past
  that, the filters and `log` repeated that window, and the rest went silent
  on any value or chord first heard later; `chunkInto` stopped transforming.
  They now decide as the pattern plays, and an error in a callback goes to
  the console.
- **A note can no longer start up to a buffer late.** An onset a hair past a
  frame boundary, as ordinary arithmetic on beat times leaves it, was rounded
  onto that frame when the buffer was split but tested as not yet due when
  notes were started. So it was skipped at the split and started at the next
  buffer instead, up to about 6 ms late with 256-frame buffers. Both checks
  now use the same rounding.
- **A malformed SoundFont can no longer crash the app.** Nested `LIST` chunks
  were read recursively, so a few megabytes of them overflowed the stack.
- **A deeply nested `bytebeat` expression is silence instead of a crash.**
  Brackets, ternaries, calls and prefix operators nested more than 64 deep, or
  more than 1,024 binary operators, used to overflow the audio thread's
  stack. Real bytebeats are far smaller.
- **A very deep pattern no longer crashes the app.** A script loop such as
  `for (…) p = p.fast(1)` builds a pattern one combinator deeper per pass,
  and playing or replacing one past about 5,000 levels overflowed the stack
  of whichever thread touched it. Querying and freeing a pattern now move to
  a fresh stack segment when the current one runs low, so depth is limited
  by memory instead.
- **Stopping playback resets `timeline` offsets,** as upstream's scheduler
  does, so a cued timeline realigns to wherever playback next starts. Rudel
  kept the old offset.

### Testing

- CI runs the test suite on Windows as well as Linux, and installs Csound on
  Linux with `RUDEL_CSOUND_REQUIRED=1`, so Csound failing to load is a test
  failure there rather than a skip.
- `cargo mutants` runs tests under nextest, as CI does, from
  `.cargo/mutants.toml`.

## [0.22.0] — 2026-10-02

### Changed

- **Rust 1.99 or newer is required** (was 1.96). It is the stable release CI
  builds with, and the workspace builds and passes its tests on it.
- **Paste in the editor's right-click menu is always enabled.** It now asks
  eframe for the clipboard, which pastes like Ctrl+V, so the menu can no
  longer check beforehand whether there is anything to paste. Pasting nothing
  does nothing.

- **Scripts run as JavaScript, on [Boa], instead of being translated into
  Koto.** A script is Strudel's own language now, read by a JavaScript engine,
  so block-bodied functions, `let`/`const`, loops, spread, ternaries, `typeof`,
  `JSON`, `Array`/`Object`/`String` methods and the rest mean exactly what they
  mean in Strudel, with nothing in between to get them wrong. The preprocessor
  keeps only the passes that are Strudel rather than JavaScript — mini-notation
  literals, widgets and sliders, `name:` labels, Mondo, the kabelsalat scope,
  alignment getters and `await` — and shrank from about 8,300 lines to about
  3,700; the 600-line shim of JavaScript builtins is gone. Every song in the
  eefano collection and every documented example that evaluated before still
  does, with identical haps; three allowlisted doc examples (`K#1`,
  `arpWith#0`, `register#0`) now evaluate too.
- **`Pattern.prototype.name = function …` replaces a built-in method, as it
  does upstream.** It used to be refused for any name Rudel already had, which
  quietly kept a song's own `warp` from running (swimandsleep plays its
  tablature as written now). `register(name, fn)` still never replaces a
  built-in, and a registration now lasts for the evaluation that made it.
- **The script engine lives on one thread of its own.** Evaluations queue for
  it, and the few script functions that run after evaluation — `new
  Pattern(state => …)` queries, `apply(pick({…: x => …}))`, `onTriggerTime`
  hooks — are handed to it from whichever thread asks. The engine for the next
  evaluation is built while the current one plays, so a short pattern still
  evaluates in well under a millisecond.
- **Querying a pattern is 2–2.7× faster.** Over a third of query time went on
  software 128-bit division inside the rational time type; its arithmetic now
  takes a 64-bit path whenever the numbers fit, with identical results. Haps
  are also no longer deep-copied on every source-location and context layer.

### Added

- **A pattern of functions is a callback.** `sometimesBy(.2, choose(x => …,
  x => …))` applies whichever function the cycle picked, as upstream's
  `register` samples a patterned argument; a function reaching a pattern
  argument becomes a pattern of that function, as `reify` makes it.
- `register([names…], fn)`, which registers each name and returns them as an
  object; `signal(t => …)`; `id`; and `pat.queryArc(begin, end)` /
  `pat.firstCycle()` for scripts that read a pattern's haps.
- `bind`, `innerBind`, `outerBind`, `squeezeBind`, `stepBind` and `polyBind`
  (with `stepJoin`/`polyJoin`); `withHaps`/`withHap`; `shrinklist` /
  `s_taperlist`; `keepif` in every alignment (`keepif.out` is `struct`);
  `voicingRegistry`, for extending a built-in dictionary with `addVoicings`;
  `tokenizeNote`; and `seqPLoop`, which arranges sections by start and stop
  cycle so they may overlap.
- **The unpatterned `_name` twin of every method** (`_scramble(4)`,
  `_fast(2)`), which upstream's `register` makes for each one.

### Fixed

- **The GPU spiral no longer crashes the app on a GPU without storage buffers
  in fragment shaders** (OpenGL-only and some software adapters). It reads its
  bands from one, which wgpu refuses there, and the refusal is a panic. Those
  devices now get the CPU spiral.
- **Copy and Cut in the editor's right-click menu work.** They were always
  disabled: egui puts the cursor under the pointer on any button's press, so
  the right-click that opened the menu had already dropped the selection. The
  selection now survives a right-click.
- **`.modulate("lfo", {...})` modulates,** and **`Kabel.bytebeat("t*2")`
  keeps its source.** Both read their string argument only when it arrived
  bare, but a script's string literal reaches them as a pattern.
- **`pure(fraction)` of an engine Fraction is that number,** not `null`.
- **`$:` alone on its line labels the statement on the next one,** and an
  indented `$:` is a label rather than a continuation of the line above. A
  label's expression now ends where JavaScript's statement does — at a line
  break with no operator on either side of it — so an indented statement
  after a label is no longer swallowed into it.
- **A tagged template may have a space before it** (``note `c e g` ``), which
  was read as mini-notation and failed to parse.
- **A script the engine panics on is an error, not a crash.** boa's `sort`
  panics on a comparator that is not a total order, which the
  `sort(() => Math.random() - 0.5)` shuffle idiom is.
- `pat.filter(x)` with something other than a predicate plays the pattern
  unfiltered, as upstream does, rather than failing.
- **`pat.add.out` and the other alignments are real getters,** so
  `.chunk(4, room(1).keep.out)` passes a function bound to its pattern. They
  used to be a text rewrite that only recognised the called form.
- **Labels after a `;` on the same line** (`…;$: n("0")`, the minified
  one-liner) are labels, not part of the statement before.
- **A script that ends on a statement plays silence,** as upstream's REPL
  does, rather than failing with "did not return a pattern". Any other
  non-pattern value is still an error.
- **A one-argument method called with none plays silence** (`.rarely()`,
  `.jux()`), as upstream's `register` makes it, rather than failing.
- **`reify(x)` is always a pattern.** Given a mini-notation string it kept the
  text as well, so `pure(reify("a b"))` held the string `"a b"` rather than
  the pattern. `pure("text")` still holds the text.
- **Three ways to crash the app from the editor, found by property tests:**
  Mondo's `..` or `&` with an operand missing panicked, and brackets nested a
  few hundred deep — in Mondo or in the script — overflowed the stack, which
  aborts the process rather than failing the evaluation. All three are now a
  `SyntaxError`. Scripts may nest 128 brackets deep, Mondo 64.

Measured against real Strudel over the 8,004 patterns shared on strudel.cc,
patterns that play in Strudel but fail in Rudel went from 83 to 15, every one
of which reaches for the browser's `window`/`document`. (The Strudel side was
corrected along the way — see Internal — so 5,198 patterns now count as
working in Strudel rather than 4,975; the 83 was against the old count.)

### Removed

- **Koto syntax.** `|x| x.fast(2)` lambdas, Koto's call-without-parentheses
  and the other Koto spellings no longer evaluate; write the JavaScript
  (`x => x.fast(2)`), which always worked.
- **`silence()` as a call.** `silence` (and `nothing`) is a pattern, as
  upstream; calling it is an error.
- **Arithmetic operators on patterns.** `pure(3) + 4` was pattern addition
  under Koto. JavaScript has no operator overloading, so it is what it is in
  Strudel: string concatenation, and `"<1 2>" / 48` is `NaN`. Use
  `.add(4)`/`.div(48)`.
- `Number('wat')` is `NaN` rather than `0`, and whole numbers reach a pattern
  as integers (`2.0` is `2`), since JavaScript has one number type.

### Internal

- **Mutation testing reaches the device code.** The audio engine runs on a fake
  output that renders when a test pulls frames. The shader, hydra and spiral
  widgets render on the GPU in tests, at 2 pixels a point. File dialogs can be
  answered by a test. The Windows GS Wavetable synth serves as a real MIDI
  port. cargo-mutants now catches 90.9% of mutants, and nearly every survivor
  left in the app, audio and MIDI crates is equivalent.
  `scripts/verify-missed.ps1` re-tests only the mutants a full run missed.
- The `arboard` dependency is gone; the menu's Paste uses
  `ViewportCommand::RequestPaste`.
- Dependencies build at `opt-level = 2` in the dev profile, so debug builds of
  the app run audio and the GPU widgets at a usable speed.
- Property tests for the script path: the preprocessor, Mondo and the source
  scanner never panic and return ranges that slice, `evaluate` never fails by
  panicking, mini-notation offsets point at their literal, and a literal
  reaches a control unchanged. `PROPTEST_CASES=3000` soaks them.
- nextest kills a test after two minutes (`.config/nextest.toml`), so a
  script callback that never returns fails its test instead of hanging CI.
- Tests that write files use `tempfile` directories rather than a hand-made
  one under the system temp directory that a failed test left behind.
- **The differential harness (`tools/oracle/strudel_diff.test.mjs`) reads
  Strudel correctly.** Its stub `document` had no `dispatchEvent`, so anything
  Strudel logged threw, and about 230 patterns that work in Strudel were
  recorded as failing. An error Strudel catches while querying, and only logs,
  now counts as a failure rather than as an empty pattern; and the logger's
  one-second de-duplication no longer lets one pattern's error hide the same
  error in a near-copy run after it, which made a verdict depend on run order.
- `sweep --errors` shows the line an engine error points at, as the engine
  ran it, with a caret under the column.

[Boa]: https://boajs.dev

## [0.21.0] — 2026-09-23

### Changed

- **Dependency bumps.** `wide` 1.7.0 → 1.7.1, `indexmap` 2.14.1 → 2.14.2,
  `eframe` and `egui_kittest` 0.36.1 → 0.36.2, `ureq` 3.4.0 → 3.4.2,
  `opus-rs` 0.1.32 → 0.1.34, and `pest`/`pest_derive` 2.9.0 → 2.9.2, all
  semver-compatible and needing no source changes. The workspace tests and
  clippy are clean on them.

## [0.20.2] — 2026-09-01

### Changed

- **Dependency bumps.** `wide` 1.6.1 → 1.7.0 and `indexmap` 2.14.0 → 2.14.1,
  both semver-compatible and needing no source changes. The workspace tests and
  clippy are clean on them.

## [0.20.1] — 2026-08-24

### Changed

- **The kabelsalat back end, the host tables and the synth voice are tested.**
  A mutation run over the whole workspace read 84.2%, down five points, and the
  drop was all new code that had landed under-tested: `dsp/kabelsalat.rs` alone
  survived 748 mutations — 43% of the workspace's total — against 7 tests for
  1581 lines. Its ugens, its instruction set and its wire format are pinned
  now, along with the gain curve in core's host tables, the synth voice's
  wavetable and noise-blend paths, the FM matrix's cross-modulation direction,
  and several of the preprocessor's walks. No behaviour changes: 838 surviving
  mutations across those files became 96, and what is left is mostly
  comparisons whose two branches agree.

## [0.20.0] — 2026-08-23

### Fixed

- **An Opus take shorter than one 20 ms frame no longer panics the writer
  thread.** The final packet's granule position was computed by adding a whole
  frame and subtracting it back off, which goes below zero when no whole frame
  was ever encoded — a debug build panicked, a release build wrote a nonsense
  position. The real length is stamped on the packet directly now. A take with
  no audio at all writes its headers and closes cleanly too.

- **A WAV take stops growing before its RIFF size wraps.** The data chunk was
  capped at the 32-bit ceiling but the RIFF size, which is 36 bytes larger,
  was not: at about six hours of 48 kHz stereo the header arithmetic overflowed
  instead. The cap is the RIFF size now, so a take that long ends with a file
  that still describes itself correctly.

- **A tempo change no longer sends note-offs for notes that never sounded.**
  0.19.0 kept every queued note-off across a `cps` change to stop notes
  hanging; the lookahead has usually queued the *next* note's on and off too,
  and re-querying means that note never plays — so its off would arrive later
  and cut short whatever the new schedule had put at that pitch. Only offs
  whose note actually started are kept.

### Changed

- **Opus recording is pure Rust, and no longer offers 24 kHz.** `opus`/
  `audiopus_sys` are replaced by [`opus-rs`], a port of libopus 1.6. With FLAC
  already moved, that takes the last CMake user out of the tree: `cargo tree -i
  cmake` returns nothing, the release workflow's policy override is gone, and
  building rudel needs no CMake at all.

  The output was checked against libopus itself before the binding was dropped:
  a tone recorded through `opus-rs` decodes in libopus at the amplitude it went
  in with, and with nothing else in it, at 8, 12, 16 and 48 kHz — but **not at
  24 kHz**, where this encoder writes noise. So 24 kHz is refused now, like
  44.1 kHz already was; no sound card runs at it, and a take that cannot be
  played is worse than a take that was never started. A recorded tone is
  decoded and measured in the test suite now, which is what caught it.

- **FLAC recording is pure Rust.** `flac-bound`/`libflac-sys` are replaced by
  [`flacenc`], which removes libFLAC's vendored C build from the tree — one of
  the two crates that made `cmake` a build requirement (`libopus` is the other,
  so it is still needed). The crate's one-call API encodes a whole stream in
  memory; rudel drives its frame-level API instead, writing each frame through
  to the file and patching `STREAMINFO` at the end, so a take stays bounded by
  the file system as before. Being lossless, the swap is checked exactly: a
  take through FLAC decodes to the same samples as the same take through WAV.
  It encodes at about 190x realtime, between Vorbis and Opus.

- **The recording dialog lists the formats separately.** It offered one
  combined `Audio (*.wav;*.flac;*.mp3;*.ogg;*.opus)` filter, so the only way to
  pick a format was to type its extension. Each format now has its own entry
  under that one, and choosing it in the file-type list is what sets the
  extension — which is still what the encoder follows.

- **A recording that dropped blocks says so.** The status line after a take
  names how many the audio thread had to throw away because the encoder could
  not keep up, rather than reporting a gap-free-looking success.

## [0.19.0] — 2026-08-23

### Added

- **Unsaved edits are not thrown away without asking.** Opening another file or
  closing the window while the buffer differs from what is on disk asks first,
  naming the file. The comparison is against the file's own contents, so a
  buffer restored from the autosave on the next launch is still "unsaved" if it
  was never written — the warning survives a restart, which is exactly when the
  edits are easiest to lose. The window title carries the same state: a
  trailing `*` next to the file name while the buffer differs from it.

### Fixed

- **A tempo change no longer leaves MIDI notes stuck on.** Changing `cps`
  re-anchors the clock and re-queries the window, which dropped everything
  still queued at the old rate — including the note-offs of notes already
  sounding. Nothing re-queried those, because the notes they end were scheduled
  before the change, so the note played until something else happened to
  retrigger it. Queued note-offs now survive the change at their original time,
  which is the duration the note was actually started with.

- **A take that is running when the app closes is still finalised.** Dropping
  the recorder now stops it and waits for the writer thread, so the container
  gets its lengths and trailing pages written. Without that, closing mid-take
  left a file that would not decode.

- **A stalled disk drops blocks instead of memory.** The audio thread handed
  blocks to the writer over an unbounded channel, which grows without limit if
  the encoder cannot keep up. The queue now holds a few seconds and the audio
  thread drops what will not fit rather than blocking or queuing forever;
  dropped blocks are counted, so a glitched take is knowable rather than
  silent.

- **An inline `slider(...)` is drawn inside its call.** `epaint` applies a
  section's letter spacing *before* each of its glyphs, so hanging the reserved
  gap on the `(` opened it in front of the bracket and the control drew over
  the word `slider`. The gap belongs to the value literal's first character.

- **Sounding keys on the claviature are visible in both themes.** They were
  filled with the theme foreground, which is near-white in a dark theme:
  invisible on a white key, and indistinguishable from an unpressed white key
  on a black one. A pressed key with no `color` control now gets its own
  accent; `color` still overrides it.

- **The release builds run at all.** The hosted Windows and macOS images now
  ship CMake 4, which removed support for `cmake_minimum_required` below 3.5 —
  the vendored opus that `audiopus_sys` builds asks for 3.1, so both jobs died
  in the build script. The release workflow sets
  `CMAKE_POLICY_VERSION_MINIMUM=3.5`, which is the escape hatch CMake's own
  error points at. Linux was never affected: its cmake comes from apt.

## [0.18.1] — 2026-08-23

### Changed

- **`naga` is declared once**, in `[workspace.dependencies]`. Both `rudel-app`
  (validating a widget's shader before wgpu can abort the process over a typo)
  and `rudel-lang` (checking the hydra port's output in tests) were pinning it
  separately. It was the only dependency declared in more than one crate.

- **Internal crate dependencies carry no `version` requirement**, reversing
  half of what 0.17.0 did. Centralising them in `[workspace.dependencies]` was
  worth it; the `version` on each was only ever there to satisfy `cargo
  package`, which strips the `path` and refuses a dependency without one. Rudel
  ships binaries and is not published to crates.io, so that was seven more
  literals to bump on every release for a command nobody runs. `cargo package
  --workspace` does not succeed here any more, by choice.

## [0.18.0] — 2026-08-23

### Added

- **Record the output to WAV, FLAC, MP3, Ogg Vorbis or Opus.** A `⏺ Rec` button
  next to Open/Save picks a destination; the extension picks the format. The
  master mix is captured at the device's own sample rate and streamed to disk by
  a writer thread rather than buffered, so a take is bounded by the file system
  and not by memory. Only the audio output is captured; MIDI and OSC send
  events, not sound.

  The lossy and lossless formats go through each codec's own reference library —
  LAME, libFLAC, libvorbis and libopus — built from vendored source, so there is
  nothing to install to play a rudel recording anywhere. WAV is the exception:
  a 44-byte header and raw PCM is less code than a dependency.

  Two consequences worth knowing. `cmake` must now be on `PATH` to build the
  workspace, because that is how `libopus`'s crate builds it (see
  CONTRIBUTING.md; Visual Studio ships one). And Opus encodes only at
  8/12/16/24/48 kHz — on a device running at 44.1 kHz, starting an `.opus` take
  says so rather than writing a file that plays at the wrong speed. Every other
  format takes the device rate as it finds it.

- **The wheel drives every slider.** Hovering a slider and scrolling moves it:
  the transport `cps`, the editor font size, the inline `slider(...)` controls
  in the code, and the volume slider. One notch is a hundredth of the slider's
  *range*, not of its own step, so every slider takes the same amount of
  scrolling end to end however wide it is — `cps` would otherwise have needed
  190 notches and an inline `slider(300, 2000)` a thousand. A slider with
  coarser stops than that (the font size, 11..32 by 1) keeps them. The wheel
  event is consumed while hovered, so a slider inside the editor adjusts rather
  than scrolling the view out from under the pointer.

  This reads the raw wheel events rather than egui's `smooth_scroll_delta`.
  The smoothed value is spread over several frames and rudel only repaints on
  input while the transport is stopped, so the ramp was cut off partway and
  most of every notch was dropped — measured at about a sixth of the scroll
  arriving.

### Changed

- **Pattern files are `.js`, not `.koto`.** Patterns are written as Strudel
  JavaScript — the preprocessor turns them into Koto — and the corpora they
  come from are directories of `.js`. Open/Save filter and default accordingly.

- **Dependencies updated**: `rfd` 0.15 → 0.17, `pest`/`pest_derive` 2.8 → 2.9,
  `symphonia` 0.6.0 → 0.6.1, `cpal` 0.18.1 → 0.18.2, `naga` 30.0.0 → 30.0.1.
  Nothing in the tree declares a `rust-version` above the workspace's own 1.96.

- **CI checks the MSRV it publishes.** The workspace has declared
  `rust-version = "1.96"` for some time with nothing verifying it, so a
  dependency bump could raise the real floor silently. There is now a job that
  reads the version out of `Cargo.toml` and runs `cargo check` on exactly that
  toolchain. Both workflows also name `cmake` and the autotools among their
  Linux packages: the hosted images carry them, but the recorder's codec
  libraries build from vendored C and a change to those images should fail at
  the install step rather than inside a build script.

### Fixed

- **`dracula` no longer reads as unimplemented in the API inventory.** The Note
  column carried the raw allowlist category ("editor theme"), which under an
  `intentional` status looks like a verdict; the theme is in fact a native
  `EditorTheme` and selectable in the editor settings, it is just not a name a
  pattern can call. `docs/API_INVENTORY.md` now renders each category's
  recorded reason, which says so — for all four categories, not just this one.

## [0.17.0] — 2026-08-23

Files, and the two clocks that were wrong.

### Added

- **Open and Save.** `Ctrl+O`, `Ctrl+S`, `Ctrl+Shift+S`, plus toolbar buttons,
  through the native file dialogs. Patterns are Koto source, so the dialogs
  filter on `.koto` and a name typed without an extension gets one. `Ctrl+S`
  with no file yet falls through to Save As rather than saving into nowhere,
  and the window title names the open file.

- **The editor buffer survives a close.** eframe's `persistence` is on, so the
  buffer is autosaved every 30 seconds and on exit, and restored on the next
  launch — along with the file it came from, so `Ctrl+S` after a restart still
  knows where to write. A failed read reports to the error bar instead of
  clobbering what is in the editor.

- **Release binaries.** `.github/workflows/release.yml` builds
  `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` and
  `aarch64-apple-darwin` on a pushed `v*` tag and attaches them to a GitHub
  release.

### Fixed

- **A live `cps` change no longer stalls or bursts MIDI and OSC output.** Both
  schedulers mapped seconds to cycles from the origin (`cycle = now * cps`).
  Slowing the tempo therefore re-derived a target cycle *behind* what was
  already scheduled — nothing new was queued until the clock caught up, which
  at a large enough drop is tens of seconds of silence — and raising it jumped
  the target forward, dumping a window of events at once. Both now use
  `Clock`, which re-anchors at the moment of the change the way Strudel's
  cyclist does; the audio path already did.

- **The audio callback renders one block instead of one frame at a time.**
  `write_frames` called `render_frame()` per sample, so every frame re-ran the
  event drain, the Csound `try_lock` and the pending-onset scan — work whose
  whole point is to be done once per buffer. It now renders the callback as a
  single `render_block` into a buffer owned by the stream callback, grown once
  and never reallocated on the audio thread.

- **The transport bar no longer overlaps itself.** The row ended in a
  right-to-left block, which anchors to the right edge however much space is
  left, so an overflowing row painted the output selector, the status text and
  the state light on top of each other. The controls row wraps now, and the
  status moved to the `i/o` row.

### Changed

- **`Clock` moved from `rudel-audio` to `rudel-core`**, so the MIDI and OSC
  schedulers can share the re-anchoring the audio path had. `rudel_audio::Clock`
  still resolves; the `rudel_audio::clock` module path does not.

- **Internal crate dependencies are declared once**, in
  `[workspace.dependencies]` with a version. `cargo package --workspace` failed
  before this — a path dependency with no version cannot be packaged.

## [0.16.0] — 2026-08-22

Kabelsalat, ported.

`K(...)` was listed as not portable in 0.15.0, on the grounds that it "needs a
JS engine per sample, as `dough` does". That was wrong, and the reason is worth
recording: the per-sample JavaScript superdough runs is *generated*.
Kabelsalat's compiler (`core/compiler.js`) topologically sorts the graph and
emits an instruction list, and the JavaScript is one of two pluggable back ends
beside a C one (`lib/src/lang/{js,c}.js`). Rudel is now a third — it emits
nothing and interprets the list directly, so no JavaScript runs at sample rate
and the graph is compiled once per evaluation rather than once per hap.

### Added

- **`K(...)`: kabelsalat graphs, compiled to a register program.**
  `@kabelsalat/lib` 0.4.1, written the way a patch is actually written:

  ```koto
  s("bd*4").K(audioin().lpf(sine(2).range(.2, .7)).mul(sGate.perc(.2)))
  ```

  73 node types, 26 ugens ported from `ugens.js`/`synth.js`, and the module
  macros (`perc`, `ad`, `ar`, `lpf`, `hpf`, `rangex`, `pan`, `mix`, `fork`, the
  `q*` filter family). Multichannel expansion works, so `sine([220, 330])` is
  genuinely two oscillators, and so does feedback — `x => x.delay(.2).mul(.8)`
  builds a cyclic graph whose loop reads a register one sample old, which is the
  mechanism upstream relies on too.

  A graph wraps whatever the hap would otherwise have played, as upstream's
  `chain.connect(workletNode)` does: `audioin()` reads that voice, so `K(...)`
  is an insert effect when a patch uses it and a synth when it does not. Both
  spellings work — `K(graph)` as a control pattern of its own, and
  `pat.K(graph)` as a method, which is the one a tune reaches for.

- **Bare kabelsalat names inside `K(...)`.** `sine`, `saw`, `noise`, `time`,
  `range`, `clock` and `delay` all already mean something else in Strudel.
  Upstream never evaluates the expression where it is written — its transpiler
  stringifies it for superdough to evaluate in kabelsalat's own scope. Rudel
  evaluates it in place, so a preprocessor pass qualifies the names instead
  (`crates/rudel-lang/src/preprocess/kabelsalat.rs`). Only kabelsalat's own
  names, only in call position, only inside `K(...)`: a method call is already
  unambiguous, and a name the table does not have is left alone so a patch can
  still reach an outer variable for a cutoff.

- **Patterns inside a graph, sampled per hap.** `K(sine(S("220 440")).out())` is
  two events a cycle — the graph is fixed, so the structure comes from the
  pattern. Upstream lifts these out of the source text as `pat[i]` placeholders
  and splices the values back before compiling; here any pattern reaching an
  inlet becomes a `pat` node, which covers `S(...)` and mini-notation with one
  rule. `sFreq` and `sGate` are nodes for the same reason.

- **A kabelsalat oracle.** `tools/oracle/gen_kabelsalat_oracle.mjs` replays
  `GenericProcessor`'s own loop against the real library and dumps 38 patches —
  the graph and 64 stereo frames each. `kabelsalat_parity` checks both; all 38
  agree node for node and sample for sample. It found four bugs before this
  shipped: `n(69)` wiring its argument as an inlet instead of *being* the value
  (so `midinote()` read 0 and every note came out at 8Hz), `argmin`/`argmax`
  breaking ties towards the first index where upstream's reduce takes the last,
  f32 phase accumulation drifting inside 64 samples, and `ad`/`pan` building the
  right sound from the wrong graph.

- **`bytebeat`, `floatbeat` and `raw`**, on the expression evaluator the
  `bytebeat` synth already carried — no JavaScript engine needed. These cannot
  run upstream at all: their compile step emits `const ${name} = ...` where
  `name` is a register expression, so the generated source reads
  `const r[3] = ...` and the whole graph dies on a SyntaxError.

### Fixed

- **`ByteBeatExpr` resolves identifiers**, so an expression can name values
  other than `t`. Unknown names are still NaN, as an unbound global is in
  JavaScript; `raw` uses this for `$input` and `time`.

### Changed

- **Oscillator phase accumulates in `f64`**, as a JavaScript number does. In
  `f32` it drifted measurably within 64 samples and audibly over a long note.
  Affects `bytebeat`'s siblings not at all, but `pulse` at an exact submultiple
  of the sample rate now lands on the boundary the way upstream does.

- **`NoteEvent` carries a `worklet` field**, and `rudel-dsp` gained
  `KabelProgram`/`KabelVoice`. Breaking for anything constructing a `NoteEvent`
  directly.

### Not ported

The MIDI nodes (`midin`, `midifreq`, `midigate`, `midivel`, `midicc`, `cc`) read
a live input device, and `scope`/`split` post buffers to a UI that does not
exist here; all pass their input through, which is upstream's own
`fallbackType`. `noise`, `pink`, `brown` and `dust` call `Math.random()`
upstream, so a patch sounds different every time it is played — rudel seeds a
generator per node instead, which means those four are the one place the numbers
cannot match. `worklet(src, ...)`, the string-taking form the transpiler emits,
is still missing: it expects source *text* evaluated at play time, and `K(...)`
needs no such thing.

## [0.15.0] — 2026-08-22

A parity campaign, measured rather than guessed. Running all 8004 patterns
shared on strudel.cc through both engines and crossing the results isolates the
ones that work *there* and fail *here* — 111 of them at the start, 88 now. Most
of what follows was found that way, and each fix was re-measured against the
whole corpus before the next.

Two numbers moved, and only one of them is the obvious one. The square counts
whether a pattern evaluates; it cannot see a pattern that still evaluates and
now plays something else. Exact hap-count agreement with Strudel, over the
patterns both engines run, went **1773 → 1786** — and one change scored *better*
on the square while moving 61 working patterns away from Strudel's output. It
was reverted and done properly.

### Added

- **`FX` effect chains.** `.FX(fx1, fx2, …)` puts a pattern through a chain of
  effects, each a pattern of controls, with repeated calls extending the chain:

  ```koto
  s("bd*4").FX(coarse(4)).FX(lpf(500).lpe(4).lpa(1).lpd(2)).FX(distort(1))
  ```

  Upstream builds this by running its post-effects section once per entry with
  the pattern's own controls appended last; here each entry becomes another
  wrapper around the voice, so the first stage sits nearest the source and the
  pattern's own controls are heard last. A stage carries filters as well as the
  post-effect rack, which is what upstream's own `lpf` example needs. It does
  not carry a stage's `delay`/`room` sends, `lfo`/`env` modulators or `gain` —
  those are resolved once per event, outside the voice. See
  [`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md).

- **`dough`, played as the bytebeat it always is.** ``dough`…` `` compiles the
  script's own JavaScript into an AudioWorklet running at sample rate, which
  needs a JS engine on the audio thread. What that code *is*, in every use of it
  in the wild, is a bytebeat player reading the hap's `s` value — so `.dough()`
  moves that value to `byteBeatExpression` and plays it through the bytebeat
  voice Rudel already ports. An interpretation rather than a port, and marked as
  one: worklet code doing something else gets bytebeat anyway. The alternative
  was not silence but the expression read as a sound name, playing something
  unrelated.

- **`JSON.parse`, `Boolean`, and `Array.prototype.reduce`.** Songs embed a
  lookup table — a pixel font, a chord book — as one JSON string literal;
  `lines.filter(Boolean)` is the idiom for dropping what a `split` left empty;
  and `reduce` builds a `timeCat` argument list carrying a running total.

- **JavaScript's counting loop.** `for (let i = 0; i < 50; i++) { … }` has no
  Koto spelling and becomes a `while`. With it, `+=` on a string — which Koto
  has no operator for at all — since building a mini-notation string a step at a
  time is what the loop is usually for.

### Fixed

- **`mini` did not parse its argument.** It was wired to the conversion that
  deliberately does *not* read a bare string as mini-notation. That rule is
  right for an ordinary pattern argument and wrong for `mini`, which is the
  parser: `mini('48 49 50')` was one hap of literal text instead of three notes.
  It evaluated, so the square never showed it — three corpus tunes were playing
  8 haps where Strudel gives 400.

- **`map` never recovered from an arity error.** It called the callback with
  JS's `(value, index)` and retried with one argument on failure, so a
  one-parameter callback took the error path on *every* element — and past a few
  dozen of those the VM stopped recovering. A 94-entry table failed where a
  59-entry one worked. It now reads the callback's declared arity and calls it
  correctly the first time.

- **`register` ignored its `patternify` argument**, and decided what to
  patternify by whether an argument was written as a mini literal rather than by
  whether it has structure. Upstream keys that on its pure fast path, which
  Rudel already mirrors as `Pattern::pure_value`. A helper doing arithmetic on
  its argument was handed the source text: `noteToMidi: not a note:
  "<d5 <g5!2 g4>>/2"`.

- **A block-bodied lambda with an argument after it.** `reduce((a, x) => { …;
  return a }, [])` — Koto ends a lambda at the end of its indented block, so the
  `, []` was read as part of the body. The error pointed eight lines below
  anything wrong. This was the largest single bucket in the corpus.

- **JavaScript escapes in a brace-carrying literal.** Such a literal becomes a
  Koto *raw* string, which applies no escapes, so a JSON table's `\"` reached
  the parser as `\\"`.

- **Two declarations on one line.** `var a = 1; var b = 2` dropped only the
  keyword that opened the line, and looked up a variable named `var`.

- **The declaration sort hoisted an update.** `melo = rudel_concat(melo, ']')`
  reads the name it assigns; lifting it above the line that first bound the name
  moved the read to where there was nothing to read.

- **A bare `silence` statement.** Upstream exports the silent *pattern*, not a
  factory, so a scratch pad ends with the bare word to go quiet — 39 of the 8004
  corpus patterns do. Here it returned the function itself.

### Changed

- **`filter` uses JavaScript's truthiness.** It kept `0` and `""`, which JS does
  not. `filter(Boolean)` depends on this.

- **`mini(x)` and `.dough()` produce different patterns** than in 0.14.0, per
  the two entries above. Both were wrong before; a pattern relying on the old
  behaviour will sound different.

- **`NoteEvent` carries an `fx_chain` field** and `VoiceSpec` gained
  `into_chained_voice`, for the `FX` work. Breaking for anything constructing a
  `NoteEvent` directly.

### Not ported

`K` is [Kabelsalat](https://kabel.salat.dev/), an external modular-synth DSL
with its own graph compiler targeting a worklet; `worklet` runs that compiled
source. Both need a JS engine per sample, as `dough` does, and porting the
language is a project rather than a feature.

## [0.14.0] — 2026-08-21

Hydra, ported. 0.13.0 gave the GPU a shader widget; this release puts a real
visual language on top of it.

### Added

- **Hydra chains, compiled to WGSL.** 51 of hydra's 52 functions, chained the
  way they are upstream and rendered in an inline widget:

  ```koto
  s("bd*4").hydra({ chain: Hydra.osc(20, 0.1, 0.8).kaleid(5).colorama(0.02) })
  ```

  A chain folds into one fragment shader per evaluation using hydra's own
  five-way composition rule from `generate-glsl.js`, so `osc().rotate().color()`
  wraps coordinates inward and colours outward exactly as it does there. The
  Koto VM is never invoked on the draw path.

- **Four output buffers, `src`, and feedback.** `o0`–`o3` take a chain each,
  `src` reads any of them, `prev()` reads the one the chain draws into, and
  `render` picks what is displayed — `render: 'all'` tiles all four, hydra's
  `render()` with no argument.

  ```koto
  s("bd*4").hydra({
    o0: Hydra.osc(15, 0.1, 0.7).kaleid(4),
    o1: Hydra.voronoi(10, 0.4).thresh(0.45, 0.1),
    o2: Hydra.src(Hydra.o0).modulate(Hydra.src(Hydra.o1), 0.35).colorama(0.01),
    render: 2,
  })
  ```

  Every buffer read is of the previous frame, which is upstream's rule rather
  than a concession: `format-arguments.js` binds the fbo that is *not* being
  drawn into, which is why `src(o0)` inside o0's own chain means `prev()` there
  too. Reproducing it keeps the four passes independent and means no texture is
  ever sampled while it is a render target.

- **An oracle for a package that has no vendored source.** `@strudel/hydra` is a
  loader that fetches hydra-synth from a CDN, so unlike every other feature here
  there was nothing in the Strudel tree to port against.
  `tools/oracle/gen_hydra_oracle.mjs` pins hydra-synth's own function table into
  `hydra_golden.json`, and `crates/rudel-lang/tests/hydra_parity.rs` checks every
  name, composition type, input name and default against it, then compiles every
  function through `naga`. That second guard caught a real bug during the port:
  hydra prepends a `color` input to `combine`/`combineCoord` at registration, so
  upstream's argument indices run one ahead of the raw table's.

  The CDN URL carries no version and `package.json` asks for `^1.3.29`, so the
  reference moves by construction. Rudel follows **1.4.0**, which is what a
  Strudel user gets today — and which changed `shift` from
  `c2.r = fract(c2.r + r)` (wrapping into `[0,1)`) to `c2.r += fract(r)` (not
  wrapping). Pinning does not stop the reference moving; it makes the movement
  reviewable.

- **Source functions live on `Hydra`.** `Hydra.osc(…)`, `Hydra.noise(…)` and so
  on. Upstream claims those as globals — which is why `clearHydra` has to put
  `shape` and `speed` back afterwards — but Rudel already has `osc` (the OSC
  output), `noise` and `shape` (core), and shadowing them would break patterns
  that never mentioned hydra. Chained methods keep hydra's own spelling.

### Changed

- **`hydra` is a widget method, not a stub.** It used to sit in the
  "not supported here, ignored" list beside `P5`; it now renders a chain.
  `initHydra(…)` and `clearHydra()` became silent no-ops rather than logging
  stubs, so a pattern copied from Strudel runs unchanged — there is no CDN fetch
  to perform and no canvas to tear down.

### Not ported

`sum` is broken upstream and is left that way: hydra generates
`vec4 sum(vec4 _c0, vec4 scale)` from a body that reads an undefined `s` and
returns a float from a `vec4` function. Compiling it confirms the first error;
either alone is fatal. Hydra only emits a function's GLSL when a chain uses it,
so this is latent there rather than fatal. A working one here would be invented
behaviour that diverges from every Strudel install rather than matching one.

`H(pattern)` samples a pattern per animation frame to drive a uniform, which a
chain compiled once per evaluation has nowhere to put; it is accepted and
ignored. `s0`–`s3` are webcam, video and screen capture.

## [0.13.0] — 2026-08-21

Rudel renders through eframe's wgpu backend, and until now nothing used it.
This release puts the GPU to work: a widget that runs a shader you write, and
a spiral that no longer has seams.

### Added

- **`shader`, a WGSL fragment body in an inline widget.** `.shader({ code:
  '…' })` paints into the widget rect through egui's own render pass — no
  second surface, and egui sets the viewport and scissor from the rect, so a
  shader cannot draw outside its widget. The body is wrapped in a prelude
  giving it `uv` (0..1 across the widget, y down) and a `u` uniform block
  carrying `res`, `time` in cycles, `gain`, `note` and `voices` from the
  sounding events, so a visual can follow the pattern:

  ```koto
  s("bd*4").shader({ code: '
    let d = length(uv - vec2<f32>(0.5, 0.5));
    return vec4<f32>(u.gain * (1.0 - d), 0.1, d, 1.0);
  ' })
  ```

  Write the body in **single quotes** — double-quoted strings are
  mini-notation, and a WGSL blob is not a pattern.

  The source is parsed and validated by `naga` before wgpu sees it. wgpu's
  default handler aborts the process on a validation error, which would end a
  set on a typo; instead the widget draws the message, the offending line, and
  a line number counted from your body rather than from the prelude you never
  wrote.

  This is not Hydra and is not a step towards it: there is no video-synth DSL,
  no shader graph, and no user callback on the query path. See
  [`docs/UNSUPPORTED.md`](docs/UNSUPPORTED.md).

### Changed

- **The spiral is drawn per pixel instead of stroked.** Strudel strokes one
  canvas polyline per hap, and so did Rudel. Adjacent haps then meet butt-end
  to butt-end with ends that are not parallel, so one side of every boundary
  overlaps — and because hap colours are translucent under the default `fade`,
  that overlap composites twice and reads as a bright radial seam — while the
  other side leaves a sliver of background. Finer sampling does not help; the
  seam is inherent to stroking.

  The spiral is now a signed distance field: a pixel's polar coordinates invert
  straight back to a spiral angle, so coverage is exact and the only soft edge
  is one pixel of deliberate anti-aliasing. Both painters take their bands from
  the same code, so colour, fade and geometry are unchanged — rendering a
  pattern each way gives the same colour histogram, with more band pixels and
  no seam pixels on the GPU side.

  `spiral({gpu: false})` asks for the old painter back. It is also selected
  automatically when the wgpu backend is not running, since the SDF painter's
  pipeline and buffers live in that renderer.

- **The spiral's default surface is 400×400, not 275×275.** Strudel's widget
  registration takes `size || 275` for the canvas and passes `size / 5` as the
  radius unit, so at the default `inset: 3` the "now" arc lands at radius 165 —
  outside the 137.5 a 275 canvas inscribes. The current position, the one thing
  a spiral most needs to show, was clipped into the corners.

  The geometry is not what moved: `spiral_size` still defaults to 55 and
  `inset` keeps its documented 3, so a pattern copied from Strudel draws the
  same spiral at the same radii. Only the canvas under it is bigger. Naming
  `size` still sets both, as upstream does — `spiral({size: 275})` reproduces
  the upstream framing, corner-clipped playhead included.

### Fixed

- **`let`, `const` and `var` inside a string literal are left alone.** The
  declaration-rewriting pass scanned line by line with no idea where strings
  began, so it stripped the keyword from any line *inside* a string that
  started with one. All three words are WGSL as well as JavaScript, which is
  how this surfaced — a multi-line shader body lost a keyword on nearly every
  line — but it applied to any string in any script. The pass now consults the
  same string mask its sibling passes already used, so the fix reaches every
  caller rather than the shader path alone.

## [0.12.4] — 2026-08-19

Two of 0.12.3's additions were placeholders. They are implementations now, and
the placeholder that could not become one was made a method instead of a
top-level stub.

### Added

- **`Math`, in full.** It held `pow` and nothing else, and a missing member is
  not a parse error — it surfaces as `'floor' not found in 'map'` somewhere
  down the expression that used it. All eight constants and every function are
  there, including the ones whose obvious Rust spelling is wrong: `Math.round`
  breaks ties towards +infinity where Rust breaks them away from zero (JS gives
  `0` for `-0.5`, Rust gives `-1`), `Math.sign` keeps a signed zero, and
  `max()`/`min()` with no arguments are the infinities. Every value in the test
  was read out of a real JavaScript engine rather than assumed.

  `Math.random` is the one member that cannot be pure: it is seeded once per
  process from the clock and is not repeatable, exactly as the host's is under
  Strudel. A pattern that wants a *reproducible* random still wants rudel's
  `rand` signal.

- **`setGainCurve`.** Really applied now: `setGainCurve((x) => x * x)` makes
  `.gain(0.5)` sound like 0.25, and the curve reaches all eight controls
  superdough puts through `applyGainCurve` — gain, postgain, velocity, delay,
  busgain, shapevol, distortvol, tremolodepth. Strudel's own documented example
  for it now runs, and it has come off the reference allowlist as implemented.

  The curve is *sampled* at evaluation time rather than called per note: the
  scheduler and audio paths have no Koto VM to call it on, which is the same
  constraint `probe_patternify` tabulates its callbacks around. 2049 points
  across `0..=8`, interpolated between and extrapolated beyond, so a curve with
  a step in it gets that step rounded over.

- **`dough`** is a pattern method, as it is upstream, not a top-level name. It
  attaches an `onTrigger` there because a Strudel pattern is inert until
  something listens; rudel's scheduler already routes whatever a script
  returns, so the pattern comes back unchanged.

`initHydra`, `hydra`, `H`, `P5` and `p5` remain accepted-and-logged rather than
implemented. They drive a visual runtime rudel does not have
(`docs/UNSUPPORTED.md`), none of them is part of Strudel's documented surface,
and the log line says so where the console panel will show it.

### Internal

6318 of the 8004 public strudel.cc patterns play, up from 6284, with the
rudel-only failures at 342. 31/31 strudel.cc tunes, 89/89 of eefano's
collection, 491/491 of the vendored drum patterns, no panics.

## [0.12.3] — 2026-08-18

Working down the differential list from 0.12.2. Of the 8004 public strudel.cc
patterns, **6284 now play, up from 6198**, and the rudel-only failures are down
from 388 to 346 — 265 distinct sources, since the corpus holds many near-copies.

### Fixed

- **`_name` variables.** Koto reads a leading underscore as a value to
  *discard*, so a script that merely follows the JavaScript convention
  (`let _drums = ...`, then `stack(_drums)`) failed with `attempting to access
  an ignored value`, pointing at the use and never at the name. Bare
  identifiers are renamed; a member access, a map key and Koto's real `_`
  discard are left alone, which is what keeps rudel's own `._spiral` inline
  widgets working.
- **JavaScript spread into call arguments**, the largest single cause on the
  list — 109 of the 133 spreads in the corpus, every one of them a plain call
  to a pattern factory. `stack(...xs)` cannot be lowered to `stack(xs)`,
  because a list argument is one *sequenced* pattern rather than several
  stacked ones, so the spread survives to runtime: each argument becomes a
  group, and `rudel_apply` flattens the groups back into an argument list at
  the call. A regression test pins that `stack(...xs)` matches
  `stack(a, b)` while `stack(xs)` still differs from both.
- **Names that existed in only one of the two forms.** `fastgap`, `euclidrot`
  and `fastchunk` are spellings Strudel registers explicitly and rudel had only
  in camelCase; `struct`, `scale` and `voicing` existed as methods but not as
  the standalone functions Strudel also exposes.
- **`reify`, `chooseWith`, `chooseInWith`** — all three were already
  implemented in `rudel-core` and simply not reachable from a script.
- **JS builtins**: `toUpperCase`/`toLowerCase` (aliases onto Koto's own),
  `slice` on strings, lists and iterators, and `concat` on lists — JS
  semantics, negative indices included.

### Added

`initHydra`, `hydra`, `H`, `P5`, `p5` and `dough` are accepted and ignored, and
`console.log`/`warn`/`error` write to the pattern log the console panel shows.
A pattern that also draws visuals now plays its *audio* instead of failing
outright, and says in the log why there are no visuals. Only names Strudel does
not document are stubbed: a documented one would be counted as implemented by
`docs/API_INVENTORY.md`, which a no-op is not.

### Internal

The regression corpora are unchanged: 31/31 strudel.cc tunes, 89/89 of
eefano's collection, 491/491 of the vendored drum patterns, and no panics.

Progress on the list understates itself, because a pattern usually has more
than one thing wrong with it: the spread fix moved 94 patterns past their first
error and 61 of them stopped at the next one, JavaScript's `"mask" + n` string
concatenation, which Koto rejects. That is now the largest remaining bucket at
61 patterns — but only 17 distinct sources, one of them duplicated 53 times, so
it is not worth rewriting every `+` in the language to reach.

## [0.12.2] — 2026-08-18

The other half of the 8004-pattern run: every pattern was also evaluated in a
real Strudel runtime, so a failure can be attributed rather than guessed at.
Of the 8004, **1260 fail in both engines** — user sketches that were never
finished — and the 933 that only rudel refused are what this release is about.
Six of them are now fixed, leaving 388.

### Fixed

- **`x. gain(1)` and `x .gain(1)`.** JavaScript ignores whitespace either side
  of a member-access dot; Koto reported `expected key after '.' in Map access`
  on the method the user did write. 315 patterns.
- **An argument written *out*dented from the one above.** A bracketed group's
  lines have to land on one column in Koto, and an over-indented argument was
  already pulled back — but an under-indented one broke the call just as badly,
  with the error landing on the closing paren several lines later. The
  alignment now works in both directions.
- **A comment inside a labelled chain.** `strip_comments` blanks a comment
  rather than deleting it, so error messages still point at the line the user
  wrote — but the label pass read that blank as the end of its expression and
  cut the chain in half. A gap followed by a leading dot now continues.
- **`sd : s("bd")`.** A labelled statement may have a gap before its colon.
- **`rev()`.** Strudel's `register` curries, so calling a no-argument transform
  with no argument hands back the transform rather than applying it to nothing:
  `.sometimesBy(0.8, rev())` passes a function. Every transform in the
  `noarg` group now does this, and `rev` — which was registered by hand and so
  missed the currying entirely — joined the group.
- **`seq([a, b])`.** A list argument is a sequence, as Strudel's `reify` makes
  it. It used to evaluate to silence.

### Added

`tools/oracle/strudel_diff.test.mjs` evaluates a directory of patterns in a
real Strudel runtime and emits the same `<outcome>\t<id>\t<detail>` shape the
rudel side produces, so any corpus can be diffed against upstream instead of
against a snapshot. Setup — it needs rather more of the strudel workspace
linked than the golden generators do — is in `tools/oracle/README.md`.

### Internal

Over the 8004 public strudel.cc patterns: **6198 play, up from 5069** before
this run began, with 0 panics. The regression corpora are unchanged: 31/31
strudel.cc tunes, 89/89 of eefano's collection, 491/491 of the vendored drum
patterns.

Two more intermittent test failures fixed, both pre-existing: `rudel-core`'s
two MIDI-note-queue tests share a process-global queue whose "any device" entry
is drained by either of them, and failed together about half the time; they now
take the same lock the MIDI port tests do.

The 388 that remain are 301 distinct sources spread over a dozen error shapes —
JavaScript spread into call arguments, `initHydra` (visuals, unsupported by
design), and a long tail of indentation interactions with no single cause left
in it.

## [0.12.1] — 2026-08-18

Four crashes and one silent mis-parse, all found by running the 8004 public
patterns from the strudel.cc pattern database. All predate 0.12.0.

### Fixed

- **Multi-byte characters no longer panic the preprocessor.** Three separate
  passes indexed a `&str` by byte and sliced without checking char boundaries:
  `quote_numeric_map_keys` copied its input a byte at a time, the widget scan
  in `rewrite_editor_widgets_with_context` stepped its cursor a byte at a time
  and then sliced from it, and `indent_dot_continuations` pulled a line back by
  subtracting a *column* count from a *byte* length. Any of the three brought
  the whole evaluation down on a non-breaking space, a CJK character, or — the
  case that found the third — a line indented with U+2006. `char::is_whitespace`
  is Unicode-aware, so those indents counted one column per three bytes.
- **`loopAt(0)` no longer panics.** `loop_at` divided the step count by its
  factor before checking it, so a zero factor reached `Ratio` and asserted
  `denominator == 0`. A zero factor makes the pattern silence, as `slow(0)`
  already did; `contract` next door had the guard already. Reachable from a
  literal `.loopAt(0)` and from `.loopAt("1 1")`, where a mini-notation string
  coerces to zero.
- **`f (x)` is a call again.** JavaScript ignores the space, so `stack (a, b)`
  is an ordinary call; Koto reads the parentheses as an expression of their own,
  making it `stack((a, b))` — one tuple argument, with the stacked patterns
  never reaching `stack`. The failure surfaced far from its cause, as
  `'rudel_widget_pianoroll' not found in 'tuple'` on whatever was chained
  afterwards. A new `tighten_call_parens` pass closes the gap, leaving
  control-flow keywords (`if (x)`), calls split across lines, and anything
  preceded by a string rather than a name alone. 361 of the strudel.cc patterns
  space a call this way.
- **A flaky UI test.** `the_frame_loop_keeps_going_for_each_thing_that_moves`
  pushed a sample job whose thread returned immediately, so whether the job was
  still in flight when the frame was stepped came down to scheduling — it failed
  about one run in ten, on this revision and before it. The job now blocks until
  the test is done with it.

### Internal

Measured over all 8004 public patterns in the strudel.cc database: **0 panics,
down from 3, and 5173 patterns play, up from 5069** (with 151 evaluating to
silence, down from 198). The regression corpora are unchanged — 31/31
strudel.cc tunes, 89/89 of eefano's song collection, 491/491 of the drum
patterns vendored in `website/src/repl/drum_patterns.mjs`.

The largest remaining failure is `initHydra` (252 patterns), which is visuals
and unsupported by design; the rest need a differential run against Strudel to
tell a rudel gap from a pattern that is broken anyway.

## [0.12.0] — 2026-08-18

An over-engineering audit of the whole tree, applied. Net −1651 lines across
59 files, no behaviour intended to change except where noted below.

### Removed

- `Align` and `Pattern::op_align`. Only `aligned_variants!` consumed them and
  `Align::In` was never constructed; the macro calls the `op_*` methods it was
  dispatching to.
- `EvalMeta::mini_locations`, `EvalMeta::labels`, `EvalMeta::cleanup`, and the
  `LabelMeta`/`CleanupHints` types behind the latter two. All three were written
  and never read — the editor takes per-hap source locations from the `m(...)`
  calls the preprocessor writes into the script, not from a side table, and
  nothing ever populated the label or cleanup channels at all.
- `rudel_dsp::note_name_to_midi`, a one-line re-export of
  `rudel_core::note_to_midi`.
- `Frac::min`/`Frac::max`. `Frac` derives `Ord`, which provides both with the
  same semantics, so every `a.min(b)` call site is unaffected.
- `reset_state`, a one-line alias for `reset_timelines` with no caller.
- The app's permanently-disabled `multi_cursor` setting, and `todo.md`, whose
  135 checkboxes were all closed and whose remaining-work notes contradicted
  later entries.

### Changed

- The reference pane's control list comes from `rudel_lang::reference()`
  instead of a hand-typed copy of 112 names, so it cannot advertise a control
  the engine no longer has. It reads alphabetically now rather than in the old
  curated order.
- The window icon is a baked `icon.png` loaded through
  `eframe::icon_data::from_png_bytes`, not a 256×256 signed-distance rasteriser
  run at every launch. Same artwork, ~125 fewer lines, nothing to rasterise on
  the startup path.

### Internal

Duplicate implementations collapsed onto one each: a second radix-2 FFT in the
spectrum widget onto `rudel_dsp::Fft` (which precomputes its twiddles once,
rather than per repaint); three copies of `value_to_midi` onto one in
`rudel_core`; three copies of the identifier-fragment scanner in the completion
code; two byte-identical `MethodContext` adapters; two brace/paren walkers onto
one parameterised `matching_delimiter`; two local `MidiSink` recorders in the
MIDI tests onto the module-level one; three sample-map resolutions onto
`resolve_map_source`; `fetch_cached_text` onto a UTF-8 decode of
`fetch_cached_bytes`; nine multi-control setters onto one macro; the duplicate
`EXTRA_CONTROL_BUILDERS`/`EXTRA_CONTROL_KEYS` tables onto one
`(spelling, key, builder)` row set; and mini's seeded chooser plus `randcat`'s
parallel selector onto a core `choose_in_with`.

Ten inline `if pure { … } else { fmap().inner_join() }` fast paths in `draw`,
`xen`, `edo` and `tonal` now route through one crate-visible
`patternify_value`, which also means they push a bypassed pure argument's
source location the way Strudel's `register` does — the tune-hap and songs
parity oracles agree either way.

`EdoScale → Intervals → Pitches` was a three-type pipeline feeding a single
call site; it is one constructor. Its `tonic` field was always 1 (so the base
ratio it computed was always 1.0) and `medium` was always an alias for `large`.

Hand-rolled code the platform already ships: base64 decoding (the `base64`
crate was already in the resolved graph via `ureq`), hex colour parsing
(`Color32::from_hex`, which also accepts `#rgb`), `startsWith`/`endsWith` (now
aliases onto Koto's own `starts_with`/`ends_with` rather than second
implementations), and Euclid's copying split and rotate-copy (slice `split_at`
and `rotate_left`).

`WidgetDrawColors` carried five fields over three distinct theme colours —
`text` and `active` were both the foreground, `muted` and `inactive` both the
gutter foreground. It carries three.

The hand-maintained standalone-transform inventory in the tests is gone; the
committed reference-surface snapshot already covers every name in it exactly,
and shows a removal as a diff line.

### Migrating

The removals above are all of items with no in-tree consumer, but they are
public API. `Align`/`op_align` callers want the `op_*` method the variant named;
`note_name_to_midi` callers want `rudel_core::note_to_midi`; `Frac::min`/`max`
callers need no change (`Ord` provides them); `reset_state` callers want
`reset_timelines`. Readers of `EvalMeta::mini_locations` want the hap contexts.

## [0.11.3] — 2026-08-17

A second test-only release, continuing the 2026-08-17 mutation sweep into
core's pure transforms.

### Internal

69 more surviving mutants killed, over core's pure transforms (`samples`,
`stepwise`, `choice`, `timing`, `xen`, `euclid`, `morph`) and three `rudel-lang`
binding files — 386 of the 2026-08-17 baseline's 926 now, leaving ~540.

Two of those needed a test that had never been written rather than a better
assertion: `wchoose`'s index arithmetic is only pinnable by driving its private
core with a constant chooser instead of the `rand()` it ships with, and
`fmap`/`filter` repeat a fixed 16-cycle probe window, so nothing below cycle 16
could see the repeat calculation at all.

`stepwise::slices` no longer computes a per-slice duration. `retime`, its only
consumer, discarded it. Upstream *appears* to weigh a stepless slice by
`dur * (occupied_steps / occupied_perc)`, but derives `occupied_perc` from
`.filter((t, pat) => pat.hasSteps)` — `Array.filter` passes `(element, index)`,
so `pat` is a number, the filter keeps nothing, and the weight is always
`undefined`. Real Strudel lays slices of 3/4 + 1/4 out exactly as it lays out
1/2 + 1/2, which is what rudel already did.

Two other survivors that looked like bugs are faithful ports, now recorded in
the tests that cover them: `linger` with a negative amount is silence (upstream
throws there — its own branch is handed a number where it expects a Fraction),
and `swingBy` genuinely does nothing to a 2-step pattern at `n = 2`, upstream
included.

## [0.11.2] — 2026-08-17

A test-only release, from a full mutation run over all eight crates. 11 539
mutants: **89.5% caught before, ~92.6% after** — 317 surviving mutants killed
across 43 files.

Nothing here changes what Rudel does. Four pieces of code were *removed*,
because mutation testing is how you find code that cannot affect the output:
each was a guard or a computation that something downstream already did.

### Fixed

Nothing user-visible. The three deletions are behaviour-preserving, and each is
verified by the tests that already covered the paths through them:

- `control_to_midi` filtered a non-positive `bendRange` before storing it — the
  third copy of that guard. `bend_value` and `bend_range_key` both fall back to
  `DEFAULT_BEND_RANGE` themselves, so the middle one could never be observed.
- `MpeState::free_expired` swept expired channel slots for its only caller,
  which independently tests the same `*slot <= on` predicate one line later.
- `process_input` had an arm returning `None` for note-offs, which the `match`
  below it already answered `None` for.
- `PostFxVoice::with_mods` computed the phaser's notch centre and Q at
  construction; `tick` recomputes both from scratch every sample before the
  first `process` ever runs.

The `Fixed` heading is a formality — none of these were reachable as a bug.

### Internal

Per crate, surviving mutants before → after: core 268 → 159, lang 169 → 104,
dsp 230 → 133, audio 124 → 108, app 79 → 65, midi 44 → 33, osc 6 → 3,
mini 6 → 4.

Two things did most of the work. In `rudel-lang`, **no test anywhere had ever
passed a Koto tuple** — `delete match arm KValue::Tuple(t)` survived in six
files, because every API that takes a list takes a tuple too and scripts only
write lists. And `match guard o.is_a::<KPattern>()` needs a second object type
to reject: `KFrac` (`Fraction(1)` in a script) is the only other one in the
workspace, and mutating the guard to `true` makes the `cast().unwrap()` behind
it panic.

In `rudel-dsp`, the lever was the JS oracles rather than Rust tests: extending
`gen_bytebeat_oracle.mjs` with 15 expressions took `bytebeat.rs` from 20
survivors to 2, and both goldens (`zzfx_golden.json`, `bytebeat_golden.json`)
are regenerated from the vendored superdough and still match sample for sample.
The `rudel-osc` and `rudel-midi` scheduler threads turn out to need no hardware
at all — a `UdpSocket` on `127.0.0.1:0` and a `MidiSink` that records to a
`Vec` respectively.

Files taken to zero or near it: `core/hap.rs` 13 → 0, `core/voicing.rs` 16 → 0,
`core/value.rs` 14 → 1, `dsp/spec.rs` 19 → 0, `dsp/filter.rs` 12 → 1,
`lang/lib.rs` 16 → 0, and the five `app/editor` files 15 → 1.

Much of what survives is genuinely equivalent, and the shapes repeat: every
comparison mutant in a continuous piecewise envelope agrees at its boundaries by
construction (23 of them across `envelope.rs`, `pitch.rs` and `zzfx.rs`),
fast-path predicates produce identical samples down either path, and `sign(s)`
returning ±1 makes `*` and `/` the same operation.

## [0.11.1] — 2026-08-15

A release about two crashes that a passing test suite could not see. Both were
found by writing tests for surviving mutants rather than for behaviour, and both
are the same shape: code that runs on every keystroke, handed input that is
correct only halfway through being typed.

Neither produced a wrong answer. They panicked, which is why nothing that
compares outputs had ever noticed.

### Fixed

- **Csound no longer takes the process down when a second tune starts.** Every
  `Csound::new` opened its own handle on libcsound, so the last instance dropped
  unmapped the library — while a process-wide pointer into it was still being
  called by the message callback. Stop one Csound tune, start another, and the
  second one crashed. The library is loaded once now and never unmapped. It
  surfaced as `rudel-audio`'s entire mutant run dying at "0 mutants tested" on a
  test that passes on its own.

- **The preprocessor no longer panics on a line that opens with an operator.**
  `rewrite_block_bodies`, `rewrite_ternaries` and `condition_start` each read one
  byte to the *left* of a `>` to tell `=>` from a comparison, and underflowed
  when the `>` was the first byte of the source. `> {` is not JavaScript anyone
  means to write, but it is what sits on screen midway through typing a lambda,
  and the preprocessor runs on every keystroke.

- **An unterminated `` mondo` `` template no longer indexes past the end of the
  source.** The same case: a half-typed template is the normal state of the
  buffer between keystrokes. It compiles to `silence()` now.

### Internal

Mutation coverage across five files, 293 surviving mutants to 63:
`preprocess/syntax.rs` 128 → 24, `app/samples.rs` 55 → 11, `app/panels.rs`
41 → 10, `editor.rs` 37 → 10, `preprocess/mondo.rs` 32 → 8. What is left is
either equivalent (verified by applying each mutation and diffing output, not by
argument) or needs hardware — a cpal device or a live MIDI clock.

Two of those files had been written off as untestable and were not: `app/`
already had `RudelApp::headless()` and an `egui_kittest` harness driving the real
app, and a widget that paints rather than returns can be checked by reading the
shapes out of egui's `FullOutput` — `Shape::Text` carries the galley, so the
gutter's numbers, positions and colours are all assertable without extracting
anything.

## [0.11.0] — 2026-08-13

A release about the songs people actually write. The corpus this time is
[eefano/strudel-songs-collection](https://github.com/eefano/strudel-songs-collection)
— 88 complete Strudel songs, not documentation snippets — and **16 of them
evaluated when it was first run; all 88 do now.**

Very little of what it found was JavaScript being exotic. It was Rudel's:
`register`, the way a script defines its own pattern method, was not bound at
all, and it opens about a quarter of the corpus. Under that sat a preprocessor
that had only ever been asked to read documentation, where nobody writes a
`typeof`, a brace-bodied arrow, or a chain long enough to need three line
breaks Koto does not allow.

The last two songs wanted something the engine could not do at any price: they
define a *combinator*, and a combinator has to look at the haps of the cycle it
is given. Every Koto callback before this ran once at construction and baked its
answer, which cannot express "number these, and say how many there were". So
Koto is built with its `arc` feature now, a script's own function can run during
a query, and the engine's own vocabulary — `Pattern`, `Hap`, `TimeSpan`,
`Fraction` — is something a script can hold. That is a real widening of the
compatibility surface, taken deliberately.

The editor also got a right-click menu, which is the first way into any of its
actions that is not a keyboard shortcut.

### Added

- **A right-click menu in the editor.** Every action it offers already had a
  keyboard shortcut and no other way in: evaluate, evaluate block, hush, panic,
  cut/copy/paste/select-all, toggle comment, indent and outdent. Each entry
  routes through the same code path as its shortcut rather than repeating the
  edit, and shows the shortcut beside it. Paste needs the clipboard *read* egui
  does not expose, so the platform clipboard is used directly; entries that
  cannot act — cut and copy with no selection, paste with an empty clipboard —
  are greyed rather than silently doing nothing.

- **A script can define a pattern combinator, not just use one.** The engine's
  own vocabulary is bound — `Pattern(state => haps)`, `Hap`, `TimeSpan`,
  `Fraction`, and a pattern's `query` / `splitQueries` / `sortHapsByPart` — and
  `Pattern.prototype.name = function (…) { … }` binds the result as a method,
  which is how a Strudel script writes a combinator before it lands upstream:

  ```js
  Pattern.prototype.enumerate = function () {
    const pat = this.sortHapsByPart()
    return new Pattern(state => {
      const haps = pat.query(state.withSpan(span => span.begin.wholeCycle()))
      return haps.map((hap, i) => new Hap(hap.whole, hap.part.intersection(state.span),
                                          [hap.value, i, haps.length]))
                 .filter(hap => hap.part != undefined)
    }).splitQueries()
  }
  ```

  Spans, haps and states are plain maps, so `hap.part.begin` is ordinary
  access; fractions are an object because they carry exact rational
  arithmetic that a float would quietly lose. A prototype method's arguments
  are *not* patternified — a combinator reads its argument pattern's haps, and
  sampling it per cycle would make that impossible.

  This is the one place a script's code runs during a query, and it is what no
  amount of eager probing can replace: "look at the haps of this cycle and
  number them" is not knowable before the query asks.

- **`compressSpan`, `focusSpan` and `zoomArc`**, the span-object forms of
  `compress`/`focus`/`zoom`, as methods and as top-level functions taking the
  pattern last (`zoomArc(span, pat)` == `pat.zoomArc(span)`). They were the last
  names in the API inventory marked unsupported for a reason that had stopped
  being true: they take a `TimeSpan`, which no script could hold until the
  engine vocabulary above exposed one.

### Fixed

- **Songs written in real Strudel evaluate.** Measured against
  [eefano/strudel-songs-collection](https://github.com/eefano/strudel-songs-collection),
  88 complete songs rather than doc snippets: **16 of them evaluated, now all 88**.
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

  And the JavaScript the preprocessor did not read: `new`, `this`, `typeof`,
  the conditional operator, brace-bodied arrow, `function` and `if`/`else`
  bodies, `&&`/`||`/`!`,
  block comments, object spread, numeric object keys, `.length`/`.value`/`.n`,
  comma-separated declarations, a name Koto reserves used as a variable, and the
  line breaks JavaScript allows anywhere and Koto does not — after `=`, `=>` or
  `:`, before a call's `(`, and inside a nested argument list.

- Two allowlisted documentation examples (`seed`, `onTriggerTime`) run now —
  both were only ever blocked by syntax the above adds — and are no longer
  allowlisted.

### Changed

- **A script's own function can be called from the query path.** Koto is built
  with its `arc` feature rather than the default `rc`, so its values are
  `Arc`-backed and the VM is `Send`; a VM behind a mutex then satisfies the
  `Send + Sync` bound on a pattern's query closure. `crates/rudel-lang/tests/send_sync.rs`
  pins that, since the feature is otherwise invisible from the code.

  This is what `apply` needed to take a **pattern of functions** as well as a
  function — `apply(pick("<0 1>", [x => x.gain(1), x => x.fast(2)]))`, how a
  script switches arrangement per section, and previously an error. Which
  function to call is not known until the pattern is queried, so it is the one
  place the VM is reached from a query; every other callback is still applied
  eagerly at construction, which is cheaper and keeps errors attached to the
  evaluation that caused them.

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
