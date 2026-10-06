//! An `FX(...)` stage's own `delay` and `room`.
//!
//! superdough sends the main chain's delay and reverb to the orbit, but builds a
//! stage's inline, in the voice's own chain (superdough.mjs, the `key !==
//! 'main'` branches): a fresh feedback delay, `dry·x + delay·delayed(x)`, and
//! then a fresh convolver, `dry·x + room·reverb(x)`. They go when the chain is
//! released at the end of the note, so a stage's tail stops with it.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    bus::{DelayConfig, ReverbConfig, StereoDelay, build_reverb},
    convolver::Convolver,
    modulator::{ModBank, ModSpec, ModTarget},
    voice::VoiceLike,
};
use rudel_core::ValueMap;

/// A stage's delay and reverb, as its control map asks for them.
#[derive(Clone, Default)]
pub struct StageSends {
    /// `dry`: the level of the signal each effect passes on beside its wet
    /// output.
    pub dry: f32,
    /// `delay` (not through the gain curve, unlike the main chain's) and its
    /// line, when `delay`, `delaytime` and `delayfeedback` are all positive.
    pub delay: Option<(f32, DelayConfig)>,
    /// `room`, when positive, and its reverb.
    pub room: Option<StageRoom>,
}

/// A stage's reverb, built ahead of the audio thread.
pub struct StageRoom {
    pub wet: f32,
    pub cfg: ReverbConfig,
    /// The convolver and the sample rate it was built for. A 2 s room is
    /// megabytes of spectra, so [`StageSends::prepare`] builds it where the
    /// event is made rather than where the voice starts.
    built: Option<(f32, Box<Convolver>)>,
}

/// A clone builds its own convolver again.
impl Clone for StageRoom {
    fn clone(&self) -> StageRoom {
        StageRoom {
            wet: self.wet,
            cfg: self.cfg.clone(),
            built: None,
        }
    }
}

impl StageSends {
    /// Read a stage's map, with `cps` turning `delaysync` into seconds.
    pub fn from_controls(map: &ValueMap, cps: f64) -> StageSends {
        let get = |k: &str| map.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
        let reverb = ReverbConfig::default();
        let time = get("delaytime")
            .unwrap_or((f64::from(get("delaysync").unwrap_or(3.0 / 16.0)) / cps.max(1e-9)) as f32);
        let feedback = get("delayfeedback").unwrap_or(0.5);
        let delay = get("delay").unwrap_or(0.0);
        let room = get("room").unwrap_or(0.0);
        StageSends {
            dry: get("dry").unwrap_or(1.0),
            delay: (delay > 0.0 && time > 0.0 && feedback > 0.0).then_some((
                delay,
                DelayConfig {
                    time,
                    feedback: feedback.clamp(0.0, 0.98),
                },
            )),
            room: (room > 0.0).then(|| StageRoom {
                wet: room,
                cfg: ReverbConfig {
                    size: get("size").unwrap_or(reverb.size),
                    fade: get("roomfade").unwrap_or(reverb.fade),
                    lp: get("roomlp").unwrap_or(reverb.lp),
                    dim: get("roomdim").unwrap_or(reverb.dim),
                    ir: None,
                    irspeed: get("irspeed").unwrap_or(reverb.irspeed),
                    irbegin: get("irbegin").unwrap_or(reverb.irbegin),
                },
                built: None,
            }),
        }
    }

    pub fn is_active(&self) -> bool {
        self.delay.is_some() || self.room.is_some()
    }

    /// Build the reverb for `sample_rate` now, off the audio thread.
    pub fn prepare(&mut self, sample_rate: f32) {
        if let Some(room) = &mut self.room {
            room.built = Some((sample_rate, Box::new(build_reverb(sample_rate, &room.cfg))));
        }
    }
}

/// A stage's delay and reverb around the voice before them.
pub struct StageSendsVoice {
    inner: Box<dyn VoiceLike>,
    dry: f32,
    delay: Option<(f32, StereoDelay)>,
    room: Option<(f32, Box<Convolver>)>,
    mods: ModBank,
}

