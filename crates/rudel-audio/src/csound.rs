//! Csound as an alternative sound engine, via the installed `libcsound`.
//!
//! Upstream (`@strudel/csound`) loads the Csound WebAssembly build into the
//! browser and routes haps to it with `inputMessage`, bypassing superdough
//! entirely. Rudel does the same against the native library.
//!
//! The library is **not** a build dependency: there is no pure-Rust Csound and
//! linking `csound-sys` would make a Csound install mandatory for everyone who
//! compiles Rudel, for a feature almost nobody uses. It is opened by name at
//! runtime the first time a script asks for it, and a script that never calls
//! `loadCsound` never touches this module. Without an install, `.csound(...)`
//! reports that and the rest of the pattern still plays.
//!
//! Csound renders *inside the audio callback*, one k-cycle at a time, with
//! host-implemented audio IO — so its output is a signal in Rudel's mixer
//! rather than a second device with its own clock, and note onsets stay
//! sample-accurate against everything else in the tune. That is the same place
//! the browser runs it (an `AudioWorklet`), for the same reason.

use libloading::{Library, Symbol};
use std::{
    ffi::{CString, c_char, c_double, c_int, c_uint, c_void},
    path::PathBuf,
};

/// Csound's `MYFLT`. Checked against `csoundGetSizeOfMYFLT` at load: the
/// single-precision build exists, and reading its `spout` as `f64` would be
/// noise rather than a wrong-sounding note.
type Myflt = c_double;

/// Opaque `CSOUND *`.
type CsoundPtr = *mut c_void;

/// `csoundInitialize` flags: leave the host's `atexit` and signal handlers
/// alone. Rudel is a GUI app that outlives any one Csound instance, and Csound
/// installing its own `SIGINT` handler would take the process down with it.
const CSOUNDINIT_NO_ATEXIT: c_int = 1;
const CSOUNDINIT_NO_SIGNAL_HANDLER: c_int = 2;

/// The orchestra header, compiled before anything a script supplies.
///
/// `sr` is the device rate, so Csound and the mixer agree frame for frame and
/// nothing has to resample. `ksmps` is the granularity a note can start on —
/// 32 frames is 0.7 ms at 44.1 kHz, below the ~1 ms that scheduling jitter
/// already costs, and small enough that `--sample-accurate` places the onset
/// inside the k-cycle. `0dbfs = 1` makes `spout` a normal −1..1 signal.
fn header(sample_rate: f32) -> String {
    format!("sr = {sample_rate}\nksmps = 32\nnchnls = 2\n0dbfs = 1\n")
}

/// `presets.orc` from `@strudel/csound`, so `.csound()` with no orchestra of
/// its own still makes a sound — upstream compiles it at init for the same
/// reason. Vendored rather than fetched: it is 3 kB and part of the engine.
const PRESETS_ORC: &str = include_str!("csound/presets.orc");

/// The entry points Rudel needs. Csound's C API has carried all of these since
/// 6.0; they are resolved individually so a missing one names itself.
struct Api {
    // `Library` must outlive every symbol taken from it. The symbols below are
    // raw fn pointers rather than `Symbol<'_>` values, so this field is what
    // keeps the library mapped — dropping it unmaps the code they point at.
    _lib: Library,
    create: unsafe extern "C" fn(*mut c_void) -> CsoundPtr,
    set_option: unsafe extern "C" fn(CsoundPtr, *const c_char) -> c_int,
    compile_orc: unsafe extern "C" fn(CsoundPtr, *const c_char) -> c_int,
    start: unsafe extern "C" fn(CsoundPtr) -> c_int,
    perform_ksmps: unsafe extern "C" fn(CsoundPtr) -> c_int,
    input_message: unsafe extern "C" fn(CsoundPtr, *const c_char),
    get_spout: unsafe extern "C" fn(CsoundPtr) -> *mut Myflt,
    get_ksmps: unsafe extern "C" fn(CsoundPtr) -> c_uint,
    get_nchnls: unsafe extern "C" fn(CsoundPtr) -> c_uint,
    host_audio_io: unsafe extern "C" fn(CsoundPtr, c_int, c_int),
    set_message_callback: unsafe extern "C" fn(CsoundPtr, MessageCallback),
    destroy: unsafe extern "C" fn(CsoundPtr),
}

