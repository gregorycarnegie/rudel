# Unsupported and intentionally different features

Rudel is a **native Rust** application. Strudel is a **browser** application. A
number of Strudel packages exist only to bridge to browser/web-platform APIs
(WebGL, DeviceMotion, Web Serial, the Gamepad API, MQTT-over-WebSockets, the
Csound WASM build, web components / iframes) or to provide alternative language
front-ends (Tidal, Mondo). Rudel deliberately does not port these; this page is
the authoritative list of what is intentionally unsupported, what is deferred,
and how Rudel differs where it does provide an equivalent surface.

This document tracks the *user-visible* contract. The internal parity checklist
lives in [`FULL_STRUDEL.md`](../FULL_STRUDEL.md).

## Pattern functions

### Names that are documented but have no source in the pinned Strudel

These appear in Strudel documentation or in the wild but are not defined
anywhere in the vendored checkout this parity work targets (see
[`STRUDEL_SOURCE.md`](STRUDEL_SOURCE.md) for the pinned revision), so there is no
reference behaviour to port. They are not available in Rudel:

| Name | Notes |
| --- | --- |
| `degreeToNote`, `toScale` | Custom interval-list scales. `scale("C:major")` and the named-scale table cover the documented cases. |
| `ncat` | Not defined in the pinned checkout. `timecat`/`stepcat` cover weighted concatenation. |
| `envL`, `envLR`, `envEq`, … | Envelope *signals*. `lfo`/`env` modulators, the per-effect envelopes (`lpenv`, `penv`, `fmenv`, `wtenv`) and `range` over a signal cover the same ground. |
| `ifp` | Not defined in the pinned checkout. |

If a future Strudel bump introduces them, `crates/rudel-lang/tests/reference_parity.rs`
fails on the new name and points here.

### `compressSpan`, `focusSpan`, `zoomArc` — internal, not exposed

Upstream these take a `TimeSpan` **object** (with `.begin`/`.end`) and throw on a
plain array, so they are internal helpers rather than user API. Rudel has no Koto
span type and exposes the user-facing two-argument forms instead: `compress(a, b)`,
`focus(a, b)`, `zoom(a, b)`.

## Drawing and visuals

### Draw runtime (`@strudel/draw` `draw.mjs`) — partial, by design

Strudel's `draw.mjs` drives a full-screen `<canvas>` painter lifecycle:
`getDrawContext` grabs/creates a global canvas, `Pattern.prototype.draw` and
`onPaint` register arbitrary JavaScript painter callbacks, `getPainters`
collects them, and a `Framer`/`Drawer` pair runs a `requestAnimationFrame` loop
that maintains a rolling memory of visible haps (with lookbehind/lookahead
windows and future-hap invalidation) and calls every registered painter once per
frame. `cleanupDraw` / `cleanupDrawContext` tear the canvas and painters down.

**What Rudel does instead.** Rudel runs a scheduler-time drawing loop for the
*inline editor widgets* only (`_pianoroll`, `_punchcard`, `_wordfall`,
`_pitchwheel`, `_spiral`). Each frame the editor queries the active pattern over
a draw window (`crates/rudel-app/src/editor/widgets/query.rs`) and repaints the
reusable per-`(type, id)` native surfaces owned by the widget host
(`crates/rudel-app/src/editor/widgets/host.rs`). This is the equivalent of
Strudel's `Drawer` querying haps and invoking painters, but the painters are
Rudel's native Rust drawing code, not user-supplied callbacks.

**Intentional limitation.** Rudel does **not** run arbitrary user painter
callbacks (`Pattern.draw(ctx => …)`, `onPaint`) and does not maintain a global
full-screen draw context. By design the Koto VM is never invoked from the
real-time/draw query path, so a pattern cannot register a Koto closure that runs
every animation frame. Only the built-in inline visualisers are available. The
full-screen draw context, `Framer`/`Drawer` rolling visible-hap *memory*,
lookbehind/lookahead window bookkeeping, future-hap invalidation, and the
`cleanupDraw`/`cleanupDrawContext` lifecycle are not ported; the inline widget
host re-queries the pattern each frame instead of keeping painter-side hap
memory.

