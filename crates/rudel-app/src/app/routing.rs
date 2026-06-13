use super::{Output, RudelApp};
use crate::volume::MAX_VOLUME_PERCENT;
use rudel_midi::{MidiEngine, MidiIn, MidiOut};
use rudel_osc::{OscEngine, OscOut};

impl RudelApp {
    /// Connect (or reconnect) a MIDI input device: incoming CCs feed `ccin`, and
    /// MIDI clock can drive `cps` when `clock_sync` is on.
    pub(super) fn connect_input(&mut self) {
        let port = {
            let p = self.midi_in_port.trim();
            if p.is_empty() { None } else { Some(p) }
        };
        match MidiIn::connect(port) {
            Ok(input) => {
                self.midi_in = Some(input);
                self.io_error = None;
                self.status = "MIDI input connected".to_string();
            }
            Err(e) => self.io_error = Some(format!("MIDI in: {e}")),
        }
    }

    pub(super) fn set_playing(&mut self, playing: bool) {
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
        self.volume_percent = volume_percent.max(0.0).min(MAX_VOLUME_PERCENT);
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

    fn ensure_midi(&mut self) {
        if self.midi.is_some() {
            return;
        }
        let port = {
            let p = self.midi_port.trim();
            if p.is_empty() { None } else { Some(p) }
        };
        match MidiOut::connect(port) {
            Ok(out) => {
                let pat = self.current.clone().unwrap_or_else(rudel_core::silence);
                self.midi = Some(MidiEngine::start(out, pat, self.cps));
                self.io_error = None;
            }
            Err(e) => {
                self.io_error = Some(format!("MIDI: {e}"));
            }
        }
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
