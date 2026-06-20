# Parity oracle generators

These scripts dump golden reference values from Strudel's real engine so the
Rust port can be checked against them. The committed goldens live in
`crates/rudel-mini/tests/{mini_golden.json,core_golden.json,tonal_golden.json}`
(mini-notation, core transforms, and tonal/xen) and are embedded by the
`*_parity.rs` integration tests. `tools/gen_parity_oracle.mjs` (one level up)
generates the RNG/signal goldens for `crates/rudel-core/tests/parity_oracle.rs`
and needs no setup.

## Setup (one-time)

Strudel uses a pnpm workspace; we only need `@strudel/core` + `@strudel/mini`
plus their single npm dep, `fraction.js`. Node resolves the packages' bare
imports from their real location, so `node_modules` must sit at the strudel root.

```sh
cd tools/oracle && npm install            # installs fraction.js here
```

Then create the package junctions (Windows; junctions need no admin). From a
PowerShell prompt at the repo root:

```powershell
$strudel = "$pwd\strudel"; $nm = "$strudel\node_modules"
New-Item -ItemType Directory -Force "$nm\@strudel" | Out-Null
Copy-Item -Recurse -Force tools\oracle\node_modules\fraction.js "$nm\fraction.js"
foreach ($p in 'core','mini') {
  New-Item -ItemType Junction -Path "$nm\@strudel\$p" -Target "$strudel\packages\$p"
}
```

(On Linux/macOS use `ln -s` symlinks instead of junctions.)

### Tonal/xen oracle (extra deps)

`gen_tonal_oracle.mjs` additionally imports `@strudel/{tonal,xen,edo}`.
`@strudel/xen` and `@strudel/edo` are self-contained (bundled `tunejs.js` /
`ratios.mjs`), but `@strudel/tonal` pulls in `@tonaljs/tonal` and
`chord-voicings`. Install those and link all five packages into
`tools/oracle/node_modules/@strudel` (the symlinks are what node resolves — note
that `npm install` prunes them, so re-create them afterwards):

```sh
cd tools/oracle
npm install --no-save @tonaljs/tonal chord-voicings
cd node_modules/@strudel
for p in core mini tonal xen edo; do ln -s "$PWD/../../../../strudel/packages/$p" "$p"; done
```

## Regenerate

```sh
cd tools/oracle
node gen_mini_oracle.mjs        # -> mini_golden.json
node gen_core_oracle.mjs        # -> core_golden.json
node gen_tonal_oracle.mjs       # -> tonal_golden.json  (needs the tonal/xen/edo deps above)
node gen_tune_table_oracle.mjs  # -> tune_table_golden.json  (whole tune.js archive)
cp mini_golden.json core_golden.json tonal_golden.json tune_table_golden.json \
  ../../crates/rudel-mini/tests/
```

`gen_zzfx_oracle.mjs` is independent — it inlines superdough's `buildSamples`
(only the `getAudioContext().sampleRate` line is replaced with a fixed rate), so
it needs no `@strudel` symlinks. Its golden lives with the DSP tests:

```sh
node gen_zzfx_oracle.mjs        # -> zzfx_golden.json  (ZzFX audio golden)
node gen_lfo_oracle.mjs         # -> lfo_golden.json   (LFO modulator-source golden)
node gen_adsr_oracle.mjs        # -> adsr_golden.json  (linear ADSR gain-envelope golden)
cp zzfx_golden.json lfo_golden.json adsr_golden.json ../../crates/rudel-dsp/tests/
```

Then run `cargo test -p rudel-mini`.