/// `void (*)(CSOUND *, int attr, const char *str)`.
type MessageCallback = unsafe extern "C" fn(CsoundPtr, c_int, *const c_char);

/// Everything Csound has said since the last [`Messages::take`].
///
/// Csound reports an orchestra error by writing text and returning `-1`, so
/// without this a mistyped `instr` surfaces as "could not compile (-1)" while
/// the line and column go to a stderr no GUI user ever sees. The sink is the
/// instance's *host data*, which is how a Csound callback finds its way back
/// to the host — a global would work for one instance and quietly mix two.
#[derive(Default)]
struct Messages(std::sync::Mutex<Sink>);

/// Completed lines, plus whatever has arrived since the last newline.
#[derive(Default)]
struct Sink {
    lines: Vec<String>,
    partial: String,
}

impl Messages {
    /// Record one message. The callback is a *stream*, not a line writer: a
    /// syntax error echoes the offending source one character per call (that is
    /// how `synterr` marks the fault position), so treating each call as a line
    /// turns the whole diagnostic into a column of single letters — and the cap
    /// below then truncates it after eight of them. Text is buffered and cut on
    /// newlines instead.
    fn push(&self, text: &str) {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // Bounded: `perform_ksmps` runs in the audio callback and an orchestra
        // printing every k-cycle would grow this without limit. A line that
        // never ends is capped the same way.
        if held.lines.len() >= 200 || held.partial.len() >= 4096 {
            return;
        }
        held.partial.push_str(text);
        while let Some(nl) = held.partial.find('\n') {
            let line = held.partial.drain(..=nl).collect::<String>();
            let line = line.trim();
            if !line.is_empty() {
                held.lines.push(line.to_string());
            }
        }
    }

    fn take(&self) -> Vec<String> {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // A diagnostic need not end in a newline, and the last line is usually
        // the one that says what went wrong.
        let tail = std::mem::take(&mut held.partial);
        let tail = tail.trim();
        if !tail.is_empty() {
            held.lines.push(tail.to_string());
        }
        std::mem::take(&mut held.lines)
    }
}

/// Csound's message callback: find the host's sink and record the line.
///
/// # Safety
/// Called by Csound with the instance it was registered on and a NUL-terminated
/// string. Registered only by [`Csound::start`], which sets the host data to a
/// leaked `Messages` living as long as the instance.
unsafe extern "C" fn on_message(cs: CsoundPtr, _attr: c_int, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    // SAFETY: the caller guarantees a live instance whose host data is the
    // `Messages` set in `start`, and a NUL-terminated `msg` valid for this call.
    unsafe {
        let Some(api) = CURRENT_GET_HOST_DATA.get() else {
            return;
        };
        let sink = api(cs) as *const Messages;
        if sink.is_null() {
            return;
        }
        (*sink).push(&std::ffi::CStr::from_ptr(msg).to_string_lossy());
    }
}

/// `csoundGetHostData`, so the C callback above can reach the host's sink.
///
/// A C function pointer carries no state, and the one thing it needs — how to
/// ask Csound for the host data — lives in the dynamically loaded [`Api`]. All
/// instances share one library, so this is set once when the first one loads.
static CURRENT_GET_HOST_DATA: std::sync::OnceLock<unsafe extern "C" fn(CsoundPtr) -> *mut c_void> =
    std::sync::OnceLock::new();

