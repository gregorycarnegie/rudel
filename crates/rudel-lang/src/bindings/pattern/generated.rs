//! The bulk of `Pattern.prototype`: every method whose arguments fall into one
//! of a handful of shapes, generated from lists of the core method names.
//! Methods with bespoke argument handling live in `methods.rs`.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    args::*,
    callback::{with_callback, with_cb_f64, with_cb_frac, with_cb_frac2, with_cb_i64},
    methods::*,
};
use crate::js::{Arg, Res, Scope};
use rudel_core::{Pattern, PickJoin};

type Handler = fn(&Pattern, &[Arg]) -> Res;

pub(super) fn register_generated(p: &Scope) {
    macro_rules! pattern_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| Ok(pat.$m(pattern_arg(a, 0)).into()));
        )*};
    }
    macro_rules! no_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, _| Ok(pat.$m().into()));
        )*};
    }
    macro_rules! i64_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| Ok(pat.$m(i64_arg(a, 0)).into()));
        )*};
    }
    // Same shape as `i64_arg`, but a patterned count is laid out stepwise
    // rather than sampled per cycle.
    macro_rules! stepwise_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                Ok(stepwise_call(pat, a.first(), |p, n| p.$m(n)).into())
            });
        )*};
    }
    macro_rules! f64_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| Ok(pat.$m(f64_arg(a, 0)).into()));
        )*};
    }
    macro_rules! frac_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| Ok(pat.$m(frac_arg(a, 0)).into()));
        )*};
    }
    macro_rules! pattern_pattern_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                Ok(pat.$m(pattern_arg(a, 0), pattern_arg(a, 1)).into())
            });
        )*};
    }
    macro_rules! frac_frac_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                Ok(pat.$m(frac_arg(a, 0), frac_arg(a, 1)).into())
            });
        )*};
    }
    macro_rules! f64_f64_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                Ok(pat.$m(f64_arg(a, 0), f64_arg(a, 1)).into())
            });
        )*};
    }
    // `pat.method(f)` where `f` is a function `Pattern -> Pattern`.
    macro_rules! fn_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                with_callback(pat, a, 0, |pat, cb| pat.$m(|p| cb.apply(p)))
            });
        )*};
    }
    // `pat.method(n, f)` where `n` is a number (or a pattern of numbers,
    // `chunk("<2 4>", f)`) and `f` a function.
    macro_rules! i64_fn_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                with_cb_i64(pat, arg(a, 0), arg(a, 1), |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! frac_fn_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                with_cb_frac(pat, arg(a, 0), arg(a, 1), |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! f64_fn_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                with_cb_f64(pat, arg(a, 0), arg(a, 1), |p, n, cb| p.$m(n, |p| cb.apply(p)))
            });
        )*};
    }
    macro_rules! pattern_fn_arg {
        ($($m:ident),* $(,)?) => {$(
            method(p, stringify!($m), |pat, a| {
                let x = pattern_arg(a, 0);
                with_callback(pat, a, 1, |pat, cb| pat.$m(x, |p| cb.apply(p)))
            });
        )*};
    }

    pattern_arg![
        degrade_by,
        undegrade_by,
        fast,
        slow,
        ply,
        segment,
        seg,
        add,
        sub,
        mul,
        div,
        modulo,
        pow,
        set,
        keep,
        mask,
        struct_pat,
        early,
        late,
        fast_gap,
        // comparison / logic composers (boolean results)
        lt,
        gt,
        lte,
        gte,
        eq,
        eqt,
        ne,
        net,
        and,
        or,
        keepif,
        bypass,
        // bitwise composers (int32 results)
        band,
        bor,
        bxor,
        blshift,
        brshift,
        // Simple controls and their aliases (note, s, gain, lpf, the numbered
        // FM families, MIDI controls, ...) are NOT listed here: they are
        // registered from rudel-core's `control_builders` registry by
        // `register_methods`, so adding a control to the macros in
        // rudel-core/src/controls.rs is all that's needed.
        // alignment matrix (`in` is the default plain op; these are the full
        // out/mix/squeeze/squeezeout/reset/restart/poly set for each composer)
        add_out,
        add_mix,
        add_squeeze,
        add_squeezeout,
        add_reset,
        add_restart,
        add_poly,
        sub_out,
        sub_mix,
        sub_squeeze,
        sub_squeezeout,
        sub_reset,
        sub_restart,
        sub_poly,
        mul_out,
        mul_mix,
        mul_squeeze,
        mul_squeezeout,
        mul_reset,
        mul_restart,
        mul_poly,
        div_out,
        div_mix,
        div_squeeze,
        div_squeezeout,
        div_reset,
        div_restart,
        div_poly,
        set_out,
        set_mix,
        set_squeeze,
        set_squeezeout,
        set_reset,
        set_restart,
        set_poly,
        keep_out,
        keep_mix,
        keep_squeeze,
        keep_squeezeout,
        keep_reset,
        keep_restart,
        keep_poly,
        modulo_out,
        modulo_mix,
        modulo_squeeze,
        modulo_squeezeout,
        modulo_reset,
        modulo_restart,
        modulo_poly,
        pow_out,
        pow_mix,
        pow_squeeze,
        pow_squeezeout,
        pow_reset,
        pow_restart,
        pow_poly,
        keepif_out,
        keepif_mix,
        keepif_squeeze,
        keepif_squeezeout,
        keepif_reset,
        keepif_restart,
        keepif_poly,
        transpose,
        scale_transpose,
        bend_range,
        overlay,
        arp,
        trans,
        strans,
        // impure: live-coding timeline cue alignment (core/impure.mjs)
        timeline,
        // waveshaping-distortion shortcuts (superdough distortion family)
        soft,
        hard,
        cubic,
        diode,
        asym,
        fold,
        sinefold,
        chebyshev,
        // multi-control helpers (`adsr` expands into attack/decay/sustain/
        // release, `control` sets ccn/ccv, `sysex` sets sysexid/sysexdata)
        // and sample scrubbing
        adsr,
        ad,
        ds,
        ar,
        control,
        sysex,
        scrub,
        // @strudel/draw animate transforms over the x/y/w/h visual params
        // (no `animate` runtime in Rudel; these set the params for parity).
        rescale,
        zoom_in,
        loop_play,
        loop_begin,
        loop_end,
    ];
    // The chained forms of the factories: `s("hh*4").stack(note("c"))` takes
    // `this` as the first pattern, as upstream's methods do.
    for name in ["cat", "slowcat"] {
        method(p, name, |pat, a| Ok(pat.cat_with(pattern_arg(a, 0)).into()));
    }
    for name in ["seq", "fastcat", "sequence"] {
        method(p, name, |pat, a| Ok(pat.seq_with(pattern_arg(a, 0)).into()));
    }
    no_arg![
        dough,
        hush,
        rev,
        revv,
        palindrome,
        degrade,
        undegrade,
        press,
        brak,
        round,
        floor,
        ceil,
        log2,
        to_bipolar,
        from_bipolar,
        ratio,
        fit,
        arpeggiate,
        voicing,
        piano,
        invert,
        collect,
        // The joins: a pattern whose *values* are patterns is flattened by one
        // of these. Reachable because a callback returning a pattern already
        // converts to `Value::Pat`, so `fmap(v => …).innerJoin()` — the shape a
        // `register`ed helper is written in — works with no script in the query
        // path.
        inner_join,
        outer_join,
        squeeze_join,
        step_join,
        poly_join,
        join,
        reset_join,
        restart_join,
    ];
    i64_arg![
        iter,
        iter_back,
        repeat_cycles,
        chop,
        striate,
        root_notes,
        shuffle,
        scramble,
    ];
    // https://strudel.cc/learn/stepwise/ — the count may be a pattern, and is
    // then laid out stepwise rather than sampled per cycle.
    stepwise_arg![
        expand, extend, contract, shrink, grow, take, drop, replicate
    ];
    f64_arg![cpm];
    frac_arg![hurry, press_by, swing, loop_at, pace, seed, linger];
    pattern_pattern_arg![slice, splice, bite, beat, xfade, move_xy, speak];
    frac_frac_arg![focus, swing_by, compress, zoom, ribbon, rib];
    f64_f64_arg![range, range2, rangex];
    method(p, "echo", |pat, a| {
        Ok(pat
            .echo(i64_arg(a, 0), frac_arg(a, 1), f64_arg(a, 2))
            .into())
    });
    method(p, "stut", |pat, a| {
        Ok(pat
            .stut(i64_arg(a, 0), f64_arg(a, 1), frac_arg(a, 2))
            .into())
    });
    fn_arg![
        jux,
        jux_flip,
        sometimes,
        often,
        rarely,
        almost_always,
        almost_never,
        some_cycles,
        always,
        never,
    ];
    i64_fn_arg![chunk, chunk_back, fast_chunk];
    frac_fn_arg![inside, outside];
    f64_fn_arg![jux_by, jux_flip_by, sometimes_by, some_cycles_by];
    pattern_fn_arg![off, when];
    method(p, "within", |pat, a| {
        with_cb_frac2(pat, arg(a, 0), arg(a, 1), arg(a, 2), |p, x, y, cb| {
            p.within(x, y, |p| cb.apply(p))
        })
    });

    // Bespoke method families whose argument parsing lives in `methods.rs`,
    // each under every name it answers to.
    let forward: &[(&[&str], Handler)] = &[
        // `apply` takes a *pattern* of functions as well as a function, so it
        // cannot use the plain callback group.
        (&["apply"], kpattern_apply),
        (&["layer"], kpattern_layer),
        (&["superimpose"], kpattern_superimpose),
        // `withValue` is Strudel's own name for `fmap` (core/pattern.mjs binds
        // both to the same function); songs in the wild use it more than `fmap`.
        (&["fmap", "withValue"], kpattern_fmap),
        (&["tour", "s_tour"], kpattern_tour),
        (&["FX", "fx"], kpattern_fx),
        (&["arp_with", "arpWith"], kpattern_arp_with),
        (&["when_key", "whenKey"], kpattern_when_key),
        (&["tag"], kpattern_tag),
        (
            &["degrade_by_with", "degradeByWith"],
            kpattern_degrade_by_with,
        ),
        (&["set_steps", "setSteps"], kpattern_set_steps),
        // The euclid family takes patterned counts, so it cannot go in a
        // plain-integer argument group.
        (&["euclid"], kpattern_euclid),
        (
            &["euclid_rot", "euclidRot", "euclidrot"],
            kpattern_euclid_rot,
        ),
        (&["euclid_legato", "euclidLegato"], kpattern_euclid_legato),
        (
            &["euclid_legato_rot", "euclidLegatoRot"],
            kpattern_euclid_legato_rot,
        ),
        (&["soundfont"], kpattern_soundfont),
        (&["set_context", "setContext"], kpattern_set_context),
        // `filterHaps` is upstream's own name for it (core/pattern.mjs defines
        // `filter` as a `register`ed wrapper around the method).
        (&["filter", "filterHaps"], kpattern_filter),
        (&["filter_values", "filterValues"], kpattern_filter_values),
        (&["filter_when", "filterWhen"], kpattern_filter_when),
        (&["key_down", "keyDown"], kpattern_key_down),
        (&["voicings"], kpattern_voicings),
        (&["scale"], kpattern_scale),
        (&["markcss"], kpattern_markcss),
        (&["log"], kpattern_log),
        (&["log_values", "logValues"], kpattern_log_values),
        (
            &["on_trigger_time", "onTriggerTime"],
            kpattern_on_trigger_time,
        ),
        (&["edo_scale", "edoScale"], kpattern_edo_scale),
        (&["i"], kpattern_i),
        (&["freq"], kpattern_freq),
        (&["tune"], kpattern_tune),
        (&["xen"], kpattern_xen),
        (&["tuning"], kpattern_tuning),
        (&["with_base", "withBase"], kpattern_with_base),
        (
            &["ftrans", "ftranspose", "fTrans", "fTranspose"],
            kpattern_ftrans,
        ),
        (&["partials"], kpattern_partials),
        (&["phases"], kpattern_phases),
        (&["ctrl"], kpattern_ctrl),
        (&["as_controls", "as"], kpattern_as_controls),
        (&["midi"], kpattern_midi),
        (&["osc"], kpattern_osc),
        (&["chord"], kpattern_chord),
        // The public (non-underscore) visualizer names are Strudel's global
        // full-screen painters; Rudel has no global draw canvas, so they expose
        // the same inline editor widget as their `_`-prefixed variants (the
        // preprocess rewrites either spelling to the same widget host).
        (
            &[
                "_pianoroll",
                "pianoroll",
                "_punchcard",
                "punchcard",
                "_spiral",
                "spiral",
                "_scope",
                "scope",
                "tscope",
                "_fscope",
                "fscope",
                "_pitchwheel",
                "pitchwheel",
                "_spectrum",
                "spectrum",
                "_wordfall",
                "wordfall",
                "_claviature",
                "claviature",
                "_shader",
                "shader",
                "_hydra",
                "hydra",
                "rudel_widget_pianoroll",
                "rudel_widget_punchcard",
                "rudel_widget_spiral",
                "rudel_widget_scope",
                "rudel_widget_pitchwheel",
                "rudel_widget_spectrum",
                "rudel_widget_wordfall",
                "rudel_widget_claviature",
                "rudel_widget_shader",
                "rudel_widget_hydra",
                "rudel_widget_fscope",
            ],
            kpattern_visual_widget,
        ),
        (
            &["loop_at_cps", "loopAtCps", "loopatcps"],
            kpattern_loop_at_cps,
        ),
        (&["apply_n", "applyN"], kpattern_apply_n),
        (
            &["echo_with", "echoWith", "echowith", "stutWith", "stutwith"],
            kpattern_echo_with,
        ),
        (&["ply_with", "plyWith", "plywith"], kpattern_ply_with),
        (
            &["ply_for_each", "plyForEach", "plyforeach"],
            kpattern_ply_for_each,
        ),
        (&["into"], kpattern_into),
        (
            &["chunk_into", "chunkInto", "chunkinto"],
            kpattern_chunk_into,
        ),
        (
            &["chunk_back_into", "chunkBackInto", "chunkbackinto"],
            kpattern_chunk_back_into,
        ),
        (&["euclidish", "eish"], kpattern_euclidish),
        (&["hsl"], kpattern_hsl),
        (&["hsla"], kpattern_hsla),
        (&["bjork"], kpattern_bjork),
        // `every`/`firstOf`/`lastOf` take a *patternified* cycle count
        // (`every("<2 4>", f)`), so they bypass the scalar `i64_fn_arg` group.
        (&["every", "first_of", "firstOf"], kpattern_every),
        (&["last_of", "lastOf"], kpattern_last_of),
    ];
    for (names, handler) in forward {
        for name in *names {
            method(p, name, *handler);
        }
    }
    // `struct` is a reserved word in Rust, so the core method is `struct_pat`.
    p.alias("struct", "struct_pat");
    p.alias("loop", "loop_play");
    for name in ["loopBegin", "loopb"] {
        p.alias(name, "loop_begin");
    }
    for name in ["loopEnd", "loope"] {
        p.alias(name, "loop_end");
    }

    for (name, bipolar) in [("choose", false), ("choose2", true)] {
        method(p, name, move |pat, a| kpattern_choose(pat, a, bipolar));
    }
    for (names, modulo, join) in [
        (&["pick"][..], false, PickJoin::Inner),
        (&["pickmod"][..], true, PickJoin::Inner),
        (&["pick_out", "pickOut"][..], false, PickJoin::Outer),
        (&["pickmod_out", "pickmodOut"][..], true, PickJoin::Outer),
        (&["pick_reset", "pickReset"][..], false, PickJoin::Reset),
        (
            &["pickmod_reset", "pickmodReset"][..],
            true,
            PickJoin::Reset,
        ),
        (
            &["pick_restart", "pickRestart"][..],
            false,
            PickJoin::Restart,
        ),
        (
            &["pickmod_restart", "pickmodRestart"][..],
            true,
            PickJoin::Restart,
        ),
        (
            &["inhabit", "pickSqueeze", "pick_squeeze"][..],
            false,
            PickJoin::Squeeze,
        ),
        (
            &["inhabitmod", "pickmodSqueeze", "pickmod_squeeze"][..],
            true,
            PickJoin::Squeeze,
        ),
    ] {
        for name in names {
            method(p, name, move |pat, a| {
                kpattern_pick_join(pat, a, modulo, join)
            });
        }
    }
    for (names, modulo) in [
        (["pick_f", "pickF"], false),
        (["pickmod_f", "pickmodF"], true),
    ] {
        for name in names {
            method(p, name, move |pat, a| kpattern_pick_f(pat, a, modulo));
        }
    }

    // CamelCase and other second spellings: the same method under another
    // name.
    for (alias, name) in [
        // camelCase control names (wavetablePosition, compressorKnee, ...) come
        // from the registry; only non-control transforms and the
        // keyword-safe `bendRange` spelling are listed here.
        ("bendRange", "bend_range"),
        ("fastGap", "fast_gap"),
        ("fastgap", "fast_gap"),
        ("scaleTranspose", "scale_transpose"),
        ("scaleTrans", "strans"),
        ("sparsity", "slow"),
        // Bare alignment methods default to the `set` op (Strudel's
        // `pat.out(x) == pat.set.out(x)`); `squeezein` aliases `squeeze`.
        ("out", "set_out"),
        ("mix", "set_mix"),
        ("squeeze", "set_squeeze"),
        ("squeezeout", "set_squeezeout"),
        ("squeezeOut", "set_squeezeout"),
        ("squeezein", "set_squeeze"),
        ("squeezeIn", "set_squeeze"),
        ("reset", "set_reset"),
        ("restart", "set_restart"),
        ("poly", "set_poly"),
        // per-operator squeezein aliases for the arithmetic composers
        ("add_squeezein", "add_squeeze"),
        ("sub_squeezein", "sub_squeeze"),
        ("mul_squeezein", "mul_squeeze"),
        ("div_squeezein", "div_squeeze"),
        ("set_squeezein", "set_squeeze"),
        ("keep_squeezein", "keep_squeeze"),
        // @strudel/draw animate transform (zoomIn over x/y/w/h params)
        ("zoomIn", "zoom_in"),
        ("stack", "overlay"),
        // patternified amounts, as upstream registers them
        ("degradeBy", "degrade_by"),
        ("undegradeBy", "undegrade_by"),
        ("moveXY", "move_xy"),
        ("toBipolar", "to_bipolar"),
        ("fromBipolar", "from_bipolar"),
        ("inv", "invert"),
        ("innerJoin", "inner_join"),
        ("outerJoin", "outer_join"),
        ("squeezeJoin", "squeeze_join"),
        ("stepJoin", "step_join"),
        ("polyJoin", "poly_join"),
        ("resetJoin", "reset_join"),
        ("restartJoin", "restart_join"),
        ("someCycles", "some_cycles"),
        ("almostAlways", "almost_always"),
        ("almostNever", "almost_never"),
        ("juxFlip", "jux_flip"),
        ("flux", "jux_flip"),
        ("iterBack", "iter_back"),
        ("repeatCycles", "repeat_cycles"),
        ("rootNotes", "root_notes"),
        // deprecated Strudel stepwise aliases
        ("s_taper", "shrink"),
        ("s_add", "take"),
        ("s_sub", "drop"),
        ("s_expand", "expand"),
        ("s_extend", "extend"),
        ("s_contract", "contract"),
        ("pressBy", "press_by"),
        ("loopAt", "loop_at"),
        ("steps", "pace"),
        ("swingBy", "swing_by"),
        ("chunkBack", "chunk_back"),
        ("fastChunk", "fast_chunk"),
        ("fastchunk", "fast_chunk"),
        ("slowChunk", "chunk"),
        ("juxBy", "jux_by"),
        ("juxFlipBy", "jux_flip_by"),
        ("fluxBy", "jux_flip_by"),
        ("sometimesBy", "sometimes_by"),
        ("someCyclesBy", "some_cycles_by"),
        // `mod` is Strudel's spelling; `modulo` is the one Rust can use.
        ("mod", "modulo"),
    ] {
        p.alias(alias, name);
    }
}
