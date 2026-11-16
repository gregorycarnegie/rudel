// engine.rs - dough's engine: voices, orbits, the event schedule and the
// per-sample mix. Ported from dough.h; where the C does something odd that
// shapes the output, the port does it too, and says so.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::dsp::{
    Adsr, AdsrState, Biquad, BrownNoise, Chorus, Coarse, Delay, FPI, FileSource, FilterType,
    Flanger, Lag, Phaser, PinkNoise, Rates, Rng, Verb, clamp, crush, distort, exp2f, ftz, par_cosf,
    par_sinf,
};
use crate::event::{Command, Event, Source};

/// Channels per voice and per orbit.
const CHANNELS: usize = 2;
/// Orbits (`max_orbits`): dough.js lays out one.
const MAX_ORBITS: usize = 1;
/// `GAIN_SINE` and the other source gains, a `double` in dough.
const SOURCE_GAIN: f64 = 0.2;

/// What dough.js hands `dough_init` in the browser.
pub(crate) struct Limits {
    pub(crate) voices: usize,
    pub(crate) delay_seconds: usize,
    pub(crate) events: usize,
    pub(crate) files: usize,
    /// Sample memory, in floats.
    pub(crate) pcm: usize,
}

/// One channel's insert effects (`ChannelFX`).
#[derive(Clone, Copy, Default)]
struct ChannelFx {
    lp: [Biquad; 4],
    hp: [Biquad; 4],
    bp: [Biquad; 4],
    coarse: Coarse,
    phaser: Phaser,
    flanger: Flanger,
}

/// A voice: its event, its oscillator and envelope state, its effects.
#[derive(Clone, Copy)]
struct Voice {
    p: Event,
    time: f32,
    ch: [f32; CHANNELS],
    phasor: f32,
    glide_lag: Lag,
    gain_adsr: Adsr,
    lp_adsr: Adsr,
    hp_adsr: Adsr,
    bp_adsr: Adsr,
    fm_adsr: Adsr,
    p_adsr: Adsr,
    vib_lfo: f32,
    fm_modulator: f32,
    am_lfo: f32,
    rm_lfo: f32,
    noise_pink: PinkNoise,
    noise_brown: BrownNoise,
    id: u64,
    fx: [ChannelFx; CHANNELS],
    chorus: Chorus,
}

impl Default for Voice {
    fn default() -> Voice {
        Voice {
            p: Event::default(),
            time: 0.0,
            ch: [0.0; CHANNELS],
            phasor: 0.0,
            glide_lag: Lag::default(),
            gain_adsr: Adsr::default(),
            lp_adsr: Adsr::default(),
            hp_adsr: Adsr::default(),
            bp_adsr: Adsr::default(),
            fm_adsr: Adsr::default(),
            p_adsr: Adsr::default(),
            vib_lfo: 0.0,
            fm_modulator: 0.0,
            am_lfo: 0.0,
            rm_lfo: 0.0,
            noise_pink: PinkNoise::default(),
            noise_brown: BrownNoise::default(),
            id: 0,
            fx: Default::default(),
            chorus: Chorus::default(),
        }
    }
}

impl Voice {
    /// `reset_voice`: everything a fresh note starts from. The event, id and
    /// output are written by whoever plays it.
    fn reset(&mut self) {
        for fx in &mut self.fx {
            fx.phaser = Phaser::default();
            fx.flanger = Flanger::default();
            fx.coarse = Coarse::default();
            fx.lp = Default::default();
            fx.hp = Default::default();
            fx.bp = Default::default();
        }
        self.gain_adsr = Adsr::default();
        self.lp_adsr = Adsr::default();
        self.hp_adsr = Adsr::default();
        self.bp_adsr = Adsr::default();
        self.fm_adsr = Adsr::default();
        self.p_adsr = Adsr::default();
        self.phasor = 0.0;
        self.noise_pink = PinkNoise::default();
        self.noise_brown = BrownNoise::default();
        self.vib_lfo = 0.0;
        self.fm_modulator = 0.0;
        self.am_lfo = 0.0;
        self.rm_lfo = 0.0;
        self.chorus = Chorus::default();
        self.glide_lag = Lag::default();
        self.time = 0.0;
    }

