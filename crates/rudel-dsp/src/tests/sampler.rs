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