/// Where to look for the shared library, in order.
///
/// `RUDEL_CSOUND_LIB` first so a user with an unpacked or non-standard build
/// can point at it; then the bare name, which lets the OS loader use `PATH` /
/// `LD_LIBRARY_PATH` and covers every packaged install; then the default
/// install locations, because the Windows installer does not put its `bin` on
/// `PATH` and "install Csound, restart nothing" should work.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("RUDEL_CSOUND_LIB") {
        out.push(PathBuf::from(explicit));
    }
    let (bare, defaults): (&str, &[&str]) = if cfg!(target_os = "windows") {
        (
            "csound64.dll",
            &[r"C:\Program Files\Csound6_x64\bin\csound64.dll"],
        )
    } else if cfg!(target_os = "macos") {
        (
            "libcsound64.dylib",
            &[
                "/Library/Frameworks/CsoundLib64.framework/CsoundLib64",
                "/opt/homebrew/lib/libcsound64.dylib",
                "/usr/local/lib/libcsound64.dylib",
            ],
        )
    } else {
        (
            "libcsound64.so",
            &["/usr/lib/libcsound64.so", "/usr/local/lib/libcsound64.so"],
        )
    };
    out.push(PathBuf::from(bare));
    out.extend(defaults.iter().map(PathBuf::from));
    out
}

impl Api {
    /// Open the library and resolve every entry point.
    fn load() -> Result<Api, String> {
        Api::load_from(&candidates())
    }

    fn load_from(paths: &[PathBuf]) -> Result<Api, String> {
        let mut tried = Vec::new();
        for path in paths {
            // SAFETY: opening a shared library runs its initialisers. This is
            // the library the user installed and named; there is no way to
            // check it further before it is mapped.
            match unsafe { Library::new(path) } {
                Ok(lib) => return Api::bind(lib),
                Err(e) => tried.push(format!("{}: {e}", path.display())),
            }
        }
        Err(format!(
            "libcsound not found. Install Csound (https://csound.com/download.html), \
             or set RUDEL_CSOUND_LIB to the library. Tried:\n  {}",
            tried.join("\n  ")
        ))
    }

    fn bind(lib: Library) -> Result<Api, String> {
        /// Resolve one symbol at the signature the Csound C API documents for
        /// it, and copy out the bare function pointer.
        ///
        /// The pointer outlives the `Symbol` that produced it, which is sound
        /// only because [`Api`] keeps the `Library` alive alongside it — see
        /// the `_lib` field. Naming the type here rather than transmuting an
        /// opaque pointer means the compiler checks each signature is at least
        /// a function pointer of that shape.
        macro_rules! sym {
            ($name:literal: $ty:ty) => {{
                // SAFETY: the symbol is bound to the signature Csound
                // documents for that name. A name that is absent is an error
                // rather than a null call.
                let s: Symbol<$ty> = unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| format!("libcsound is missing {}: {e}", $name))?;
                *s
            }};
        }

        let size_of_myflt = sym!("csoundGetSizeOfMYFLT": unsafe extern "C" fn() -> c_int);
        // SAFETY: a plain query, no instance involved. Checked before anything
        // reads `spout`, since a single-precision build would make that noise
        // rather than a wrong-sounding note.
        if unsafe { size_of_myflt() } as usize != size_of::<Myflt>() {
            return Err(
                "libcsound is a single-precision build; Rudel needs the double-precision \
                 one (csound64)"
                    .to_string(),
            );
        }
        let initialize = sym!("csoundInitialize": unsafe extern "C" fn(c_int) -> c_int);
        // SAFETY: the documented process-wide setup call, idempotent and safe
        // to repeat; these flags only decline handlers Csound would install.
        unsafe { initialize(CSOUNDINIT_NO_ATEXIT | CSOUNDINIT_NO_SIGNAL_HANDLER) };
        let _ = CURRENT_GET_HOST_DATA
            .set(sym!("csoundGetHostData": unsafe extern "C" fn(CsoundPtr) -> *mut c_void));

