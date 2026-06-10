#![allow(non_snake_case)]

use super::KPattern;
use super::args::*;
use super::callback::with_callback;
use super::methods::*;
use koto::derive::*;
use koto::prelude::*;
use koto::runtime::Result as KotoResult;

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

            #[koto_method]
            fn pick(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pick(ctx)
            }

            #[koto_method]
            fn pickmod(ctx: MethodContext<Self>) -> KotoResult<KValue> {
                kpattern_pickmod(ctx)
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
        note, n, s, mpe, gain, postgain, pan, speed, cutoff, resonance, room, roomlp, roomdim,
        roomfade, size, shape, crush, delay,
        delaytime, delayfeedback, dry, attack, decay, sustain, release, vowel, bank, cut, accelerate, coarse,
        orbit, velocity, begin, end, legato, clip,
        hcutoff, hresonance, bandf, bandq, ftype,
        // filter envelopes + short aliases
        lpenv, lpattack, lpdecay, lpsustain, lprelease,
        hpenv, hpattack, hpdecay, hpsustain, hprelease,
        bpenv, bpattack, bpdecay, bpsustain, bprelease, fanchor,
        lpe, lpa, lpd, lps, lpr, hpe, hpa, hpd, hps, hpr, bpe, bpa, bpd, bps, bpr,
        // supersaw + FM + ADSR shortcuts
        unison, detune, spread, fm, fmh, fmi, fmwave, fmattack, fmdecay, fmsustain, fmrelease,
        fmi2, fmh2, fmwave2, fmattack2, fmdecay2, fmsustain2, fmrelease2,
        pw, noise, pcurve, adsr, ad, ar, hold,
        // vibrato + pitch envelope (+ aliases)
        vib, vibmod, penv, pattack, pdecay, psustain, prelease, panchor,
        vibrato, vmod, patt, pdec, psus, prel,
        // post-fx: tremolo + phaser
        tremolo, tremolodepth, phaser, phaserrate, phaserdepth, phasercenter, phasersweep,
        // filter / envelope / misc aliases
        lpf, lp, ctf, lpq, hpf, hp, hpq, bpf, bp, bpq, vel, att, rel, sus, dec,
        delayt, delayfb, rlp, rdim, rfade, o, trans, strans,
        // alignment matrix (`in` is the default plain op; these are the rest)
        add_out, add_mix, add_squeeze, add_squeezeout, add_reset, add_restart,
        sub_out, mul_out, mul_squeeze, div_out,
        set_out, set_mix, set_squeeze, set_squeezeout,
        keep_out, keep_squeeze,
        add_poly, mul_poly, set_poly, keep_poly,
        transpose, scale_transpose, bend_range,
        overlay, arp,
        // tonal / voicing controls
        mtranspose, ctranspose, dictionary, dict, anchor, offset, octaves, mode,
        // OSC routing controls
        oschost, oscport,
        // wavetable position / envelope (+ aliases)
        wt, wtenv, wtattack, wtdecay, wtsustain, wtrelease, wtrate, wtsync, wtdepth,
        wtshape, wtdc, wtskew, wtphaserand, wtatt, wtdec, wtsus, wtrel,
        // wavetable warp (+ aliases)
        warp, warpenv, warpattack, warpdecay, warpsustain, warprelease, warprate,
        warpsync, warpdepth, warpshape, warpdc, warpskew, warpmode,
        warpatt, warpdec, warpsus, warprel,
        // sound / amplitude / sample-window extras (+ aliases)
        source, src, amp, stretch, duration, dur, gate, gat,
        // filter LFO modulation (+ aliases)
        lprate, lpsync, lpdepth, lpdepthfrequency, lpdepthfreq, lpshape, lpdc, lpskew,
        bprate, bpsync, bpdepth, bpdepthfrequency, bpdepthfreq, bpshape, bpdc, bpskew,
        hprate, hpsync, hpdepth, hpdepthfrequency, hpdepthfreq, hpshape, hpdc, hpskew,
        // delay extras + DJ filter (+ aliases)
        delayspeed, delaysync, delays, ds, dfb, dt, djf, lock,
        // tremolo extras (+ aliases)
        tremolosync, tremoloskew, tremolophase, tremoloshape,
        trem, tremdepth, tremskew, tremphase, tremshape,
        // phaser aliases
        ph, phs, phc, phd, phasdp,
        // fx: chorus / drive / ducking / channels / pw LFO / leslie (+ aliases)
        chorus, drive, duckorbit, duck, duckdepth, duckonset, duckons,
        duckattack, duckatt, datt, channels, ch, channel, pwrate, pwr, pwsweep, pws,
        leslie, lrate, lsize,
        // tonal / spatial extras (+ aliases)
        degree, harmonic, nudge, octave, oct, bus, busgain, bgain, overgain, overshape,
        panspan, pansplay, panwidth, panorient, slide, semitone, voice,
        // impulse-response reverb + distortion + compressor (+ aliases)
        ir, iresponse, irspeed, irbegin, roomsize, sz, rsize,
        distort, dist, distortvol, distvol, distorttype, disttype, compressor,
        // SuperDirt / SuperDough misc
        analyze, fft, squiz, waveloss, density, expression, sustainpedal,
        fshift, fshiftnote, fshiftphase, triode, krush, kcutoff,
        octer, octersub, octersubsub, ring, ringf, ringdf, freeze,
        xsdelay, tsdelay, real, imag, enhance, comb, smear, scram,
        binshift, hbrick, lbrick, frames, hours, minutes, seconds, uid, val,
        // ZZFX
        zrand, curve, znoise, zmod, zcrush, zdelay, zzfx,
        // visuals / event metadata (+ aliases)
        color, colour, transient,
        // synth aliases + FM envelope
        det, fmenv, fme, fmatt, fmdec, fmsus, fmrel, v,
        // byte-beat / FX-release lowercase aliases
        bbexpr, bb, bbst, fxr,
        // MIDI controls + multi-control helpers (`control` sets ccn/ccv,
        // `sysex` sets sysexid/sysexdata) + sample scrubbing
        midichan, midimap, midiport, midicmd, ccn, ccv, nrpnn, nrpv,
        sysexid, sysexdata, midibend, miditouch,
        control, sysex, scrub,
        // numbered FM operators, matrix edges, and aliases
        fmh3, fmh4, fmh5, fmh6, fmh7, fmh8, fmi3, fmi4, fmi5, fmi6, fmi7, fmi8,
        fmenv2, fmenv3, fmenv4, fmenv5, fmenv6, fmenv7, fmenv8, fmattack3, fmattack4, fmattack5, fmattack6, fmattack7,
        fmattack8, fmdecay3, fmdecay4, fmdecay5, fmdecay6, fmdecay7, fmdecay8, fmsustain3, fmsustain4, fmsustain5, fmsustain6, fmsustain7,
        fmsustain8, fmrelease3, fmrelease4, fmrelease5, fmrelease6, fmrelease7, fmrelease8, fmwave3, fmwave4, fmwave5, fmwave6, fmwave7,
        fmwave8, fmi00, fmi01, fmi02, fmi03, fmi04, fmi05, fmi06, fmi07, fmi08, fmi10, fmi11,
        fmi12, fmi13, fmi14, fmi15, fmi16, fmi17, fmi18, fmi20, fmi21, fmi22, fmi23, fmi24,
        fmi25, fmi26, fmi27, fmi28, fmi30, fmi31, fmi32, fmi33, fmi34, fmi35, fmi36, fmi37,
        fmi38, fmi40, fmi41, fmi42, fmi43, fmi44, fmi45, fmi46, fmi47, fmi48, fmi50, fmi51,
        fmi52, fmi53, fmi54, fmi55, fmi56, fmi57, fmi58, fmi60, fmi61, fmi62, fmi63, fmi64,
        fmi65, fmi66, fmi67, fmi68, fmi70, fmi71, fmi72, fmi73, fmi74, fmi75, fmi76, fmi77,
        fmi78, fmi80, fmi81, fmi82, fmi83, fmi84, fmi85, fmi86, fmi87, fmi88, fmh1, fmi1,
        fm1, fmenv1, fmattack1, fmwave1, fmdecay1, fmsustain1, fmrelease1, fm2, fm3, fm4, fm5, fm6,
        fm7, fm8, fme1, fme2, fme3, fme4, fme5, fme6, fme7, fme8, fmatt1, fmatt2,
        fmatt3, fmatt4, fmatt5, fmatt6, fmatt7, fmatt8, fmdec1, fmdec2, fmdec3, fmdec4, fmdec5, fmdec6,
        fmdec7, fmdec8, fmsus1, fmsus2, fmsus3, fmsus4, fmsus5, fmsus6, fmsus7, fmsus8, fmrel1, fmrel2,
        fmrel3, fmrel4, fmrel5, fmrel6, fmrel7, fmrel8, fm00, fm01, fm02, fm03, fm04, fm05,
        fm06, fm07, fm08, fm10, fm11, fm12, fm13, fm14, fm15, fm16, fm17, fm18,
        fm20, fm21, fm22, fm23, fm24, fm25, fm26, fm27, fm28, fm30, fm31, fm32,
        fm33, fm34, fm35, fm36, fm37, fm38, fm40, fm41, fm42, fm43, fm44, fm45,
        fm46, fm47, fm48, fm50, fm51, fm52, fm53, fm54, fm55, fm56, fm57, fm58,
        fm60, fm61, fm62, fm63, fm64, fm65, fm66, fm67, fm68, fm70, fm71, fm72,
        fm73, fm74, fm75, fm76, fm77, fm78, fm80, fm81, fm82, fm83, fm84, fm85,
        fm86, fm87, fm88,
    ],
    no_arg: [
        rev, revv, palindrome, degrade, undegrade, press, brak, round, floor, ceil,
        to_bipolar, from_bipolar, ratio, fit, arpeggiate, voicing, piano,
    ],
    i64_arg: [
        iter, iter_back, repeat_cycles, expand, extend, contract, shrink, grow,
        chop, striate, take, drop, root_notes,
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
        bendRange => bend_range, fastGap => fast_gap, scaleTranspose => scale_transpose,
        scaleTrans => strans,
        // camelCase control names (Camel => snake builder writing the Strudel key)
        wavetablePosition => wt, wavetableWarp => warp, wavetableWarpMode => warpmode,
        wavetablePhaseRand => wtphaserand,
        stepsPerOctave => steps_per_octave, octaveR => octave_r,
        ctlNum => ctl_num, progNum => prog_num, polyTouch => poly_touch,
        compressorKnee => compressor_knee, compressorRatio => compressor_ratio,
        compressorAttack => compressor_attack, compressorRelease => compressor_release,
        frameRate => frame_rate, songPtr => song_ptr, deltaSlide => delta_slide,
        pitchJump => pitch_jump, pitchJumpTime => pitch_jump_time,
        fadeTime => fade_time, fadeOutTime => fade_time, fadeInTime => fade_in_time,
        byteBeatExpression => byte_beat_expression, byteBeatStartTime => byte_beat_start_time,
        FXrelease => fx_release, FXrel => fx_release, FXr => fx_release,
    ],
    camel_literal_or_pattern: [withBase => with_base, fTrans => ftrans, fTranspose => ftranspose],
    camel_no_arg: [toBipolar => to_bipolar, fromBipolar => from_bipolar],
    camel_noarg_fn: [someCycles => some_cycles, almostAlways => almost_always, almostNever => almost_never],
    camel_i64: [iterBack => iter_back, repeatCycles => repeat_cycles, rootNotes => root_notes],
    camel_frac: [pressBy => press_by, loopAt => loop_at],
    camel_frac_frac: [swingBy => swing_by],
    camel_i64_i64: [euclidLegato => euclid_legato],
    camel_i64_i64_i64: [euclidRot => euclid_rot, euclidLegatoRot => euclid_legato_rot],
    camel_i64_fn: [firstOf => first_of, lastOf => last_of, chunkBack => chunk_back],
    camel_f64_fn: [juxBy => jux_by, sometimesBy => sometimes_by, someCyclesBy => some_cycles_by],
}