    /// `shift_envelopes`.
    fn shift_envelopes(&mut self, offset: f32) {
        for env in [
            &mut self.gain_adsr,
            &mut self.lp_adsr,
            &mut self.hp_adsr,
            &mut self.bp_adsr,
            &mut self.fm_adsr,
            &mut self.p_adsr,
        ] {
            env.start_time += offset;
        }
    }

    /// `run_source`: the source's next output, in `ch`'s place. `None` when a
    /// sample ran out, having still written what it read to `ch`, as dough
    /// does: the mix reads it from there.
    fn run_source(
        &mut self,
        freq: f32,
        rates: Rates,
        pcm: &[Box<[f32]>],
        rng: &mut Rng,
    ) -> Option<[f32; 2]> {
        let scaled = |s: f32| (s as f64 * SOURCE_GAIN) as f32;
        let isr = rates.isr;
        let left = match self.p.sound {
            Source::Tri => scaled(crate::dsp::tri(&mut self.phasor, freq, isr)),
            Source::Sine => scaled(crate::dsp::sine(&mut self.phasor, freq, isr)),
            Source::Zaw => scaled(crate::dsp::zaw(&mut self.phasor, freq, isr)),
            Source::Saw => scaled(crate::dsp::saw(&mut self.phasor, freq, isr)),
            Source::Pulse => scaled(crate::dsp::pulse(&mut self.phasor, freq, self.p.pw, isr)),
            Source::Pulze => scaled(crate::dsp::pulze(&mut self.phasor, freq, self.p.pw, isr)),
            Source::White => scaled(rng.float() * 2.0 - 1.0),
            Source::Pink => scaled(self.noise_pink.update(rng)),
            Source::Brown => scaled(self.noise_brown.update(rng)),
            Source::Const => 1.0,
            Source::File => {
                let Some(source) = self.p.file_source else {
                    return Some(self.ch);
                };
                let data = pcm.get(source.index).map_or(&[][..], |d| &d[..]);
                let mut out = self.ch;
                let wrapped = crate::dsp::file(
                    &mut self.phasor,
                    freq,
                    isr,
                    &source,
                    data,
                    (self.p.begin, self.p.end),
                    &mut out,
                );
                if wrapped {
                    self.ch = out;
                    return None;
                }
                return Some(out);
            }
            // Leaves `ch` as it was.
            Source::Unknown => return Some(self.ch),
        };
        Some([left, self.ch[1]])
    }

