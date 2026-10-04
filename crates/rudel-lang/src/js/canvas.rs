//! Native draw/animation API. Script objects stay on the engine thread.
// SPDX-License-Identifier: AGPL-3.0-or-later
use super::native::*;
use super::{
    Context, FunctionObjectBuilder, JsResult, JsString, JsValue, NativeFunction,
    PropertyDescriptor, native, side,
};
use boa_engine::object::builtins::JsPromise;
use std::cell::RefCell;
use std::collections::HashSet;

// Context data is outside the GC heap, so its JsValue handles remain roots.
// Release every RefCell borrow before invoking a script: it can re-enter us.
struct Canvas {
    used: bool,
    now: JsValue,
    size: JsValue,
    context: JsValue,
    body: JsValue,
    handlers: JsValue,
    listeners: Vec<(JsValue, Vec<JsValue>)>,
    frames: Vec<(u64, JsValue)>,
    next_frame: u64,
    painters: Vec<JsValue>,
    on_paints: Vec<JsValue>,
    drawable: JsValue,
    said: HashSet<String>,
}
fn state(ctx: &Context) -> &RefCell<Canvas> {
    ctx.get_data::<RefCell<Canvas>>()
        .expect("native canvas state")
}
fn used(ctx: &Context) {
    state(ctx).borrow_mut().used = true;
}
fn noop(ctx: &Context) -> JsValue {
    native::function(|_, _, _, _| Ok(JsValue::undefined()), vec![], ctx)
}

