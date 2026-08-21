# Unsupported and intentionally different features

Rudel is a **native Rust** application. Strudel is a **browser** application. A
number of Strudel packages exist only to bridge to browser/web-platform APIs
(WebGL, DeviceMotion, Web Serial, the Gamepad API, MQTT-over-WebSockets, the
Csound WASM build, web components / iframes) or to provide alternative language
front-ends (Tidal). Rudel deliberately does not port these; this page is
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
full-screen draw context. (The `_shader` and `_hydra` widgets below are not
exceptions: the user supplies WGSL, or a hydra chain that compiles to WGSL, and
it runs on the GPU — never a Koto callback on the query path.) By design the Koto VM is never invoked from the
real-time/draw query path, so a pattern cannot register a Koto closure that runs
every animation frame. Only the built-in inline visualisers are available. The
full-screen draw context, `Framer`/`Drawer` rolling visible-hap *memory*,
lookbehind/lookahead window bookkeeping, future-hap invalidation, and the
`cleanupDraw`/`cleanupDrawContext` lifecycle are not ported; the inline widget
host re-queries the pattern each frame instead of keeping painter-side hap
memory.

### `spiral` — drawn as an SDF, not as strokes

Strudel's `spiral` strokes one canvas polyline per hap. Adjacent haps then meet
butt-end to butt-end with ends that are not parallel, so one side of every
boundary overlaps — and because hap colours are translucent under the default
`fade`, that overlap composites twice and reads as a bright radial seam — while
the other side leaves a sliver of background. The seam is inherent to stroking
and does not go away at a finer sampling rate.

Rudel evaluates the same bands per pixel instead
(`crates/rudel-app/src/editor/widgets/spiral_gpu.rs`): a pixel's polar
coordinates invert straight back to a spiral angle, so coverage is exact and the
only soft edge is one pixel of deliberate anti-aliasing. This is the **default**
— the one place Rudel's spiral deliberately looks better than upstream's rather
than the same.

Both painters take their bands from the same `spiral_bands`, so colour, fade and
geometry are identical and only the seams differ: rendering a pattern each way
gives the same colour histogram, with more band pixels and no seam pixels on the
GPU side. `spiral({gpu: false})` asks for the tessellated painter back, and it
is also used automatically when the wgpu backend is not running, since the SDF
painter's pipeline and buffers live in that renderer.

**The default surface is 400×400, not upstream's 275×275.** Strudel's `_spiral`
widget registration (`packages/codemirror/widget.mjs`) takes `size || 275` for
the canvas and passes `size / 5` as the spiral's radius unit, so at the default
`inset: 3` the "now" arc lands at radius `3 × 55 = 165` — outside the `137.5`
a 275 canvas inscribes. Upstream therefore clips the current position, the one
thing a spiral most needs to show, into the canvas corners.

Rudel widens the surface instead of touching the geometry. `spiral_size` still
defaults to `55` and `inset` keeps its documented `3`, so a pattern copied from
Strudel draws the same spiral at the same radii; only the canvas it is drawn on
is bigger, and the current position fits on it. Naming `size` explicitly still
sets both, exactly as upstream does — `spiral({size: 275})` reproduces the
upstream framing, corner-clipped "now" included.

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

### Hydra (`@strudel/hydra`) — the chain DSL is ported; the runtime is not

