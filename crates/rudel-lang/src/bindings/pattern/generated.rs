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
        f64_arg: [$($f64_arg_method:ident),* $(,)?],
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
        forward: [
            $(
                $(#[$forward_attr:meta])*
                $forward_method:ident => $forward_handler:ident
            ),* $(,)?
        ],
        choose: [
            $(
                $(#[$choose_attr:meta])*
                $choose_method:ident => $choose_bipolar:expr
            ),* $(,)?
        ],
        pick_join: [
            $(
                $(#[$pick_join_attr:meta])*
                $pick_join_method:ident => ($pick_join_modulo:expr, $pick_join_mode:expr)
            ),* $(,)?
        ],
        pick_f: [
            $(
                $(#[$pick_f_attr:meta])*
                $pick_f_method:ident => $pick_f_modulo:expr
            ),* $(,)?
        ],
        // CamelCase alias groups: each maps Camel => snake
        camel_pattern: [$($camel_pattern:ident => $snake_pattern:ident),* $(,)?],
        camel_literal_or_pattern: [$($camel_literal_or_pattern:ident => $snake_literal_or_pattern:ident),* $(,)?],
        camel_no_arg: [$($camel_no_arg:ident => $snake_no_arg:ident),* $(,)?],
        camel_noarg_fn: [$($camel_noarg_fn:ident => $snake_noarg_fn:ident),* $(,)?],
        camel_i64: [$($camel_i64:ident => $snake_i64:ident),* $(,)?],
        camel_f64: [$($camel_f64:ident => $snake_f64:ident),* $(,)?],
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
                fn $f64_arg_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_f64_arg(&ctx, |pat, n| pat.$f64_arg_method(n))
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

            // Bespoke method families whose argument parsing lives in
            // `methods.rs`.
            $(
                $(#[$forward_attr])*
                fn $forward_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    $forward_handler(ctx)
                }
            )*

            $(
                $(#[$choose_attr])*
                fn $choose_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    kpattern_choose(ctx, $choose_bipolar)
                }
            )*

            $(
                $(#[$pick_join_attr])*
                fn $pick_join_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    kpattern_pick_join(ctx, $pick_join_modulo, $pick_join_mode)
                }
            )*

            $(
                $(#[$pick_f_attr])*
                fn $pick_f_method(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    kpattern_pick_f(ctx, $pick_f_modulo)
                }
            )*

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
                fn $camel_f64(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                    with_f64_arg(&ctx, |pat, n| pat.$snake_f64(n))
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
    f64_arg: [degrade_by, undegrade_by],
    frac_arg: [hurry, press_by, swing, loop_at, pace, seed],
    pattern_pattern_arg: [slice, splice, bite],
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
    i64_fn_arg: [chunk, chunk_back],
    frac_fn_arg: [inside, outside],
    f64_fn_arg: [jux_by, sometimes_by, some_cycles_by],
    pattern_fn_arg: [off, when],
    frac_frac_fn_arg: [within],
    forward: [
        #[koto_method]
        layer => kpattern_layer,
        #[koto_method]
        fmap => kpattern_fmap,
        #[koto_method]
        tour => kpattern_tour,
        #[koto_method]
        s_tour => kpattern_tour,
        #[koto_method]
        arp_with => kpattern_arp_with,
        #[koto_method]
        voicings => kpattern_voicings,
        #[koto_method]
        scale => kpattern_scale,
        #[koto_method]
        i => kpattern_i,
        #[koto_method]
        freq => kpattern_freq,
        #[koto_method]
        tune => kpattern_tune,
        #[koto_method]
        xen => kpattern_xen,
        #[koto_method]
        with_base => kpattern_with_base,
        #[koto_method]
        ftrans => kpattern_ftrans,
        #[koto_method]
        ftranspose => kpattern_ftranspose,
        #[koto_method]
        partials => kpattern_partials,
        #[koto_method]
        phases => kpattern_phases,
        #[koto_method]
        ctrl => kpattern_ctrl,
        #[koto_method(alias = "as")]
        as_controls => kpattern_as_controls,
        #[koto_method]
        sound => kpattern_sound,
        #[koto_method(alias = "struct")]
        struct_alias => kpattern_struct_alias,
        #[koto_method(alias = "loop")]
        loop_play => kpattern_loop_play,
        #[koto_method(alias = "loopBegin", alias = "loopb")]
        loop_begin => kpattern_loop_begin,
        #[koto_method(alias = "loopEnd", alias = "loope")]
        loop_end => kpattern_loop_end,
        #[koto_method]
        p => kpattern_p,
        #[koto_method]
        midi => kpattern_midi,
        #[koto_method]
        osc => kpattern_osc,
        #[koto_method]
        chord => kpattern_chord,
        #[koto_method(alias = "loopAtCps", alias = "loopatcps")]
        loop_at_cps => kpattern_loop_at_cps,
        #[koto_method(alias = "eish")]
        euclidish => kpattern_euclidish,
        #[koto_method]
        bjork => kpattern_bjork,
        // `every`/`firstOf`/`lastOf` take a *patternified* cycle count
        // (`every("<2 4>", f)`), so they bypass the scalar `i64_fn_arg` group.
        #[koto_method]
        every => kpattern_every,
        #[koto_method(alias = "firstOf")]
        first_of => kpattern_every,
        #[koto_method(alias = "lastOf")]
        last_of => kpattern_last_of,
    ],
    choose: [
        #[koto_method]
        choose => false,
        #[koto_method]
        choose2 => true,
    ],
    pick_join: [
        #[koto_method]
        pick => (false, PickJoin::Inner),
        #[koto_method]
        pickmod => (true, PickJoin::Inner),
        #[koto_method(alias = "pickOut")]
        pick_out => (false, PickJoin::Outer),
        #[koto_method(alias = "pickmodOut")]
        pickmod_out => (true, PickJoin::Outer),
        #[koto_method(alias = "pickReset")]
        pick_reset => (false, PickJoin::Reset),
        #[koto_method(alias = "pickmodReset")]
        pickmod_reset => (true, PickJoin::Reset),
        #[koto_method(alias = "pickRestart")]
        pick_restart => (false, PickJoin::Restart),
        #[koto_method(alias = "pickmodRestart")]
        pickmod_restart => (true, PickJoin::Restart),
        #[koto_method(alias = "pickSqueeze", alias = "pick_squeeze")]
        inhabit => (false, PickJoin::Squeeze),
        #[koto_method(alias = "pickmodSqueeze", alias = "pickmod_squeeze")]
        inhabitmod => (true, PickJoin::Squeeze),
    ],
    pick_f: [
        #[koto_method(alias = "pickF")]
        pick_f => false,
        #[koto_method(alias = "pickmodF")]
        pickmod_f => true,
    ],
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
    camel_f64: [degradeBy => degrade_by, undegradeBy => undegrade_by],
    camel_frac: [pressBy => press_by, loopAt => loop_at, steps => pace],
    camel_frac_frac: [swingBy => swing_by],
    camel_i64_i64: [euclidLegato => euclid_legato],
    camel_i64_i64_i64: [euclidRot => euclid_rot, euclidLegatoRot => euclid_legato_rot],
    camel_i64_fn: [chunkBack => chunk_back],
    camel_f64_fn: [juxBy => jux_by, sometimesBy => sometimes_by, someCyclesBy => some_cycles_by],
}
