//! The handful of JavaScript builtins Strudel scripts reach for on ordinary
//! values — not on patterns.
//!
//! Helpers registered with `register(...)` are written in plain JS, and they
//! index and slice hap values with `Array.isArray`, `arr.map`, `s.endsWith` and
//! friends. Koto's own core library has most of the behaviour under different
//! names (`ends_with`, `each`), so these are thin aliases inserted into the
//! *core* `list`/`string` modules — the same maps the VM resolves a method
//! against — plus an `Array` object in the prelude.
//!
//! Deliberately small: only what songs actually call, and each one either maps
//! straight onto a Koto core function or is three lines of Rust. Anything
//! needing real JS semantics (`typeof`, prototypes) is out of scope.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use koto::{prelude::*, runtime::unexpected_args_after_instance};

/// Extend the core `list` and `string` modules reachable from `prelude`, and add
/// the `Array` global. The prelude holds the very same `KMap`s the runtime uses
/// for method lookup, so inserting here adds real methods.
pub(crate) fn register_js_builtins(prelude: &KMap) {
    if let Some(KValue::Map(list)) = prelude.get("list") {
        register_list(&list);
    }
    if let Some(KValue::Map(string)) = prelude.get("string") {
        register_string(&string);
    }
    let array = KMap::new();
    array.add_fn("isArray", |ctx| {
        Ok(matches!(ctx.args().first(), Some(KValue::List(_) | KValue::Tuple(_))).into())
    });
    prelude.insert("Array", array);
    // JS's absent value. Helpers test hap values with `x.value !== undefined`.
    prelude.insert("undefined", KValue::Null);
    // Backs the preprocessor's rewrite of JS property access. Reading a field
    // off something that has none is `undefined` in JS, where Koto errors, and
    // that difference is the whole point of the helper — `v.value` is how a
    // script asks "is this a control map or a bare value?".
    prelude.add_fn("rudel_prop", |ctx| {
        let args = ctx.args();
        let (Some(KValue::Map(map)), Some(key)) = (args.first(), args.get(1)) else {
            return Ok(KValue::Null);
        };
        let KValue::Str(key) = key else {
            return Ok(KValue::Null);
        };
        Ok(map.get(key.as_str()).unwrap_or(KValue::Null))
    });
}

fn register_list(list: &KMap) {
    // `arr.map((value, index) => ...)`: a new list, with the index passed as
    // JS does. Koto's `each` yields values only and returns an iterator.
    list.add_fn("map", |ctx| {
        let expected = "|List, |Any, Number| -> Any|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), [f]) if f.is_callable() => {
                let source = l.data().clone();
                let f = f.clone();
                let mut out = Vec::with_capacity(source.len());
                for (i, value) in source.iter().enumerate() {
                    let args = [value.clone(), KValue::Number(KNumber::from(i as i64))];
                    // Koto is strict about arity, so a callback that ignores the
                    // index has to be retried with one argument. If that fails
                    // too the two-argument error is the one worth reporting —
                    // the retry's "insufficient arguments" only describes the
                    // retry.
                    let mapped = match ctx.vm.call_function(f.clone(), CallArgs::Separate(&args)) {
                        Ok(value) => value,
                        Err(err) => ctx
                            .vm
                            .call_function(f.clone(), value.clone())
                            .map_err(|_| err)?,
                    };
                    out.push(mapped);
                }
                Ok(KValue::List(KList::with_data(out.into())))
            }
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
    // `arr.length` is a property in JS; the preprocessor turns it into a call.
    list.add_fn("length", |ctx| {
        let expected = "|List|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), []) => Ok(KValue::Number(KNumber::from(l.data().len() as i64))),
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
}

/// The string a method was called on, plus its arguments.
fn instance_str(
    ctx: &mut CallContext,
    expected: &'static str,
) -> koto::runtime::Result<(KString, Vec<KValue>)> {
    match ctx.instance_and_args(|v| matches!(v, KValue::Str(_)), expected)? {
        (KValue::Str(s), args) => Ok((s.clone(), args.to_vec())),
        (instance, args) => unexpected_args_after_instance(expected, instance, args),
    }
}

fn register_string(string: &KMap) {
    string.add_fn("endsWith", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, String|")?;
        let Some(KValue::Str(suffix)) = args.first() else {
            return runtime_error!("endsWith expects a string");
        };
        Ok(s.as_str().ends_with(suffix.as_str()).into())
    });
    string.add_fn("startsWith", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, String|")?;
        let Some(KValue::Str(prefix)) = args.first() else {
            return runtime_error!("startsWith expects a string");
        };
        Ok(s.as_str().starts_with(prefix.as_str()).into())
    });
    // `s.substring(from, to)` clamps and swaps like JS rather than panicking,
    // and counts characters, not bytes.
    string.add_fn("substring", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, Number, Number?|")?;
        let chars: Vec<char> = s.as_str().chars().collect();
        let index = |slot: usize, fallback: usize| match args.get(slot) {
            Some(KValue::Number(n)) => (f64::from(*n).round().max(0.0) as usize).min(chars.len()),
            _ => fallback,
        };
        let (a, b) = (index(0, 0), index(1, chars.len()));
        let (from, to) = if a <= b { (a, b) } else { (b, a) };
        Ok(chars[from..to].iter().collect::<String>().into())
    });
    string.add_fn("indexOf", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, String|")?;
        let Some(KValue::Str(needle)) = args.first() else {
            return runtime_error!("indexOf expects a string");
        };
        // JS reports a character index, and -1 for "not found".
        let found = s
            .as_str()
            .find(needle.as_str())
            .map(|byte| s.as_str()[..byte].chars().count() as i64)
            .unwrap_or(-1);
        Ok(KValue::Number(KNumber::from(found)))
    });
    string.add_fn("length", move |ctx| {
        let (s, _) = instance_str(ctx, "|String|")?;
        Ok(KValue::Number(KNumber::from(
            s.as_str().chars().count() as i64
        )))
    });
}