**The event-annotation controls around the draw runtime are implemented.**
`label` (a multi-control, so `label("bd:BD!")` also sets `activeLabel`) and
`activeLabel` set the text the pianoroll draws for an event, with `activeLabel`
replacing `label` only while the event sounds. `markcss` styles an event's
source span in the editor while it plays — Rudel paints a background rather
than applying CSS, so the declaration list is scanned for a colour
(`markcss('outline: solid 2px #ff0000')` flashes red) and the rest of the CSS
is ignored; with no `markcss`, the `color` control colours the flash instead.
Note that a CSS string needs single quotes: double-quoted strings are
mini-notation.

### `log`, `logValues`, `onTriggerTime` — implemented, with different timing

`pat.log()` and `pat.logValues()` write a line per event as it plays. Strudel
sends these through `logger()` into the REPL's side menu; Rudel's scheduler
writes them to a process-global ring which the app drains into a **console
panel** below the editor (hidden until something logs, with a clear button).
Both accept a formatting callback (`logValues(v => 'saw ' + v.s)`); since the
Koto VM cannot run in the realtime path, the callback is applied ahead of time
over a 16-cycle probe and the resulting message is carried on the event — the
same probe-and-bake `filter`/`fmap` use, so a callback that depends on
something other than the hap will not see later changes.

`pat.onTriggerTime(f)` fires a Koto callback as each event's onset passes. This
is the one callback that has to run *later* rather than at build time, so the
evaluation's Koto VM is kept alive past `eval` and the app fires the hooks from
its frame loop (`crates/rudel-lang/src/triggers.rs`). Timing is therefore
frame-accurate rather than sample-accurate — the same caveat upstream carries,
where the hook is a `window.setTimeout` and its own docs call it "innacurate
for audio tasks". This does not reopen the draw-callback path above: the hook
runs on the UI thread after the fact, not inside the query path.

### Editor themes — `strudelTheme`, `whitescreen`, `dracula`

Strudel's theme catalog is a set of CodeMirror themes selected in the REPL
settings. Rudel ports three as native `EditorTheme` variants
(`crates/rudel-app/src/editor/settings.rs`), each carrying both the syntax
palette and the matching `DrawTheme` the inline visualisers use. Because a
theme is an editor setting rather than a name a pattern can call, theme names
do not appear in Rudel's scripting surface. The rest of Strudel's catalog is
not ported.

### `clearScope` — accepted, no-op

`clearScope()` deletes the user variables Strudel's block-based eval leaks into
its shared `strudelScope`. Rudel evaluates each script in a fresh Koto VM, so
nothing accumulates across evaluations and there is nothing to delete. It is
accepted and returns silence, like `registerSoundfonts()`.

### `getDuration` / `getDur` — implemented, and synchronous

`getDuration(name[, n])` returns a loaded sample's length in seconds. Upstream
reads it off the decoded `AudioBuffer` and returns a promise (so patterns must
`await` it); Rudel's sample bank publishes each length as it registers the
sample, so the call returns the number directly and needs no `await`. A sound
that is unknown or not loaded yet reads as `0`.

### `animate` (`@strudel/draw` `animate.mjs`) — intentionally unsupported

`animate` is built directly on the `draw.mjs` runtime: it registers a per-frame
JavaScript painter that draws arbitrary shapes from patterned visual params
(`x`, `y`, `w`, `h`, `angle`, `r`, `fill`, `smear`) onto the global canvas, plus
helpers (`rescale`, `moveXY`, `zoomIn`) and a `smear`/clear toggle, and reports a
"sync mode" status. Because it depends on the arbitrary-callback draw runtime
described above — running user-driven drawing every animation frame — the
`animate` painter is **intentionally unsupported** in Rudel. There is no native
equivalent surface; patterns that call `animate` will not produce visuals. The
supported way to get scheduler-time visuals in Rudel is the inline editor widgets
(`_pianoroll`, `_punchcard`, `_wordfall`, `_pitchwheel`, `_spiral`,
`_claviature`, `_scope`, `_spectrum`).

The `register`-based param transforms themselves — `rescale`, `moveXY`, `zoomIn`
— **are** implemented (`crates/rudel-core/src/draw.rs`), since they are pure
pattern transforms over the `x`/`y`/`w`/`h` params rather than painters. They
evaluate and emit the same control maps as Strudel, so `.rescale(2)` /
`.moveXY(0.1, 0.1)` / `.zoomIn(0.5)` are chainable and queryable for parity — but
with no `animate` painter to consume `x`/`y`/`w`/`h`, they produce no visual on
their own.

