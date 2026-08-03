use super::common::*;

#[test]
fn sampler_plays_a_buffer_then_finishes() {
    // a 0.1s buffer of a 200 Hz sine
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (TAU * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut v = SamplerVoice::new(SamplerParams::new(sample), sr);
    let mut out = Vec::new();
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        out.push(v.tick().0);
        frames += 1;
    }
    assert_is_signal(&out, "sampler playing a 200 Hz sine");
    assert!(v.is_done(), "sampler should finish at the buffer end");
    assert!(frames < 44100, "sampler should not run forever");
}

#[test]
fn sampler_speed_changes_duration() {
    let sr = 44100.0;
    let data = vec![0.5f32; 4410]; // 0.1s of DC
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut fast = SamplerParams::new(sample.clone());
    fast.speed = 2.0;
    let mut v = SamplerVoice::new(fast, sr);
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        v.tick();
        frames += 1;
    }
    // at 2x speed the 0.1s buffer should take ~0.05s (~2205 frames)
    assert!(frames < 3000, "2x speed should play back in ~half the time");
}

#[test]
fn loop_plays_past_the_buffers_natural_length() {
    // A 0.1s buffer asked to loop for 0.5s should still be audible well past
    // its own length, then stop near the hold time (not run forever).
    let sr = 44100.0;
    let n = (sr * 0.1) as usize;
    let data: Vec<f32> = (0..n)
        .map(|i| (TAU * 200.0 * i as f32 / sr).sin())
        .collect();
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });
    let mut p = SamplerParams::new(sample);
    p.loop_on = true;
    p.duration = 0.5; // hold far longer than the 0.1s buffer
    let mut v = SamplerVoice::new(p, sr);

    let mut peak_late = 0.0f32;
    let mut frames = 0;
    while !v.is_done() && frames < 44100 {
        let s = v.tick().0.abs();
        if frames > (sr * 0.2) as usize {
            peak_late = peak_late.max(s); // sampled past the natural end
        }
        frames += 1;
    }
    assert!(
        peak_late > 0.0,
        "a looping sample should still sound past its natural length"
    );
    assert!(
        frames >= (sr * 0.4) as usize,
        "should play roughly the hold duration"
    );
    assert!(
        frames < (sr * 0.7) as usize,
        "should stop after the hold + release, not loop forever"
    );
}

#[test]
fn vibrato_and_pitch_env_detune_a_sample() {
    // superdough applies `getVibratoOscillator` / `getPitchEnvelope` to a
    // sampler's `detune` just as it does to a synth's, so `vib`/`penv` must
    // change how fast the buffer is read.
    let sr = 44100.0;
    let n = (sr * 2.0) as usize;
    let data: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect(); // a ramp
    let sample = Arc::new(Sample {
        data,
        sample_rate: sr,
    });

    // How far into the buffer a voice has read after 0.5s, read off the ramp.
    let read_position = |apply: &dyn Fn(&mut SamplerParams)| -> f32 {
        let mut p = SamplerParams::new(sample.clone());
        p.duration = 1.5;
        apply(&mut p);
        let mut v = SamplerVoice::new(p, sr);
        let mut last = 0.0;
        for _ in 0..(sr * 0.5) as usize {
            last = v.tick().0;
        }
        last
    };

    let plain = read_position(&|_p| {});
    assert!(plain > 0.0, "the ramp should be audible");

    // A pitch envelope that starts an octave up and stays there (long attack)
    // reads the buffer roughly twice as fast.
    let up = read_position(&|p| {
        p.pitch = PitchMod::new(
            None,
            0.0,
            Some(12.0), // penv: +12 semitones
            Some(0.0),  // pattack
            Some(0.0),  // pdecay
            Some(1.0),  // psustain: hold at the top
            None,
            Some(0.0), // panchor: 0 so the range is 0..+12
            false,
        );
    });
    assert!(
        up > plain * 1.8 && up < plain * 2.2,
        "an octave-up pitch envelope should read ~2x as far: {up} vs {plain}"
    );

    // Vibrato alone averages out over whole cycles, but it must move the read
    // position somewhere other than exactly the unmodulated one.
    let wobbled = read_position(&|p| {
        p.pitch = PitchMod::new(Some(3.0), 7.0, None, None, None, None, None, None, false);
    });
    assert!(
        (wobbled - plain).abs() > 1e-4,
        "vibrato should perturb the read position: {wobbled} vs {plain}"
    );
}

