//! The JavaScript engine (boa) and the bridge between its values and rudel's.
//!
//! Three things live here; Boa is confined to this module and its submodules:
//!
//! - **One JS thread.** A boa `Context` and everything it allocates is tied to
//!   the thread that made it, while a pattern is queried from the scheduler and
//!   the UI at once. So every script runs on a single dedicated thread, and
//!   whatever needs the engine — an evaluation, or a script function being
//!   called from a query — is handed to it ([`on_js_thread`]).
//! - **The value bridge.** A native function never sees a boa value: its
//!   arguments arrive as [`Arg`]s, converted on the way in, and its result is
//!   converted on the way out. That keeps the bindings free of the engine's
//!   context, which they would otherwise have to thread through every helper.
//! - **Registration.** [`Scope`] puts native functions and values on an object
//!   — the global one, a namespace, or a prototype.
//!
//! SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{hydra::Chain, kabelsalat::NodeId};
pub(crate) use boa_engine::Context;
use boa_engine::{
    JsData, JsError, JsObject, JsResult, JsString, JsValue, NativeFunction, NativeObject, Source,
    builtins::object::OrdinaryObject,
    js_string,
    object::{
        FunctionObjectBuilder,
        builtins::{JsArray, JsProxy},
    },
    property::{PropertyDescriptor, PropertyKey},
};
use boa_gc::{Finalize, Trace};
use rudel_core::{Frac, Pattern, State, TimeSpan};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

mod canvas;
mod native;
mod prelude;

pub(crate) fn register_prelude() {
    lent(|ctx| prelude::register(ctx).expect("register the native prelude"));
}

pub(crate) fn register_canvas() {
    lent(|ctx| canvas::register(ctx).expect("register the native canvas"));
}

/// What a native function returns: a value for the script, or the message of
/// the error to throw.
pub(crate) type Res = Result<Arg, String>;

/// A native function: `this` (when it is one of rudel's own objects) and the
/// arguments. `Send + Sync` is what makes handing the closure to the engine
/// sound — see [`function`].
pub(crate) type Native = Arc<dyn Fn(&Arg, &[Arg]) -> Res + Send + Sync>;

/// A script value, as the bindings see it.
#[derive(Clone)]
pub(crate) enum Arg {
    /// `undefined` or `null`; leaves as `undefined`.
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Arg>),
    /// A plain object, keys in the script's own order.
    Map(Vec<(String, Arg)>),
    Pat(Pattern),
    Frac(Frac),
    /// A time span: an object with `begin` and `end`, and the span methods on
    /// its prototype.
    Span(TimeSpan),
    /// A query state: `span`, and `withSpan`/`setSpan` on its prototype.
    State(State),
    Hydra(Chain),
    Kabel(NodeId),
    /// A function, the script's own or a native one.
    Func(JsObject),
    /// Outbound only: a Rust closure handed to the script as a function.
    Native(Native),
    /// Outbound only: a native function that partially applies until it has
    /// this many arguments.
    Curried(usize, Native),
}

/// An absent argument.
pub(crate) const NULL: &Arg = &Arg::Null;

impl Arg {
    pub(crate) fn native(f: impl Fn(&[Arg]) -> Res + Send + Sync + 'static) -> Arg {
        Arg::Native(Arc::new(move |_, args| f(args)))
    }

    pub(crate) fn curried(arity: usize, f: impl Fn(&[Arg]) -> Res + Send + Sync + 'static) -> Arg {
        Arg::Curried(arity, Arc::new(move |_, args| f(args)))
    }

    pub(crate) fn is_callable(&self) -> bool {
        matches!(self, Arg::Func(_))
    }

    /// What kind of value this is, for an error message.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Arg::Null => "undefined",
            Arg::Bool(_) => "a boolean",
            Arg::Num(_) => "a number",
            Arg::Str(_) => "a string",
            Arg::List(_) => "an array",
            Arg::Map(_) => "an object",
            Arg::Pat(_) => "a pattern",
            Arg::Frac(_) => "a Fraction",
            Arg::Span(_) => "a TimeSpan",
            Arg::State(_) => "a query state",
            Arg::Hydra(_) => "a hydra chain",
            Arg::Kabel(_) => "a kabelsalat node",
            Arg::Func(_) | Arg::Native(_) | Arg::Curried(..) => "a function",
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Arg::Null)
    }

    /// A field of a plain object.
    pub(crate) fn get(&self, key: &str) -> Option<&Arg> {
        match self {
            Arg::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

impl From<Pattern> for Arg {
    fn from(pat: Pattern) -> Arg {
        Arg::Pat(pat)
    }
}

impl From<f64> for Arg {
    fn from(n: f64) -> Arg {
        Arg::Num(n)
    }
}

impl From<bool> for Arg {
    fn from(b: bool) -> Arg {
        Arg::Bool(b)
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Str(s)
    }
}

impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Str(s.to_string())
    }
}

