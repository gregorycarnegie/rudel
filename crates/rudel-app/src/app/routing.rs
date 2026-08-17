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
        if let Some(e) = &self.engine {
            e.set_pattern(rudel_lang::filter_output(
                &active,
                "audio",
                self.output == Output::Audio,
            ));
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
