# Remaining parity gaps

What still separates rudel from Strudel once every exported name is accounted
for ([API_INVENTORY.md](API_INVENTORY.md): 709 names, none unaccounted) and
FULL_STRUDEL.md has no partial items left. The device-motion signals are out of
scope (no accelerometer to read on a desktop; see
[UNSUPPORTED.md](UNSUPPORTED.md)), and so are the names that exist only in
Strudel's docs.

## Done

Each was possible natively, and each is now in. Details are in
[UNSUPPORTED.md](UNSUPPORTED.md) and the changelog.

- [x] **Gamepad** (`@strudel/gamepad`): `gamepad(index)`, `buttonMap`,
      `getGamepadStates`/`clearGamepadStates`, read through gilrs.
- [x] **Serial output** (`@strudel/serial`): `.serial()`, through the
      serialport crate.
- [x] **MQTT** (`@strudel/mqtt`): `.mqtt()`, MQTT 3.1.1 over WebSockets.
- [x] **`registerVoicings`**: registers a dictionary as `addVoicings` does.
- [x] **`worklet("…")`**: the text form of a `K(...)` graph. Along the way,
      `K(() => { ... })` was fixed: it built an empty graph.
- [x] **kabelsalat live-input nodes**: they hold their initial values, as in
      Strudel, whose worklet never feeds them.
- [x] **`FX` stage sends**: a stage has its own inline delay and reverb.
- [x] **`stretch` pre-roll**: a stretched hap starts 0.04 s early.
- [x] **The last vendored tune**: `juxUndTollerei` matches, now that
      `every`/`firstOf`/`lastOf` are empty before cycle 0 where Strudel's are.

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
- **No browser prompts**: `.serial()` has no port picker (a `name` that is a
  port picks it, else the first USB serial port), and `.mqtt()` has no
  password prompt.
- **A hop of stretch latency**: a stretched voice is 128 samples (under 3 ms)
  later than upstream's, the buffering of rudel's per-sample chain.
- **kabelsalat `scope`/`split`** pass their input through; upstream they throw
  inside the worklet and the graph falls silent.
- **Two tunes differ in representation only**: `csoundDemo` and
  `loungeSponge` carry `csound` as a control where upstream hangs it off the
  hap's context, and `loungeSponge` keeps `n` as a MIDI number where upstream
  keeps the note name. Same sound.
- **Not applicable**: the Tidal front-end (`@strudel/tidal`), and
  `@strudel/web`/`@strudel/embed`, which embed Strudel in web pages.