### Audio analyzer visuals — `scope`/`tscope`/`fscope`/`spectrum` (`@strudel/webaudio`) — implemented

The analyzer widgets **are** implemented with Strudel's semantics
(`crates/rudel-app/src/editor/widgets/analyzer.rs`): `scope`/`tscope` (the
same painter, like upstream) draws a falling-edge-triggered oscilloscope with
`align`/`trigger`/`pos`/`scale`/`thickness`/`smear` options; `fscope` draws
per-bin frequency bars (`scale`/`pos`/`lean`/`min`/`max`); `spectrum` draws
the scrolling log-frequency spectrogram with per-widget color memory.

Like Strudel's per-`analyze`-id `AnalyserNode`s, each widget has its own
analyzer: the mixer keeps a lock-free ring per widget tag
(`rudel_audio::ScopeTaps`) fed only by the voices of the tagged pattern, plus
a master-mix ring, and frequency data is smoothed across frames like
`AnalyserNode.getFloatFrequencyData` (τ = 0.5). The explicit `.analyze(id)`
control itself is not exposed — Rudel wires taps through widget ids only.

## Audio effects

### Reverb — convolution, with a seeded impulse response

Strudel's reverb is a `ConvolverNode` fed an impulse response that
`reverbGen.mjs` *generates from white noise*: an exponentially decaying random
buffer with an optional fade-in and a gradual lowpass sweep. Rudel does the same
— `crates/rudel-dsp/src/convolver.rs` ports `generateReverb`,
`applyGradualLowpass` and `adjustLength`, and convolves with a
uniform-partitioned overlap-save FFT convolver, one reverb per orbit.

All of the controls work: `size`/`roomsize` (the -60dB decay time),
**`roomfade`** (the IR's fade-in), `roomlp` → `roomdim` (the lowpass sweep across
the tail), and **`ir` / `iresponse` / `irspeed` / `irbegin`** — pass the name of
any loaded sample to convolve against it instead of the generated tail.

The impulse response is normalized exactly as a `ConvolverNode` does — its
`normalize` flag defaults to `true` and superdough never turns it off — so the
wet level stays roughly constant as `size` changes, and `size` alters the tail's
*length* rather than its loudness.

Two deliberate differences:

- **The noise is seeded.** Upstream calls `Math.random()` per sample, so its
  impulse response is different every time it is built; Rudel uses a fixed seed,
  so a given room setting always sounds the same and rebuilding the reverb
  mid-session does not change its character. Because upstream's IR is random,
  there is no sample-exact target to match here anyway — only the parameter
  semantics.
- **The wet return is one partition late** (~23ms at 44.1kHz), which is inherent
  to uniform partitioning; Web Audio's `ConvolverNode` has no such latency. It
  reads as a short reverb pre-delay.

### `leslie`, `squiz`, `fshift` — no effect (control-only upstream too)

These are SuperDirt effects. Strudel registers the controls but `superdough` has
no DSP for them either — they exist to be forwarded to a SuperDirt instance over
OSC. Rudel does forward them (`crates/rudel-osc`), so they work exactly as well
as they do in Strudel when you are driving SuperDirt, and they are silent in the
native engine on both sides.

### Modulators (`lfo`, `env`, `bmod`) — partial

`lfo(...)`, `env(...)` and `bmod(...)` work: the sources are ported from
superdough's worklets and their output is added to the target control's own
value, as Web Audio does when a node is connected to an `AudioParam`.
`depth`/`depthabs`,
`rate`/`sync`, `shape`/`skew`/`curve`/`dcoffset`/`phaseoffset`, `retrig`, the
envelope's `attack`/`decay`/`sustain`/`release` and its `acurve`/`dcurve`/
`rcurve` curvatures all behave as upstream, including the rule that a modulator
with no explicit `control` targets whatever was applied just before it in the
chain.

**Only a subset of controls can be modulated.** Strudel's target table covers
every parameter of its Web Audio graph; Rudel bakes most controls into a voice
when it is constructed, so a modulator can only reach the parameters its DSP
already varies per sample:

