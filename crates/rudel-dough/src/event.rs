// event.rs - dough's event: what a message sets, how it is parsed, merged
// into a sounding voice, and given its defaults. Ported from dough.h.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::dsp::{FileSource, exp2f, init_envelope, nan_fallback};

/// `Source`: what a voice plays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Unknown,
    Tri,
    Sine,
    Saw,
    Zaw,
    Pulse,
    Pulze,
    White,
    Pink,
    Brown,
    Const,
    File,
}

/// `get_source`.
fn source(name: &[u8]) -> Source {
    match name {
        b"triangle" | b"tri" => Source::Tri,
        b"sine" => Source::Sine,
        b"sawtooth" | b"saw" => Source::Saw,
        b"zawtooth" | b"zaw" => Source::Zaw,
        b"pulse" | b"square" => Source::Pulse,
        b"pulze" | b"zquare" => Source::Pulze,
        b"white" => Source::White,
        b"pink" => Source::Pink,
        b"brown" => Source::Brown,
        b"const" => Source::Const,
        _ => Source::Unknown,
    }
}

/// `Command`: what a message asks the engine to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Unknown,
    Play,
    UpdateAll,
    UpdateGroup,
    Panic,
    Hush,
    HushEndless,
    HushGroup,
    Reset,
    ResetTime,
    ResetSchedule,
}

/// `get_command`.
fn command(name: &[u8]) -> Command {
    match name {
        b"play" => Command::Play,
        b"update_all" => Command::UpdateAll,
        b"update_group" => Command::UpdateGroup,
        b"panic" => Command::Panic,
        b"hush" => Command::Hush,
        b"hush_endless" => Command::HushEndless,
        b"hush_group" => Command::HushGroup,
        b"reset" => Command::Reset,
        b"reset_time" => Command::ResetTime,
        b"reset_schedule" => Command::ResetSchedule,
        _ => Command::Unknown,
    }
}

/// `get_ftype`: the filter slope, as a stage count selector.
fn ftype(name: &[u8]) -> i32 {
    match name {
        b"12db" => 0,
        b"24db" => 1,
        b"48db" => 2,
        _ => atoi(name),
    }
}

/// dough's own `atoi` (the WebAssembly build's): any non-digit makes it 0.
fn atoi(s: &[u8]) -> i32 {
    let (negative, digits) = match s {
        [b'-', rest @ ..] => (true, rest),
        _ => (false, s),
    };
    let mut acc = 0_i32;
    for &c in digits {
        let digit = i32::from(c) - i32::from(b'0');
        if !(0..=9).contains(&digit) {
            return 0;
        }
        acc = acc.wrapping_mul(10).wrapping_add(digit);
    }
    if negative { acc.wrapping_neg() } else { acc }
}

/// dough's own `atof` (the WebAssembly build's), in its order of operations.
fn atof(s: &[u8]) -> f64 {
    let (sign, s) = match s {
        [b'-', rest @ ..] => (-1.0, rest),
        _ => (1.0, s),
    };
    let integer_end = s.iter().take_while(|c| c.is_ascii_digit()).count();
    let mut integer = 0.0_f64;
    for &c in &s[..integer_end] {
        integer *= 10.0;
        integer += f64::from(c - b'0');
    }
    let mut rest = &s[integer_end..];
    let Some(&stop) = rest.first() else {
        return sign * integer;
    };
    let mut significand = integer;
    if stop == b'.' {
        rest = &rest[1..];
        let mut multiplier = 1.0_f64;
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        for &c in &rest[..digits] {
            multiplier *= 0.1;
            significand += multiplier * f64::from(c - b'0');
        }
        rest = &rest[digits..];
    }
    let Some(&stop) = rest.first() else {
        return sign * significand;
    };
    let mut exponent = if stop == b'e' || stop == b'E' {
        atoi(&rest[1..])
    } else {
        0
    };
    let mut value = significand;
    while exponent > 0 {
        value *= 10.0;
        exponent -= 1;
    }
    while exponent < 0 {
        value *= 0.1;
        exponent += 1;
    }
    sign * value
}