impl From<Frac> for Arg {
    fn from(f: Frac) -> Arg {
        Arg::Frac(f)
    }
}

// ---------------------------------------------------------------------------
// The JS thread

type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    static ON_JS_THREAD: Cell<bool> = const { Cell::new(false) };
    /// The context a frame further up this thread's stack has lent out. Null
    /// when there is none, or while [`with_ctx`] has it.
    static CTX: Cell<*mut Context> = const { Cell::new(std::ptr::null_mut()) };
    /// Contexts kept past their evaluation because a pattern still calls into
    /// them, by session id.
    static PARKED: RefCell<HashMap<u64, Context>> = RefCell::new(HashMap::new());
}

fn jobs() -> &'static mpsc::Sender<Job> {
    static JOBS: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rudel-js".into())
            // boa's parser takes tens of kilobytes of stack per level of
            // nesting (hundreds in a debug build), and an overflow aborts the
            // process. This is address space reserved, not memory committed:
            // a page is only backed once the stack actually reaches it.
            .stack_size(256 << 20)
            .spawn(move || {
                ON_JS_THREAD.set(true);
                for job in rx {
                    job();
                }
            })
            .expect("spawn the JS thread");
        tx
    })
}

/// Run `f` on the JS thread and wait for what it returns. A panic in `f` is
/// re-raised here, so it fails the caller rather than killing the thread every
/// later evaluation needs.
pub(crate) fn on_js_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    if ON_JS_THREAD.get() {
        return f();
    }
    let (tx, rx) = mpsc::sync_channel(1);
    jobs()
        .send(Box::new(move || {
            let _ = tx.send(catch_unwind(AssertUnwindSafe(f)));
        }))
        .expect("the JS thread is running");
    match rx.recv().expect("the JS thread answers") {
        Ok(out) => out,
        Err(panic) => resume_unwind(panic),
    }
}

/// Queue `f` for the JS thread without waiting for it — from the JS thread
/// too, where it runs after the current job.
pub(crate) fn post(f: impl FnOnce() + Send + 'static) {
    let _ = jobs().send(Box::new(move || {
        let _ = catch_unwind(AssertUnwindSafe(f));
    }));
}

/// Puts the previous lent context back, also when unwinding.
struct Lent(*mut Context);

impl Drop for Lent {
    fn drop(&mut self) {
        CTX.set(self.0);
    }
}

/// Make `ctx` reachable through [`with_ctx`] for as long as `f` runs.
pub(crate) fn lend<R>(ctx: &mut Context, f: impl FnOnce() -> R) -> R {
    let _restore = Lent(CTX.replace(ctx));
    f()
}

/// Use the context lent to this thread. `None` when there is none: off the JS
/// thread, or from inside another `with_ctx`.
pub(crate) fn with_ctx<R>(f: impl FnOnce(&mut Context) -> R) -> Option<R> {
    let ptr = CTX.replace(std::ptr::null_mut());
    let _restore = Lent(ptr);
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the pointer came from the `&mut Context` a `lend` further up this
    // stack is holding and not touching until it returns. Taking it out of the
    // cell for the length of `f` means no second reference can be made from
    // it; a native function entered from `f` lends the context it is given.
    Some(f(unsafe { &mut *ptr }))
}

// ---------------------------------------------------------------------------
// Per-context state

