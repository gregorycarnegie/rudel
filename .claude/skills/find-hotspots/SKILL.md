---
name: find-hotspots
description: Profile rudel with samply and rank the hottest functions by CPU time. Use when asked where the time goes, what is slow, to find or confirm a performance hotspot, to check whether an optimisation actually helped, or when a change needs a before/after CPU comparison rather than a guess.
---

# Finding hotspots

`samply` records a sampling profile; `hotspots.mjs` turns it into a ranked list
of function names. The list is the point — samply's own output is a browser UI
and a `profile.json.gz` full of raw hex addresses.

Paths below are relative to the repo root. Recording the app needs a real
interactive desktop session (see [run-rudel](../run-rudel/SKILL.md)).

## 1. Build with symbols

```powershell
cargo build --profile profiling -p rudel-app
```

`[profile.profiling]` is release + `debug = 1`, so frames resolve to names
without the app being sluggish. The binary is `target/profiling/rudel.exe` —
**not** `rudel-app.exe`; `rudel-app` declares `[[bin]] name = "rudel"`.

## 2. Record

The app, driven while it records — start samply in the background, drive the
app with the run-rudel driver, then quit it to stop the recording:

```powershell
Start-Process samply -ArgumentList 'record','--save-only','-o','hot.json.gz','./target/profiling/rudel.exe'
pwsh -File .claude/skills/run-rudel/driver.ps1 -Play -Eval 's("bd*8").jux(rev).room(0.6)' -Wait 20
pwsh -File .claude/skills/run-rudel/driver.ps1 -Quit
```

Profile the thing you actually suspect. A pattern that does not exercise the
suspect path produces a profile of egui repainting itself.

Anything else that runs to completion works the same way — a test binary, an
example, a corpus run:

```powershell
samply record --save-only -o hot.json.gz -- ./target/profiling/deps/some_bench-abc123.exe
```

## 3. Rank

```bash
node .claude/skills/find-hotspots/hotspots.mjs hot.json.gz --module rudel.exe --top 20
```

```
hot.json.gz — 66.1 s of CPU across 190 threads

CPU by thread
    48552 ms  73.5%  rudel.exe (49620)      <- the UI thread
     7344 ms  11.1%  cpal_wasapi_out (82652) <- the audio callback

Self time — top 20 in rudel.exe
      172 ms   2.3%  egui::context::Context::check_for_id_clash()  (context.rs:1163)
      169 ms   2.3%  egui::response::Response::widget_info<..>()  (response.rs:877)
      ...
```

- **Self time** is where the CPU actually is — start here.
- **Total (inclusive) time** says which caller owns it. Near-100% entries at
  the top are just `main` and the thread starts.
- `--thread rudel.exe` restricts everything to one thread (the UI thread here;
  `cpal_wasapi_out` is the audio callback). `--module` filters by binary —
  without it the top of the list is `ntdll` wait stubs.
- Weights are CPU microseconds, not sample counts: the profile is mostly idle
  samples parked in `NtWaitForMultipleObjects`, and counting them would rank
  the parked threads first.

To compare before and after, record twice under the same driver script and
diff the two self-time lists. Absolute ms only compares across runs of equal
length; percentages are safer.

## Gotchas

- **Symbolicate before rebuilding.** samply stores raw addresses and resolves
  them lazily against the PDB. Rebuild the binary and the build id no longer
  matches, so every name in an older profile falls back to `17:18439557`
  forever. If that is what you see, the profile is stale — re-record.
- **System DLLs stay unresolved** unless you pass a symbol server:
  `--windows-symbol-server https://msdl.microsoft.com/download/symbols` on
  `samply load`/`record`. Rarely worth it; `--module rudel.exe` is the answer
  to `ntdll.dll` noise.
- **`hotspots.mjs` starts `samply load` on port 3579** to reach its
  `/symbolicate/v5` endpoint, and kills it on the way out. `--port` if that
  clashes. It never opens a browser. It also passes `--symbol-dir` for every
  directory the profile loaded a module from — the profile names each PDB by
  bare filename, so without that samply looks in the cwd and resolves nothing.
- **Inlined callees are attributed to the caller.** A "missing" function in a
  release build was almost certainly inlined; look for its caller.
- **The UI thread is usually the bottleneck, not the DSP.** In the run above,
  `rudel.exe` (UI) burned 80% of the CPU and `cpal_wasapi_out` (audio) 9%, and
  the whole self-time top ten was egui widget bookkeeping. Confirm which thread
  owns the time before optimising anything.

## Human path

```powershell
samply record ./target/profiling/rudel.exe
```

Opens the Firefox Profiler UI with a flame graph, which is better than any
text list for *shape* — useless to an agent, which cannot read it.