/// `midi2freq`.
fn midi2freq(midi: f32) -> f32 {
    (exp2f(((midi as f64 - 69.0) / 12.0) as f32) as f64 * 440.0) as f32
}

/// dough's `Event`: every parameter a message can set, NaN (or -1) until it is
/// set, and the switches `init_event` derives from them.
#[derive(Clone, Copy)]
pub(crate) struct Event {
    pub(crate) cmd: Command,
    pub(crate) group: i32,
    pub(crate) voices: i32,
    pub(crate) reset: i32,
    pub(crate) freq: f32,
    pub(crate) rate: f32,
    pub(crate) speed: f32,
    pub(crate) glide: f32,
    pub(crate) glide_active: bool,
    pub(crate) gain: f32,
    pub(crate) pan: f32,
    pub(crate) velocity: f32,
    pub(crate) postgain: f32,
    pub(crate) gain_adsr_active: bool,
    pub(crate) gate: f32,
    pub(crate) duration: f32,
    pub(crate) sound: Source,
    pub(crate) file_source: Option<FileSource>,
    pub(crate) file: i32,
    pub(crate) pw: f32,
    pub(crate) attack: f32,
    pub(crate) decay: f32,
    pub(crate) sustain: f32,
    pub(crate) release: f32,
    pub(crate) lpf: f32,
    pub(crate) lpq: f32,
    pub(crate) lp_active: bool,
    pub(crate) lpe: f32,
    pub(crate) lpa: f32,
    pub(crate) lpd: f32,
    pub(crate) lps: f32,
    pub(crate) lpr: f32,
    pub(crate) lp_adsr_active: bool,
    pub(crate) hpf: f32,
    pub(crate) hpq: f32,
    pub(crate) hp_active: bool,
    pub(crate) hpe: f32,
    pub(crate) hpa: f32,
    pub(crate) hpd: f32,
    pub(crate) hps: f32,
    pub(crate) hpr: f32,
    pub(crate) hp_adsr_active: bool,
    pub(crate) bpf: f32,
    pub(crate) bpq: f32,
    pub(crate) bp_active: bool,
    pub(crate) bpe: f32,
    pub(crate) bpa: f32,
    pub(crate) bpd: f32,
    pub(crate) bps: f32,
    pub(crate) bpr: f32,
    pub(crate) bp_adsr_active: bool,
    pub(crate) ftype: i32,
    pub(crate) penv: f32,
    pub(crate) patt: f32,
    pub(crate) pdec: f32,
    pub(crate) psus: f32,
    pub(crate) prel: f32,
    pub(crate) p_adsr_active: bool,
    pub(crate) vib: f32,
    pub(crate) vibmod: f32,
    pub(crate) vib_active: bool,
    pub(crate) fm: f32,
    pub(crate) fmh: f32,
    pub(crate) fm_active: bool,
    pub(crate) fme: f32,
    pub(crate) fma: f32,
    pub(crate) fmd: f32,
    pub(crate) fms: f32,
    pub(crate) fmr: f32,
    pub(crate) fm_adsr_active: bool,
    pub(crate) am: f32,
    pub(crate) amdepth: f32,
    pub(crate) am_active: bool,
    pub(crate) rm: f32,
    pub(crate) rmdepth: f32,
    pub(crate) rm_active: bool,
    pub(crate) phaser: f32,
    pub(crate) phaserdepth: f32,
    pub(crate) phasersweep: f32,
    pub(crate) phasercenter: f32,
    pub(crate) phaser_active: bool,
    pub(crate) flanger: f32,
    pub(crate) flangerdepth: f32,
    pub(crate) flangerfeedback: f32,
    pub(crate) flanger_active: bool,
    pub(crate) chorus: f32,
    pub(crate) chorusdepth: f32,
    pub(crate) chorusdelay: f32,
    pub(crate) chorus_active: bool,
    pub(crate) begin: f32,
    pub(crate) end: f32,
    pub(crate) coarse: f32,
    pub(crate) coarse_active: bool,
    pub(crate) crush: f32,
    pub(crate) crush_active: bool,
    pub(crate) distort: f32,
    pub(crate) distortvol: f32,
    pub(crate) distort_active: bool,
    pub(crate) delay: f32,
    pub(crate) delaytime: f32,
    pub(crate) delayfeedback: f32,
    pub(crate) delay_active: bool,
    pub(crate) verb: f32,
    pub(crate) verbdecay: f32,
    pub(crate) verbdamp: f32,
    pub(crate) verbpredelay: f32,
    pub(crate) verbdiff: f32,
    pub(crate) verb_active: bool,
    pub(crate) orbit: i32,
    pub(crate) channels: i32,
    pub(crate) time: f64,
    pub(crate) repeat: f32,
}

