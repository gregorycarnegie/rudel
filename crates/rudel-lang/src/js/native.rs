//! Engine-facing helpers. Captured script values are explicitly traced by Boa.
// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

pub(super) fn function<F>(f: F, captures: Vec<JsValue>, ctx: &Context) -> JsValue
where
    F: Fn(&JsValue, &[JsValue], &Vec<JsValue>, &mut Context) -> JsResult<JsValue> + Copy + 'static,
{
    NativeFunction::from_copy_closure_with_captures(f, captures)
        .to_js_function(ctx.realm())
        .into()
}

pub(super) fn arg(args: &[JsValue], i: usize) -> JsValue {
    args.get(i).cloned().unwrap_or_default()
}

pub(super) fn default(args: &[JsValue], i: usize, value: JsValue) -> JsValue {
    let v = arg(args, i);
    if v.is_undefined() { value } else { v }
}

pub(super) fn text(v: &JsValue, ctx: &mut Context) -> JsResult<String> {
    Ok(v.to_string(ctx)?.to_std_string_lossy())
}

pub(super) fn get(v: &JsValue, name: &str, ctx: &mut Context) -> JsResult<JsValue> {
    v.to_object(ctx)?.get(JsString::from(name), ctx)
}

pub(super) fn set(v: &JsValue, name: &str, value: JsValue, ctx: &mut Context) -> JsResult<()> {
    v.to_object(ctx)?
        .set(JsString::from(name), value, true, ctx)?;
    Ok(())
}

pub(super) fn call(
    f: &JsValue,
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let f = f
        .as_callable()
        .ok_or_else(|| boa_engine::JsNativeError::typ().with_message("expected a function"))?;
    f.call(this, args, ctx)
}

pub(super) fn method(
    v: &JsValue,
    name: &str,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let f = get(v, name, ctx)?;
    call(&f, v, args, ctx)
}

pub(super) fn global(name: &str, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let g: JsValue = ctx.global_object().into();
    method(&g, name, args, ctx)
}

pub(super) fn array(values: impl IntoIterator<Item = JsValue>, ctx: &mut Context) -> JsValue {
    JsArray::from_iter(values, ctx).into()
}

pub(super) fn items(v: &JsValue, ctx: &mut Context) -> JsResult<Vec<JsValue>> {
    let o = v.to_object(ctx)?;
    let len = o.get(js_string!("length"), ctx)?.to_length(ctx)?;
    (0..len).map(|i| o.get(i, ctx)).collect()
}

pub(super) fn object(
    entries: impl IntoIterator<Item = (&'static str, JsValue)>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let o = JsObject::with_object_proto(ctx.intrinsics());
    for (k, v) in entries {
        o.create_data_property_or_throw(JsString::from(k), v, ctx)?;
    }
    Ok(o.into())
}

pub(super) fn def(o: &JsValue, name: &str, v: JsValue, ctx: &mut Context) -> JsResult<()> {
    o.to_object(ctx)?.define_property_or_throw(
        JsString::from(name),
        PropertyDescriptor::builder()
            .value(v)
            .writable(true)
            .configurable(true)
            .enumerable(false),
        ctx,
    )?;
    Ok(())
}

pub(super) fn signature(f: &JsValue, name: &str, length: usize, ctx: &mut Context) -> JsResult<()> {
    for (key, value) in [("name", string(name)), ("length", (length as f64).into())] {
        f.to_object(ctx)?.define_property_or_throw(
            JsString::from(key),
            PropertyDescriptor::builder()
                .value(value)
                .writable(false)
                .enumerable(false)
                .configurable(true),
            ctx,
        )?;
    }
    Ok(())
}

pub(super) fn accessor(
    o: &JsValue,
    name: &str,
    getter: JsValue,
    setter: Option<JsValue>,
    enumerable: bool,
    ctx: &mut Context,
) -> JsResult<()> {
    o.to_object(ctx)?.define_property_or_throw(
        JsString::from(name),
        PropertyDescriptor::builder()
            .get(getter)
            .maybe_set(setter)
            .configurable(true)
            .enumerable(enumerable),
        ctx,
    )?;
    Ok(())
}

pub(super) fn curry(
    arity: usize,
    f: JsValue,
    supplied: Vec<JsValue>,
    ctx: &mut Context,
) -> JsValue {
    let supplied = array(supplied, ctx);
    function(
        |_, a, c, ctx| {
            let mut args = items(&c[2], ctx)?;
            args.extend_from_slice(a);
            let arity = c[0].as_number().unwrap() as usize;
            if args.len() >= arity {
                call(&c[1], &JsValue::undefined(), &args, ctx)
            } else {
                Ok(curry(arity, c[1].clone(), args, ctx))
            }
        },
        vec![(arity as f64).into(), f, supplied],
        ctx,
    )
}

pub(super) fn keys(v: &JsValue, ctx: &mut Context) -> JsResult<Vec<String>> {
    Ok(v.to_object(ctx)?
        .own_property_keys(ctx)?
        .into_iter()
        .filter_map(|k| match k {
            PropertyKey::String(s) => Some(s.to_std_string_lossy()),
            PropertyKey::Index(i) => Some(i.get().to_string()),
            PropertyKey::Symbol(_) => None,
        })
        .collect())
}

pub(super) fn copy(v: &JsValue, exclude: &[&str], ctx: &mut Context) -> JsResult<JsValue> {
    let o = JsObject::with_object_proto(ctx.intrinsics());
    o.copy_data_properties(
        v,
        exclude
            .iter()
            .map(|s| PropertyKey::from(JsString::from(*s)))
            .collect(),
        ctx,
    )?;
    Ok(o.into())
}

pub(super) fn is_array(v: &JsValue, ctx: &mut Context) -> JsResult<bool> {
    // Array.isArray also accepts proxies of arrays and rejects revoked proxies.
    let array = get(&ctx.global_object().into(), "Array", ctx)?;
    Ok(method(&array, "isArray", std::slice::from_ref(v), ctx)?.to_boolean())
}
pub(super) fn string(s: &str) -> JsValue {
    JsString::from(s).into()
}
pub(super) fn nullish(v: JsValue, fallback: JsValue) -> JsValue {
    if v.is_null_or_undefined() {
        fallback
    } else {
        v
    }
}
