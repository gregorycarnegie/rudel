#![allow(non_snake_case)]

use super::KPattern;
use super::args::*;
use super::callback::with_callback;
use super::methods::*;
use koto::derive::*;
use koto::prelude::*;
use koto::runtime::Result as KotoResult;
use rudel_core::PickJoin;

macro_rules! kpattern_methods {
    (
        pattern_arg: [$($pattern_arg_method:ident),* $(,)?],
        no_arg: [$($no_arg_method:ident),* $(,)?],
        i64_arg: [$($i64_arg_method:ident),* $(,)?],
        frac_arg: [$($frac_arg_method:ident),* $(,)?],
        pattern_pattern_arg: [$($pattern_pattern_arg_method:ident),* $(,)?],
        frac_frac_arg: [$($frac_frac_arg_method:ident),* $(,)?],
        f64_f64_arg: [$($f64_f64_arg_method:ident),* $(,)?],
        i64_i64_arg: [$($i64_i64_arg_method:ident),* $(,)?],
        i64_i64_i64_arg: [$($i64_i64_i64_arg_method:ident),* $(,)?],
        i64_frac_f64_arg: [$($i64_frac_f64_arg_method:ident),* $(,)?],
        i64_f64_frac_arg: [$($i64_f64_frac_arg_method:ident),* $(,)?],
        fn_arg: [$($fn_arg_method:ident),* $(,)?],
        i64_fn_arg: [$($i64_fn_arg_method:ident),* $(,)?],
        frac_fn_arg: [$($frac_fn_arg_method:ident),* $(,)?],
        f64_fn_arg: [$($f64_fn_arg_method:ident),* $(,)?],
        pattern_fn_arg: [$($pattern_fn_arg_method:ident),* $(,)?],
        frac_frac_fn_arg: [$($frac_frac_fn_arg_method:ident),* $(,)?],
        // CamelCase alias groups: each maps Camel => snake
        camel_pattern: [$($camel_pattern:ident => $snake_pattern:ident),* $(,)?],
        camel_literal_or_pattern: [$($camel_literal_or_pattern:ident => $snake_literal_or_pattern:ident),* $(,)?],
        camel_no_arg: [$($camel_no_arg:ident => $snake_no_arg:ident),* $(,)?],
        camel_noarg_fn: [$($camel_noarg_fn:ident => $snake_noarg_fn:ident),* $(,)?],
        camel_i64: [$($camel_i64:ident => $snake_i64:ident),* $(,)?],
        camel_frac: [$($camel_frac:ident => $snake_frac:ident),* $(,)?],
        camel_frac_frac: [$($camel_frac_frac:ident => $snake_frac_frac:ident),* $(,)?],
        camel_i64_i64: [$($camel_i64_i64:ident => $snake_i64_i64:ident),* $(,)?],
        camel_i64_i64_i64: [$($camel_i64_i64_i64:ident => $snake_i64_i64_i64:ident),* $(,)?],
        camel_i64_fn: [$($camel_i64_fn:ident => $snake_i64_fn:ident),* $(,)?],
        camel_f64_fn: [$($camel_f64_fn:ident => $snake_f64_fn:ident),* $(,)?],
    ) => {
        #[koto_impl]
        impl KPattern {
            $(
                #[koto_method]
                fn $pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_arg(&ctx, |pat, arg| pat.$pattern_arg_method(arg))
                }
            )*

            $(
                #[koto_method]
                fn $no_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_instance(&ctx, |pat| pat.$no_arg_method())
                }
            )*

            $(
                #[koto_method]
                fn $i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_arg(&ctx, |pat, n| pat.$i64_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_arg(&ctx, |pat, n| pat.$frac_arg_method(n))
                }
            )*

            $(
                #[koto_method]
                fn $pattern_pattern_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_pattern_args(&ctx, |pat, a, b| pat.$pattern_pattern_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $frac_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_frac_args(&ctx, |pat, a, b| pat.$frac_frac_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $f64_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_f64_f64_args(&ctx, |pat, a, b| pat.$f64_f64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_args(&ctx, |pat, a, b| pat.$i64_i64_arg_method(a, b))
                }
            )*

            $(
                #[koto_method]
                fn $i64_i64_i64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_i64_args(&ctx, |pat, a, b, c| pat.$i64_i64_i64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_frac_f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_frac_f64_args(&ctx, |pat, a, b, c| pat.$i64_frac_f64_arg_method(a, b, c))
                }
            )*

            $(
                #[koto_method]
                fn $i64_f64_frac_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_f64_frac_args(&ctx, |pat, a, b, c| pat.$i64_f64_frac_arg_method(a, b, c))
                }
            )*

            // `pat.method(f)` where `f` is a Koto function `Pattern -> Pattern`.
            $(
                #[koto_method]
                fn $fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_callback(&ctx, 0, |pat, cb| pat.$fn_arg_method(|p| cb.apply(p)))
                }
            )*

            // `pat.method(n, f)` where `n` is an integer and `f` a function.
            $(
                #[koto_method]
                fn $i64_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_i64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$i64_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $frac_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_frac_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$frac_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $f64_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_f64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$f64_fn_arg_method(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $pattern_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let arg = method_pattern_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$pattern_fn_arg_method(arg, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                fn $frac_frac_fn_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let a = method_frac_arg(&ctx, 0);
                    let b = method_frac_arg(&ctx, 1);
                    with_callback(&ctx, 2, |pat, cb| pat.$frac_frac_fn_arg_method(a, b, |p| cb.apply(p)))
                }
            )*

            // `pat.layer([f, g, ...])`: stack the results of applying each
            // function in the list to the pattern. Accepts a list/tuple of
            // callables, or bare callable args.
            #[koto_method]
            fn layer(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_layer(ctx)
            }

            // `pat.fmap(f)`: Strudel's value-level mapper. The Koto VM isn't
            // Send+Sync, so map one cycle eagerly and repeat that shape.
            #[koto_method]
            fn fmap(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_fmap(ctx)
            }

            // `pat.tour(a, b, ...)`: insert the pattern into the list of
            // patterns stepwise, moving backwards one slot per repetition.
            #[koto_method]
            fn tour(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_tour(ctx)
            }

            // Deprecated Strudel alias for `tour`.
            #[koto_method]
            fn s_tour(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_tour(ctx)
            }

            // CamelCase aliases: generate small wrappers that call the
            // existing snake_case implementations to reduce duplication.
            // Each alias group maps CamelCase -> snake_case and uses the
            // appropriate argument extractor.
            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_pattern(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_pattern_arg(&ctx, |pat, arg| pat.$snake_pattern(arg))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_literal_or_pattern(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_literal_or_pattern_arg(&ctx, |pat, arg| pat.$snake_literal_or_pattern(arg))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_no_arg(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_instance(&ctx, |pat| pat.$snake_no_arg())
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_noarg_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_callback(&ctx, 0, |pat, cb| pat.$snake_noarg_fn(|p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_arg(&ctx, |pat, n| pat.$snake_i64(n))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_frac(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_arg(&ctx, |pat, n| pat.$snake_frac(n))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_frac_frac(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_frac_frac_args(&ctx, |pat, a, b| pat.$snake_frac_frac(a, b))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_args(&ctx, |pat, a, b| pat.$snake_i64_i64(a, b))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_i64_i64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_i64_i64_i64_args(&ctx, |pat, a, b, c| pat.$snake_i64_i64_i64(a, b, c))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_i64_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_i64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$snake_i64_fn(n, |p| cb.apply(p)))
                }
            )*

            $(
                #[koto_method]
                #[allow(non_snake_case)]
                fn $camel_f64_fn(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    let n = method_f64_arg(&ctx, 0);
                    with_callback(&ctx, 1, |pat, cb| pat.$snake_f64_fn(n, |p| cb.apply(p)))
                }
            )*

            // (no camel_frac_fn group)

            // `pat.arp_with(|chord| ...)`: arpeggiate chords, transforming each
            // chord (presented as a sequence of its notes) with a callback.
            //
            // The callback can't run in the (Send+Sync) query path because the
            // Koto VM isn't Send, so we evaluate it eagerly here: probe the
            // distinct chords over the first `PROBE` cycles, run the callback on
            // each, and bake the results into a lookup the query path consults.
            // Chords first appearing after the probe window fall back to silence.
            #[koto_method]
            fn arp_with(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_arp_with(ctx)
            }

            // `pat.voicings("lefthand")`: voice chords with a named dictionary.
            #[koto_method]
            fn voicings(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_voicings(ctx)
            }

            #[koto_method]
            fn scale(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_scale(ctx)
            }

            #[koto_method]
            fn i(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_i(ctx)
            }

            #[koto_method]
            fn freq(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_freq(ctx)
            }

            #[koto_method]
            fn tune(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_tune(ctx)
            }

            #[koto_method]
            fn xen(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_xen(ctx)
            }

            #[koto_method]
            fn with_base(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_with_base(ctx)
            }

            #[koto_method]
            fn ftrans(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ftrans(ctx)
            }

            #[koto_method]
            fn ftranspose(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ftranspose(ctx)
            }

            // `bendRange` alias generated above.

            // Sample looping. `loop` is a Koto keyword but is allowed after `.`,
            // so these expose the Strudel names (`loop`/`loopBegin`/`loopEnd`)
            // as aliases of the keyword-safe Rust method names.
            // `pat.partials([1, 0.5, 0.3])` / `pat.partials(8)`: additive
            // harmonic magnitudes (or a count). `pat.phases([...])`: per-harmonic
            // phase offsets. The value is the whole list, applied to every event.
            #[koto_method]
            fn partials(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_partials(ctx)
            }

            #[koto_method]
            fn phases(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_phases(ctx)
            }

            // `pat.ctrl("fmi20", 3)`: set an arbitrary named control. The escape
            // hatch for controls without a dedicated method.
            #[koto_method]
            fn ctrl(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_ctrl(ctx)
            }

            // `pat.as("note:clip")`: map bare positional values into named
            // controls (`as` is keyword-safe after `.`, like `loop`).
            #[koto_method(alias = "as")]
            fn as_controls(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_as_controls(ctx)
            }

            #[koto_method]
            fn sound(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_sound(ctx)
            }

            #[koto_method(alias = "struct")]
            fn struct_alias(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_struct_alias(ctx)
            }

            // The pick family (strudel core/pick.mjs): the instance is the
            // selector pattern, the argument a list/map of patterns. Variants
            // differ in index wrapping (`pickmod*`) and join: pick = inner,
            // pickOut = outer, pickReset/pickRestart = retriggering,
            // inhabit/pickSqueeze = squeeze.
            #[koto_method]
            fn pick(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, false, PickJoin::Inner)
            }

            #[koto_method]
            fn pickmod(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, true, PickJoin::Inner)
            }

            #[koto_method(alias = "pickOut")]
            fn pick_out(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, false, PickJoin::Outer)
            }

            #[koto_method(alias = "pickmodOut")]
            fn pickmod_out(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, true, PickJoin::Outer)
            }

            #[koto_method(alias = "pickReset")]
            fn pick_reset(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, false, PickJoin::Reset)
            }

            #[koto_method(alias = "pickmodReset")]
            fn pickmod_reset(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, true, PickJoin::Reset)
            }

            #[koto_method(alias = "pickRestart")]
            fn pick_restart(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, false, PickJoin::Restart)
            }

            #[koto_method(alias = "pickmodRestart")]
            fn pickmod_restart(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, true, PickJoin::Restart)
            }

            #[koto_method(alias = "pickSqueeze", alias = "pick_squeeze")]
            fn inhabit(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, false, PickJoin::Squeeze)
            }

            #[koto_method(alias = "pickmodSqueeze", alias = "pickmod_squeeze")]
            fn inhabitmod(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_join(ctx, true, PickJoin::Squeeze)
            }

            // `pat.pickF(selector, funcs)`: pick which function to apply.
            #[koto_method(alias = "pickF")]
            fn pick_f(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_f(ctx, false)
            }

            #[koto_method(alias = "pickmodF")]
            fn pickmod_f(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick_f(ctx, true)
            }

            #[koto_method(alias = "loop")]
            fn loop_play(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_play(ctx)
            }

            #[koto_method(alias = "loopBegin", alias = "loopb")]
            fn loop_begin(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_begin(ctx)
            }

            #[koto_method(alias = "loopEnd", alias = "loope")]
            fn loop_end(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_loop_end(ctx)
            }

            // `.p(name)`: tag a pattern with an `id` (Strudel's per-pattern
            // naming, e.g. `s("bd").p("drums")`). The name may be a string or a
            // number (`$1`-style slots).
            #[koto_method]
            fn p(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_p(ctx)
            }

            // `.midi(device?)`: route this pattern to the MIDI output. The
            // optional device-name hint is stored as `_midiport`. Sets the
            // routing tag the app reads via `output_targets`/`filter_output`.
            #[koto_method]
            fn midi(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_midi(ctx)
            }

            // `.osc(target?)`: route this pattern to the OSC output. An optional
            // `"host:port"` target sets `oschost`/`oscport` (per-event routing).
            #[koto_method]
            fn osc(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_osc(ctx)
            }

            // `.chord()` (zero-arg) expands chord names into note stacks;
            // `.chord(value)` sets the Strudel-style chord control consumed by
            // `.voicing()` / `.root_notes()`.
            #[koto_method]
            fn chord(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_chord(ctx)
            }

            // CamelCase aliases are generated above from compact lists.
        }
    };
}

