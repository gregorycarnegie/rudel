use super::engine::next_schedule_window;
use super::*;
use crate::scope::{SCOPE_TAP_LEN, ScopeTap};
use crate::{Clock, SampleBank, collect_events};
use rudel_core::Pattern;

fn test_volume(value: f64) -> Arc<AtomicU64> {
    let volume = Arc::new(AtomicU64::new(0));
    store_f64(&volume, value);
    volume
}

fn test_mixer(rx: Receiver<NoteEvent>) -> Mixer {
    test_mixer_with_volume(rx, test_volume(DEFAULT_MASTER_VOLUME))
}

fn test_mixer_with_volume(rx: Receiver<NoteEvent>, volume: Arc<AtomicU64>) -> Mixer {
    Mixer {
        rx,
        pending: Vec::new(),
        active: Vec::new(),
        sample_clock: 0,
        sample_rate: 44100.0,
        played: Arc::new(AtomicU64::new(0)),
        orbits: HashMap::new(),
        signal_buses: HashMap::new(),
        volume,
        scratch: MixScratch::default(),
        taps: Arc::new(ScopeTaps::new()),
        tag_bufs: HashMap::new(),
    }
}

#[test]
fn scope_tap_returns_the_most_recent_samples() {
    let tap = ScopeTap::new();
    // Fewer samples written than requested: left-padded with silence,
    // stereo frames averaged to mono.
    tap.write_frames(&[(1.0, 1.0), (1.0, 3.0)]);
    let mut out = [9.0f32; 4];
    tap.latest(&mut out);
    assert_eq!(out, [0.0, 0.0, 1.0, 2.0]);
    // Wrap the ring and confirm the newest window is still returned.
    tap.write_frames(&vec![(0.5, 0.5); SCOPE_TAP_LEN + 3]);
    tap.latest(&mut out);
    assert_eq!(out, [0.5; 4]);
}

#[test]
fn tagged_voices_feed_their_widget_tap_only() {
    let mut mixer = OfflineMixer::new(44100.0);
    let tagged = mixer.taps().get_or_create("w1");
    let silent = mixer.taps().get_or_create("w2");
    let ev = |tags: Vec<String>| NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
            &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
            10.0,
        ))),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send: OrbitSend::default(),
        duck: Vec::new(),
        mods: Default::default(),
        tags,
    };
    mixer.schedule(ev(vec!["w1".to_string()]));
    mixer.schedule(ev(Vec::new()));
    let mut out = vec![(0.0f32, 0.0f32); 2048];
    mixer.render_block(&mut out);

    let mut got = [0.0f32; 256];
    tagged.latest(&mut got);
    assert!(
        got.iter().any(|s| s.abs() > 0.0),
        "the w1 tap should hear the w1-tagged voice"
    );
    silent.latest(&mut got);
    assert!(
        got.iter().all(|s| *s == 0.0),
        "the w2 tap should stay silent (no voice carries its tag)"
    );
    let mut master = [0.0f32; 256];
    mixer.taps().master().latest(&mut master);
    assert!(
        master.iter().any(|s| s.abs() > 0.0),
        "the master tap hears the mix"
    );
}

#[test]
fn stereo_delay_echoes_after_its_time() {
    let mut d = StereoDelay::new(
        1000.0,
        DelayConfig {
            time: 0.01, // 10-sample delay
            feedback: 0.5,
        },
    );
    let (o0, _) = d.process(1.0, 0.0); // impulse in
    assert_eq!(o0, 0.0, "no output before the delay time");
    let mut max_echo = 0.0f32;
    for _ in 0..20 {
        max_echo = max_echo.max(d.process(0.0, 0.0).0);
    }
    assert!(
        max_echo > 0.0,
        "impulse should re-emerge after the delay time"
    );
}

#[test]
fn reverb_send_produces_a_tail() {
    let (tx, rx) = mpsc::channel::<NoteEvent>();
    let mut mixer = test_mixer(rx);
    // a short note with a big reverb send
    let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69))).room(1.0);
    for ev in collect_events(&pat, 4.0, 0.0, 1.0, &SampleBank::new()) {
        tx.send(ev).unwrap();
    }
    drop(tx);

    // play past the (short) note, then measure the tail afterwards
    for _ in 0..6000 {
        mixer.render_frame();
    }
    let mut tail = 0.0f32;
    for _ in 0..4000 {
        tail += mixer.render_frame().0.abs();
    }
    assert!(tail > 0.0, "reverb should ring out after the note ends");
}

/// Render `secs` seconds of `pat` at `cps` and return the mono frames.
fn render_pattern(pat: &Pattern, cps: f64, secs: f32) -> Vec<f32> {
    render_pattern_with_bank(pat, cps, secs, SampleBank::new())
}

/// Like [`render_pattern`], but resolving sounds against `bank` (so a test
/// can supply an impulse response or a sample).
fn render_pattern_with_bank(pat: &Pattern, cps: f64, secs: f32, bank: SampleBank) -> Vec<f32> {
    let (tx, rx) = mpsc::channel::<NoteEvent>();
    let mut mixer = test_mixer(rx);
    for ev in collect_events(pat, cps, 0.0, 1.0, &bank) {
        tx.send(ev).unwrap();
    }
    drop(tx);
    (0..(44100.0 * secs) as usize)
        .map(|_| {
            let (l, r) = mixer.render_frame();
            (l + r) * 0.5
        })
        .collect()
}

