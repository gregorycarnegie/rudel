# Vendored: dough

`dough.h` is the dough synth engine by Felix Roos and contributors,
<https://codeberg.org/uzu/dough>, licensed AGPL-3.0-or-later (the same licence
as Rudel; see the repository's `LICENSE`).

- Taken from the npm package `dough-synth` version **0.4.1**, the version
  Strudel's `packages/dough` pins.
- SHA-256 of the file as published:
  `02492d266f5eb804f2bf86b0566af762459e07349ae323cbe25aee8bf4222313`

## Modification

This copy differs from the published file in one way: two comment lines near
the top (a URL and a marker string, lines 5 and 6 as published) were removed on
2026-11-15. No code was changed.

## Updating

Replace `dough.h` with the new version's file, remove the same two comment
lines, and update the version and hash above. Rudel compiles it through
`../src/dough.c` with no audio back end (neither `DOUGH_MINIAUDIO` nor
`DOUGH_PORTAUDIO`), so only the engine itself is built.