impl StageSendsVoice {
    pub fn new(
        inner: Box<dyn VoiceLike>,
        sends: StageSends,
        sample_rate: f32,
        mods: &[ModSpec],
    ) -> StageSendsVoice {
        StageSendsVoice {
            inner,
            dry: sends.dry,
            delay: sends
                .delay
                .map(|(wet, cfg)| (wet, StereoDelay::new(sample_rate, cfg))),
            room: sends.room.map(|room| {
                let conv = match room.built {
                    Some((rate, conv)) if rate == sample_rate => conv,
                    // Only a test renders at another rate than it collected at.
                    _ => Box::new(build_reverb(sample_rate, &room.cfg)),
                };
                (room.wet, conv)
            }),
            mods: ModBank::new(mods, sample_rate as f64),
        }
    }
}

impl VoiceLike for StageSendsVoice {
    fn tick(&mut self) -> (f32, f32) {
        self.mods.tick();
        let (mut l, mut r) = self.inner.tick();
        if let Some((wet, line)) = &mut self.delay {
            let (dl, dr) = line.process_with(
                l,
                r,
                self.mods.get(ModTarget::DelayTime),
                self.mods.get(ModTarget::DelayFeedback),
            );
            let wet = *wet + self.mods.get(ModTarget::DelaySend);
            (l, r) = (self.dry * l + wet * dl, self.dry * r + wet * dr);
        }
        if let Some((wet, conv)) = &mut self.room {
            let (rl, rr) = conv.process(l, r);
            let wet = *wet + self.mods.get(ModTarget::RoomSend);
            (l, r) = (self.dry * l + wet * rl, self.dry * r + wet * rr);
        }
        (l, r)
    }

    fn set_bus_input(&mut self, bus: i32, left: &[f32], right: &[f32]) {
        self.mods.set_bus_input(bus, left, right);
        self.inner.set_bus_input(bus, left, right);
    }

    fn is_done(&self) -> bool {
        self.inner.is_done()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample of 1, then silence.
    struct Impulse(bool);

    impl VoiceLike for Impulse {
        fn tick(&mut self) -> (f32, f32) {
            let x = if self.0 { 0.0 } else { 1.0 };
            self.0 = true;
            (x, x)
        }

        fn is_done(&self) -> bool {
            false
        }
    }

    fn render(map: &[(&str, f64)], frames: usize) -> Vec<f32> {
        let map: ValueMap = map
            .iter()
            .map(|(k, v)| (k.to_string(), rudel_core::Value::F64(*v)))
            .collect();
        let sends = StageSends::from_controls(&map, 1.0);
        let mut voice = StageSendsVoice::new(Box::new(Impulse(false)), sends, 1000.0, &[]);
        (0..frames).map(|_| voice.tick().0).collect()
    }

    #[test]
    fn the_delay_is_inline_dry_plus_wet_echoes() {
        // 10 ms at 1 kHz is 10 samples; each echo is the last one times the
        // feedback, and the wet level scales them all.
        let out = render(&[("delay", 0.5), ("delaytime", 0.01), ("delayfeedback", 0.5)], 31);
        assert_eq!(out[0], 1.0, "the dry signal passes at `dry` (1)");
        assert_eq!((out[10], out[20], out[30]), (0.5, 0.25, 0.125));
        assert_eq!(out[1..10].iter().sum::<f32>(), 0.0);
        let dry0 = render(&[("delay", 0.5), ("delaytime", 0.01), ("dry", 0.0)], 11);
        assert_eq!((dry0[0], dry0[10]), (0.0, 0.5), "`dry` scales only the dry path");
    }

    #[test]
    fn no_delay_without_all_three_positive_and_no_room_at_zero() {
        let none = |map: &[(&str, f64)]| !StageSends::from_controls(
            &map.iter().map(|(k, v)| (k.to_string(), rudel_core::Value::F64(*v))).collect(),
            1.0,
        )
        .is_active();
        assert!(none(&[]));
        assert!(none(&[("delay", 0.5), ("delayfeedback", 0.0)]));
        assert!(none(&[("delay", 0.5), ("delaytime", 0.0)]));
        assert!(none(&[("room", 0.0)]));
    }

    #[test]
    fn the_room_adds_a_tail_to_the_dry_signal() {
        // The convolver returns one 1024-sample partition late.
        let out = render(&[("room", 1.0), ("size", 0.1)], 3000);
        assert_eq!(out[0], 1.0);
        assert!(out[1..].iter().any(|x| x.abs() > 1e-4), "a reverb tail follows");
    }
}
