# Changelog

Notable changes to Rudel. Format follows [Keep a Changelog][kac]; versioning is
[semantic][semver], with the pre-1.0 convention that the minor number carries
breaking changes.

This file starts at 0.7.0. Earlier history is in the git log.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [0.7.0] — 2026-08-01

A parity and testing release. Several long-standing divergences from Strudel
turned up once the DSP voices were compared against superdough sample-for-sample
rather than merely asserted to make a sound. **Three of the fixes change what
existing patterns sound like** — see Migrating below.

### Fixed

- **Mini-notation no longer crashes the process on deeply nested input.** Every
  level of `[]`/`<>`/`{}`/`()` recursed through the parser, and every chained
  operator through the pattern builder, so a few hundred levels overflowed the
  stack. That is an abort, not a panic: no `Result` could catch it and the
  editor buffer went with it. Nesting deeper than 64 is now rejected before the
  grammar runs, which reads as silence like any other invalid pattern. Reachable
  by paste, or by a script that generates mini-notation.
- **A one-voice super-saw played ~9 cents flat.** `s("supersaw").unison(1)` kept
  subtracting the per-voice centering offset even though superdough's
  `getDetuner` returns a flat `0` below two voices, detuning the whole voice by
  half the spread.
- **The pitch envelope released ten times too fast.** `getADSRValues` was never
  ported: naming any one of `pattack`/`pdecay`/`psustain`/`prelease` sends the
  rest to a clamped branch where release floors at 10ms, but Rudel filled each
  stage in from its own defaults, so an unset release stayed at 1ms.
- **`.shape(1)` put NaN into the mix.** The waveshaper backs its divisor off
  from 1 with upstream's `1.0 - 4e-10`, which is fine in JS doubles but rounds
  straight back to 1.0 in `f32` — so `1 - shape` was zero, the shaping
  coefficient infinite, and the result `inf/inf`. NaN does not stay local; it
  propagates through everything downstream of the voice. Now backs off one
  `f32` ulp, giving the hard clipper the bound was always meant to produce.

### Changed

- **The gain envelope now resolves through `getADSRValues` as well.** Its stages
  are decided together rather than filled in field-wise, so naming one changes
  what the others default to — matching upstream. Audible on any pattern that
  sets some but not all of attack/decay/sustain/release.
- **`cp` (clap) re-attacks.** Its second and third envelope stages sat at
  `exp(0) = 1` before their onsets rather than at 0, so all three fired at once
  and the result decayed smoothly — a noise blip, not a clap. They are now gated
  on, giving bursts at 0, 10ms and 20ms like the 808 circuit and like the
  recorded clap Strudel plays here. Peak level is held roughly constant (0.606 →
  0.546); average energy drops, the sound now being three bursts with gaps.
- `adsr`/`ad`/`ds`/`ar` are handled only in `rudel-core`, where they expand into
  plain `attack`/`decay`/`sustain`/`release` controls as they do upstream. The
  duplicate handling in the DSP layer was unreachable and had drifted (it forced
  `ad`'s sustain to 0 by hand); it is gone. No user-visible change.

### Added

- `cargo run -p rudel-dsp --example envelope_ab` renders the gain envelope
  before and after the change to WAV, for A/B listening.
- Audio parity oracles for the super-saw oscillator (`gen_supersaw_oracle.mjs`,
  including a moving pitch driven by `getPitchEnvelope` + `getVibratoOscillator`)
  and for the additive wavetable builder and pink/brown noise filters
  (`gen_oscillator_oracle.mjs`).
- The native app has its first tests that render a frame, via `egui_kittest` —
  transport buttons, eval/hush shortcuts, and the inline widget paint path.

### Internal

- CI runs `cargo nextest`, one process per test, so the engine's global
  registries cannot let a test pass on a neighbour's setup.
- `getParamADSR` and the Web Audio parameter mock are shared from
  `tools/oracle/lib.mjs` instead of living in one generator.
- Mutation survivors in the voices that were compared sample-for-sample:
  `next_supersaw` 0 of 71, `oscillator.rs` 2 of 226, `drum.rs` 2 of 161 — all
  four equivalent mutants (`<` against a time boundary that accumulated `f32`
  never lands on, and a peak-normalisation guard).

### Migrating

Patterns that name only *some* envelope stages will sound different, because the
unnamed ones now re-default the way Strudel's do. Spell out the stage you were
relying on to get the old shape back:

| pattern | was | now | to restore |
| --- | --- | --- | --- |
| `.attack(0.1)` | decay 0.05, sustain 0.6 | decay 0.001, sustain 1.0 | `.attack(0.1).decay(0.05).sustain(0.6)` |
| `.decay(0.15)` | sustain 0.6 (held) | sustain 0.001 (percussive) | `.decay(0.15).sustain(0.6)` |
| `.release(0.001)` | release 0.001 | release 0.01 (upstream's floor) | not restorable; the floor is upstream's |

Patterns naming all four stages are unaffected, as are `s("cp")` users who want
the old sound only in that they cannot have it — the previous envelope was a bug
against its own stated intent.
