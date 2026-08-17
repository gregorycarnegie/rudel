//! The orbit bus: the sidechain duck envelope, the send routing read off
//! controls, and the `s("bus")` source voice.
//!
//! None of this had a test. The duck envelope is the bulk of it and is a pure
//! deterministic state machine, so it can be pinned exactly rather than
//! approximately: superdough schedules a pair of `exponentialRampToValueAtTime`s
//! and an exponential ramp is geometric, so each segment is a constant
//! per-sample multiplier with known endpoints.

use super::common::*;
use rudel_core::Value;

const SR: f32 = 44100.0;

fn duck(onset: f32, attack: f32, depth: f32) -> Duck {
    Duck {
        orbit: 1,
        onset,
        attack,
        depth,
    }
}

// --- the duck envelope ------------------------------------------------------

#[test]
fn an_untriggered_duck_sits_at_unity_forever() {
    let mut env = DuckEnv::default();
    assert!(env.is_idle(), "a fresh envelope is idle");
    for _ in 0..1024 {
        assert_eq!(env.next_gain(), 1.0, "an idle envelope applies no gain");
    }
    assert!(env.is_idle(), "and stays idle");
}

#[test]
fn the_duck_floor_is_one_minus_the_square_root_of_the_depth() {
    // superdough: `clamp(1 - sqrt(depth), 0.01, currVal)`. Depth is *not* the
    // gain reduction directly — the square root is what makes small depths
    // audible.
    // `next_gain` advances *then* reports, so with a zero onset the first
    // sample is already one step into the recovery rather than the floor
    // itself. A long recovery makes that step negligible.
    let floor_for = |depth: f32| {
        let mut env = DuckEnv::default();
        env.trigger(SR, &duck(0.0, 30.0, depth));
        env.next_gain()
    };

    for depth in [0.0f32, 0.25, 0.5, 0.75] {
        let want = (1.0 - depth.sqrt()).max(0.01);
        let got = floor_for(depth);
        assert!(
            (got - want).abs() < 1e-4,
            "depth {depth} should floor at {want:.4}, got {got:.4}"
        );
    }
    // Depth 0.25 gives 1 - 0.5 = 0.5, not 0.75: the square root matters.
    assert!((floor_for(0.25) - 0.5).abs() < 1e-4);
    // Full depth would reach zero, where an exponential ramp is undefined, so
    // it clamps to the 0.01 floor rather than silencing the orbit.
    assert!((floor_for(1.0) - 0.01).abs() < 1e-4);
    // A negative depth is treated as none.
    assert_eq!(floor_for(-1.0), 1.0);
}

#[test]
fn the_duck_dips_then_recovers_over_its_two_segments() {
    // 10ms dip, 20ms recovery, depth 0.75 -> floor 1 - sqrt(0.75) = 0.1340.
    let (onset, attack, depth) = (0.01f32, 0.02f32, 0.75f32);
    let floor = 1.0 - depth.sqrt();
    let dip_samples = (SR * onset) as usize;
    let rise_samples = (SR * attack) as usize;

    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(onset, attack, depth));
    assert!(!env.is_idle(), "a triggered envelope is not idle");

    let gains: Vec<f32> = (0..dip_samples + rise_samples)
        .map(|_| env.next_gain())
        .collect();

    // The dip descends monotonically and lands exactly on the floor.
    assert!(
        gains[..dip_samples].windows(2).all(|w| w[1] <= w[0]),
        "the dip should descend"
    );
    assert!(
        (gains[dip_samples - 1] - floor).abs() < 1e-4,
        "the dip should end on the floor {floor:.4}, got {:.4}",
        gains[dip_samples - 1]
    );
    // ...and it is geometric, not linear: the halfway gain is the geometric
    // mean of the endpoints (sqrt of the floor), well above the arithmetic one.
    let mid = gains[dip_samples / 2 - 1];
    assert!(
        (mid - floor.sqrt()).abs() < 0.02,
        "the dip should be exponential: halfway should be {:.4}, got {mid:.4}",
        floor.sqrt()
    );

    // The recovery climbs monotonically back to exactly unity.
    let rise = &gains[dip_samples..];
    assert!(
        rise.windows(2).all(|w| w[1] >= w[0]),
        "the recovery should climb"
    );
    assert_eq!(
        *rise.last().unwrap(),
        1.0,
        "the recovery should land exactly on unity"
    );
    assert!(env.is_idle(), "and the envelope is idle again after it");
}