    /// `run_voice`: one sample into `ch`, returned as well, so the mix need
    /// not read back what was just stored. `None` when the voice is over and
    /// is to be freed (its envelope ended, or its sample ran out).
    fn run(&mut self, rates: Rates, pcm: &[Box<[f32]>], rng: &mut Rng) -> Option<[f32; 2]> {
        let p = &self.p;
        let mut gain_envelope = 0.0;
        if p.gain_adsr_active {
            gain_envelope = self
                .gain_adsr
                .update(self.time, p.gate, p.attack, p.decay, p.sustain, p.release);
        }
        if self.gain_adsr.state == AdsrState::Off {
            return None;
        }

        let mut freq = p.rate;
        freq *= p.speed;
        if p.glide_active {
            freq = self.glide_lag.update(freq, p.glide, rates.lag_unit);
        }
        if p.fm_active {
            // `apply_fm`
            let mut fm = p.fm;
            if p.fm_adsr_active {
                let env = self
                    .fm_adsr
                    .update(self.time, p.gate, p.fma, p.fmd, p.fms, p.fmr);
                fm = p.fme * env * fm + fm;
            }
            let modfreq = freq * p.fmh;
            let modgain = modfreq * fm;
            let modulator = crate::dsp::sine(&mut self.fm_modulator, modfreq, rates.isr);
            freq += modulator * modgain;
        }
        if p.p_adsr_active {
            // `apply_penv`: never enters its release phase.
            let mut env = self
                .p_adsr
                .update(self.time, 1.0, p.patt, p.pdec, p.psus, p.prel);
            if p.psus == 1.0 {
                env -= 1.0;
            }
            freq *= exp2f((env * p.penv) / 12.0);
        }
        if p.vib_active {
            // `apply_vib`
            let modulator = crate::dsp::sine(&mut self.vib_lfo, p.vib, rates.isr);
            freq *= exp2f(modulator * p.vibmod / 12.0);
        }

        let mut ch = self.run_source(freq, rates, pcm, rng)?;

        let p = &self.p;
        let mut lp_cutoff = p.lpf;
        if p.lp_active && p.lp_adsr_active {
            let env = self
                .lp_adsr
                .update(self.time, p.gate, p.lpa, p.lpd, p.lps, p.lpr);
            lp_cutoff = clamp(p.lpe * env * lp_cutoff + lp_cutoff, 0.0, 20000.0);
        }
        let mut hp_cutoff = p.hpf;
        if p.hp_active && p.hp_adsr_active {
            let env = self
                .hp_adsr
                .update(self.time, p.gate, p.hpa, p.hpd, p.hps, p.hpr);
            hp_cutoff = clamp(p.hpe * env * hp_cutoff + hp_cutoff, 0.0, 20000.0);
        }
        let mut bp_cutoff = p.bpf;
        if p.bp_active && p.bp_adsr_active {
            let env = self
                .bp_adsr
                .update(self.time, p.gate, p.bpa, p.bpd, p.bps, p.bpr);
            bp_cutoff = clamp(p.bpe * env * bp_cutoff + bp_cutoff, 0.0, 20000.0);
        }

        let mut am_gain = 1.0_f32;
        if p.am_active {
            let modulator = crate::dsp::sine(&mut self.am_lfo, p.am, rates.isr);
            let depth = clamp(p.amdepth, 0.0, 1.0);
            am_gain = 1.0 + modulator * depth;
        }
        let mut rm_gain = 1.0_f32;
        if p.rm_active {
            let modulator = crate::dsp::sine(&mut self.rm_lfo, p.rm, rates.isr);
            let depth = clamp(p.rmdepth, 0.0, 1.0);
            rm_gain = ((1.0 - depth as f64) + (modulator * depth) as f64) as f32;
        }

        let stages = match p.ftype {
            0 => 1,
            1 => 2,
            _ => 4,
        };
        // A file with more channels than dough mixes would overrun its arrays
        // in C; the rest are ignored here.
        let channels = (p.channels.max(1) as usize).min(CHANNELS);
        for (sample, fx) in ch.iter_mut().zip(&mut self.fx).take(channels) {
            *sample *= p.gain * p.velocity;
            if p.lp_active {
                for bq in &mut fx.lp[..stages] {
                    *sample = bq.update(*sample, FilterType::Lowpass, lp_cutoff, p.lpq, rates);
                }
            }
            if p.hp_active {
                for bq in &mut fx.hp[..stages] {
                    *sample = bq.update(*sample, FilterType::Highpass, hp_cutoff, p.hpq, rates);
                }
            }
            if p.bp_active {
                for bq in &mut fx.bp[..stages] {
                    *sample = bq.update(*sample, FilterType::Bandpass, bp_cutoff, p.bpq, rates);
                }
            }
            if p.coarse_active {
                *sample = fx.coarse.update(*sample, p.coarse as i32);
            }
            if p.crush_active {
                *sample = crush(*sample, p.crush);
            }
            if p.distort_active {
                *sample = distort(*sample, p.distort, p.distortvol);
            }
            if p.am_active {
                *sample *= am_gain;
            }
            if p.rm_active {
                *sample *= rm_gain;
            }
            if p.phaser_active {
                *sample = fx.phaser.update(
                    *sample,
                    p.phaser,
                    p.phaserdepth,
                    p.phasercenter,
                    p.phasersweep,
                    rates,
                );
            }
            if p.flanger_active {
                *sample =
                    fx.flanger
                        .update(*sample, p.flanger, p.flangerdepth, p.flangerfeedback, rates);
            }
            *sample *= gain_envelope * p.postgain;
        }
        // `ch[c % channels]` for the rest, with `channels` 1 or 2 here: a
        // mono voice copies its left channel to the right.
        if channels == 1 {
            ch[1] = ch[0];
        }

        if p.chorus_active {
            let [left, right] = &mut ch;
            self.chorus
                .update(left, right, p.chorus, p.chorusdepth, p.chorusdelay, rates);
        }
        if p.pan != 0.5 {
            let panpos = (p.pan * FPI) / 2.0;
            ch[0] *= par_cosf(panpos);
            ch[1] *= par_sinf(panpos);
        }
        self.ch = ch;
        self.time += rates.time_step;
        // Release once the duration is over; a duration of 0 is endless.
        if !p.duration.is_nan() && p.duration != 0.0 && self.time > p.duration {
            self.p.gate = 0.0;
        }
        Some(ch)
    }
}

