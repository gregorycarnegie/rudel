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
    // Koto's `split` and friends hand back an iterator, and JS code goes on to
    // `.slice(...)` it as if it were an array.
    if let Some(KValue::Map(iterator)) = prelude.get("iterator") {
        iterator.add_fn("slice", |ctx| {
            let expected = "|Iterable, Number?, Number?|";
            let (instance, args) = ctx.instance_and_args(KValue::is_iterable, expected)?;
            let (instance, args) = (instance.clone(), args.to_vec());
            let items: Vec<KValue> = ctx
                .vm
                .make_iterator(instance)?
                .filter_map(|out| match out {
                    KIteratorOutput::Value(v) => Some(v),
                    _ => None,
                })
                .collect();
            let (from, to) = js_slice_bounds(&args, items.len());
            Ok(KValue::List(KList::from_slice(&items[from..to])))
        });
    }
    let array = KMap::new();
    array.add_fn("isArray", |ctx| {
        Ok(matches!(ctx.args().first(), Some(KValue::List(_) | KValue::Tuple(_))).into())
    });
    prelude.insert("Array", array);
    let object = KMap::new();
    // `Object.fromEntries([[k, v], …])`: the pairs as a map. Helpers build a
    // control map from a list of names and a list of values with it.
    object.add_fn("fromEntries", |ctx| {
        let map = KMap::new();
        let pairs = match ctx.args().first() {
            Some(KValue::List(l)) => l.data().to_vec(),
            Some(KValue::Tuple(t)) => t.data().to_vec(),
            _ => Vec::new(),
        };
        for pair in pairs {
            let entry = match &pair {
                KValue::List(l) => l.data().to_vec(),
                KValue::Tuple(t) => t.data().to_vec(),
                _ => continue,
            };
            // A non-string key is stringified, as JS object keys are.
            if let Some(key) = entry.first() {
                let key = match key {
                    KValue::Str(s) => s.to_string(),
                    KValue::Number(n) => n.to_string(),
                    _ => continue,
                };
                map.insert(key.as_str(), entry.get(1).cloned().unwrap_or(KValue::Null));
            }
        }
        Ok(KValue::Map(map))
    });
    prelude.insert("Object", object);
    // Backs the preprocessor's `typeof` rewrite. Koto's own `type` answers with
    // its names (`String`, `Map`), and a script compares against JavaScript's.
    prelude.add_fn("rudel_typeof", |ctx| {
        Ok(match ctx.args().first() {
            Some(KValue::Str(_)) => "string",
            Some(KValue::Number(_)) => "number",
            Some(KValue::Bool(_)) => "boolean",
            Some(KValue::Null) | None => "undefined",
            Some(KValue::Function(_) | KValue::NativeFunction(_)) => "function",
            // Everything else — maps, lists, patterns — is an object in JS.
            Some(_) => "object",
        }
        .into())
    });
    // `String(x)` / `Number(x)`: JavaScript's conversions, used to do arithmetic
    // on the numeric part of a note name and put it back together.
    prelude.add_fn("String", |ctx| {
        Ok(match ctx.args().first() {
            Some(KValue::Str(s)) => s.to_string(),
            Some(KValue::Number(n)) => n.to_string(),
            Some(KValue::Bool(b)) => b.to_string(),
            Some(KValue::Null) | None => "undefined".to_string(),
            Some(_) => String::new(),
        }
        .into())
    });
    prelude.add_fn("Number", |ctx| {
        Ok(match ctx.args().first() {
            Some(KValue::Number(n)) => KValue::Number(*n),
            // JS gives `NaN` for a string that will not parse; the nearest
            // useful answer here is zero, which is what an empty prefix means.
            Some(KValue::Str(s)) => KValue::Number(KNumber::from(
                s.as_str().trim().parse::<f64>().unwrap_or(0.0),
            )),
            _ => KValue::Number(KNumber::from(0.0)),
        })
    });
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
    // `arr.slice(from, to)` and `arr.concat(other, ...)`: both return a new
    // list, as JS does, and neither has a Koto equivalent that takes the same
    // shape of arguments.
    list.add_fn("slice", |ctx| {
        let expected = "|List, Number?, Number?|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), args) => {
                let data = l.data();
                let (from, to) = js_slice_bounds(args, data.len());
                Ok(KValue::List(KList::from_slice(&data[from..to])))
            }
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
    list.add_fn("concat", |ctx| {
        let expected = "|List, Any...|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), args) => {
                let mut out = l.data().to_vec();
                for arg in args {
                    match arg {
                        KValue::List(other) => out.extend(other.data().iter().cloned()),
                        KValue::Tuple(other) => out.extend(other.iter().cloned()),
                        other => out.push(other.clone()),
                    }
                }
                Ok(KValue::List(KList::from_slice(&out)))
            }
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
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
    // `arr.filter(f)`: a new list of the entries `f` accepts. Koto's `retain`
    // edits in place and `keep` returns an iterator, so neither reads as JS.
    list.add_fn("filter", |ctx| {
        let expected = "|List, |Any| -> Bool|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), [f]) if f.is_callable() => {
                let source = l.data().clone();
                let f = f.clone();
                let mut out = Vec::new();
                for value in source.iter() {
                    // JS keeps whatever is truthy; `null` and `false` are not.
                    let keep = ctx.vm.call_function(f.clone(), value.clone())?;
                    if !matches!(keep, KValue::Null | KValue::Bool(false)) {
                        out.push(value.clone());
                    }
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

/// JS `slice(from, to)` bounds against `len`: negative counts back from the
/// end, both ends clamp, and a crossed pair is empty rather than an error.
fn js_slice_bounds(args: &[KValue], len: usize) -> (usize, usize) {
    let index = |slot: usize, fallback: i64| match args.get(slot) {
        Some(KValue::Number(n)) => f64::from(*n) as i64,
        _ => fallback,
    };
    let resolve = |i: i64| {
        if i < 0 {
            (len as i64 + i).max(0) as usize
        } else {
            (i as usize).min(len)
        }
    };
    let from = resolve(index(0, 0));
    let to = resolve(index(1, len as i64));
    (from, to.max(from))
}

fn register_string(string: &KMap) {
    // Koto already ships these two under snake_case names, so the JS spelling
    // is the same function under a second key rather than a reimplementation.
    for (js, koto) in [
        ("startsWith", "starts_with"),
        ("endsWith", "ends_with"),
        ("toUpperCase", "to_uppercase"),
        ("toLowerCase", "to_lowercase"),
    ] {
        if let Some(f) = string.get(koto) {
            string.insert(js, f);
        }
    }
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
    // `s.slice(from, to)` counts characters, not bytes, and accepts negatives.
    string.add_fn("slice", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, Number?, Number?|")?;
        let chars: Vec<char> = s.as_str().chars().collect();
        let (from, to) = js_slice_bounds(&args, chars.len());
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