#[test]
fn a_zero_onset_drops_immediately() {
    // Upstream's default is a clicky instant drop; only the recovery is ramped.
    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(0.0, 30.0, 0.75));
    let first = env.next_gain();
    let floor = 1.0 - 0.75f32.sqrt();
    // Within one recovery step of the floor — see the note above about
    // `next_gain` advancing before it reports.
    assert!(
        (first - floor).abs() < 1e-3,
        "a zero onset should be at the floor on the first sample ({floor:.4}), got {first:.4}"
    );
    assert!(first < 0.2, "and it should actually have dropped");
}

#[test]
fn the_recovery_is_always_at_least_one_sample() {
    // `attack` floors at 2ms and the sample count floors at 1, so even a zero
    // attack cannot produce a divide-by-zero exponent or a stuck envelope.
    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(0.0, 0.0, 0.5));
    let mut n = 0;
    while !env.is_idle() && n < 4410 {
        let g = env.next_gain();
        assert!(g.is_finite() && g > 0.0, "gain stayed finite and positive");
        n += 1;
    }
    assert!(env.is_idle(), "a zero attack must still resolve to unity");
    assert!(n >= 1, "and take at least a sample doing it");
}

#[test]
fn retriggering_mid_duck_ramps_from_the_current_gain() {
    // Upstream cancels the scheduled ramps and re-anchors at the current value,
    // so a second hit part-way through a recovery does not jump back to unity
    // first. The clamp `min(.., currVal)` is what enforces that.
    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(0.0, 0.2, 0.75));
    let mut before = 1.0;
    for _ in 0..441 {
        before = env.next_gain();
    }
    assert!(before < 1.0, "should still be recovering, got {before:.4}");

    // A much shallower duck cannot lift the gain back toward unity: the floor
    // is capped at the current value, so the envelope carries on from where it
    // was rather than restarting from 1.0.
    env.trigger(SR, &duck(0.0, 0.2, 0.01));
    let after = env.next_gain();
    assert!(
        after < before * 1.01,
        "a retrigger should resume from the current gain, not jump up:          {before:.4} -> {after:.4}"
    );
    assert!(
        after < 0.3,
        "and certainly not snap back to unity, got {after:.4}"
    );
}

// --- send routing off controls ----------------------------------------------

#[test]
fn orbit_send_reads_its_controls() {
    let map: ValueMap = [
        ("orbit".to_string(), Value::F64(3.0)),
        ("dry".to_string(), Value::F64(0.25)),
        ("room".to_string(), Value::F64(0.6)),
        ("delay".to_string(), Value::F64(0.4)),
        ("bus".to_string(), Value::F64(2.0)),
        ("busgain".to_string(), Value::F64(0.8)),
    ]
    .into_iter()
    .collect();
    let send = OrbitSend::from_controls(&map, 0.5);
    assert_eq!(send.orbit, 3);
    assert!((send.dry - 0.25).abs() < 1e-6);
    assert!((send.room - 0.6).abs() < 1e-6);
    assert!((send.delay - 0.4).abs() < 1e-6);
    assert_eq!(send.bus, Some(2));
    assert!((send.busgain - 0.8).abs() < 1e-6);

    // Defaults: orbit 1, full dry, no sends, no bus.
    let bare = OrbitSend::from_controls(&ValueMap::new(), 0.5);
    assert_eq!(bare.orbit, 1);
    assert_eq!(bare.bus, None);
    assert_eq!(bare.room, 0.0);
    assert_eq!(bare.delay, 0.0);
}