/// Index of the loudest frame in `frames[from..]`, as a time in seconds.
fn peak_time(frames: &[f32], from: usize) -> f32 {
    let (i, _) = frames[from..]
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap();
    (from + i) as f32 / 44100.0
}

#[test]
fn delaytime_places_the_echo() {
    // `delaytime` used to be inert (the delay line was hardwired to 1/6s).
    // A short note with a full delay send should echo at `delaytime`.
    let echo_at = |delaytime: f64| {
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .delay(rudel_core::Value::F64(1.0))
            .delaytime(rudel_core::Value::F64(delaytime))
            .delayfeedback(rudel_core::Value::F64(0.0));
        // Skip the direct signal (the note itself is ~0.06s at 4 cps).
        let frames = render_pattern(&pat, 4.0, 0.6);
        peak_time(&frames, (44100.0 * 0.12) as usize)
    };
    for want in [0.2, 0.35] {
        let got = echo_at(want);
        assert!(
            (got - want as f32).abs() < 0.02,
            "delaytime({want}) should echo at ~{want}s, got {got}s"
        );
    }
}

#[test]
fn delaysync_scales_the_echo_with_cps() {
    // With no explicit `delaytime`, superdough derives it from `delaysync`
    // (a fraction of a cycle), so the echo tracks the tempo.
    let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
        .delay(rudel_core::Value::F64(1.0))
        .delaysync(rudel_core::Value::F64(0.25))
        .delayfeedback(rudel_core::Value::F64(0.0));
    // 0.25 cycles at 1 cps = 0.25s; at 2 cps = 0.125s.
    let slow = peak_time(&render_pattern(&pat, 1.0, 0.6), (44100.0 * 0.06) as usize);
    let fast = peak_time(&render_pattern(&pat, 2.0, 0.6), (44100.0 * 0.06) as usize);
    assert!(
        (slow - 0.25).abs() < 0.02,
        "1 cps echo at {slow}s, want 0.25s"
    );
    assert!(
        (fast - 0.125).abs() < 0.02,
        "2 cps echo at {fast}s, want 0.125s"
    );
}

#[test]
fn roomsize_lengthens_the_reverb_tail() {
    // `size`/`roomsize` used to be inert (one fixed 1.5s reverb).
    let tail_energy = |size: f64| {
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .room(rudel_core::Value::F64(1.0))
            .dry(rudel_core::Value::F64(0.0))
            .size(rudel_core::Value::F64(size));
        let frames = render_pattern(&pat, 4.0, 3.0);
        // Energy well after the note has finished.
        frames[(44100.0 * 1.5) as usize..]
            .iter()
            .map(|x| x.abs())
            .sum::<f32>()
    };
    let short = tail_energy(0.3);
    let long = tail_energy(6.0);
    assert!(
        long > short * 2.0,
        "a 6s room ({long}) should ring far longer than a 0.3s room ({short})"
    );
}

#[test]
fn roomfade_delays_the_onset_of_the_reverb_tail() {
    // `roomfade` fades the impulse response in, so the wet signal builds
    // gradually instead of arriving with the note. It was accepted but
    // ignored while the reverb was an FDN.
    let wet = |fade: f64| {
        let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .room(rudel_core::Value::F64(1.0))
            .dry(rudel_core::Value::F64(0.0))
            .size(rudel_core::Value::F64(2.0))
            .roomfade(rudel_core::Value::F64(fade));
        render_pattern(&pat, 4.0, 2.0)
    };
    // Energy in the first 200ms of wet signal, past the convolver's own
    // one-partition latency.
    let early = |frames: &[f32]| {
        let from = Convolver::LATENCY + 64;
        frames[from..from + (44100.0 * 0.2) as usize]
            .iter()
            .map(|x| x.abs())
            .sum::<f32>()
    };
    let sharp = early(&wet(0.0));
    let faded = early(&wet(1.0));
    assert!(
        faded < sharp * 0.5,
        "a 1s fade-in ({faded}) should suppress the early wet signal vs no fade ({sharp})"
    );
}

#[test]
fn ir_uses_a_loaded_sample_as_the_impulse_response() {
    // `ir`/`iresponse` convolves against a loaded sample instead of the
    // generated noise tail. A single-spike IR makes the reverb a pure delay
    // line, which is easy to spot.
    let spike_at = 4410; // 0.1s at 44.1kHz
    let mut data = vec![0.0f32; spike_at + 1];
    data[spike_at] = 1.0;
    let mut bank = SampleBank::new();
    bank.register(
        "spike",
        Arc::new(rudel_dsp::Sample {
            data,
            sample_rate: 44100.0,
        }),
    );

    let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
        .room(rudel_core::Value::F64(1.0))
        .dry(rudel_core::Value::F64(0.0))
        .size(rudel_core::Value::F64(0.2))
        .ctrl("ir", rudel_core::Value::Str("spike".into()));

    let frames = render_pattern_with_bank(&pat, 4.0, 1.0, bank);
    // The wet signal is the note delayed by the spike position, plus the
    // convolver's own latency.
    let want = (spike_at + Convolver::LATENCY) as f32 / 44100.0;
    let got = peak_time(&frames, 0);
    assert!(
        (got - want).abs() < 0.02,
        "the spike IR should place the wet signal at ~{want}s, got {got}s"
    );
}