// --- how the parameters land on the read ------------------------------------
//
// The 2026-08 mutation run left 61 of sampler.rs's 121 mutants alive, nearly all
// of them arithmetic inside `with_mods`, `envelope` and `tick`. The tests above
// assert that a sample *sounds* and roughly how long for, which every one of
// those mutants survives: swap a `*` for a `/` in the step and the buffer still
// plays, just at the wrong rate.
//
// These read the position back instead. A ramp buffer's value at frame `i` is
// `i / n`, so the sample coming out names where in the buffer it came from, and
// the whole begin/end/speed/loop chain becomes a number rather than a duration.

/// A buffer whose value at frame `i` is `i / n`.
fn ramp(n: usize, sample_rate: f32) -> Arc<Sample> {
    Arc::new(Sample {
        data: (0..n).map(|i| i as f32 / n as f32).collect(),
        sample_rate,
    })
}

/// Parameters that read a ramp with nothing shaping the output: hard left so
/// `tick().0` is the raw sample, no attack ramp, and a hold long enough that
/// the envelope stays at 1.
fn plain(sample: Arc<Sample>) -> SamplerParams {
    let mut p = SamplerParams::new(sample);
    p.pan = 0.0;
    p.attack = 0.0;
    p.release = 0.0;
    p.duration = 100.0;
    p
}

/// Every left-channel sample a voice produces before it finishes.
fn play(p: SamplerParams, sample_rate: f32) -> Vec<f32> {
    let mut v = SamplerVoice::new(p, sample_rate);
    let mut out = Vec::new();
    while !v.is_done() && out.len() < 400_000 {
        out.push(v.tick().0);
    }
    out
}

#[test]
fn begin_and_end_select_a_slice_of_the_buffer() {
    let sr = 44100.0;
    let s = ramp(1000, sr);

    // Playing the whole buffer runs the ramp from ~0 to ~1.
    let all = play(plain(s.clone()), sr);
    let furthest = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(*x));
    assert!(all[0] < 0.01, "starts at the buffer start: {}", all[0]);
    assert!(furthest(&all) > 0.99, "and reaches the end");

    // `begin` is a fraction of the buffer, so half-way in starts at 0.5.
    let mut p = plain(s.clone());
    p.begin = 0.5;
    let half = play(p, sr);
    assert!(
        (half[0] - 0.5).abs() < 0.01,
        "begin 0.5 starts half-way: {}",
        half[0]
    );
    assert!(half.len() < all.len() * 3 / 5, "and plays the shorter half");

    // `end` bounds the far side.
    let mut p = plain(s.clone());
    p.end = 0.25;
    let quarter = play(p, sr);
    assert!(
        (furthest(&quarter) - 0.25).abs() < 0.01,
        "end 0.25 stops a quarter in: {}",
        furthest(&quarter)
    );

    // Out-of-range fractions clamp rather than reading outside the buffer.
    let mut p = plain(s.clone());
    p.begin = -1.0;
    p.end = 5.0;
    let clamped = play(p, sr);
    assert_eq!(clamped.len(), all.len(), "clamps to the whole buffer");
}

#[test]
fn the_step_is_the_resample_ratio_times_the_speed() {
    let sr = 44100.0;

    // A buffer recorded at half the engine rate is read at half a frame per
    // tick, so it takes twice as many ticks.
    let same = play(plain(ramp(1000, sr)), sr);
    let slow = play(plain(ramp(1000, sr / 2.0)), sr);
    assert!(
        (slow.len() as f32 / same.len() as f32 - 2.0).abs() < 0.05,
        "a half-rate buffer takes ~2x the ticks: {} vs {}",
        slow.len(),
        same.len()
    );

    // Speed multiplies that ratio.
    let mut p = plain(ramp(1000, sr));
    p.speed = 2.0;
    let fast = play(p, sr);
    assert!(
        (same.len() as f32 / fast.len() as f32 - 2.0).abs() < 0.05,
        "2x speed halves the ticks: {} vs {}",
        fast.len(),
        same.len()
    );
}

#[test]
fn unit_cycles_scales_the_speed_by_the_buffers_own_duration() {
    // `unit: c` is what loopAt/fit/splice use to time-stretch: the rate is
    // multiplied by the sample's length in seconds, so a half-second buffer
    // plays at half speed for a `speed` of 1.
    let sr = 44100.0;
    let half_second = ramp((sr * 0.5) as usize, sr);

    let normal = play(plain(half_second.clone()), sr);
    let mut p = plain(half_second);
    p.unit_cycles = true;
    let stretched = play(p, sr);

    assert!(
        (stretched.len() as f32 / normal.len() as f32 - 2.0).abs() < 0.05,
        "a 0.5s buffer in cycle units plays 2x as long: {} vs {}",
        stretched.len(),
        normal.len()
    );
}

