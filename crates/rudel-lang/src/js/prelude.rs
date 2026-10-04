//! Native implementation of the Strudel and Hydra script-facing prelude.
// SPDX-License-Identifier: AGPL-3.0-or-later
use super::native::*;
use super::{Context, JsResult, JsValue, native, side};
use boa_engine::js_string;
use boa_engine::object::builtins::JsPromise;

pub(super) fn register(ctx: &mut Context) -> JsResult<()> {
    let g: JsValue = ctx.global_object().into();
    let p: JsValue = side(ctx).pattern.clone().into();
    for (bind, join) in [
        ("bind", "join"),
        ("innerBind", "innerJoin"),
        ("outerBind", "outerJoin"),
        ("squeezeBind", "squeezeJoin"),
        ("stepBind", "stepJoin"),
        ("polyBind", "polyJoin"),
    ] {
        let f = native::function(
            move |this, a, _, ctx| {
                let mapped = native::method(this, "fmap", &[arg(a, 0)], ctx)?;
                native::method(&mapped, join, &[], ctx)
            },
            vec![],
            ctx,
        );
        def(&p, bind, f, ctx)?;
        let f = native::function(
            move |_, a, _, ctx| {
                let pat = global("reify", &[arg(a, 1)], ctx)?;
                native::method(&pat, bind, &[arg(a, 0)], ctx)
            },
            vec![],
            ctx,
        );
        let f = curry(2, f, vec![], ctx);
        def(&g, bind, f, ctx)?;
    }
    def(&g, "window", g.clone(), ctx)?;
    register_hydra(ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            let f = native::function(
                |_, _, c, ctx| {
                    let value = native::call(&c[0], &JsValue::undefined(), &[], ctx)?;
                    global("reify", &[value], ctx)
                },
                vec![arg(a, 0)],
                ctx,
            );
            let pat = global("pure", &[1.into()], ctx)?;
            let pat = native::method(&pat, "withValue", &[f], ctx)?;
            native::method(&pat, "innerJoin", &[], ctx)
        },
        vec![],
        ctx,
    );
    def(&g, "ref", f, ctx)?;
    let f = native::function(
        |this, a, _, ctx| {
            let callback = native::function(
                |_, a, c, ctx| {
                    let g: JsValue = ctx.global_object().into();
                    let f = get(&g, "getTime", ctx)?;
                    let time = if f.is_null_or_undefined() {
                        0.into()
                    } else {
                        nullish(native::call(&f, &g, &[], ctx)?, 0.into())
                    };
                    native::call(&c[0], &JsValue::undefined(), &[arg(a, 0), time], ctx)
                },
                vec![arg(a, 0)],
                ctx,
            );
            native::method(this, "onTriggerTime", &[callback], ctx)
        },
        vec![],
        ctx,
    );
    def(&p, "onTrigger", f, ctx)?;
    for key in ["theme", "fontFamily", "fontSize"] {
        let f = native::function(
            move |this, a, _, ctx| {
                let mapper = native::function(
                    move |_, a, _, ctx| {
                        let value = arg(a, 0);
                        let value = if is_array(&value, ctx)? {
                            native::method(&value, "join", &[string(" ")], ctx)?
                        } else {
                            global("String", &[value], ctx)?
                        };
                        let result = object([], ctx)?;
                        set(&result, &format!("__{key}"), value, ctx)?;
                        Ok(result)
                    },
                    vec![],
                    ctx,
                );
                let values = global("reify", &[arg(a, 0)], ctx)?;
                let values = native::method(&values, "fmap", &[mapper], ctx)?;
                let pat = native::method(this, "set", &[values], ctx)?;
                let callback = native::function(
                    move |_, a, _, ctx| {
                        let value = get(&arg(a, 0), "value", ctx)?;
                        let value = get(&value, &format!("__{key}"), ctx)?;
                        global("__setting", &[string(key), value], ctx)
                    },
                    vec![],
                    ctx,
                );
                native::method(&pat, "onTrigger", &[callback, false.into()], ctx)
            },
            vec![],
            ctx,
        );
        def(&p, key, f.clone(), ctx)?;
        let standalone = native::function(
            |_, a, c, ctx| {
                let pat = global("reify", &[arg(a, 1)], ctx)?;
                native::call(&c[0], &pat, &[arg(a, 0)], ctx)
            },
            vec![f],
            ctx,
        );
        let standalone = curry(2, standalone, vec![], ctx);
        def(&g, key, standalone, ctx)?;
    }
    let f = native::function(|_, a, _, ctx| create_param(arg(a, 0), ctx), vec![], ctx);
    def(&g, "createParam", f, ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            let result = object([], ctx)?;
            for name in a {
                let f = create_param(name.clone(), ctx)?;
                set(&result, &text(name, ctx)?, f, ctx)?;
            }
            Ok(result)
        },
        vec![],
        ctx,
    );
    def(&g, "createParams", f, ctx)?;
    for name in ["shrinklist", "s_taperlist"] {
        let f = native::function(
            move |_, a, _, ctx| {
                let pat = global("reify", &[arg(a, 1)], ctx)?;
                native::method(&pat, name, &[arg(a, 0)], ctx)
            },
            vec![],
            ctx,
        );
        def(&g, name, f, ctx)?;
    }
    let f = native::function(
        |_, parts, _, ctx| {
            let mut total = global("Fraction", &[0.into()], ctx)?;
            for part in parts {
                if items(part, ctx)?.len() == 2 {
                    native::method(part, "unshift", &[total], ctx)?;
                }
                total = part.to_object(ctx)?.get(1, ctx)?;
            }
            let mut sections = Vec::new();
            for part in parts {
                let values = items(part, ctx)?;
                let pat = global("reify", &[arg(&values, 2)], ctx)?;
                let pat = global("pure", &[pat], ctx)?;
                let start = global("Fraction", &[arg(&values, 0)], ctx)?;
                let start = native::method(&start, "div", &[total.clone()], ctx)?;
                let stop = global("Fraction", &[arg(&values, 1)], ctx)?;
                let stop = native::method(&stop, "div", &[total.clone()], ctx)?;
                sections.push(native::method(&pat, "compress", &[start, stop], ctx)?);
            }
            let pat = global("stack", &sections, ctx)?;
            let pat = native::method(&pat, "slow", &[total], ctx)?;
            native::method(&pat, "innerJoin", &[], ctx)
        },
        vec![],
        ctx,
    );
    def(&g, "seqPLoop", f, ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            let v = arg(a, 0);
            if !v.is_string() {
                return Ok(array([], ctx));
            }
            let regexp = global("RegExp", &[string("^([a-gA-G])([#bsf]*)(-?[0-9]*)$")], ctx)?;
            let matched = native::method(&regexp, "exec", &[v], ctx)?;
            if matched.is_null() {
                return Ok(array([], ctx));
            }
            let matched = items(&matched, ctx)?;
            let octave = arg(&matched, 3);
            let octave = if octave.to_boolean() {
                octave.to_number(ctx)?.into()
            } else {
                JsValue::undefined()
            };
            Ok(array([arg(&matched, 1), arg(&matched, 2), octave], ctx))
        },
        vec![],
        ctx,
    );
    def(&g, "tokenizeNote", f, ctx)
}

