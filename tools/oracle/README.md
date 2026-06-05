# Parity oracle generators

These scripts dump golden reference values from Strudel's real engine so the
Rust port can be checked against them. The committed goldens live in
`crates/rudel-mini/tests/{mini_golden.json,core_golden.json}` (mini-notation and
core transforms) and are embedded by the `*_parity.rs` integration tests.
`tools/gen_parity_oracle.mjs` (one level up) generates the RNG/signal goldens for
`crates/rudel-core/tests/parity_oracle.rs` and needs no setup.

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

## Regenerate

```sh
cd tools/oracle
node gen_mini_oracle.mjs   # -> mini_golden.json
node gen_core_oracle.mjs   # -> core_golden.json
cp mini_golden.json core_golden.json ../../crates/rudel-mini/tests/
```

Then run `cargo test -p rudel-mini`.
