# rudel-app

Native live-coding editor for Rudel.

`rudel-app` is an `egui` desktop app that evaluates Strudel JavaScript and routes the
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
| `Ctrl+Enter` / `Alt+Enter` | Evaluate the editor contents, or the current block when `block eval` is enabled |
| `Ctrl+Shift+Enter` | Evaluate the blank-line-delimited block at the cursor, or the full editor when `block eval` is enabled |
| `Ctrl+.` / `Alt+.` | Hush (stop playback, keep the pattern) |
| `Ctrl+Shift+.` | Panic / reset (stop and flush stuck MIDI notes) |
| `Ctrl+/` / `Ctrl+\` | Toggle `//` comments on the line or selection |
| `Tab` / `Shift+Tab` | Indent / outdent the line or selection |
| `Alt+w` / `Alt+q` | Jump the cursor to the next / previous `$` block marker |
| `Tab` / `Enter` | Accept the highlighted autocomplete suggestion (when the popup is open) |
| `↑` / `↓` / `Esc` | Navigate / dismiss the autocomplete popup |

### Keymaps

The `keys` picker in the editor settings swaps the editing keys, as Strudel's
`keybindings` setting does: `codemirror` (the default, the keys above), `vim`,
`emacs`, `vscode` and `helix`. Each is the everyday core of its editor, not all
of it; the transport keys above work in all of them.

- **vim**: normal, insert, visual (`v`) and visual-line (`V`) modes, shown at
  the editor's bottom right. Counts; motions `h j k l w b e W B E 0 ^ $ gg G
  { } % f t F T`; operators `d c y > < gc` with any motion, doubled for the
  line (`dd`, `gcc`), and text objects `iw aw i" a" i( a( i[ i{` and the like;
  `x X D C s S Y p P r J ~ u Ctrl+R i a I A o O`; `/` search with `n N`.
  Strudel's own: `:w` evaluates (as Ctrl+Enter), `:q` hushes, `gc` comments.
  `:12` goes to line 12.
- **helix**: selection first. `w b e` select over words, `x` the line (again
  for the next), `%` everything, `;` collapses; then `d c y p P r ~ > < J`, `v`
  for select mode, `gg ge gh gl gs`, `f t F T`, `u U`, `Ctrl+C` comments, and
  `:w` / `:q` as in vim.
- **emacs**: `C-f C-b C-n C-p C-a C-e M-f M-b M-< M->`, `C-d M-d M-Backspace`,
  `C-k` (repeated kills add up) `C-y`, `C-Space` to set the mark then `C-w` /
  `M-w`, `C-g`, `C-t`, and `C-/` to undo (Ctrl+\ still comments).
- **vscode**: `Alt+↑/↓` move lines, `Shift+Alt+↑/↓` copy them, `Ctrl+Shift+K`
  deletes them, `Ctrl+L` selects the line (again, the next), `Ctrl+D` the word
  and then its next occurrence, `Ctrl+]` / `Ctrl+[` indent, `Ctrl+Shift+\`
  jumps to the matching bracket, `Home` goes to the indentation first, and copy
  or cut with nothing selected takes the line.

The `editor settings` panel mirrors Strudel's CodeMirror compartments for line
wrapping, bracket matching/closing, line numbers, active-line highlighting,
autocomplete, pattern highlighting, flash, tab indentation, block-based eval,
theme, font family, font size, and tooltips. Multi-cursor is visible as a
deferred setting until egui has a matching native selection surface.
Auto-pairing of `()`, `[]`, `{}`, quotes, and backticks, auto-indent after a
newline inside brackets, live bracket-match highlighting around the cursor,
contextual autocomplete, and Ctrl-held reference tooltips match the CodeMirror
REPL where their settings are enabled. The selected editor theme also supplies
the draw colors used by inline visual surfaces, sliders, native
`_pianoroll`/`_pitchwheel`/`_spiral`/`_claviature`/`_scope`/`_spectrum`
widgets, and the native one-cycle visualizer. The editor settings are kept
between sessions.

### Themes

All 39 of Strudel's themes are built in (`themes/*.toml`). Your own go in
rudel's themes folder, one `.toml` file each, and are read at launch; the
`folder` button beside the theme picker opens it (`%APPDATA%\rudel\themes` on
Windows, `~/.local/share/rudel/themes` on Linux,
`~/Library/Application Support/rudel/themes` on macOS). A theme with a
built-in one's name replaces it, and `theme("name")` in a script picks it like
any other. Only `background` and `foreground` are required; every other colour
is worked out from them unless given:

```toml
name = "midnight"          # optional: the file name otherwise
light = false              # whether visualisers draw dark on light
background = "#101020"
foreground = "#e0e0ff"
keyword = "#ff79c6"        # the rest, all optional:
method = "#50fa7b"         # .fast in a method call
string = "#f1fa8c"
number = "#bd93f9"
comment = "#6272a4"
mini_op = "#ff79c6"        # mini-notation's [ ] < > * /
mini_word = "#f1fa8c"
caret = "#f8f8f0"
selection = "#ffffff1a"    # #rrggbbaa for transparency
selection_match = "#ffffff33"
line_highlight = "#ffffff1a"
line_background = "#10102099"
gutter_background = "#101020"
gutter_foreground = "#6272a4"
muted = "#e0e0ff50"
```

A file that does not read is named, with what is wrong, under the editor.

## Features

- Multiline JavaScript editor with full-buffer and current-block evaluation.
- CodeMirror-style editor settings with Strudel-compatible defaults.
- Play/stop transport and cycles-per-second slider.
- Audio, MIDI, and OSC output selector.
- Lazy MIDI/OSC connection with graceful fallback to audio on connection errors.
- Sample-folder loading into the audio engine.
- Syntax highlighting with mini-notation awareness inside string literals.
- Reference panel for built-in synths, drums, loaded samples, controls, signals,
  and factories.
- Inline Strudel-style visual widgets for pianoroll/punchcard/wordfall,
  pitchwheel, spiral, and claviature patterns, including common static
  size/draw options.
- Scope/tscope (triggered oscilloscope with smear), fscope (frequency bars),
  and spectrum (scrolling spectrogram) widgets, each fed by its own lock-free
  per-widget tap on the live audio output.
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