#[test]
fn duck_controls_expand_per_target() {
    // `duckorbit` may name several orbits; the other three controls are read
    // per target, falling back to entry 0 and then to the default.
    let map: ValueMap = [
        (
            "duckorbit".to_string(),
            Value::List(vec![Value::F64(1.0), Value::F64(2.0)]),
        ),
        (
            "duckdepth".to_string(),
            Value::List(vec![Value::F64(0.5), Value::F64(0.25)]),
        ),
        ("duckonset".to_string(), Value::F64(0.01)),
    ]
    .into_iter()
    .collect();
    let ducks = Duck::from_controls(&map);
    assert_eq!(ducks.len(), 2, "one duck per named orbit");
    assert_eq!(ducks[0].orbit, 1);
    assert_eq!(ducks[1].orbit, 2);
    // Per-target depth.
    assert!((ducks[0].depth - 0.5).abs() < 1e-6);
    assert!((ducks[1].depth - 0.25).abs() < 1e-6);
    // A single onset applies to both (entry 0 fallback).
    assert!((ducks[0].onset - 0.01).abs() < 1e-6);
    assert!((ducks[1].onset - 0.01).abs() < 1e-6);
    // Unset attack takes the default, floored at 2ms.
    assert!((ducks[0].attack - 0.1).abs() < 1e-6);

    // No `duckorbit` means no ducking at all, whatever else is set.
    let orphan: ValueMap = [("duckdepth".to_string(), Value::F64(0.5))]
        .into_iter()
        .collect();
    assert!(Duck::from_controls(&orphan).is_empty());
    assert!(Duck::from_controls(&ValueMap::new()).is_empty());

    // The attack floor applies to an explicitly tiny value too.
    let tiny: ValueMap = [
        ("duckorbit".to_string(), Value::F64(1.0)),
        ("duckattack".to_string(), Value::F64(0.0)),
    ]
    .into_iter()
    .collect();
    assert!((Duck::from_controls(&tiny)[0].attack - 0.002).abs() < 1e-6);
}

// --- the `s("bus")` source voice --------------------------------------------
//
// A bus voice plays back whatever the mixer last handed it, so its own tests
// have to supply that input directly through `set_bus_input` — which is also
// the routing check: a block addressed to a different bus must be ignored.

fn bus_params(bus: i32) -> BusParams {
    BusParams {
        bus,
        adsr: Adsr {
            attack: 0.0001,
            decay: 0.0001,
            sustain: 1.0,
            release: 0.01,
        },
        duration: 1.0,
        gain: 1.0,
        pan: 0.5,
        filters: FilterSet::default(),
    }
}

#[test]
fn a_bus_voice_replays_the_block_it_was_given() {
    // With nothing supplied it is silent rather than reading uninitialised.
    assert_eq!(
        BusVoice::new(bus_params(0), SR).tick(),
        (0.0, 0.0),
        "an unfed bus voice is silent"
    );

    let n = 32;
    let left: Vec<f32> = (0..n).map(|i| (i as f32 + 1.0) * 0.02).collect();
    let right: Vec<f32> = left.iter().map(|x| -x).collect();
    let mut v = BusVoice::new(bus_params(0), SR);
    v.set_bus_input(0, &left, &right);

    let out: Vec<(f32, f32)> = (0..n).map(|_| v.tick()).collect();
    // Centre pan is equal power, so both sides carry 1/sqrt(2) of the input.
    // Compared past the envelope's attack+decay (0.1ms each, so ~9 samples),
    // where the gain has reached its sustain of 1.
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    for i in 12..n {
        let (l, r) = out[i];
        assert!(
            (l - left[i] * centre).abs() < 1e-3,
            "sample {i}: left should replay {}, got {l}",
            left[i] * centre
        );
        assert!(
            (r - right[i] * centre).abs() < 1e-3,
            "sample {i}: right should replay its own channel, got {r}"
        );
    }
    // The two channels stay independent all the way through — the right is the
    // negated left here, and that survives the envelope and pan.
    for (i, (l, r)) in out.iter().enumerate().take(n) {
        assert!(
            (l + r).abs() < 1e-6,
            "sample {i}: the channels should stay opposite, got ({l}, {r})"
        );
    }
    // Reading past the end of the supplied block is silence, not a panic.
    for _ in 0..4 {
        assert_eq!(v.tick(), (0.0, 0.0), "past the block should be silent");
    }
}

