# rudel-app

Native live-coding editor for Rudel.

`rudel-app` is an `egui` desktop app that evaluates Koto scripts and routes the
resulting pattern to audio, MIDI, or OSC output.

## Run

```bash
cargo run --release -p rudel-app
```

Release mode is recommended for smoother real-time audio.

## Features

- Multiline Koto editor with Ctrl+Enter evaluation.
- Play/stop transport and cycles-per-second slider.
- Audio, MIDI, and OSC output selector.
- Lazy MIDI/OSC connection with graceful fallback to audio on connection errors.
- Sample-folder loading into the audio engine.
- Reference panel for built-in synths, drums, loaded samples, controls, signals,
  and factories.
- One-cycle visualizer with a live playhead and per-orbit bands.

## Output Notes

- Audio uses `rudel-audio` and the default `cpal` output device.
- MIDI uses the first available output port by default, or a case-insensitive
  substring typed in the port field.
- OSC defaults to `127.0.0.1:57120`, the standard local SuperDirt port.

## Tests

```bash
cargo test -p rudel-app
```

The app crate is primarily integration glue; most behavior is tested in the
engine, language, audio, MIDI, and OSC crates.
