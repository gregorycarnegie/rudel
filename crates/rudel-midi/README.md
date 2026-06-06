# rudel-midi

MIDI output for Rudel.

`rudel-midi` turns `rudel-core` control events into MIDI note, control-change,
and program-change messages, with helper methods for transport and clock
messages. It depends only on `rudel-core` and `midir`.

## Public Surface

- `control_to_midi`: converts a control map into a `MidiNote`.
- `schedule_window`: emits sorted `TimedMidi` messages for a cycle window.
- `MidiOut`: lists ports, connects to an output port, and sends raw bytes.
- `MidiEngine`: real-time lookahead scheduler that drives any `MidiSink`.
- `MidiSink`: trait used for both real output and test sinks.

## Controls

- Pitch from `note` or `n`; numbers are MIDI notes and note names such as `a4`
  are resolved through `rudel-core`.
- Velocity from `velocity`, then `gain`, defaulting to `0.9`.
- Channel from `midichan` or `channel`, using 1-based input.
- CC from `ccn` and `ccv`.
- Program change from `progNum`.

## Example

```rust
let out = rudel_midi::MidiOut::connect(None)?;
let pat = rudel_core::note(rudel_core::seq([60, 64, 67]));
let engine = rudel_midi::MidiEngine::start(out, pat, 0.5);
```

## Tests

```bash
cargo test -p rudel-midi
```