#[test]
fn duration_caps_the_natural_length_but_never_extends_it() {
    let sr = 44100.0;
    let s = ramp((sr * 0.4) as usize, sr); // 0.4s of buffer

    // Nothing set: play to the natural end.
    let mut p = plain(s.clone());
    p.duration = 0.0;
    let natural = play(p, sr);
    assert!(
        (natural.len() as f32 / sr - 0.4).abs() < 0.02,
        "natural length is the buffer's own: {} frames",
        natural.len()
    );

    // Shorter than natural: the hold wins.
    let mut p = plain(s.clone());
    p.duration = 0.1;
    let short = play(p, sr);
    assert!(
        (short.len() as f32 / sr - 0.1).abs() < 0.02,
        "a shorter hold cuts it: {} frames",
        short.len()
    );

    // Longer than natural: a one-shot still stops at the buffer end.
    let mut p = plain(s);
    p.duration = 10.0;
    let long = play(p, sr);
    assert!(
        (long.len() as f32 / sr - 0.4).abs() < 0.02,
        "a longer hold does not stretch a one-shot: {} frames",
        long.len()
    );
}

#[test]
fn looping_needs_a_forward_step_and_a_region_with_width() {
    let sr = 44100.0;
    let s = ramp(1000, sr);

    // The plain case loops and so outlives the buffer.
    let mut p = plain(s.clone());
    p.loop_on = true;
    p.duration = 0.5;
    let looped = play(p, sr);
    assert!(
        looped.len() as f32 / sr > 0.4,
        "a loop plays for its hold: {} frames",
        looped.len()
    );

    // A zero-width region has nowhere to wrap to.
    let mut p = plain(s.clone());
    p.loop_on = true;
    p.duration = 0.5;
    p.loop_begin = 0.5;
    p.loop_end = 0.5;
    let flat = play(p, sr);
    assert!(
        flat.len() < looped.len() / 2,
        "an empty loop region does not loop: {} frames",
        flat.len()
    );

    // Nor does a backwards one.
    let mut p = plain(s.clone());
    p.loop_on = true;
    p.duration = 0.5;
    p.loop_begin = 0.75;
    p.loop_end = 0.25;
    let reversed = play(p, sr);
    assert!(reversed.len() < looped.len() / 2, "backwards region");

    // A stopped read head cannot reach the loop end either.
    let mut p = plain(s);
    p.loop_on = true;
    p.duration = 0.2;
    p.speed = 0.0;
    let stopped = play(p, sr);
    assert!(
        stopped.iter().all(|s| s.abs() < 0.01),
        "speed 0 stays at the buffer start rather than looping"
    );
}

#[test]
fn a_loop_wraps_the_read_head_back_inside_its_region() {
    // The read position has to land back at `loop_start`, not at 0 and not
    // somewhere outside — a ramp shows exactly where.
    let sr = 44100.0;
    let mut p = plain(ramp(1000, sr));
    p.loop_on = true;
    p.loop_begin = 0.5;
    p.loop_end = 0.75;
    p.duration = 0.2;
    let out = play(p, sr);

    // Once past the first pass, every sample sits inside the loop region.
    let settled = &out[out.len() / 2..];
    let lo = settled.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = settled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        lo >= 0.49 && hi <= 0.76,
        "the loop stays in 0.5..0.75, got {lo}..{hi}"
    );
    // ...and it really does wrap, rather than sitting on one value.
    assert!(hi - lo > 0.2, "the whole region is read, got {lo}..{hi}");
}

#[test]
fn pan_splits_the_voice_across_the_channels() {
    let sr = 44100.0;
    let s = ramp(1000, sr);
    let at = |pan: f32| {
        let mut p = plain(s.clone());
        p.pan = pan;
        p.begin = 0.5; // a non-zero sample to scale
        let mut v = SamplerVoice::new(p, sr);
        v.tick()
    };

    let (l, r) = at(0.0);
    assert!(l > 0.4 && r.abs() < 1e-6, "hard left: {l} {r}");
    let (l, r) = at(1.0);
    assert!(l.abs() < 1e-6 && r > 0.4, "hard right: {l} {r}");
    let (l, r) = at(0.5);
    assert!((l - r).abs() < 1e-6 && l > 0.3, "centre is equal: {l} {r}");
    // Out-of-range pans clamp rather than inverting a channel.
    let (l, r) = at(-1.0);
    assert!(l > 0.4 && r.abs() < 1e-6, "clamped left: {l} {r}");
}