`@strudel/hydra` (`initHydra`, `H`, `clearHydra`) is a ~50-line loader: it makes
a canvas and `await import`s [hydra-synth](https://hydra.ojack.xyz/) from a CDN
at runtime. There is no hydra source in the vendored Strudel tree to port
against, so the reference here is **hydra-synth 1.4.0**, pinned into
`tools/oracle/hydra_golden.json` by `tools/oracle/gen_hydra_oracle.mjs`.
`crates/rudel-lang/tests/hydra_parity.rs` holds the port to it.

That URL carries no version, and `package.json` asks for `^1.3.29`, so what a
Strudel user actually runs is whatever npm calls latest that day — the
reference is a moving target by construction. Pinning it makes the movement
reviewable rather than silent, and it is not hypothetical: 1.4.0 changed
`shift` from `c2.r = fract(c2.r + r)`, which wrapped the result into `[0,1)`,
to `c2.r += fract(r)`, which does not. Rudel follows 1.4.0.

**What works.** 51 of hydra's 52 functions, as a chain that compiles to WGSL and
renders in an inline widget:

```koto
s("bd*4").hydra({ chain: Hydra.osc(20, 0.1, 0.8).kaleid(5).colorama(0.02) })
```

The chain is folded into a shader once per evaluation — hydra's own five-way
composition rule from `generate-glsl.js`, ported — so the Koto VM never runs on
the draw path. Every function's WGSL is checked by `naga` in a test, and every
signature (name, composition type, input names, input defaults) is compared
against the pinned table, so a chain written for hydra means the same thing
here.

**Sources live on `Hydra`, not on the globals.** Upstream puts `osc`, `noise`
and `shape` in global scope, which is why `clearHydra` has to put `shape` and
`speed` *back* afterwards. Rudel already has all three — `osc` is the OSC
output, `noise` and `shape` are core — so taking them would break patterns that
never mentioned hydra. They are `Hydra.osc(…)`, `Hydra.noise(…)` and so on,
alongside `Math` and `Object`. Chained methods keep hydra's own spelling.

**What is missing.** Everything that needs render-to-texture, which is the whole
output-buffer half of hydra:

**Four output buffers, and feedback.** A hydra widget owns `o0`–`o3`. A chain
is bound to one by the option it is written under, `src` reads any of them, and
`render` picks which is displayed:

```koto
s("bd*4").hydra({
  o0: Hydra.osc(15, 0.1, 0.7).kaleid(4),
  o1: Hydra.voronoi(10, 0.4).thresh(0.45, 0.1),
  o2: Hydra.src(Hydra.o0).modulate(Hydra.src(Hydra.o1), 0.35).colorama(0.01),
  render: 2,
})
```

`chain` is `o0` under its single-output name, so the one-chain form is
unchanged, and `render` defaults to `0`.

Each output alternates between two textures, so a chain writes one while every
buffer read comes from the set written last frame — its own via `prev()`,
another output's via `src(o1)`. That is upstream's rule too:
`format-arguments.js` binds `output.getTexture()`, the fbo that is *not*
currently being drawn into, which is why `src(o0)` inside o0's own chain is
`prev()` there as well. It also means no texture is ever sampled while it is a
render target, which wgpu rejects outright.

Only outputs a script gave a chain are allocated; the rest bind a shared 1×1
texture and read as the empty buffers they are
(`crates/rudel-app/src/editor/widgets/hydra_gpu.rs`).

`render: 'all'` tiles all four instead, which is hydra's `render()` with no
argument — same column-major order (o0 top-left, o1 bottom-left, o2 top-right,
o3 bottom-right), ported from its `renderAll` shader.

**The loader is accepted and ignored.** `initHydra(…)` fetches hydra-synth from
a CDN upstream and `clearHydra()` tears its canvas down; there is nothing here
to fetch or tear down, so both are no-ops and a pattern copied from Strudel
still runs.

**What is missing.**

| Not ported | Why |
| --- | --- |
| `sum` | broken upstream, not a porting decision. Hydra generates `vec4 sum(vec4 _c0, vec4 scale)` from a body that reads an undefined `s` and returns a float from a `vec4` function — two hard GLSL errors. Hydra only emits a function's GLSL when a chain uses it, so this is latent rather than fatal: hydra runs fine until something calls `.sum()`, and then that output's shader fails to compile there too |
| `H(pattern)` | samples a pattern once per animation frame to drive a uniform. A chain here compiles once per evaluation, so its parameters are constants for that evaluation's life. Accepted and ignored, with a line in the console |
| `s0`–`s3` | webcam, video, screen capture |

`hydra::UNIMPLEMENTED` carries the list in code, and the parity test fails if
hydra grows a function that is neither implemented nor listed there.

### `shader` — raw WGSL, no chain

Separate from hydra and lower level: the `_shader` widget (`shader`) paints a user-written WGSL fragment body into an
inline widget through the wgpu render callback egui already runs on
(`crates/rudel-app/src/editor/widgets/shader.rs`). The body is wrapped in a
fixed prelude giving it `uv` (0..1 across the widget, y down) and a `u` uniform
block carrying `res`, `time` (cycles), `gain`, `note` and `voices` from the
pattern's sounding events:

```koto
s("bd*4").shader({ code: '
  let d = length(uv - vec2<f32>(0.5, 0.5));
  return vec4<f32>(u.gain * (1.0 - d), 0.1, d, 1.0);
' })
```

Write the body in **single quotes** — double-quoted strings are mini-notation.
The WGSL is parsed and validated by `naga` before it reaches wgpu, so a typo
draws its message in the widget rather than aborting the process; the reported
line number counts the body, not the prelude.

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

### `setMaxPolyphony` — supported, with a different fade shape

`setMaxPolyphony(n)` caps how many voices sound at once. Past the cap, Rudel
fades the oldest ones out — first in, first out — over a quarter of a second,
which is what superdough's `linearRampToValueAtTime(0, t + 0.25)` does to the
oldest entries of its `activeSoundSources` map. A voice already fading no
longer counts against the cap, mirroring upstream deleting it from that map as
it starts the ramp.

The one difference is the fade itself: Rudel's is linear in amplitude and
applied per sample by the mixer, upstream's is a Web Audio gain ramp. The
voice is also dropped when it reaches silence rather than left parked at zero
gain, so `active_len()` falls to the cap instead of staying above it.

### `speak` (`core/speak.mjs`) — supported, against the platform synthesiser

Upstream `.speak(lang, voice)` is an `onTrigger` over the browser's Web Speech
API: it filters `speechSynthesis.getVoices()` by language tag, picks one by
index or name, and utters `hap.value`. Rudel does the same thing with the
operating system's synthesiser, split across two halves because its query path
is `Send`/`Sync` and cannot carry the closure:

- `.speak(lang, voice)` (`crates/rudel-core/src/speak.rs`) marks each hap with
  the words, the language and the voice as controls. Both arguments are
  patternable and both may be `null` for the system default, as upstream's are.
- `crates/rudel-app/src/speech.rs` says them, from the same per-frame sweep the
  `onTriggerTime` hooks run on — so the frame rate bounds the timing, exactly as
  for those hooks, and the audio callback never waits on an OS call.

A spoken hap makes no sound of its own, matching upstream's *dominant*
`onTrigger`: it replaces the sound rather than adding to it.

| Platform | Synthesiser | Difference |
| --- | --- | --- |
| Windows | SAPI, via the `windows` crate cpal already pulls in | none |
| macOS | `say` | |
| Linux | `spd-say` (speech-dispatcher), if installed | |

On macOS and Linux the voice list comes from `say -v '?'` / `spd-say -L`, so
language filtering and selection by index or name work the same way; a machine
with no synthesiser installed reports that once rather than on every hap.

### Voicing dictionaries — `addVoicings` yes, `registerVoicings` no

`addVoicings(name, dictionary, range)` registers a chord dictionary at run time
and is supported: a name registered this way shadows a built-in one, as
upstream's `Object.assign` onto `voicingRegistry` does. Its `range` argument is
accepted and ignored, for the same reason `setVoicingRange` is a no-op — `range`
reaches only the deprecated `.voicings(dict)` voice-leading path, which Rudel
aliases to `voicing`.

`registerVoicings(name, dictionary, options)` — the newer call signature — is
not exposed. Its `options` carry `mode` and `anchor`, and both are dead for the
`voicing` path in Strudel itself: `voicing` spreads the value's `undefined`
`anchor`/`mode` controls *over* the registry entry, so they always fall back to
`renderVoicing`'s `c5`/`below` defaults.

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

The installed Csound has to be recent enough for the orchestra, and the desktop
releases lag the WebAssembly ones. `@strudel/csound` pins `@csound/browser`
6.18.7, which parses the parenthesised opcode syntax (`opcode set_tempo(itempo):
void`); the 6.18.0 desktop installer does not, and rejects `livecode.orc` — the
orchestra behind the "Lounge sponge" tune — at its first `opcode` line with
`syntax error, unexpected T_IDENT, expecting ','`. Csound 6.19 or newer parses
it. Nothing on Rudel's side can substitute for the parser, so the compile error
is reported as Csound wrote it and the rest of the pattern keeps playing.

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

### Mondo (`@strudel/mondo`, `@strudel/mondough`) — supported

[Mondo Notation](https://strudel.cc/learn/mondo-notation/) is available, two
ways:

- **A whole document**, which is how upstream's examples are written — put
  `// mondo` on the first line and the rest of the script is read as mondo.
  (Upstream types those into a REPL switched to mondo mode; the marker line
  stands in for that mode here.) A script that is mondo *without* the marker
  fails to compile as Koto, and the error says so rather than pointing at the
  first `$`.
- **One pattern inside a Koto script**, the surface upstream's library exposes:
  a tagged template, `` mondo`s hh*8` `` (or `mondolang`, or `mondi` for a
  bracketed sequence).

It is compiled to Koto by
`crates/rudel-lang/src/preprocess/mondo.rs`, which ports upstream's parser and
plays the role of `mondough.mjs`'s evaluator, so every control, transform and
signal Rudel exposes is reachable from mondo without a second dispatch table.

Function calls, `#` chaining, `#`-lambdas, all four bracket kinds, the infix
operators (`* / ! @ % ? & : ..`), `,`/`$` stacks, `|` choices, strings, comments
and `def` all work. Two things do not:

- **`:` and `..` need literal operands.** `bd:3`, `C4:minor` and `0..7` compile
  through mini-notation, which builds exactly the same values; `bd:<0 1>` does
  not. Use `# euclid <3 6> <8>` for the patterned euclid case.
- **`def` binds values, not functions.** `(def melody [0 1 2])` works;
  `(def (f x) …)` does not. Upstream's runner is a full Lisp — `let`, `match`,
  `if`, recursion, `cons`/`car`/`cdr` — but that half of it is exercised only by
  its own arithmetic test suite, never by the pattern language, so it is not
  ported.

Source locations are not mapped through the compiler, so mini-notation
highlighting is off by the length of the rewrite in a script that mixes mondo
with ordinary patterns.

### Tidal (`@strudel/tidal`) — intentionally unsupported

`@strudel/tidal` (`initTidal`, `tidal`) is an experimental interpreter for
Haskell-flavoured TidalCycles code — a third *source language* over the same
pattern engine. Unlike Mondo, it is a different language rather than a different
notation for the one Rudel already has, and it is experimental upstream. Rudel's
authoring surface stays **Koto** plus Strudel-style **mini-notation**
(`crates/rudel-mini`), with Mondo as the one alternative notation.

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