/// `pat.lfo({...})` / `pat.env({...})` with a literal config.
fn modulate(pat: &Pattern, kind: &str, cfg: &[(&str, rudel_core::Value)]) -> Pattern {
    let config = cfg
        .iter()
        .map(|(k, v)| (k.to_string(), rudel_core::pure(v.clone())))
        .collect();
    rudel_core::modulate(pat, kind, config, rudel_core::pure(rudel_core::Value::Null))
}

/// The loudest-to-quietest ratio across an amplitude envelope, skipping the
/// first two windows (the note's own attack).
fn spread(v: &[f32]) -> f32 {
    let (lo, hi) = v[2..]
        .iter()
        .fold((f32::MAX, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    hi / lo.max(1e-9)
}

/// Per-window peak levels of `frames`, `windows` windows across the whole
/// buffer — a cheap amplitude envelope.
fn window_peaks(frames: &[f32], windows: usize) -> Vec<f32> {
    let w = frames.len() / windows;
    (0..windows)
        .map(|i| {
            frames[i * w..(i + 1) * w]
                .iter()
                .fold(0.0f32, |m, x| m.max(x.abs()))
        })
        .collect()
}

#[test]
fn an_lfo_modulates_the_gain_it_targets() {
    // `.gain(1).lfo({control:'gain', rate:8})`: depth defaults to 1, scaled
    // by the target's own value (1), and the LFO's dcoffset of -0.5 makes
    // the offset swing +/-0.5 — so the gain tremolos between 0.5 and 1.5.
    let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
        .gain(rudel_core::Value::F64(1.0));
    let modulated = modulate(
        &held,
        "lfo",
        &[
            ("control", rudel_core::Value::Str("gain".into())),
            ("rate", rudel_core::Value::F64(2.0)),
            ("shape", rudel_core::Value::Str("sine".into())),
        ],
    );

    // 32 windows over 0.9s is ~28ms each, well inside the 500ms LFO period.
    let flat = window_peaks(&render_pattern(&held, 1.0, 0.9), 32);
    let swept = window_peaks(&render_pattern(&modulated, 1.0, 0.9), 32);
    assert!(spread(&flat) < 1.2, "unmodulated gain should be steady");
    assert!(
        spread(&swept) > 2.5,
        "a gain LFO should swing the level ~3x ({})",
        spread(&swept)
    );
}

#[test]
fn an_lfo_defaults_to_the_control_before_it_in_the_chain() {
    // The documented example: `.lpf(500).lfo({rate:2})` sweeps the cutoff,
    // because a modulator with no explicit `control` targets whatever was
    // applied just before it.
    // The note sits at 440Hz and the cutoff sweeps 200..600Hz, so the
    // fundamental moves in and out of the passband.
    let saw = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("sawtooth".into())))
        .note(rudel_core::Value::Int(69))
        .cutoff(rudel_core::Value::F64(400.0));
    let modulated = modulate(&saw, "lfo", &[("rate", rudel_core::Value::F64(2.0))]);

    let flat = window_peaks(&render_pattern(&saw, 1.0, 0.9), 32);
    let swept = window_peaks(&render_pattern(&modulated, 1.0, 0.9), 32);
    assert!(spread(&flat) < 1.2, "a static cutoff should be steady");
    assert!(
        spread(&swept) > 2.0,
        "a cutoff LFO should sweep the level ({})",
        spread(&swept)
    );
}

#[test]
fn an_envelope_modulator_sweeps_its_target() {
    // `.gain(1).env({attack:0.5, sustain:1})` ramps the gain offset 0 -> 1
    // over half the note and holds it, so the level roughly doubles.
    let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
        .gain(rudel_core::Value::F64(1.0));
    let swept = modulate(
        &held,
        "env",
        &[
            ("attack", rudel_core::Value::F64(0.5)),
            ("sustain", rudel_core::Value::F64(1.0)),
        ],
    );
    let flat = window_peaks(&render_pattern(&held, 1.0, 0.9), 8);
    let swept = window_peaks(&render_pattern(&swept, 1.0, 0.9), 8);
    // Against the unmodulated note: still climbing early (the offset is
    // partway up its 0.5s attack), fully doubled once it reaches sustain.
    let early = swept[1] / flat[1];
    let late = swept[6] / flat[6];
    assert!(
        early < 1.6,
        "the envelope should still be climbing early ({early})"
    );
    assert!(
        late > 1.8,
        "the envelope should hold the gain at ~2x once sustained ({late})"
    );
}

