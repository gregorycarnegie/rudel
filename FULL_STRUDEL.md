# Full Strudel Parity

This is the canonical parity checklist for making Rudel fully compatible with the local Strudel checkout in `strudel/`.

Completing this file should mean that Rudel can run Strudel code, mini-notation, controls, transforms, editor workflows, outputs, examples, and tests with matching behavior unless a difference is explicitly documented as intentional.

## Markers

These apply to every checklist item in this document:

- `[x]` **done** — implemented and matched, or resolved as intentionally different / not applicable (with the reason in the item's note).
- `[~]` **partial or deferred** — some sub-parts are implemented and others are postponed/not yet ported; the note says what is done versus pending.
- `[ ]` **not started**.

## Implementation Guidance

- Prefer adding names through existing alias groups and generated binding lists before writing a new implementation from scratch.
- Add a unique `KPattern` implementation only when the Strudel behavior cannot be represented by an existing alias, control wrapper, generated method group, or shared helper.
- When adding a new unique `KPattern` method, include parity tests that show why it needs bespoke behavior.

## Definition of Done

- [ ] Pin the exact Strudel source of truth used for parity: package versions, git commit if available, and any local patches under `strudel/`.
- [ ] Build an automated API inventory for every Strudel package under `strudel/packages`.
- [ ] For every exported function, registered pattern function, `Pattern.prototype` method, control, alias, top-level REPL helper, and editor command, mark one of: implemented, intentionally different, not applicable, or missing.
- [ ] For every implemented item, add parity tests against Strudel behavior or a documented golden output.
- [ ] For every intentionally different item, document the reason and user-visible behavior.
- [ ] Run all upstream Strudel tests that can be run locally, or port equivalent tests into Rudel where direct execution is not possible.
- [ ] Add a docs/example corpus from Strudel docs and examples, and verify Rudel parses/evaluates/renders/plays them as expected.
- [ ] Add CI checks so new Rudel changes cannot silently regress the parity surface.

## Source Packages to Audit

- [ ] `strudel/packages/core`
- [ ] `strudel/packages/mini`
- [ ] `strudel/packages/transpiler`
- [ ] `strudel/packages/codemirror`
- [ ] `strudel/packages/repl`
- [ ] `strudel/packages/reference`
- [ ] `strudel/packages/xen`
- [ ] `strudel/packages/tonal`
- [ ] `strudel/packages/edo`
- [ ] `strudel/packages/webaudio`
- [ ] `strudel/packages/superdough`
- [ ] `strudel/packages/supradough`
- [ ] `strudel/packages/sampler`
- [ ] `strudel/packages/soundfonts`
- [ ] `strudel/packages/midi`
- [ ] `strudel/packages/osc`
- [ ] `strudel/packages/desktopbridge`
- [ ] `strudel/packages/draw`
- [ ] `strudel/packages/motion`
- [ ] `strudel/packages/gamepad`
- [ ] `strudel/packages/serial`
- [ ] `strudel/packages/mqtt`
- [ ] `strudel/packages/csound`
- [ ] `strudel/packages/hydra`
- [ ] `strudel/packages/tidal`
- [ ] `strudel/packages/mondo`
- [ ] `strudel/packages/mondough`
- [ ] `strudel/packages/web`
- [ ] `strudel/packages/embed`

## Core Pattern Engine

- [ ] Match Strudel's `Pattern`, `Hap`, `State`, `TimeSpan`, `Fraction`, and `Value` data model semantics.
- [ ] Match query semantics including `query`, `queryArc`, whole spans, part spans, event splitting, event clipping, and source locations.
- [~] Audit and implement every export and `Pattern.prototype` method in `core/pattern.mjs`. The bulk of the surface is already implemented (the time/structure/math/higher-order/stepwise/euclid/pick families and the factories `stack`/`cat`/`seq`/`fastcat`/`slowcat`/`pure`/`silence`/`arrange`/`polymeter`). Added in this audit pass: the boolean COMPOSERS `lt`/`gt`/`lte`/`gte`/`eq`/`eqt`/`ne`/`net`/`and`/`or` (via `op_in`, `and`/`or` method-only as they are Koto keywords) and `keepif` (structure from the left, so it keeps the control value intact rather than merging — verified against Strudel on control patterns); `invert`/`inv`, `linger`, `replicate`, `applyN`; the `chunk`/`jux` variants `fastChunk`, `slowChunk` (=`chunk`), `juxFlip`/`flux`, `juxFlipBy`/`fluxBy`; the step-aligned stacks `stackLeft`/`stackRight`/`stackCentre` (pad shorter patterns to the longest's step count); and the aliases `sparsity` (=`slow`), `sequence` (=`seq`), `polyrhythm`/`pr` (=`stack`), `nothing` (=`silence`). Owned by other checklist items (cross-referenced, not duplicated here): the alignment matrix and `_opIn`/`_opOut`/… (pattern alignment item); `soft`/`hard`/`cubic`/`diode`/`asym`/`fold`/`sinefold`/`chebyshev`/`FX`/`worklet`/`partials`/`phases` (distortion/effects item); `band`/`bor`/`bxor`/`blshift`/`brshift` (bitwise, deferred under signal); `hsl`/`hsla` and `density` (controls — `density`'s control shadows the `fast` alias upstream); `cpm`/`hush`/`reset`/`restart`/`ref` (REPL/impure items). Intentionally different (Koto VM can't run in the `Send`/`Sync` query path): the raw bind/join family (`bind`/`innerBind`/`outerBind`/`squeezeBind`/`polyBind`/`stepBind`/`stepJoin`) and `withValue` are not exposed standalone (the high-level combinators, `fmap`/`arpWith` probing, and the `pick` family cover the reachable cases); `shrinklist`/`s_taperlist` stay internal. The span-argument variants `compressSpan`/`focusSpan`/`zoomArc` are intentionally not exposed: in Strudel they take a `TimeSpan` *object* (`.begin`/`.end`) and throw on a plain array, so they are internal helpers — the user-facing two-arg `compress`/`focus`/`zoom` already exist. Also added: `echoWith`/`stutWith` (indexed delayed copies — a new `Callback::apply2` passes `(copy, index)` and falls back to a one-arg call since Koto is strict about arity); `bypass` (per-cycle mute); and `plyWith`/`plyForEach` (repeat each event `factor` times, transforming the copies — their callback runs per value in the query path, so the per-value copies are probed and baked like `arp_with`). And `into` (break a pattern into looped subcycles per a binary pattern, applying a transform — the user-facing `sound(...).into("1 0", f)`) plus `chunkInto`/`chunkBackInto` built on it (the transformed ribbons are probed and baked). Still to implement (this item stays `[~]`): the standalone `arpWith`, `morph` (rhythm morphing between two list-valued patterns), `filter`/`filterWhen` (a per-hap predicate in the query path), `beat`, `stackBy` (the mode-dispatching wrapper over the now-implemented stack aligners), and `collect`/`tag`/`xfade`.
- [x] Audit and implement every export in `core/euclid.mjs`, including aliases such as `euclidRot`, `euclidLegato`, `euclidLegatoRot`, `euclidish`, and `eish`. All in `rudel-core/src/euclid.rs`. `bjorklund` (the Bjorklund algorithm, with negative-pulse inversion), `euclid`, `euclidRot`/`euclidrot`, `euclidLegato`, and `euclidLegatoRot` were already present and tested (tresillo/cinquillo goldens, gapless-legato spans, late-offset rotation, plus proptests for length/pulse-count and inversion). Added the two missing exports: `bjork` (Tidal-style euclid taking a `[pulses, steps, rotation]` tuple, a lone number defaulting `steps = pulses`) and `euclidish`/`eish` (the Malcolm-Braff morph from straight euclidean at `perc = 0` to even pulse at `perc = 1`, porting `_morph`: each onset becomes a `true` hap of width `1/steps` at a position interpolated between its euclidean position `i/steps` and its even position `k/pulses`). Both are bound as methods (with the `eish` alias) and standalone factories (pattern last), verified hap-for-hap against current Strudel for static `perc` (0/0.25/0.5/1), a discrete pattern `perc`, and a *continuous-signal* `perc` (`sine.slow(8)`) across multiple cycles — exact in every case. `euclidish` mirrors Strudel's `register` patternification so a continuous `perc` is sampled once per cycle (`pulses`/`steps` are pure, giving per-cycle structure; `perc` is sampled by `appLeft` and then `innerJoin`ed) rather than once at the query start.
- [x] Audit and implement every export in `core/pick.mjs`, including `pick`, `pickmod`, `pickF`, `pickmodF`, `pickOut`, `pickmodOut`, `pickRestart`, `pickmodRestart`, `pickReset`, `pickmodReset`, `inhabit`, `pickSqueeze`, `inhabitmod`, `pickmodSqueeze`, and the standalone `squeeze`. All bound as methods and prelude factories, parity-tested against the oracle. Intentionally different: `pickF`/`pickmodF` apply their function lookups eagerly at construction (the Koto VM can't be driven from the query path), so functions of time-varying patterns are baked once — equivalent for the function lookups Strudel docs show.
- [~] Audit and implement every export in `core/signal.mjs`, including continuous signals (`saw`/`isaw`/`sine`/`cosine`/`square`/`tri`/`itri` and the `2` bipolar variants, `time`, `steady`), random signals (`rand`/`rand2`/`irand`/`brand`/`brandBy`/`perlin`/`berlin`/`randrun`/`run`/`scan`), seed behavior (`seed`/`withSeed` via a `randSeed` control that `rand` now honors), `choose`/`chooseIn`/`chooseOut`/`choose`/`choose2`, weighted choice (`wchoose`/`wchooseCycles`/`wrandcat`), `shuffle`, `scramble`, conditional transforms (`degrade*`/`sometimes*`/`someCycles*`/`often`/`rarely`/...), and `per`/`perCycle`/`cyclesPer`/`perx`. All bound as prelude factories/methods and golden-tested against the oracle (`tools/gen_parity_oracle.mjs`). Fixed a latent parity bug: rudel's `tri` was `fastcat(isaw, saw)` (Strudel's `itri`); it is now `fastcat(saw, isaw)`. Intentionally deferred/different: `binary`/`binaryN`/`binaryL`/`binaryNL` and `randL` need patternified bitwise ops (not yet ported); the `precise` murmur RNG and `useRNG` are unimplemented (legacy RNG is Strudel's default and is bit-exact); `mousex`/`mousey` and the keyboard signals (`keyDown`/`whenKey`) are browser-only `external_io` and are unsupported.
- [x] Audit and implement every export in `core/value.mjs`. The user-reachable export is `unionWithObj`, which powers control arithmetic via `_composeOp` — Rudel's analog is `Value::union_with` (called by `compose_op` in `transforms/core.rs`). Implemented the missing issue #1026 guard: combining a control map with a bare scalar (wrapped to `{value: x}`) is now refused, returning the control unchanged, so `n("0 2 4").add(7)` is a no-op and you must write `n("0 2 4").add(n(7))` — verified hap-for-hap against current Strudel, including the asymmetry (the guard checks the right operand only, so a scalar on the left still unions, e.g. `add(n("10"), "0 2")` → `{value:0, n:10}`). Intentionally different/not ported: the `Value` Maybe/functor class and its `valued`/`mul`/`map`/`.ap`/`.map`/`.mul`/`.unionWith` helpers are an internal JS applicative abstraction never used by pattern code (only `unionWithObj` is, from `pattern.mjs`); Rudel patternifies with Rust closures (`ValueFn`) and `Value::union_with` directly, so there is no monad wrapper to expose. Strudel logs `[warn]: Can't do arithmetic on control pattern.` on the guarded path; rudel-core has no logger, so the no-op pass-through is the only observable effect.
- [ ] Audit and implement every public utility from `core/util.mjs` that Strudel users can reach from the REPL.
- [x] Match Strudel's currying, registration, alias, and method-chaining behavior. — Method-chaining and registration are matched: `kpattern_methods!` (`generated.rs`) registers every `Pattern.prototype` transform as a chainable KPattern method, and controls are registered from rudel-core's `control_builders` registry by `extend_control_entries` (the analog of Strudel's `register`). Aliases are matched: camelCase method aliases via the macro's `camel_*` groups, and control-name aliases via `control_name`. Standalone (curried-style) forms are provided for the full transform set — value-arg transforms by `register_pattern_fns!` and the higher-order combinators by `register_standalone_callbacks` — taking the pattern last, under both snake_case and Strudel's camelCase names (`fast(2, pat)`, `fastGap`, `iterBack`, `euclidRot`, `echo`, `stut`, `jux(rev, pat)`, `every`/`firstOf`/`chunk`/`juxBy`/`inside`/`off`/`when`/`within`/`someCycles`/…). Hap-for-hap equivalence with the method form is tested across every arg group, and a completeness guard asserts the standalone names are all registered. Intentionally different: Koto has no partial application, so only fully-applied standalone forms exist (no `fast(2)` returning a function, and callback args must be a function value like `rev` or `|x| x.fast(2)`).
- [x] Match Strudel's higher-order function behavior for callbacks, pattern-of-functions, and pattern-valued functions. **Callbacks** (`Pattern -> Pattern`): fully supported via the `Callback` plumbing — `every`/`firstOf`/`lastOf`/`chunk`/`chunkBack`/`jux`/`juxBy`/`superimpose`/`sometimes(By)`/`often`/`rarely`/`some(Cycles)(By)`/`off`/`within`/`inside`/`outside`/`apply`/`always`/`never`/`layer` all run a Koto function value (`rev`, `|x| x.fast(2)`). The VM isn't `Send`/`Sync`, so callbacks are applied **eagerly** at construction, not in the query path (as for `fmap`/`arpWith`, which probe). **Patterned arguments** now match Strudel for every combinator: `every`/`firstOf`/`lastOf` patternify their cycle count (`every("<2 4>", rev)`) via `every_pat`/`every_cycles` + `inner_join` (callback applied once, placed by a per-cycle count); `chunk`/`chunkBack`/`inside`/`outside`/`sometimesBy`/`juxBy`/`someCyclesBy` and `within`'s bounds patternify via `with_cb_*`/`probe_patternify`, which bakes the combinator result for each distinct argument value over a probe window and selects it per cycle with `inner_join` (mirroring `register`'s `arg.fmap(v => combinator(v,f,pat)).innerJoin()`); both the method and camelCase and standalone forms route through this, and scalar args keep a direct fast path. `off`'s time and the arithmetic operands patternify through the existing `late`/`appLeft` paths. All verified hap-for-hap against current Strudel (incl. randomized `sometimesBy("<0 1>", …)` and `juxBy`/`within` patterned bounds). **Boolean patterns**: a bare Koto `true`/`false` now reifies to `pure(Bool)` (`reify(true)`), fixing `when(true, …)`/`struct(true)`/`mask(true)`/`pat(true)` (previously silence). Intentionally different (Koto VM can't run in the `Send`/`Sync` query path): a **pattern-of-functions** as the transform argument (Strudel's `apply(slowcat(rev, fast2))`, function varying per cycle in the query path) is unsupported — callbacks bind to one eagerly-applied function; and the raw bind/join family (`bind`/`innerBind`/`outerBind`/`squeezeBind`/`polyBind`/`stepBind`) isn't exposed standalone for the same reason (the high-level combinators and the `pick` family cover the reachable cases). Patterned args use a 16-cycle probe window, so an argument value first appearing after cycle 16 falls back to silence (the same documented limit as `fmap`/`arpWith`).
- [ ] Match Strudel's pattern alignment behavior across all in/out/mix/squeeze/reset/restart/poly variants.
- [x] Match Strudel's stepwise functions: `take`, `drop`, `expand`, `extend`, `contract`, `shrink`, `grow`, `tour`, `zip`, `pace`, `stepcat`, `timecat`, `stepalt`, and aliases (`timeCat`, `steps`, and the deprecated `s_*` family). Intentionally not exposed: `shrinklist`/`s_taperlist` (internal helper in Rudel).
- [x] Match Strudel's sample/time transforms: `chop`, `striate`, `loopAt`, `loopAtCps`, `slice`, `splice`, `fit`, `scrub`, and related helpers. All live in `rudel-core/src/samples.rs` (rewriting `begin`/`end`/`speed`/`unit`/`_slices` control keys; cps-dependent ones read `_cps` from the query state, defaulting to Strudel's `0.5`). `chop`/`striate`/`slice`/`splice`/`fit`/`loopAt`/`scrub` were already present and verified hap-for-hap against current Strudel (including `chop` nesting into an existing `begin`/`end` sub-range, `slice`'s `{begin,end,_slices,...o}` merge with the sound's keys winning, and `slice`'s split-point-list form). Added the missing members: `bite` (zoom-and-squeeze pattern slicing — `ipat.fmap(i => n => pat.zoom(i/n mod 1, +1/n)).appLeft(npat).squeezeJoin()`, bound as method + standalone `pat2` factory and guarded by the completeness test) and the deprecated `loopAtCps`/`loopatcps` (explicit-cps `loopAt`), plus the lowercase `loopat` alias. Intentionally different/not ported: `chew` does not exist in the vendored Strudel (`packages/core`), so there is nothing to match; for `slice`/`splice` with a *list* of split points Strudel stores the whole array in `_slices` (making `splice`'s `cps / _slices / dur` speed `NaN`), whereas Rudel stores the slice count (`len - 1`) so list-based `splice` yields a sane speed.
- [ ] Match Strudel's distortion/worklet/effects pattern helpers such as `soft`, `hard`, `cubic`, `diode`, `asym`, `fold`, `sinefold`, `chebyshev`, `partials`, `phases`, `FX`, and `worklet`.
- [ ] Match Strudel REPL pattern slots and aliases from `core/repl.mjs`, including `p`, `q`, `d1`-style slots, `p1`-style slots, `q1`-style silence helpers, `cpm`, stack behavior, and hush/update behavior.
- [ ] Match Strudel scheduler behavior: CPS, latency, trigger timing, pattern replacement, and event deadlines.
- [ ] Match reset/timeline/impure behavior from `core/impure.mjs` where it is user-visible.
- [ ] Match speech support from `core/speak.mjs` or document it as intentionally unsupported.

## Mini-Notation and Transpilation

- [x] Match `mini/krill.pegjs` grammar behavior: greedy step tokens (letters, digits, `~ - # . ^ _`), JS `Number()` atom classification (`1e3`, `0x10`, `.5`, `-x`), `~`/`-` rests, `^` steps marker, adjacency rules for `@`/`!`/`?` amounts, and `slice_with_ops` euclid args (ops parsed but discarded, as in mini.mjs). Intentionally different: the experimental "haskellish" operator/command layer at the bottom of krill.pegjs (`cat [...]`, `setcps`, `hush`, reached only via the `h()` API) is not implemented; a lone `^` atom is a parse error in Rudel instead of the atom `"^"`.
- [x] Match `mini/krill-parser.js` output for all upstream mini tests: every deterministic case from `mini/test/mini.test.mjs` (plus tokenizer edge cases) is golden-tested hap-for-hap against Strudel's real parser via `tools/oracle/gen_mini_oracle.mjs`, including `_steps`. Upstream's statistical PRNG tests are covered more strongly by exact PRNG-parity goldens (`?`/`|` use per-occurrence seeds offsetting `rand` by `0.0003 * seed`, matching krill's `seed++` order).
- [x] Match `mini/mini.mjs` APIs: `mini` ≈ `rudel_mini::parse`, `m` ≈ `parse_with_offset`, `getLeafLocation`/`getLeafLocations` ≈ `leaf_locations` (golden-tested against Strudel for every oracle pattern), `minify` ≈ `parse_or_silence`/`IntoPattern`, `miniAllStrings` ≈ `install`. Intentionally different: `mini2ast`/`getLeaves` (raw krill AST objects) are internal to the pest parser, and `h` (haskellish parsing) is unsupported with the rest of that layer. Locations are byte offsets into the bare pattern string (Strudel's are quoted-string offsets, i.e. +1).
- [x] Preserve source locations from mini-notation leaves through Rudel patterns: atoms are tagged with `Pattern::with_loc`, hap contexts accumulate locations through all combinators (including op arguments via the `pure_loc` patternify fast-path, matching Strudel's `__pure_loc`), and per-hap locations are golden-tested hap-for-hap against Strudel across the whole mini oracle.
- [x] Match mini-notation parser edge cases: nesting, alternation, Euclidean syntax (incl. patterned args `a(<3 5>,<8 16>)` via appLeft+innerJoin), polymeter syntax (incl. patterned `%<2 3>` and weighted sequences), ratios, weights, rests, holds, lists, subdivisions, repetition (krill's accumulate/move-to-end op semantics for `!`, `_repeatCycles`-based), degradation, randomness, and patterned ranges (`<0 1> .. <2 4>` via squeezeBind). Source offsets are tracked by the source-locations item above.
- [x] Match `transpiler/transpiler.mjs` behavior. Rudel runs Koto rather than JavaScript, so the analog is the source pass in `rudel-lang/src/preprocess.rs` (`preprocess_strudel`), not an Acorn/escodegen AST walk. Equivalents: label statements `x: expr` (Strudel's `labelToP` → `.p('x')`) are collected by `rewrite_labels` into `rudel_label("x", expr)` and stacked, and `$:` is supported as an anonymous label (golden-tested in `per_pattern_naming_and_mute`); Strudel's "add `return` to the last statement" is unnecessary because Koto already yields the last expression; the empty-body → `silence` fallback is matched (an empty or fully commented-out script preprocesses to `silence()`). Intentionally different/not ported: the `blockBased` REPL machinery (`strudelScope`/`globalThis`/`userDefinedKeys` cross-block persistence, `all()` block detection, widget/visualizer subtree scanning) is a CodeMirror-REPL concern that does not apply to Rudel's egui app; `// mini-off`/`// mini-on` disable ranges are unimplemented because Rudel does not blanket-convert every double-quoted string to mini-notation (see below), so there is nothing to selectively disable.
- [x] Match `transpiler/plugin-mini.mjs` double-quoted mini-notation transformation. Intentionally different mechanism, same user-visible result: Strudel rewrites *every* double-quoted (and backtick) string into an `m(value, offset)` call up front, whereas Rudel parses strings as mini-notation lazily and contextually — pattern-typed function arguments run through `arg_to_pattern`/`rudel_mini::parse`, and a bare string that is the target of a method chain (`"0 1".fast(2)`) is wrapped into `pat("0 1")` by `rewrite_string_method_chains`. Source locations from those strings are preserved per-hap (covered by the mini source-locations item above). Not ported: the tagged-template language layer (`mini`/`tidal`/`minilang` registration, `registerLanguage`) and backtick template literals, since Koto has no tagged templates; `mini2ast`/`getLeaves` remain pest-internal (noted above).
- [x] Match `transpiler/plugin-widgets.mjs` inline widget transformation — intentionally unsupported. The slider/`scope`/`pianoroll`/`pitchwheel`/`spectrum`/`punchcard`/`spiral`/`wordfall` rewrites (`sliderWithID`, widget-id generation) are bindings into `@strudel/codemirror`'s editor-overlay machinery. Rudel's editor is egui, not CodeMirror, so there is no inline-widget surface to target; these are out of scope rather than missing parity.
- [x] Match `transpiler/plugin-sample.mjs` sample shorthand behavior — intentionally different, no rewrite needed. Strudel wraps bare `samples(...)` calls in `await` so the browser can load sample maps asynchronously before the pattern runs. Rudel evaluates synchronously and instead collects `samples(...)`/`aliasBank(...)` requests into `SampleEffects` (returned from `eval_with_samples`) for the host to apply against its own sample bank, so no `await`-injection pass is required (covered by the `samples_*` tests).
- [x] Match `transpiler/plugin-kabelsalat.mjs` behavior — intentionally unsupported, as the item permits. The `K(...)` → `worklet(...)` transform (template/placeholder extraction, `S(...)` pattern wraps, kabelsalat-language codegen) targets the kabelsalat audio-worklet graph DSL, which Rudel's DSP engine does not implement; this matches the deferral of `worklet`/`FX` in the effects item above.
- [x] Support Strudel-style JavaScript user-code conveniences in Rudel's language layer or provide a compatibility translation path. Handled by `preprocess_strudel`: `const` declarations are stripped to plain assignments, `//` line comments are removed (string-aware), `Math.pow` is bound, and JavaScript arrow functions are rewritten into Koto lambdas — `x => …`, `(x) => …`, `(a, b) => …`, and `() => …` become `|x| …` / `|a, b| …` / `|| …` (`rewrite_arrow_functions`, string-aware so `=>` inside a pattern string and the `>=` operator are left intact). Intentionally not converted: arrow bodies that are block statements (`x => { … }`), because Koto would read `{ … }` as a map literal — Strudel's docs use expression-bodied callbacks, which map cleanly.
- [x] Add differential tests that compare Rudel and Strudel transpiled output for representative code snippets. `rudel-lang/src/tests.rs` exercises `preprocess_strudel` directly against expected rewrites (arrow forms, string/operator preservation, empty → `silence()`) and includes a differential check asserting that arrow-function and Koto-lambda spellings of the same callback (`every`/`superimpose`/`within`/`layer`) produce hap-for-hap identical patterns. (Rudel emits Koto, not JavaScript, so the comparison is against expected Koto source and against behavioural parity rather than against Strudel's JS AST output.)

## Controls Parity

Source: `strudel/packages/core/controls.mjs`.

Checked items mean the Strudel-style chained control name is exposed in Rudel's public Koto/Rust surface. Unchecked items are missing as dedicated Strudel-compatible control methods, even if a value could be set manually with `.ctrl("name", value)`.

Ranges such as `fmh3`-`fmh8` mean every control name in that range shares the row's status.

### Sound, Pitch, and Amplitude

- [x] `s`
- [x] `sound`
- [x] `source`
- [x] `src`
- [x] `n`
- [x] `i`
- [x] `note`
- [x] `freq`
- [x] `accelerate`
- [x] `velocity`
- [x] `vel`
- [x] `gain`
- [x] `postgain`
- [x] `amp`
- [x] `bank`
- [x] `pan`
- [x] `speed`
- [x] `stretch`
- [x] `unit`
- [x] `clip`
- [x] `legato`
- [x] `duration`
- [x] `dur`

### Wavetable and Warp Controls

- [x] `wt`
- [x] `wavetablePosition`
- [x] `wtenv`
- [x] `wtattack`
- [x] `wtatt`
- [x] `wtdecay`
- [x] `wtdec`
- [x] `wtsustain`
- [x] `wtsus`
- [x] `wtrelease`
- [x] `wtrel`
- [x] `wtrate`
- [x] `wtsync`
- [x] `wtdepth`
- [x] `wtshape`
- [x] `wtdc`
- [x] `wtskew`
- [x] `warp`
- [x] `wavetableWarp`
- [x] `warpattack`
- [x] `warpatt`
- [x] `warpdecay`
- [x] `warpdec`
- [x] `warpsustain`
- [x] `warpsus`
- [x] `warprelease`
- [x] `warprel`
- [x] `warprate`
- [x] `warpdepth`
- [x] `warpshape`
- [x] `warpdc`
- [x] `warpskew`
- [x] `warpmode`
- [x] `wavetableWarpMode`
- [x] `wtphaserand`
- [x] `wavetablePhaseRand`
- [x] `warpenv`
- [x] `warpsync`

### FM and Supersaw Controls

- [x] `fmh`
- [x] `fmh1`
- [x] `fmh2`
- [x] `fmh3`-`fmh8`
- [x] `fmi`
- [x] `fmi1`
- [x] `fmi2`
- [x] `fmi3`-`fmi8`
- [x] `fm`
- [x] `fm1`-`fm8`
- [x] `fmenv`
- [x] `fmenv1`-`fmenv8`
- [x] `fme`
- [x] `fmattack`
- [x] `fmattack1`
- [x] `fmattack2`
- [x] `fmattack3`-`fmattack8`
- [x] `fmatt`
- [x] `fmatt1`-`fmatt8`
- [x] `fmwave`
- [x] `fmwave1`
- [x] `fmwave2`
- [x] `fmwave3`-`fmwave8`
- [x] `fmdecay`
- [x] `fmdecay1`
- [x] `fmdecay2`
- [x] `fmdecay3`-`fmdecay8`
- [x] `fmdec`
- [x] `fmdec1`-`fmdec8`
- [x] `fmsustain`
- [x] `fmsustain1`
- [x] `fmsustain2`
- [x] `fmsustain3`-`fmsustain8`
- [x] `fmsus`
- [x] `fmsus1`-`fmsus8`
- [x] `fmrelease`
- [x] `fmrelease1`
- [x] `fmrelease2`
- [x] `fmrelease3`-`fmrelease8`
- [x] `fmrel`
- [x] `fmrel1`-`fmrel8`
- [x] `fmi11`-`fmi88`
- [x] `fm11`-`fm88`
- [x] `unison`
- [x] `detune`
- [x] `det`
- [x] `spread`

### Envelopes and Sample Windows

- [x] `attack`
- [x] `att`
- [x] `decay`
- [x] `dec`
- [x] `sustain`
- [x] `sus`
- [x] `release`
- [x] `rel`
- [x] `hold`
- [x] `begin`
- [x] `end`
- [x] `loop`
- [x] `loopBegin`
- [x] `loopb`
- [x] `loopEnd`
- [x] `loope`
- [x] `pattack`
- [x] `patt`
- [x] `pdecay`
- [x] `pdec`
- [x] `psustain`
- [x] `psus`
- [x] `prelease`
- [x] `prel`
- [x] `penv`
- [x] `pcurve`
- [x] `panchor`
- [x] `gate`
- [x] `gat`

### Filters and Filter Envelopes

- [x] `cutoff`
- [x] `ctf`
- [x] `lpf`
- [x] `lp`
- [x] `resonance`
- [x] `lpq`
- [x] `hcutoff`
- [x] `hpf`
- [x] `hp`
- [x] `hresonance`
- [x] `hpq`
- [x] `bandf`
- [x] `bpf`
- [x] `bp`
- [x] `bandq`
- [x] `bpq`
- [x] `lpenv`
- [x] `lpe`
- [x] `hpenv`
- [x] `hpe`
- [x] `bpenv`
- [x] `bpe`
- [x] `lpattack`
- [x] `lpa`
- [x] `hpattack`
- [x] `hpa`
- [x] `bpattack`
- [x] `bpa`
- [x] `lpdecay`
- [x] `lpd`
- [x] `hpdecay`
- [x] `hpd`
- [x] `bpdecay`
- [x] `bpd`
- [x] `lpsustain`
- [x] `lps`
- [x] `hpsustain`
- [x] `hps`
- [x] `bpsustain`
- [x] `bps`
- [x] `lprelease`
- [x] `lpr`
- [x] `hprelease`
- [x] `hpr`
- [x] `bprelease`
- [x] `bpr`
- [x] `ftype`
- [x] `fanchor`
- [x] `lprate`
- [x] `lpsync`
- [x] `lpdepth`
- [x] `lpdepthfrequency`
- [x] `lpdepthfreq`
- [x] `lpshape`
- [x] `lpdc`
- [x] `lpskew`
- [x] `bprate`
- [x] `bpsync`
- [x] `bpdepth`
- [x] `bpdepthfrequency`
- [x] `bpdepthfreq`
- [x] `bpshape`
- [x] `bpdc`
- [x] `bpskew`
- [x] `hprate`
- [x] `hpsync`
- [x] `hpdepth`
- [x] `hpdepthfrequency`
- [x] `hpdepthfreq`
- [x] `hpshape`
- [x] `hpdc`
- [x] `hpskew`

### Modulation, Delay, and Effects

- [x] `vib`
- [x] `vibrato`
- [x] `v`
- [x] `vibmod`
- [x] `vmod`
- [x] `noise`
- [x] `delay`
- [x] `delayfeedback`
- [x] `delayfb`
- [x] `dfb`
- [x] `delayspeed`
- [x] `delaytime`
- [x] `delayt`
- [x] `dt`
- [x] `delaysync`
- [x] `delays`
- [x] `ds`
- [x] `dry`
- [x] `crush`
- [x] `coarse`
- [x] `tremolo`
- [x] `trem`
- [x] `tremolosync`
- [x] `tremolodepth`
- [x] `tremdepth`
- [x] `tremoloskew`
- [x] `tremskew`
- [x] `tremolophase`
- [x] `tremphase`
- [x] `tremoloshape`
- [x] `tremshape`
- [x] `phaser`
- [x] `phaserrate`
- [x] `ph`
- [x] `phasersweep`
- [x] `phs`
- [x] `phasercenter`
- [x] `phc`
- [x] `phaserdepth`
- [x] `phd`
- [x] `phasdp`
- [x] `chorus`
- [x] `drive`
- [x] `duck`
- [x] `duckdepth`
- [x] `duckonset`
- [x] `duckons`
- [x] `duckattack`
- [x] `duckatt`
- [x] `datt`
- [x] `byteBeatExpression`
- [x] `bbexpr`
- [x] `bb`
- [x] `byteBeatStartTime`
- [x] `bbst`
- [x] `channels`
- [x] `ch`
- [x] `pw`
- [x] `pwrate`
- [x] `pwr`
- [x] `pwsweep`
- [x] `pws`
- [x] `channel`
- [x] `cut`
- [x] `djf`
- [x] `lock`
- [x] `fadeTime`
- [x] `fadeOutTime`
- [x] `fadeInTime`
- [x] `leslie`
- [x] `lrate`
- [x] `lsize`

### Tonal, Voicing, and Spatial Controls

- [x] `mtranspose`
- [x] `ctranspose`
- [x] `degree`
- [x] `harmonic`
- [x] `stepsPerOctave`
- [x] `octaveR`
- [x] `nudge`
- [x] `octave`
- [x] `oct`
- [x] `orbit`
- [x] `o`
- [x] `bus`
- [x] `busgain`
- [x] `bgain`
- [x] `overgain`
- [x] `overshape`
- [x] `panspan`
- [x] `pansplay`
- [x] `panwidth`
- [x] `panorient`
- [x] `slide`
- [x] `semitone`
- [x] `voice`
- [x] `chord`
- [x] `dictionary`
- [x] `dict`
- [x] `anchor`
- [x] `offset`
- [x] `octaves`
- [x] `mode`

### Reverb, Room, IR, and Distortion

- [x] `room`
- [x] `roomlp`
- [x] `rlp`
- [x] `roomdim`
- [x] `rdim`
- [x] `roomfade`
- [x] `rfade`
- [x] `ir`
- [x] `iresponse`
- [x] `irspeed`
- [x] `irbegin`
- [x] `roomsize`
- [x] `size`
- [x] `sz`
- [x] `rsize`
- [x] `shape`
- [x] `distort`
- [x] `dist`
- [x] `distortvol`
- [x] `distvol`
- [x] `distorttype`
- [x] `disttype`
- [x] `compressor`
- [x] `compressorKnee`
- [x] `compressorRatio`
- [x] `compressorAttack`
- [x] `compressorRelease`

### SuperDirt, SuperDough, ZZFX, and Miscellaneous Controls

- [x] `analyze`
- [x] `fft`
- [x] `squiz`
- [x] `vowel`
- [x] `waveloss`
- [x] `density`
- [x] `expression`
- [x] `sustainpedal`
- [x] `fshift`
- [x] `fshiftnote`
- [x] `fshiftphase`
- [x] `triode`
- [x] `krush`
- [x] `kcutoff`
- [x] `octer`
- [x] `octersub`
- [x] `octersubsub`
- [x] `ring`
- [x] `ringf`
- [x] `ringdf`
- [x] `freeze`
- [x] `xsdelay`
- [x] `tsdelay`
- [x] `real`
- [x] `imag`
- [x] `enhance`
- [x] `comb`
- [x] `smear`
- [x] `scram`
- [x] `binshift`
- [x] `hbrick`
- [x] `lbrick`
- [x] `frameRate`
- [x] `frames`
- [x] `hours`
- [x] `minutes`
- [x] `seconds`
- [x] `songPtr`
- [x] `uid`
- [x] `val`
- [x] `cps`
- [x] `zrand`
- [x] `curve`
- [x] `deltaSlide`
- [x] `pitchJump`
- [x] `pitchJumpTime`
- [x] `znoise`
- [x] `zmod`
- [x] `zcrush`
- [x] `zdelay`
- [x] `zzfx`
- [x] `color`
- [x] `colour`
- [x] `transient`
- [x] `FXrelease`
- [x] `FXrel`
- [x] `FXr`
- [x] `fxr`

### MIDI and OSC Controls

- [x] `midichan`
- [x] `midimap`
- [x] `midiport`
- [x] `midicmd`
- [x] `ccn`
- [x] `ccv`
- [x] `ctlNum`
- [x] `nrpnn`
- [x] `nrpv`
- [x] `progNum`
- [x] `sysexid`
- [x] `sysexdata`
- [x] `midibend`
- [x] `miditouch`
- [x] `polyTouch`
- [x] `oschost`
- [x] `oscport`

### Other APIs in `controls.mjs`

- [x] Match Strudel behavior for `adsr`, `ad`, `ds`, and `ar` envelope helpers: `:`-list values expand into `attack`/`decay`/`sustain`/`release` with Strudel's defaults (`ad` decay=attack, `ds` sustain=0, `ar` release=attack).
- [x] Implement `control([ccn, ccv])` MIDI helper.
- [x] Implement `sysex([id, data])` MIDI helper.
- [x] Implement `as(mapping)` batch control mapper (`pat("c:.5").as("note:clip")`), with alias canonicalization.
- [x] Implement `scrub(begin)` sample scrub helper (structure from the positions pattern; `"pos:speed"` lists scale playback speed; clip forced to 1).
- [x] Implement `createParams(...)` / custom control parameter creation. Intentionally different: Rudel exposes `.ctrl(name, value)` for arbitrary named controls instead of creating new global functions at runtime.
- [ ] Implement `modulate(type, config, id)`, `lfo(config, id)`, `env(config, id)`, and `bmod(config, id)` behavior. Blocked on porting superdough's generic modulator engine to rudel-dsp; the dedicated LFO controls (`lprate`, `tremolo`, ...) cover the common cases.
- [x] Verify alias canonicalization matches Strudel's `getControlName`: `rudel_core::control_name` resolves every alias by probing the macro-generated builder registry, with parity tests.

## Xenharmonic, Tonal, and EDO

- [ ] Match every export in `xen/xen.mjs`, including `edo`, `xen`, `withBase`, `ftrans`, `fTrans`, `ftranspose`, `fTranspose`, and `tuning`.
- [ ] Match every export in `xen/tune.mjs`, including named scale lookup behavior and `tunejs` scale compatibility.
- [ ] Verify Rudel's generated tune table against `xen/tunejs.js` and upstream scale data.
- [ ] Match all examples from Strudel's xenharmonic docs.
- [ ] Match every export in `tonal/tonal.mjs`, including `transpose`, `trans`, `scaleTranspose`, `scaleTrans`, `strans`, and `scale`.
- [ ] Match every export in `tonal/tonleiter.mjs`, including chord tokenization, pitch-class conversion, scale-step behavior, named scales, voicing rendering, and note transposition.
- [ ] Match every export in `tonal/voicings.mjs`, including dictionaries, aliases, ranges, `voicings`, `rootNotes`, and `voicing`.
- [ ] Match `tonal/ireal.mjs` simple and complex dictionaries.
- [ ] Match every export and test in `edo`, including intervals, ratios, pitch naming, and EDO scale behavior.

## Audio and Output Backends

- [ ] Match `webaudio/webaudio.mjs` output behavior, including oscillator fallback, sample/synth controls, `webaudioRepl`, and `Pattern.prototype.dough`.
- [ ] Match `webaudio/scope.mjs`, `webaudio/spectrum.mjs`, and `webaudio/supradough.mjs`, including `tscope`, `fscope`, `scope`, `spectrum`, and `supradough`.
- [ ] Match `superdough` synthesis and sampler behavior, including wavetable, vowel, feedback delay, reverb generation, modulators, ZZFX, node pools, and worklet behavior.
- [ ] Match `supradough` behavior or document any intentional difference.
- [ ] Match `sampler` package behavior, including sample server expectations and remote/local sample resolution.
- [ ] Match `soundfonts` behavior, including font loading, GM map, and SoundFont playback.
- [ ] Match sample naming, bank resolution, sample indexes, sample URL schemes, and sample caching behavior.
- [ ] Match Strudel's gain, clipping, pan, envelope, effect, reverb, distortion, compressor, modulation, and bus semantics audibly enough for representative examples.
- [ ] Add audio golden tests where deterministic output is possible, and smoke tests for real-time outputs where exact samples are not stable.

## MIDI, OSC, and Bridges

- [ ] Match `midi/midi.mjs` output behavior, including `Pattern.prototype.midi`, device selection, default maps, channel behavior, note-on/off timing, CC, NRPN, pitch bend, aftertouch, sysex, and MIDI clock commands.
- [ ] Match `midi/input.mjs`, including incoming MIDI signals and `ccin`-style behavior.
- [ ] Match `osc/osc.mjs` and `osc/superdirtoutput.js`, including control parsing, event timing, SuperDirt OSC address/value behavior, and target routing.
- [ ] Match `desktopbridge` MIDI, OSC, and logger bridges where Rudel exposes equivalent desktop behavior.
- [ ] Add integration tests with fake MIDI/OSC devices or loopback ports.

## Editor, REPL, and Live Coding UX

- [~] Add inline UI controls as code inputs in the editor, matching Strudel-style live widgets such as sliders/knobs/toggles embedded in pattern code. — Deferred: inline live widgets are intentionally postponed until the rest of the editor/REPL surface lands.
- [~] Support Strudel-style inline UI values as live pattern inputs, not just visual editor widgets. — Deferred with the widget work above.
- [~] Add inline animations/visuals in the editor so code can create or drive visual feedback directly. — Deferred; depends on the inline-widget surface and the `draw` package port (see Visuals section).
- [~] Support Strudel-style inline animation/visual outputs as first-class runtime effects. — Deferred with the inline-visual work above.
- [x] Add `Ctrl+\` comment/uncomment for the current line or current selection. — `crates/rudel-app/src/editor.rs` toggles `//` line comments over the cursor line or selection on `Ctrl+\` and on `Ctrl+/` (Strudel/CodeMirror's actual binding); selection bounds are preserved. Covered by `editor_toggles_line_comments`.
- [x] Add basic syntax highlighting for Rudel/Strudel-like code.
- [~] Upgrade syntax highlighting to Strudel/CodeMirror-grade highlighting, including richer token categories and mini-notation awareness. — `editor.rs::tokenize` now distinguishes keywords/factories/controls/signals, methods, numbers, strings, and comments, and tokenizes mini-notation inside string literals (words, numbers, rests `~`, and operators `*/!@<>[]{}(),.?:|%-` get distinct colors). Still missing vs CodeMirror: a real Lezer grammar, selectable themes, and bracket-depth coloring. Tests: `tokenizes_mini_notation_inside_strings`, `tokenizes_note_names_and_decimals_in_mini`, `highlights_keywords_methods_and_numbers_in_code`.
- [x] Add active-event highlighting for mini-notation and code spans while playback runs. — `app/panels.rs::active_source_spans` queries the live pattern at the audio clock's current position each frame and flashes the byte ranges of the active haps' source locations in the editor (`editor.rs`, background highlight under the overlapping tokens). Covers mini-notation leaves and any code span their absolute locations point to.
- [x] Preserve source locations through preprocessing/evaluation so live playback can point back to the exact code that produced each hap. — The preprocessor wraps every string literal in `m("...", offset)` (`annotate_mini_offsets`), so per-hap `context.locations` are absolute byte offsets into the editor source rather than offsets relative to each mini string. The raw text is remembered on the pattern so raw-string consumers still work (see the `m(...)` plumbing). Tested by `per_hap_locations_are_absolute_to_source` and `locations_distinguish_multiple_source_strings`.
- [x] Add editor conveniences expected from Strudel's CodeMirror-based REPL, such as bracket matching, selection-aware commands, and completion/reference help. — Done: auto-pairing/closing of `()[]{}\"'`` and `` ` ``, auto-indent after newline inside brackets, selection-aware indent/outdent (Tab/Shift+Tab) and comment toggle, live bracket-match highlighting around the cursor (`bracket_match_spans`), jump-to-block (`Alt+w`/`Alt+q` move the cursor between `$` markers, `jump_to_marker`), and keyword autocomplete (`completion_at`: Tab/Enter accept, arrows navigate, Esc dismiss; candidates come from the generated reference surface).
- [~] Add a reference/autocomplete surface generated from this parity data and Strudel's `reference` package. — `rudel_lang::reference()` now generates the authoritative surface (top-level functions, pattern methods, control names) by introspecting the live Koto runtime, so it can't drift from what is actually exposed. The editor's keyword highlighting is driven by it (`RudelApp::build_highlight_idents`), and a `reference.rs` test guards the curated panel categories against it. The editor now has keyword autocomplete that consumes this surface (`completion_at`). Still missing: a diff against Strudel's `reference` package (see the Reference/Docs item).
- [x] Audit keyboard shortcuts against Strudel's REPL and document the supported subset. — Audited against `strudel/packages/codemirror`: Ctrl/Alt+Enter (eval), Ctrl/Alt+. (hush), Ctrl+/ and Ctrl+\ (comment), Tab/Shift+Tab (indent). Documented in `crates/rudel-app/README.md`; `Alt+w`/`Alt+q` block-jump and per-block eval are noted as not yet supported.
- [~] Match `codemirror` package behavior: autocomplete, highlight, flash, widgets, sliders, labels, block utilities, tooltips, keybindings, themes, and HTML helpers. — Highlight, active-event flash, keyword autocomplete, and the transport/edit keybindings are matched; widgets, sliders, labels, block utilities, tooltips, themes, and HTML helpers are not (widgets/sliders deferred above).
- [~] Match `repl/repl-component.mjs`, `repl/prebake.mjs`, and `repl/index.mjs` user-visible behavior. — The native app covers the core REPL loop (evaluate, hush, transport, output routing, sample prebake-style loading); web-specific component/embedding behavior is out of scope or not yet ported.
- [x] Match code evaluation semantics: update while playing, hush, multiple outputs, output routing, error reporting, user-defined state, and reset behavior. — Done: update-while-playing (re-eval re-routes the live pattern), hush (Ctrl/Alt+.), panic/reset (Ctrl+Shift+. stops and tears down the MIDI/OSC back-ends so stuck notes get an all-notes-off — `RudelApp::panic`), multiple outputs and output routing (audio/MIDI/OSC defaults plus per-pattern `.midi()`/`.osc()` tags), and error reporting (errors panel). Intentionally different: there is no persistent user-defined state across evals because Rudel re-evaluates the whole editor buffer each time (the buffer *is* the state), rather than Strudel's block-based REPL that accumulates `userDefinedKeys` across separate evaluations — the same reason the `blockBased` machinery is marked not-applicable in the transpiler item.
- [~] Add tests or scripted UI checks for editor shortcuts, inline controls, inline visual feedback, active-event highlighting, and live update behavior. — Unit tests cover comment toggle, indent/outdent, auto-pair, auto-indent, the highlighter tokenizer, active-span overlap, and the absolute source offsets that drive active highlighting. Still untested: inline controls, inline visual feedback, and end-to-end live-update behaviour through the running egui app (several depend on deferred features).

## Visuals, Draw, Motion, and External Inputs

- [ ] Match `draw` package behavior: pianoroll, pitchwheel, spiral, color, draw, and animate.
- [ ] Match Hydra integration or document it as intentionally unsupported.
- [ ] Match motion/device-motion input package behavior or document it as intentionally unsupported.
- [ ] Match gamepad input package behavior or document it as intentionally unsupported.
- [ ] Match serial and MQTT packages or document them as intentionally unsupported.
- [ ] Match `csound` package behavior or document it as intentionally unsupported.
- [ ] Match `tidal`, `mondo`, and `mondough` packages or document them as intentionally unsupported.
- [ ] Match `web` and `embed` package user-visible embedding behavior where Rudel has an equivalent surface.

## Reference, Docs, and Examples

- [~] Generate a complete reference table from Strudel's `reference` package and compare it with Rudel's exposed names. — Rudel's side is generated and introspectable via `rudel_lang::reference()` (functions, methods, controls). Strudel's side is its `reference` package, which re-exports the jsdoc `doc.json` built by Node tooling; that table is not checked in, so the automated diff still needs a build step (or a source-scan of `registerControl`/`register` calls) to produce the comparison.
- [ ] Port or execute every runnable example from Strudel docs against Rudel.
- [ ] Add a parity example suite covering first sounds, notes, effects, pattern effects, mini-notation, tonal, xen, MIDI, OSC, samples, synths, and visual feedback.
- [ ] Keep `FULL_STRUDEL.md` in sync with generated API inventories so manual drift is obvious.
- [ ] Document every unsupported or intentionally different feature in user-facing docs.

## Test Strategy

- [ ] Add a Strudel differential harness that can query local Strudel patterns through Node and compare Rudel haps for deterministic examples.
- [ ] Add golden tests for mini-notation ASTs, event spans, values, controls, and source locations.
- [ ] Add property tests for core time transforms where exact Strudel behavior can be expressed generically.
- [ ] Add snapshot tests for reference docs/autocomplete output.
- [ ] Add audio smoke tests for WebAudio/superdough-equivalent paths.
- [ ] Add MIDI/OSC loopback tests.
- [ ] Add editor automation tests for syntax highlighting, widgets, shortcuts, active highlights, and live-code updates.
- [ ] Add performance benchmarks against representative Strudel patterns.
- [ ] Add regression tests for every bug found while working through this checklist.

## Migration and Maintenance

- [ ] Add a script to regenerate the controls/API checklist from `strudel/packages` so `FULL_STRUDEL.md` can be audited after Strudel updates.
- [ ] Add a documented process for updating Rudel when the vendored Strudel version changes.
- [ ] Check licensing requirements for any code, data, samples, or generated tables ported from Strudel.
