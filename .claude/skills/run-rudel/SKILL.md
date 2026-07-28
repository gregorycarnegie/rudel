---
name: run-rudel
description: Build, launch, drive and screenshot the rudel live-coding app (native egui desktop window on Windows). Use when asked to run, start, or screenshot rudel, to see a pattern or an inline widget (pianoroll, spiral, pitchwheel, scope) actually rendering, or to check a UI/DSP change in the real app rather than in tests.
---

# Running rudel

`rudel` is a native Rust live-coding editor — `winit` + `egui` + `cpal`, built
by `crates/rudel-app`. There is no DOM, no accessibility tree, and no CLI flag
that loads a script, so the only way in is the OS: put source on the clipboard,
paste it into the editor, and click the toolbar with synthetic mouse input.

`.claude/skills/run-rudel/driver.ps1` does all of that, DPI-correctly. **Use the
driver — do not try to type source into the window yourself.**

Paths below are relative to the repo root. Requires a real interactive desktop
session: the driver moves the actual cursor, so don't use the machine while it
runs.

## Build

```powershell
cargo build --release -p rudel-app
```

Debug builds work but the app is visibly sluggish; release is worth the wait.

## Run (agent path)

One invocation does a whole flow. Switches are applied in this order:
`-Launch`, `-Play`/`-Stop`, `-Eval`, `-Wait`, `-Shot`, `-Quit`.

```powershell
pwsh -File .claude/skills/run-rudel/driver.ps1 -Launch -Play -Eval 's("bd sd hh cp").pianoroll({smear: 1})' -Wait 4 -Shot $env:TEMP\rudel\roll.png
```

Prints one line per step, e.g.:

```
launched 1672x1016 at 342,342 scale 1.5
play
evaluated
saved C:\Users\you\AppData\Local\Temp\rudel\roll.png (1672x1016)
```

The driver creates the output directory if needed. Write screenshots outside
the repo (as above) so they don't dirty the working tree. Then **read the PNG**. A blank or unchanged widget means the evaluation did not
land — see Gotchas.

Re-evaluate against an already-running app (no `-Launch`):

```powershell
pwsh -File .claude/skills/run-rudel/driver.ps1 -Eval 's("bd*4").spiral({cap: "round", thickness: 20, padding: 0.1})' -Wait 3 -Shot $env:TEMP\rudel\spiral.png
```

Shut it down when finished:

```powershell
pwsh -File .claude/skills/run-rudel/driver.ps1 -Quit
```

`-Scale <n>` overrides the auto-detected DPI scale if clicks land in the wrong
place; the driver otherwise reads it from `GetDpiForWindow`.

## Run (human path)

```powershell
cargo run --release -p rudel-app
```

A window opens; type a pattern, `Ctrl+Enter` to evaluate. Useful for a human at
the keyboard, useless to an agent — `Ctrl+Enter` cannot be driven (Gotchas).

## Test

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Adding or renaming any Koto binding fails three drift guards on purpose. Bless
them, then re-run and review the diff:

```powershell
$env:RUDEL_BLESS = "1"
cargo test -p rudel-lang --test reference_snapshot --test api_inventory
$env:RUDEL_BLESS = $null
```

That rewrites `crates/rudel-lang/tests/reference_surface.txt` and
`docs/API_INVENTORY.md`. Both are committed; a surprising diff there means the
exposed surface changed more than intended.

## Gotchas

- **`Ctrl+Enter` never reaches the window.** It is the app's documented eval
  shortcut and works for a human, but synthetic `WScript.Shell.SendKeys`
  `^{ENTER}` is silently swallowed — while `^a` and `^v` in the same editor
  work. The driver clicks the **Eval button** instead.

  This failure is actively misleading: the paste succeeds, so the editor shows
  your new source while the widget keeps rendering the **previous**
  evaluation. That looks exactly like a stale-widget-cleanup bug in the app.
  It isn't. If a widget seems not to update, confirm the evaluation happened
  before believing anything you see.

- **Call `SetProcessDPIAware()` before any screen coordinate.** On a scaled
  display (150% here), a non-DPI-aware PowerShell gets virtualized coordinates
  and `CopyFromScreen` grabs the top-left of the *desktop* instead of the
  window. The driver does this; any ad-hoc capture script you write must too.

- **`FindWindow(null, "rudel")` returns `IntPtr.Zero`.** Get the handle from
  `Get-Process rudel | Where MainWindowHandle -ne 0` instead.

- **Paste source, never type it.** The editor auto-pairs brackets and quotes,
  so `SendKeys`-ing `s("bd*4")` produces mangled code. The driver uses
  `Set-Clipboard` + `Ctrl+V`.

- **`winit` publishes the window handle before laying the window out**, so for
  the first moment `GetWindowRect` reports ~22x22 and any click computed from
  it lands nowhere. The driver waits for width > 400.

- **Widgets render on eval even with the transport stopped**, but nothing
  moves. Anything time-dependent — `smear` accumulating, spiral trails, the
  playhead — needs `-Play` and a `-Wait` of a few seconds.

- **Inline widgets are keyed by source position.** Editing a line shifts the
  widget id, so a widget can legitimately disappear and reappear across edits.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `missing …\target\release\rudel.exe` | `cargo build --release -p rudel-app` |
| `rudel is not running (use -Launch)` | Previous run was `-Quit`ed or crashed; add `-Launch`. |
| Screenshot shows the desktop or another window | DPI awareness (see Gotchas). Use the driver, or `-Scale` if your display scale is unusual. |
| Clicks land on the wrong control | Window still sizing, or an unusual DPI. Add `-Wait 1` after `-Launch`, or pass `-Scale`. |
| Editor text changed but the widget didn't | The evaluation didn't run — see the `Ctrl+Enter` gotcha. |
| `evaluated` printed but the status pill reads an error | The script itself failed; the error text is in the app's status area — screenshot and read it. |
| No sound | Expected under audio-less/remote sessions; the visuals still work. `out` must be `Audio` for the transport to drive them. |