pub(super) fn register(ctx: &mut Context) -> JsResult<()> {
    let g: JsValue = ctx.global_object().into();
    let shown = global("__canvasSize", &[], ctx)?;
    let shown = items(&shown, ctx)?;
    let size = object([("width", arg(&shown, 0)), ("height", arg(&shown, 1))], ctx)?;
    let canvas = object([], ctx)?;
    let context = object([], ctx)?;
    let body = element(size.clone(), context.clone(), ctx)?;
    let handlers = object([], ctx)?;
    let drawable = drawable(ctx)?;
    ctx.insert_data(RefCell::new(Canvas {
        used: false,
        now: 0.into(),
        size: size.clone(),
        context: context.clone(),
        body: body.clone(),
        handlers,
        listeners: Vec::new(),
        frames: Vec::new(),
        next_frame: 1,
        painters: Vec::new(),
        on_paints: Vec::new(),
        drawable,
        said: HashSet::new(),
    }));
    for (name, field) in [
        ("width", "width"),
        ("height", "height"),
        ("clientWidth", "width"),
        ("clientHeight", "height"),
    ] {
        let getter = native::function(
            move |_, _, c, ctx| get(&c[0], field, ctx),
            vec![size.clone()],
            ctx,
        );
        let setter = if name == "width" || name == "height" {
            Some(noop(ctx))
        } else {
            None
        };
        accessor(&canvas, name, getter, setter, true, ctx)?;
    }
    let style = object([], ctx)?;
    set(&canvas, "style", style, ctx)?;
    let get_context = native::function(|_, _, c, _| Ok(c[0].clone()), vec![context.clone()], ctx);
    set(&canvas, "getContext", get_context, ctx)?;
    for name in ["addEventListener", "removeEventListener"] {
        set(&canvas, name, noop(ctx), ctx)?;
    }
    let f = native::function(
        |_, _, c, ctx| {
            let width = get(&c[0], "width", ctx)?;
            let height = get(&c[0], "height", ctx)?;
            object(
                [
                    ("left", 0.into()),
                    ("top", 0.into()),
                    ("width", width),
                    ("height", height),
                ],
                ctx,
            )
        },
        vec![size.clone()],
        ctx,
    );
    set(&canvas, "getBoundingClientRect", f, ctx)?;
    for (name, field) in [("innerWidth", "width"), ("innerHeight", "height")] {
        let f = native::function(
            move |_, _, c, ctx| get(&c[0], field, ctx),
            vec![size.clone()],
            ctx,
        );
        accessor(&g, name, f, None, false, ctx)?;
    }
    let f = native::function(|_, _, _, _| Ok(1.into()), vec![], ctx);
    accessor(&g, "devicePixelRatio", f, None, false, ctx)?;
    let head = element(size, context.clone(), ctx)?;
    let page = object(
        [
            ("body", body.clone()),
            ("documentElement", body),
            ("head", head),
            ("cookie", string("")),
        ],
        ctx,
    )?;
    let f = native::function(
        |_, a, c, ctx| {
            let tag = global("String", &[arg(a, 0)], ctx)?;
            if text(&tag, ctx)?.to_lowercase() == "canvas" {
                Ok(c[0].clone())
            } else {
                let size = state(ctx).borrow().size.clone();
                element(size, c[1].clone(), ctx)
            }
        },
        vec![canvas.clone(), context.clone()],
        ctx,
    );
    set(&page, "createElement", f, ctx)?;
    let f = native::function(
        |_, _, _, ctx| {
            let s = state(ctx).borrow();
            let (size, context) = (s.size.clone(), s.context.clone());
            drop(s);
            element(size, context, ctx)
        },
        vec![],
        ctx,
    );
    set(&page, "createElementNS", f, ctx)?;
    for name in ["getElementById", "querySelector"] {
        let f = native::function(|_, _, _, _| Ok(JsValue::null()), vec![], ctx);
        set(&page, name, f, ctx)?;
    }
    for name in ["querySelectorAll", "getElementsByTagName"] {
        let f = native::function(|_, _, _, ctx| Ok(array([], ctx)), vec![], ctx);
        set(&page, name, f, ctx)?;
    }
    let add = native::function(
        |_, a, _, ctx| {
            used(ctx);
            let key = arg(a, 0);
            let mut s = state(ctx).borrow_mut();
            if let Some((_, fns)) = s
                .listeners
                .iter_mut()
                .find(|(k, _)| JsValue::same_value_zero(k, &key))
            {
                fns.push(arg(a, 1));
            } else {
                s.listeners.push((key, vec![arg(a, 1)]));
            }
            Ok(JsValue::undefined())
        },
        vec![],
        ctx,
    );
    let remove = native::function(
        |_, a, _, ctx| {
            if let Some((_, fns)) = state(ctx)
                .borrow_mut()
                .listeners
                .iter_mut()
                .find(|(k, _)| JsValue::same_value_zero(k, &arg(a, 0)))
                && let Some(i) = fns.iter().position(|f| f.strict_equals(&arg(a, 1)))
            {
                fns.remove(i);
            }
            Ok(JsValue::undefined())
        },
        vec![],
        ctx,
    );
    set(&page, "addEventListener", add.clone(), ctx)?;
    set(&page, "removeEventListener", remove.clone(), ctx)?;
    def(&g, "addEventListener", add, ctx)?;
    def(&g, "removeEventListener", remove, ctx)?;
    for kind in [
        "mousemove",
        "mousedown",
        "mouseup",
        "click",
        "keydown",
        "keyup",
    ] {
        let getter = native::function(
            move |_, _, _, ctx| {
                let handlers = state(ctx).borrow().handlers.clone();
                Ok(nullish(get(&handlers, kind, ctx)?, JsValue::null()))
            },
            vec![],
            ctx,
        );
        let setter = native::function(
            move |_, a, _, ctx| {
                used(ctx);
                let handlers = state(ctx).borrow().handlers.clone();
                set(&handlers, kind, arg(a, 0), ctx)?;
                Ok(JsValue::undefined())
            },
            vec![],
            ctx,
        );
        accessor(
            &page,
            &format!("on{kind}"),
            getter,
            Some(setter),
            false,
            ctx,
        )?;
    }
    def(&g, "document", page, ctx)?;
    let init = get(&g, "initHydra", ctx)?;
    let f = native::function(
        |_, a, c, ctx| {
            used(ctx);
            let result = native::call(&c[0], &JsValue::undefined(), &[arg(a, 0)], ctx);
            Ok(match result {
                Ok(v) => JsPromise::resolve(v, ctx),
                Err(e) => JsPromise::reject(e, ctx),
            }?
            .into())
        },
        vec![init],
        ctx,
    );
    def(&g, "initHydra", f, ctx)?;
    register_context(&context, canvas, ctx)?;
    let f = native::function(
        |_, _, _, ctx| {
            used(ctx);
            Ok(state(ctx).borrow().context.clone())
        },
        vec![],
        ctx,
    );
    def(&g, "getDrawContext", f, ctx)?;
    let f = native::function(
        |_, _, _, ctx| Ok(state(ctx).borrow().now.clone()),
        vec![],
        ctx,
    );
    def(&g, "getTime", f, ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            used(ctx);
            let mut s = state(ctx).borrow_mut();
            let id = s.next_frame;
            s.next_frame += 1;
            s.frames.push((id, arg(a, 0)));
            Ok((id as f64).into())
        },
        vec![],
        ctx,
    );
    def(&g, "requestAnimationFrame", f, ctx)?;
    let f = native::function(
        |_, a, _, ctx| {
            let mut s = state(ctx).borrow_mut();
            let before = s.frames.len();
            s.frames.retain(|(id, _)| {
                !JsValue::same_value_zero(&JsValue::from(*id as f64), &arg(a, 0))
            });
            Ok((s.frames.len() != before).into())
        },
        vec![],
        ctx,
    );
    def(&g, "cancelAnimationFrame", f, ctx)?;
    let p: JsValue = side(ctx).pattern.clone().into();
    let f = native::function(
        |this, a, _, ctx| {
            used(ctx);
            let options = default(a, 1, object([], ctx)?);
            let lookbehind = nullish(get(&options, "lookbehind", ctx)?, 0.into())
                .to_number(ctx)?
                .abs();
            let lookahead = nullish(get(&options, "lookahead", ctx)?, 0.into());
            let painter = object(
                [
                    ("pattern", this.clone()),
                    ("fn", arg(a, 0)),
                    ("lookbehind", lookbehind.into()),
                    ("lookahead", lookahead),
                    ("memory", JsValue::null()),
                    ("last", JsValue::null()),
                ],
                ctx,
            )?;
            state(ctx).borrow_mut().painters.push(painter);
            Ok(this.clone())
        },
        vec![],
        ctx,
    );
    def(&p, "draw", f, ctx)?;
    let f = native::function(
        |this, a, _, ctx| {
            used(ctx);
            let painter = object([("pattern", this.clone()), ("painter", arg(a, 0))], ctx)?;
            state(ctx).borrow_mut().on_paints.push(painter);
            Ok(this.clone())
        },
        vec![],
        ctx,
    );
    def(&p, "onPaint", f, ctx)?;
    let f = native::function(
        |_, _, _, ctx| Ok(state(ctx).borrow().used.into()),
        vec![],
        ctx,
    );
    set(&g, "__drawUsed", f, ctx)?;
    let f = native::function(|_, a, _, ctx| draw_frame(a, ctx), vec![], ctx);
    set(&g, "__drawFrame", f, ctx)?;
    let f = native::function(|this, a, _, ctx| animate(this, a, ctx), vec![], ctx);
    def(&p, "animate", f, ctx)
}