#[test]
fn bmod_modulates_a_carrier_from_another_pattern_s_bus() {
    // The documented shape of `bmod`: one pattern is routed to a signal bus
    // and silenced with `dry(0)`, another reads that bus as a modulation
    // source. Here the modulator is a 2Hz sine, so it tremolos the carrier's
    // gain the way an LFO would — except the waveform comes from a pattern.
    let modulator = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("sine".into())))
        .freq(rudel_core::Value::F64(2.0))
        .bus(rudel_core::Value::Int(1))
        .dry(rudel_core::Value::F64(0.0));
    let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
        .gain(rudel_core::Value::F64(1.0));
    // No `control`: like `lfo`, it defaults to `gain`, applied just before.
    let carrier = modulate(&held, "bmod", &[("bus", rudel_core::Value::Int(1))]);

    // `dry(0)` and no sends: the modulator itself must be inaudible.
    let alone = render_pattern(&modulator, 1.0, 0.9);
    let level = alone.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(level < 1e-6, "a bus-only voice should be silent ({level})");

    let flat = window_peaks(&render_pattern(&held, 1.0, 0.9), 32);
    assert!(spread(&flat) < 1.2, "unmodulated gain should be steady");
    // Both orderings: the mixer has to render the sender before the carrier
    // whichever way round the stack puts them.
    let render =
        |pats: [Pattern; 2]| window_peaks(&render_pattern(&rudel_core::stack(&pats), 1.0, 0.9), 32);
    let swept = render([modulator.clone(), carrier.clone()]);
    assert!(
        spread(&swept) > 2.0,
        "the bus signal should swing the carrier's gain ({})",
        spread(&swept)
    );
    assert_eq!(
        swept,
        render([carrier, modulator]),
        "stack order must not change what the carrier reads"
    );
}

#[test]
fn s_bus_plays_back_what_another_pattern_sent_to_the_bus() {
    // The other half of what `bus` is for: `s("bus").n(1)` reads bus 1 as a
    // source, so a second pattern can run it through its own effects.
    let held = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)));
    let sender = held
        .clone()
        .bus(rudel_core::Value::Int(1))
        .dry(rudel_core::Value::F64(0.0));
    let player = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("bus".into())))
        .n(rudel_core::Value::Int(1));

    // Alone, the sender is silent and the player has nothing to play.
    for pat in [&sender, &player] {
        let level = render_pattern(pat, 1.0, 0.6)
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(level < 1e-6, "expected silence, got {level}");
    }

    // Together, the player reproduces the sender's signal. Both envelopes
    // are at sustain across this window, and `s("bus")`'s own is 1 there, so
    // what is left is its centre-pan gain of cos(pi/4).
    let direct = peak_between(&render_pattern(&held, 1.0, 0.6), 0.3, 0.5);
    let piped = peak_between(
        &render_pattern(&rudel_core::stack(&[sender, player]), 1.0, 0.6),
        0.3,
        0.5,
    );
    let ratio = piped / direct;
    assert!(
        (ratio - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.05,
        "the bus should play back at the source's level ({ratio})"
    );
}

fn rms(frames: &[f32]) -> f32 {
    (frames.iter().map(|x| x * x).sum::<f32>() / frames.len().max(1) as f32).sqrt()
}

#[test]
fn per_voice_filters_reach_the_fixed_recipe_voices() {
    // The drum, ZZFX, bytebeat and bus voices render from a fixed recipe
    // rather than from `VoiceParams`, so `lpf`/`hpf`/`bpf` used to pass
    // straight through them — in Strudel every one of these is a sample or a
    // node, and gets filtered like anything else. The filters themselves are
    // the same code the oscillator voice runs and are golden-tested there;
    // what this checks is that the chain is now *reached*.
    //
    // The probe has to suit the source: a bass drum is already almost
    // entirely below 200Hz, so only a high-pass moves it, while a hi-hat is
    // 7kHz noise a low-pass all but erases.
    for (name, control, freq, max_ratio) in [
        ("bd", "hcutoff", 2000.0, 0.1),
        ("hh", "cutoff", 200.0, 0.01),
        ("zzfx", "cutoff", 200.0, 0.5),
        ("bytebeat", "cutoff", 200.0, 0.7),
    ] {
        let plain = rudel_core::s(rudel_core::pure(rudel_core::Value::Str(name.into())));
        let filtered = plain.clone().ctrl(control, rudel_core::Value::F64(freq));
        let open = rms(&render_pattern(&plain, 1.0, 0.5));
        let closed = rms(&render_pattern(&filtered, 1.0, 0.5));
        assert!(open > 0.0, "{name} should make a sound");
        assert!(
            closed < open * max_ratio,
            "{name}: {control}({freq}) should cut it below {max_ratio}x ({open} -> {closed})"
        );
    }
}

#[test]
fn per_voice_filters_reach_a_bus_return() {
    // The point of `s("bus")`: run what another pattern sent you through
    // your own effects. A 440Hz saw on the bus, low-passed on the way out.
    let sender = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("saw".into())))
        .note(rudel_core::Value::Int(69))
        .bus(rudel_core::Value::Int(1))
        .dry(rudel_core::Value::F64(0.0));
    let player = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("bus".into())))
        .n(rudel_core::Value::Int(1));
    let dull = player.clone().cutoff(rudel_core::Value::F64(200.0));

    let level = |p: Pattern| {
        rms(&render_pattern(
            &rudel_core::stack(&[sender.clone(), p]),
            1.0,
            0.5,
        ))
    };
    let (open, closed) = (level(player), level(dull));
    assert!(open > 0.0, "the bus return should make a sound");
    assert!(
        closed < open * 0.3,
        "a 200Hz lowpass on the bus return should cut it ({open} -> {closed})"
    );
}