| Target | Controls |
| --- | --- |
| Oscillator frequency | `s`, `freq`, `note` |
| Level | `gain`, `postgain` |
| Filters | `cutoff`, `resonance`, `hcutoff`, `hresonance`, `bandf`, `bandq` |
| Post effects | `shape`, `shapevol`, `distort`, `distortvol`, `crush`, `coarse` |

A modulator naming anything else is skipped and has no effect — the same outcome
as Strudel's "may not be modulatable" path, which also carries on. For sampler
voices only `gain`, `cutoff` and `resonance` apply. The drum, ZZFX, bytebeat and
bus voices render from a fixed recipe, so of that table only the **filters** are
theirs to modulate — `gain` and the oscillator frequency are baked in when the
voice is built.

`bmod` carries the same restriction, and one more of its own: `.bus(n)` mixes a
voice's post-effect output into signal bus `n` (scaled by `busgain`) on top of
its normal orbit routing, so `dry(0)` turns a pattern into a pure modulation
source, and `bmod({ b: n })` reads it back as `(signal + dc) * depth / 0.3`.
The bus is summed to mono on the way in, which is what Web Audio does on the way
into an `AudioParam` anyway. Voices that send to a bus are rendered before the
ones that read it, so a reader sees the same block its sender just wrote — one
level deep: a voice that both sends to a bus and reads one is rendered with the
senders, and sees a partly-filled bus.

`s("bus").n(n)` plays a bus back as a source, gated by its own linear ADSR
(`[0.001, 0.05, 1, 0.01]`), so a second pattern can run it through effects —
the per-voice filters and the post-effects alike.

Not implemented: **`subControl`** (pointing a modulator at another modulator's
parameters) is ignored; and **`fxi`** (which link of an `FX` chain to target) is
moot while `FX` itself is unported.

### Soundfonts — supported, but General MIDI fetches over the network

Both of Strudel's soundfont paths work.

**General MIDI (`gm_*`).** The 125 `gm_*` sound names play, backed by the same
[WebAudioFont](https://github.com/surikov/webaudiofont) presets Strudel uses.
Presets are fetched **on first use** from
`https://felixroos.github.io/webaudiofontdata/sound` — the same default
Strudel has — so:

- Rudel makes an HTTP request the first time a pattern plays a given
  instrument. This is the only feature that reaches the network at play time.
- That first note is **silent** while the fetch runs, exactly as it is upstream
  (there the loader is async and the note is missed). It sounds from the next
  one on.
- Presets are ~1MB each and are cached on disk alongside downloaded samples, so
  each is fetched once per machine.
- `setSoundfontUrl(url)` repoints this at another mirror or a local directory
  if you would rather not use the CDN.

`registerSoundfonts()` exists and is accepted, but is a no-op: Rudel knows the
General MIDI names from a built-in table and loads them on demand, so there is
nothing to register up front.

**`.sf2` files.** `loadSoundfont(path)` reads a local SoundFont and returns the
sound name to play it with; each preset in the file is an `n` index, so
`loadSoundfont('/packs/piano.sf2')` then `s('piano').n(2)` — or
`.soundfont('piano', 2)` — plays its third preset. No network involved.

`loadSoundfont` also accepts an http(s) URL, which is cached on disk like a
sample pack.

A zone's own SoundFont amplitude/pitch envelope generators are not read; the
`attack`/`decay`/`sustain`/`release`, `vib` and `penv` controls shape the note
instead. That matches upstream, whose soundfont loader also drives the envelope
from `value.attack`/… rather than from the font (`fontloader.mjs`).

### `stretch` and `bytebeat` — implemented

**`stretch`** is a pitch shifter: superdough's `phase-vocoder-processor`
worklet, ported in `crates/rudel-dsp/src/vocoder.rs` along with the overlap-add
framework it sits on (`ola-processor.js`). The control maps to a pitch factor the
same way (`max(0, (v < 0 ? v * 0.25 : v) + 1)`), so `stretch(1)` is an octave up
and `stretch(-1)` is a minor third down. It runs per voice, at the head of the
post-effect chain, as it does upstream.

Two differences: it is by far Rudel's most expensive per-voice effect (two
2048-point FFTs per 128 samples per channel — the same cost upstream pays), and
upstream compensates the vocoder's latency by scheduling a stretched voice 0.04s
early, which Rudel's scheduler has no per-effect pre-roll for, so a stretched
voice sounds fractionally late.

