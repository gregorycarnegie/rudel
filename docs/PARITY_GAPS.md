# Remaining parity gaps

What still separates rudel from Strudel once every exported name is accounted
for ([API_INVENTORY.md](API_INVENTORY.md): 709 names, none unaccounted) and
FULL_STRUDEL.md has no partial items left. The device-motion signals are out of
scope (no accelerometer to read on a desktop; see
[UNSUPPORTED.md](UNSUPPORTED.md)), and so are the names that exist only in
Strudel's docs.

## To do

These are all possible natively. None of them depends on the browser.

- [x] **Gamepad** (`@strudel/gamepad`): buttons, axes and the button-sequence
      helpers, read from a native gamepad library on Windows, macOS and Linux.
- [x] **Serial output** (`@strudel/serial`): send haps to a serial port, as the
      Web Serial version does.
- [x] **MQTT** (`@strudel/mqtt`): publish haps to an MQTT broker.
- [ ] **`registerVoicings`**: register a voicing dictionary by name, as
      `addVoicings` already can.
- [ ] **`worklet("…")`**: the string form of a kabelsalat graph, evaluated at
      play time. `K(...)` works.
- [ ] **kabelsalat live-input nodes**: the parts of a `K(...)` graph that read
      live input.
- [ ] **`FX` stage sends**: a stage's own `delay`/`room` sends. Today a stage
      uses the main controls' delay and reverb.
- [ ] **`stretch` pre-roll**: superdough starts a stretched voice 0.04 s early
      to cover the phase vocoder's latency; rudel starts it on time, so it
      sounds late.
- [ ] **The last vendored tune**: one of the 27 tunes in Strudel's test
      snapshot still doesn't match its haps exactly.

## Stays different

By design, or because of the platform. Each is described in
[UNSUPPORTED.md](UNSUPPORTED.md).

- **Reverb**: the impulse response is seeded (upstream re-randomises it on
  every rebuild), and the wet signal returns one partition (about 23 ms)
  late, which is inherent to a uniform-partitioned convolver.
- **Timing**: `log`/`onTriggerTime` fire from the frame loop, and MIDI timing
  differs slightly.
- **Soundfonts**: General MIDI sounds are fetched over the network.
- **Smaller differences**: `setMaxPolyphony` fades voices out with a
  different shape, `spiral` is drawn as a distance field rather than
  strokes, and `shader` takes raw WGSL with no chain.
- **Not applicable**: the Tidal front-end (`@strudel/tidal`), and
  `@strudel/web`/`@strudel/embed`, which embed Strudel in web pages.