#[test]
fn the_envelope_ramps_up_holds_and_ramps_down() {
    let sr = 44100.0;
    // DC so the envelope is the only thing shaping the output.
    let s = Arc::new(Sample {
        data: vec![1.0; 44100],
        sample_rate: sr,
    });
    let mut p = SamplerParams::new(s);
    p.pan = 0.0;
    p.attack = 0.1;
    p.release = 0.1;
    p.duration = 0.3;
    let out = play(p, sr);

    let at = |secs: f32| out[(secs * sr) as usize];
    // Attack climbs from silence to full.
    assert!(at(0.0).abs() < 0.01, "starts silent: {}", at(0.0));
    assert!(
        (at(0.05) - 0.5).abs() < 0.05,
        "half-way up the attack: {}",
        at(0.05)
    );
    assert!(at(0.02) < at(0.08), "the attack rises");
    // Then holds at full for the rest of the duration.
    assert!((at(0.2) - 1.0).abs() < 0.01, "holds at 1: {}", at(0.2));
    // Then falls back to silence over the release.
    assert!(
        (at(0.35) - 0.5).abs() < 0.05,
        "half-way down the release: {}",
        at(0.35)
    );
    assert!(
        *out.last().unwrap() < 0.05,
        "ends near silence: {}",
        out.last().unwrap()
    );
    assert!(
        (out.len() as f32 / sr - 0.4).abs() < 0.02,
        "runs for hold + release: {} frames",
        out.len()
    );
}

#[test]
fn a_fractional_step_interpolates_between_neighbouring_frames() {
    // Half-speed reads land between frames; without interpolation they would
    // repeat each frame twice and the ramp would come out as a staircase.
    let sr = 44100.0;
    let mut p = plain(ramp(64, sr));
    p.speed = 0.5;
    let out = play(p, sr);
    // Only while the ramp is climbing; the envelope closes the tail.
    let steps: Vec<f32> = out[..out.len() / 2]
        .windows(2)
        .map(|w| w[1] - w[0])
        .collect();
    // A staircase would alternate 0 and 1/64; interpolation gives 1/128 every
    // tick.
    assert!(
        steps.iter().all(|d| (*d - 1.0 / 128.0).abs() < 1e-6),
        "each half-frame step is half a ramp step: {:?}",
        &steps[..6]
    );
}