**`s("bytebeat")`** plays an integer expression sampled per audio frame, with
`byteBeatExpression` (`bb`) choosing the formula and `n` selecting one of the 15
built-in beats. Upstream compiles the expression with `new Function`, i.e. it
runs real JavaScript; Rudel has no JS engine, so
`crates/rudel-dsp/src/bytebeat.rs` carries a parser and evaluator for the
integer-expression subset bytebeats actually use — JS operator precedence, JS
`ToInt32` coercion on the bitwise operators, 5-bit shift masking, ternaries and
short-circuiting, and the `Math` functions. It is pinned against V8 by
`crates/rudel-dsp/tests/bytebeat_golden.rs`.

What that leaves out: the exotic `chyx` helpers upstream injects (`bitC`, `br`,
`sinf`, `regG`, …), which none of the built-in beats use, and anything needing a
real JS runtime (string methods, regexes, the `eval(unescape(escape…))`
compression idiom). An expression that fails to parse falls back to silence.

Note that Rudel treats only *double*-quoted strings as mini-notation, so write a
bytebeat in single quotes: `s("bytebeat").bb('t&t>>8')`.

### Wavetable oscillator — implemented

`tables(url[, frameLen])` loads a collection of wavetables and `s("name")`
plays them, as upstream: each `.wav` is sliced into `frameLen`-sample
single-cycle frames (default 2048), `wt` sweeps the read position through the
frame stack with interpolation, and `warp` + `warpmode` distort the read phase.
All 22 warp modes are ported from the `wavetable-oscillator-processor` worklet
and golden-tested against it (`crates/rudel-dsp/tests/warp_golden.rs`), as are
the `wt`/`warp` envelopes and LFOs (`wtenv`/`wt{adsr}`/`wtrate`/`wtsync`/
`wtdepth`/`wtshape`/`wtskew`/`wtdc`, and the `warp*` twins) and the
`unison`/`detune`/`spread`/`wtphaserand` unison stack. The *additive* wavetable
(`partials`/`phases`) remains a separate, also-implemented path.

Like `samples(...)`, a `tables(...)` source is a local folder/`strudel.json`,
an http(s) URL, or a `github:`/`bubo:` pseudo-URL, and is fetched once per
session rather than on every re-evaluation.

## External integrations and inputs

### Hydra (`@strudel/hydra`) — intentionally unsupported