/// Peak level of `frames` over the time window `[from, to)` seconds.
fn peak_between(frames: &[f32], from: f32, to: f32) -> f32 {
    let idx = |t: f32| ((t * 44100.0) as usize).min(frames.len());
    frames[idx(from)..idx(to)]
        .iter()
        .fold(0.0f32, |m, x| m.max(x.abs()))
}

/// A held note on orbit 2, plus a silent ducker on orbit 1 that fires at
/// the half cycle and ducks the given targets.
fn duck_pattern(targets: Pattern, extra: impl Fn(Pattern) -> Pattern) -> Pattern {
    let held = |orbit: i64| {
        rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)))
            .orbit(rudel_core::Value::Int(orbit))
    };
    // `postgain(0)` silences the ducker itself, like Strudel's own examples,
    // so the measurement only sees the ducked orbit.
    let ducker = rudel_core::sequence(&[
        rudel_core::silence(),
        extra(
            rudel_core::s(rudel_core::pure(rudel_core::Value::Str("bd".into())))
                .orbit(rudel_core::Value::Int(1))
                .postgain(rudel_core::Value::F64(0.0))
                .duckorbit(targets),
        ),
    ]);
    rudel_core::stack(&[held(2), ducker])
}

#[test]
fn duckorbit_dips_the_target_orbit_and_recovers() {
    let pat = duck_pattern(rudel_core::pure(rudel_core::Value::Int(2)), |p| {
        p.duckattack(rudel_core::Value::F64(0.3))
    });
    let frames = render_pattern(&pat, 1.0, 1.0);
    let before = peak_between(&frames, 0.35, 0.49);
    let during = peak_between(&frames, 0.5, 0.55);
    let after = peak_between(&frames, 0.85, 0.99);
    assert!(before > 0.01, "the held note should be sounding ({before})");
    assert!(
        during < before * 0.2,
        "duckorbit(2) should dip orbit 2 ({during} vs {before})"
    );
    assert!(
        after > before * 0.8,
        "orbit 2 should recover after duckattack ({after} vs {before})"
    );
}

#[test]
fn duckdepth_zero_leaves_the_target_alone() {
    // floor = 1 - sqrt(0) = 1, so there is nothing to duck.
    let pat = duck_pattern(rudel_core::pure(rudel_core::Value::Int(2)), |p| {
        p.duckdepth(rudel_core::Value::F64(0.0))
    });
    let frames = render_pattern(&pat, 1.0, 1.0);
    let before = peak_between(&frames, 0.35, 0.49);
    let during = peak_between(&frames, 0.5, 0.55);
    assert!(
        during > before * 0.8,
        "duckdepth(0) should not duck ({during} vs {before})"
    );
}

#[test]
fn duck_control_lists_are_read_per_target() {
    // `duckorbit("2:3")` with `duckdepth("1:0")`: orbit 2 ducks fully,
    // orbit 3 not at all. Only orbit 2 carries the held note here, so a
    // per-target read is what makes it dip; a first-entry-for-all read
    // would too, so also check the reverse order.
    let targets = |a: i64, b: i64| {
        rudel_core::pure(rudel_core::Value::List(vec![
            rudel_core::Value::Int(a),
            rudel_core::Value::Int(b),
        ]))
    };
    let depths =
        rudel_core::Value::List(vec![rudel_core::Value::Int(1), rudel_core::Value::Int(0)]);
    let dip = |t: Pattern| {
        let pat = duck_pattern(t, |p| p.duckdepth(depths.clone()));
        let frames = render_pattern(&pat, 1.0, 1.0);
        (
            peak_between(&frames, 0.35, 0.49),
            peak_between(&frames, 0.5, 0.55),
        )
    };
    // Orbit 2 is first, so it takes depth 1 and is ducked.
    let (before, during) = dip(targets(2, 3));
    assert!(during < before * 0.2, "orbit 2 first: {during} vs {before}");
    // Orbit 2 is second, so it takes depth 0 and is left alone.
    let (before, during) = dip(targets(3, 2));
    assert!(
        during > before * 0.8,
        "orbit 2 second: {during} vs {before}"
    );
}

#[test]
fn orbits_have_independent_effect_buses() {
    // A heavy `djf` lowpass on orbit 2 must not touch orbit 1. Both orbits
    // play the same bright note; only the filtered one should lose level.
    let note = |orbit: i64, djf: Option<f64>| {
        let p = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(90)))
            .orbit(rudel_core::Value::Int(orbit));
        match djf {
            Some(v) => p.djf(rudel_core::Value::F64(v)),
            None => p,
        }
    };
    let peak = |pat: &Pattern| {
        render_pattern(pat, 4.0, 0.3)
            .iter()
            .fold(0.0f32, |m, x| m.max(x.abs()))
    };

    let clean = peak(&note(1, None));
    // Same note on orbit 2 with the DJ filter fully closed.
    let filtered = peak(&note(2, Some(0.0)));
    assert!(
        filtered < clean * 0.5,
        "djf(0) should cut the note ({filtered} vs {clean})"
    );
    // Stacking them: orbit 1 keeps its level even though orbit 2 is filtered.
    let both = peak(&rudel_core::stack(&[note(1, None), note(2, Some(0.0))]));
    assert!(
        both >= clean * 0.9,
        "orbit 2's djf must not affect orbit 1 ({both} vs {clean})"
    );
}

