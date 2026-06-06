# rudel-audio

Real-time audio engine for Rudel.

`rudel-audio` connects pure Rudel patterns to a `cpal` output stream. It owns the
lookahead scheduler, sample bank loading, mixer, delay send, and `fundsp` reverb.

## Public Surface

- `Engine`: starts the default output device, schedules patterns, and exposes
  `set_pattern`, `set_cps`, `load_samples`, `register_sample`, `sample_names`,
  and `position_cycles`.
- `collect_events`: pure pattern-to-note-event conversion used by the scheduler.
- `NoteEvent`: timestamped voice event sent to the mixer.
- `SampleBank`: load and index decoded samples by sound name.

## Sample Folders

`Engine::load_samples` expects a directory whose immediate subdirectories are
sound names:

```text
samples/
  bd/
    000.wav
    001.wav
  hh/
    closed.wav
```

Use `s("bd")` for the first sample and `s("bd:1")` or `s("bd").n(1)` for later
indices. Indices wrap.

## Examples

```bash
cargo run -p rudel-audio --example play
cargo run -p rudel-audio --example live -- 'note("c e g").fast(2).room(0.4)'
cargo run -p rudel-audio --example samples -- path/to/samples
```

## Tests

```bash
cargo test -p rudel-audio
```

Most tests drive the scheduler and mixer without requiring an audio device.