fn element(size: JsValue, context: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let style = object([], ctx)?;
    let dataset = object([], ctx)?;
    let class_list = object([], ctx)?;
    for name in ["add", "remove", "toggle"] {
        set(&class_list, name, noop(ctx), ctx)?;
    }
    let f = native::function(|_, _, _, _| Ok(false.into()), vec![], ctx);
    set(&class_list, "contains", f, ctx)?;
    let element = object(
        [
            ("style", style),
            ("dataset", dataset),
            ("classList", class_list),
        ],
        ctx,
    )?;
    for name in ["appendChild", "removeChild"] {
        let f = native::function(|_, a, _, _| Ok(arg(a, 0)), vec![], ctx);
        set(&element, name, f, ctx)?;
    }
    for name in [
        "remove",
        "setAttribute",
        "addEventListener",
        "removeEventListener",
    ] {
        set(&element, name, noop(ctx), ctx)?;
    }
    let f = native::function(|_, _, _, _| Ok(JsValue::null()), vec![], ctx);
    set(&element, "getAttribute", f, ctx)?;
    let f = native::function(|_, _, c, _| Ok(c[0].clone()), vec![context], ctx);
    set(&element, "getContext", f, ctx)?;
    for (name, field) in [("clientWidth", "width"), ("clientHeight", "height")] {
        let f = native::function(
            move |_, _, c, ctx| get(&c[0], field, ctx),
            vec![size.clone()],
            ctx,
        );
        accessor(&element, name, f, None, true, ctx)?;
    }
    Ok(element)
}