impl Default for Event {
    /// `reset_event`.
    fn default() -> Event {
        let nan = f32::NAN;
        Event {
            cmd: Command::Play,
            group: -1,
            voices: -1,
            reset: -1,
            freq: nan,
            rate: nan,
            speed: nan,
            glide: nan,
            glide_active: false,
            gain: nan,
            pan: nan,
            velocity: nan,
            postgain: nan,
            gain_adsr_active: false,
            gate: nan,
            duration: nan,
            sound: Source::Unknown,
            file_source: None,
            file: -1,
            pw: nan,
            attack: nan,
            decay: nan,
            sustain: nan,
            release: nan,
            lpf: nan,
            lpq: nan,
            lp_active: false,
            lpe: nan,
            lpa: nan,
            lpd: nan,
            lps: nan,
            lpr: nan,
            lp_adsr_active: false,
            hpf: nan,
            hpq: nan,
            hp_active: false,
            hpe: nan,
            hpa: nan,
            hpd: nan,
            hps: nan,
            hpr: nan,
            hp_adsr_active: false,
            bpf: nan,
            bpq: nan,
            bp_active: false,
            bpe: nan,
            bpa: nan,
            bpd: nan,
            bps: nan,
            bpr: nan,
            bp_adsr_active: false,
            ftype: -1,
            penv: nan,
            patt: nan,
            pdec: nan,
            psus: nan,
            prel: nan,
            p_adsr_active: false,
            vib: nan,
            vibmod: nan,
            vib_active: false,
            fm: nan,
            fmh: nan,
            fm_active: false,
            fme: nan,
            fma: nan,
            fmd: nan,
            fms: nan,
            fmr: nan,
            fm_adsr_active: false,
            am: nan,
            amdepth: nan,
            am_active: false,
            rm: nan,
            rmdepth: nan,
            rm_active: false,
            phaser: nan,
            phaserdepth: nan,
            phasersweep: nan,
            phasercenter: nan,
            phaser_active: false,
            flanger: nan,
            flangerdepth: nan,
            flangerfeedback: nan,
            flanger_active: false,
            chorus: nan,
            chorusdepth: nan,
            chorusdelay: nan,
            chorus_active: false,
            begin: nan,
            end: nan,
            coarse: nan,
            coarse_active: false,
            crush: nan,
            crush_active: false,
            distort: nan,
            distortvol: nan,
            distort_active: false,
            delay: nan,
            delaytime: nan,
            delayfeedback: nan,
            delay_active: false,
            verb: nan,
            verbdecay: nan,
            verbdamp: nan,
            verbpredelay: nan,
            verbdiff: nan,
            verb_active: false,
            orbit: -1,
            channels: 1,
            time: f64::NAN,
            repeat: nan,
        }
    }
}