`@strudel/hydra` (`initHydra`, `H`, `clearHydra`) embeds the
[Hydra](https://hydra.ojack.xyz/) live-coding video synth, a WebGL/`regl`-based
fragment-shader engine, and lets patterns drive its uniforms. It is fundamentally
a browser WebGL integration with its own JavaScript DSL. Rudel is a native egui
application with no embedded JavaScript/WebGL video-synth engine, so Hydra is
**intentionally unsupported**. There is no native equivalent surface and no plan
to embed a shader video synth; use Hydra in Strudel's web REPL if you need it.

### Device motion / orientation (`@strudel/motion`) — intentionally unsupported

`@strudel/motion` (`enableMotion` plus signals such as `accX`/`accY`/`accZ`,
`gravity*`, `rotation*`, and `(absolute)orientation*`) exposes the browser
`DeviceMotionEvent`/`DeviceOrientationEvent` sensor streams as patternable
signals, intended for phones/tablets running the web REPL. Rudel targets desktop
(Windows/macOS/Linux) and has no device-motion sensor source, so these signals
are **intentionally unsupported**. There is no native equivalent surface.

### Mouse and keyboard — supported natively

`mousex`/`mousey` (and the `mouseX`/`mouseY` spellings) and
`keyDown`/`whenKey` **do** work. Strudel reads them from `document` event
listeners; Rudel's egui window is the source instead, publishing the pointer
position and the held keys to a process-global input bus each frame
(`crates/rudel-core/src/input.rs`), which the signals read at query time. Keys
are named exactly as in Strudel — the browser's `KeyboardEvent.key` values
(`"a"`, `"Control"`, `"ArrowUp"`, `" "` for space) — including Strudel's
shorthands (`ctrl`, `alt`, `shift`, `up`/`down`/`left`/`right`), so
`whenKey("ctrl:j", …)` names the same combination in both.

### MIDI device input (`midin`, `midikeys`) — supported, with different timing

`midin(device)` and `midikeys(device)` both work. Each opens the named MIDI
input port and returns a factory, exactly as upstream: `midin` gives
`(cc[, channel]) -> pattern` reading only that device's control changes, and
`midikeys` gives `(noteLength?) -> pattern` of the notes played on it. The
app-selected input device and the device-agnostic `ccin(cc[, chan])` signal
still work alongside them.

Upstream returns a promise (WebMidi is async, so patterns `await midin(...)`).
Rudel returns the factory immediately and opens the port in the background, so
the `await` is unnecessary — a signal reads 0, and `midikeys` yields no notes,
until the port is open.

Two timing differences in `midikeys`, both because Rudel has no wall-clock →
cycle map outside the scheduler. A note is placed at the start of the scheduler
block that picks it up (upstream stamps it with the cyclist time the message
arrived at, which lands in the same block either way), and there is no
out-of-band immediate trigger — the note sounds on the next scheduler block
rather than being dispatched straight to the audio engine. As upstream does,
note-*offs* are ignored: a `midikeys` hap's length comes from the pattern
(`kb(0.25)`), not from when the key is released.

### MIDI output `midicmd` and `midimap` — supported, with two differences

`.midicmd("clock"|"start"|"stop"|"continue")` sends the system-realtime byte,
and the array forms `["progNum", n]`, `["cc", ccn, ccv]` (with `ccv` in 0..1)
and `["sysex", id, data]` send a program change, control change and sysex frame
on the hap's channel. Upstream's `['cc', …]` branch fires at length **2** and
passes `midicmd[0]` — the literal string `'cc'` — as the controller number, so
it can only throw; Rudel implements the evidently intended three-element form.
Upstream also sends a Start from every hap whose whole begins at cycle 0, which
would mean one Start per hap; Rudel leaves that to its engine-level transport.

`defaultmidimap({lpf: 74})` and `midimaps({mymap: {lpf: {ccn: 74, min: 0, max:
20000, exp: 0.5}}})` register control-to-CC tables, and a hap picks one with
`.midimap("mymap")` (or uses `default`). `midimaps("github:user/repo")` reads
that repo's `midimap.json`, and any other string is used as a URL or a local
path. The inline form applies during evaluation; a string source is fetched in
the background like `samples(...)`, so the first cycles after a fresh
`midimaps(url)` send no mapped CCs (upstream `await`s the fetch instead).

### Gamepad (`@strudel/gamepad`) — intentionally unsupported (no native input source yet)

`@strudel/gamepad` (`gamepad`, `buttonMap`, `getGamepadStates`,
`clearGamepadStates`) reads controllers through the browser
[Gamepad API](https://developer.mozilla.org/docs/Web/API/Gamepad_API) and
exposes axes/buttons as patternable signals. Rudel has no gamepad input source
wired into the engine, so this is **currently unsupported** and patterns
referencing `gamepad` have no input. Unlike the strictly browser-only packages,
a native port is technically feasible (e.g. via a Rust controller crate such as
`gilrs`) — the input bus that already carries the pointer, keyboard and MIDI CC
would be its home — but it needs a new dependency and a polling thread for a
surface of ~60 names (16 buttons in four spellings each, plus axes, toggles and
the button-sequence detector), so it is not implemented and not yet planned.

### Serial and MQTT (`@strudel/serial`, `@strudel/mqtt`) — intentionally unsupported

`@strudel/serial` (`Pattern.prototype.serial`, `getWriter`) writes hap values to
a serial device through the browser
[Web Serial API](https://developer.mozilla.org/docs/Web/API/Web_Serial_API),
and `@strudel/mqtt` (`Pattern.prototype.mqtt`) publishes hap values to an MQTT
broker over WebSockets. Both are browser-platform output bridges. Rudel does not
implement either, so `.serial(...)` and `.mqtt(...)` are **intentionally
unsupported** and have no effect.

For getting events out of Rudel to other hardware/software, the supported,
native output paths are **MIDI** (`crates/rudel-midi`) and **SuperDirt-compatible
OSC over UDP** (`crates/rudel-osc`) — selectable in the app's output picker.
These cover the common "drive external gear / another program" use cases without
needing the Web Serial or MQTT-over-WebSocket bridges.

### Csound (`@strudel/csound`) — supported, with Csound installed separately

`@strudel/csound` (`loadCsound`/`loadCSound`, `loadOrc`, and the `csound` /
`csoundm` outputs) routes haps to [Csound](https://csound.com/) instruments as
an alternative sound engine. Rudel supports all of it, against the *native*
Csound rather than the browser's WebAssembly build.

There is one condition: **Csound has to be installed on the machine.** There is
no pure-Rust Csound, and the WebAssembly build upstream uses is Emscripten
output that needs a browser or Node to run at all. Rudel therefore opens
`libcsound` by name at run time, the first time a script calls `loadCsound` or
`loadOrc`:

| Platform | What it looks for |
| --- | --- |
| Windows | `csound64.dll` on `PATH`, then `C:\Program Files\Csound6_x64\bin\csound64.dll` |
| macOS | `libcsound64.dylib`, then `CsoundLib64.framework`, Homebrew and `/usr/local/lib` |
| Linux | `libcsound64.so`, then `/usr/lib` and `/usr/local/lib` |

Set `RUDEL_CSOUND_LIB` to the library's full path to override the search. That
is all an unpacked build (Csound's `…-windows-x64-binaries.zip`, say) needs —
including for orchestras as large as `livecode.orc`. Set `OPCODE6DIR64` to the
same directory as well if an orchestra reaches for a *plugin* opcode, which the
installers register and an unpacked copy does not.

Csound is **not** a build dependency: nothing links against it, and a Rudel
built and run on a machine without it behaves exactly as before. Only a script
that asks for Csound loads the library, and if it is not there the error names
what to install and the rest of the pattern still plays.

Csound renders inside the audio callback with host-implemented audio IO, so its
output is a signal in Rudel's mixer — one device, one clock, onsets
sample-accurate against every other layer — rather than a second output stream.
It sums into the master after the orbits, so it is not fed by `room`, `delay`
or the DJ filter; upstream wires it straight to the audio context's destination,
which has the same effect. Master volume still applies.

## Alternative language front-ends

### Tidal, Mondo, Mondough (`@strudel/tidal`, `@strudel/mondo`, `@strudel/mondough`) — intentionally unsupported

These packages provide *alternative source languages* that compile down to the
same Strudel pattern engine:

- `@strudel/tidal` (`initTidal`, `tidal`) — an experimental interpreter for
  Haskell-flavoured TidalCycles code.
- `@strudel/mondo` (`mondo`, `mondolang`) — a small Lisp-like functional
  composition language that translates to JS.
- `@strudel/mondough` (`mondo`, `mondi`, `mondolang`) — the Mondo notation
  wired into Strudel.

Rudel's authoring surface is **Koto** (its scripting layer) plus Strudel-style
**mini-notation** (`crates/rudel-mini`). These provide the same underlying
pattern engine through a different, native front-end. Porting additional parser
front-ends (Tidal/Haskell, Mondo Lisp) is **intentionally out of scope**:
they are parallel language choices, not additional musical capability, and the
Koto + mini-notation combination is Rudel's deliberate single authoring surface.
There is no native equivalent surface for these alternative languages.

## Web embedding

### `@strudel/web` and `@strudel/embed` — intentionally unsupported (no equivalent surface)

`@strudel/web` is an opinionated browser bundle of Strudel, and
`@strudel/embed` is an embeddable Web Component (`<strudel-editor>`) that loads a
Strudel REPL into an `<iframe>` on any web page. Both exist purely to put the
Strudel REPL on the web: they assume a DOM, a `<script>`/custom-element host
page, and an iframe sandbox.

Rudel is a **native desktop application** (`crates/rudel-app`, an `egui`
binary). It has no web page, no DOM, no custom element, and no iframe — so there
is no equivalent user-visible surface to match. Embedding Rudel in a web page is
**intentionally unsupported**; embed Strudel's web build if you need an
in-browser/iframe REPL. (Rudel can be run as a normal desktop process and driven
via its native output paths — MIDI/OSC — but it is not a web-embeddable
component.)