        Ok(Api {
            create: sym!("csoundCreate": unsafe extern "C" fn(*mut c_void) -> CsoundPtr),
            set_option: sym!("csoundSetOption":
                unsafe extern "C" fn(CsoundPtr, *const c_char) -> c_int),
            compile_orc: sym!("csoundCompileOrc":
                unsafe extern "C" fn(CsoundPtr, *const c_char) -> c_int),
            start: sym!("csoundStart": unsafe extern "C" fn(CsoundPtr) -> c_int),
            perform_ksmps: sym!("csoundPerformKsmps": unsafe extern "C" fn(CsoundPtr) -> c_int),
            input_message: sym!("csoundInputMessage":
                unsafe extern "C" fn(CsoundPtr, *const c_char)),
            get_spout: sym!("csoundGetSpout": unsafe extern "C" fn(CsoundPtr) -> *mut Myflt),
            get_ksmps: sym!("csoundGetKsmps": unsafe extern "C" fn(CsoundPtr) -> c_uint),
            get_nchnls: sym!("csoundGetNchnls": unsafe extern "C" fn(CsoundPtr) -> c_uint),
            host_audio_io: sym!("csoundSetHostImplementedAudioIO":
                unsafe extern "C" fn(CsoundPtr, c_int, c_int)),
            set_message_callback: sym!("csoundSetMessageStringCallback":
                unsafe extern "C" fn(CsoundPtr, MessageCallback)),
            destroy: sym!("csoundDestroy": unsafe extern "C" fn(CsoundPtr)),
            _lib: lib,
        })
    }
}

/// A started Csound instance rendering into Rudel's mixer.
pub struct Csound {
    api: Api,
    cs: CsoundPtr,
    /// What Csound has said, as the instance's host data. Boxed so the address
    /// handed to `csoundCreate` stays valid however this struct is moved.
    messages: Box<Messages>,
    /// Frames per k-cycle, and channels — read once at start, since a running
    /// Csound cannot change either.
    ksmps: usize,
    nchnls: usize,
    /// Frames of the current k-cycle not yet handed to the mixer. A callback
    /// buffer is rarely a whole number of k-cycles, so what is left over has to
    /// survive to the next one or every block boundary would drop samples.
    spare: Vec<(f32, f32)>,
    /// Set once `perform_ksmps` reports the performance is over, so a finished
    /// instance goes quiet instead of being pumped forever.
    finished: bool,
}

// SAFETY: a Csound instance is not internally synchronised, but it is not
// thread-*bound* either — the API is explicit that a host may drive one from
// whichever thread it likes as long as only one does at a time. Rudel creates
// the instance on the host thread and hands ownership to the mixer through a
// channel; after that only the audio callback touches it, so there is exactly
// one owner throughout.
unsafe impl Send for Csound {}

impl Csound {
    /// Open the library, create an instance at `sample_rate` and start it.
    pub fn new(sample_rate: f32) -> Result<Csound, String> {
        let api = Api::load()?;
        let messages = Box::new(Messages::default());
        // SAFETY: the host data is the boxed sink, whose heap address outlives
        // the instance — `Csound` owns the box and destroys the instance first.
        // `csoundCreate` returns null on failure, checked before any use.
        let cs = unsafe { (api.create)(&*messages as *const Messages as *mut c_void) };
        if cs.is_null() {
            return Err("csoundCreate returned null".to_string());
        }
        let mut csound = Csound {
            api,
            cs,
            messages,
            ksmps: 0,
            nchnls: 0,
            spare: Vec::new(),
            finished: false,
        };
        csound.start(sample_rate).map_err(|why| {
            // A start failure is where a bad install shows itself (a missing
            // opcode directory, say), and Csound will have said why.
            match csound.messages.take().last() {
                Some(said) => format!("{why}: {said}"),
                None => why,
            }
        })?;
        Ok(csound)
    }