#[test]
fn gain_scales_the_output() {
    let sr = 44100.0;
    let peak = |gain: f32| {
        let mut p = plain(ramp(1000, sr));
        p.gain = gain;
        play(p, sr).iter().fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let unity = peak(1.0);
    assert!(
        (peak(0.5) / unity - 0.5).abs() < 0.01,
        "half gain halves it"
    );
    assert!(
        (peak(2.0) / unity - 2.0).abs() < 0.01,
        "double gain doubles"
    );
    assert!(peak(0.0) < 1e-6, "zero gain is silence");
}

#[test]
fn the_filter_slot_carries_every_control_it_is_given() {
    // `with_mods` builds the sampler's lowpass from four separate fields, and
    // dropping any one of them leaves a filter that still runs — just not the
    // one that was asked for.
    let sr = 44100.0;
    // Bright content, so a lowpass has something to remove.
    let n = 4410;
    let s = Arc::new(Sample {
        data: (0..n)
            .map(|i| (TAU * 8000.0 * i as f32 / sr).sin())
            .collect(),
        sample_rate: sr,
    });
    let energy = |apply: &dyn Fn(&mut SamplerParams)| {
        let mut p = plain(s.clone());
        apply(&mut p);
        play(p, sr).iter().map(|x| x * x).sum::<f32>()
    };

    let dry = energy(&|_| {});
    let cut = energy(&|p| p.cutoff = Some(300.0));
    assert!(
        cut < dry * 0.5,
        "the cutoff removes the 8k tone: {cut} {dry}"
    );

    // Resonance, model and drive each change the result of the same cutoff.
    let base = energy(&|p| {
        p.cutoff = Some(1000.0);
        p.model = FilterModel::Ladder;
    });
    let resonant = energy(&|p| {
        p.cutoff = Some(1000.0);
        p.model = FilterModel::Ladder;
        p.resonance = 12.0;
    });
    assert!(
        (resonant - base).abs() > base * 0.01,
        "resonance changes it: {resonant} vs {base}"
    );
    let driven = energy(&|p| {
        p.cutoff = Some(1000.0);
        p.model = FilterModel::Ladder;
        p.drive = 8.0;
    });
    assert!(
        (driven - base).abs() > base * 0.01,
        "drive changes it: {driven} vs {base}"
    );
    let other_model = energy(&|p| {
        p.cutoff = Some(1000.0);
        p.model = FilterModel::Db24;
    });
    assert!(
        (other_model - base).abs() > base * 0.01,
        "the model changes it: {other_model} vs {base}"
    );
}

#[test]
fn apply_controls_maps_the_pattern_names_onto_the_parameters() {
    // Nothing drove `apply_controls` at all: the mutation run replaced its whole
    // body with `()` and every test still passed. It is the only path from a
    // pattern's controls to a sampler voice, so an unread control is a control
    // that silently does nothing.
    let sr = 44100.0;
    let s = ramp(1000, sr);
    let with = |k: &str, v: Value| {
        let mut map = ValueMap::new();
        map.insert(k.to_string(), v);
        let mut p = SamplerParams::new(s.clone());
        p.apply_controls(&map);
        p
    };

    assert_eq!(with("gain", Value::F64(0.25)).gain, 0.25);
    assert_eq!(with("pan", Value::F64(0.75)).pan, 0.75);
    assert_eq!(with("speed", Value::F64(2.5)).speed, 2.5);
    assert_eq!(with("cutoff", Value::F64(800.0)).cutoff, Some(800.0));
    assert_eq!(with("drive", Value::F64(3.0)).drive, 3.0);
    assert_eq!(with("attack", Value::F64(0.2)).attack, 0.2);
    assert_eq!(with("release", Value::F64(0.3)).release, 0.3);

    // Resonance has a floor: a Q of zero is a divide-by-zero in the biquad.
    assert_eq!(with("resonance", Value::F64(4.0)).resonance, 4.0);
    assert_eq!(with("resonance", Value::F64(0.0)).resonance, 0.1);

    // Positions are fractions, clamped rather than read past the buffer.
    assert_eq!(with("begin", Value::F64(0.3)).begin, 0.3);
    assert_eq!(with("end", Value::F64(0.8)).end, 0.8);
    assert_eq!(with("begin", Value::F64(-2.0)).begin, 0.0);
    assert_eq!(with("end", Value::F64(9.0)).end, 1.0);
    assert_eq!(with("loopBegin", Value::F64(-1.0)).loop_begin, 0.0);
    assert_eq!(with("loopEnd", Value::F64(5.0)).loop_end, 1.0);

    // `unit` selects cycle-relative speed, and only the exact string does.
    assert!(with("unit", Value::from("c")).unit_cycles, "unit: c");
    assert!(!with("unit", Value::from("s")).unit_cycles, "unit: s");
    assert!(
        !with("unit", Value::from("cycles")).unit_cycles,
        "not a prefix"
    );

    // `loop` is a number used as a flag, so anything non-zero turns it on.
    assert!(with("loop", Value::F64(1.0)).loop_on, "loop 1");
    assert!(
        with("loop", Value::F64(-1.0)).loop_on,
        "loop -1 is still on"
    );
    assert!(!with("loop", Value::F64(0.0)).loop_on, "loop 0 is off");

    // An empty map changes nothing.
    let mut untouched = SamplerParams::new(s.clone());
    let before = (untouched.gain, untouched.speed, untouched.begin);
    untouched.apply_controls(&ValueMap::new());
    assert_eq!((untouched.gain, untouched.speed, untouched.begin), before);
}

#[test]
fn the_natural_length_is_the_slice_divided_by_the_step() {
    // `natural` caps the hold, so it only shows up where it is *shorter* than
    // the buffer would otherwise play for — which needs a slice and a release
    // long enough to keep the voice ticking past it.
    let sr = 44100.0;
    let s = ramp(4410, sr); // 0.1s

    // Half the buffer at unit speed is half the time.
    let mut p = plain(s.clone());
    p.begin = 0.5;
    p.duration = 0.0;
    let half = play(p, sr);
    assert!(
        (half.len() as f32 / sr - 0.05).abs() < 0.005,
        "half a 0.1s buffer is 0.05s, got {} frames",
        half.len()
    );

    // Reading at half speed doubles it.
    let mut p = plain(s.clone());
    p.speed = 0.5;
    p.duration = 0.0;
    let slow = play(p, sr);
    assert!(
        (slow.len() as f32 / sr - 0.2).abs() < 0.01,
        "0.1s at half speed is 0.2s, got {} frames",
        slow.len()
    );

    // And a middle slice is bounded on both sides.
    let mut p = plain(s);
    p.begin = 0.25;
    p.end = 0.75;
    p.duration = 0.0;
    let middle = play(p, sr);
    assert!(
        (middle.len() as f32 / sr - 0.05).abs() < 0.005,
        "the middle half is 0.05s, got {} frames",
        middle.len()
    );
}

#[test]
fn a_one_shot_stops_at_its_end_position_even_with_a_release_running() {
    // The release keeps the voice ticking after the hold, so without the
    // position check the read head would carry on past `end` into the rest of
    // the buffer.
    let sr = 44100.0;
    let mut p = plain(ramp(4410, sr));
    p.end = 0.25;
    p.release = 0.1; // far longer than the 0.025s slice
    let out = play(p, sr);
    let furthest = out.iter().fold(0.0f32, |m, x| m.max(*x));
    assert!(
        (furthest - 0.25).abs() < 0.01,
        "the read stops a quarter in even while releasing, reached {furthest}"
    );
}

#[test]
fn a_stopped_read_head_has_no_natural_length_to_hold_for() {
    // `step > 0` gates looping; without it a `speed: 0` voice would hold its
    // first frame for the whole hap instead of ending immediately.
    let sr = 44100.0;
    let mut p = plain(ramp(1000, sr));
    p.begin = 0.5; // a non-zero frame, so silence is not the giveaway
    p.speed = 0.0;
    p.loop_on = true;
    p.duration = 0.2;
    let out = play(p, sr);
    assert!(
        (out.len() as f32) < sr * 0.05,
        "a stopped read head ends at once, played {} frames",
        out.len()
    );
}

#[test]
fn a_gain_modulator_adds_to_the_voices_own_gain() {
    // The LFO is offset to stay positive, so it can only push the level up —
    // which is what makes the sign of the `gain + mod` term checkable.
    let sr = 44100.0;
    let level = |mods: &[ModSpec]| {
        let mut p = plain(ramp(2000, sr));
        p.gain = 0.5;
        let mut v = SamplerVoice::with_mods(p, sr, mods);
        let mut peak = 0.0f32;
        while !v.is_done() {
            peak = peak.max(v.tick().0.abs());
        }
        peak
    };
    let plain_level = level(&[]);
    let modulated = level(&positive_lfo("gain", 0.5, 3.0).voice);
    assert!(
        modulated > plain_level * 1.05,
        "a positive gain LFO raises the level: {modulated} vs {plain_level}"
    );
}

#[test]
fn bus_input_reaches_the_voices_modulators() {
    // A sampler can be modulated by another orbit's output; the mixer hands
    // that over through `set_bus_input`, and dropping it leaves the modulator
    // reading silence.
    let sr = 44100.0;
    let mut map = ValueMap::new();
    let mut entry = ValueMap::new();
    entry.insert("control".to_string(), Value::from("gain"));
    entry.insert("bus".to_string(), Value::F64(0.0));
    entry.insert("depthabs".to_string(), Value::F64(0.5));
    entry.insert("dcoffset".to_string(), Value::F64(0.0));
    let mut desc = ValueMap::new();
    desc.insert("__ids".to_string(), Value::List(vec![Value::from("0")]));
    desc.insert("0".to_string(), Value::Map(entry));
    map.insert("bmod".to_string(), Value::Map(desc));
    let ctx = ModContext {
        cps: 0.5,
        cycle: 0.0,
        note_seconds: 1.0,
    };
    let specs = ModSpecs::from_controls(&map, &ctx, |_| 25.0);
    assert!(
        !specs.voice.is_empty(),
        "the bus modulator descriptor should resolve, or this test is vacuous"
    );

    let peak_with = |feed: bool| {
        let mut p = plain(ramp(2000, sr));
        p.gain = 0.5;
        let mut v = SamplerVoice::with_mods(p, sr, &specs.voice);
        if feed {
            let block: Vec<f32> = vec![1.0; 256];
            v.set_bus_input(0, &block, &block);
        }
        let mut peak = 0.0f32;
        for _ in 0..256 {
            peak = peak.max(v.tick().0.abs());
        }
        peak
    };
    assert!(
        peak_with(true) > peak_with(false),
        "feeding the bus should reach the gain modulator"
    );
}
