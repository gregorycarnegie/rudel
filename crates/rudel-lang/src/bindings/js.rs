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
    if let Some(KValue::Map(number)) = prelude.get("number") {
        register_number(&number);
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
    // `Array.from({length: n})` builds a list of that many holes, which a
    // script maps over to repeat something `n` times. The iterable form is the
    // other half of what JS accepts.
    array.add_fn("from", |ctx| {
        let items: Vec<KValue> = match ctx.args().first() {
            Some(KValue::List(l)) => l.data().to_vec(),
            Some(KValue::Tuple(t)) => t.data().to_vec(),
            Some(KValue::Map(m)) => {
                let length = match m.get("length") {
                    Some(KValue::Number(n)) => f64::from(n).max(0.0) as usize,
                    _ => 0,
                };
                vec![KValue::Null; length]
            }
            _ => Vec::new(),
        };
        Ok(KValue::List(KList::with_data(items.into())))
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
    // JSON's numbers are all one type, so every one becomes a Koto float — the
    // same thing `Number(...)` hands back, and what the arithmetic downstream
    // of a parsed table expects.
    fn json_to_koto(value: &serde_json::Value) -> KValue {
        use serde_json::Value as Json;
        match value {
            Json::Null => KValue::Null,
            Json::Bool(b) => KValue::Bool(*b),
            Json::Number(n) => n.as_f64().map_or(KValue::Null, |f| KValue::Number(f.into())),
            Json::String(s) => KValue::Str(s.as_str().into()),
            Json::Array(items) => KValue::List(KList::with_data(
                items.iter().map(json_to_koto).collect::<Vec<_>>().into(),
            )),
            Json::Object(fields) => {
                let map = KMap::default();
                for (key, value) in fields {
                    map.insert(key.as_str(), json_to_koto(value));
                }
                KValue::Map(map)
            }
        }
    }
    // `Object.entries(map)`: the pairs as a list of `[key, value]`, the
    // inverse of `fromEntries` above.
    object.add_fn("entries", |ctx| {
        let Some(KValue::Map(map)) = ctx.args().first() else {
            return Ok(KValue::List(KList::default()));
        };
        let pairs: Vec<KValue> = map
            .data()
            .iter()
            .map(|(key, value)| {
                let key = match key.value() {
                    KValue::Str(s) => s.to_string(),
                    other => js_string(Some(other)),
                };
                KValue::List(KList::from_slice(&[key.into(), value.clone()]))
            })
            .collect();
        Ok(KValue::List(KList::with_data(pairs.into())))
    });
    prelude.insert("Object", object);
    // `JSON.parse(text)`: the object graph as Koto maps and lists. Songs embed a
    // lookup table — a pixel font, a chord book — as one JSON string literal and
    // read it back at the top of the script. `stringify` has no such use and is
    // not here.
    let json = KMap::default();
    json.add_fn("parse", |ctx| {
        let Some(KValue::Str(text)) = ctx.args().first() else {
            return runtime_error!("JSON.parse expects a string");
        };
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => Ok(json_to_koto(&value)),
            Err(e) => runtime_error!("JSON.parse: {e}"),
        }
    });
    prelude.insert("JSON", json);
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
    prelude.add_fn("String", |ctx| Ok(js_string(ctx.args().first()).into()));
    // `Boolean(x)`: the same conversion as a value. Songs pass it by name —
    // `lines.filter(Boolean)` is the idiom for dropping the empty strings a
    // `split` leaves at the ends.
    prelude.add_fn("Boolean", |ctx| Ok(js_truthy(ctx.args().first()).into()));
    // Backs the preprocessor's rewrite of `+` around a string literal. JS folds
    // an additive chain left to right, adding while both sides are numbers and
    // concatenating from the first string on.
    prelude.add_fn("rudel_concat", |ctx| {
        let mut args = ctx.args().iter();
        let Some(first) = args.next() else {
            return Ok(KValue::Null);
        };
        let mut folded = first.clone();
        for next in args {
            folded = match (&folded, next) {
                (KValue::Number(a), KValue::Number(b)) => KValue::Number(*a + *b),
                _ => format!("{}{}", js_string(Some(&folded)), js_string(Some(next))).into(),
            };
        }
        Ok(folded)
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
    // Back the preprocessor's rewrite of `<<` and `>>`, which Koto has no
    // operators for. JavaScript coerces both sides to a signed 32-bit integer,
    // masks the count to five bits, and gives a signed 32-bit result.
    for (name, left) in [("rudel_shl", true), ("rudel_shr", false)] {
        prelude.add_fn(name, move |ctx| {
            let arg = |slot: usize| match ctx.args().get(slot) {
                Some(KValue::Number(n)) => js_int32(f64::from(*n)),
                _ => 0,
            };
            let (value, count) = (arg(0), arg(1) as u32 & 31);
            let shifted = if left {
                value.wrapping_shl(count)
            } else {
                value.wrapping_shr(count)
            };
            Ok(KValue::Number(KNumber::from(shifted)))
        });
    }
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
    // `arr.join(sep)`: the entries stringified and glued, JS's default being a
    // comma. Koto's `to_string` on a list writes its brackets too.
    list.add_fn("join", |ctx| {
        let expected = "|List, String?|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), args) => {
                let separator = match args.first() {
                    Some(KValue::Str(s)) => s.to_string(),
                    _ => ",".to_string(),
                };
                let parts: Vec<String> = l.data().iter().map(|v| js_string(Some(v))).collect();
                Ok(parts.join(&separator).into())
            }
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
    /// Whether a callback declares a second parameter — the index a JS-style
    /// `map` would pass. A native function's arity is not visible, so it is
    /// given both and left to ignore what it does not want.
    fn wants_args(f: &KValue, n: u8) -> bool {
        match f {
            KValue::Function(f) => f.expected_arg_count() >= n,
            _ => true,
        }
    }

    // `arr.map((value, index) => ...)`: a new list, with the index passed as
    // JS does. Koto's `each` yields values only and returns an iterator.
    // `flatMap` is the same, with one level of the result flattened away.
    for (name, flatten) in [("map", false), ("flatMap", true)] {
        list.add_fn(name, move |ctx| {
            let expected = "|List, |Any, Number| -> Any|";
            match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
                (KValue::List(l), [f]) if f.is_callable() => {
                    let source = l.data().clone();
                    let f = f.clone();
                    let mut out = Vec::with_capacity(source.len());
                    // Koto is strict about arity, and most callbacks written in
                    // the wild ignore the index, so the call is trimmed to what
                    // this one declares rather than made and retried. A retry is
                    // not free: it would take the error path on *every* entry,
                    // and a VM that has raised a few dozen of them stops
                    // recovering — a 94-entry table failed where a 59-entry one
                    // did not.
                    let with_index = wants_args(&f, 2);
                    for (i, value) in source.iter().enumerate() {
                        let args = [value.clone(), KValue::Number(KNumber::from(i as i64))];
                        let call = if with_index {
                            CallArgs::Separate(&args)
                        } else {
                            CallArgs::Single(value.clone())
                        };
                        let mapped = ctx.vm.call_function(f.clone(), call)?;
                        match (flatten, &mapped) {
                            (true, KValue::List(inner)) => {
                                out.extend(inner.data().iter().cloned());
                            }
                            (true, KValue::Tuple(inner)) => out.extend(inner.iter().cloned()),
                            _ => out.push(mapped),
                        }
                    }
                    Ok(KValue::List(KList::with_data(out.into())))
                }
                (instance, args) => unexpected_args_after_instance(expected, instance, args),
            }
        });
    }
    // `arr.flat()`: one level of nesting undone. `flatMap(f)` is `map` then
    // `flat`, which is how a script turns each entry into several.
    list.add_fn("flat", |ctx| {
        let expected = "|List|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), _) => {
                let mut out = Vec::new();
                for value in l.data().iter() {
                    match value {
                        KValue::List(inner) => out.extend(inner.data().iter().cloned()),
                        KValue::Tuple(inner) => out.extend(inner.iter().cloned()),
                        other => out.push(other.clone()),
                    }
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
                    // JS keeps whatever the predicate answers truthily, which
                    // rules out zero and the empty string as well as `false`.
                    let keep = ctx.vm.call_function(f.clone(), value.clone())?;
                    if js_truthy(Some(&keep)) {
                        out.push(value.clone());
                    }
                }
                Ok(KValue::List(KList::with_data(out.into())))
            }
            (instance, args) => unexpected_args_after_instance(expected, instance, args),
        }
    });
    // `arr.reduce(f, seed?)`: fold left, in JS's argument order
    // `(acc, value, index)`. Songs build a `timeCat` argument list this way —
    // one entry per word, carrying a running total. Koto's `fold` takes the seed
    // first and has no index, so neither reads as JS.
    list.add_fn("reduce", |ctx| {
        let expected = "|List, |Any, Any| -> Any, Any?|";
        match ctx.instance_and_args(|v| matches!(v, KValue::List(_)), expected)? {
            (KValue::List(l), [f, seed @ ..]) if f.is_callable() => {
                let source = l.data().clone();
                let f = f.clone();
                let with_index = wants_args(&f, 3);
                let mut items = source.iter().enumerate();
                // Without a seed the first entry is it. JS throws on an empty
                // list with no seed; `null` is what the rest of this module
                // answers when there is nothing to give.
                let mut acc = match seed.first() {
                    Some(seed) => seed.clone(),
                    None => match items.next() {
                        Some((_, first)) => first.clone(),
                        None => return Ok(KValue::Null),
                    },
                };
                for (i, value) in items {
                    let args = [
                        acc,
                        value.clone(),
                        KValue::Number(KNumber::from(i as i64)),
                    ];
                    let args = if with_index { &args[..] } else { &args[..2] };
                    acc = ctx.vm.call_function(f.clone(), CallArgs::Separate(args))?;
                }
                Ok(acc)
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

/// JavaScript's `ToInt32`: truncate towards zero, then wrap into the signed
/// 32-bit range. Anything that is not a finite number is zero.
fn js_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let wrapped = x.trunc().rem_euclid(4_294_967_296.0);
    (if wrapped >= 2_147_483_648.0 {
        wrapped - 4_294_967_296.0
    } else {
        wrapped
    }) as i32
}

/// JavaScript's truthiness: everything counts but `null`, `false`, zero and the
/// empty string. A missing argument is `undefined`, which is false too.
fn js_truthy(value: Option<&KValue>) -> bool {
    match value {
        None | Some(KValue::Null) => false,
        Some(KValue::Bool(b)) => *b,
        Some(KValue::Number(n)) => f64::from(n) != 0.0,
        Some(KValue::Str(s)) => !s.is_empty(),
        _ => true,
    }
}

/// JavaScript's `String(x)`.
fn js_string(value: Option<&KValue>) -> String {
    match value {
        Some(KValue::Str(s)) => s.to_string(),
        Some(KValue::Number(n)) => n.to_string(),
        Some(KValue::Bool(b)) => b.to_string(),
        Some(KValue::Null) | None => "undefined".to_string(),
        Some(_) => String::new(),
    }
}

fn register_number(number: &KMap) {
    // `n.toString(radix)`. The radix form is how a script turns a number into a
    // bit pattern, which is the only reason this is here.
    number.add_fn("toString", |ctx| {
        let expected = "|Number, Number?|";
        let (instance, args) =
            ctx.instance_and_args(|v| matches!(v, KValue::Number(_)), expected)?;
        let KValue::Number(n) = instance else {
            return unexpected_args_after_instance(expected, instance, args);
        };
        let radix = match args.first() {
            Some(KValue::Number(r)) => f64::from(*r) as u32,
            _ => 10,
        };
        if !(2..=36).contains(&radix) || radix == 10 {
            return Ok(js_string(Some(instance)).into());
        }
        // JS truncates towards zero before converting, and keeps the sign.
        let mut left = (f64::from(*n).trunc()).abs() as u64;
        let mut digits = Vec::new();
        while {
            digits.push(char::from_digit((left % u64::from(radix)) as u32, radix).unwrap_or('0'));
            left /= u64::from(radix);
            left > 0
        } {}
        if f64::from(*n) < 0.0 {
            digits.push('-');
        }
        Ok(digits.iter().rev().collect::<String>().into())
    });
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
    // `s.split(sep)`. Koto hands back a lazy iterator where JS gives an array,
    // and the script goes straight on to `.map(...)`; an empty separator is
    // JS's "split into characters", which Koto has no answer for at all.
    string.add_fn("split", move |ctx| {
        let (s, args) = instance_str(ctx, "|String, String?|")?;
        let parts: Vec<KValue> = match args.first() {
            Some(KValue::Str(sep)) if sep.is_empty() => {
                s.as_str().chars().map(|c| c.to_string().into()).collect()
            }
            Some(KValue::Str(sep)) => s
                .as_str()
                .split(sep.as_str())
                .map(|part| part.into())
                .collect(),
            _ => vec![s.as_str().into()],
        };
        Ok(KValue::List(KList::with_data(parts.into())))
    });
    // `s.padStart(len, pad)` / `padEnd`: a short string is filled out to `len`
    // characters with `pad` repeated and clipped, a long one comes back as is.
    for (name, at_start) in [("padStart", true), ("padEnd", false)] {
        string.add_fn(name, move |ctx| {
            let (s, args) = instance_str(ctx, "|String, Number, String?|")?;
            let chars: Vec<char> = s.as_str().chars().collect();
            let width = match args.first() {
                Some(KValue::Number(n)) => f64::from(*n).max(0.0) as usize,
                _ => 0,
            };
            let pad: Vec<char> = match args.get(1) {
                Some(KValue::Str(p)) => p.as_str().chars().collect(),
                _ => vec![' '],
            };
            if chars.len() >= width || pad.is_empty() {
                return Ok(s.into());
            }
            let fill: String = pad.iter().cycle().take(width - chars.len()).collect();
            let body: String = chars.iter().collect();
            Ok(if at_start { fill + &body } else { body + &fill }.into())
        });
    }
    string.add_fn("length", move |ctx| {
        let (s, _) = instance_str(ctx, "|String|")?;
        Ok(KValue::Number(KNumber::from(
            s.as_str().chars().count() as i64
        )))
    });
}