    fn start(&mut self, sample_rate: f32) -> Result<(), String> {
        // The host takes the audio: no device is opened, and `spout` holds each
        // k-cycle for the mixer to read. Must be set before the instance
        // starts, or Csound has already chosen a real-time output module.
        // SAFETY: `self.cs` is a live instance; every string is NUL-terminated
        // and only borrowed for the duration of the call.
        unsafe {
            (self.api.set_message_callback)(self.cs, on_message);
            (self.api.host_audio_io)(self.cs, 1, 0);
            for opt in [
                "-m0d",              // no banners, no per-note amplitude reports
                "-d",                // no graphs
                "--sample-accurate", // place an onset inside the k-cycle
                "--nodisplays",
            ] {
                let c = CString::new(opt).expect("option has no NUL");
                if (self.api.set_option)(self.cs, c.as_ptr()) != 0 {
                    return Err(format!("csoundSetOption({opt}) failed"));
                }
            }
        }
        self.compile_orc(&header(sample_rate))?;
        // SAFETY: the instance is configured and its header compiled, which is
        // the documented precondition for `csoundStart`.
        let rc = unsafe { (self.api.start)(self.cs) };
        if rc != 0 {
            return Err(format!("csoundStart failed ({rc})"));
        }
        // SAFETY: both are simple reads off a started instance.
        unsafe {
            self.ksmps = (self.api.get_ksmps)(self.cs) as usize;
            self.nchnls = (self.api.get_nchnls)(self.cs) as usize;
        }
        if self.ksmps == 0 || self.nchnls == 0 {
            return Err("csound started with no ksmps or channels".to_string());
        }
        // Compiled after start so a failure in it is reported without stopping
        // the instance the script is about to add its own instruments to.
        self.compile_orc(PRESETS_ORC)
    }

    /// Compile orchestra code. Valid before *and* after start — that is what
    /// makes `loadCsound` re-evaluable while a pattern plays.
    pub fn compile_orc(&mut self, code: &str) -> Result<(), String> {
        let c = CString::new(code).map_err(|_| "orchestra contains a NUL byte".to_string())?;
        // Anything said before this compile belongs to an earlier one.
        let _ = self.messages.take();
        // SAFETY: `self.cs` is live and `c` outlives the call. Csound parses
        // the text and reports a syntax error by return code rather than by
        // aborting.
        let rc = unsafe { (self.api.compile_orc)(self.cs, c.as_ptr()) };
        if rc != 0 {
            // The return code alone is `-1` for every kind of mistake, so the
            // lines Csound wrote are the whole diagnostic. Kept in order and
            // capped, since a long orchestra can fault on many lines at once.
            let said = self.messages.take();
            let detail: Vec<&str> = said
                .iter()
                .map(String::as_str)
                .filter(|l| !l.starts_with("--Csound version") && !l.starts_with("[commit:"))
                .take(8)
                .collect();
            return Err(if detail.is_empty() {
                format!("csound could not compile the orchestra ({rc})")
            } else {
                format!("csound: {}", detail.join("\n"))
            });
        }
        Ok(())
    }

    /// Send a score statement (`i "Name" 0 0.5 440 0.16 "..."`).
    pub fn input_message(&mut self, message: &str) {
        let Ok(c) = CString::new(message) else { return };
        // SAFETY: `self.cs` is live and `c` outlives the call. Csound queues
        // the statement for the next k-cycle; a malformed one is logged and
        // dropped, not fatal.
        unsafe { (self.api.input_message)(self.cs, c.as_ptr()) }
    }

    /// Render `out.len()` stereo frames, *adding* into `out` so Csound sits
    /// alongside the tune's other layers rather than replacing them.
    pub fn render_into(&mut self, out: &mut [(f32, f32)]) {
        let mut written = 0;
        while written < out.len() {
            if self.spare.is_empty() && (self.finished || !self.perform()) {
                return;
            }
            let take = self.spare.len().min(out.len() - written);
            for (dst, (l, r)) in out[written..written + take].iter_mut().zip(&self.spare) {
                dst.0 += l;
                dst.1 += r;
            }
            self.spare.drain(..take);
            written += take;
        }
    }