fn create_param(names: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let multi = is_array(&names, ctx)?;
    let names = if multi { names } else { array([names], ctx) };
    let name = names.to_object(ctx)?.get(0, ctx)?;
    let mapper = native::function(
        |_, a, c, ctx| {
            let mut xs = arg(a, 0);
            let mut bag = None;
            if (xs.is_object() && !xs.is_callable()) || xs.is_null() {
                let value = get(&xs, "value", ctx)?;
                if !value.is_undefined() {
                    let copied = copy(&xs, &[], ctx)?;
                    xs = get(&xs, "value", ctx)?;
                    copied
                        .to_object(ctx)?
                        .delete_property_or_throw(js_string!("value"), ctx)?;
                    bag = Some(copied);
                }
            }
            let result = match bag {
                Some(v) => v,
                None => object([], ctx)?,
            };
            if c[1].to_boolean() && is_array(&xs, ctx)? {
                let each = native::function(
                    |_, a, c, ctx| {
                        let i = arg(a, 1);
                        if i.to_number(ctx)? < get(&c[0], "length", ctx)?.to_number(ctx)? {
                            let name = c[0].to_object(ctx)?.get(i.to_property_key(ctx)?, ctx)?;
                            set(&c[1], &text(&name, ctx)?, arg(a, 0), ctx)?;
                        }
                        Ok(JsValue::undefined())
                    },
                    vec![c[0].clone(), result.clone()],
                    ctx,
                );
                native::method(&xs, "forEach", &[each], ctx)?;
            } else {
                set(&result, &text(&c[2], ctx)?, xs, ctx)?;
            }
            Ok(result)
        },
        vec![names, multi.into(), name.clone()],
        ctx,
    );
    let factory = native::function(
        |_, a, c, ctx| {
            let value = arg(a, 0);
            let pat = arg(a, 1);
            if !pat.to_boolean() {
                let pat = global("reify", &[value], ctx)?;
                native::method(&pat, "withValue", &[c[0].clone()], ctx)
            } else if value.is_undefined() {
                native::method(&pat, "fmap", &[c[0].clone()], ctx)
            } else {
                let value = global("reify", &[value], ctx)?;
                let value = native::method(&value, "withValue", &[c[0].clone()], ctx)?;
                native::method(&pat, "set", &[value], ctx)
            }
        },
        vec![mapper],
        ctx,
    );
    let method = native::function(
        |this, a, c, ctx| {
            native::call(
                &c[0],
                &JsValue::undefined(),
                &[arg(a, 0), this.clone()],
                ctx,
            )
        },
        vec![factory.clone()],
        ctx,
    );
    let p = side(ctx).pattern.clone().into();
    def(&p, &text(&name, ctx)?, method, ctx)?;
    Ok(factory)
}