/// An orbit: the shared delay and reverb voices send to.
struct Orbit {
    delay: [Delay; CHANNELS],
    delaysend: [f32; CHANNELS],
    delaytime: f32,
    delayfeedback: f32,
    verb: Verb,
    verbsend: [f32; CHANNELS],
    verbdecay: f32,
    verbdamp: f32,
    verbpredelay: f32,
    verbdiff: f32,
}

impl Orbit {
    fn new(delay_samples: usize) -> Orbit {
        Orbit {
            delay: [Delay::new(delay_samples), Delay::new(delay_samples)],
            delaysend: [0.0; CHANNELS],
            delaytime: 0.0,
            delayfeedback: 0.0,
            verb: Verb::new(),
            verbsend: [0.0; CHANNELS],
            verbdecay: 0.75,
            verbdamp: 0.95,
            verbpredelay: 0.1,
            verbdiff: 0.7,
        }
    }
}

/// The dough engine.
pub(crate) struct Engine {
    rates: Rates,
    /// Every voice slot; the first `active` are sounding. One contiguous
    /// block, as dough lays them out: voices boxed one by one land at the same
    /// offset within a page, and their hot fields then fight over the same
    /// cache sets.
    voices: Vec<Voice>,
    active: usize,
    voice_counter: u64,
    orbits: [Orbit; MAX_ORBITS],
    schedule: Vec<Event>,
    max_events: usize,
    /// Registered files, by index, and their PCM alongside.
    files: Vec<Option<FileSource>>,
    pcm: Vec<Box<[f32]>>,
    pcm_used: usize,
    max_pcm: usize,
    rng: Rng,
    /// Frames rendered since the clock was last reset (dough.js's `tick`).
    tick: i64,
}

impl Engine {
    /// `dough_engine_init`.
    pub(crate) fn new(sample_rate: u32, limits: &Limits) -> Engine {
        let sr = sample_rate as f32;
        let rates = Rates {
            sr,
            isr: (1.0 / sr as f64) as f32,
            lag_unit: (sample_rate / 10) as i32,
            time_step: 1.0 / sr,
        };
        let delay_samples = (limits.delay_seconds * sample_rate as usize).max(1);
        Engine {
            rates,
            voices: vec![Voice::default(); limits.voices],
            active: 0,
            voice_counter: 0,
            orbits: std::array::from_fn(|_| Orbit::new(delay_samples)),
            schedule: Vec::with_capacity(limits.events),
            max_events: limits.events,
            files: vec![None; limits.files],
            pcm: (0..limits.files).map(|_| Box::default()).collect(),
            pcm_used: 0,
            max_pcm: limits.pcm,
            rng: Rng::default(),
            tick: 0,
        }
    }

    /// `dough_register_pcm`: copy interleaved PCM in as file `index`. False
    /// when the index is out of range or the sample memory is full.
    pub(crate) fn register_pcm(
        &mut self,
        index: usize,
        pcm: &[f32],
        channels: usize,
        freq: f32,
    ) -> bool {
        if index >= self.files.len() || self.pcm_used + pcm.len() > self.max_pcm || channels == 0 {
            return false;
        }
        self.pcm_used += pcm.len();
        self.pcm[index] = pcm.into();
        self.files[index] = Some(FileSource {
            index,
            channels: channels as i32,
            frames: (pcm.len() / channels) as i32,
            freq,
        });
        true
    }

