# Performance

This page compares Rudel 0.22 with Rudel 0.21.0 and with Strudel. 0.21.0 was the last release that ran scripts on Koto rather than JavaScript.

The comparison covers the two jobs that decide whether live coding feels instant:

- **Querying** happens continuously while music plays. The scheduler asks a pattern for its events, about 100 ms ahead of the audio.
- **Evaluating** happens when you press Ctrl+Enter. It turns the script into a pattern.

Sound generation isn't compared. Strudel synthesises through the browser's built-in audio engine (Web Audio), so there is no like-for-like test.

**Measured** on 2026-10-02, on an AMD Ryzen 9 7950X with 64 GB of RAM, running Windows 11:

- **Rudel 0.22 and 0.21.0:** release builds.
- **Strudel:** `@strudel/core` 1.2.6 running in Node 26.

The patterns are the seven cases in [`crates/rudel-lang/benches/patterns.rs`](../crates/rudel-lang/benches/patterns.rs). They mirror Strudel's own `packages/core/bench` and add some typical live-coding patterns.

## Querying 16 cycles

| Pattern | Rudel 0.22 | Rudel 0.21.0 | Strudel |
|---|---:|---:|---:|
| `n("0 .. 63")` | 1.09 ms | 2.91 ms | 7.1 ms |
| `n("0 .. 63").iter(64).fast(64)` (65,536 events) | 120 ms | 320 ms | 656 ms |
| `stack(…)` of 8 patterns | 0.23 ms | 0.49 ms | 1.02 ms |
| `rand.segment(128)` | 0.55 ms | 1.62 ms | 4.67 ms |
| `s("bd(3,8) sd(5,8,2) hh*8").gain(…)` | 0.96 ms | 2.00 ms | 4.01 ms |
| `note(…).fast(2).every(3, x => x.rev())…` | 0.47 ms | 1.04 ms | 2.15 ms |
| `n("0 .. 7").scale("c:major")…` | 0.40 ms | 0.65 ms | 1.75 ms |

Rudel 0.22 queries **4 to 8.5 times faster than Strudel**, and 1.6 to 2.9 times faster than 0.21.0. All three return the same number of events for each pattern.

The speedup over 0.21.0 doesn't come from the change of script engine, because querying never runs it. It comes from doing fraction arithmetic in 64-bit integers whenever the values fit, and from no longer copying each event as it passes through a transform.

## Evaluating

Each figure is the median of 40 evaluations, with a 30 ms pause before each one. The pause gives roughly the conditions of someone typing.

| Pattern | Rudel 0.22 | Rudel 0.21.0 | Strudel |
|---|---:|---:|---:|
| `n("0 .. 63")` | 0.22 ms | 0.76 ms | 0.40 ms |
| `n("0 .. 63").iter(64).fast(64)` | 0.41 ms | 0.87 ms | 0.73 ms |
| `stack(…)` of 8 patterns | 0.43 ms | 1.15 ms | 1.50 ms |
| `rand.segment(128)` | 0.15 ms | 0.96 ms | 0.17 ms |
| `s("bd(3,8) sd(5,8,2) hh*8").gain(…)` | 0.22 ms | 1.00 ms | 0.82 ms |
| `note(…).fast(2).every(3, x => x.rev())…` | 0.55 ms | 0.82 ms | 0.85 ms |
| `n("0 .. 7").scale("c:major")…` | 0.36 ms | 0.69 ms | 0.67 ms |

All three take well under the few milliseconds it would take to notice. Rudel 0.22 is the fastest on every pattern: 1.5 to 6 times faster than 0.21.0, and 1.1 to 3.8 times faster than Strudel.

## Where Rudel 0.22 is slower

**Evaluating back to back.** Building a JavaScript engine with Rudel's roughly 3,000 functions registered takes about 3 ms. Rudel builds the next one in the background after each evaluation, so a person pressing Ctrl+Enter never waits for it. A program that evaluates in a tight loop does wait, every time:

| | Rudel 0.22 | Rudel 0.21.0 | Strudel |
|---|---:|---:|---:|
| back-to-back evaluation | 3.1–4.0 ms | 0.27–0.34 ms | 0.03–1.5 ms |

This only matters for batch work, such as checking every song in a collection. `cargo bench -p rudel-lang` times this case.

**Scripts that do a lot of JavaScript work.** Rudel runs scripts on [Boa](https://boajs.dev), which interprets JavaScript, while V8 in Strudel's browser compiles it to machine code. Pattern code spends little time in JavaScript, so the difference rarely shows. A script that does a lot of plain JavaScript computation can still evaluate several times slower than in Strudel. One song in our test collection, which builds its music from JavaScript helper functions, takes about 35 ms to evaluate, against 3 ms on 0.21.0.

## Caveats

- **Strudel's timings vary.** They moved by about 30% between runs, as Node's V8 compiles code and collects garbage. The tables use the faster of two runs, which is the most favourable reading for Strudel.
- **Strudel ran in Node, not a browser.** It's the same V8 engine as Chrome, but a browser tab is also drawing the page and running Web Audio.
- **One machine.** The ratios matter more than the absolute times.

## Running the comparison

- **Rudel:**
  - `cargo bench -p rudel-lang` prints the back-to-back evaluation times and the query times.
  - The paced evaluation times need a 30 ms sleep before each timed `rudel_lang::eval` call.
- **0.21.0:**
  - Check out commit `029e61e` in a git worktree and run the same benchmark.
  - Copy the current `time` function into its benchmark first: that version has no one-second cap per case, so the 65,536-event pattern runs for about half an hour.
- **Strudel:**
  - Write a small Vitest file next to [`tools/oracle/strudel_diff.test.mjs`](../tools/oracle/strudel_diff.test.mjs), using a copy of its config.
  - Call `evaluate(code)` from `@strudel/transpiler` and `pattern.queryArc(0, 16)` on the result.
  - The README there explains how to link the vendored Strudel checkout.
