use super::pattern::{
    arg_to_f64, arg_to_group, arg_to_pattern, arg_to_pattern_weight, arg_to_raw_str, arg_to_value,
    arg_to_weighted_pair, arg0, bjork_counts, euclid_call, pick_args, stepwise_call, to_value,
};
use crate::js::{self, Arg, NULL, Scope};
use rudel_core::{Frac, Pattern, PickJoin, Value};

/// The pattern a side-effecting call hands back, so it can sit on a line of
/// its own.
fn done() -> Result<Arg, String> {
    Ok(rudel_core::silence().into())
}

/// Register the standalone form of pattern transforms that are also methods,
/// taking the pattern as the *last* argument to mirror Strudel's `register`ed
/// functions (`fast(2, pat)` == `pat.fast(2)`). Each group matches the argument
/// types in `generated.rs`. Calls missing the trailing pattern partially apply.
macro_rules! register_pattern_fns {
    ($p:expr;
     pattern1: [$($n_a1:literal => $a1:ident),* $(,)?];
     noarg:    [$($n_a0:literal => $a0:ident),* $(,)?];
     i64_1:    [$($n_b1:literal => $b1:ident),* $(,)?];
     f64_1:    [$($n_h1:literal => $h1:ident),* $(,)?];
     frac1:    [$($n_c1:literal => $c1:ident),* $(,)?];
     f64_2:    [$($n_d2:literal => $d2:ident),* $(,)?];
     frac2:    [$($n_e2:literal => $e2:ident),* $(,)?];
     i64_frac_f64: [$($n_ja:literal => $ja:ident),* $(,)?];
     i64_f64_frac: [$($n_jb:literal => $jb:ident),* $(,)?];
     pat2:     [$($n_g2:literal => $g2:ident),* $(,)?];
    ) => {{
        // The pattern is the last argument; leading arg `i` exists only when
        // `i < last` (otherwise it would be the pattern itself). Each arity is
        // the full argument count; shorter calls curry.
        fn lead(a: &[Arg], i: usize) -> &Arg {
            let last = a.len().saturating_sub(1);
            a.get(i).filter(|_| last > i).unwrap_or(NULL)
        }
        fn subject(a: &[Arg]) -> Pattern {
            arg_to_pattern(a.last().unwrap_or(NULL))
        }
        $($p.curried($n_a1, 2, |a| Ok(subject(a).$a1(arg_to_pattern(lead(a, 0))).into()));)*
        // Strudel's `register` curries, so calling one of these with no
        // argument hands back the transform rather than applying it to
        // nothing: `.sometimesBy(0.8, rev())` passes a function.
        $($p.curried($n_a0, 1, |a| Ok(subject(a).$a0().into()));)*
        $($p.curried($n_b1, 2, |a| Ok(subject(a).$b1(arg_to_f64(lead(a, 0)) as i64).into()));)*
        $($p.curried($n_h1, 2, |a| Ok(subject(a).$h1(arg_to_f64(lead(a, 0))).into()));)*
        $($p.curried($n_c1, 2, |a| {
            Ok(subject(a).$c1(Frac::from_f64(arg_to_f64(lead(a, 0)))).into())
        });)*
        $($p.curried($n_d2, 3, |a| {
            Ok(subject(a).$d2(arg_to_f64(lead(a, 0)), arg_to_f64(lead(a, 1))).into())
        });)*
        $($p.curried($n_e2, 3, |a| {
            let x = Frac::from_f64(arg_to_f64(lead(a, 0)));
            let y = Frac::from_f64(arg_to_f64(lead(a, 1)));
            Ok(subject(a).$e2(x, y).into())
        });)*
        $($p.curried($n_ja, 4, |a| {
            let x = arg_to_f64(lead(a, 0)) as i64;
            let y = Frac::from_f64(arg_to_f64(lead(a, 1)));
            let z = arg_to_f64(lead(a, 2));
            Ok(subject(a).$ja(x, y, z).into())
        });)*
        $($p.curried($n_jb, 4, |a| {
            let x = arg_to_f64(lead(a, 0)) as i64;
            let y = arg_to_f64(lead(a, 1));
            let z = Frac::from_f64(arg_to_f64(lead(a, 2)));
            Ok(subject(a).$jb(x, y, z).into())
        });)*
        $($p.curried($n_g2, 3, |a| {
            Ok(subject(a).$g2(arg_to_pattern(lead(a, 0)), arg_to_pattern(lead(a, 1))).into())
        });)*
    }};
}

/// The patterns of every argument, as `stack(a, b, c)` takes them.
fn patterns(a: &[Arg]) -> Vec<Pattern> {
    a.iter().map(arg_to_pattern).collect()
}