#[test]
fn a_bus_voice_ignores_blocks_addressed_to_another_bus() {
    // The mixer calls `set_bus_input` on every voice for every bus, so the
    // number check is the whole routing — without it every bus voice would
    // play whatever was sent last.
    let mut v = BusVoice::new(bus_params(2), SR);
    v.set_bus_input(1, &[1.0; 8], &[1.0; 8]);
    assert_eq!(v.tick(), (0.0, 0.0), "bus 1 must not reach a bus 2 voice");

    v.set_bus_input(2, &[1.0; 8], &[1.0; 8]);
    let (l, _r) = v.tick();
    assert!(l.abs() > 0.1, "its own bus should reach it, got {l}");
}

#[test]
fn a_new_block_restarts_the_read_position() {
    // Each block replaces the last and rewinds; otherwise the voice would run
    // off the end of the first block it ever saw.
    let mut v = BusVoice::new(bus_params(0), SR);
    v.set_bus_input(0, &[0.5; 4], &[0.5; 4]);
    for _ in 0..4 {
        v.tick();
    }
    assert_eq!(v.tick(), (0.0, 0.0), "the first block is used up");

    v.set_bus_input(0, &[0.5; 4], &[0.5; 4]);
    let (l, _r) = v.tick();
    assert!(l.abs() > 0.1, "a fresh block should be readable, got {l}");
}

#[test]
fn a_bus_voice_applies_its_envelope_gain_and_pan() {
    let peak = |gain: f32, pan: f32| {
        let mut p = bus_params(0);
        p.gain = gain;
        p.pan = pan;
        let mut v = BusVoice::new(p, SR);
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for _ in 0..16 {
            v.set_bus_input(0, &[1.0; 1], &[1.0; 1]);
            let (a, b) = v.tick();
            l = l.max(a.abs());
            r = r.max(b.abs());
        }
        (l, r)
    };
    let (l1, _) = peak(1.0, 0.5);
    let (l2, _) = peak(0.5, 0.5);
    assert!(
        (l2 - l1 * 0.5).abs() < 1e-3,
        "gain should scale linearly: {l1:.4} vs {l2:.4}"
    );

    let (hl, hr) = peak(1.0, 0.0);
    assert!(hl > 0.0 && hr < 1e-6, "pan 0 is hard left");
    let (hl, hr) = peak(1.0, 1.0);
    assert!(hr > 0.0 && hl < 1e-6, "pan 1 is hard right");
}

#[test]
fn a_bus_voice_finishes_after_its_duration_plus_release() {
    // `end = duration + release + 0.01`, matching the timeout the sound uses.
    let mut p = bus_params(0);
    p.duration = 0.05;
    p.adsr.release = 0.02;
    let mut v = BusVoice::new(p, SR);
    assert!(!v.is_done());
    let mut n = 0;
    while !v.is_done() && n < SR as usize {
        v.set_bus_input(0, &[1.0; 1], &[1.0; 1]);
        v.tick();
        n += 1;
    }
    let got = n as f32 / SR;
    assert!(
        (got - 0.08).abs() < 0.002,
        "should end at duration + release + 0.01 = 0.08s, got {got:.3}s"
    );
    assert_eq!(v.tick(), (0.0, 0.0), "and stay silent after");
}

// --- the DJ filter ----------------------------------------------------------

#[test]
fn the_djf_knob_picks_a_mode_by_zone() {
    // Below 0.49 is a low-pass, above 0.51 a high-pass, and the dead zone
    // between them is a bypass — so a knob parked at centre costs nothing.
    for v in [0.0f32, 0.2, 0.48] {
        assert!(!Djf::new(SR, v).is_bypass(), "{v} should be a low-pass");
    }
    for v in [0.52f32, 0.8, 1.0] {
        assert!(!Djf::new(SR, v).is_bypass(), "{v} should be a high-pass");
    }
    for v in [0.49f32, 0.5, 0.51] {
        assert!(Djf::new(SR, v).is_bypass(), "{v} should be the dead zone");
    }
    // Out-of-range values clamp into the knob rather than wrapping.
    assert!(
        !Djf::new(SR, -1.0).is_bypass(),
        "below 0 clamps to full low"
    );
    assert!(
        !Djf::new(SR, 4.0).is_bypass(),
        "above 1 clamps to full high"
    );
}