kpattern_methods! {
    pattern_arg: [
        fast, slow, ply, segment, seg, add, sub, mul, div, modulo, pow, set, keep, mask, struct_pat,
        early, late, fast_gap,
        // Simple controls and their aliases (note, s, gain, lpf, the numbered
        // FM families, MIDI controls, ...) are NOT listed here: they are
        // registered dynamically from rudel-core's `control_builders`
        // registry by `extend_control_entries`, so adding a control to the
        // macros in rudel-core/src/controls.rs is all that's needed.
        // alignment matrix (`in` is the default plain op; these are the rest)
        add_out, add_mix, add_squeeze, add_squeezeout, add_reset, add_restart,
        sub_out, mul_out, mul_squeeze, div_out,
        set_out, set_mix, set_squeeze, set_squeezeout,
        keep_out, keep_squeeze,
        add_poly, mul_poly, set_poly, keep_poly,
        transpose, scale_transpose, bend_range,
        overlay, arp, trans, strans,
        // multi-control helpers (`adsr` expands into attack/decay/sustain/
        // release, `control` sets ccn/ccv, `sysex` sets sysexid/sysexdata)
        // and sample scrubbing
        adsr, ad, ds, ar, control, sysex, scrub,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio, fit, arpeggiate, voicing, piano,
    ],
    i64_arg: [
        iter, iter_back, repeat_cycles, expand, extend, contract, shrink, grow,
        chop, striate, take, drop, root_notes, shuffle, scramble,
    ],
    frac_arg: [hurry, press_by, swing, loop_at, pace],
    pattern_pattern_arg: [slice, splice],
    frac_frac_arg: [focus, swing_by, compress, zoom, ribbon, rib],
    f64_f64_arg: [range, range2, rangex],
    i64_i64_arg: [euclid, euclid_legato],
    i64_i64_i64_arg: [euclid_rot, euclid_legato_rot],
    i64_frac_f64_arg: [echo],
    i64_f64_frac_arg: [stut],
    fn_arg: [
        superimpose, jux, sometimes, often, rarely, almost_always, almost_never, some_cycles,
        apply, always, never,
    ],
    i64_fn_arg: [every, first_of, last_of, chunk, chunk_back],
    frac_fn_arg: [inside, outside],
    f64_fn_arg: [jux_by, sometimes_by, some_cycles_by],
    pattern_fn_arg: [off, when],
    frac_frac_fn_arg: [within],
    // CamelCase alias mappings (Camel => snake)
    camel_pattern: [
        // camelCase control names (wavetablePosition, compressorKnee, ...)
        // come from the dynamic registry; only non-control transforms and the
        // keyword-safe `bendRange` spelling are listed here.
        bendRange => bend_range, fastGap => fast_gap, scaleTranspose => scale_transpose,
        scaleTrans => strans,
    ],
    camel_literal_or_pattern: [withBase => with_base, fTrans => ftrans, fTranspose => ftranspose],
    camel_no_arg: [toBipolar => to_bipolar, fromBipolar => from_bipolar],
    camel_noarg_fn: [someCycles => some_cycles, almostAlways => almost_always, almostNever => almost_never],
    camel_i64: [
        iterBack => iter_back, repeatCycles => repeat_cycles, rootNotes => root_notes,
        // deprecated Strudel stepwise aliases
        s_taper => shrink, s_add => take, s_sub => drop,
        s_expand => expand, s_extend => extend, s_contract => contract,
    ],
    camel_frac: [pressBy => press_by, loopAt => loop_at, steps => pace],
    camel_frac_frac: [swingBy => swing_by],
    camel_i64_i64: [euclidLegato => euclid_legato],
    camel_i64_i64_i64: [euclidRot => euclid_rot, euclidLegatoRot => euclid_legato_rot],
    camel_i64_fn: [firstOf => first_of, lastOf => last_of, chunkBack => chunk_back],
    camel_f64_fn: [juxBy => jux_by, sometimesBy => sometimes_by, someCyclesBy => some_cycles_by],
}
