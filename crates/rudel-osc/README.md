# rudel-osc

SuperDirt-compatible OSC output for Rudel.

`rudel-osc` encodes Rudel control events as OSC 1.0 UDP messages for
`/dirt/play`, the SuperDirt/Tidal playback endpoint.

## Public Surface

- `OscArg` and `OscMessage`: minimal OSC message model and encoder.
- `superdirt_message`: converts a control map into a `/dirt/play` message.
- `schedule_window`: emits sorted `TimedOsc` packets for a cycle window.
- `OscOut`: UDP sender.
- `OscEngine`: real-time lookahead scheduler.
- `DIRT_PLAY` and `SUPERDIRT_PORT`: default SuperDirt address and port.

## Message Behavior

Messages include `cps`, `cycle`, and `delta`, preserve supported control values,
derive `midinote` from `note`, and adjust `unit: "c"` sample speeds for
SuperDirt's own cycle-speed handling.

## Example

```rust
let out = rudel_osc::OscOut::connect("127.0.0.1:57120")?;
let pat = rudel_core::s(rudel_core::seq(["bd", "sd"]));
let engine = rudel_osc::OscEngine::start(out, pat, 0.5);
```

## Tests

```bash
cargo test -p rudel-osc
```

The tests include OSC encoding checks and UDP loopback coverage.