fn hydra_arg(v: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    if is_array(&v, ctx)? {
        let array = get(&ctx.global_object().into(), "Array", ctx)?;
        let seq = native::method(&array, "from", std::slice::from_ref(&v), ctx)?;
        let result = object([("hydraSeq", seq)], ctx)?;
        for (key, field) in [
            ("speed", "_speed"),
            ("smooth", "_smooth"),
            ("ease", "_ease"),
            ("offset", "_offset"),
        ] {
            let value = get(&v, field, ctx)?;
            set(&result, key, value, ctx)?;
        }
        Ok(result)
    } else if v.is_callable() {
        let f = native::function(
            |_, a, c, ctx| {
                let g: JsValue = ctx.global_object().into();
                if get(&g, "time", ctx)?.is_number() {
                    let time = get(&arg(a, 0), "time", ctx)?;
                    set(&g, "time", time, ctx)?;
                }
                let analyser = get(&g, "a", ctx)?;
                if !analyser.is_null_or_undefined() {
                    let tick = get(&analyser, "tick", ctx)?;
                    if !tick.is_null_or_undefined() {
                        native::call(&tick, &analyser, &[], ctx)?;
                    }
                }
                native::call(&c[0], &JsValue::undefined(), &[arg(a, 0)], ctx)
            },
            vec![v],
            ctx,
        );
        object([("hydraFn", f)], ctx)
    } else {
        Ok(v)
    }
}

fn wrap_args(o: &JsValue, ctx: &mut Context) -> JsResult<()> {
    for name in keys(o, ctx)? {
        let f = get(o, &name, ctx)?;
        if !f.is_callable() || name == "constructor" {
            continue;
        }
        let wrapped = native::function(
            |this, a, c, ctx| {
                let args = a
                    .iter()
                    .cloned()
                    .map(|v| hydra_arg(v, ctx))
                    .collect::<JsResult<Vec<_>>>()?;
                native::call(&c[0], this, &args, ctx)
            },
            vec![f],
            ctx,
        );
        set(o, &name, wrapped, ctx)?;
    }
    Ok(())
}