#[test]
fn cut_group_chokes_the_previous_voice() {
    // Two sustained notes in cut group 1, the second a little later. After
    // the second starts, the first should be choked to silence within the
    // ~10ms fade, leaving only one voice's worth of energy.
    let (tx, rx) = mpsc::channel::<NoteEvent>();
    let mut mixer = test_mixer(rx);
    // A long held saw so the voice is still audible when the next one cuts it.
    let held = |onset: f64| NoteEvent {
        onset_seconds: onset,
        spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
            &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
            10.0,
        ))),
        fx: rudel_dsp::PostFx::default(),
        cut: Some(1),
        send: OrbitSend::default(),
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    };
    tx.send(held(0.0)).unwrap();
    tx.send(held(0.2)).unwrap();
    drop(tx);

    // Render up to just before the second onset: only voice A is active.
    for _ in 0..((0.2 * 44100.0) as usize) {
        mixer.render_frame();
    }
    assert_eq!(mixer.active.len(), 1);
    // Render past the choke fade (~10ms). The choked first voice is dropped,
    // leaving just the second voice.
    for _ in 0..((CHOKE_SECS * 44100.0) as usize + 64) {
        mixer.render_frame();
    }
    assert_eq!(mixer.active.len(), 1, "the choked voice should be gone");
    assert!(
        mixer.active[0].choke_gain.is_none(),
        "the surviving voice is the new one, not choking"
    );
}

#[test]
fn block_render_matches_frame_render_across_onsets() {
    // The sub-block splitting in `render_block` must be sample-for-sample
    // equivalent to stepping `render_frame`, including onsets that land
    // partway through a buffer. Drive two identical mixers with the same
    // staggered notes — one in a single 256-frame block, one frame by frame —
    // and confirm they agree. The notes are plain synths (no post-fx), so the
    // default `process_block` is a `tick` loop and the two paths are exact.
    let note = |onset: f64| NoteEvent {
        onset_seconds: onset,
        spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams::from_controls(
            &rudel_core::to_control_map(&rudel_core::Value::Str("sawtooth".into())),
            10.0,
        ))),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send: OrbitSend::default(),
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    };
    // Onsets at frames 0, ~37 and ~150 (44.1kHz) force mid-buffer splits.
    let onsets = [0.0, 37.0 / 44100.0, 150.0 / 44100.0];

    let (tx_a, rx_a) = mpsc::channel::<NoteEvent>();
    let (tx_b, rx_b) = mpsc::channel::<NoteEvent>();
    for &o in &onsets {
        tx_a.send(note(o)).unwrap();
        tx_b.send(note(o)).unwrap();
    }
    drop(tx_a);
    drop(tx_b);

    let mut by_block = test_mixer(rx_a);
    let mut by_frame = test_mixer(rx_b);

    let n = 256;
    let mut block_out = vec![(0.0f32, 0.0f32); n];
    by_block.render_block(&mut block_out);

    let mut max_diff = 0.0f32;
    for frame in block_out {
        let (fl, fr) = by_frame.render_frame();
        max_diff = max_diff.max((frame.0 - fl).abs()).max((frame.1 - fr).abs());
    }
    assert!(
        max_diff < 1e-6,
        "block render diverged from frame render (max diff {max_diff:e})"
    );
    assert_eq!(by_block.active.len(), by_frame.active.len(), "voice counts");
}

#[test]
fn mixer_renders_a_scheduled_note() {
    // Drive a Mixer directly (no audio device) and confirm a scheduled
    // note produces non-silent output once its onset passes.
    let (tx, rx) = mpsc::channel::<NoteEvent>();
    let mut mixer = test_mixer(rx);
    let pat = rudel_core::note(rudel_core::pure(rudel_core::Value::Int(69)));
    let events = collect_events(&pat, 1.0, 0.0, 1.0, &SampleBank::new());
    for ev in events {
        tx.send(ev).unwrap();
    }
    drop(tx);

    let mut peak = 0.0f32;
    for _ in 0..4410 {
        let (l, _r) = mixer.render_frame();
        peak = peak.max(l.abs());
    }
    assert!(peak > 0.0, "scheduled note should produce sound");
}

#[test]
fn master_volume_scales_the_final_mix() {
    struct ConstVoice;

    impl VoiceLike for ConstVoice {
        fn tick(&mut self) -> (f32, f32) {
            (1.0, 1.0)
        }

        fn is_done(&self) -> bool {
            false
        }
    }

    let (_tx, rx) = mpsc::channel::<NoteEvent>();
    let volume = test_volume(0.5);
    let mut mixer = test_mixer_with_volume(rx, volume.clone());
    mixer.active.push(ActiveVoice {
        voice: Box::new(ConstVoice),
        tags: Vec::new(),
        cut: None,
        send: OrbitSend::default(),
        choke_gain: None,
    });

    assert_eq!(mixer.render_frame(), (0.5, 0.5));
    store_f64(&volume, 2.0);
    assert_eq!(mixer.render_frame(), (2.0, 2.0));
}

#[test]
fn scheduler_window_continues_from_the_cursor() {
    // cps=1, now=10s, lookahead 0.1 -> current 10.0, target 10.1.
    let clock = Clock::new(1.0);
    let (begin, end) =
        next_schedule_window(10.08, clock.cycle_at(10.0), clock.cycle_at(10.1)).unwrap();
    assert!((begin - 10.08).abs() < 1e-9);
    assert!((end - 10.1).abs() < 1e-9);
}

