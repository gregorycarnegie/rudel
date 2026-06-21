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

### `animate` (`@strudel/draw` `animate.mjs`) — intentionally unsupported

`animate` is built directly on the `draw.mjs` runtime: it registers a per-frame
JavaScript painter that draws arbitrary shapes from patterned visual params
(`x`, `y`, `w`, `h`, `angle`, `r`, `fill`, `smear`) onto the global canvas, plus
helpers (`rescale`, `moveXY`, `zoomIn`) and a `smear`/clear toggle, and reports a
"sync mode" status. Because it depends on the arbitrary-callback draw runtime
described above — running user-driven drawing every animation frame — it is
**intentionally unsupported** in Rudel. There is no native equivalent surface;
patterns that call `animate` will not produce visuals. The supported way to get
scheduler-time visuals in Rudel is the inline editor widgets (`_pianoroll`,
`_punchcard`, `_wordfall`, `_pitchwheel`, `_spiral`).