const EASINGS: &[&str] = &[
    "linear",
    "easeInQuad",
    "easeOutQuad",
    "easeInOutQuad",
    "easeInCubic",
    "easeOutCubic",
    "easeInOutCubic",
    "easeInQuart",
    "easeOutQuart",
    "easeInOutQuart",
    "easeInQuint",
    "easeOutQuint",
    "easeInOutQuint",
    "sin",
];

fn register_hydra(ctx: &mut Context) -> JsResult<()> {
    let g: JsValue = ctx.global_object().into();
    let h = get(&g, "Hydra", ctx)?;
    let src = get(&h, "src", ctx)?;
    let f = native::function(
        |_, a, c, ctx| {
            let mut args = a.to_vec();
            let s = arg(a, 0);
            let index = if s.is_object()
                && !s.is_callable()
                && s.to_object(ctx)?.has_property(js_string!("index"), ctx)?
            {
                get(&s, "index", ctx)?
            } else {
                s
            };
            if args.is_empty() {
                args.push(index);
            } else {
                args[0] = index;
            }
            native::call(&c[0], &JsValue::undefined(), &args, ctx)
        },
        vec![src],
        ctx,
    );
    set(&h, "src", f, ctx)?;
    for i in 0..4 {
        let s = object([("index", (4 + i).into())], ctx)?;
        for name in ["init", "initStream", "clear"] {
            let f = native::function(|this, _, _, _| Ok(this.clone()), vec![], ctx);
            set(&s, name, f, ctx)?;
        }
        for kind in ["Image", "Video", "Cam", "Screen"] {
            let f = native::function(
                move |this, a, _, ctx| {
                    let h = get(&ctx.global_object().into(), "Hydra", ctx)?;
                    let mut args = vec![i.into()];
                    match kind {
                        "Cam" => {
                            let n = global("Number", &[default(a, 0, 0.into())], ctx)?
                                .to_number(ctx)?
                                .floor();
                            let n = if n == 0.0 || n.is_nan() { 0.0 } else { n };
                            args.push(string("camera:").add(&n.into(), ctx)?);
                        }
                        "Screen" => {
                            args.push(string(""));
                            args.push(string("screen"));
                        }
                        _ => {
                            args.push(default(a, 0, string("")));
                            if kind == "Video" {
                                args.push(string("video"));
                            }
                        }
                    }
                    native::method(&h, "_image", &args, ctx)?;
                    Ok(this.clone())
                },
                vec![],
                ctx,
            );
            set(&s, &format!("init{kind}"), f, ctx)?;
        }
        set(&h, &format!("s{i}"), s, ctx)?;
    }
    set(&h, "width", 1920.into(), ctx)?;
    set(&h, "height", 1080.into(), ctx)?;
    let mouse = object([("x", 0.into()), ("y", 0.into())], ctx)?;
    set(&h, "mouse", mouse, ctx)?;
    wrap_args(&h, ctx)?;
    let chain = native::method(&h, "osc", &[], ctx)?;
    let proto = chain.as_object().unwrap().prototype().unwrap().into();
    wrap_args(&proto, ctx)?;
    let f = native::function(
        |_, a, _, ctx| global("reify", &[arg(a, 0)], ctx),
        vec![],
        ctx,
    );
    def(&g, "H", f, ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            let result: JsResult<JsValue> = (|| {
                let g: JsValue = ctx.global_object().into();
                let h = get(&g, "Hydra", ctx)?;
                for name in keys(&h, ctx)? {
                    let v = get(&h, &name, ctx)?;
                    set(&g, &name, v, ctx)?;
                }
                set(&g, "time", 0.into(), ctx)?;
                let array_proto = ctx.intrinsics().constructors().array().prototype().into();
                for name in ["fast", "smooth", "ease", "offset", "fit"] {
                    let f = native::function(
                        move |this, a, _, ctx| array_util(name, this, a, ctx),
                        vec![],
                        ctx,
                    );
                    def(&array_proto, name, f, ctx)?;
                }
                let options = default(a, 0, object([], ctx)?);
                if get(&options, "detectAudio", ctx)?.to_boolean() {
                    let analyser = audio_analyser(ctx)?;
                    set(&g, "a", analyser, ctx)?;
                }
                let feed = get(&options, "feedStrudel", ctx)?.to_boolean();
                native::method(&h, "_feed", &[feed.into()], ctx)?;
                Ok(JsValue::undefined())
            })();
            Ok(match result {
                Ok(v) => JsPromise::resolve(v, ctx),
                Err(e) => JsPromise::reject(e, ctx),
            }?
            .into())
        },
        vec![],
        ctx,
    );
    def(&g, "initHydra", f, ctx)
}