fn defaults(context: &JsValue, ctx: &mut Context) -> JsResult<()> {
    for (key, value) in [
        ("fillStyle", string("#000000")),
        ("strokeStyle", string("#000000")),
        ("lineWidth", 1.into()),
        ("lineCap", string("butt")),
        ("lineJoin", string("miter")),
        ("globalAlpha", 1.into()),
        ("font", string("10px sans-serif")),
        ("textAlign", string("start")),
        ("textBaseline", string("alphabetic")),
        ("globalCompositeOperation", string("source-over")),
    ] {
        set(context, key, value, ctx)?;
    }
    Ok(())
}
fn font_size(context: &JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let font = get(context, "font", ctx)?;
    let regexp = get(&ctx.global_object().into(), "RegExp", ctx)?;
    let regexp = native::call(
        &regexp,
        &JsValue::undefined(),
        &[string(r"(\d*\.?\d+)px")],
        ctx,
    )?;
    let matched = native::method(&regexp, "exec", &[font], ctx)?;
    if matched.is_null() {
        Ok(10.into())
    } else {
        Ok(matched.to_object(ctx)?.get(1, ctx)?.to_number(ctx)?.into())
    }
}
fn style(v: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    if v.to_boolean() && v.is_object() && !v.is_callable() {
        Ok(nullish(get(&v, "color", ctx)?, string("#000000")))
    } else {
        global("String", &[v], ctx)
    }
}
fn register_context(context: &JsValue, canvas: JsValue, ctx: &mut Context) -> JsResult<()> {
    defaults(context, ctx)?;
    set(context, "canvas", canvas.clone(), ctx)?;
    let stack = array([], ctx);
    set(context, "stack", stack, ctx)?;
    let proto = object([], ctx)?;
    context
        .as_object()
        .unwrap()
        .set_prototype(proto.as_object());
    let constructor = NativeFunction::from_copy_closure_with_captures(
        |new_target, _, captures: &Vec<JsValue>, ctx| {
            if new_target.is_undefined() {
                return Err(boa_engine::JsNativeError::typ()
                    .with_message("cannot call class constructor without new")
                    .into());
            }
            let context = object([], ctx)?;
            let prototype = get(new_target, "prototype", ctx)?
                .as_object()
                .unwrap_or_else(|| ctx.intrinsics().constructors().object().prototype());
            context.as_object().unwrap().set_prototype(Some(prototype));
            defaults(&context, ctx)?;
            set(&context, "canvas", captures[0].clone(), ctx)?;
            let stack = array([], ctx);
            set(&context, "stack", stack, ctx)?;
            Ok(context)
        },
        vec![canvas],
    );
    let constructor: JsValue = FunctionObjectBuilder::new(ctx.realm(), constructor)
        .name(JsString::from("Context2D"))
        .constructor(true)
        .build()
        .into();
    constructor.as_object().unwrap().define_property_or_throw(
        JsString::from("prototype"),
        PropertyDescriptor::builder()
            .value(proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        ctx,
    )?;
    def(&proto, "constructor", constructor, ctx)?;
    let native = get(&ctx.global_object().into(), "__canvas", ctx)?;
    for name in [
        "save",
        "restore",
        "reset",
        "fill",
        "stroke",
        "fillRect",
        "strokeRect",
        "fontSize",
        "fillText",
        "strokeText",
        "measureText",
        "drawImage",
        "putImageData",
        "getImageData",
        "createImageData",
        "createLinearGradient",
        "createRadialGradient",
        "createConicGradient",
        "createPattern",
        "setLineDash",
        "getLineDash",
        "clip",
        "isPointInPath",
    ] {
        let f = native::function(
            move |this, a, c, ctx| context_method(name, this, a, &c[0], ctx),
            vec![native.clone()],
            ctx,
        );
        let length = match name {
            "fill" | "measureText" => 1,
            "createImageData" => 2,
            "fillText" | "strokeText" => 3,
            "fillRect" | "strokeRect" | "getImageData" => 4,
            _ => 0,
        };
        signature(&f, name, length, ctx)?;
        def(&proto, name, f, ctx)?;
    }
    for name in [
        "beginPath",
        "moveTo",
        "lineTo",
        "closePath",
        "rect",
        "roundRect",
        "ellipse",
        "arc",
        "arcTo",
        "bezierCurveTo",
        "quadraticCurveTo",
        "setTransform",
        "resetTransform",
        "getTransform",
        "transform",
        "translate",
        "scale",
        "rotate",
        "clearRect",
    ] {
        let f = get(&native, name, ctx)?;
        def(&proto, name, f, ctx)?;
    }
    Ok(())
}
fn context_method(
    name: &str,
    this: &JsValue,
    a: &[JsValue],
    native: &JsValue,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    match name {
        "drawImage" | "putImageData" | "setLineDash" | "clip" => {}
        "save" => {
            let snapshot = copy(this, &["stack", "canvas"], ctx)?;
            let stack = get(this, "stack", ctx)?;
            native::method(&stack, "push", &[snapshot], ctx)?;
            native::method(native, "save", &[], ctx)?;
        }
        "restore" => {
            let stack = get(this, "stack", ctx)?;
            let snapshot = native::method(&stack, "pop", &[], ctx)?;
            if snapshot.to_boolean() {
                assign(this, &snapshot, ctx)?;
            }
            native::method(native, "restore", &[], ctx)?;
        }
        "reset" => {
            defaults(this, ctx)?;
            let stack = array([], ctx);
            set(this, "stack", stack, ctx)?;
            native::method(native, "reset", &[], ctx)?;
            let size = state(ctx).borrow().size.clone();
            let width = get(&size, "width", ctx)?;
            let height = get(&size, "height", ctx)?;
            native::method(this, "clearRect", &[0.into(), 0.into(), width, height], ctx)?;
        }
        "fontSize" => return font_size(this, ctx),
        "fill" => {
            let rule = arg(a, 0);
            let rule = if rule.is_object() || rule.is_null() {
                arg(a, 1)
            } else {
                rule
            };
            let fill = style(get(this, "fillStyle", ctx)?, ctx)?;
            let alpha = get(this, "globalAlpha", ctx)?;
            native::method(
                native,
                "fillWith",
                &[fill, alpha, rule.strict_equals(&string("evenodd")).into()],
                ctx,
            )?;
        }
        "stroke" | "strokeRect" | "fillRect" => {
            let stroke = name.starts_with("stroke");
            let mut args = if name == "stroke" {
                vec![]
            } else {
                (0..4).map(|i| arg(a, i)).collect()
            };
            args.push(style(
                get(this, if stroke { "strokeStyle" } else { "fillStyle" }, ctx)?,
                ctx,
            )?);
            args.push(get(this, "globalAlpha", ctx)?);
            if stroke {
                for key in ["lineWidth", "lineCap", "lineJoin"] {
                    args.push(get(this, key, ctx)?);
                }
            }
            native::method(native, &format!("{name}With"), &args, ctx)?;
        }
        "fillText" | "strokeText" => {
            let text_value = global("String", &[arg(a, 0)], ctx)?;
            let size = native::method(this, "fontSize", &[], ctx)?;
            let fill = style(
                get(
                    this,
                    if name == "fillText" {
                        "fillStyle"
                    } else {
                        "strokeStyle"
                    },
                    ctx,
                )?,
                ctx,
            )?;
            let alpha = get(this, "globalAlpha", ctx)?;
            let align = get(this, "textAlign", ctx)?;
            let baseline = get(this, "textBaseline", ctx)?;
            native::method(
                native,
                "textWith",
                &[
                    text_value,
                    arg(a, 1),
                    arg(a, 2),
                    size,
                    fill,
                    alpha,
                    align,
                    baseline,
                ],
                ctx,
            )?;
        }
        "measureText" => {
            let size = native::method(this, "fontSize", &[], ctx)?.to_number(ctx)?;
            let text_value = global("String", &[arg(a, 0)], ctx)?;
            let len = get(&text_value, "length", ctx)?.to_number(ctx)?;
            return object(
                [
                    ("width", (len * size * 0.55).into()),
                    ("actualBoundingBoxAscent", (size * 0.8).into()),
                    ("actualBoundingBoxDescent", (size * 0.2).into()),
                ],
                ctx,
            );
        }
        "getImageData" => {
            let width = arg(a, 2);
            let height = arg(a, 3);
            let len = width.to_number(ctx)? * height.to_number(ctx)? * 4.0;
            let len = if len.is_nan() { len } else { len.max(0.0) };
            let ctor = get(&ctx.global_object().into(), "Uint8ClampedArray", ctx)?;
            let data = ctor
                .as_object()
                .unwrap()
                .construct(&[len.into()], None, ctx)?
                .into();
            return object([("width", width), ("height", height), ("data", data)], ctx);
        }
        "createImageData" => {
            return native::method(
                this,
                "getImageData",
                &[0.into(), 0.into(), arg(a, 0), arg(a, 1)],
                ctx,
            );
        }
        "createLinearGradient" => {
            let gradient = object([("color", JsValue::undefined())], ctx)?;
            let f = native::function(
                |this, a, _, ctx| {
                    if get(this, "color", ctx)?.is_null_or_undefined() {
                        set(this, "color", arg(a, 1), ctx)?;
                    }
                    Ok(JsValue::undefined())
                },
                vec![],
                ctx,
            );
            set(&gradient, "addColorStop", f, ctx)?;
            return Ok(gradient);
        }
        "createRadialGradient" | "createConicGradient" => {
            return native::method(this, "createLinearGradient", &[], ctx);
        }
        "createPattern" => return object([("color", string("#000000"))], ctx),
        "getLineDash" => return Ok(array([], ctx)),
        "isPointInPath" => return Ok(false.into()),
        _ => unreachable!(),
    }
    Ok(JsValue::undefined())
}
fn assign(target: &JsValue, source: &JsValue, ctx: &mut Context) -> JsResult<()> {
    let object = get(&ctx.global_object().into(), "Object", ctx)?;
    native::method(&object, "assign", &[target.clone(), source.clone()], ctx)?;
    Ok(())
}
fn span(hap: &JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let whole = get(hap, "whole", ctx)?;
    if whole.is_null_or_undefined() {
        get(hap, "part", ctx)
    } else {
        Ok(whole)
    }
}
fn drawable(ctx: &mut Context) -> JsResult<JsValue> {
    let proto = object([], ctx)?;
    for name in [
        "hasOnset",
        "isInFuture",
        "isInNearPast",
        "isActive",
        "endClipped",
    ] {
        let f = native::function(
            move |this, a, _, ctx| {
                let s = span(this, ctx)?;
                match name {
                    "hasOnset" => {
                        let whole = get(this, "whole", ctx)?;
                        if !whole.to_boolean() {
                            return Ok(false.into());
                        }
                        let begin = get(&whole, "begin", ctx)?.to_number(ctx)?;
                        let part = get(this, "part", ctx)?;
                        Ok((begin == get(&part, "begin", ctx)?.to_number(ctx)?).into())
                    }
                    "endClipped" => Ok(get(&s, "end", ctx)?.to_number(ctx)?.into()),
                    "isInFuture" => Ok((get(&s, "begin", ctx)?.to_number(ctx)?
                        > arg(a, 0).to_number(ctx)?)
                    .into()),
                    "isInNearPast" => Ok((get(&s, "end", ctx)?.to_number(ctx)?
                        >= arg(a, 1).to_number(ctx)? - arg(a, 0).to_number(ctx)?)
                    .into()),
                    "isActive" => {
                        let t = arg(a, 0).to_number(ctx)?;
                        Ok((get(&s, "begin", ctx)?.to_number(ctx)? <= t
                            && get(&s, "end", ctx)?.to_number(ctx)? >= t)
                            .into())
                    }
                    _ => unreachable!(),
                }
            },
            vec![],
            ctx,
        );
        if name == "endClipped" {
            accessor(&proto, name, f, None, true, ctx)?;
        } else {
            set(&proto, name, f, ctx)?;
        }
    }
    Ok(proto)
}
fn query(pattern: &JsValue, begin: JsValue, end: JsValue, ctx: &mut Context) -> JsResult<JsValue> {
    let haps = native::method(pattern, "queryArc", &[begin, end], ctx)?;
    let proto = state(ctx).borrow().drawable.as_object();
    for hap in items(&haps, ctx)? {
        hap.to_object(ctx)?.set_prototype(proto.clone());
    }
    Ok(haps)
}
fn attempt(
    what: &str,
    ctx: &mut Context,
    run: impl FnOnce(&mut Context) -> JsResult<()>,
) -> JsResult<()> {
    if let Err(e) = run(ctx) {
        // Engine limits are uncatchable in JS; keep that behavior in natives.
        if e.as_engine().is_some() {
            return Err(e);
        }
        let opaque = e.into_opaque(ctx)?;
        let message = if opaque.is_null_or_undefined() {
            opaque.clone()
        } else {
            nullish(get(&opaque, "message", ctx)?, opaque.clone())
        };
        let message = format!("{what}: {}", text(&message, ctx)?);
        if state(ctx).borrow_mut().said.insert(message.clone()) {
            let console = get(&ctx.global_object().into(), "console", ctx)?;
            native::method(&console, "log", &[string(&message)], ctx)?;
        }
    }
    Ok(())
}
fn dispatch(values: &JsValue, ctx: &mut Context) -> JsResult<()> {
    let a = items(values, ctx)?;
    let kind = arg(&a, 0);
    let body = state(ctx).borrow().body.clone();
    let event = object(
        [
            ("type", kind.clone()),
            ("clientX", arg(&a, 1)),
            ("clientY", arg(&a, 2)),
            ("pageX", arg(&a, 1)),
            ("pageY", arg(&a, 2)),
            ("offsetX", arg(&a, 1)),
            ("offsetY", arg(&a, 2)),
            ("button", arg(&a, 3)),
            ("key", arg(&a, 4)),
            ("code", arg(&a, 5)),
            ("ctrlKey", arg(&a, 6)),
            ("shiftKey", arg(&a, 7)),
            ("altKey", arg(&a, 8)),
            ("metaKey", arg(&a, 9)),
            ("repeat", false.into()),
            ("target", body),
        ],
        ctx,
    )?;
    for name in ["preventDefault", "stopPropagation"] {
        set(&event, name, noop(ctx), ctx)?;
    }
    if kind.strict_equals(&string("mousemove")) {
        let h = get(&ctx.global_object().into(), "Hydra", ctx)?;
        let mouse = get(&h, "mouse", ctx)?;
        set(&mouse, "x", arg(&a, 1), ctx)?;
        set(&mouse, "y", arg(&a, 2), ctx)?;
    }
    let (handlers, listeners) = {
        let s = state(ctx).borrow();
        (
            s.handlers.clone(),
            s.listeners
                .iter()
                .find(|(key, _)| JsValue::same_value_zero(key, &kind))
                .map(|(_, fns)| fns.clone())
                .unwrap_or_default(),
        )
    };
    let handler = get(&handlers, &text(&kind, ctx)?, ctx)?;
    for f in std::iter::once(handler).chain(listeners) {
        if f.is_callable() {
            attempt(&format!("document.on{}", text(&kind, ctx)?), ctx, |ctx| {
                native::call(&f, &JsValue::undefined(), std::slice::from_ref(&event), ctx)?;
                Ok(())
            })?;
        }
    }
    Ok(())
}
fn draw_frame(a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let time = arg(a, 0);
    let ms = arg(a, 1);
    let math = get(&ctx.global_object().into(), "Math", ctx)?;
    let size = state(ctx).borrow().size.clone();
    for (i, field) in [(2, "width"), (3, "height")] {
        let rounded = native::method(&math, "round", &[arg(a, i)], ctx)?;
        let value = native::method(&math, "max", &[1.into(), rounded], ctx)?;
        set(&size, field, value, ctx)?;
    }
    state(ctx).borrow_mut().now = time.clone();
    let events = default(a, 4, array([], ctx));
    for event in items(&events, ctx)? {
        dispatch(&event, ctx)?;
    }
    let due = std::mem::take(&mut state(ctx).borrow_mut().frames);
    for (_, callback) in due {
        attempt("requestAnimationFrame", ctx, |ctx| {
            native::call(
                &callback,
                &JsValue::undefined(),
                std::slice::from_ref(&ms),
                ctx,
            )?;
            Ok(())
        })?;
    }
    // As with a JS array iterator, newly registered painters run this frame.
    let mut i = 0;
    loop {
        let p = state(ctx).borrow().painters.get(i).cloned();
        let Some(p) = p else {
            break;
        };
        if i as u64 > ctx.runtime_limits().loop_iteration_limit() {
            return Err(boa_engine::error::RuntimeLimitError::LoopIteration.into());
        }
        i += 1;
        attempt("draw", ctx, |ctx| {
            let t = time.add(&get(&p, "lookahead", ctx)?, ctx)?;
            let pattern = get(&p, "pattern", ctx)?;
            let mut memory = get(&p, "memory", ctx)?;
            if memory.is_null() {
                let haps = query(&pattern, time.clone(), t.clone(), ctx)?;
                let mut onsets = Vec::new();
                for hap in items(&haps, ctx)? {
                    if native::method(&hap, "hasOnset", &[], ctx)?.to_boolean() {
                        onsets.push(hap);
                    }
                }
                memory = array(onsets, ctx);
                set(&p, "memory", memory.clone(), ctx)?;
            }
            let behind = get(&p, "lookbehind", ctx)?;
            let mut recent = Vec::new();
            for hap in items(&memory, ctx)? {
                if native::method(&hap, "isInNearPast", &[behind.clone(), time.clone()], ctx)?
                    .to_boolean()
                {
                    recent.push(hap);
                }
            }
            memory = array(recent, ctx);
            set(&p, "memory", memory.clone(), ctx)?;
            let last = nullish(get(&p, "last", ctx)?, t.clone());
            let begin =
                native::method(&math, "max", &[last, (t.to_number(ctx)? - 0.1).into()], ctx)?;
            if t.to_number(ctx)? > begin.to_number(ctx)? {
                let haps = query(&pattern, begin, t.clone(), ctx)?;
                let mut onsets = Vec::new();
                for hap in items(&haps, ctx)? {
                    if native::method(&hap, "hasOnset", &[], ctx)?.to_boolean() {
                        onsets.push(hap);
                    }
                }
                native::method(&memory, "push", &onsets, ctx)?;
            }
            set(&p, "last", t.clone(), ctx)?;
            native::method(&p, "fn", &[memory, time.clone(), t, pattern], ctx)?;
            Ok(())
        })?;
    }
    let mut i = 0;
    loop {
        let p = state(ctx).borrow().on_paints.get(i).cloned();
        let Some(p) = p else {
            break;
        };
        if i as u64 > ctx.runtime_limits().loop_iteration_limit() {
            return Err(boa_engine::error::RuntimeLimitError::LoopIteration.into());
        }
        i += 1;
        attempt("onPaint", ctx, |ctx| {
            let pattern = get(&p, "pattern", ctx)?;
            let haps = query(
                &pattern,
                (time.to_number(ctx)? - 2.0).into(),
                time.add(&2.into(), ctx)?,
                ctx,
            )?;
            let context = state(ctx).borrow().context.clone();
            let draw_time = array([(-2).into(), 2.into()], ctx);
            native::method(
                &p,
                "painter",
                &[context, time.clone(), haps, draw_time],
                ctx,
            )?;
            Ok(())
        })?;
    }
    Ok(JsValue::undefined())
}
fn animate(this: &JsValue, a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let options = default(a, 0, object([], ctx)?);
    let callback = get(&options, "callback", ctx)?;
    let smear = get(&options, "smear", ctx)?;
    let smear = if smear.is_undefined() {
        0.5.into()
    } else {
        smear
    };
    let frame = get(&ctx.global_object().into(), "frame", ctx)?;
    if frame.to_boolean() {
        global("cancelAnimationFrame", &[frame], ctx)?;
    }
    let context = global("getDrawContext", &[], ctx)?;
    let canvas = get(&context, "canvas", ctx)?;
    let width = get(&canvas, "clientWidth", ctx)?;
    let height = get(&canvas, "clientHeight", ctx)?;
    let mut part = if smear.strict_equals(&0.into()) {
        "99".to_string()
    } else {
        text(
            &native::method(
                &((1.0 - smear.to_number(ctx)?) * 100.0).into(),
                "toFixed",
                &[0.into()],
                ctx,
            )?,
            ctx,
        )?
    };
    if part.len() == 1 {
        part.insert(0, '0');
    }
    // The recursive callback's self-reference is an explicitly traced object.
    let holder = object([], ctx)?;
    let render = native::function(
        |_, a, c, ctx| render(a, c, ctx),
        vec![
            this.clone(),
            context,
            width,
            height,
            string(&format!("#200010{part}")),
            callback,
            holder.clone(),
        ],
        ctx,
    );
    set(&holder, "render", render.clone(), ctx)?;
    let frame = global("requestAnimationFrame", &[render], ctx)?;
    set(&ctx.global_object().into(), "frame", frame, ctx)?;
    get(&ctx.global_object().into(), "silence", ctx)
}
fn render(a: &[JsValue], c: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let math = get(&ctx.global_object().into(), "Math", ctx)?;
    let t = native::method(&math, "round", &[arg(a, 0)], ctx)?;
    let pattern = native::method(&c[0], "slow", &[1000.into()], ctx)?;
    let frame = native::method(&pattern, "queryArc", &[t.clone(), t], ctx)?;
    set(&c[1], "fillStyle", c[4].clone(), ctx)?;
    native::method(
        &c[1],
        "fillRect",
        &[0.into(), 0.into(), c[2].clone(), c[3].clone()],
        ctx,
    )?;
    let ww = c[2].to_number(ctx)?;
    let wh = c[3].to_number(ctx)?;
    for hap in items(&frame, ctx)? {
        let value = get(&hap, "value", ctx)?;
        let w = get(&value, "w", ctx)?.to_number(ctx)? * ww;
        let h = get(&value, "h", ctx)?.to_number(ctx)? * wh;
        let r = get(&value, "r", ctx)?;
        let angle = get(&value, "angle", ctx)?;
        let angle = if angle.is_undefined() {
            0.into()
        } else {
            angle
        };
        let (x, y) = if !r.is_undefined() && !angle.is_undefined() {
            let radians = angle.to_number(ctx)? * 2.0 * std::f64::consts::PI;
            let r = r.to_number(ctx)?;
            let (cx, cy) = ((ww - w) / 2.0, (wh - h) / 2.0);
            (cx + radians.cos() * r * cx, cy + radians.sin() * r * cy)
        } else {
            (
                get(&value, "x", ctx)?.to_number(ctx)? * (ww - w),
                get(&value, "y", ctx)?.to_number(ctx)? * (wh - h),
            )
        };
        let val = copy(&value, &[], ctx)?;
        for (key, n) in [("x", x), ("y", y), ("w", w), ("h", h)] {
            set(&val, key, n.into(), ctx)?;
        }
        let fill = get(&value, "fill", ctx)?;
        set(
            &c[1],
            "fillStyle",
            if fill.is_undefined() {
                string("darkseagreen")
            } else {
                fill
            },
            ctx,
        )?;
        let shape = get(&value, "s", ctx)?;
        if shape.strict_equals(&string("rect")) {
            native::method(
                &c[1],
                "fillRect",
                &[x.into(), y.into(), w.into(), h.into()],
                ctx,
            )?;
        } else if shape.strict_equals(&string("ellipse")) {
            native::method(&c[1], "beginPath", &[], ctx)?;
            native::method(
                &c[1],
                "ellipse",
                &[
                    (x + w / 2.0).into(),
                    (y + h / 2.0).into(),
                    (w / 2.0).into(),
                    (h / 2.0).into(),
                    0.into(),
                    0.into(),
                    (2.0 * std::f64::consts::PI).into(),
                ],
                ctx,
            )?;
            native::method(&c[1], "fill", &[], ctx)?;
        }
        if c[5].to_boolean() {
            native::call(&c[5], &JsValue::undefined(), &[c[1].clone(), val, hap], ctx)?;
        }
    }
    let render = get(&c[6], "render", ctx)?;
    let frame = global("requestAnimationFrame", &[render], ctx)?;
    set(&ctx.global_object().into(), "frame", frame, ctx)?;
    Ok(JsValue::undefined())
}