/// Add the rudel top-level functions to the global scope.
pub(crate) fn register(prelude: &Scope) {
    // Pattern methods first: the controls below skip any name already bound.
    let proto = Scope::pattern();
    super::pattern::register_methods(&proto);
    // `osc`, `noise`, `shape`, ... — the hydra sources that start a chain.
    super::hydra::register(prelude);
    // `Kabel.sine(220).out()` — the kabelsalat node DSL behind `K(...)`.
    super::kabelsalat::register(prelude, &proto);
    // `Pattern`, `Hap`, `Fraction`, `TimeSpan` — the engine's own vocabulary,
    // plus the standalone forms of the transforms that take a span.
    super::pattern::register_engine_fns(prelude);
    super::pattern::register_span_fns(prelude);

    for (name, f) in [
        ("note", rudel_core::note as fn(Pattern) -> Pattern),
        ("n", rudel_core::n),
        ("i", rudel_core::i),
        ("freq", rudel_core::freq),
        ("mpe", rudel_core::mpe),
        ("bendRange", rudel_core::bend_range),
        ("s", rudel_core::s),
        ("sound", rudel_core::sound),
    ] {
        prelude.func(name, move |a| Ok(f(arg_to_pattern(arg0(a))).into()));
    }
    prelude.func("getFreq", |a| {
        Ok(rudel_core::get_freq(&to_value(arg0(a)))
            .unwrap_or(0.0)
            .into())
    });
    // Scalar conversion/util helpers from core/util.mjs that a user can reach
    // from the REPL (the rest of util.mjs is registration/curry/hashing/keyboard
    // plumbing). These operate on numbers, not patterns.
    prelude.func("midiToFreq", |a| {
        Ok(rudel_core::midi_to_freq(arg_to_f64(arg0(a))).into())
    });
    prelude.func("freqToMidi", |a| {
        Ok(rudel_core::freq_to_midi(arg_to_f64(arg0(a))).into())
    });
    prelude.func("noteToMidi", |a| {
        // noteToMidi(note, defaultOctave = 3); throws on a non-note, like Strudel.
        let value = to_value(arg0(a));
        let default_octave = a.get(1).map(arg_to_f64).unwrap_or(3.0) as i32;
        match value.as_str() {
            Some(s) => match rudel_core::note_to_midi_with_octave(s, default_octave) {
                Some(m) => Ok((m as f64).into()),
                None => Err(format!("noteToMidi: not a note: \"{s}\"")),
            },
            None => Err("noteToMidi: expected a note string".to_string()),
        }
    });
    prelude.func("clamp", |a| {
        // clamp(num, min, max) = min(max(num, min), max).
        let num = arg_to_f64(arg0(a));
        let lo = a.get(1).map(arg_to_f64).unwrap_or(0.0);
        let hi = a.get(2).map(arg_to_f64).unwrap_or(1.0);
        Ok(num.max(lo).min(hi).into())
    });
    // `silence` is a pattern, not a function, as upstream. `nothing` is its
    // other name.
    prelude.value("silence", rudel_core::silence());
    prelude.value("nothing", rudel_core::silence());
    // hush(): clear the REPL pattern slots and return silence (core/repl.mjs).
    prelude.func("hush", |_| {
        super::pattern::reset_slots();
        done()
    });
    // clearScope(): upstream deletes the user variables block-based eval leaked
    // into the shared `strudelScope`. Rudel runs each evaluation in a fresh
    // engine, so nothing accumulates across blocks and there is nothing to
    // delete; the persistent state Rudel *does* keep is the slot registry,
    // which is already cleared per eval. So this returns silence, as upstream
    // does, and is otherwise a no-op (like `registerSoundfonts()`).
    prelude.func("clearScope", |_| done());
    // getDuration(name[, n]) / getDur: the length in seconds of a loaded
    // sample, so a pattern can set its tempo from it
    // (`setcps(1 / getDuration('sax'))`). Upstream returns a promise resolved
    // from the decoded AudioBuffer; Rudel's bank publishes the length as it
    // registers each sample, so this returns the number directly — no `await`.
    // An unknown sound (or one not loaded yet) reads as 0, like an unseen CC.
    for name in ["getDuration", "getDur"] {
        prelude.func(name, |a| {
            let sound = a.first().and_then(arg_to_raw_str).unwrap_or_default();
            let n = a.get(1).map(arg_to_f64).unwrap_or(0.0).round() as i64;
            Ok(rudel_core::sample_duration(&sound, n).unwrap_or(0.0).into())
        });
    }
    // Strudel-style chord control: `chord("<Am C>").voicing()`.
    prelude.func("chord", |a| {
        Ok(rudel_core::control_dyn("chord", arg_to_pattern(arg0(a))).into())
    });
    // `$:`/`name:` labels: register the pattern into the slot registry so it is
    // picked up by `applyPatternTransforms` (stacking, `each`/`all`, soloing),
    // exactly like Strudel's transpiler-injected `.p(label)`.
    prelude.func("rudel_label", |a| {
        let name = match arg0(a) {
            Arg::Str(s) => s.clone(),
            _ => String::new(),
        };
        let pat = a
            .get(1)
            .map(arg_to_pattern)
            .unwrap_or_else(rudel_core::silence);
        Ok(super::pattern::register_slot(&name, pat).into())
    });
    // all(f): apply `f` to all running patterns stacked together (core/repl.mjs).
    // each(f): apply `f` to each running pattern separately. Both take a
    // function value (`rev`, `x => x.fast(2)`) and return silence so they can
    // sit on their own line. Patterns must be labeled (`$:`) or slotted
    // (`.d1()`) to be picked up.
    prelude.func("all", |a| {
        super::pattern::push_all(arg0(a).clone());
        done()
    });
    prelude.func("each", |a| {
        super::pattern::set_each(arg0(a).clone());
        done()
    });
    for (name, f) in [
        ("stack", rudel_core::stack as fn(&[Pattern]) -> Pattern),
        ("polyrhythm", rudel_core::stack), // Strudel alias: polyrhythm = stack
        ("pr", rudel_core::stack),
        ("stackLeft", rudel_core::stack_left),
        ("stackRight", rudel_core::stack_right),
        ("stackCentre", rudel_core::stack_centre),
        ("stackCenter", rudel_core::stack_centre), // US spelling
        ("cat", rudel_core::cat),
        ("seq", rudel_core::fastcat),
        ("sequence", rudel_core::fastcat),
        ("fastcat", rudel_core::fastcat),
        ("slowcat", rudel_core::slowcat),
        // chooseCycles is randcat over reified args.
        ("randcat", rudel_core::randcat),
        ("chooseCycles", rudel_core::randcat),
        // zip: interleave the steps of the given patterns into one dense cycle.
        ("zip", rudel_core::zip),
        ("s_zip", rudel_core::zip), // deprecated Strudel alias
        // polymeter / pm: align patterns to a common (LCM) step count.
        ("polymeter", rudel_core::polymeter),
        ("pm", rudel_core::polymeter),
        ("s_polymeter", rudel_core::polymeter), // deprecated Strudel alias
        // choose / chooseOut / chooseIn: continuously pick from the given
        // values. `choose`/`chooseOut` take structure from the random chooser;
        // `chooseIn` takes it from the chosen values.
        ("choose", rudel_core::choose),
        ("chooseOut", rudel_core::choose),
        ("chooseIn", rudel_core::choose_in),
    ] {
        prelude.func(name, move |a| Ok(f(&patterns(a)).into()));
    }
    // `parray([p0, p1, ...])`: pack one value from each pattern into a list
    // value per hap. Strudel passes a single array; also accept bare varargs.
    prelude.func("parray", |a| {
        let pats = if a.len() == 1 {
            arg_to_group(&a[0])
        } else {
            patterns(a)
        };
        Ok(rudel_core::parray(&pats).into())
    });
    // stackBy(mode, ...pats): dispatch to a step-alignment by mode name.
    // (Strudel patternifies `mode`; here it is taken as a constant string.)
    prelude.func("stackBy", |a| {
        let mode = to_value(arg0(a));
        let pats = patterns(a.get(1..).unwrap_or(&[]));
        let out = match mode.as_str().unwrap_or("expand") {
            "left" => rudel_core::stack_left(&pats),
            "right" => rudel_core::stack_right(&pats),
            "centre" | "center" => rudel_core::stack_centre(&pats),
            "repeat" => rudel_core::polymeter(&pats),
            _ => rudel_core::stack(&pats), // "expand"
        };
        Ok(out.into())
    });

    // -- Factories ---------------------------------------------------------
    prelude.func("pure", |a| {
        Ok(rudel_core::pure(arg_to_value(arg0(a))).into())
    });
    prelude.func("gap", |a| {
        let n = arg_to_f64(arg0(a)) as i64;
        Ok(rudel_core::gap(Frac::int(n.max(0))).into())
    });
    // stepcat / timecat: weighted stepwise concatenation. Each arg is either a
    // pattern (weight = its step count) or a `[weight, pattern]` pair.
    for name in ["stepcat", "timecat", "timeCat", "s_cat"] {
        prelude.func(name, |a| {
            let pairs: Vec<(Frac, Pattern)> = a.iter().map(arg_to_weighted_pair).collect();
            Ok(rudel_core::timecat(&pairs).into())
        });
    }
    // arrange: each arg is a `[cycles, pattern]` section laid out on a timeline.
    prelude.func("arrange", |a| {
        let sections: Vec<(Frac, Pattern)> = a.iter().map(arg_to_weighted_pair).collect();
        Ok(rudel_core::arrange(&sections).into())
    });
    // wchoose: continuously choose from weighted [pattern, weight] pairs.
    prelude.func("wchoose", |a| {
        let pairs: Vec<(Pattern, f64)> = a.iter().map(arg_to_pattern_weight).collect();
        Ok(rudel_core::wchoose(&pairs).into())
    });
    // wchooseCycles / wrandcat: pick one weighted pattern per cycle.
    for name in ["wchooseCycles", "wrandcat"] {
        prelude.func(name, |a| {
            let pairs: Vec<(Pattern, f64)> = a.iter().map(arg_to_pattern_weight).collect();
            Ok(rudel_core::wrandcat(&pairs).into())
        });
    }
    // stepalt: alternate stepwise between groups of patterns.
    for name in ["stepalt", "s_alt"] {
        prelude.func(name, |a| {
            let groups: Vec<Vec<Pattern>> = a.iter().map(arg_to_group).collect();
            Ok(rudel_core::stepalt(&groups).into())
        });
    }
    // The pick family (strudel core/pick.mjs): select patterns from an array
    // (by index) or an object (by name) via a selector pattern. `pickmod*`
    // wraps out-of-range indices instead of clamping; the suffix picks the
    // join. squeeze(pat, xs): pick from a list with wrapping, squeezing the
    // picked pattern into the selecting event (strudel's standalone `squeeze`).
    for (name, modulo, join) in [
        ("pick", false, PickJoin::Inner),
        ("pickmod", true, PickJoin::Inner),
        ("pickOut", false, PickJoin::Outer),
        ("pickmodOut", true, PickJoin::Outer),
        ("pickReset", false, PickJoin::Reset),
        ("pickmodReset", true, PickJoin::Reset),
        ("pickRestart", false, PickJoin::Restart),
        ("pickmodRestart", true, PickJoin::Restart),
        ("inhabit", false, PickJoin::Squeeze),
        ("pickSqueeze", false, PickJoin::Squeeze),
        ("inhabitmod", true, PickJoin::Squeeze),
        ("pickmodSqueeze", true, PickJoin::Squeeze),
        ("squeeze", true, PickJoin::Squeeze),
    ] {
        prelude.func(name, move |a| Ok(pick_args(a, modulo, join).into()));
    }
    prelude.func("pat", |a| Ok(arg_to_pattern(arg0(a)).into()));
    // `useRNG(mode)` picks the random generator (signal.mjs). Rudel ports only
    // the legacy one — Strudel's default, and what the tunes that call this ask
    // for — so `'legacy'` is a no-op and `'precise'` says so rather than
    // quietly handing back different random numbers.
    prelude.func("useRNG", |a| match arg_to_raw_str(arg0(a)).as_deref() {
        None | Some("legacy") => done(),
        Some(mode) => Err(format!(
            "useRNG({mode:?}): only the legacy RNG is ported; \
             remove the call or use useRNG('legacy')"
        )),
    });
    // `setVoicingRange(name, [low, high])` (tonal/voicings.mjs) narrows a
    // dictionary's register. Upstream it only reaches the deprecated
    // `.voicings(dict)` path — `.voicing()` aligns by `mode`/`anchor` and never
    // reads `range` — and Rudel's voicing does not model a range on either
    // path, so this is a no-op rather than an error: rejecting it would stop a
    // tune that upstream runs identically without it. Dinofunk pins that in
    // `tunes.rs`, matching Strudel's own haps with the call ignored.
    prelude.func("setVoicingRange", |_| done());
    // `register(name, fn)` (core/pattern.mjs) defines a new pattern method,
    // with the pattern as the callback's last argument. Returns the function,
    // as upstream does, so `const f = register(...)` still binds something.
    // An array of names registers each, and returns them as an object, which
    // is how `const {beat, beatOut} = register(['beat', 'beatOut'], f)` reads.
    prelude.func("register", |a| {
        let names: Vec<String> = match arg0(a) {
            Arg::List(names) => names.iter().filter_map(arg_to_raw_str).collect(),
            one => arg_to_raw_str(one).into_iter().collect(),
        };
        if names.is_empty() {
            return Err("register(name, fn): name must be a string".to_string());
        }
        let func = a.get(1).unwrap_or(NULL);
        if !func.is_callable() {
            return Err("register(name, fn) needs a name and a function".to_string());
        }
        // `register(name, fn, patternify = true)`. A helper that says `false`
        // does its own `reify`/join on the argument and would be handed a
        // per-cycle sample instead of the pattern it means to work on.
        let patternify = !matches!(a.get(2), Some(Arg::Bool(false)));
        for name in &names {
            super::pattern::register_pattern_method(name, func, patternify);
        }
        Ok(match arg0(a) {
            Arg::List(_) => Arg::Map(names.into_iter().map(|n| (n, func.clone())).collect()),
            _ => func.clone(),
        })
    });
    // `id(x)`: upstream's identity function, which scripts pass where a
    // transform is wanted and none is.
    prelude.func("id", |a| Ok(arg0(a).clone()));
    // `signal(t => value)`: a continuous pattern whose value at each query is
    // the function of the query's start. The function runs from the query, so
    // on the engine's thread (see `convert::fn_to_value`).
    prelude.func("signal", |a| {
        let Value::Func(f) = super::pattern::fn_to_value(arg0(a)) else {
            return Err("signal(fn) needs a function".to_string());
        };
        Ok(rudel_core::signal::signal(move |t| f(Value::F64(t.to_f64()))).into())
    });
    // `addVoicings(name, dictionary, range)` (tonal/voicings.mjs) registers a
    // chord dictionary a later `.voicing(name)` can name. `range` is accepted
    // and ignored for the reason `setVoicingRange` above is a no-op: upstream
    // reads it only on the deprecated `.voicings(dict)` path.
    prelude.func("addVoicings", |a| {
        let (Some(name), Some(Arg::Map(dictionary))) = (arg_to_raw_str(arg0(a)), a.get(1)) else {
            return Err(
                "addVoicings(name, dictionary) needs a name and an object of chord symbols"
                    .to_string(),
            );
        };
        let entries: Vec<(String, Vec<String>)> = dictionary
            .iter()
            .filter_map(|(symbol, voicings)| {
                // A single voicing may be written without its array, as one
                // string.
                let voicings = match voicings {
                    Arg::List(l) => l.iter().filter_map(arg_to_raw_str).collect(),
                    other => vec![arg_to_raw_str(other)?],
                };
                Some((symbol.clone(), voicings))
            })
            .collect();
        rudel_core::voicing::add_voicings(&name, entries);
        done()
    });
    // `voicingRegistry` (tonal/voicings.mjs): `{name: {dictionary}}`, read by
    // a script extending a dictionary rather than starting one from nothing.
    let strings = |list: Vec<String>| Arg::List(list.into_iter().map(Arg::Str).collect());
    let registry = rudel_core::voicing::voicing_dictionaries()
        .into_iter()
        .map(|(name, table)| {
            let table = table.into_iter().map(|(k, v)| (k, strings(v))).collect();
            (
                name,
                Arg::Map(vec![("dictionary".to_string(), Arg::Map(table))]),
            )
        })
        .collect();
    prelude.value("voicingRegistry", Arg::Map(registry));
    // `setDefaultVoicings(name)` (tonal/voicings.mjs) picks the dictionary a
    // later bare `.voicing()` reads. Process-global upstream and here; songs
    // call it once at the top.
    prelude.func("setDefaultVoicings", |a| {
        if let Some(dict) = arg_to_raw_str(arg0(a)) {
            rudel_core::voicing::set_default_voicings(dict);
        }
        done()
    });
    // `mini(...strings)`: each argument parsed as mini-notation, laid out across
    // the cycle (mini/mini.mjs). A bare string reaching an ordinary pattern
    // argument is deliberately *not* mini -- see `arg_to_pattern` -- but this
    // call is the parser itself, and it is how a tune plays a mini string it
    // built at runtime, which is the only way it can be parsed at all.
    prelude.func("mini", |a| {
        let pats: Vec<Pattern> = a
            .iter()
            .map(|arg| match arg_to_raw_str(arg) {
                Some(text) => rudel_mini::parse_with_offset(&text, 0)
                    .unwrap_or_else(|_| rudel_core::silence())
                    .with_source(text),
                None => arg_to_pattern(arg),
            })
            .collect();
        Ok(rudel_core::fastcat(&pats).into())
    });
    // The list-valued additive-synthesis controls, as standalone factories to
    // match their method forms.
    for key in ["partials", "phases"] {
        prelude.func(key, move |a| {
            Ok(rudel_core::control_dyn(key, rudel_core::pure(to_value(arg0(a)))).into())
        });
    }
    // m(value, offset): mini-notation with a source offset. Emitted by the
    // preprocessor for every double-quoted literal so per-hap locations are
    // absolute to the editor source. Numbers/patterns pass through unchanged.
    // The raw source text is remembered so raw-string consumers can recover it.
    prelude.func("m", |a| {
        let offset = a.get(1).map(arg_to_f64).unwrap_or(0.0) as usize;
        Ok(match arg0(a) {
            Arg::Str(s) => rudel_mini::parse_with_offset(s, offset)
                .unwrap_or_else(|_| rudel_core::silence())
                .with_source(s.as_str())
                .into(),
            other => arg_to_pattern(other).into(),
        })
    });
    // `setGainCurve(f)`: rescale every gain-like value through `f`, as
    // superdough's `applyGainCurve` does (gain, postgain, velocity, delay,
    // busgain, shapevol, distortvol, tremolodepth). The function is sampled
    // here, at evaluation time, because the audio path runs no script — see
    // `rudel_core::set_gain_curve` for the ceiling that carries.
    prelude.func("setGainCurve", |a| {
        let f = arg0(a);
        if !f.is_callable() {
            rudel_core::clear_gain_curve();
            return Ok(Arg::Null);
        }
        let step = rudel_core::GAIN_CURVE_MAX / (rudel_core::GAIN_CURVE_POINTS - 1) as f64;
        let mut samples = Vec::with_capacity(rudel_core::GAIN_CURVE_POINTS);
        for i in 0..rudel_core::GAIN_CURVE_POINTS {
            let out = js::call(f, vec![Arg::Num(i as f64 * step)])?;
            samples.push(arg_to_f64(&out));
        }
        rudel_core::set_gain_curve_samples(samples);
        Ok(Arg::Null)
    });
    // `setMaxPolyphony(n)`: the most voices allowed to sound at once. Past it
    // the mixer fades the oldest ones out, first in first out, as superdough
    // does when its `activeSoundSources` map outgrows the cap.
    prelude.func("setMaxPolyphony", |a| {
        // `parseInt(polyphony)`: a number, or the leading digits of a string.
        // Anything that does not read as one leaves the default standing,
        // which is what upstream's `?? DEFAULT_MAX_POLYPHONY` intends.
        let voices = match arg0(a) {
            Arg::Num(n) => Some(n.trunc()),
            Arg::Str(s) => {
                let digits: String = s
                    .trim_start()
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                digits.parse::<f64>().ok()
            }
            _ => None,
        };
        rudel_core::set_max_polyphony(match voices {
            Some(n) if n.is_finite() && n >= 0.0 => n as usize,
            _ => rudel_core::DEFAULT_MAX_POLYPHONY,
        });
        Ok(Arg::Null)
    });

    // Hydra's browser loader: `initHydra` fetches hydra-synth from a CDN and
    // `clearHydra` tears its canvas down. Rudel implements hydra natively (see
    // `crate::hydra`), so there is nothing to fetch and nothing to tear down —
    // accepted and ignored, so a pattern copied from Strudel still runs.
    //
    // `hydra` itself is deliberately *not* here any more: it is the widget
    // method that renders a chain, and a stub of that name would shadow it.
    for name in ["initHydra", "clearHydra"] {
        prelude.func(name, |_| Ok(Arg::Null));
    }
    // `H(pattern)` samples a pattern once per animation frame to drive a hydra
    // uniform. A chain here compiles once per evaluation, so its parameters are
    // constants for that evaluation's life and a per-frame value has nowhere to
    // go.
    for name in ["H", "P5", "p5"] {
        prelude.func(name, move |_| {
            rudel_core::log_line(format!("{name}: not supported here, ignored"));
            Ok(Arg::Null)
        });
    }
    // ``dough`…` ``: compile the given JavaScript into an AudioWorklet and run
    // its `dsp(t)` at sample rate (superdough/dspworklet.mjs). That needs a JS
    // engine on the audio thread, so the code is accepted and not run — and the
    // pattern it was installed for still plays, because `.dough()` renders the
    // bytebeat these worklets are always built to play. See `Pattern::dough`
    // for what that covers and what it does not.
    prelude.func("dough", |_| {
        rudel_core::log_line(
            "dough: the DSP worklet needs a JS engine on the audio thread and is \
             not run here; `.dough()` plays its pattern as bytebeat"
                .to_string(),
        );
        Ok(Arg::Null)
    });
    // `console.log` goes to the same place the pattern's own `log` does, which
    // is the console panel.
    let console = prelude.namespace("console");
    for level in ["log", "info", "warn", "error", "debug"] {
        console.func(level, |a| {
            let parts: Vec<String> = a.iter().map(js::display).collect();
            rudel_core::log_line(parts.join(" "));
            Ok(Arg::Null)
        });
    }

    // `reify(x)`: anything as a pattern. Strudel's own coercion, exposed
    // because scripts call it directly when building patterns by hand. The
    // result forgets the mini text it was parsed from: that text is what lets
    // `pure("x")` hold a string, and a script asking for a pattern means one —
    // `pure(reify(pat))` is a pattern of patterns, as `seqPLoop` builds it.
    prelude.func("reify", |a| {
        let mut pat = arg_to_pattern(arg0(a));
        pat.source = None;
        Ok(pat.into())
    });
    // `chooseWith(signal, [a, b, ...])` / `chooseInWith`: index the list with an
    // arbitrary 0..1 signal, taking structure from the signal or from the
    // chosen patterns respectively.
    for (name, in_form) in [("chooseWith", false), ("chooseInWith", true)] {
        prelude.func(name, move |a| {
            let chooser = arg_to_pattern(arg0(a));
            let pats: Vec<Pattern> = match a.get(1) {
                Some(other) => arg_to_group(other),
                None => Vec::new(),
            };
            Ok(if in_form {
                rudel_core::choose_in_with(chooser, &pats)
            } else {
                rudel_core::choose_with(chooser, &pats)
            }
            .into())
        });
    }
    // Integer-count signals and factories: scan (growing runs), irand, randrun
    // (0..n once each per cycle, in a random order), run, binary (bit patterns
    // of a number) and randL (a list of n random numbers).
    for (name, f) in [
        ("scan", rudel_core::scan as fn(i64) -> Pattern),
        ("irand", rudel_core::irand),
        ("randrun", rudel_core::randrun),
        ("run", rudel_core::run),
        ("binary", rudel_core::binary),
        ("randL", rudel_core::rand_l),
    ] {
        prelude.func(name, move |a| Ok(f(arg_to_f64(arg0(a)) as i64).into()));
    }
    // tour(pat, a, b, ...): standalone form of `pat.tour(a, b, ...)`.
    prelude.func("tour", |a| {
        let pats = patterns(a);
        let Some((head, many)) = pats.split_first() else {
            return done();
        };
        Ok(head.tour(many).into())
    });

    // -- Signals --------------------------------------------------------
    // Continuous signals are pattern *values* (like Strudel), so
    // `sine.range(0,1)` works without calling `sine()`.
    for (name, f) in [
        ("sine", rudel_core::sine as fn() -> Pattern),
        ("cosine", rudel_core::cosine),
        ("saw", rudel_core::saw),
        ("isaw", rudel_core::isaw),
        ("tri", rudel_core::tri),
        ("itri", rudel_core::itri),
        ("square", rudel_core::square),
        ("sine2", rudel_core::sine2),
        ("cosine2", rudel_core::cosine2),
        ("saw2", rudel_core::saw2),
        ("isaw2", rudel_core::isaw2),
        ("tri2", rudel_core::tri2),
        ("itri2", rudel_core::itri2),
        ("square2", rudel_core::square2),
        ("rand", rudel_core::rand),
        ("rand2", rudel_core::rand2),
        ("brand", rudel_core::brand),
        ("time", rudel_core::time),
        ("perlin", rudel_core::perlin),
        ("berlin", rudel_core::berlin),
        // Pointer position, 0..1 across the app window (Strudel reads the
        // browser's mousemove events; the egui app is the source here).
        ("mousex", rudel_core::mousex),
        ("mouseX", rudel_core::mousex),
        ("mousey", rudel_core::mousey),
        ("mouseY", rudel_core::mousey),
        // Event-duration signals (take structure from the pattern they meet).
        ("per", rudel_core::per),
        ("perCycle", rudel_core::per),
        ("cyclesPer", rudel_core::cycles_per),
        ("perx", rudel_core::perx),
    ] {
        prelude.value(name, f());
    }
    // brandBy(p): a 0/1 signal that is 1 with probability `p`.
    prelude.func("brandBy", |a| {
        Ok(rudel_core::brand_by(arg_to_f64(arg0(a))).into())
    });
    // steady(value): a continuous pattern of a single constant value.
    // slider(value, min?, max?, step?): Strudel's transpiler rewrites this to
    // sliderWithID(id, value, ...). The untranspiled fallback is steady(value).
    for name in ["steady", "slider"] {
        prelude.func(name, |a| {
            Ok(rudel_core::steady(arg_to_value(arg0(a))).into())
        });
    }
    for name in ["slider_with_id", "sliderWithID"] {
        prelude.func(name, |a| {
            let id = a.first().and_then(arg_to_raw_str).unwrap_or_default();
            let value = arg_to_value(a.get(1).unwrap_or(NULL));
            Ok(crate::sliders::slider_with_id(id, value).into())
        });
    }
    // binaryN(n, nBits) / binaryL(n) / binaryNL(n, nBits): bit patterns of a
    // number, and the bits packed into a list value. nBits defaults to 16, as
    // in Strudel.
    fn nbits_arg(a: &[Arg]) -> i64 {
        a.get(1).map(arg_to_f64).unwrap_or(16.0) as i64
    }
    prelude.func("binaryN", |a| {
        Ok(rudel_core::binary_n(arg_to_pattern(arg0(a)), nbits_arg(a)).into())
    });
    prelude.func("binaryL", |a| {
        Ok(rudel_core::binary_l(arg_to_pattern(arg0(a))).into())
    });
    prelude.func("binaryNL", |a| {
        Ok(rudel_core::binary_nl(arg_to_pattern(arg0(a)), nbits_arg(a)).into())
    });
    // morph(from, to, by): morph between two binary rhythms. `from`/`to` are
    // list-valued (a `[1,0,1,...]` array or a `"1:0:1:..."` mini list); `by` is
    // a 0→1 number or signal.
    prelude.func("morph", |a| {
        let list_or_pat = |v: &Arg| match v {
            Arg::List(_) => rudel_core::pure(to_value(v)),
            _ => arg_to_pattern(v),
        };
        let from = list_or_pat(arg0(a));
        let to = list_or_pat(a.get(1).unwrap_or(NULL));
        let by = arg_to_pattern(a.get(2).unwrap_or(NULL));
        Ok(rudel_core::morph(from, to, by).into())
    });
    // MIDI input: `ccin(cc)` / `ccin(cc, chan)` is a 0..1 signal of the latest
    // value of an incoming control-change (the input counterpart to `ccn`).
    prelude.func("ccin", |a| {
        let cc = arg_to_f64(arg0(a)) as u8;
        let chan = a.get(1).map(|v| arg_to_f64(v) as u8).filter(|c| *c >= 1);
        Ok(rudel_core::cc_in(cc, chan).into())
    });
    // Keyboard input: `keyDown("Control:j")` is a boolean signal that is true
    // while every named key is held. The argument is patternified like
    // Strudel's `register`, so a `:`-list is a combination and `<a b>`
    // alternates which key is watched.
    prelude.func("keyDown", |a| Ok(key_down_pattern(arg0(a)).into()));

    // Standalone (curried-style) forms of the transforms, so Strudel code
    // written as `fast(2, pat)` / `jux(rev, pat)` works as well as the method
    // forms, under both snake_case and Strudel's camelCase names. The
    // function-callback combinators are registered separately since their
    // `Callback` plumbing lives in the pattern module.
    super::pattern::register_standalone_callbacks(prelude);
    // Standalone `lfo`/`env`/`bmod` modulator factories (build on an empty map).
    super::pattern::register_modulate_fns(prelude);

    // euclid morph / tuple-euclid standalone forms (pattern last); their
    // signatures don't fit the `register_pattern_fns!` arg groups.
    for name in ["euclidish", "eish"] {
        prelude.func(name, |a| {
            let pulses = arg_to_f64(arg0(a)) as i64;
            let steps = arg_to_f64(a.get(1).unwrap_or(NULL)) as i64;
            let perc = arg_to_pattern(a.get(2).unwrap_or(NULL));
            let pat = arg_to_pattern(a.last().unwrap_or(NULL));
            Ok(pat.euclidish(pulses, steps, perc).into())
        });
    }

    // `hsl(h, s, l, pat)` / `hsla(h, s, l, a, pat)`: CSS colour helpers writing
    // the `color` control (pattern last, mirroring Strudel's `register`).
    prelude.func("hsl", |a| {
        let at = |i: usize| arg_to_pattern(a.get(i).unwrap_or(NULL));
        let pat = arg_to_pattern(a.last().unwrap_or(NULL));
        Ok(pat.hsl(at(0), at(1), at(2)).into())
    });
    prelude.func("hsla", |a| {
        let at = |i: usize| arg_to_pattern(a.get(i).unwrap_or(NULL));
        let pat = arg_to_pattern(a.last().unwrap_or(NULL));
        Ok(pat.hsla(at(0), at(1), at(2), at(3)).into())
    });
    prelude.func("bjork", |a| {
        let counts = bjork_counts(a.first().unwrap_or(NULL));
        let pat = arg_to_pattern(a.last().unwrap_or(NULL));
        Ok(pat.bjork(&counts).into())
    });

    // The euclid family, pattern-last like every other standalone transform.
    // Its counts may be patterns (`euclid("<3 5>", 8, s("bd"))`), so it shares
    // `euclid_call` with the methods rather than living in an integer group.
    type EuclidBuild = fn(&Pattern, i64, i64, i64) -> Pattern;
    let euclid_plain: EuclidBuild = |pat, a, b, _| pat.euclid(a, b);
    let euclid_legato: EuclidBuild = |pat, a, b, _| pat.euclid_legato(a, b);
    for (names, rotated, build) in [
        (&["euclid"][..], false, euclid_plain),
        (&["euclidLegato", "euclid_legato"][..], false, euclid_legato),
        (
            &["euclidRot", "euclid_rot", "euclidrot"][..],
            true,
            Pattern::euclid_rot as EuclidBuild,
        ),
        (
            &["euclidLegatoRot", "euclid_legato_rot"][..],
            true,
            Pattern::euclid_legato_rot as EuclidBuild,
        ),
    ] {
        for name in names {
            prelude.curried(name, if rotated { 4 } else { 3 }, move |a| {
                // Curried, so there is always a count per slot before the
                // pattern; a plain euclid's third slot is the pattern, which
                // `euclid_call` never reads as a count.
                let pat = arg_to_pattern(a.last().unwrap_or(NULL));
                let counts = [a.first(), a.get(1), a.get(2)];
                Ok(euclid_call(&pat, counts, rotated, build).into())
            });
        }
    }

    // The stepwise counts, pattern-last like every other standalone transform.
    // A patterned count is laid out stepwise (https://strudel.cc/learn/stepwise/),
    // so these share `stepwise_call` with the methods instead of living in the
    // plain-integer group.
    type StepwiseBuild = fn(&Pattern, i64) -> Pattern;
    for (name, build) in [
        ("expand", (|p, n| p.expand(n)) as StepwiseBuild),
        ("extend", |p, n| p.extend(n)),
        ("contract", |p, n| p.contract(n)),
        ("shrink", |p, n| p.shrink(n)),
        ("grow", |p, n| p.grow(n)),
        ("take", |p, n| p.take(n)),
        ("drop", |p, n| p.drop(n)),
        ("replicate", |p, n| p.replicate(n)),
    ] {
        prelude.curried(name, 2, move |a| {
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(NULL));
            let count = a.first().filter(|_| last >= 1);
            Ok(stepwise_call(&pat, count, build).into())
        });
    }

    register_pattern_fns!(prelude;
        pattern1: [
            "fast" => fast, "slow" => slow, "ply" => ply,
            "sparsity" => slow, // Strudel alias (`density` is a control, not fast)
            "segment" => segment, "seg" => seg,
            "add" => add, "sub" => sub, "mul" => mul, "div" => div, "modulo" => modulo,
            "set" => set, "keep" => keep, "keepif" => keepif, "mask" => mask, "bypass" => bypass,
            "struct" => struct_pat, "scale" => scale,
            "timeline" => timeline,
            // waveshaping-distortion shortcuts (pattern-last standalone form)
            "soft" => soft, "hard" => hard, "cubic" => cubic, "diode" => diode,
            "asym" => asym, "fold" => fold, "sinefold" => sinefold, "chebyshev" => chebyshev,
            "early" => early, "late" => late,
            "lt" => lt, "gt" => gt, "lte" => lte, "gte" => gte,
            "eq" => eq, "eqt" => eqt, "ne" => ne, "net" => net,
            "band" => band, "bor" => bor, "bxor" => bxor,
            "blshift" => blshift, "brshift" => brshift,
            "fastGap" => fast_gap, "fast_gap" => fast_gap, "fastgap" => fast_gap,
            "transpose" => transpose, "trans" => trans,
            "scaleTranspose" => scale_transpose, "scale_transpose" => scale_transpose,
            "scaleTrans" => strans, "strans" => strans,
        ];
        noarg: [
            "palindrome" => palindrome, "degrade" => degrade, "undegrade" => undegrade,
            "press" => press, "brak" => brak, "ratio" => ratio, "fit" => fit,
            "invert" => invert, "inv" => invert, "collect" => collect,
            "rev" => rev,
            "voicing" => voicing,
            // The rounding transforms, which a script reaches for standalone
            // (`n(floor(rand.range(1, 6)))`) as often as it chains them.
            "floor" => floor, "ceil" => ceil, "round" => round,
        ];
        i64_1: [
            "iter" => iter, "iterBack" => iter_back, "iter_back" => iter_back, "iterback" => iter_back,
            "repeatCycles" => repeat_cycles, "repeat_cycles" => repeat_cycles,
            "chop" => chop, "striate" => striate,
            "rootNotes" => root_notes, "root_notes" => root_notes,
            "shuffle" => shuffle, "scramble" => scramble,
        ];
        f64_1: [
            "degradeBy" => degrade_by, "degrade_by" => degrade_by,
            "undegradeBy" => undegrade_by, "undegrade_by" => undegrade_by,
            "cpm" => cpm,
        ];
        frac1: [
            "hurry" => hurry, "swing" => swing,
            "pressBy" => press_by, "press_by" => press_by,
            "loopAt" => loop_at, "loop_at" => loop_at, "loopat" => loop_at,
            "pace" => pace, "seed" => seed, "linger" => linger,
        ];
        f64_2: ["range" => range, "range2" => range2, "rangex" => rangex];
        frac2: [
            "focus" => focus, "compress" => compress, "zoom" => zoom,
            "ribbon" => ribbon, "rib" => rib,
            "swingBy" => swing_by, "swing_by" => swing_by,
        ];
        i64_frac_f64: ["echo" => echo];
        i64_f64_frac: ["stut" => stut];
        pat2: [
            "slice" => slice, "splice" => splice, "bite" => bite,
            "beat" => beat, "xfade" => xfade,
        ];
    );

    // `degradeByWith(withPat, x, pat)` is the only (pattern, f64) transform, so
    // it is registered directly rather than growing the macro a one-member group.
    for name in ["degradeByWith", "degrade_by_with"] {
        prelude.curried(name, 3, |a| {
            let last = a.len().saturating_sub(1);
            let pat = arg_to_pattern(a.get(last).unwrap_or(NULL));
            let with_pat = arg_to_pattern(a.first().filter(|_| last >= 1).unwrap_or(NULL));
            let x = arg_to_f64(a.get(1).filter(|_| last >= 2).unwrap_or(NULL));
            Ok(pat.degrade_by_with(with_pat, x).into())
        });
    }

    // Every control also gets a standalone factory, matching Strudel: each
    // `registerControl` call exports a top-level function as well as a method,
    // so `speed("1 2").s("bd")` reads the same as `s("bd").speed("1 2")`.
    // Registered last and only for names the prelude has not already claimed,
    // so hand-written bindings (`note`, `n`, `s`, `i`, `freq`, …) win.
    register_control_factories(prelude);
    // What upstream writes in JavaScript over the methods above, kept in it.
    crate::js::run_lent(include_str!("prelude.js"));
}