/// The tokens of a message as the WebAssembly build's `strtok` splits it on
/// `/`: an empty token between two slashes (or before a leading one) counts,
/// and a trailing slash adds none.
fn tokens(message: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = Some(message).filter(|m| !m.is_empty());
    std::iter::from_fn(move || {
        let current = rest?;
        match current.iter().position(|&c| c == b'/') {
            Some(at) => {
                let next = &current[at + 1..];
                rest = Some(next).filter(|n| !n.is_empty());
                Some(&current[..at])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

impl Event {
    /// `parse_event`: set what `key/value/...` names. An unknown key is
    /// ignored, a missing last value too.
    pub(crate) fn parse(&mut self, message: &[u8]) {
        // `strncpy` stops at a NUL, and the buffer holds 1023 bytes.
        let message = &message[..message
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(message.len())];
        let message = &message[..message.len().min(1023)];
        let mut key: &[u8] = &[];
        for (index, token) in tokens(message).enumerate() {
            if index % 2 == 0 {
                key = token;
                continue;
            }
            let float = || atof(token) as f32;
            match key {
                b"dough" | b"dirt" => self.cmd = command(token),
                b"time" | b"t" => self.time = atof(token),
                b"repeat" | b"rep" => self.repeat = float(),
                b"gate" => self.gate = float(),
                b"duration" | b"d" => self.duration = float(),
                b"group" => self.group = atoi(token),
                b"voices" => self.voices = atoi(token),
                b"freq" => self.freq = float(),
                b"note" => self.freq = midi2freq(float()),
                b"speed" => self.speed = float(),
                b"glide" => self.glide = float(),
                b"sound" | b"s" => self.sound = source(token),
                b"file" => self.file = atoi(token),
                b"pw" => self.pw = float(),
                b"lpf" | b"cutoff" => self.lpf = float(),
                b"lpq" | b"resonance" => self.lpq = float(),
                b"lpa" | b"lpattack" => self.lpa = float(),
                b"lpd" | b"lpdecay" => self.lpd = float(),
                b"lps" | b"lpsustain" => self.lps = float(),
                b"lpr" | b"lprelease" => self.lpr = float(),
                b"lpe" | b"lpenv" => self.lpe = float(),
                b"hpf" | b"hcutoff" => self.hpf = float(),
                b"hpq" | b"hresonance" => self.hpq = float(),
                b"hpa" => self.hpa = float(),
                b"hpd" => self.hpd = float(),
                b"hps" => self.hps = float(),
                b"hpr" => self.hpr = float(),
                b"hpe" | b"hpenv" => self.hpe = float(),
                b"bpf" | b"bandf" => self.bpf = float(),
                b"bpq" | b"bandq" => self.bpq = float(),
                b"bpa" | b"bpattack" => self.bpa = float(),
                b"bpd" | b"bpdecay" => self.bpd = float(),
                b"bps" | b"bpsustain" => self.bps = float(),
                b"bpr" | b"bprelease" => self.bpr = float(),
                b"bpe" | b"bpenv" => self.bpe = float(),
                b"ftype" => self.ftype = ftype(token),
                b"gain" => self.gain = float(),
                b"postgain" => self.postgain = float(),
                b"velocity" => self.velocity = float(),
                b"pan" => self.pan = float(),
                b"reset" => self.reset = atoi(token),
                b"attack" => self.attack = float(),
                b"decay" => self.decay = float(),
                b"sustain" => self.sustain = float(),
                b"release" => self.release = float(),
                b"penv" => self.penv = float(),
                b"patt" => self.patt = float(),
                b"pdec" => self.pdec = float(),
                b"psus" => self.psus = float(),
                b"prel" => self.prel = float(),
                b"vib" => self.vib = float(),
                b"vibmod" => self.vibmod = float(),
                b"fm" | b"fmi" => self.fm = float(),
                b"fmh" => self.fmh = float(),
                b"fme" => self.fme = float(),
                b"fma" => self.fma = float(),
                b"fmd" => self.fmd = float(),
                b"fms" => self.fms = float(),
                b"fmr" => self.fmr = float(),
                b"am" => self.am = float(),
                b"amdepth" => self.amdepth = float(),
                b"rm" => self.rm = float(),
                b"rmdepth" => self.rmdepth = float(),
                b"phaser" | b"phaserrate" => self.phaser = float(),
                b"phaserdepth" => self.phaserdepth = float(),
                b"phasersweep" => self.phasersweep = float(),
                b"phasercenter" => self.phasercenter = float(),
                b"flanger" | b"flangerrate" => self.flanger = float(),
                b"flangerdepth" => self.flangerdepth = float(),
                b"flangerfeedback" => self.flangerfeedback = float(),
                b"chorus" | b"chorusrate" => self.chorus = float(),
                b"chorusdepth" => self.chorusdepth = float(),
                b"chorusdelay" => self.chorusdelay = float(),
                b"begin" => self.begin = crate::dsp::clamp(float(), 0.0, 1.0),
                b"end" => self.end = crate::dsp::clamp(float(), 0.0, 1.0),
                b"coarse" => self.coarse = float(),
                b"crush" => self.crush = float(),
                b"distort" => self.distort = float(),
                b"distortvol" => self.distortvol = float(),
                b"delay" => self.delay = float(),
                b"delaytime" => self.delaytime = float(),
                b"delayfeedback" => self.delayfeedback = float(),
                b"verb" | b"reverb" | b"room" => self.verb = float(),
                b"verbdecay" | b"roomdecay" => self.verbdecay = float(),
                b"verbdamp" | b"roomdamp" => self.verbdamp = float(),
                b"verbpredelay" | b"roompredelay" => self.verbpredelay = float(),
                b"verbdiff" | b"roomdiff" => self.verbdiff = float(),
                b"orbit" => self.orbit = atoi(token),
                _ => {}
            }
        }
    }

    /// `merge_event`: copy into `target` every parameter this event sets.
    /// Time and command are left alone: they make no sense for a voice that is
    /// already sounding.
    pub(crate) fn merge_into(&self, target: &mut Event) {
        if self.orbit != -1 {
            target.orbit = self.orbit;
        }
        if self.sound != Source::Unknown {
            target.sound = self.sound;
        }
        if self.ftype != -1 {
            target.ftype = self.ftype;
        }
        macro_rules! merge {
            ($($field:ident),* $(,)?) => {$(
                if !self.$field.is_nan() {
                    target.$field = self.$field;
                }
            )*};
        }
        merge!(
            freq,
            speed,
            glide,
            gain,
            pan,
            gate,
            velocity,
            postgain,
            duration,
            pw,
            attack,
            decay,
            sustain,
            release,
            lpf,
            lpq,
            lpe,
            lpa,
            lpd,
            lps,
            lpr,
            hpf,
            hpq,
            hpe,
            hpa,
            hpd,
            hps,
            hpr,
            bpf,
            bpq,
            bpe,
            bpa,
            bpd,
            bps,
            bpr,
            penv,
            patt,
            pdec,
            psus,
            prel,
            vib,
            vibmod,
            fm,
            fmh,
            fme,
            fma,
            fmd,
            fms,
            fmr,
            am,
            amdepth,
            rm,
            rmdepth,
            phaser,
            phaserdepth,
            phasersweep,
            phasercenter,
            flanger,
            flangerdepth,
            flangerfeedback,
            chorus,
            chorusdepth,
            chorusdelay,
            begin,
            end,
            crush,
            distort,
            distortvol,
            delay,
            delaytime,
            delayfeedback,
            verb,
            verbdecay,
            verbdamp,
            verbpredelay,
            verbdiff,
        );
    }

    /// `init_event`: fill in defaults and work out which stages are active.
    /// `files[i]` is the registered file at index `i`, if any.
    pub(crate) fn init(&mut self, files: &[Option<FileSource>], sr: f32) {
        self.channels = 1;
        self.rate = self.freq;
        // No file is -1, which no index converts from.
        if let Some(slot) = usize::try_from(self.file).ok().and_then(|f| files.get(f)) {
            self.file_source = *slot;
        }
        if let Some(source) = self.file_source {
            self.sound = Source::File;
            nan_fallback(&mut self.begin, 0.0);
            nan_fallback(&mut self.end, 1.0);
            self.channels = source.channels;
            self.rate = if self.rate.is_nan() {
                1.0
            } else {
                self.rate / source.freq
            };
            let slice_frames = ((self.end - self.begin) * source.frames as f32) as i32;
            if slice_frames != 0 {
                self.rate *= sr / slice_frames as f32;
            }
        } else if self.sound == Source::Unknown {
            self.sound = Source::Tri;
        }
        nan_fallback(&mut self.gate, 1.0);
        nan_fallback(&mut self.gain, 1.0);
        nan_fallback(&mut self.velocity, 1.0);
        nan_fallback(&mut self.postgain, 1.0);
        nan_fallback(&mut self.rate, 330.0);
        nan_fallback(&mut self.speed, 1.0);
        nan_fallback(&mut self.pan, 0.5);
        nan_fallback(&mut self.pw, 0.5);
        self.init_vib();
        self.init_fm();
        self.init_am();
        self.init_rm();
        self.init_phaser();
        self.init_flanger();
        self.init_chorus();
        self.init_filters();
        self.init_distort();
        self.init_delay();
        self.init_verb();
        if !self.coarse.is_nan() {
            self.coarse_active = true;
        }
        if !self.crush.is_nan() {
            self.crush_active = true;
        }
        if !self.glide.is_nan() {
            self.glide_active = true;
        }
        if self.ftype == -1 {
            self.ftype = 0;
        }
        self.lp_adsr_active = init_envelope(
            &mut self.lpe,
            &mut self.lpa,
            &mut self.lpd,
            &mut self.lps,
            &mut self.lpr,
        );
        self.hp_adsr_active = init_envelope(
            &mut self.hpe,
            &mut self.hpa,
            &mut self.hpd,
            &mut self.hps,
            &mut self.hpr,
        );
        self.bp_adsr_active = init_envelope(
            &mut self.bpe,
            &mut self.bpa,
            &mut self.bpd,
            &mut self.bps,
            &mut self.bpr,
        );
        self.p_adsr_active = init_envelope(
            &mut self.penv,
            &mut self.patt,
            &mut self.pdec,
            &mut self.psus,
            &mut self.prel,
        );
        init_envelope(
            &mut self.gain,
            &mut self.attack,
            &mut self.decay,
            &mut self.sustain,
            &mut self.release,
        );
        self.fm_adsr_active = init_envelope(
            &mut self.fme,
            &mut self.fma,
            &mut self.fmd,
            &mut self.fms,
            &mut self.fmr,
        );
        // The gain envelope always applies.
        self.gain_adsr_active = true;
    }

    fn init_vib(&mut self) {
        if self.vib.is_nan() && self.vibmod.is_nan() {
            self.vib_active = false;
            return;
        }
        nan_fallback(&mut self.vibmod, 0.15);
        nan_fallback(&mut self.vib, 4.0);
        self.vib_active = true;
    }

    fn init_fm(&mut self) {
        if self.fm.is_nan() && self.fmh.is_nan() {
            return;
        }
        nan_fallback(&mut self.fm, 1.0);
        nan_fallback(&mut self.fmh, 1.0);
        self.fm_active = true;
    }

    fn init_am(&mut self) {
        if self.am.is_nan() && self.amdepth.is_nan() {
            self.am_active = false;
            return;
        }
        self.am_active = true;
        nan_fallback(&mut self.am, 4.0);
        nan_fallback(&mut self.amdepth, 0.5);
    }

    fn init_rm(&mut self) {
        if self.rm.is_nan() && self.rmdepth.is_nan() {
            self.rm_active = false;
            return;
        }
        self.rm_active = true;
        nan_fallback(&mut self.rmdepth, 1.0);
    }

    fn init_phaser(&mut self) {
        if self.phaser.is_nan()
            && self.phaserdepth.is_nan()
            && self.phasersweep.is_nan()
            && self.phasercenter.is_nan()
        {
            self.phaser_active = false;
            return;
        }
        self.phaser_active = true;
        nan_fallback(&mut self.phaser, 1.0);
        nan_fallback(&mut self.phaserdepth, 0.75);
        nan_fallback(&mut self.phasersweep, 2000.0);
        nan_fallback(&mut self.phasercenter, 1000.0);
    }

    fn init_flanger(&mut self) {
        if self.flanger.is_nan() && self.flangerdepth.is_nan() && self.flangerfeedback.is_nan() {
            self.flanger_active = false;
            return;
        }
        self.flanger_active = true;
        nan_fallback(&mut self.flanger, 0.5);
        nan_fallback(&mut self.flangerdepth, 0.5);
        nan_fallback(&mut self.flangerfeedback, 0.5);
    }

    fn init_chorus(&mut self) {
        if self.chorus.is_nan() && self.chorusdepth.is_nan() && self.chorusdelay.is_nan() {
            self.chorus_active = false;
            return;
        }
        self.chorus_active = true;
        nan_fallback(&mut self.chorus, 0.5);
        nan_fallback(&mut self.chorusdepth, 0.5);
        nan_fallback(&mut self.chorusdelay, 25.0);
    }

    /// `init_lp`, `init_hp` and `init_bp`.
    fn init_filters(&mut self) {
        if !(self.lpe.is_nan() && self.lpf.is_nan()) {
            nan_fallback(&mut self.lpf, 20000.0);
            nan_fallback(&mut self.lpq, 0.2);
            self.lp_active = true;
        }
        if !(self.hpe.is_nan() && self.hpf.is_nan()) {
            nan_fallback(&mut self.hpf, 0.0);
            nan_fallback(&mut self.hpq, 0.2);
            self.hp_active = true;
        }
        if !(self.bpe.is_nan() && self.bpf.is_nan()) {
            nan_fallback(&mut self.bpf, 20000.0);
            nan_fallback(&mut self.bpq, 0.2);
            self.bp_active = true;
        }
    }

    fn init_distort(&mut self) {
        if self.distort.is_nan() {
            return;
        }
        nan_fallback(&mut self.distortvol, 1.0);
        self.distort_active = true;
    }

    fn init_delay(&mut self) {
        if self.delay.is_nan() && self.delaytime.is_nan() && self.delayfeedback.is_nan() {
            return;
        }
        nan_fallback(&mut self.delay, 0.5);
        nan_fallback(&mut self.delaytime, 0.333);
        nan_fallback(&mut self.delayfeedback, 0.6);
        self.delay_active = true;
    }

    fn init_verb(&mut self) {
        if self.verb.is_nan()
            && self.verbdecay.is_nan()
            && self.verbdamp.is_nan()
            && self.verbpredelay.is_nan()
            && self.verbdiff.is_nan()
        {
            return;
        }
        nan_fallback(&mut self.verb, 0.5);
        nan_fallback(&mut self.verbdecay, 0.75);
        nan_fallback(&mut self.verbdamp, 0.95);
        nan_fallback(&mut self.verbpredelay, 0.1);
        nan_fallback(&mut self.verbdiff, 0.7);
        self.verb_active = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_split_as_the_webassembly_strtok_does() {
        let split = |m: &str| -> Vec<String> {
            tokens(m.as_bytes())
                .map(|t| String::from_utf8_lossy(t).into_owned())
                .collect()
        };
        assert_eq!(split("dough/play/s/saw"), ["dough", "play", "s", "saw"]);
        // Empty tokens count; a trailing slash adds none.
        assert_eq!(split("a//b/"), ["a", "", "b"]);
        assert_eq!(split("/a"), ["", "a"]);
        assert!(split("").is_empty());
    }

    #[test]
    fn numbers_parse_as_doughs_own_atof_and_atoi_do() {
        assert_eq!(atof(b"0.03"), 0.0 + 0.1 * 0.0 + 0.1 * 0.1 * 3.0);
        assert_eq!(atof(b"-2.5"), -2.5);
        assert_eq!(atof(b"1e3"), 1000.0);
        assert_eq!(atof(b"12x"), 12.0);
        assert_eq!(atoi(b"42"), 42);
        assert_eq!(atoi(b"-7"), -7);
        // Unlike the C library's, any stray character makes it 0.
        assert_eq!(atoi(b"1.5"), 0);
    }
}