#[test]
fn scheduler_window_snaps_to_current_when_cursor_is_stale() {
    // A cursor left behind the live window (e.g. after a gap) snaps forward
    // to current_cycle so no time is double-scheduled.
    let (begin, end) = next_schedule_window(2.0, 5.0, 5.05).unwrap();
    assert!((begin - 5.0).abs() < 1e-9);
    assert!((end - 5.05).abs() < 1e-9);
}

#[test]
fn scheduler_window_waits_when_cursor_is_ahead_of_the_window() {
    // A cursor past the window (e.g. a cps drop shrank the lookahead) must
    // not re-schedule already-covered cycles — the window is empty.
    assert!(next_schedule_window(20.0, 5.0, 5.05).is_none());
}

#[test]
fn live_cps_change_does_not_double_schedule_or_jump() {
    // Stable at cps=1; the scheduler has reached cycle ~10.1 by t=10s.
    let mut clock = Clock::new(1.0);
    let scheduled = 10.1;
    // Halving cps at t=10 re-anchors: the cycle position is unchanged (no
    // jump), and the cycle lookahead shrinks to 0.05.
    clock.set_cps(10.0, 0.5);
    assert!(
        (clock.cycle_at(10.0) - 10.0).abs() < 1e-9,
        "cps change must not jump cycles"
    );
    // Right after the change the cursor (10.1) is past the new target
    // (10.05), so nothing is scheduled — no double-trigger.
    assert!(next_schedule_window(scheduled, clock.cycle_at(10.0), clock.cycle_at(10.1)).is_none());
    // Once time advances so the cursor enters the window, scheduling
    // continues seamlessly from it (cycle 10.1 falls at t=10.2s).
    let (begin, _end) =
        next_schedule_window(scheduled, clock.cycle_at(10.2), clock.cycle_at(10.3)).unwrap();
    assert!((begin - scheduled).abs() < 1e-9);
}

#[test]
fn a_pattern_cps_control_retunes_the_transport_from_the_window_end() {
    // Replicates one pass of the scheduler loop: query a window, then apply the
    // `cps` an event in it asked for. The change lands at the *end* of the
    // window just scheduled, so already-dispatched onsets keep their timing and
    // the cycle counter is continuous into the next window.
    let bank = SampleBank::new();
    let pat = rudel_core::s(rudel_core::pure(rudel_core::Value::Str("bd".into())))
        .ctrl("cps", rudel_core::Value::F64(2.0));

    let mut clock = Clock::new(1.0);
    let (now, lookahead) = (10.0, 0.1);
    let target_cycle = clock.cycle_at(now + lookahead);
    let (events, cps_change) = crate::collect_events_at(&pat, &clock, 10.0, target_cycle, &bank);
    assert_eq!(cps_change, Some(2.0));
    // The onset was timed against the old clock and is already on its way.
    let onset = events[0].onset_seconds;

    clock.set_cps(clock.seconds_at(target_cycle), 2.0);
    // Nothing already scheduled moved, and the cycle counter did not jump:
    // cycle 10.1 still falls at t=10.1s, where the old clock put it.
    assert!((clock.seconds_at(target_cycle) - 10.1).abs() < 1e-9);
    assert!((onset - 10.0).abs() < 1e-9);
    // From there time runs at the new rate: +1s is now +2 cycles, not +1.
    assert!((clock.cycle_at(11.1) - (target_cycle + 2.0)).abs() < 1e-9);
    // And the next pass (20ms later) continues from the cursor rather than
    // re-covering it or skipping ahead.
    let next = now + 0.02;
    let (begin, end) = next_schedule_window(
        target_cycle,
        clock.cycle_at(next),
        clock.cycle_at(next + lookahead),
    )
    .unwrap();
    assert!((begin - target_cycle).abs() < 1e-9);
    // The window now covers twice the cycles per second of wall time.
    assert!((end - clock.cycle_at(next + lookahead)).abs() < 1e-9);
    assert!(end > begin);
}

// --- send routing arithmetic ------------------------------------------------
//
// `mix_sub_block` is where every voice's output is split between its orbit's
// dry, reverb and delay accumulators, scaled by `dry`/`room`/`delay`. The
// existing tests check that effects *happen*; these check the levels, because a
// send scaled wrongly still sounds like a mix.

/// A synth voice at a known level, routed with the given sends.
fn routed_event(send: OrbitSend) -> NoteEvent {
    NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams {
            waveform: rudel_dsp::Waveform::Sine,
            freq: 441.0,
            duration: 10.0,
            adsr: rudel_dsp::Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            ..Default::default()
        })),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send,
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    }
}

fn peak_of(out: &[(f32, f32)]) -> f32 {
    out.iter()
        .fold(0.0f32, |m, (l, r)| m.max(l.abs()).max(r.abs()))
}

fn render(send: OrbitSend, n: usize) -> Vec<(f32, f32)> {
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(routed_event(send));
    let mut out = vec![(0.0f32, 0.0f32); n];
    mixer.render_block(&mut out);
    out
}