/// Register a pattern-valued factory for every control name that is not already
/// a top-level function. The counterpart of `register_methods`, which does the
/// same for the method form.
fn register_control_factories(prelude: &Scope) {
    for (name, builder) in rudel_core::control_builders() {
        if !prelude.has(name) {
            prelude.func(name, move |a| Ok(builder(arg_to_pattern(arg0(a))).into()));
        }
    }
    // The numbered FM/operator controls have no Rust builder fn; their canonical
    // keys are generated at runtime.
    for (name, key) in rudel_core::numbered_control_names() {
        if !prelude.has(&name) {
            prelude.func(&name, move |a| {
                Ok(rudel_core::control_dyn(key.clone(), arg_to_pattern(arg0(a))).into())
            });
        }
    }
}

/// `keyDown(names)` as a boolean pattern: true while **every** named key is
/// held. The argument is patternified like Strudel's `register`, so a
/// `:`-list (`"Control:j"`) is a combination and the live keyboard state is
/// read at query time rather than when the pattern is built.
pub(super) fn key_down_pattern(arg: &Arg) -> Pattern {
    arg_to_pattern(arg).fmap(|v| {
        let names: Vec<&str> = match &v {
            rudel_core::Value::List(items) => items.iter().filter_map(|x| x.as_str()).collect(),
            other => other.as_str().into_iter().collect(),
        };
        rudel_core::Value::Bool(rudel_core::keys_down(names))
    })
}