    /// `dough_evaluate`: parse a message and act on it, now or at its `time`.
    pub(crate) fn evaluate(&mut self, message: &[u8]) {
        let mut event = Event::default();
        event.parse(message);
        if event.time.is_nan() {
            self.run_event(&event);
        } else if self.schedule.len() < self.max_events {
            self.schedule.push(event);
        }
    }

    /// Render `out.len()` frames, as `dsp()` does a block, adding them to
    /// `out`.
    pub(crate) fn render_add(&mut self, out: &mut [(f32, f32)]) {
        let sr = self.rates.sr as f64;
        let start = self.tick as f64 / sr;
        for (i, frame) in out.iter_mut().enumerate() {
            self.schedule_update(start + i as f64 / sr);
            let (left, right) = self.gen_sample();
            frame.0 += left;
            frame.1 += right;
        }
        self.tick += out.len() as i64;
    }

    /// `schedule_update`: run what is due, newest slot first, as dough does.
    fn schedule_update(&mut self, time: f64) {
        let mut i = self.schedule.len();
        while i > 0 {
            i -= 1;
            // A reset among the due events empties the schedule under us.
            let Some(&event) = self.schedule.get(i) else {
                continue;
            };
            if event.time > time {
                continue;
            }
            // Only play an event that is recently due (under 10 ms late).
            if time - event.time < 0.01 {
                self.run_event(&event);
            }
            if i >= self.schedule.len() {
                continue;
            }
            if event.repeat.is_nan() || event.repeat <= 0.0 {
                self.schedule.swap_remove(i);
            } else {
                let diff = time - event.time;
                let skip = ((diff / event.repeat as f64).floor() + 1.0) as i32;
                self.schedule[i].time += skip as f64 * event.repeat as f64;
            }
        }
    }

    /// `run_event`.
    fn run_event(&mut self, event: &Event) {
        match event.cmd {
            Command::Play => self.play(event),
            Command::UpdateAll => {
                for voice in &mut self.voices {
                    event.merge_into(&mut voice.p);
                    voice.p.init(&self.files, self.rates.sr);
                }
            }
            Command::UpdateGroup => {
                for voice in &mut self.voices {
                    if event.group != -1 && voice.p.group == event.group {
                        event.merge_into(&mut voice.p);
                        voice.p.init(&self.files, self.rates.sr);
                    }
                }
            }
            Command::Panic => self.active = 0,
            Command::Hush => self.release_where(|_| true),
            Command::HushEndless => self.release_where(|v| v.p.duration.is_nan()),
            Command::HushGroup => {
                let group = event.group;
                self.release_where(|v| v.p.group != -1 && v.p.group == group);
            }
            Command::Reset => {
                self.active = 0;
                self.tick = 0;
                self.schedule.clear();
            }
            Command::ResetTime => self.tick = 0,
            Command::ResetSchedule => self.schedule.clear(),
            Command::Unknown => {}
        }
    }

    /// Close the gate of every voice slot `which` picks, sounding or not, as
    /// `hush` and friends do.
    fn release_where(&mut self, which: impl Fn(&Voice) -> bool) {
        for voice in &mut self.voices {
            if which(voice) {
                voice.p.gate = 0.0;
            }
        }
    }

    /// `play_event`: start a voice, or retrigger the oldest in a full group.
    fn play(&mut self, event: &Event) {
        let mut stolen = false;
        let slot = if event.group != -1 && event.voices != -1 {
            let (count, oldest) = self.group_state(event.group);
            stolen = count >= event.voices;
            if stolen { oldest } else { self.allocate() }
        } else {
            self.allocate()
        };
        let Some(i) = slot else {
            return;
        };
        let fresh = !stolen || event.reset == 1;
        let id = self.voice_counter;
        self.voice_counter += 1;
        let voice = &mut self.voices[i];
        if fresh {
            voice.reset();
        }
        voice.p = *event;
        voice.id = id;
        // Retriggering a voice keeps its envelopes where they were.
        voice.shift_envelopes(-voice.time);
        voice.time = 0.0;
        voice.p.init(&self.files, self.rates.sr);
        if fresh {
            // No glide from 0, and no early wrap.
            voice.glide_lag.s = voice.p.rate;
            voice.phasor = if voice.p.speed < 0.0 { 1.0 } else { 0.0 };
        }
    }

