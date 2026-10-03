use super::{Output, RudelApp};
use crate::volume::MAX_VOLUME_PERCENT;
use rudel_midi::{MidiEngine, MidiIn, MidiOut};
use rudel_osc::{OscEngine, OscOut};

impl RudelApp {
    /// Connect (or reconnect) a MIDI input device: incoming CCs feed `ccin`, and
    /// MIDI clock can drive `cps` when `clock_sync` is on. Like the output, the
    /// device open can block while the OS MIDI subsystem starts up, so it runs
    /// on a background thread and is adopted by [`poll_midi_in_connect`] instead
    /// of freezing the UI.
    ///
    /// [`poll_midi_in_connect`]: RudelApp::poll_midi_in_connect
    pub(super) fn connect_input(&mut self) {
        if self.midi_in_pending.is_some() {
            return; // a connection is already in flight
        }
        let port = {
            let p = self.midi_in_port.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_string())
            }
        };
        self.status = "connecting MIDI input…".to_string();
        self.midi_in_pending = Some(std::thread::spawn(move || MidiIn::connect(port.as_deref())));
    }

    /// Adopt a background MIDI input connection once it finishes. Called each
    /// frame; returns `true` while a connection is still in flight.
    pub(super) fn poll_midi_in_connect(&mut self) -> bool {
        match &self.midi_in_pending {
            Some(handle) if handle.is_finished() => {}
            Some(_) => return true, // still connecting
            None => return false,
        }
        let handle = self.midi_in_pending.take().unwrap();
        match handle.join() {
            Ok(Ok(input)) => {
                self.midi_in = Some(input);
                self.io_error = None;
                self.status = "MIDI input connected".to_string();
            }
            Ok(Err(e)) => {
                self.io_error = Some(format!("MIDI in: {e}"));
                self.status = "MIDI input connect failed".to_string();
            }
            Err(_) => {
                self.io_error = Some("MIDI in: connect thread panicked".to_string());
            }
        }
        false
    }

    pub(super) fn set_playing(&mut self, playing: bool) {
        if playing && !self.playing {
            self.play_start = Some(std::time::Instant::now());
        } else if !playing {
            self.play_start = None;
            // Upstream's scheduler resets `timeline` offsets whenever it
            // stops (`onToggle(false)` → `reset_state`), so a cued timeline
            // realigns to wherever playback next starts.
            rudel_core::reset_timelines();
        }
        self.playing = playing;
        self.route();
    }

    /// Silence all outputs without discarding the evaluated pattern, matching
    /// Strudel's `hush` (Ctrl/Alt+.). Playback resumes on the next evaluate
    /// or Play.
    pub(super) fn hush(&mut self) {
        self.set_playing(false);
        self.status = "hushed".to_string();
    }

    /// Panic / reset (Ctrl+Shift+.): stop playback and tear down the MIDI/OSC
    /// back-ends so any stuck notes get an all-notes-off reset. Stronger than
    /// `hush`, which leaves the schedulers running on silence. They reconnect
    /// lazily on the next play/evaluate.
    pub(super) fn panic(&mut self) {
        self.set_playing(false);
        // Dropping the engines runs their teardown: the MIDI scheduler emits
        // reset (all-notes-off / CC reset) messages as it stops.
        self.midi = None;
        self.osc = None;
        // Csound is not torn down with them: dropping it would take the
        // compiled orchestra too, and `loadCsound` only runs on the next
        // evaluate. Its notes are ended in place instead.
        if let Some(e) = &self.engine {
            e.csound_all_notes_off();
        }
        self.status = "panic".to_string();
    }

    pub(super) fn set_cps(&mut self, cps: f64) {
        self.cps = cps;
        if let Some(e) = &self.engine {
            e.set_cps(cps);
        }
        if let Some(m) = &self.midi {
            m.set_cps(cps);
        }
        if let Some(o) = &self.osc {
            o.set_cps(cps);
        }
    }

    pub(super) fn set_volume_percent(&mut self, volume_percent: f32) {
        self.volume_percent = volume_percent.clamp(0.0, MAX_VOLUME_PERCENT);
        if let Some(e) = &self.engine {
            e.set_volume((self.volume_percent / 100.0) as f64);
        }
    }

    /// Split the current pattern across the audio / MIDI / OSC back-ends.
    ///
    /// Per-pattern `.midi()` / `.osc()` tags always route to their back-end;
    /// untagged events go to the selected default `output`. MIDI/OSC back-ends
    /// are started lazily when the default selects them or a tag routes to them.
    pub(super) fn route(&mut self) {
        let active = if self.playing {
            self.current.clone().unwrap_or_else(rudel_core::silence)
        } else {
            rudel_core::silence()
        };
        let (tag_midi, tag_osc) = if self.playing {
            rudel_lang::output_targets(&active)
        } else {
            (false, false)
        };
        if self.playing && (self.output == Output::Midi || tag_midi) {
            self.ensure_midi();
        }
        if self.playing && (self.output == Output::Osc || tag_osc) {
            self.ensure_osc();
        }
        self.speaks = active
            .query_arc(rudel_core::Frac::zero(), rudel_core::Frac::one())
            .iter()
            .any(|hap| rudel_core::speak::is_speech(&hap.value));
        if let Some(e) = &self.engine {
            // `.speak(...)` is a dominant `onTrigger` upstream: it replaces the
            // sound rather than adding to it, so those haps never reach a voice.
            let audible = rudel_lang::filter_output(&active, "audio", self.output == Output::Audio)
                .filter_values(|v| !rudel_core::speak::is_speech(v));
            e.set_pattern(audible);
        }
        if let Some(m) = &self.midi {
            m.set_pattern(rudel_lang::filter_output(
                &active,
                "midi",
                self.output == Output::Midi,
            ));
        }
        if let Some(o) = &self.osc {
            o.set_pattern(rudel_lang::filter_output(
                &active,
                "osc",
                self.output == Output::Osc,
            ));
        }
    }

    /// Begin connecting the MIDI output if it isn't connected or already
    /// connecting. The first device open can block for a long time while the OS
    /// MIDI subsystem starts up, so the connection runs on a background thread
    /// to keep the UI responsive; [`poll_midi_connect`] adopts the engine when
    /// the thread finishes.
    ///
    /// [`poll_midi_connect`]: RudelApp::poll_midi_connect
    fn ensure_midi(&mut self) {
        if self.midi.is_some() || self.midi_pending.is_some() {
            return;
        }
        let port = {
            let p = self.midi_port.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_string())
            }
        };
        self.status = "connecting MIDI…".to_string();
        self.midi_pending = Some(std::thread::spawn(move || {
            MidiOut::connect(port.as_deref())
        }));
    }

    /// Adopt a background MIDI connection once it finishes: start the scheduler
    /// on the connected port and route the current pattern to it. Called each
    /// frame from the UI loop. Returns `true` while a connection is still in
    /// flight (so the caller can keep repainting).
    pub(super) fn poll_midi_connect(&mut self) -> bool {
        match &self.midi_pending {
            Some(handle) if handle.is_finished() => {}
            Some(_) => return true, // still connecting
            None => return false,
        }
        let handle = self.midi_pending.take().unwrap();
        match handle.join() {
            Ok(Ok(out)) => {
                let pat = self.current.clone().unwrap_or_else(rudel_core::silence);
                self.midi = Some(MidiEngine::start(out, pat, self.cps));
                self.io_error = None;
                self.status = "MIDI connected".to_string();
                // Push the current pattern split to the freshly started engine.
                self.route();
            }
            Ok(Err(e)) => {
                self.io_error = Some(format!("MIDI: {e}"));
                self.status = "MIDI connect failed".to_string();
            }
            Err(_) => {
                self.io_error = Some("MIDI: connect thread panicked".to_string());
            }
        }
        false
    }

    fn ensure_osc(&mut self) {
        if self.osc.is_some() {
            return;
        }
        match OscOut::connect(self.osc_target.trim()) {
            Ok(out) => {
                let pat = self.current.clone().unwrap_or_else(rudel_core::silence);
                self.osc = Some(OscEngine::start(out, pat, self.cps));
                self.io_error = None;
            }
            Err(e) => {
                self.io_error = Some(format!("OSC: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait for any connect thread this test started. Dropping the handle
    /// instead leaves the thread inside the OS MIDI subsystem while the test
    /// binary exits, which corrupts the heap on the way out.
    fn settle(app: &mut RudelApp) {
        if let Some(handle) = app.midi_pending.take() {
            let _ = handle.join();
        }
    }

    fn stopped_app() -> RudelApp {
        RudelApp {
            status: String::new(),
            ..RudelApp::headless()
        }
    }

    #[test]
    fn the_playhead_clock_starts_once_and_stops_with_the_transport() {
        let mut app = stopped_app();
        app.set_playing(true);
        let started = app.play_start.expect("playing sets the clock");

        // Pressing play again while already playing must not restart it, or
        // the pattern jumps back to cycle zero mid-performance.
        app.set_playing(true);
        assert_eq!(app.play_start, Some(started), "the clock keeps running");

        app.set_playing(false);
        assert_eq!(app.play_start, None, "stopping clears it");

        // Stopping while already stopped leaves it cleared rather than
        // starting a clock nothing is reading.
        app.set_playing(false);
        assert_eq!(app.play_start, None);
    }

    #[test]
    fn stopping_resets_timeline_offsets_as_upstream_does() {
        use rudel_core::{Frac, State, Value, ValueMap, pure, sequence};
        // The first cycle a timeline id is played from fixes its offset, so
        // what plays at a cycle tells where the timeline started.
        let pat = sequence(&[pure(Value::Int(10)), pure(Value::Int(20))])
            .slow(4)
            .timeline(pure(Value::Int(7)));
        let first_at = |cycle: i64| {
            let scheduler = ValueMap::from([("cyclist".to_string(), Value::Str("x".into()))]);
            let state = State::with_controls(
                rudel_core::TimeSpan::new(Frac::int(cycle), Frac::int(cycle + 1)),
                scheduler,
            );
            pat.query(&state)[0].value.clone()
        };
        let mut app = stopped_app();
        app.set_playing(true);
        assert_eq!(
            first_at(3),
            Value::Int(10),
            "cycle 3 is the timeline's first"
        );
        assert_eq!(first_at(5), Value::Int(20), "two cycles in");
        app.set_playing(false);
        app.set_playing(true);
        assert_eq!(first_at(5), Value::Int(10), "after a stop it starts afresh");
    }

    #[test]
    fn midi_connects_only_while_playing_and_only_when_it_is_the_output() {
        // Not playing: nothing is routed anywhere, whatever the output says.
        let mut app = stopped_app();
        app.output = Output::Midi;
        app.route();
        assert!(app.midi_pending.is_none(), "stopped, so no connection");

        // Playing with MIDI selected opens the port.
        let mut app = stopped_app();
        app.output = Output::Midi;
        app.playing = true;
        app.route();
        assert!(app.midi_pending.is_some(), "playing, so connect");
        settle(&mut app);

        // Playing with audio selected does not.
        let mut app = stopped_app();
        app.playing = true;
        app.route();
        assert!(app.midi_pending.is_none(), "audio output stays audio");
    }

    #[test]
    fn a_pattern_tagged_midi_connects_even_on_the_audio_output() {
        // `.midi()` on the pattern routes to MIDI whatever the dropdown says,
        // which is how a script sends one part to a synth and keeps the rest.
        let mut app = stopped_app();
        app.playing = true;
        app.current = Some(rudel_lang::eval(r#"note("c3").midi()"#).expect("eval"));
        app.route();
        assert!(app.midi_pending.is_some(), "the tag routes it");
        settle(&mut app);
    }

    #[test]
    fn osc_connects_on_the_same_terms_as_midi() {
        // Connecting only resolves the target and binds a local UDP socket,
        // so this needs no listener on the other end.
        let mut app = stopped_app();
        app.output = Output::Osc;
        app.route();
        assert!(app.osc.is_none(), "stopped, so no connection");

        let mut app = stopped_app();
        app.output = Output::Osc;
        app.playing = true;
        app.route();
        assert!(app.osc.is_some(), "playing on the OSC output connects");

        let mut app = stopped_app();
        app.playing = true;
        app.route();
        assert!(app.osc.is_none(), "audio output stays audio");

        // ...and a pattern tagged `.osc()` connects whatever the dropdown says.
        let mut app = stopped_app();
        app.playing = true;
        app.current = Some(rudel_lang::eval(r#"note("c3").osc()"#).expect("eval"));
        app.route();
        assert!(app.osc.is_some(), "the tag routes it");
    }

    #[test]
    fn a_midi_connection_already_in_flight_is_not_started_again() {
        let mut app = stopped_app();
        app.output = Output::Midi;
        app.playing = true;
        app.route();
        let first = app
            .midi_pending
            .as_ref()
            .map(|handle| handle.thread().id())
            .expect("a connection started");
        app.route();
        assert_eq!(
            app.midi_pending.as_ref().map(|handle| handle.thread().id()),
            Some(first),
            "the same thread, not a second one racing it"
        );
        settle(&mut app);
    }
}

#[cfg(test)]
mod backend_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A MIDI sink that keeps what it was sent.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<Vec<u8>>>>);

    impl rudel_midi::MidiSink for Recorder {
        fn send(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().push(bytes.to_vec());
        }
    }

    fn note_ons(rec: &Recorder) -> usize {
        rec.0
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m[0] & 0xF0 == 0x90)
            .count()
    }

    fn peak(frames: &[(f32, f32)]) -> f32 {
        frames
            .iter()
            .fold(0.0, |m, (l, r)| m.max(l.abs()).max(r.abs()))
    }

    /// Wait up to five seconds for `done`. A deadline rather than a fixed
    /// pause, so a loaded machine is slow rather than failing.
    fn eventually(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn the_selected_output_gets_the_untagged_pattern_and_the_others_do_not() {
        let mut app = RudelApp::headless();
        let (engine, output) = rudel_audio::Engine::with_fake_output(48_000.0);
        app.engine = Some(engine);
        let midi = Recorder::default();
        app.midi = Some(rudel_midi::MidiEngine::start(
            midi.clone(),
            rudel_core::silence(),
            2.0,
        ));
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let target = socket.local_addr().unwrap().to_string();
        let out = rudel_osc::OscOut::connect(&target).unwrap();
        app.osc = Some(rudel_osc::OscEngine::start(out, rudel_core::silence(), 2.0));
        let osc_messages = |socket: &std::net::UdpSocket, secs: f64| {
            let deadline = Instant::now() + Duration::from_secs_f64(secs);
            let mut buf = [0u8; 2048];
            let mut n = 0;
            while Instant::now() < deadline {
                if socket.recv(&mut buf).is_ok() {
                    n += 1;
                }
            }
            n
        };
        app.current = Some(rudel_lang::eval(r#"note("c4*4").s("sine")"#).unwrap());
        app.playing = true;

        app.output = Output::Audio;
        app.route();
        // The fake device's playhead moves only as frames are pulled.
        assert!(
            eventually(|| peak(&output.pull(480)) > 1e-3),
            "audio should play"
        );
        assert_eq!(note_ons(&midi), 0, "MIDI is not selected");
        assert_eq!(osc_messages(&socket, 0.3), 0, "OSC is not selected");

        app.output = Output::Midi;
        app.route();
        let before = note_ons(&midi);
        assert!(eventually(|| note_ons(&midi) > before), "MIDI should play");
        // Two seconds of device time plays out anything scheduled before the
        // switch, however long the machine takes to render it.
        output.pull(96_000);
        assert!(peak(&output.pull(14_400)) < 1e-4, "audio should stop");

        app.output = Output::Osc;
        app.route();
        assert!(osc_messages(&socket, 1.0) > 0, "OSC should play");

        // The volume slider reaches the engine as a fraction of 100%.
        app.set_volume_percent(50.0);
        assert_eq!(app.engine.as_ref().unwrap().volume(), 0.5);
    }
}