/// rudel's own object kinds. Each holds plain Rust data and nothing the
/// collector needs to follow. A `Frac` is 16-byte aligned, past what an
/// object's data slot allows, hence the boxes.
#[derive(Trace, Finalize, JsData)]
struct JsPattern(#[unsafe_ignore_trace] Box<Pattern>);

#[derive(Trace, Finalize, JsData)]
struct JsFrac(#[unsafe_ignore_trace] Box<Frac>);

#[derive(Trace, Finalize, JsData)]
struct JsSpan(#[unsafe_ignore_trace] Box<TimeSpan>);

#[derive(Trace, Finalize, JsData)]
struct JsState(#[unsafe_ignore_trace] Box<State>);

#[derive(Trace, Finalize, JsData)]
struct JsHydra(#[unsafe_ignore_trace] Chain);

#[derive(Trace, Finalize, JsData)]
struct JsKabel(#[unsafe_ignore_trace] NodeId);

/// What a context carries besides the script's own state.
struct Side {
    id: u64,
    pattern: JsObject,
    frac: JsObject,
    span: JsObject,
    state: JsObject,
    hydra: JsObject,
    kabel: JsObject,
    /// `curry(arity, f)`, from [`HELPERS`].
    curry: JsObject,
    /// `method(call, fn, patternify)`, from [`HELPERS`].
    method: JsObject,
    /// `bindAt(fn, args, i)`, from [`HELPERS`].
    bind_at: JsObject,
    /// Script functions a pattern may call after the evaluation is over.
    kept: RefCell<Vec<JsObject>>,
    /// The draw canvas's path, transform and drawing (`crate::canvas`).
    canvas: RefCell<crate::canvas::Recorder>,
}

/// The closures that have to hold a script value. A native closure cannot
/// — the collector does not see inside one — so these are written in the
/// language whose closures it does see into.
const HELPERS: &str = r"({
  curry(arity, f) {
    const c = (...a) => (a.length >= arity ? f(...a) : (...b) => c(...a, ...b));
    return c;
  },
  method(call, fn, patternify) {
    return function (...args) { return call(fn, patternify, args, this); };
  },
  bindAt(fn, args, i) {
    return (v) => { const a = args.slice(); a[i] = v; return fn(...a); };
  },
})";

/// How deep a plain object or array is read before the rest is dropped, so a
/// structure that contains itself cannot recurse forever.
const DEPTH: usize = 12;

fn side(ctx: &Context) -> &Side {
    ctx.get_data::<Side>()
        .expect("a context made by new_context")
}

/// A fresh engine with rudel's prototypes in place and nothing registered.
pub(crate) fn new_context() -> Context {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut ctx = Context::default();
    // A `while (true) {}` would otherwise hold the one JS thread forever.
    ctx.runtime_limits_mut()
        .set_loop_iteration_limit(100_000_000);
    let helpers = ctx
        .eval(Source::from_bytes(HELPERS))
        .ok()
        .and_then(|v| v.as_object())
        .expect("the helpers evaluate");
    let helper = |name: &str, ctx: &mut Context| {
        helpers
            .get(JsString::from(name), ctx)
            .ok()
            .and_then(|v| v.as_object())
            .expect("a helper")
    };
    let (curry, method) = (helper("curry", &mut ctx), helper("method", &mut ctx));
    let bind_at = helper("bindAt", &mut ctx);
    let proto = |ctx: &Context| JsObject::with_object_proto(ctx.intrinsics());
    let data = Side {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        pattern: proto(&ctx),
        frac: proto(&ctx),
        span: proto(&ctx),
        state: proto(&ctx),
        hydra: proto(&ctx),
        kabel: proto(&ctx),
        curry,
        method,
        bind_at,
        kept: RefCell::new(Vec::new()),
        canvas: RefCell::default(),
    };
    ctx.insert_data(data);
    ctx
}

/// Evaluate `source`, giving the value of its last expression statement.
pub(crate) fn run(ctx: &mut Context, source: &str) -> Res {
    match ctx.eval(Source::from_bytes(source)) {
        Ok(value) => Ok(from_js(&value, ctx, DEPTH)),
        Err(e) => Err(error_text(&e, ctx)),
    }
}

fn error_text(e: &JsError, ctx: &mut Context) -> String {
    // A thrown string is the message itself; `display` would quote it.
    if let Some(text) = e.as_opaque().and_then(JsValue::as_string) {
        return text.to_std_string_lossy();
    }
    match e.try_native(ctx) {
        // `Error: …` is what a native function's own message arrives as, and
        // the prefix says nothing the message does not.
        Ok(native) if native.is_error() => native.message().to_string(),
        Ok(native) => native.to_string(),
        Err(_) => e.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The value bridge

/// One of rudel's own objects, read out of the JS object that carries it.
fn native_of(object: &JsObject) -> Option<Arg> {
    if let Some(p) = object.downcast_ref::<JsPattern>() {
        return Some(Arg::Pat((*p.0).clone()));
    }
    if let Some(f) = object.downcast_ref::<JsFrac>() {
        return Some(Arg::Frac(*f.0));
    }
    if let Some(s) = object.downcast_ref::<JsSpan>() {
        return Some(Arg::Span(*s.0));
    }
    if let Some(s) = object.downcast_ref::<JsState>() {
        return Some(Arg::State((*s.0).clone()));
    }
    if let Some(h) = object.downcast_ref::<JsHydra>() {
        return Some(Arg::Hydra(h.0.clone()));
    }
    object.downcast_ref::<JsKabel>().map(|k| Arg::Kabel(k.0))
}

fn from_js(value: &JsValue, ctx: &mut Context, depth: usize) -> Arg {
    if let Some(n) = value.as_number() {
        return Arg::Num(n);
    }
    if let Some(b) = value.as_boolean() {
        return Arg::Bool(b);
    }
    if let Some(s) = value.as_string() {
        return Arg::Str(s.to_std_string_lossy());
    }
    let Some(object) = value.as_object() else {
        return Arg::Null;
    };
    if let Some(own) = native_of(&object) {
        return own;
    }
    if object.is_callable() {
        return Arg::Func(object);
    }
    if depth == 0 {
        return Arg::Null;
    }
    if object.is_array() {
        let len = object
            .get(js_string!("length"), ctx)
            .ok()
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as u32;
        return Arg::List(
            (0..len)
                .map(|i| {
                    object
                        .get(i, ctx)
                        .map_or(Arg::Null, |v| from_js(&v, ctx, depth - 1))
                })
                .collect(),
        );
    }
    let mut entries = Vec::new();
    for key in object.own_property_keys(ctx).unwrap_or_default() {
        let name = match &key {
            PropertyKey::String(s) => s.to_std_string_lossy(),
            PropertyKey::Index(i) => i.get().to_string(),
            PropertyKey::Symbol(_) => continue,
        };
        let value = object
            .get(key, ctx)
            .map_or(Arg::Null, |v| from_js(&v, ctx, depth - 1));
        entries.push((name, value));
    }
    Arg::Map(entries)
}

/// A value's object, on a shared shape as an object literal's is. A shape of
/// its own per object was the largest single cost of handing haps to a script.
fn object<T: NativeObject>(ctx: &Context, proto: JsObject, data: T) -> JsObject {
    JsObject::new(ctx.root_shape(), proto, data).upcast()
}

fn to_js(arg: Arg, ctx: &mut Context) -> JsValue {
    match arg {
        Arg::Null => JsValue::undefined(),
        Arg::Bool(b) => b.into(),
        Arg::Num(n) => n.into(),
        Arg::Str(s) => JsString::from(s).into(),
        Arg::List(items) => {
            let items: Vec<JsValue> = items.into_iter().map(|a| to_js(a, ctx)).collect();
            JsArray::from_iter(items, ctx).into()
        }
        Arg::Map(entries) => {
            let proto = ctx.intrinsics().constructors().object().prototype();
            let object = object(ctx, proto, OrdinaryObject);
            for (key, value) in entries {
                let value = to_js(value, ctx);
                let _ = object.create_data_property_or_throw(JsString::from(key), value, ctx);
            }
            object.into()
        }
        Arg::Pat(p) => object(ctx, side(ctx).pattern.clone(), JsPattern(Box::new(p))).into(),
        Arg::Frac(f) => object(ctx, side(ctx).frac.clone(), JsFrac(Box::new(f))).into(),
        Arg::Span(s) => {
            let object = object(ctx, side(ctx).span.clone(), JsSpan(Box::new(s)));
            // Own properties, so `span.begin` reads as it does upstream and
            // `{...span}` copies them.
            for (key, at) in [("begin", s.begin), ("end", s.end)] {
                let at = to_js(Arg::Frac(at), ctx);
                let _ = object.create_data_property_or_throw(JsString::from(key), at, ctx);
            }
            object.into()
        }
        Arg::State(state) => {
            let span = to_js(Arg::Span(state.span), ctx);
            let object = object(ctx, side(ctx).state.clone(), JsState(Box::new(state)));
            let _ = object.create_data_property_or_throw(js_string!("span"), span, ctx);
            object.into()
        }
        Arg::Hydra(h) => object(ctx, side(ctx).hydra.clone(), JsHydra(h)).into(),
        Arg::Kabel(k) => object(ctx, side(ctx).kabel.clone(), JsKabel(k)).into(),
        Arg::Func(f) => f.into(),
        Arg::Native(f) => function("", f, false, ctx).into(),
        Arg::Curried(arity, f) => {
            let f = function("", f, false, ctx);
            let curry = side(ctx).curry.clone();
            curry
                .call(&JsValue::undefined(), &[arity.into(), f.into()], ctx)
                .unwrap_or_default()
        }
    }
}

/// A JS function running `f`. A constructor is usable with `new`, where
/// whatever object `f` returns is the result — which is how `new Pattern(…)`
/// works. It never sees a `this`, so a method must not be one.
fn function(name: &str, f: Native, constructor: bool, ctx: &mut Context) -> JsObject {
    // SAFETY: a closure handed to the engine must not own anything the
    // collector traces, since it cannot look inside one. `Native` is
    // `Send + Sync` and no traced value is, so it cannot.
    let native = unsafe {
        NativeFunction::from_closure(move |this, args, ctx| call_native(&f, this, args, ctx))
    };
    FunctionObjectBuilder::new(ctx.realm(), native)
        .name(JsString::from(name))
        .constructor(constructor)
        .build()
        .into()
}

fn call_native(
    f: &Native,
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    // Only rudel's own objects are read as `this`: called bare, a function's
    // `this` can be the global object, which is no argument to convert.
    let this = this
        .as_object()
        .and_then(|o| native_of(&o))
        .unwrap_or(Arg::Null);
    let args: Vec<Arg> = args.iter().map(|a| from_js(a, ctx, DEPTH)).collect();
    match lend(ctx, || f(&this, &args)) {
        Ok(out) => Ok(to_js(out, ctx)),
        Err(message) => Err(boa_engine::JsNativeError::error()
            .with_message(message)
            .into()),
    }
}

/// Call a script function with `args`.
pub(crate) fn call(func: &Arg, args: Vec<Arg>) -> Res {
    let Arg::Func(f) = func else {
        return Err(format!("expected a function, got {}", func.kind()));
    };
    with_ctx(|ctx| {
        let args: Vec<JsValue> = args.into_iter().map(|a| to_js(a, ctx)).collect();
        match f.call(&JsValue::undefined(), &args, ctx) {
            Ok(value) => Ok(from_js(&value, ctx, DEPTH)),
            Err(e) => Err(error_text(&e, ctx)),
        }
    })
    .unwrap_or_else(|| Err("no JavaScript engine to call the function on".to_string()))
}

/// `value` as the script would print it.
pub(crate) fn display(value: &Arg) -> String {
    with_ctx(|ctx| {
        let value = to_js(value.clone(), ctx);
        match value.to_string(ctx) {
            Ok(text) => text.to_std_string_lossy(),
            Err(_) => value.display().to_string(),
        }
    })
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Registration

/// An object natives are put on: the global one, a namespace, or a prototype.
#[derive(Clone)]
pub(crate) struct Scope(JsObject);

fn lent<R>(f: impl FnOnce(&mut Context) -> R) -> R {
    with_ctx(f).expect("registration runs with a context lent")
}

impl Scope {
    pub(crate) fn global() -> Scope {
        Scope(lent(|ctx| ctx.global_object()))
    }

    /// `Pattern.prototype`.
    pub(crate) fn pattern() -> Scope {
        Scope(lent(|ctx| side(ctx).pattern.clone()))
    }

    pub(crate) fn fraction() -> Scope {
        Scope(lent(|ctx| side(ctx).frac.clone()))
    }

    /// `TimeSpan.prototype`.
    pub(crate) fn span() -> Scope {
        Scope(lent(|ctx| side(ctx).span.clone()))
    }

    /// `State.prototype`, for the state a query function is handed.
    pub(crate) fn state() -> Scope {
        Scope(lent(|ctx| side(ctx).state.clone()))
    }

    pub(crate) fn hydra() -> Scope {
        Scope(lent(|ctx| side(ctx).hydra.clone()))
    }

    pub(crate) fn kabel() -> Scope {
        Scope(lent(|ctx| side(ctx).kabel.clone()))
    }

    fn define(&self, name: &str, value: JsValue, ctx: &mut Context) {
        // Configurable, so a script's own top-level `const s = …` shadows the
        // binding instead of being refused as a redeclaration.
        let _ = self.0.define_property_or_throw(
            JsString::from(name),
            PropertyDescriptor::builder()
                .value(value)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            ctx,
        );
    }

    pub(crate) fn value(&self, name: &str, value: impl Into<Arg>) {
        let value = value.into();
        lent(|ctx| {
            let value = match value {
                // Named, so an error thrown through it says where it was.
                Arg::Native(f) => function(name, f, false, ctx).into(),
                other => to_js(other, ctx),
            };
            self.define(name, value, ctx);
        });
    }

    /// A function that also works with `new`: `new Pattern(…)`, `new Hap(…)`.
    pub(crate) fn constructor(
        &self,
        name: &str,
        f: impl Fn(&[Arg]) -> Res + Send + Sync + 'static,
    ) {
        lent(|ctx| {
            let value = function(name, Arc::new(move |_, a| f(a)), true, ctx);
            self.define(name, value.into(), ctx);
        });
    }

    /// A function of its arguments alone.
    pub(crate) fn func(&self, name: &str, f: impl Fn(&[Arg]) -> Res + Send + Sync + 'static) {
        self.value(name, Arg::native(f));
    }

    /// A function that also reads `this`.
    pub(crate) fn method(
        &self,
        name: &str,
        f: impl Fn(&Arg, &[Arg]) -> Res + Send + Sync + 'static,
    ) {
        self.value(name, Arg::Native(Arc::new(f)));
    }

    /// A function that partially applies until it has `arity` arguments, as
    /// Strudel's `register`ed functions do: `fast(2)` is a transform.
    pub(crate) fn curried(
        &self,
        name: &str,
        arity: usize,
        f: impl Fn(&[Arg]) -> Res + Send + Sync + 'static,
    ) {
        self.value(name, Arg::curried(arity, f));
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        lent(|ctx| {
            self.0
                .has_own_property(JsString::from(name), ctx)
                .unwrap_or(false)
        })
    }

    /// Bind `alias` to whatever `name` is, if it is anything.
    pub(crate) fn alias(&self, alias: &str, name: &str) {
        lent(|ctx| {
            if let Ok(value) = self.0.get(JsString::from(name), ctx)
                && !value.is_undefined()
            {
                self.define(alias, value, ctx);
            }
        });
    }

    /// A plain object under `name`, to put further names on.
    pub(crate) fn namespace(&self, name: &str) -> Scope {
        lent(|ctx| {
            let object = JsObject::with_object_proto(ctx.intrinsics());
            self.define(name, object.clone().into(), ctx);
            Scope(object)
        })
    }

    /// Make the function `name` the constructor of `proto`, so
    /// `Name.prototype.x = …` and `instanceof Name` mean what they say.
    pub(crate) fn constructs(&self, name: &str, proto: &Scope) {
        lent(|ctx| {
            let Ok(ctor) = self.0.get(JsString::from(name), ctx) else {
                return;
            };
            let Some(object) = ctor.as_object() else {
                return;
            };
            Scope(object).define("prototype", proto.0.clone().into(), ctx);
            proto.define("constructor", ctor, ctx);
        });
    }

    /// `name` as a getter: `f` runs with `this` each time it is read, as
    /// `pat.d1` registers its pattern.
    pub(crate) fn getter(&self, name: &str, f: impl Fn(&Arg) -> Res + Send + Sync + 'static) {
        lent(|ctx| {
            let get = function(name, Arc::new(move |this, _| f(this)), false, ctx);
            self.define_getter(name, get, ctx);
        });
    }

    /// `name` as a getter handing back `this[method]` bound to `this`, with
    /// each of `cells` (`(property, method)`) bound the same way as a property
    /// of it: `pat.add(x)`, and `pat.add.out(x)` with the other pattern's
    /// structure. A getter, so `room(1).keep.out` passed on uncalled still
    /// knows its pattern. The methods are read now, so `method` may be `name`.
    pub(crate) fn bound_getter(&self, name: &str, method: &str, cells: &[(&str, &str)]) {
        lent(|ctx| {
            let mut read = |key: &str| {
                self.0
                    .get(JsString::from(key), ctx)
                    .ok()
                    .and_then(|v| v.as_object())
                    .filter(JsObject::is_callable)
            };
            let Some(plain) = read(method) else {
                return;
            };
            let cells: Vec<(String, JsObject)> = cells
                .iter()
                .filter_map(|(how, m)| Some((how.to_string(), read(m)?)))
                .collect();
            // The methods are script values, so they are captures the
            // collector traces, not part of the closure.
            let get = NativeFunction::from_copy_closure_with_captures(
                |this, _, (plain, cells): &(JsObject, Vec<(String, JsObject)>), ctx| {
                    let wrapper = bound(plain, this, ctx);
                    for (how, method) in cells {
                        let method = bound(method, this, ctx);
                        wrapper.set(JsString::from(how.as_str()), method, false, ctx)?;
                    }
                    Ok(wrapper.into())
                },
                (plain, cells),
            );
            let get = FunctionObjectBuilder::new(ctx.realm(), get)
                .name(JsString::from(name))
                .build();
            self.define_getter(name, get.into(), ctx);
        });
    }

    fn define_getter(&self, name: &str, get: JsObject, ctx: &mut Context) {
        let _ = self.0.define_property_or_throw(
            JsString::from(name),
            PropertyDescriptor::builder()
                .get(get)
                .enumerable(false)
                .configurable(true),
            ctx,
        );
    }

    /// Answer any `_name` nothing else defines with `name`, through a proxy in
    /// front of this object's prototype. Every method upstream's `register`
    /// makes has an unpatterned twin under a leading underscore, which given
    /// plain arguments does what the patterned one does.
    pub(crate) fn answer_underscore_names(&self) {
        lent(|ctx| {
            let Some(target) = self.0.prototype() else {
                return;
            };
            if let Ok(proxy) = JsProxy::builder(target).get(underscore_get).build(ctx) {
                self.0.set_prototype(Some(proxy.into()));
            }
        });
    }

    /// Every name on this object, sorted.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = lent(|ctx| self.0.own_property_keys(ctx).unwrap_or_default())
            .into_iter()
            .filter_map(|key| match key {
                PropertyKey::String(s) => Some(s.to_std_string_lossy()),
                _ => None,
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// The lent context's canvas recording.
pub(crate) fn with_canvas<R>(f: impl FnOnce(&mut crate::canvas::Recorder) -> R) -> R {
    lent(|ctx| f(&mut side(ctx).canvas.borrow_mut()))
}

/// `func` with `this` fixed, as `func.bind(this)` makes it.
fn bound(func: &JsObject, this: &JsValue, ctx: &mut Context) -> JsObject {
    let call = NativeFunction::from_copy_closure_with_captures(
        |_, args, (func, this): &(JsObject, JsValue), ctx| func.call(this, args, ctx),
        (func.clone(), this.clone()),
    );
    FunctionObjectBuilder::new(ctx.realm(), call).build().into()
}

/// The `get` trap of [`Scope::answer_underscore_names`]: `(target, key,
/// receiver)`. `_steps` is not a method upstream but the step count, which
/// scripts read; `__name` is left alone.
fn underscore_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    if let Some(key) = arg(1).as_string() {
        let key = key.to_std_string_lossy();
        if let Some(name) = key.strip_prefix('_')
            && !name.starts_with('_')
            && key != "_steps"
            && let Some(receiver) = arg(2).as_object()
        {
            let method = receiver.get(JsString::from(name), ctx)?;
            if method.is_callable() {
                return Ok(method);
            }
        }
    }
    // Anything else as if there were no proxy.
    let reflect_get = ctx
        .intrinsics()
        .objects()
        .reflect()
        .get(js_string!("get"), ctx)?;
    match reflect_get.as_callable() {
        Some(get) => get.call(&JsValue::undefined(), args, ctx),
        None => Ok(JsValue::undefined()),
    }
}

/// Wrap `func` as a method: called as `pat.name(...args)`, it runs
/// `call(func, patternify, args, pat)`.
pub(crate) fn method_calling(call: Arg, func: &Arg, patternify: bool) -> Arg {
    lent(|ctx| {
        let method = side(ctx).method.clone();
        let args = [
            to_js(call, ctx),
            to_js(func.clone(), ctx),
            patternify.into(),
        ];
        match method.call(&JsValue::undefined(), &args, ctx) {
            Ok(value) => from_js(&value, ctx, DEPTH),
            Err(_) => Arg::Null,
        }
    })
}

/// `func` with every argument but the `at`th fixed: a one-argument function
/// that calls `func(...args)` with its own argument in that place.
pub(crate) fn bind_at(func: &Arg, args: Vec<Arg>, at: usize) -> Res {
    with_ctx(|ctx| {
        let bind_at = side(ctx).bind_at.clone();
        let args: Vec<JsValue> = args.into_iter().map(|a| to_js(a, ctx)).collect();
        let args = [
            to_js(func.clone(), ctx),
            JsArray::from_iter(args, ctx).into(),
            (at as u32).into(),
        ];
        match bind_at.call(&JsValue::undefined(), &args, ctx) {
            Ok(value) => Ok(from_js(&value, ctx, DEPTH)),
            Err(e) => Err(error_text(&e, ctx)),
        }
    })
    .unwrap_or_else(|| Err("no JavaScript engine to bind the function in".to_string()))
}

// ---------------------------------------------------------------------------
// Script functions that outlive their evaluation

/// A script function a pattern can call later, from any thread. Just a name
/// for it: the function itself stays on the JS thread, in the context that
/// made it.
#[derive(Clone, Copy)]
pub(crate) struct SendFn {
    session: u64,
    index: usize,
}

/// Keep `func` callable after this evaluation ends.
pub(crate) fn keep(func: &Arg) -> Option<SendFn> {
    let Arg::Func(f) = func else {
        return None;
    };
    with_ctx(|ctx| {
        let side = side(ctx);
        let mut kept = side.kept.borrow_mut();
        kept.push(f.clone());
        SendFn {
            session: side.id,
            index: kept.len() - 1,
        }
    })
}

impl SendFn {
    /// Run `job` with the function, on the JS thread. `None` once the
    /// evaluation it belongs to has been dropped.
    pub(crate) fn run<R: Send + 'static>(
        self,
        job: impl FnOnce(&Arg) -> R + Send + 'static,
    ) -> Option<R> {
        if ON_JS_THREAD.get() {
            // Mid-evaluation: the caller is the script, asking for one of its
            // own functions back.
            let func = with_ctx(|ctx| {
                let side = side(ctx);
                (side.id == self.session)
                    .then(|| side.kept.borrow().get(self.index).cloned())
                    .flatten()
            })??;
            return Some(job(&Arg::Func(func)));
        }
        on_js_thread(move || {
            let mut ctx = PARKED.with(|p| p.borrow_mut().remove(&self.session))?;
            let func = side(&ctx).kept.borrow().get(self.index).cloned()?;
            // A panic in the engine must not reach the thread asking, which is
            // the scheduler or the UI. The call just yields nothing.
            let out = catch_unwind(AssertUnwindSafe(|| {
                lend(&mut ctx, || job(&Arg::Func(func)))
            }));
            PARKED.with(|p| p.borrow_mut().insert(self.session, ctx));
            out.ok()
        })
    }
}

/// Keeps a parked context alive. Dropping the last one releases it.
pub(crate) struct Session(u64);

impl Drop for Session {
    fn drop(&mut self) {
        let id = self.0;
        let release = move || {
            PARKED.with(|p| p.borrow_mut().remove(&id));
        };
        if ON_JS_THREAD.get() {
            release();
        } else {
            // Not waited for: a pattern is dropped wherever it happens to be,
            // which may be a thread that must not block.
            let _ = jobs().send(Box::new(release));
        }
    }
}

/// End an evaluation. A context some pattern still calls into is parked, and
/// stays for as long as the returned session does; any other is dropped here.
pub(crate) fn park(ctx: Context) -> Option<Arc<Session>> {
    let data = side(&ctx);
    if data.kept.borrow().is_empty() {
        return None;
    }
    let id = data.id;
    PARKED.with(|p| p.borrow_mut().insert(id, ctx));
    Some(Arc::new(Session(id)))
}