    /// `allocate_voice`: the next free slot, if any.
    fn allocate(&mut self) -> Option<usize> {
        (self.active < self.voices.len()).then(|| {
            self.active += 1;
            self.active - 1
        })
    }

    /// `get_group_state`: how many sounding voices are in `group`, and the
    /// oldest of them.
    fn group_state(&self, group: i32) -> (i32, Option<usize>) {
        let mut count = 0;
        let mut oldest: Option<(usize, u64)> = None;
        for (i, voice) in self.voices[..self.active].iter().enumerate() {
            if voice.p.group == group {
                count += 1;
                if oldest.is_none_or(|(_, id)| voice.id < id) {
                    oldest = Some((i, voice.id));
                }
            }
        }
        (count, oldest.map(|(i, _)| i))
    }

    /// `gen_sample`: one stereo frame.
    fn gen_sample(&mut self) -> (f32, f32) {
        let rates = self.rates;
        let mut out = [0.0_f32; CHANNELS];
        let mut v = 0;
        while v < self.active {
            let output = self.voices[v].run(rates, &self.pcm, &mut self.rng);
            if output.is_none() {
                // `free_voice`: the last voice is copied into this slot, and
                // stays where it was too. Not a swap: a note whose gate closed
                // before its first sample ends there without writing its
                // output, and is mixed with whatever its slot last held, which
                // is that copy in dough.
                self.active -= 1;
                if v != self.active {
                    let (head, tail) = self.voices.split_at_mut(self.active);
                    head[v] = tail[0];
                }
            }
            // As in dough, what is mixed is whatever voice now holds the slot:
            // after a free that is the voice moved in, with last sample's
            // output, and it is not run until the next sample.
            let voice = &self.voices[v];
            let ch = output.unwrap_or(voice.ch);
            out[0] += ch[0];
            out[1] += ch[1];
            if !(voice.p.delay_active || voice.p.verb_active) {
                v += 1;
                continue;
            }
            let orbit = &mut self.orbits[voice.p.orbit.rem_euclid(MAX_ORBITS as i32) as usize];
            for c in 0..CHANNELS {
                if voice.p.delay_active {
                    orbit.delaytime = voice.p.delaytime;
                    orbit.delayfeedback = voice.p.delayfeedback;
                    orbit.delaysend[c] += ch[c] * voice.p.delay;
                }
                if voice.p.verb_active {
                    orbit.verbdecay = voice.p.verbdecay;
                    orbit.verbdamp = voice.p.verbdamp;
                    orbit.verbpredelay = voice.p.verbpredelay;
                    orbit.verbdiff = voice.p.verbdiff;
                    orbit.verbsend[c] += ch[c] * voice.p.verb;
                }
            }
            v += 1;
        }
        for (c, sample) in out.iter_mut().enumerate() {
            for orbit in &mut self.orbits {
                orbit.delaysend[c] = ftz(orbit.delaysend[c], 0.0001);
                let delayed = orbit.delay[c].update(orbit.delaysend[c], orbit.delaytime, rates.sr);
                orbit.delaysend[c] = delayed * orbit.delayfeedback;
                *sample += delayed;
            }
        }
        for orbit in &mut self.orbits {
            let mono = ftz((orbit.verbsend[0] + orbit.verbsend[1]) * 0.5, 0.0001);
            if mono != 0.0 {
                orbit.verb.set_decay(orbit.verbdecay);
                orbit.verb.set_damping(orbit.verbdamp);
                orbit.verb.set_pre_delay(orbit.verbpredelay);
                orbit.verb.set_input_diffusion(orbit.verbdiff);
            }
            orbit.verb.process(mono);
            out[0] += orbit.verb.left();
            out[1] += orbit.verb.right();
            orbit.verbsend = [0.0; CHANNELS];
        }
        let mix = |s: f32| clamp((s as f64 * 0.5) as f32, -1.0, 1.0);
        (mix(out[0]), mix(out[1]))
    }
}