#[test]
fn the_djf_passes_or_removes_the_low_band_by_side() {
    // Drive a slow sine (well inside any low-pass, well outside any high-pass)
    // and compare the level each side of the knob keeps.
    let level = |knob: f32| {
        let mut d = Djf::new(SR, knob);
        let mut peak = 0.0f32;
        for i in 0..4410 {
            let x = (std::f32::consts::TAU * 50.0 * i as f32 / SR).sin();
            let y = d.process(x);
            if i > 2205 {
                peak = peak.max(y.abs());
            }
        }
        peak
    };
    let bypass = level(0.5);
    assert!(
        (bypass - 1.0).abs() < 0.01,
        "the dead zone is a straight wire"
    );

    // A low-pass wide open keeps a 50Hz tone; a high-pass wide open removes it.
    assert!(
        level(0.48) > 0.5,
        "a nearly-open low-pass should keep a 50Hz tone, got {}",
        level(0.48)
    );
    assert!(
        level(1.0) < 0.2,
        "a full high-pass should remove a 50Hz tone, got {}",
        level(1.0)
    );
    // ...and a nearly-closed low-pass removes it too.
    assert!(
        level(0.02) < 0.2,
        "a nearly-closed low-pass should remove it, got {}",
        level(0.02)
    );
}

#[test]
fn retuning_the_djf_keeps_its_filter_state() {
    // `set_value` retunes without resetting, so sweeping the knob does not
    // click. A reset would show up as a discontinuity in the output.
    let mut d = Djf::new(SR, 0.2);
    let mut last = 0.0;
    for i in 0..1000 {
        last = d.process((std::f32::consts::TAU * 200.0 * i as f32 / SR).sin());
    }
    // Nudge the knob and take one more sample; a state reset would jump.
    d.set_value(0.21);
    let next = d.process((std::f32::consts::TAU * 200.0 * 1000.0 / SR).sin());
    assert!(
        (next - last).abs() < 0.2,
        "retuning should be continuous: {last:.4} -> {next:.4}"
    );
}

#[test]
fn the_onset_ramp_reaches_the_floor_in_exactly_its_own_length() {
    // The dip is geometric: `(floor/gain)^(1/dip)` per sample, so after `dip`
    // samples it has multiplied out to exactly `floor/gain` and no further.
    // Any other exponent lands somewhere else entirely at that sample.
    let onset = 0.01; // 441 samples at 44.1kHz
    let dip = (SR * onset) as usize;
    let depth = 0.75f32;
    let floor = 1.0 - depth.sqrt();

    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(onset, 30.0, depth));
    let mut gain = 1.0;
    for _ in 0..dip {
        gain = env.next_gain();
    }
    assert!(
        (gain - floor).abs() < 1e-3,
        "after {dip} samples the dip should be at its floor {floor:.4}, got {gain:.4}"
    );
    // Halfway there it is at the geometric midpoint, not the linear one.
    let mut env = DuckEnv::default();
    env.trigger(SR, &duck(onset, 30.0, depth));
    let mut half = 1.0;
    for _ in 0..dip / 2 {
        half = env.next_gain();
    }
    assert!(
        (half - floor.sqrt()).abs() < 1e-3,
        "halfway should be sqrt(floor) = {:.4}, got {half:.4}",
        floor.sqrt()
    );
}

#[test]
fn a_reverb_config_prints_its_impulse_length_not_its_samples() {
    // `Sample` is deliberately not `Debug` (it holds whole buffers), so this
    // impl is what any diagnostic of a bus prints.
    let cfg = ReverbConfig {
        size: 2.0,
        fade: 0.5,
        lp: 8000.0,
        dim: 0.7,
        ir: None,
        irbegin: 0.0,
        irspeed: 1.0,
    };
    let text = format!("{cfg:?}");
    assert!(text.contains("ReverbConfig"), "{text}");
    assert!(text.contains("size: 2.0"), "{text}");
    assert!(text.contains("ir: None"), "{text}");
}