    /// Run one k-cycle and copy `spout` into [`Csound::spare`]. Returns false
    /// once the performance has ended.
    fn perform(&mut self) -> bool {
        // SAFETY: `self.cs` is a started instance. `perform_ksmps` is the
        // documented host-driven render call; it returns non-zero exactly once,
        // when the performance ends, which is latched so it is not called again.
        let done = unsafe { (self.api.perform_ksmps)(self.cs) } != 0;
        if done {
            self.finished = true;
            return false;
        }
        // SAFETY: after a successful `perform_ksmps`, `spout` points at
        // `ksmps * nchnls` MYFLTs of this k-cycle's output, both read from the
        // instance at start. The slice is only read, and only until the next
        // `perform_ksmps` call, which cannot happen while it is borrowed.
        let spout = unsafe {
            let p = (self.api.get_spout)(self.cs);
            if p.is_null() {
                return false;
            }
            std::slice::from_raw_parts(p, self.ksmps * self.nchnls)
        };
        self.spare.clear();
        self.spare.extend(spout.chunks_exact(self.nchnls).map(|f| {
            // Mono orchestras feed both sides; anything wider than stereo keeps
            // its first two channels, which is what a stereo device can carry.
            let l = f[0] as f32;
            (l, if self.nchnls > 1 { f[1] as f32 } else { l })
        }));
        true
    }
}

