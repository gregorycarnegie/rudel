# Parity oracle generators

These scripts dump golden reference values from Strudel's real engine so the
Rust port can be checked against them. The committed goldens live next to these
generators in `tools/oracle/` and are embedded by the `*_parity.rs` integration
tests. `tools/gen_parity_oracle.mjs` (one level up)
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

## Differential run

The golden generators above read Strudel's *committed* values. `strudel_diff.
test.mjs` does the other thing: it evaluates a directory of patterns in a real
Strudel runtime, so an arbitrary corpus can be compared against rudel rather
than against a snapshot. That comparison is the only way to read a pass rate
over user-written patterns — a large share of them do not work in Strudel
either, and without the other side you cannot tell those from a rudel gap.

It needs more of the workspace than the generators do: every `@strudel/*`
package plus `superdough`/`supradough` linked into `strudel/node_modules`, and
vitest (superdough imports its audioworklets extensionless, which only resolves
once vite transforms the package instead of handing it to node).

```sh
cd tools/oracle
npm install --no-save vitest@3.0.4 acorn escodegen estree-walker   @tonaljs/tonal chord-voicings webmidi nanostores @kabelsalat/lib @kabelsalat/web
# link the third-party deps and every strudel package into the strudel root
cd ../../strudel/node_modules
for d in ../../tools/oracle/node_modules/*/; do ln -sfn "$PWD/$d" "$(basename $d)"; done
for p in ../packages/*/; do ln -sfn "$PWD/$p" "@strudel/$(basename $p)"; done
ln -sfn "$PWD/../packages/superdough" superdough
ln -sfn "$PWD/../packages/supradough" supradough
```

Then, from the repo root:

```sh
DIFF_CORPUS=/path/to/patterns DIFF_OUT=strudel.tsv   node strudel/node_modules/vitest/vitest.mjs run     --config tools/oracle/strudel_diff.config.mjs
```

Each line is `<OK|EMPTY|ERR>	<id>	<haps|message>`. `DIFF_FROM`/`DIFF_TO`
shard a long run; `DIFF_CYCLES` sets the query length (8 by default).

The Rudel half of the same run is `cargo run --release -p rudel-lang --example
sweep`, which writes that shape too and crosses the two into a works/fails
square. `.claude/skills/parity-square/SKILL.md` is the whole loop — fetching a
corpus, running both sides, reading the square, and checking a change for
regressions across it.

## Regenerate

```sh
cd tools/oracle
node gen_mini_oracle.mjs        # -> mini_golden.json
node gen_core_oracle.mjs        # -> core_golden.json
node gen_tonal_oracle.mjs       # -> tonal_golden.json  (needs the tonal/xen/edo deps above)
node gen_tune_table_oracle.mjs  # -> tune_table_golden.json  (whole tune.js archive)
node gen_examples_oracle.mjs    # -> examples_golden.json  (every jsdoc @example)
node gen_stepwise_oracle.mjs    # -> stepwise_golden.json  (the stepwise page)
```

`gen_stepwise_oracle.mjs` runs the real engine rather than scanning source: each
case is a snippet evaluated by Strudel, and `stepwise_parity.rs` evaluates the
*same string* through Rudel. It exists because a step count is metadata — the
page itself notes that `expand(2)` and `expand(4)` sound identical on their own —
so a stepwise example can evaluate and query, as `doc_examples.rs` requires, and
still be wrong until a `stepcat` or a `pace` reads the count back.

`gen_examples_oracle.mjs` is a source scan, like `gen_reference_oracle.mjs`: it
reconstructs the corpus upstream's `test/examples.test.mjs` walks (509 snippets
across 10 packages) without needing the unchecked-in `doc.json`. Its consumer,
`crates/rudel-lang/tests/doc_examples.rs`, runs each snippet through Rudel and
asserts the failing set equals `examples_allowlist.json` exactly.

`gen_zzfx_oracle.mjs` is independent — it inlines superdough's `buildSamples`
(only the `getAudioContext().sampleRate` line is replaced with a fixed rate), so
it needs no `@strudel` symlinks. Its golden lives with the DSP tests:

```sh
node gen_zzfx_oracle.mjs        # -> zzfx_golden.json  (ZzFX audio golden)
node gen_lfo_oracle.mjs         # -> lfo_golden.json   (LFO modulator-source golden)
node gen_adsr_oracle.mjs        # -> adsr_golden.json  (linear ADSR gain-envelope golden)
node gen_distortion_oracle.mjs  # -> distortion_golden.json  (waveshaping distortion golden)
node gen_warp_oracle.mjs        # -> warp_golden.json  (wavetable phase-warp golden)
node gen_bytebeat_oracle.mjs    # -> bytebeat_golden.json  (bytebeat expressions vs V8)
```

`gen_bytebeat_oracle.mjs` is the odd one out: it does not sample audio, it pins
an *evaluator*. Upstream compiles a bytebeat with `new Function`, so the
reference is JavaScript itself — the golden records what V8 returns for every
built-in beat plus 26 operator-surface cases over 63 values of `t`, and
`crates/rudel-dsp/tests/bytebeat_golden.rs` checks Rudel's own parser against it.

### Web Audio graph oracle (`OfflineAudioContext`)

`gen_biquad_oracle.mjs` is the first oracle that renders a *real Web Audio
graph* sample-for-sample instead of pure JS math: it drives a unit impulse
through a `BiquadFilterNode` inside an `OfflineAudioContext`, using
[`node-web-audio-api`](https://github.com/ircam-ismm/node-web-audio-api) (a
faithful native implementation of the Web Audio API in node). This is the
`OfflineAudioContext` route to golden-testing the WebAudio-rendered superdough
paths. It needs only its own npm dep (declared in `package.json`, no `@strudel`
symlinks):

```sh
npm install                     # installs node-web-audio-api
node gen_biquad_oracle.mjs      # -> biquad_golden.json  (BiquadFilterNode impulse responses)
node gen_vowel_oracle.mjs       # -> vowel_golden.json   (VowelNode formant-bank impulse responses)
node gen_phaser_oracle.mjs      # -> phaser_golden.json  (swept-notch phaser impulse responses)
```

For the biquad oracle only `bandpass`/`notch` are golden-tested (linear Q in both
WebAudio and the RBJ cookbook, so they match Rudel's `Biquad` exactly);
`lowpass`/`highpass` use WebAudio's dB-Q convention and stay on smoke tests. The
vowel oracle renders superdough's `VowelNode` (5 parallel bandpass formants ->
gains -> x8 makeup), matching Rudel's `Formant`. The phaser oracle renders
superdough's `getPhaser` notch with its `detune` swept by the `getLfo` triangle
(±sweep cents), matching Rudel's `PostFxVoice` phaser. The goldens are consumed
by `biquad_impulse_response_matches_webaudio`,
`vowel_formant_impulse_response_matches_webaudio`, and
`phaser_swept_notch_impulse_response_matches_webaudio` in `rudel-dsp`.

Then run `cargo test -p rudel-mini`.

### Reference-surface oracle (no deps)

`gen_reference_oracle.mjs` reconstructs Strudel's `@strudel/reference` name
surface without a jsdoc build: it source-scans the vendored `strudel/packages`
for jsdoc `@name`/`@synonyms` tags and `register`/`registerControl` calls (the
same names `doc.json` keys on). It needs no `@strudel` symlinks or npm deps.

```sh
node gen_reference_oracle.mjs   # -> reference_golden.json  (Strudel name surface)
```

The golden stays in `tools/oracle/` and records both the flat `names`/`controls`
surface and a per-name `packagesByName` map. It is embedded directly by three
`rudel-lang` tests:

- `reference_parity.rs` compares it against `rudel_lang::reference()` (the
  live-introspected Rudel surface). Every Strudel-documented name Rudel does not
  expose is accounted for in `tools/oracle/reference_allowlist.json`
  (category + reason); the test asserts the diff equals that allowlist exactly,
  so regenerating the golden after a Strudel bump — or adding/removing a Rudel
  name — fails until the allowlist is updated.
- `api_inventory.rs` renders the per-package classified inventory to
  `docs/API_INVENTORY.md` (implemented / intentional / deferred / unaccounted)
  and asserts the committed copy is byte-identical. Regenerate with
  `RUDEL_BLESS=1 cargo test -p rudel-lang --test api_inventory`.
- `reference_snapshot.rs` snapshots Rudel's own reference/autocomplete surface to
  `crates/rudel-lang/tests/reference_surface.txt`. Regenerate with
  `RUDEL_BLESS=1 cargo test -p rudel-lang --test reference_snapshot`.

Run `cargo test -p rudel-lang` to exercise all three.
