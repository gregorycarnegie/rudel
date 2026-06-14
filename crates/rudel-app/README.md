# rudel-app

Native live-coding editor for Rudel.

`rudel-app` is an `egui` desktop app that evaluates Koto scripts and routes the
resulting pattern to audio, MIDI, or OSC output.

## Run

```bash
cargo run --release -p rudel-app
```

Release mode is recommended for smoother real-time audio.

## Keyboard shortcuts

Audited against Strudel's `packages/codemirror`. Supported subset:

| Shortcut | Action |
| --- | --- |
| `Ctrl+Enter` / `Alt+Enter` | Evaluate the editor contents |
| `Ctrl+.` / `Alt+.` | Hush (stop playback, keep the pattern) |
| `Ctrl+/` / `Ctrl+\` | Toggle `//` comments on the line or selection |
| `Tab` / `Shift+Tab` | Indent / outdent the line or selection |
| `Alt+w` / `Alt+q` | Jump the cursor to the next / previous `$` block marker |
| `Tab` / `Enter` | Accept the highlighted autocomplete suggestion (when the popup is open) |
| `↑` / `↓` / `Esc` | Navigate / dismiss the autocomplete popup |

Auto-pairing of `()`, `[]`, `{}`, quotes, and backticks, auto-indent after a
newline inside brackets, live bracket-match highlighting around the cursor, and
keyword autocomplete (suggestions generated from the runtime's function /
method / control names) also match the CodeMirror REPL.

Not yet supported (vs Strudel): per-block evaluation.

## Features

- Multiline Koto editor with Ctrl+Enter evaluation and Ctrl+. hush.
- Play/stop transport and cycles-per-second slider.
- Audio, MIDI, and OSC output selector.
- Lazy MIDI/OSC connection with graceful fallback to audio on connection errors.
- Sample-folder loading into the audio engine.
- Syntax highlighting with mini-notation awareness inside string literals.
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