#[test]
fn dry_scales_the_direct_signal() {
    // `dry` is a straight gain on the direct path, so halving it halves the
    // output when nothing else is routed.
    let full = peak_of(&render(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        4096,
    ));
    let half = peak_of(&render(
        OrbitSend {
            dry: 0.5,
            ..Default::default()
        },
        4096,
    ));
    assert!(full > 0.0, "a dry voice should be audible");
    assert!(
        (half - full * 0.5).abs() < full * 0.05,
        "dry 0.5 should halve the direct signal: {full:.4} -> {half:.4}"
    );

    // `dry(0)` with no wet sends leaves silence — this is what makes a voice a
    // pure modulation source.
    let none = peak_of(&render(
        OrbitSend {
            dry: 0.0,
            ..Default::default()
        },
        4096,
    ));
    assert!(
        none < 1e-6,
        "dry 0 with no sends should be silent, got {none}"
    );
}

#[test]
fn the_wet_sends_are_taken_before_the_dry_gain() {
    // The reverb and delay sends read the voice's output *pre*-dry, so `dry(0)`
    // still feeds them — otherwise `room` would silently do nothing whenever a
    // voice was routed fully wet.
    let wet_only = peak_of(&render(
        OrbitSend {
            dry: 0.0,
            room: 0.8,
            ..Default::default()
        },
        16384,
    ));
    assert!(
        wet_only > 1e-4,
        "dry 0 with room 0.8 should still produce reverb, got {wet_only}"
    );

    // More send, more level.
    let quiet = peak_of(&render(
        OrbitSend {
            dry: 0.0,
            room: 0.2,
            ..Default::default()
        },
        16384,
    ));
    assert!(
        wet_only > quiet,
        "a bigger room send should be louder: {quiet:.5} vs {wet_only:.5}"
    );

    // The delay send behaves the same way.
    let delayed = peak_of(&render(
        OrbitSend {
            dry: 0.0,
            delay: 0.8,
            ..Default::default()
        },
        16384,
    ));
    assert!(
        delayed > 1e-4,
        "dry 0 with delay 0.8 should still produce delay, got {delayed}"
    );
}

#[test]
fn a_zero_send_leaves_no_tail_where_a_room_send_does() {
    // The `room > 0.0` / `delay > 0.0` guards skip the accumulation entirely.
    // Compared on a *short* note so what is measured after it is a tail rather
    // than the note still sounding.
    let tail_after = |send: OrbitSend| {
        let mut mixer = OfflineMixer::new(44100.0);
        let mut ev = routed_event(send);
        ev.spec = rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams {
            waveform: rudel_dsp::Waveform::Sine,
            freq: 441.0,
            duration: 0.02,
            adsr: rudel_dsp::Adsr {
                attack: 0.001,
                decay: 0.001,
                sustain: 1.0,
                release: 0.005,
            },
            ..Default::default()
        }));
        mixer.schedule(ev);
        let mut out = vec![(0.0f32, 0.0f32); 16384];
        mixer.render_block(&mut out);
        // Well past the note's own end (0.025s = ~1100 samples).
        peak_of(&out[4000..])
    };

    let dry_only = tail_after(OrbitSend {
        dry: 1.0,
        room: 0.0,
        delay: 0.0,
        ..Default::default()
    });
    let with_room = tail_after(OrbitSend {
        dry: 1.0,
        room: 0.8,
        ..Default::default()
    });
    assert!(
        dry_only < 1e-4,
        "no sends should leave no tail, got {dry_only}"
    );
    assert!(
        with_room > dry_only * 10.0,
        "a room send should leave one: {dry_only:.6} vs {with_room:.6}"
    );
}

#[test]
fn a_voice_is_routed_to_its_own_orbit() {
    // Orbits are independent buses; two voices on different orbits must both
    // be heard, and a voice on an orbit that does not exist yet has one created
    // rather than silently disappearing.
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(routed_event(OrbitSend {
        orbit: 7,
        dry: 1.0,
        ..Default::default()
    }));
    let mut out = vec![(0.0f32, 0.0f32); 4096];
    mixer.render_block(&mut out);
    assert!(
        peak_of(&out) > 0.0,
        "a voice on a fresh orbit should still be heard"
    );
}

#[test]
fn a_finished_voice_is_dropped_from_the_active_list() {
    // `retain_mut` returns `!is_done()`, so a voice that has ended stops being
    // rendered. Inverted, finished voices would accumulate forever.
    let mut mixer = OfflineMixer::new(44100.0);
    let mut ev = routed_event(OrbitSend {
        dry: 1.0,
        ..Default::default()
    });
    ev.spec = rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams {
        waveform: rudel_dsp::Waveform::Sine,
        freq: 441.0,
        duration: 0.01,
        adsr: rudel_dsp::Adsr {
            attack: 0.001,
            decay: 0.001,
            sustain: 1.0,
            release: 0.01,
        },
        ..Default::default()
    }));
    mixer.schedule(ev);
    let mut out = vec![(0.0f32, 0.0f32); 4096];
    mixer.render_block(&mut out);
    assert!(peak_of(&out) > 0.0, "it should sound first");
    assert_eq!(mixer.active_len(), 0, "and then be dropped");

    // Rendering on with nothing active is silence, not a panic.
    let mut more = vec![(0.0f32, 0.0f32); 512];
    mixer.render_block(&mut more);
    assert!(peak_of(&more) < 1e-6);
}