fn array_util(name: &str, this: &JsValue, a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    match name {
        "fast" => set(this, "_speed", default(a, 0, 1.into()), ctx)?,
        "smooth" => set(this, "_smooth", default(a, 0, 1.into()), ctx)?,
        "ease" => {
            let ease = default(a, 0, string("linear"));
            if ease.is_callable()
                || EASINGS.iter().any(|s| {
                    ease.as_string()
                        .is_some_and(|v| v.to_std_string_lossy() == *s)
                })
            {
                set(this, "_smooth", 1.into(), ctx)?;
                set(
                    this,
                    "_ease",
                    if ease.is_callable() {
                        string("linear")
                    } else {
                        ease
                    },
                    ctx,
                )?;
            }
        }
        "offset" => {
            let n = default(a, 0, 0.5.into()).to_number(ctx)? % 1.0;
            set(this, "_offset", n.into(), ctx)?;
        }
        "fit" => {
            let low = default(a, 0, 0.into());
            let high = default(a, 1, 1.into());
            let numbers = items(this, ctx)?
                .iter()
                .map(|v| v.to_number(ctx))
                .collect::<JsResult<Vec<_>>>()?;
            let lowest = numbers.iter().copied().fold(f64::INFINITY, js_min);
            let highest = numbers.iter().copied().fold(f64::NEG_INFINITY, js_max);
            let mapper = native::function(
                |_, a, c, ctx| {
                    let n = arg(a, 0).to_number(ctx)?;
                    let lowest = c[2].as_number().unwrap();
                    let highest = c[3].as_number().unwrap();
                    let scaled = (n - lowest) * (c[1].to_number(ctx)? - c[0].to_number(ctx)?)
                        / (highest - lowest);
                    JsValue::from(scaled).add(&c[0], ctx)
                },
                vec![low, high, lowest.into(), highest.into()],
                ctx,
            );
            let result = native::method(this, "map", &[mapper], ctx)?;
            for field in ["_speed", "_smooth", "_ease"] {
                let v = get(this, field, ctx)?;
                set(&result, field, v, ctx)?;
            }
            return Ok(result);
        }
        _ => unreachable!(),
    }
    Ok(this.clone())
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn audio_analyser(ctx: &mut Context) -> JsResult<JsValue> {
    let beat = object(
        [
            ("holdFrames", 20.into()),
            ("threshold", 40.into()),
            ("_cutoff", 0.into()),
            ("decay", 0.98.into()),
            ("_framesSinceBeat", 0.into()),
        ],
        ctx,
    )?;
    let a = object(
        [
            ("vol", 0.into()),
            ("scale", 10.into()),
            ("max", 15.into()),
            ("cutoff", 2.into()),
            ("smooth", 0.4.into()),
            ("frame", (-1).into()),
            ("beat", beat),
        ],
        ctx,
    )?;
    for name in ["onBeat", "show", "hide"] {
        let f = native::function(|_, _, _, _| Ok(JsValue::undefined()), vec![], ctx);
        set(&a, name, f, ctx)?;
    }
    for (name, key) in [
        ("setMax", "max"),
        ("setCutoff", "cutoff"),
        ("setSmooth", "smooth"),
        ("setScale", "scale"),
    ] {
        let f = native::function(
            move |this, args, _, ctx| {
                let value = arg(args, 0);
                set(this, key, value.clone(), ctx)?;
                if key != "max" {
                    let mapper = native::function(
                        move |_, a, c, ctx| {
                            let setting = copy(&arg(a, 0), &[], ctx)?;
                            set(&setting, key, c[0].clone(), ctx)?;
                            Ok(setting)
                        },
                        vec![value],
                        ctx,
                    );
                    let settings = get(this, "settings", ctx)?;
                    let settings = native::method(&settings, "map", &[mapper], ctx)?;
                    set(this, "settings", settings, ctx)?;
                }
                Ok(JsValue::undefined())
            },
            vec![],
            ctx,
        );
        set(&a, name, f, ctx)?;
    }
    let f = native::function(
        |this, args, c, ctx| {
            // Use Array's constructor for its native RangeError and coercion rules.
            let ctor = get(&ctx.global_object().into(), "Array", ctx)?;
            for key in ["bins", "prevBins", "fft"] {
                let xs = native::call(&ctor, &JsValue::undefined(), &[arg(args, 0)], ctx)?;
                native::method(&xs, "fill", &[0.into()], ctx)?;
                set(this, key, xs, ctx)?;
            }
            let bins = items(&get(this, "bins", ctx)?, ctx)?;
            let mut settings = Vec::new();
            for (index, _) in bins.iter().enumerate() {
                let cutoff = get(this, "cutoff", ctx)?;
                let scale = get(this, "scale", ctx)?;
                let smooth = get(this, "smooth", ctx)?;
                settings.push(object(
                    [("cutoff", cutoff), ("scale", scale), ("smooth", smooth)],
                    ctx,
                )?);
                let f = native::function(
                    move |_, args, c, ctx| {
                        let scale = default(args, 0, 1.into());
                        let offset = default(args, 1, 0.into());
                        Ok(native::function(
                            move |_, _, c, ctx| {
                                let fft = get(&c[0], "fft", ctx)?;
                                let value = fft.to_object(ctx)?.get(index, ctx)?.to_number(ctx)?;
                                // Numeric multiplication followed by JS addition (offset may be a string).
                                JsValue::from(value * c[1].to_number(ctx)?).add(&c[2], ctx)
                            },
                            vec![c[0].clone(), scale, offset],
                            ctx,
                        ))
                    },
                    vec![c[0].clone()],
                    ctx,
                );
                set(&ctx.global_object().into(), &format!("a{index}"), f, ctx)?;
            }
            let settings = array(settings, ctx);
            set(this, "settings", settings, ctx)?;
            Ok(JsValue::undefined())
        },
        vec![a.clone()],
        ctx,
    );
    set(&a, "setBins", f, ctx)?;
    let f = native::function(
        |this, args, _, ctx| {
            let level = arg(args, 0).to_number(ctx)?;
            let beat = get(this, "beat", ctx)?;
            let cutoff = get(&beat, "_cutoff", ctx)?.to_number(ctx)?;
            let threshold = get(&beat, "threshold", ctx)?.to_number(ctx)?;
            if level > cutoff && level > threshold {
                native::method(this, "onBeat", &[], ctx)?;
                set(&beat, "_cutoff", (level * 1.2).into(), ctx)?;
                set(&beat, "_framesSinceBeat", 0.into(), ctx)?;
            } else {
                let frames = get(&beat, "_framesSinceBeat", ctx)?.to_number(ctx)?;
                let hold = get(&beat, "holdFrames", ctx)?.to_number(ctx)?;
                if frames <= hold {
                    set(&beat, "_framesSinceBeat", (frames + 1.0).into(), ctx)?;
                } else {
                    let decay = get(&beat, "decay", ctx)?.to_number(ctx)?;
                    set(
                        &beat,
                        "_cutoff",
                        js_max(cutoff * decay, threshold).into(),
                        ctx,
                    )?;
                }
            }
            Ok(JsValue::undefined())
        },
        vec![],
        ctx,
    );
    set(&a, "detectBeat", f, ctx)?;
    let f = native::function(
        |this, _, _, ctx| {
            let h = get(&ctx.global_object().into(), "Hydra", ctx)?;
            let loudness = native::method(&h, "_loudness", &[], ctx)?;
            let frame = get(&loudness, "frame", ctx)?;
            let specific = get(&loudness, "specific", ctx)?;
            if frame.strict_equals(&get(this, "frame", ctx)?)
                || get(&specific, "length", ctx)?.strict_equals(&0.into())
            {
                return Ok(JsValue::undefined());
            }
            set(this, "frame", frame, ctx)?;
            let sum = native::function(|_, a, _, ctx| arg(a, 0).add(&arg(a, 1), ctx), vec![], ctx);
            let vol = native::method(&specific, "reduce", &[sum.clone(), 0.into()], ctx)?;
            set(this, "vol", vol.clone(), ctx)?;
            native::method(this, "detectBeat", &[vol], ctx)?;
            let bins = get(this, "bins", ctx)?;
            let spacing = (get(&specific, "length", ctx)?.to_number(ctx)?
                / get(&bins, "length", ctx)?.to_number(ctx)?)
            .floor();
            let previous = native::method(&bins, "slice", &[0.into()], ctx)?;
            set(this, "prevBins", previous, ctx)?;
            let group = native::function(
                |_, a, c, ctx| {
                    let i = arg(a, 1).to_number(ctx)?;
                    let spacing = c[1].as_number().unwrap();
                    let slice = native::method(
                        &c[0],
                        "slice",
                        &[(i * spacing).into(), ((i + 1.0) * spacing).into()],
                        ctx,
                    )?;
                    native::method(&slice, "reduce", &[c[2].clone(), 0.into()], ctx)
                },
                vec![specific, spacing.into(), sum],
                ctx,
            );
            let bins = native::method(&bins, "map", &[group], ctx)?;
            let smooth = native::function(
                |_, a, c, ctx| {
                    let settings = get(&c[0], "settings", ctx)?;
                    let setting = settings
                        .to_object(ctx)?
                        .get(arg(a, 1).to_property_key(ctx)?, ctx)?;
                    let smooth = get(&setting, "smooth", ctx)?.to_number(ctx)?;
                    let previous = get(&c[0], "prevBins", ctx)?;
                    let previous = previous
                        .to_object(ctx)?
                        .get(arg(a, 1).to_property_key(ctx)?, ctx)?
                        .to_number(ctx)?;
                    Ok((arg(a, 0).to_number(ctx)? * (1.0 - smooth) + previous * smooth).into())
                },
                vec![this.clone()],
                ctx,
            );
            let bins = native::method(&bins, "map", &[smooth], ctx)?;
            set(this, "bins", bins, ctx)?;
            let fft = native::function(
                |_, a, c, ctx| {
                    let settings = get(&c[0], "settings", ctx)?;
                    let setting = settings
                        .to_object(ctx)?
                        .get(arg(a, 1).to_property_key(ctx)?, ctx)?;
                    let cutoff = get(&setting, "cutoff", ctx)?.to_number(ctx)?;
                    let scale = get(&setting, "scale", ctx)?.to_number(ctx)?;
                    Ok(js_max(0.0, (arg(a, 0).to_number(ctx)? - cutoff) / scale).into())
                },
                vec![this.clone()],
                ctx,
            );
            let bins = get(this, "bins", ctx)?;
            let fft = native::method(&bins, "map", &[fft], ctx)?;
            set(this, "fft", fft, ctx)?;
            Ok(JsValue::undefined())
        },
        vec![],
        ctx,
    );
    set(&a, "tick", f, ctx)?;
    native::method(&a, "setBins", &[4.into()], ctx)?;
    Ok(a)
}