impl Drop for Csound {
    fn drop(&mut self) {
        // SAFETY: `self.cs` was created by `csoundCreate` and is destroyed
        // exactly once, here, since `Csound` is not `Clone`.
        unsafe { (self.api.destroy)(self.cs) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A started instance, or `None` with the reason printed.
    ///
    /// Csound is an optional runtime dependency, so these skip where it is not
    /// installed rather than failing a checkout that never asked for it. Set
    /// `RUDEL_CSOUND_REQUIRED=1` (as CI does where the library is provisioned)
    /// to turn a skip back into a failure, so "skipped everywhere, forever" is
    /// not a silent outcome.
    fn csound(sample_rate: f32) -> Option<Csound> {
        match Csound::new(sample_rate) {
            Ok(cs) => Some(cs),
            Err(why) => {
                assert!(
                    std::env::var("RUDEL_CSOUND_REQUIRED").is_err(),
                    "RUDEL_CSOUND_REQUIRED is set but Csound did not start: {why}"
                );
                eprintln!("skipping: {why}");
                None
            }
        }
    }

    #[test]
    fn an_instrument_from_a_script_renders_audio() {
        // The whole path in one: compile an orchestra the way `loadCsound`
        // does, trigger it the way a hap does, and read the samples back out
        // of `spout` the way the mixer does.
        let Some(mut cs) = csound(44100.0) else {
            return;
        };
        cs.compile_orc("instr Beep\n  asig = oscili(p5, p4)\n  out(asig, asig)\nendin\n")
            .expect("compile");
        cs.input_message(r#"i "Beep" 0 0.5 440 0.5"#);

        let mut out = vec![(0.0f32, 0.0f32); 4410]; // 100 ms
        cs.render_into(&mut out);

        let peak = out.iter().fold(0.0f32, |m, (l, _)| m.max(l.abs()));
        assert!(peak > 0.1, "expected a tone, peak was {peak}");
        // Stereo, and `out(asig, asig)` puts the same signal on both sides.
        assert!(out.iter().all(|(l, r)| (l - r).abs() < 1e-6));
    }

    #[test]
    fn rendering_adds_to_the_buffer_and_survives_block_boundaries() {
        // `ksmps` is 32 and a callback buffer is not a multiple of it, so the
        // remainder of a k-cycle has to carry over. A gap would show up as a
        // zero run at each boundary; a reset would show up as the sine
        // restarting. Checked by rendering the same tone in one pass and in
        // ragged chunks and comparing.
        let Some(mut a) = csound(44100.0) else { return };
        let Some(mut b) = csound(44100.0) else { return };
        let orc = "instr Beep\n  asig = oscili(p5, p4)\n  out(asig, asig)\nendin\n";
        let msg = r#"i "Beep" 0 1 220 0.5"#;

        let mut whole = vec![(0.0f32, 0.0f32); 3000];
        a.compile_orc(orc).expect("compile");
        a.input_message(msg);
        a.render_into(&mut whole);

        let mut ragged = vec![(0.0f32, 0.0f32); 3000];
        b.compile_orc(orc).expect("compile");
        b.input_message(msg);
        let mut at = 0;
        for len in [7usize, 100, 33, 512, 1, 1000].iter().cycle() {
            let end = (at + len).min(ragged.len());
            b.render_into(&mut ragged[at..end]);
            at = end;
            if at == ragged.len() {
                break;
            }
        }
        for (i, (x, y)) in whole.iter().zip(&ragged).enumerate() {
            assert!((x.0 - y.0).abs() < 1e-6, "frame {i} differs across blocks");
        }

        // And it *adds*: a second pass over a non-empty buffer sums into it.
        let mut sum = whole.clone();
        let Some(mut c) = csound(44100.0) else { return };
        c.compile_orc(orc).expect("compile");
        c.input_message(msg);
        c.render_into(&mut sum);
        assert!(
            sum.iter()
                .zip(&whole)
                .any(|(s, w)| (s.0 - w.0).abs() > 1e-6),
            "rendering should add into the buffer, not overwrite it"
        );
    }

    #[test]
    fn an_orchestra_error_reports_what_csound_said() {
        // `csoundCompileOrc` returns -1 for everything, so without the message
        // callback a mistyped instrument is "could not compile (-1)" and the
        // user has no line number. This is the only diagnostic there is.
        let Some(mut cs) = csound(44100.0) else {
            return;
        };
        let err = cs
            .compile_orc("instr Broken\n  asig = nosuchopcode(1, 2)\n  out(asig)\nendin\n")
            .expect_err("should not compile");
        // What matters is that the user is told *where*: the return code is -1
        // whatever went wrong.
        assert!(err.contains("error"), "unhelpful error: {err}");
        assert!(
            err.contains("line 2"),
            "error should locate the fault: {err}"
        );
        // And the instance survives it: a later, valid orchestra still compiles.
        cs.compile_orc("instr Fine\n  out(a(0), a(0))\nendin\n")
            .expect("a later compile should still work");
    }

    #[test]
    fn a_diagnostic_split_across_calls_comes_back_as_lines() {
        // Csound's message callback is a stream. A syntax error echoes the
        // offending source one *character* per call, so a sink that treats each
        // call as a line reports the fault as a column of single letters — and
        // the eight-line cap then truncates it to eight of them.
        let sink = Messages::default();
        sink.push("\nerror: syntax error (token \"it\")\nline 12:\n");
        for ch in ">>> opcode set_tempo(it  <<<".chars() {
            sink.push(&ch.to_string());
        }
        assert_eq!(
            sink.take(),
            [
                "error: syntax error (token \"it\")",
                "line 12:",
                // Flushed by `take` even though no newline ever arrived.
                ">>> opcode set_tempo(it  <<<",
            ]
        );
    }

    #[test]
    fn a_missing_library_is_reported_not_panicked() {
        // The message a user without Csound sees. It has to name the variable
        // and the download, because there is nothing else to go on.
        let err = match Api::load_from(&[PathBuf::from("definitely-not-csound")]) {
            Err(err) => err,
            Ok(_) => panic!("should not have found a library"),
        };
        assert!(err.contains("RUDEL_CSOUND_LIB"), "{err}");
        assert!(err.contains("csound.com"), "{err}");
    }
}
