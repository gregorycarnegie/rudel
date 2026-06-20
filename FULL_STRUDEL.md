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
- [x] `strudel/packages/xen`
- [x] `strudel/packages/tonal`
- [x] `strudel/packages/edo`
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

- [x] Match Strudel's `Pattern`, `Hap`, `State`, `TimeSpan`, `Fraction`, and `Value` data model semantics. Each class is a faithful port. **`TimeSpan`** (`timespan.rs`): `span_cycles`/`duration`/`cycle_arc`/`with_time`/`with_end`/`with_cycle`/`intersection`/`intersection_e`/`midpoint`/`==`/`Display` — including the zero-width-span and point-intersection edge cases (proptested). **`State`** (`state.rs`): `span` + `controls` with `set_span`/`with_span`/`with_controls`. **`Hap`** (`hap.rs`): `whole`/`part`/`value`/`context`, `whole_or_part`, `with_span` (maps `part` and, when present, `whole`), `with_value`, `has_onset`, `span_equals` (two continuous haps compare equal), `combine_context`/`set_context`/`with_context`, plus the event-clipping pair `clipped_duration` (Strudel's `Hap.duration` getter) and `end_clipped` added here; new unit tests pin `has_onset`/`span_equals`/`with_span`/`end_clipped`. **`Fraction`** (`fraction.rs`, `Ratio<i128>`): `sam`/`next_sam`/`cycle_pos`/`floor`/`ceil`/`min`/`max`/`abs`/`gcd`/`lcm` (rational gcd/lcm), `Display` matching `show` (`n/d`), the `lt`/`gt`/`eq`/… comparators provided free by `Ord`, and `lcm_opt`/`gcd_opt` matching `fraction.mjs`'s undefined-poisoning `lcm`/`gcd`; `whole_cycle` exists as a free function (`pattern.rs`). **`Value`** (`value.rs`): the union-arithmetic surface owned by the `core/value.mjs` item above. Intentionally different: `Frac::from_f64` quantizes to a 1µ-cycle grid instead of fraction.js's Farey-sequence analysis (exact `f64` fractions have denominator 2^52 and overflow under pattern arithmetic — mini-notation ratios are parsed exactly via `Frac::new`, so this only affects bare `f64` args); the JS-idiom `Fraction.prototype` helpers `mulmaybe`/`divmaybe`/`addmaybe`/`submaybe`/`or`/`maximum` are expressed natively in Rust (`?`, `Option` combinators, `lcm_opt`, iterators) rather than as methods; `Hap.stateful`/`resolveState`, `hasTag`/`context.tags`, and `ensureObjectValue`'s throw are not carried (no stateful-function haps, no tags field — cross-referenced from the pattern.mjs audit), and the `isActive`/`isInPast`/`isInFuture`/`isWithinTime` scheduler-timing predicates belong to the scheduler item, not the data model. Hap ordering uses a stable `part.begin` sort rather than Strudel's display-only `sortHapsByPart` (`part.begin`→`part.end`→`whole.begin`→`whole.end`); the parity harness re-sorts by the full serialized hap, so this is not observable.
- [x] Match query semantics including `query`, `queryArc`, whole spans, part spans, event splitting, event clipping, and source locations. The query core is a faithful port: `Pattern::query`/`query_arc` (= Strudel's `query`/`queryArc`), `State`/`TimeSpan` with `span_cycles`/`split_queries` for cycle-boundary **event splitting**, `Hap.whole`/`part` with `has_onset`/`whole_or_part`, and the `app_whole`/`app_left`/`app_right`/`bind_whole` family that builds **part spans** by `TimeSpan::intersection` (fragments) while choosing **whole spans** per combinator — all verified hap-for-hap by the mini and transform parity oracles. **Source locations** are carried in `Hap.context.locations`, accumulated through every combinator and tagged by `with_loc`/the `pure_loc` fast-path (owned and golden-tested by the mini source-locations item above). The gap closed in this pass was **event clipping**: Strudel's `Hap.duration` getter applies a numeric `duration` control (overrides the whole's length) and a numeric `clip` control (multiplies it) to decide an event's *sounding* length, which feeds the scheduler/synth seconds-duration. Added `Hap::clipped_duration` mirroring that getter (numeric-only guard matching `typeof === 'number'`, so a string/boolean control is ignored) and routed `query_controls`' `duration_seconds` through it, while leaving the structural `Hap::duration` (= Strudel's `hap.whole.duration`) for `splice`/`fit`. Also fixed `legato`: Strudel registers it as an alias of `clip` (`registerControl('clip', 'legato')`), so `.legato(x)` must write the canonical `clip` key — it was a separate `legato`-keyed plain control in Rudel and so never clipped; it is now an alias of `clip`. Unit-tested in `hap.rs` (clip multiply, `duration` override then clip, non-numeric ignored, structural vs clipped) and `query.rs` (clip/legato/`duration` shaping `duration_seconds`, onsets unchanged). Intentionally different: `queryArc`'s `try/catch`-to-`[]` error swallowing is not ported (Rust has no JS-exception equivalent in the `Send`/`Sync` query path; a malformed pattern panics rather than silently returning no haps), and the `stateful`/`resolveState` hap machinery is unused (Rudel has no stateful-function haps).
- [~] Audit and implement every export and `Pattern.prototype` method in `core/pattern.mjs`. The bulk of the surface is already implemented (the time/structure/math/higher-order/stepwise/euclid/pick families and the factories `stack`/`cat`/`seq`/`fastcat`/`slowcat`/`pure`/`silence`/`arrange`/`polymeter`). Added in this audit pass: the boolean COMPOSERS `lt`/`gt`/`lte`/`gte`/`eq`/`eqt`/`ne`/`net`/`and`/`or` (via `op_in`, `and`/`or` method-only as they are Koto keywords) and `keepif` (structure from the left, so it keeps the control value intact rather than merging — verified against Strudel on control patterns); `invert`/`inv`, `linger`, `replicate`, `applyN`; the `chunk`/`jux` variants `fastChunk`, `slowChunk` (=`chunk`), `juxFlip`/`flux`, `juxFlipBy`/`fluxBy`; the step-aligned stacks `stackLeft`/`stackRight`/`stackCentre` (pad shorter patterns to the longest's step count); and the aliases `sparsity` (=`slow`), `sequence` (=`seq`), `polyrhythm`/`pr` (=`stack`), `nothing` (=`silence`). Owned by other checklist items (cross-referenced, not duplicated here): the alignment matrix and `_opIn`/`_opOut`/… (pattern alignment item); `soft`/`hard`/`cubic`/`diode`/`asym`/`fold`/`sinefold`/`chebyshev`/`FX`/`worklet`/`partials`/`phases` (distortion/effects item); `band`/`bor`/`bxor`/`blshift`/`brshift` (bitwise composers, implemented under the signal item); `hsl`/`hsla` and `density` (controls — `density`'s control shadows the `fast` alias upstream); `cpm`/`hush`/`reset`/`restart`/`ref` (REPL/impure items). Intentionally different (Koto VM can't run in the `Send`/`Sync` query path): the raw bind/join family (`bind`/`innerBind`/`outerBind`/`squeezeBind`/`polyBind`/`stepBind`/`stepJoin`) and `withValue` are not exposed standalone (the high-level combinators, `fmap`/`arpWith` probing, and the `pick` family cover the reachable cases); `shrinklist`/`s_taperlist` stay internal. The span-argument variants `compressSpan`/`focusSpan`/`zoomArc` are intentionally not exposed: in Strudel they take a `TimeSpan` *object* (`.begin`/`.end`) and throw on a plain array, so they are internal helpers — the user-facing two-arg `compress`/`focus`/`zoom` already exist. Also added: `echoWith`/`stutWith` (indexed delayed copies — a new `Callback::apply2` passes `(copy, index)` and falls back to a one-arg call since Koto is strict about arity); `bypass` (per-cycle mute); and `plyWith`/`plyForEach` (repeat each event `factor` times, transforming the copies — their callback runs per value in the query path, so the per-value copies are probed and baked like `arp_with`). And `into` (break a pattern into looped subcycles per a binary pattern, applying a transform — the user-facing `sound(...).into("1 0", f)`) plus `chunkInto`/`chunkBackInto` built on it (the transformed ribbons are probed and baked). Also `stackBy(mode, …)` dispatches by mode name (`left`/`right`/`centre`/`expand`/`repeat`) to the stack aligners (the mode is taken as a constant string rather than patternified). The `arpWith` camelCase method alias and its standalone form are now exposed (the probe-based chord arpeggiation core is shared). Added in a later pass: `beat` (structure from cycle divisions — `s("bd").beat("0,7,10", 16)`; the literal `pat.fmap(x => pure(x).compress(t/div,(t+1)/div)).innerJoin()` with `t`/`div` patternified, so a mini stack of positions stacks beats), `morph` (morph between two binary-rhythm *lists* by a 0→1 pattern, porting `_morph`'s position interpolation into a boolean structure pattern; `from`/`to` accept `[1,0,1,…]` arrays or `"1:0:1:…"` mini lists), and `xfade` (equal-plateau cross-fade that scales each side's `gain` and stacks — a pure pattern combinator, not DSP). `collect` was already implemented (groups congruent haps into a list-valued hap). Tested hap-for-hap (beat division placement, morph onset interpolation at `by` = 0/0.5/1, xfade complementary gains) plus Koto-binding tests. Still pending (so this stays `[~]`): `filter`/`filterWhen` evaluate a per-hap predicate **in the query path**, which the Koto VM cannot do (it isn't `Send`/`Sync` there — the same constraint behind eager callbacks); and `tag` (which writes `Hap.context.tags` for `filter` to read) is only useful with `filter`, so it is deferred with it.
- [x] Audit and implement every export in `core/euclid.mjs`, including aliases such as `euclidRot`, `euclidLegato`, `euclidLegatoRot`, `euclidish`, and `eish`. All in `rudel-core/src/euclid.rs`. `bjorklund` (the Bjorklund algorithm, with negative-pulse inversion), `euclid`, `euclidRot`/`euclidrot`, `euclidLegato`, and `euclidLegatoRot` were already present and tested (tresillo/cinquillo goldens, gapless-legato spans, late-offset rotation, plus proptests for length/pulse-count and inversion). Added the two missing exports: `bjork` (Tidal-style euclid taking a `[pulses, steps, rotation]` tuple, a lone number defaulting `steps = pulses`) and `euclidish`/`eish` (the Malcolm-Braff morph from straight euclidean at `perc = 0` to even pulse at `perc = 1`, porting `_morph`: each onset becomes a `true` hap of width `1/steps` at a position interpolated between its euclidean position `i/steps` and its even position `k/pulses`). Both are bound as methods (with the `eish` alias) and standalone factories (pattern last), verified hap-for-hap against current Strudel for static `perc` (0/0.25/0.5/1), a discrete pattern `perc`, and a *continuous-signal* `perc` (`sine.slow(8)`) across multiple cycles — exact in every case. `euclidish` mirrors Strudel's `register` patternification so a continuous `perc` is sampled once per cycle (`pulses`/`steps` are pure, giving per-cycle structure; `perc` is sampled by `appLeft` and then `innerJoin`ed) rather than once at the query start.
- [x] Audit and implement every export in `core/pick.mjs`, including `pick`, `pickmod`, `pickF`, `pickmodF`, `pickOut`, `pickmodOut`, `pickRestart`, `pickmodRestart`, `pickReset`, `pickmodReset`, `inhabit`, `pickSqueeze`, `inhabitmod`, `pickmodSqueeze`, and the standalone `squeeze`. All bound as methods and prelude factories, parity-tested against the oracle. Intentionally different: `pickF`/`pickmodF` apply their function lookups eagerly at construction (the Koto VM can't be driven from the query path), so functions of time-varying patterns are baked once — equivalent for the function lookups Strudel docs show.
- [x] Audit and implement every export in `core/signal.mjs`, including continuous signals (`saw`/`isaw`/`sine`/`cosine`/`square`/`tri`/`itri` and the `2` bipolar variants, `time`, `steady`), random signals (`rand`/`rand2`/`irand`/`brand`/`brandBy`/`perlin`/`berlin`/`randrun`/`run`/`scan`), seed behavior (`seed`/`withSeed` via a `randSeed` control that `rand` now honors), `choose`/`chooseIn`/`chooseOut`/`choose`/`choose2`, weighted choice (`wchoose`/`wchooseCycles`/`wrandcat`), `shuffle`, `scramble`, conditional transforms (`degrade*`/`sometimes*`/`someCycles*`/`often`/`rarely`/...), and `per`/`perCycle`/`cyclesPer`/`perx`. All bound as prelude factories/methods and golden-tested against the oracle (`tools/gen_parity_oracle.mjs`). Fixed a latent parity bug: rudel's `tri` was `fastcat(isaw, saw)` (Strudel's `itri`); it is now `fastcat(saw, isaw)`. Added in a later pass: the **bitwise composers** `band`/`bor`/`bxor`/`blshift`/`brshift` (`transforms/core.rs`, `op_in` with int32 value ops — Strudel's `numeralArgs` integer ops, with note-name→midi numeral parsing and JS's 5-bit shift masking), bound as pattern methods and pattern-last standalones, and the **binary generators** `binary`/`binaryN` (MSB-first bit patterns, the literal `n.segment(nBits).brshift(bitPos).band(1)` chain, so a patterned `n` is sampled per step), `binaryL`/`binaryNL` (the bits packed into a list value, per-value or fixed width), and `randL` (a continuous list of `n` legacy-RNG randoms). Verified by `signal.rs` unit tests (the int32 op semantics, `binary(5)` = `1 0 1`, `binaryN(55532,16)` matching Strudel's documented 16-bit example exactly, the list forms, and `randL` length/range) and Koto-binding tests. Intentionally different/unsupported: the `precise` murmur RNG and `useRNG` are not ported (the legacy RNG is Strudel's default and is bit-exact); `mousex`/`mousey` and the keyboard signals (`keyDown`/`whenKey`) are browser-only `external_io`.
- [x] Audit and implement every export in `core/value.mjs`. The user-reachable export is `unionWithObj`, which powers control arithmetic via `_composeOp` — Rudel's analog is `Value::union_with` (called by `compose_op` in `transforms/core.rs`). Implemented the missing issue #1026 guard: combining a control map with a bare scalar (wrapped to `{value: x}`) is now refused, returning the control unchanged, so `n("0 2 4").add(7)` is a no-op and you must write `n("0 2 4").add(n(7))` — verified hap-for-hap against current Strudel, including the asymmetry (the guard checks the right operand only, so a scalar on the left still unions, e.g. `add(n("10"), "0 2")` → `{value:0, n:10}`). Intentionally different/not ported: the `Value` Maybe/functor class and its `valued`/`mul`/`map`/`.ap`/`.map`/`.mul`/`.unionWith` helpers are an internal JS applicative abstraction never used by pattern code (only `unionWithObj` is, from `pattern.mjs`); Rudel patternifies with Rust closures (`ValueFn`) and `Value::union_with` directly, so there is no monad wrapper to expose. Strudel logs `[warn]: Can't do arithmetic on control pattern.` on the guarded path; rudel-core has no logger, so the no-op pass-through is the only observable effect.
- [x] Audit and implement every public utility from `core/util.mjs` that Strudel users can reach from the REPL. Audited the whole module: the great majority of `util.mjs` is internal plumbing that is never meaningfully called from user code and is realized natively in Rust rather than exposed — the registration/functional helpers (`curry`, `pipe`, `compose`, `id`, `constant`, `flatten`, `removeUndefineds`, `zipWith`, `pairs`, `splitAt`, `mapArgs`, `numeralArgs`/`parseNumeral`, `fractionalArgs`/`parseFractional`, `listRange`, `rotate`, `objectMap`), the dedup/sort helpers (`uniq`/`uniqsort`/`uniqsortr`), the code-hashing helpers (`unicodeToBase64`/`base64ToUnicode`/`code2hash`/`hash2code`), the scheduler/clock utilities (`ClockCollator`, `getPerformanceTimeSeconds`, `getEventOffsetMs`, `cycleToSeconds` — covered by the scheduler item's `Clock`), the browser-only keyboard helpers (`keyAlias`/`getCurrentKeyboardState`), the value-render helpers (`stringifyValues`/`getPlayableNoteValue`/`getFrequency`/`valueToMidi`/`getSoundIndex`/`nanFallback`), and the note predicates/tokenizers (`isNote`/`isNoteWithOctave`/`tokenizeNote`/`getAccidentalsOffset`) which exist inside `rudel-core` (`tonal.rs`) but as internal helpers. What a user actually reaches in the REPL are the scalar conversion/clamp helpers; these were missing as standalone Koto functions (only the deprecated `getFreq` was bound) and are now exposed in the prelude: `midiToFreq`, `freqToMidi`, `noteToMidi` (`note`, `defaultOctave = 3`; raises a Koto error on a non-note, matching Strudel's throw), and `clamp(num, min, max)` (`min(max(num,min),max)`), all delegating to `rudel-core` (`xen::{midi_to_freq,freq_to_midi}`, `tonal::note_to_midi_with_octave`) and tested in `tests/util.rs` (midi/freq round-trip, note-name parsing incl. default octave, non-note error, clamp bounds, and composition inside `note(...)`). Intentionally not exposed: `midi2note` and `getFreq`'s sibling are `@noAutocomplete`/deprecated upstream (`getFreq` is kept only because it was already bound); `_mod` (numeric modulo) is redundant with the patterned `mod`; and `sol2note`/the solmization tables are unused scaffolding in Strudel itself ("not used yet").
- [x] Match Strudel's currying, registration, alias, and method-chaining behavior. — Method-chaining and registration are matched: `kpattern_methods!` (`generated.rs`) registers every `Pattern.prototype` transform as a chainable KPattern method, and controls are registered from rudel-core's `control_builders` registry by `extend_control_entries` (the analog of Strudel's `register`). Aliases are matched: camelCase method aliases via the macro's `camel_*` groups, and control-name aliases via `control_name`. Standalone (curried-style) forms are provided for the full transform set — value-arg transforms by `register_pattern_fns!` and the higher-order combinators by `register_standalone_callbacks` — taking the pattern last, under both snake_case and Strudel's camelCase names (`fast(2, pat)`, `fastGap`, `iterBack`, `euclidRot`, `echo`, `stut`, `jux(rev, pat)`, `every`/`firstOf`/`chunk`/`juxBy`/`inside`/`off`/`when`/`within`/`someCycles`/…). Hap-for-hap equivalence with the method form is tested across every arg group, and a completeness guard asserts the standalone names are all registered. Intentionally different: Koto has no partial application, so only fully-applied standalone forms exist (no `fast(2)` returning a function, and callback args must be a function value like `rev` or `|x| x.fast(2)`).
- [x] Match Strudel's higher-order function behavior for callbacks, pattern-of-functions, and pattern-valued functions. **Callbacks** (`Pattern -> Pattern`): fully supported via the `Callback` plumbing — `every`/`firstOf`/`lastOf`/`chunk`/`chunkBack`/`jux`/`juxBy`/`superimpose`/`sometimes(By)`/`often`/`rarely`/`some(Cycles)(By)`/`off`/`within`/`inside`/`outside`/`apply`/`always`/`never`/`layer` all run a Koto function value (`rev`, `|x| x.fast(2)`). The VM isn't `Send`/`Sync`, so callbacks are applied **eagerly** at construction, not in the query path (as for `fmap`/`arpWith`, which probe). **Patterned arguments** now match Strudel for every combinator: `every`/`firstOf`/`lastOf` patternify their cycle count (`every("<2 4>", rev)`) via `every_pat`/`every_cycles` + `inner_join` (callback applied once, placed by a per-cycle count); `chunk`/`chunkBack`/`inside`/`outside`/`sometimesBy`/`juxBy`/`someCyclesBy` and `within`'s bounds patternify via `with_cb_*`/`probe_patternify`, which bakes the combinator result for each distinct argument value over a probe window and selects it per cycle with `inner_join` (mirroring `register`'s `arg.fmap(v => combinator(v,f,pat)).innerJoin()`); both the method and camelCase and standalone forms route through this, and scalar args keep a direct fast path. `off`'s time and the arithmetic operands patternify through the existing `late`/`appLeft` paths. All verified hap-for-hap against current Strudel (incl. randomized `sometimesBy("<0 1>", …)` and `juxBy`/`within` patterned bounds). **Boolean patterns**: a bare Koto `true`/`false` now reifies to `pure(Bool)` (`reify(true)`), fixing `when(true, …)`/`struct(true)`/`mask(true)`/`pat(true)` (previously silence). Intentionally different (Koto VM can't run in the `Send`/`Sync` query path): a **pattern-of-functions** as the transform argument (Strudel's `apply(slowcat(rev, fast2))`, function varying per cycle in the query path) is unsupported — callbacks bind to one eagerly-applied function; and the raw bind/join family (`bind`/`innerBind`/`outerBind`/`squeezeBind`/`polyBind`/`stepBind`) isn't exposed standalone for the same reason (the high-level combinators and the `pick` family cover the reachable cases). Patterned args use a 16-cycle probe window, so an argument value first appearing after cycle 16 falls back to silence (the same documented limit as `fmap`/`arpWith`).
- [x] Match Strudel's pattern alignment behavior across all in/out/mix/squeeze/reset/restart/poly variants. All eight alignments are implemented in `transforms/core.rs` (`op_in`/`op_out`/`op_mix`/`op_squeeze`/`op_squeeze_out`/`op_reset_impl` for reset+restart/`op_poly`, the last via `poly_join` = step-ratio `extend` + `outer_join`) and verified hap-for-hap against Strudel for every variant. The full `out`/`mix`/`squeeze`/`squeezeout`/`reset`/`restart`/`poly` matrix is now bound for all arithmetic and structural composers — `add`/`sub`/`mul`/`div`/`modulo`/`pow`/`set`/`keep` — under Strudel's flattened naming (`add_out`, `set_squeeze`, …; Koto can't do the `add.out` getter-object form). `squeezein`/`squeezeIn` alias `squeeze` (bare and per-operator), and the bare alignment methods `out`/`mix`/`squeeze`/`squeezeout`/`reset`/`restart`/`poly` default to the `set` op (Strudel's `pat.out(x) == pat.set.out(x)`). Intentionally different: the default `in` alignment is the plain method (`add`, not `add_in`), and `in` isn't a bare method since it is a Koto keyword; the boolean composers (`lt`/`gt`/…/`and`/`or`) and `keepif` expose only the default `in` alignment — Strudel generates the full matrix for them too, but the aligned forms of comparisons are effectively unused, so they are omitted rather than adding ~70 rarely-touched bindings.
- [x] Match Strudel's stepwise functions: `take`, `drop`, `expand`, `extend`, `contract`, `shrink`, `grow`, `tour`, `zip`, `pace`, `stepcat`, `timecat`, `stepalt`, and aliases (`timeCat`, `steps`, and the deprecated `s_*` family). Intentionally not exposed: `shrinklist`/`s_taperlist` (internal helper in Rudel).
- [x] Match Strudel's sample/time transforms: `chop`, `striate`, `loopAt`, `loopAtCps`, `slice`, `splice`, `fit`, `scrub`, and related helpers. All live in `rudel-core/src/samples.rs` (rewriting `begin`/`end`/`speed`/`unit`/`_slices` control keys; cps-dependent ones read `_cps` from the query state, defaulting to Strudel's `0.5`). `chop`/`striate`/`slice`/`splice`/`fit`/`loopAt`/`scrub` were already present and verified hap-for-hap against current Strudel (including `chop` nesting into an existing `begin`/`end` sub-range, `slice`'s `{begin,end,_slices,...o}` merge with the sound's keys winning, and `slice`'s split-point-list form). Added the missing members: `bite` (zoom-and-squeeze pattern slicing — `ipat.fmap(i => n => pat.zoom(i/n mod 1, +1/n)).appLeft(npat).squeezeJoin()`, bound as method + standalone `pat2` factory and guarded by the completeness test) and the deprecated `loopAtCps`/`loopatcps` (explicit-cps `loopAt`), plus the lowercase `loopat` alias. Intentionally different/not ported: `chew` does not exist in the vendored Strudel (`packages/core`), so there is nothing to match; for `slice`/`splice` with a *list* of split points Strudel stores the whole array in `_slices` (making `splice`'s `cps / _slices / dur` speed `NaN`), whereas Rudel stores the slice count (`len - 1`) so list-based `splice` yields a sane speed.
- [~] Match Strudel's distortion/worklet/effects pattern helpers such as `soft`, `hard`, `cubic`, `diode`, `asym`, `fold`, `sinefold`, `chebyshev`, `partials`, `phases`, `FX`, and `worklet`. The **waveshaping-distortion family** (`soft`/`hard`/`cubic`/`diode`/`asym`/`fold`/`sinefold`/`chebyshev`) is fully implemented end to end. The pattern helpers (`controls/multi.rs` `distort_shortcuts!`) mirror Strudel's `_distortWithAlg`: each maps its arg to `[amount, volume, algName]` and spreads it into the `distort`/`distortvol`/`distorttype` controls (method `pat.soft(2)` via the `pattern_arg` group + pattern-last standalone `soft(2, pat)` via `pattern1`). `distort` itself was upgraded from a plain control to a multi-control (`distort`/`distortvol`/`distorttype`, registered in `EXTRA_CONTROL_BUILDERS`), so `.distort("3:0.5:diode")` splits positionally like Strudel's `registerControl(['distort','distortvol','distorttype'])`; `distortvol`/`distorttype`/`dist`/`distvol`/`disttype` keep their spellings. The DSP side (`rudel-dsp/postfx.rs`) ports superdough's `distortionAlgorithms` sample-for-sample into a `DistortAlgo` enum (the 8 algorithms plus the default `scurve`), resolved from `distorttype` by name or numeric index (wrapping, matching `getDistortionAlgorithm`); `PostFxVoice` applies `postgain * algo.shape(x, e^distort - 1)`, exactly superdough's `DistortProcessor`. Verified by DSP unit tests (per-algorithm reference formulas at zero drive, `hard` saturation, `fold` range/identity, 0→0 + finiteness for all, name/index resolution, and the voice selecting the algorithm) and Koto tests (shortcuts set amount/vol/type, `distort` colon-splitting, standalone≡method). **`partials`/`phases`** were already implemented as list-control helpers *and* rendered: the additive oscillator (`rudel-dsp/oscillator.rs` `waveformN`) builds a wavetable from the `partials` magnitudes over a base harmonic series with optional per-harmonic `phases`, matching Web Audio's `createPeriodicWave` normalization (covered by existing tests). Still pending (so this stays `[~]`): **`FX`** — the effects-*chain* helper that establishes an explicit, ordered effect graph (`.FX(phaser(...), bpf(...), distort(...))`) — is not ported; Rudel applies post-effects in a fixed order in `PostFxVoice`, so arbitrary per-call effect ordering needs an audio-graph rework that can't be verified without end-to-end audio. **`worklet`** stays intentionally unsupported (the kabelsalat audio-worklet DSL — same deferral as the `plugin-kabelsalat` transpiler item).
- [~] Match Strudel REPL pattern slots and aliases from `core/repl.mjs`, including `p`, `q`, `d1`-style slots, `p1`-style slots, `q1`-style silence helpers, `cpm`, stack behavior, and hush/update behavior. The user-visible pattern-slot surface is ported in `rudel-lang/src/bindings/pattern/repl.rs`: `p(id)` registers a pattern into a per-evaluation thread-local registry (Strudel's `pPatterns`) and tags it with its `id` control (mirroring `withState(setControls({id}))`); `d1`-`d9`/`p1`-`p9` are fixed-id shorthands for `p(i)`; `q`/`q1`-`q9` are silent (queued/muted) slots. `eval_with_samples` clears the registry on entry and, when any slot was registered, returns the **stack** of all registered slots instead of the script's own return value — exactly `applyPatternTransforms`'s "pPatterns non-empty → stack them" rule — so `note("c").d1()\nnote("e").d2()` stacks even though Koto only returns the last expression. Slot **muting** (`_x`/`x_` → silence) and anonymous `$`-suffix indexing match Strudel, and **`hush()`** clears the registry and returns silence. **`cpm`** is implemented in `rudel-core` (`samples.rs`) as a per-pattern tempo: it reads the live `_cps` from the query state and fast-es by `cpm/60/cps` (Strudel reads `scheduler.cps` at build time; Rudel reads the scheduler-set `_cps` per query, so it also tracks live tempo changes). All covered by `tests/repl.rs` (single/multi slot stacking + id tagging, `p`/`p1` ids, `q`/`q1` silence, `_`/`x_` muting, `hush`, per-eval isolation, and `cpm` fast-ing relative to cps). Intentionally different / still pending (so this stays `[~]`): `d1`/`p1` are **methods** (`pat.d1()`) rather than Strudel's no-arg property getters (`pat.d1`), since Koto host objects have no property getters; Rudel now has native current-block eval, but Strudel's persistent **block registry/update state** (`updateState`/`onUpdateState`, `codeBlocks`, `blockPatterns`, persistent cross-block declarations, anonymous-label restrictions, and label aggregation) is still deferred; and the REPL combiners `all`/`each` (apply a transform to the stacked / each registered pattern) and soloing (`S$:`) are not yet ported. The global tempo helpers `setcps`/`setcpm`/`setCps`/`setCpm` already work via the app's tempo-effect routing to `Engine::set_cps` (the continuous-re-anchoring scheduler item above).
- [~] Match Strudel scheduler behavior: CPS, latency, trigger timing, pattern replacement, and event deadlines. Rudel's scheduler lives in `rudel-audio` (`lib.rs` `scheduler_loop`/`Engine` + `events.rs`), structured like `core/{zyklus,cyclist}.mjs`: a lookahead scheduler thread queries the pattern ~100ms ahead of the audio sample-clock and feeds timed `NoteEvent`s to the audio-callback mixer, which starts each voice when its `onset_seconds` passes. **CPS**: default `0.5` (Strudel's), and live changes now match cyclist's re-anchoring — added a pure `Clock` (`clock.rs`) holding `(anchor_seconds, anchor_cycle, cps)` with `cycle_at`/`seconds_at`/`set_cps`; `set_cps` re-anchors at the current playhead (cyclist's `num_cycles_at_cps_change`/`seconds_at_cps_change`) so the cycle counter is *continuous* across a tempo change instead of jumping (it previously recomputed `seconds*cps` from the origin, so every `setcps`/UI-slider/MIDI-clock change snapped the playhead). A constant-cps clock anchored at the origin is byte-identical to the old `seconds*cps`, so the common path is unchanged. **Trigger timing**: onsets convert through the clock (`collect_events_at` → `clock.seconds_at(onset_cycle)`), so they stay correct after a re-anchor. **Pattern replacement**: `set_pattern` swaps an `RwLock<Pattern>` and the loop keeps its `scheduled_cycle` cursor, so a new pattern takes effect from the next window forward without re-querying already-scheduled time (matching cyclist querying from `lastEnd`). The window picker `next_schedule_window` was hardened for tempo changes: when a cps drop shrinks the cycle-lookahead so the cursor sits past the new window it schedules nothing and waits (no double-trigger), and when the scheduler stalls and the cursor falls behind it snaps forward to drop the backlog. All covered by `Clock` unit tests (continuity, inverse mapping, unchanged/invalid-rate no-ops) and `next_schedule_window` tests (continue / wait-when-ahead / snap-when-stale / live-cps-change). **Latency / event deadlines** — intentionally different mechanism: Strudel adds a fixed `latency` (0.1s) to each `targetTime` and passes a legacy `deadline = targetTime - phase` to `onTrigger`; Rudel instead schedules 0.1s of lookahead and the mixer's pending-event queue fires each voice sample-accurately at its onset, so there is no separate latency offset or deadline value (the lookahead is the scheduling headroom). Still pending (so this stays `[~]`): **per-hap `cps` tempo modulation** — Strudel's cyclist reads `hap.value.cps` from the query stream and changes tempo mid-pattern (`.cps("<0.5 1>")`); Rudel's `cps` is a control but the scheduler does not yet re-anchor the clock from scheduled events (only the eval-time `setcps()`/`setcpm()` helpers and the UI/MIDI clock drive `Engine::set_cps`), and correctly applying an in-stream cps change in the realtime path needs end-to-end audio verification. The CPS-source helpers `cpm`/`setcps` REPL plumbing belong to the REPL item below.
- [x] Match reset/timeline/impure behavior from `core/impure.mjs` where it is user-visible. `impure.mjs`'s entire surface is `timeline` plus `reset_state`/`reset_timelines`, now ported in `rudel-core/src/impure.rs`. **`timeline`** (method `pat.timeline(tpat)` and standalone `timeline(tpat, pat)`, bound via the `pattern_arg`/`pattern1` groups) carries cross-query state for live-coding cue alignment: a timeline-id pattern selects a per-id cycle offset — id `0` plays unshifted; a fresh id captures the begin of the cycle it first appears in (or the cycle *end* when first seen past the halfway point, so a just-typed timeline starts cleanly), and `pat.late(offset)` realigns from there; the offset persists until reset, and negating an id (`-2` vs `2`) drops the opposite-signed entry to reset it. State lives in a process-global `LazyLock<RwLock<HashMap<Frac, Frac>>>` (the same singleton pattern as the MIDI CC-in bus), keyed by the timeline id as a `Frac`. Crucially the offset is only *mutated* on scheduler queries: `query_controls` now tags the back-end/trigger query state with a `cyclist` control (matching Strudel's cyclist, which sets `cyclist: 'cyclist'`), and `timeline` only writes when that marker is present, so visualiser/`queryArc` reads observe but never advance the state — verified hap-for-hap in `impure.rs` tests (id-0 unshifted, activation-cycle offset capture and persistence, no-mutation on non-scheduler queries, negation reset) plus a Koto binding test. **`reset_timelines`/`reset_state`** are exported from `rudel-core` for the REPL to call on reset/hush. Intentionally different: timeline ids are read as `Frac` (Strudel coerces the number to an object key), so a non-numeric id collapses to `0` rather than acting as a distinct string key — timeline ids are documented as numbers. The `reset`/`hush`/`restart` REPL *helpers* that would call `reset_state` are part of the `core/repl.mjs` item below; the impure machinery they drive is in place here.
- [x] Match speech support from `core/speak.mjs` or document it as intentionally unsupported. **Intentionally unsupported.** `speak.mjs` is a thin wrapper over the browser Web Speech API: it reads `window.speechSynthesis`, builds a `SpeechSynthesisUtterance`, picks a voice by language/index, and speaks it from `pat.onTrigger((hap) => triggerSpeech(hap.value, lang, voice))`. Both halves are unavailable in Rudel: there is no browser (`window`/`speechSynthesis`/`SpeechSynthesisUtterance` don't exist in the native egui app), and the per-hap `onTrigger` mechanism — a user JS function invoked in the scheduler's trigger path — has no analog because Rudel's query/scheduler path is `Send`/`Sync` and cannot drive the Koto VM (the same constraint that makes callbacks eager rather than query-time, noted on the higher-order-function and pattern-engine items). So `speak`/`onTrigger` are out of scope rather than missing parity; a native text-to-speech backend (e.g. an OS TTS crate) could be added later as a Rudel-specific extension, but it would not be a port of this browser-only module.

## Mini-Notation and Transpilation

- [x] Match `mini/krill.pegjs` grammar behavior: greedy step tokens (letters, digits, `~ - # . ^ _`), JS `Number()` atom classification (`1e3`, `0x10`, `.5`, `-x`), `~`/`-` rests, `^` steps marker, adjacency rules for `@`/`!`/`?` amounts, and `slice_with_ops` euclid args (ops parsed but discarded, as in mini.mjs). Intentionally different: the experimental "haskellish" operator/command layer at the bottom of krill.pegjs (`cat [...]`, `setcps`, `hush`, reached only via the `h()` API) is not implemented; a lone `^` atom is a parse error in Rudel instead of the atom `"^"`.
- [x] Match `mini/krill-parser.js` output for all upstream mini tests: every deterministic case from `mini/test/mini.test.mjs` (plus tokenizer edge cases) is golden-tested hap-for-hap against Strudel's real parser via `tools/oracle/gen_mini_oracle.mjs`, including `_steps`. Upstream's statistical PRNG tests are covered more strongly by exact PRNG-parity goldens (`?`/`|` use per-occurrence seeds offsetting `rand` by `0.0003 * seed`, matching krill's `seed++` order).
- [x] Match `mini/mini.mjs` APIs: `mini` ≈ `rudel_mini::parse`, `m` ≈ `parse_with_offset`, `getLeafLocation`/`getLeafLocations` ≈ `leaf_locations` (golden-tested against Strudel for every oracle pattern), `minify` ≈ `parse_or_silence`/`IntoPattern`, `miniAllStrings` ≈ `install`. Intentionally different: `mini2ast`/`getLeaves` (raw krill AST objects) are internal to the pest parser, and `h` (haskellish parsing) is unsupported with the rest of that layer. Locations are byte offsets into the bare pattern string (Strudel's are quoted-string offsets, i.e. +1).
- [x] Preserve source locations from mini-notation leaves through Rudel patterns: atoms are tagged with `Pattern::with_loc`, hap contexts accumulate locations through all combinators (including op arguments via the `pure_loc` patternify fast-path, matching Strudel's `__pure_loc`), and per-hap locations are golden-tested hap-for-hap against Strudel across the whole mini oracle.
- [x] Match mini-notation parser edge cases: nesting, alternation, Euclidean syntax (incl. patterned args `a(<3 5>,<8 16>)` via appLeft+innerJoin), polymeter syntax (incl. patterned `%<2 3>` and weighted sequences), ratios, weights, rests, holds, lists, subdivisions, repetition (krill's accumulate/move-to-end op semantics for `!`, `_repeatCycles`-based), degradation, randomness, and patterned ranges (`<0 1> .. <2 4>` via squeezeBind). Source offsets are tracked by the source-locations item above.
- [x] Match `transpiler/transpiler.mjs` behavior. Rudel runs Koto rather than JavaScript, so the analog is the source pass in `rudel-lang/src/preprocess.rs` (`preprocess_strudel`), not an Acorn/escodegen AST walk. Equivalents: label statements `x: expr` (Strudel's `labelToP` → `.p('x')`) are collected by `rewrite_labels` into `rudel_label("x", expr)` and stacked, and `$:` is supported as an anonymous label (golden-tested in `per_pattern_naming_and_mute`); Strudel's "add `return` to the last statement" is unnecessary because Koto already yields the last expression; the empty-body → `silence` fallback is matched (an empty or fully commented-out script preprocesses to `silence()`). Intentionally different/not ported: the `blockBased` REPL machinery (`strudelScope`/`globalThis`/`userDefinedKeys` cross-block persistence, `all()` block detection, widget/visualizer subtree scanning) is a CodeMirror-REPL concern that does not apply to Rudel's egui app; `// mini-off`/`// mini-on` disable ranges are unimplemented because Rudel does not blanket-convert every double-quoted string to mini-notation (see below), so there is nothing to selectively disable.
- [x] Match `transpiler/plugin-mini.mjs` double-quoted mini-notation transformation. Intentionally different mechanism, same user-visible result: Strudel rewrites *every* double-quoted (and backtick) string into an `m(value, offset)` call up front, whereas Rudel parses strings as mini-notation lazily and contextually — pattern-typed function arguments run through `arg_to_pattern`/`rudel_mini::parse`, and a bare string that is the target of a method chain (`"0 1".fast(2)`) is wrapped into `pat("0 1")` by `rewrite_string_method_chains`. Source locations from those strings are preserved per-hap (covered by the mini source-locations item above). Not ported: the tagged-template language layer (`mini`/`tidal`/`minilang` registration, `registerLanguage`) and backtick template literals, since Koto has no tagged templates; `mini2ast`/`getLeaves` remain pest-internal (noted above).
- [~] Match `transpiler/plugin-widgets.mjs` inline widget transformation. Previously documented as intentionally unsupported because Rudel had only an egui text editor; now planned as a native StrudelMirror-compatible editor surface. Source of truth: `plugin-widgets.mjs` rewrites `slider(value, min?, max?, step?)` to `sliderWithID(id, value, min?, max?, step?)`, where `id` is the first argument's source range (`from:to`), and emits `widgets` metadata; registered widget methods (`_pianoroll`, `_scope`, `_pitchwheel`, `_spectrum`, `_punchcard`, `_spiral`, `_wordfall`) are rewritten by injecting a stable widget id built from REPL id, widget type, per-type index, and source range. Rudel now has full-source slider scanning/rewrite, eval metadata for slider configs, a query-time live slider registry, and visual widget method scanning/rewrite; it still needs the native editor widget/drawing host; see the Native StrudelMirror plan below.
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
- [~] Implement `modulate(type, config, id)`, `lfo(config, id)`, `env(config, id)`, and `bmod(config, id)` behavior. The deterministic **LFO modulation source** is ported and golden-tested: `rudel-dsp/src/modulator.rs` ports superdough's `lfo-processor` AudioWorklet — the `waveshape` table (tri/sine/ramp/saw/square/sawblep, with `custom` deferred as it needs break-point arrays the scalar path can't carry) and the per-sample `Lfo` loop (phase init `ffrac(time*frequency + phaseoffset)`, `(shape + dcoffset) * depth`, `pow(curve)`, min/max clamp) plus `getLfo`'s defaults. Verified **sample-for-sample** against the real worklet across 11 cases (every shape plus skew, depth/dc, phase offset, clamping, and the exponential curve) — `crates/rudel-dsp/tests/lfo_golden.rs` + `tools/oracle/gen_lfo_oracle.mjs`. Still pending (so this stays `[~]`): the user-facing `modulate`/`lfo`/`env`/`bmod` **Pattern methods** (which build the nested `{lfo:{id:{control,rate,depth,…}}}` modulator descriptor in the hap value, incl. the "default to the previous control in the chain" rule and id tracking), the **envelope** and **bus** modulator sources, and the **per-voice control-target routing** that binds a source to a node's param across superdough's modulatable-control matrix (`connectLFO`/`connectEnvelope`/`connectBusModulator` — a Web Audio graph concern with no current rudel analog). The dedicated per-effect LFOs (`tremolo`, filter `lpenv`, …) still cover the common cases meanwhile.
- [x] Verify alias canonicalization matches Strudel's `getControlName`: `rudel_core::control_name` resolves every alias by probing the macro-generated builder registry, with parity tests.

## Xenharmonic, Tonal, and EDO

A tonal/xen parity oracle was added in this pass: `tools/oracle/gen_tonal_oracle.mjs` dumps pitch output from the real `@strudel/tonal` + `@strudel/xen` + `@strudel/edo` engines (59 labelled cases covering transpose/scale/scaleTranspose, xen/edo/withBase/ftrans/tuning/tune, voicing/rootNotes, edoScale, and the xenharmonic doc examples) and `crates/rudel-mini/tests/tonal_parity.rs` rebuilds each with rudel-core and compares hap timing exactly and pitch within tolerance. Separately, `tools/oracle/gen_tune_table_oracle.mjs` + `crates/rudel-mini/tests/tune_table_parity.rs` verify the whole tune.js scale archive (3304 scales) hap-for-hap. Note-name values are normalised to MIDI on both sides — rudel intentionally emits MIDI note **numbers** where Strudel emits enharmonic note-name **strings** (and rudel-core inlines the scale/chord/interval/voicing tables instead of depending on `@tonaljs/tonal`/`chord-voicings`), so the comparison is musical-pitch equivalence, not string identity.

- [x] Match every export in `xen/xen.mjs`, including `edo`, `xen`, `withBase`, `ftrans`, `fTrans`, `ftranspose`, `fTranspose`, and `tuning`. All in `rudel-core/src/xen.rs`, bound as Koto methods. `edo` (= `edo_ratios`), `xen` (EDO names like `31edo`, the `12ji` preset, Tune.js scale names, and explicit ratio lists; tags `edoSize` on the hap context for EDO scales), `withBase`/`with_base` (rescale by `base` or `[base, originalBase]`), and the `ftrans`/`fTrans`/`ftranspose`/`fTranspose` family (EDO-step frequency transpose, reading `edoSize` from context then defaulting to 12, accepting `[steps, edo]` or the mini `step:edo` list form) were already present; added `tuning` (the proto-`xen` registered-but-not-exported in `xen.mjs`: reads the bare value as the scale index and returns the raw ratio with no `i` control and no 220Hz base). Oracle-verified hap-for-hap for `xen` (31edo/12edo/ratio-list/12ji/hexany15/negative-step/`<5edo 12edo>` alternation), `withBase` (scalar + pair), `ftrans` (context-edo / explicit `[7,31]` / default-12 / `1:12` mini list / `<8 -8>` alternation), and `tuning`. Intentionally different: `precise`/murmur-RNG and browser-only tuning UI are out of scope; rudel emits MIDI/freq numbers, not note-name strings.
- [x] Match every export in `xen/tune.mjs`, including named scale lookup behavior and `tunejs` scale compatibility. `tune` (Tune.js lookup) accepts a named archive scale (e.g. `hexany15`) or an explicit frequency list, returning the per-step **ratio** (tonic normalised to `1`, matching Strudel's `tune.tonicize(1)`); the named-scale table is the generated `tune_table.rs` ported from `tunejs.js`. Oracle-verified hap-for-hap for `tune("hexany15")` and `tune([261.6…, 302.7…, 350.3…])`. Intentionally different: a `tune` call on a value without an `i`/object control yields silence rather than Strudel's thrown error (rudel's query path is `Send`/`Sync` and cannot throw user-visible exceptions).
- [x] Verify Rudel's generated tune table against `xen/tunejs.js` and upstream scale data. `crates/rudel-mini/tests/tune_table_parity.rs` rebuilds `tune(name)` for **every** scale in tune.js's archive (3304 scales, enumerated via `Tune.search('')`) and compares its per-degree ratios — degrees `0..length` including the octave — against tune.js's own runtime `tune.note()` output (tonic 1), within `1e-6`. This verifies both the generated `tune_table.rs` frequency data *and* rudel's ratio derivation (`ratios_from_frequencies`, matching tune.js's `loadScale` `freq[i]/freq[0]`) against the real engine for the whole archive. The golden is produced by `tools/oracle/gen_tune_table_oracle.mjs`; the table itself is regenerated from `tunejs.js` by `tools/generate_tune_table.py`, so it stays in sync by construction, and a unit test guards the sorted-for-binary-search invariant.
- [x] Match all examples from Strudel's xenharmonic docs. The oracle reconstructs the `xen`/`ftrans`/`withBase`/`tune` doc examples hap-for-hap: the 31edo minor triad `i("0 8 18").xen("31edo")` and its explicit-ratio equivalent, the mixed `<5edo 10edo 15edo hexany15>` scale alternation, `withBase("<220 [300 200]>")` over `hexany23`, `ftrans("<0 1:31 1:12>")` (the `step:edo` mini-list alternation) and `ftrans("<0 7:31 7>")` over a `freq` pattern, and named-scale `tune("tranh3")`. Intentionally not reproduced: the one `withBase` snippet that mini-parses bare decimal ratios (`[1/1,16/15,…].join(' ')`), since rudel quantises bare `f64` mini args to a 1µ-cycle grid (the documented `Frac::from_f64` difference) rather than carrying them exactly.
- [x] Match every export in `tonal/tonal.mjs`, including `transpose`, `trans`, `scaleTranspose`, `scaleTrans`, `strans`, and `scale`. All in `rudel-core/src/tonal.rs`. `transpose`/`trans` (numeric semitones or interval strings like `3M`/`-2M`/`5P`, per-event and patterned), `scale` (scale-degree→note, note→nearest-scale-note quantisation, root octaves, negative degrees, `#`/`b` degree accidentals, `:`-joined multi-word scale types, and a patterned scale name), and `scaleTranspose`/`scaleTrans`/`strans` (transpose within the tagged scale, patterned offsets) are oracle-verified hap-for-hap. Two quantisation parity bugs were fixed in this pass to match Strudel's `_getNearestScaleNote`: ties now resolve to the **higher** scale tone (`preferHigher`), and the octave is included as a wrap candidate (a note nearer the next root than the 7th quantises up). Also added Strudel's list-scale-argument handling (`Array.isArray(scale) → flat().join(' ')`) so a mini colon-list like `<C:major A:minor>` selects scales per cycle, and `parse_scale` now splits on whitespace as well as `:`. Intentionally different: rudel writes a MIDI `note` number rather than an enharmonic note-name string (so interval-string transposition preserves pitch but not spelling), and `scaleTranspose` without a prior `.scale(...)` is a no-op rather than a thrown error.
- [x] Match every export in `tonal/tonleiter.mjs`, including chord tokenization, pitch-class conversion, scale-step behavior, named scales, voicing rendering, and note transposition. Scale-step placement (`scaleStep`/`stepInNamedScale` via `scale_step`/`step_in_named_scale`, incl. the `anchor` realignment), named-scale intervals, note→MIDI/pitch-class conversion, chord tokenization (`tokenize_chord`/`chord_notes`/`chord_symbol`), and `renderVoicing` (the chord-voicing renderer with its `below`/`duck`/`above`/`root` anchor modes, `offset`, `n`-as-scale, and `octaves`) are all implemented in `tonal.rs`/`voicing.rs` and oracle/unit-tested. Intentionally different: the standalone diatonic `transpose(note, step)` helper and the `Step`/`Note` tokenizer objects are internal JS utilities (not Strudel user API), realized natively in Rust rather than exposed as separate functions; rudel emits MIDI numbers, not note-name strings.
- [x] Match every export in `tonal/voicings.mjs`, including dictionaries, aliases, ranges, `voicings`, `rootNotes`, and `voicing`. `voicing` (the recommended path) defaults to the `ireal` dictionary (Strudel's `defaultDict`), matching `renderVoicing` hap-for-hap across the `ireal`/`ireal-ext`/`lefthand`/`triads`/`guidetones`/`legacy` dictionaries plus the `anchor`/`mode`/`offset`/`n` controls (oracle-verified). The default `ireal`/`ireal-ext` dictionaries (the `simple`/`complex` iReal tables from `ireal.mjs`) are inlined into `voicing.rs`, generated from the real package **after** its `voicingAlias` side-effects (so the `^`/`-`/`+`/`M`/`m`/`aug` spellings are all present). `rootNotes` maps chords to their root in an octave. Fixed in this pass: every dictionary now voices below the `c5` anchor (not the registry's `a4`/`above`) — Strudel's `voicing` spreads the value's `undefined` `anchor`/`mode` controls *over* the registry entry, so they always fall back to `renderVoicing`'s `c5`/`below` defaults; the per-dict registry settings were dead code for this path. Intentionally different: the **deprecated** `voicings()` (lowercase, with `s`) uses the external `chord-voicings` package's smoothest-voice-leading (`dictionaryVoicing`/`minTopNoteDiff`/`lastVoicing` state and per-dictionary `range`); rudel's `voicings(dict)` instead aliases `voicing` with a named dictionary (no voice-leading state, so `ranges`/`setVoicingRange` are not carried). `addVoicings`/`registerVoicings` (runtime custom dictionaries) are not exposed — the built-in dictionaries cover the documented examples.
- [x] Match `tonal/ireal.mjs` simple and complex dictionaries. Both the `simple` and `complex` voicing dictionaries are inlined into `voicing.rs` (as `IREAL`/`IREAL_EXT`), generated directly from the real `ireal.mjs` after `@strudel/tonal`'s `voicingAlias` side-effects so the `^`/`-`/`+`/`M`/`m`/`aug` chord-symbol aliases are present. `simple` powers the default `voicing()` (`ireal`) and `complex` powers `.dict('ireal-ext')`, both oracle-verified hap-for-hap (`voicing_default`, `voicing_ireal_ext`). `ireal.mjs` exports nothing else (the dicts are its entire surface).
- [x] Match every export and test in `edo`, including intervals, ratios, pitch naming, and EDO scale behavior. The `edo` package's sole registered export, `edoScale` (MOS large/small-step scale notation, e.g. `C:LLsLLLs:2:1`), is ported in `rudel-core/src/edo.rs` and bound as `.edoScale(...)`. The construction-and-read path of the upstream `EdoScale`/`Intervals`/`Pitches`/`ratios` modules (originally robmckinnon's `pitfalls` Lua) is faithfully ported: the `L`/`M`/`s` step sequence and large/small/medium sizes build the per-step divisions and total `edivisions`; `Intervals` derives octave ratios and nearest-named interval labels from the 45-entry whole-number `ratios` table (`nearestInterval` within 1%); and `Pitches` produces frequencies/MIDI via `get_freq`/`midi_to_hz`/`hz_to_midi` with the `toFixed(3)`/`toFixed(4)` rounding and the `octdeg` octave-wrapping. `edoScale` maps a numeric scale-degree pattern to MIDI notes (bare values, incl. fractional MIDI for non-12 EDOs) or, for control maps, to a `freq` map carrying `degree`/`degreeIndexes`/`intLabels`/`root`/`edo`. Oracle-verified hap-for-hap (12-EDO C major, a 16-EDO 6-note scale, octave wrapping, the `freq` map form, and a per-cycle alternation of two definitions) plus a unit test pinning the full metadata (intLabels `[null,M2,M3,P4,P5,M6,T7,P8]`, degreeIndexes, `root` `130.8128`). Intentionally not ported: the interactive `change*`/`set*` mutators on `EdoScale` (UI-only, never reached by `edoScale`), the `intNoms`/`intRatios`/`uniqLabels`/FJS fields (used only by the upstream pitchwheel UI, not by `edoScale`'s output), and the legacy note-name passthrough is kept.

## Audio and Output Backends

- [ ] Match `webaudio/webaudio.mjs` output behavior, including oscillator fallback, sample/synth controls, `webaudioRepl`, and `Pattern.prototype.dough`.
- [ ] Match `webaudio/scope.mjs`, `webaudio/spectrum.mjs`, and `webaudio/supradough.mjs`, including `tscope`, `fscope`, `scope`, `spectrum`, and `supradough`.
- [~] Match `superdough` synthesis and sampler behavior, including wavetable, vowel, feedback delay, reverb generation, modulators, ZZFX, node pools, and worklet behavior. Implemented and tested so far: the additive **wavetable** (`partials`/`phases` → `waveformN` + Web Audio normalization, `oscillator.rs`), the **vowel** formant filter (`postfx.rs`), the waveshaping **distortion** algorithms (sample-for-sample vs superdough, `postfx.rs`), and **ZZFX** — `zzfx.rs` ports `zzfx_fork.mjs::buildSamples` (a `ZzfxSynth` + `build_samples`, with `ZzfxParams::from_controls` mirroring `getZZFX`'s control mapping and a `ZzfxVoice` playing the rendered buffer), wired into the voice dispatch for the `zzfx` and `z_<wave>` sound names. ZZFX is verified **sample-for-sample** against the real superdough across 14 cases (every wave shape plus slide, modulation, bit-crush, sample-delay, pitch-jump, noise-FM, tremolo/repeat, and decay) — see `crates/rudel-dsp/tests/zzfx_golden.rs` + `tools/oracle/gen_zzfx_oracle.mjs`. Still pending: superdough's **feedback delay** and **reverb generation** (the impulse-response reverb is generated from noise — not yet sample-matched), the generic **modulator** engine (`modulate`/`lfo`/`env`/`bmod`, shared with the controls item), **node pools** (a Web Audio resource-reuse concern with no rudel analog), and **worklet** behavior (kabelsalat/dsp-worklet, intentionally unsupported). Intentionally different: ZZFX's `zrand` randomness uses a native per-voice RNG rather than `Math.random()`, so it is non-reproducible (as it is upstream) and is excluded from the golden (all golden cases use `zrand = 0`).
- [ ] Match `supradough` behavior or document any intentional difference.
- [ ] Match `sampler` package behavior, including sample server expectations and remote/local sample resolution.
- [ ] Match `soundfonts` behavior, including font loading, GM map, and SoundFont playback.
- [ ] Match sample naming, bank resolution, sample indexes, sample URL schemes, and sample caching behavior.
- [ ] Match Strudel's gain, clipping, pan, envelope, effect, reverb, distortion, compressor, modulation, and bus semantics audibly enough for representative examples.
- [x] Add audio golden tests where deterministic output is possible, and smoke tests for real-time outputs where exact samples are not stable. **Deterministic node→Rust goldens** cover every superdough DSP path whose math is pure JS (so it runs in plain Node and is reproducible): ZZFX `build_samples` (`zzfx_golden.rs`, 14 cases, `gen_zzfx_oracle.mjs`), the LFO modulator source (`lfo_golden.rs`, `gen_lfo_oracle.mjs`), the **linear ADSR gain envelope** (`adsr_golden.rs`, `gen_adsr_oracle.mjs` — drives superdough's verbatim `getParamADSR(..., 0, 1, 0, duration, 'linear')` through a mock Web Audio param, samples the automation curve, and compares `adsr_value` across 9 cases incl. the `attack > duration` / `attack + decay > duration` cutoffs), and the **waveshaping distortion algorithms** (`distortion_golden.rs`, `gen_distortion_oracle.mjs` — all 9 of superdough's verbatim `distortionAlgorithms` over an (x, k) grid; realistic drive agrees <1e-5, extreme-drive `diode`/`asym` floor ~3e-4 from documented f32 cancellation). These sit alongside the reference-formula DSP tests for filters/drums/crush. **Real-time smoke tests** cover the paths superdough renders through Web Audio nodes (oscillators, biquad filters, drums, reverb, delay, phaser, vowel), which can't run in Node so their exact samples aren't reproducible: `rudel-dsp` `tests/voice.rs` (oscillator/noise/supersaw/FM-chain/additive-partials/pulse-width/noise-mix/pitch-envelope/vibrato/pan, plus the ADSR cut-decay note that finishes to silence), `tests/postfx.rs` (vowel/crush/coarse/distort/tremolo/phaser), and `rudel-audio` (stereo delay echo, reverb tail). Intentional limitation documented here: Web Audio-rendered nodes are validated by behavioral smoke tests rather than sample-for-sample goldens because `OscillatorNode`/`BiquadFilterNode`/etc. have no Node-runnable reference.

## MIDI, OSC, and Bridges

- [ ] Match `midi/midi.mjs` output behavior, including `Pattern.prototype.midi`, device selection, default maps, channel behavior, note-on/off timing, CC, NRPN, pitch bend, aftertouch, sysex, and MIDI clock commands.
- [ ] Match `midi/input.mjs`, including incoming MIDI signals and `ccin`-style behavior.
- [ ] Match `osc/osc.mjs` and `osc/superdirtoutput.js`, including control parsing, event timing, SuperDirt OSC address/value behavior, and target routing.
- [ ] Match `desktopbridge` MIDI, OSC, and logger bridges where Rudel exposes equivalent desktop behavior.
- [ ] Add integration tests with fake MIDI/OSC devices or loopback ports.

## Editor, REPL, and Live Coding UX

- [~] Add inline UI controls as code inputs in the editor, matching Strudel-style live widgets such as sliders/knobs/toggles embedded in pattern code. — Language-side widget support is now partially in place: `slider(...)` and registered visual widget methods are scanned/rewritten, widget metadata is emitted from eval, and `slider_with_id` reads a query-time live registry. The native editor now has CodeMirror-style range/decorator state, an anchored inline slider host that rewrites the source literal and updates the live registry while dragging, reusable per-id visual widget surfaces, native hap-driven painters for `_pianoroll`/`_punchcard`/`_wordfall`, `_pitchwheel`, and `_spiral`, and a settings panel for the CodeMirror-style editor compartments. Still pending: non-slider control types and the full Strudel visual option surface.
- [~] Support Strudel-style inline UI values as live pattern inputs, not just visual editor widgets. — Deferred with the widget work above.
- [~] Add inline animations/visuals in the editor so code can create or drive visual feedback directly. — The native widget host now creates/reuses editor-owned visual surfaces for registered visual widget calls, cleans up removed ids, tags widget branches with Strudel-style ids, and repaints `_pianoroll`/`_punchcard`/`_wordfall`, `_pitchwheel`, and `_spiral` from current pattern haps using the selected draw theme. `_scope`/`_spectrum`, arbitrary draw callbacks, analyzer-backed visuals, and the full option surface are still deferred to the draw/webaudio visual-port items below.
- [~] Support Strudel-style inline animation/visual outputs as first-class runtime effects. — Deferred with the inline-visual work above.
- [x] Add `Ctrl+\` comment/uncomment for the current line or current selection. — `crates/rudel-app/src/editor.rs` toggles `//` line comments over the cursor line or selection on `Ctrl+\` and on `Ctrl+/` (Strudel/CodeMirror's actual binding); selection bounds are preserved. Covered by `editor_toggles_line_comments`.
- [x] Add basic syntax highlighting for Rudel/Strudel-like code.
- [~] Upgrade syntax highlighting to Strudel/CodeMirror-grade highlighting, including richer token categories and mini-notation awareness. — `editor.rs::tokenize` now distinguishes keywords/factories/controls/signals, methods, numbers, strings, and comments, and tokenizes mini-notation inside string literals (words, numbers, rests `~`, and operators `*/!@<>[]{}(),.?:|%-` get distinct colors). The editor settings panel can switch between the native Strudel-dark and light palettes, and selected themes feed the native draw theme. Still missing vs CodeMirror: a real Lezer grammar, the full Strudel theme catalog, and bracket-depth coloring. Tests: `tokenizes_mini_notation_inside_strings`, `tokenizes_note_names_and_decimals_in_mini`, `highlights_keywords_methods_and_numbers_in_code`.
- [x] Add active-event highlighting for mini-notation and code spans while playback runs. — `app/panels.rs::active_source_spans` queries the live pattern at the audio clock's current position each frame and flashes the byte ranges of the active haps' source locations in the editor (`editor.rs`, background highlight under the overlapping tokens). Covers mini-notation leaves and any code span their absolute locations point to.
- [x] Preserve source locations through preprocessing/evaluation so live playback can point back to the exact code that produced each hap. — The preprocessor wraps every string literal in `m("...", offset)` (`annotate_mini_offsets`), so per-hap `context.locations` are absolute byte offsets into the editor source rather than offsets relative to each mini string. The raw text is remembered on the pattern so raw-string consumers still work (see the `m(...)` plumbing). Tested by `per_hap_locations_are_absolute_to_source` and `locations_distinguish_multiple_source_strings`.
- [x] Add editor conveniences expected from Strudel's CodeMirror-based REPL, such as bracket matching, selection-aware commands, and completion/reference help. — Done: auto-pairing/closing of `()[]{}\"'`` and `` ` ``, auto-indent after newline inside brackets, selection-aware indent/outdent (Tab/Shift+Tab) and comment toggle, live bracket-match highlighting around the cursor (`bracket_match_spans`), jump-to-block (`Alt+w`/`Alt+q` move the cursor between `$` markers, `jump_to_marker`), and keyword autocomplete (`completion_at`: Tab/Enter accept, arrows navigate, Esc dismiss; candidates come from the generated reference surface).
- [~] Add a reference/autocomplete surface generated from this parity data and Strudel's `reference` package. — `rudel_lang::reference()` now generates the authoritative surface (top-level functions, pattern methods, control names) by introspecting the live Koto runtime, so it can't drift from what is actually exposed. The editor's keyword highlighting is driven by it (`RudelApp::build_highlight_idents`), and a `reference.rs` test guards the curated panel categories against it. The editor autocomplete now consumes this surface for fallback function/method/control/keyword completions and uses Rudel's loaded sample names plus the native tonal tables for Strudel-style sound/bank/chord/scale/mode contexts. Still missing: a diff against Strudel's generated `doc.json` / `reference` package (see the Reference/Docs item).
- [x] Audit keyboard shortcuts against Strudel's REPL and document the supported subset. — Audited against `strudel/packages/codemirror`: Ctrl/Alt+Enter evaluates the full buffer by default and switches to current-block eval when block eval is enabled, Ctrl+Shift+Enter runs the opposite eval action, Ctrl/Alt+. hushes, Ctrl+Shift+. panics, Ctrl+/ and Ctrl+\ toggle comments, Tab/Shift+Tab indent/outdent when tab indentation is enabled, and Alt+w/Alt+q jump between `$` markers. Documented in `crates/rudel-app/README.md`.
- [~] Match `codemirror` package behavior: autocomplete, highlight, flash, widgets, sliders, labels, block utilities, tooltips, keybindings, themes, and HTML helpers. — Highlight, active-event flash, contextual autocomplete/tooltips, and the transport/edit keybindings are matched. Slider/visual-widget transpiler metadata and the slider live-value registry are now in `rudel-lang`; `rudel-app` has the native decoration/range state that CodeMirror would normally provide; native slider controls are hosted in the editor; visual widget calls create reusable editor-owned surfaces; blank-line block detection/range-aware block eval is implemented; the CodeMirror settings-compartment toggles now drive the native editor where egui supports them; and selected editor themes now feed a native draw theme for inline visual surfaces. Actual visual drawing, label metadata, full Strudel theme catalog, and HTML helpers remain pending.
- [~] Match `repl/repl-component.mjs`, `repl/prebake.mjs`, and `repl/index.mjs` user-visible behavior. — The native app covers the core REPL loop (evaluate, hush, transport, output routing, sample prebake-style loading); web-specific component/embedding behavior is out of scope or not yet ported.
- [x] Match code evaluation semantics: update while playing, hush, multiple outputs, output routing, error reporting, user-defined state, and reset behavior. — Done: update-while-playing (re-eval re-routes the live pattern), hush (Ctrl/Alt+.), panic/reset (Ctrl+Shift+. stops and tears down the MIDI/OSC back-ends so stuck notes get an all-notes-off — `RudelApp::panic`), multiple outputs and output routing (audio/MIDI/OSC defaults plus per-pattern `.midi()`/`.osc()` tags), and error reporting (errors panel). Intentionally different: there is no persistent user-defined state across evals because Rudel re-evaluates the whole editor buffer each time (the buffer *is* the state), rather than Strudel's block-based REPL that accumulates `userDefinedKeys` across separate evaluations — the same reason the `blockBased` machinery is marked not-applicable in the transpiler item.
- [~] Add tests or scripted UI checks for editor shortcuts, inline controls, inline visual feedback, active-event highlighting, and live update behavior. — Unit tests cover comment toggle, indent/outdent, auto-pair, auto-indent, autocomplete handler contexts, tooltip lookup, the highlighter tokenizer, active-span overlap, the absolute source offsets that drive active highlighting, decoration/range remapping, slider source/registry updates, and visual widget host lifecycle/reuse/removal. Still untested: scripted UI interaction with the running egui app, real inline visual drawing, and end-to-end live-update behaviour through a real window.

### Native StrudelMirror / CodeMirror Parity Plan

Rule for every item in this subsection: inspect the corresponding Strudel source before implementing (`strudel/packages/codemirror/*`, `strudel/packages/transpiler/plugin-widgets.mjs`, and, for visual widgets, `strudel/packages/draw/*` / `strudel/packages/webaudio/{scope,spectrum}.mjs`), then add either parity tests or a documented intentional difference.

- [x] Add an evaluation metadata object in `rudel-lang`, analogous to Strudel's `afterEval(options.meta)`: `eval_result` now returns a single `EvalResult { pattern, sample_effects, meta }`, while `eval` and `eval_with_samples` remain compatibility wrappers. `EvalMeta` carries mini/source locations, widget configs (including simple static option literals), label metadata, and cleanup hints; mini locations are populated from the existing string-literal `m(value, offset)` preprocessing pass, and widget/label/cleanup fields start empty until their specific feature items land. `RudelApp::evaluate` now stores the returned metadata for the editor/widget host. Tested by `preprocess_metadata_reports_mini_locations`, `eval_result_carries_editor_metadata`, and `eval_result_collects_sample_effects`.
- [x] Port `plugin-widgets.mjs` scanning/rewrite semantics for `slider(...)`: `preprocess_strudel_with_meta` now detects standalone `slider(...)` calls outside strings/comments and not as method calls, computes `from`/`to` from the first argument in the original editor source, builds the stable `from:to` id, collects `{ type: "slider", from, to, id, value, min, max, step }`, and rewrites to `slider_with_id(id, value, min?, max?, step?)`. The generated id string is intentionally skipped by the mini-notation annotation pass so mini source-location metadata stays clean. Covered by slider preprocessor/eval tests.
- [x] Port `plugin-widgets.mjs` registered widget method semantics: Rudel now maintains the registered visual widget method list (`_pianoroll`, `_punchcard`, `_spiral`, `_scope`, `_pitchwheel`, `_spectrum`, `_wordfall`), detects method calls outside strings/comments, collects `{ from, to, index, type, id, options }` with per-type indices, injects the stable Strudel-style widget id (`{base}_widget_{type}_{index}_{from}-{to}`) as the first argument, preserves simple static option literals (`bool`/number/string) for the native host, and keeps full-document ranges absolute. The scanner is structured with a `nodeOffset`/base-id context like Strudel's block-eval path, although Rudel has no range-eval caller until the later decoration/block-utility item lands. Covered by visual widget preprocessor/eval tests.
- [x] Add a query-time live-value registry for editor UI controls, mirroring Strudel's `sliderValues` + `ref(() => sliderValues[id])` model. `slider_with_id` / `sliderWithID` now sync the code value into a global registry at eval time and return a continuous signal that reads the current registered value during each query; `set_slider_value(id, value)` is the editor-facing hook for immediate slider-drag updates and refuses unknown ids, like Strudel's message handler. Covered by `slider_with_id_reads_live_registry_at_query_time`.
- [x] Add native editor decoration/range state equivalent to CodeMirror `StateField` + `DecorationSet`: `rudel-app/src/editor/decorations.rs` now stores slider decorations, visual widget decorations, mini/source-location ranges, and active flash ranges; maps all of them across editor text changes using byte-accurate replacement ranges; refreshes all decorations from full-document eval metadata; and exposes a range-aware update path that preserves outside-range decorations using Strudel's strict range-end preservation rule. `RudelApp` feeds eval metadata into this state, maps active hap flashes from eval-time source offsets into the current edited buffer, and uses the range update path for current-block eval. Covered by decoration-state unit tests for text-change detection, range remapping, eval-source flash remapping, and range update preservation.
- [x] Add a native inline slider host equivalent to `slider.mjs`: slider decorations are deduped by source range on full eval, remapped across text edits by the decoration state, and rendered by `rudel-app/src/editor/sliders.rs` as small anchored controls at the numeric literal position. Dragging a slider rewrites the current source literal, maps the decoration range through the replacement, updates the stored literal value, and calls `rudel_lang::set_slider_value` immediately so the already-running pattern reads the new query-time value without re-eval. Intentionally different UI mechanism: egui `TextEdit` does not support true inline child widgets that push text like CodeMirror DOM decorations, so Rudel uses foreground anchored controls positioned from monospace line/column geometry. Covered by app tests for literal replacement, formatting, source-position mapping, and live-registry update.
- [x] Add a native inline/block widget host equivalent to `widget.mjs`: `rudel-app/src/editor/widgets.rs` now syncs visual widget decorations into reusable per-`(type, id)` surfaces, keeps existing surfaces when ids survive eval/range updates, removes surfaces for missing widget ids, and paints anchored canvas-like placeholders at the CodeMirror placement point (`to || from`). Default surface sizes follow Strudel's registered widget defaults: 500x60 for pianoroll/punchcard/scope-style canvases, 275x275 for spiral, and 200x200 for pitchwheel/spectrum. The decoration state already dedupes by `(type, id)` and preserves widgets outside range-aware updates. Intentionally different UI mechanism: egui cannot insert DOM nodes into `TextEdit` flow, so Rudel uses anchored foreground surfaces until a richer native text layout exists. Covered by app tests for surface creation/reuse/removal, `(type, id)` identity, placement, and default sizes.
- [~] Port Strudel's block utilities (`getBlockRegions`, `getBlockAt`, `evalBlock`): `rudel-app/src/editor/blocks.rs` implements Strudel's blank-line-separated region detection and cursor-based lookup, `Ctrl+Shift+Enter` / the transport button evaluate the current block, and `rudel_lang::eval_result_with_source_range` mirrors Strudel's transpiler `range`/`nodeOffset` behavior so mini locations, slider ids, and visual widget ids stay absolute to the full editor buffer. Block eval flashes the evaluated range and updates editor decorations with `replace_range`, preserving outside-range sliders/widgets/mini locations. Covered by app block-region/range-preservation tests and language absolute-offset tests. Still pending/intentional difference: Strudel's persistent JS `codeBlocks`/`blockPatterns`/`userDefinedKeys`, anonymous-label restrictions, `all`/`each` block combiners, and label metadata aggregation are not ported; Rudel evaluates the selected block as the active pattern and keeps persistent cross-block scope deferred with the broader REPL settings/state work.
- [~] Port CodeMirror settings compartments as Rudel editor settings: line wrapping, bracket matching, bracket closing, line numbers, active line, autocomplete, pattern highlighting, flash, tooltips, tab indentation, block-based eval, theme, font family, and font size are now native `EditorSettings` with Strudel-compatible defaults and a compact egui settings panel. The toggles reconfigure highlighting/wrapping/bracket behavior/completion/flash/tooltips and make `Ctrl+Enter` switch to current-block eval when block eval is enabled, with `Ctrl+Shift+Enter` becoming the full-buffer fallback. Deferred: settings persistence, arbitrary CSS font-family strings, the full Strudel theme catalog, and multi-cursor (egui `TextEdit` only supports one native selection).
- [~] Port keybinding behavior from `keybindings.mjs` and `codemirror.mjs`: Ctrl/Alt+Enter full eval by default and current-block eval when block-based eval is enabled, Ctrl+Shift+Enter as the opposite action, Ctrl/Alt+. hush, Ctrl+Shift+. panic, Alt+w/Alt+q `$` jumps, Ctrl+/ and Ctrl+\ comment toggles, Tab/Shift+Tab indent/outdent when tab indentation is enabled, and completion navigation are implemented. Still pending: user-selectable Vim/Emacs/VSCode/Helix keymaps.
- [~] Port autocomplete/tooltip behavior against Rudel's generated reference surface and, later, Strudel's `doc.json`: the native completion engine now mirrors Strudel's handler order (`s`/`sound`, `bank`, `chord`, `scale`, `mode`, then fallback docs), completes loaded sample names and derived bank prefixes, uses Rudel's native tonal scale/chord tables, filters hidden `_...` names like Strudel's docs autocomplete, and shows a Ctrl-held reference tooltip for the word under the editor cursor. Deferred: importing/generated `doc.json`-grade descriptions/params/examples, synonym docs, `superdirtOnly`/`noAutocomplete` tag filtering from upstream docs, explicit-completion-only scale-root behavior, and true mouse-position Ctrl-hover (egui exposes cursor position more naturally than CodeMirror DOM hover).
- [~] Port theme and draw-theme coupling from `themes.mjs`: `EditorTheme` now exposes a native `DrawTheme` with the same field names and defaults Strudel passes to `@strudel/draw` (`background`, `lineBackground`, `foreground`, `muted`, `caret`, `selection`, `selectionMatch`, `lineHighlight`, `gutterBackground`, `gutterForeground`, `light`) for `strudelTheme` and `whitescreen`. The selected theme drives syntax colors, line numbers, active/bracket/flash backgrounds, inline slider styling, reusable visual widget surfaces, native pianoroll/pitchwheel/spiral painters, and the native one-cycle visualizer; widget placeholder active/inactive colors follow Strudel draw defaults (`foreground` / `gutterForeground`). Covered by settings/widget/visualizer tests. Deferred: the full Strudel theme catalog, runtime theme persistence, and the remaining draw-option surface.
- [ ] Add editor automation tests for the native StrudelMirror contract: slider rewrite + live value, range remapping after edits, block eval preserving outside widgets, widget removal cleanup, active source highlights, flash ranges, and animation widget repainting.

## Visuals, Draw, Motion, and External Inputs

- [ ] Match `draw/draw.mjs` runtime behavior: `getDrawContext`, `Pattern.prototype.draw`, `onPaint`, `getPainters`, `Framer`, `Drawer`, visible-hap memory, lookbehind/lookahead windows, future-hap invalidation, full-screen cleanup (`cleanupDraw`) and non-inline cleanup (`cleanupDrawContext`). Rudel should model Strudel's scheduler-time drawing loop without running the Koto VM in the realtime query path; if arbitrary user painter callbacks remain unsupported, document the exact limitation.
- [~] Match inline visual widget registration from `codemirror/widget.mjs`: `_pianoroll`, `_punchcard`, `_spiral`, `_scope`, `_pitchwheel`, and `_spectrum` now scan/rewrite with injected ids and create/reuse editor-owned native surfaces keyed by `(type, id)`. The injected id tags the pattern branch, static option objects drive native surface size, and the native host queries/repaints `_pianoroll`/`_punchcard`/`_wordfall`, `_pitchwheel`, and `_spiral` from current haps. Still pending: `_scope`/`_spectrum`, dynamic/non-literal option expressions, and the full `draw.mjs` painter lifecycle.
- [~] Match `draw/pianoroll.mjs`: `pianoroll`, `_pianoroll`, `punchcard`, `_punchcard`, and `wordfall`, including `cycles`, `playhead`, `overscan`, `hideNegative`, `vertical`, `labels`, `flipTime`, `flipValues`, `smear`, `fold`, active/inactive colors, playhead line, note/sample value mapping, velocity/gain alpha, `color`/`label`/`activeLabel`, autorange, and id/tag filtering. Native inline `_pianoroll`/`_punchcard` now draw a Strudel-default folded window with a playhead, note/freq/sound value mapping, tag/source filtering, theme colors, hex `color`, and velocity/gain alpha; static options now cover `cycles`, `playhead`, `vertical`, `labels`, `flipTime`, `flipValues`, `fold`, `hideNegative`, `hideInactive`, `fill`, `fillActive`, `stroke`, `strokeActive`, `colorizeInactive`, `minMidi`, `maxMidi`, `autorange`, `active`/`inactive`/`playheadColor`, and canvas `width`/`height`; `_wordfall` uses the same mapper vertically with labels. Pending: `overscan`, smear/memory, dynamic option expressions, richer custom labels, and non-inline `pianoroll`/`punchcard` runtime painters.
- [~] Match `draw/pitchwheel.mjs`: pitch-circle rendering from `getFrequency`, root/EDO ring, degree labels, `hapcircles`, `circle`, `thickness`, `hapRadius`, `mode` (`flake` vs `polygon`), margin, event color/velocity/gain alpha, and `_pitchwheel` inline widget behavior. Native `_pitchwheel` now renders active tagged haps as Strudel-default flake centerlines/dots around an EDO ring using Rudel frequency/note resolution, theme colors, hex `color`, and velocity/gain alpha; static options now cover canvas `size`/`width`/`height`, `edo`, `hapcircles`, `circle`, `thickness`, `hapRadius`, `margin`, and `mode: "polygon"`. Pending: degree/interval labels, root/EDO from pattern controls, per-segment polygon colors, dynamic option expressions, and non-inline runtime painter plumbing.
- [~] Match `draw/spiral.mjs`: spiral segment geometry, `stretch`, `size`, `thickness`, `cap`, `inset`, playhead segment, `padding`, `steady`, active/inactive colors, `colorizeInactive`, fade behavior, draw-time window handling, and `_spiral` inline widget behavior. Native `_spiral` now paints tagged hap segments with Strudel's polar spiral mapping, default inline size/thickness/inset/playhead segment, theme active/inactive colors, hex `color`, velocity/gain alpha, and fade over the REPL-style `[-2, 2]` draw window; static options now cover canvas `size`/`width`/`height` with Strudel's inline `size / 5` draw-size mapping, plus `stretch`, `thickness`, `inset`, `playheadLength`, `playheadThickness`, `padding`, `steady`, `activeColor`/`inactiveColor`/`playheadColor`, `colorizeInactive`, and `fade`. Pending: line cap style, dynamic option expressions, true visible-hap memory/future invalidation, and non-inline runtime painter plumbing.
- [ ] Match `draw/animate.mjs`: `animate`, visual shape params (`x`, `y`, `w`, `h`, `angle`, `r`, `fill`, `smear`), `rescale`, `moveXY`, `zoomIn`, smear/clear behavior, sync mode status, and any intentional difference for arbitrary callback support.
- [x] Match `draw/color.mjs`: CSS named color table and `convertColorToNumber` / `convertHexToNumber` behavior where Rudel exposes color values or uses them in visuals. — `rudel-core/src/color.rs` ports the 148-entry `colorMap` (in upstream order) plus `convert_hex_to_number` (`parseInt(hex.slice(1), 16)`) and `convert_color_to_number` (lowercase → `#hex` passthrough / named-table lookup / `-1` on miss), with `css_color_hex` for name→hex. The native draw widgets now resolve CSS names through this table: `rudel-app/src/editor/widgets/style.rs::resolve_color` accepts a `#rrggbb`/`#rrggbbaa` hex or a CSS name, and both the per-hap `color` control (`event_color`) and the widget option colors (`options.rs::option_color`, e.g. `activeColor`/`playheadColor`) route through it, so `color("red")` works in `_pianoroll`/`_pitchwheel`/`_spiral`/etc. instead of falling back. Intentional difference: `convert_hex_to_number` returns `-1` for an unparseable hex rather than JS `NaN` (no integer representation). Covered by `color.rs` tests (hex/named/unrecognized conversion, every table entry converting to its own hex and staying in 24-bit range, case-insensitive lookup) and `rudel-app` `resolves_css_named_colors_and_hex`.
- [ ] Match `webaudio/scope.mjs` and `webaudio/spectrum.mjs` where Rudel has equivalent analyzer data: `scope`/`tscope`, `fscope`, `_scope`, `spectrum`, `_spectrum`, align/trigger options, smear, scrolling spectrum history, color memory, and behavior when no analyzer is available. If native audio analyzer support is deferred, document `_scope`/`_spectrum` separately from pure pattern visuals.
- [~] Add visual parity tests: pure geometry/unit tests for pianoroll value/time mapping, pitchwheel angle mapping, spiral coordinates/fade, color conversion, and scheduler draw-window memory; plus scripted UI or screenshot smoke tests for inline widgets and animation repainting. Unit coverage now checks widget option parsing/sizing, widget tag filtering, pianoroll value priority and time mapping, pitchwheel frequency-angle mapping, spiral polar coordinates, hex/CSS-named color conversion (`rudel-core` `color.rs` against `draw/color.mjs`), and hex color/alpha handling. Pending: fade/scheduler-memory tests, broader option-combination tests, analyzer visuals, and scripted UI/screenshot smoke tests.
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
