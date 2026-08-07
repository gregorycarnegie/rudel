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
fn a_widget_tap_carries_the_mono_sum_of_every_voice_wearing_its_tag() {
    // The tap is fed the mean of the voice's two channels, summed across every
    // voice with the tag — so an off-centre voice must not arrive at its left
    // channel's level, and a second tagged voice must add to the first rather
    // than replace it.
    let render = |n: usize, tagged: usize| -> (Vec<(f32, f32)>, Vec<f32>) {
        let mut mixer = OfflineMixer::new(44100.0);
        let tap = mixer.taps().get_or_create("w");
        for i in 0..tagged {
            let mut ev = panned_event(
                OrbitSend {
                    dry: 1.0,
                    ..Default::default()
                },
                0.9,
            );
            ev.tags = vec!["w".to_string()];
            // Two voices an octave apart, so their sum is not just twice one.
            if let rudel_dsp::VoiceSpec::Synth(p) = &mut ev.spec {
                p.freq *= (i + 1) as f32;
            }
            mixer.schedule(ev);
        }
        let mut out = vec![(0.0f32, 0.0f32); n];
        mixer.render_block(&mut out);
        let mut got = vec![0.0f32; n];
        tap.latest(&mut got);
        (out, got)
    };

    // One voice: the tap is the mean of its channels, which for a voice panned
    // off centre is neither channel.
    let (out, tap) = render(256, 1);
    for (i, ((l, r), t)) in out.iter().zip(&tap).enumerate() {
        assert!(
            (t - (l + r) * 0.5).abs() < 1e-6,
            "frame {i}: tap {t} should be the mean of {l} and {r}"
        );
    }
    assert!(
        out.iter().any(|(l, r)| (l - r).abs() > 1e-4),
        "the voice must be off centre for that to mean anything"
    );

    // Two voices: the tap sums them, so it tracks the (also summed) master mix.
    let (both_out, both_tap) = render(256, 2);
    for (i, ((l, r), t)) in both_out.iter().zip(&both_tap).enumerate() {
        assert!(
            (t - (l + r) * 0.5).abs() < 1e-6,
            "frame {i}: two tagged voices should sum in the tap"
        );
    }
    let one = tap.iter().map(|x| x.abs()).sum::<f32>();
    let two = both_tap.iter().map(|x| x.abs()).sum::<f32>();
    assert!(
        two > one * 1.2,
        "the second voice should add: {one} -> {two}"
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
    let (o0, r0) = d.process(1.0, 1.0); // impulse into both channels
    assert_eq!((o0, r0), (0.0, 0.0), "no output before the delay time");
    let tail: Vec<(f32, f32)> = (0..25).map(|_| d.process(0.0, 0.0)).collect();
    // The impulse re-emerges after 10 samples, and the feedback path puts a
    // half-level copy 10 samples after that — *the same way up*. Fed back
    // inverted, a delay line turns every echo into a comb notch.
    assert_eq!(tail[9], (1.0, 1.0), "first echo, at the delay time");
    assert_eq!(
        tail[19],
        (0.5, 0.5),
        "second echo, at half through feedback"
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
// `mix_sub_block` splits every voice's output between its orbit's dry, reverb
// and delay accumulators. These compare *sample for sample* against a rendering
// of the same voice at a known routing, rather than comparing peaks: a peak
// taken over `abs()` is blind to a sign flip, and a tolerance wide enough for
// float drift is wide enough to hide a scaling error.

/// A sine voice at a known level, routed with the given sends.
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

fn render(send: OrbitSend, n: usize) -> Vec<(f32, f32)> {
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(routed_event(send));
    let mut out = vec![(0.0f32, 0.0f32); n];
    mixer.render_block(&mut out);
    out
}

fn peak_of(out: &[(f32, f32)]) -> f32 {
    out.iter()
        .fold(0.0f32, |m, (l, r)| m.max(l.abs()).max(r.abs()))
}

/// Largest signed deviation of `got` from `want * scale`, sample for sample.
/// Signed on purpose: this is what catches an accumulation that subtracts.
fn worst_scaled_diff(got: &[(f32, f32)], want: &[(f32, f32)], scale: f32) -> f32 {
    got.iter().zip(want).fold(0.0f32, |m, (g, w)| {
        m.max((g.0 - w.0 * scale).abs())
            .max((g.1 - w.1 * scale).abs())
    })
}

#[test]
fn dry_scales_the_direct_signal_sample_for_sample() {
    let full = render(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        4096,
    );
    let level = peak_of(&full);
    assert!(level > 0.01, "a dry voice should be audible, got {level}");

    // Every sample of the half-dry rendering is exactly half of the full one —
    // which a sign flip or a swapped operator cannot satisfy.
    for scale in [0.5f32, 0.25, 0.75] {
        let scaled = render(
            OrbitSend {
                dry: scale,
                ..Default::default()
            },
            4096,
        );
        let worst = worst_scaled_diff(&scaled, &full, scale);
        assert!(
            worst < level * 1e-3,
            "dry {scale} should scale every sample: worst deviation {worst:.6}              against a {level:.4} signal"
        );
    }

    // ...and the direct path is genuinely positive-going, not an inverted copy.
    let first = full
        .iter()
        .find(|(l, _)| l.abs() > level * 0.5)
        .expect("some loud sample");
    let scaled = render(
        OrbitSend {
            dry: 0.5,
            ..Default::default()
        },
        4096,
    );
    let same = scaled
        .iter()
        .zip(&full)
        .find(|((l, _), (fl, _))| fl.abs() > level * 0.5 && l.abs() > 0.0)
        .expect("a matching sample");
    assert!(
        same.0.0.signum() == same.1.0.signum(),
        "scaling must not invert the signal ({first:?})"
    );

    // `dry(0)` with no wet sends is exactly silence.
    let none = render(
        OrbitSend {
            dry: 0.0,
            ..Default::default()
        },
        4096,
    );
    assert!(
        none.iter().all(|(l, r)| *l == 0.0 && *r == 0.0),
        "dry 0 with no sends should be exactly silent"
    );
}

#[test]
fn the_wet_sends_are_taken_before_the_dry_gain() {
    // The reverb and delay sends read the voice's output *pre*-dry, so `dry(0)`
    // still feeds them — otherwise `room` would silently do nothing on any
    // fully-wet voice, which is how a voice is made a pure modulation source.
    let room_only = render(
        OrbitSend {
            dry: 0.0,
            room: 0.8,
            ..Default::default()
        },
        16384,
    );
    assert!(
        peak_of(&room_only) > 1e-4,
        "dry 0 with room 0.8 should still produce reverb"
    );

    // The send is a gain: a quarter of the send is a quarter of the wet signal,
    // sample for sample.
    let quarter = render(
        OrbitSend {
            dry: 0.0,
            room: 0.2,
            ..Default::default()
        },
        16384,
    );
    let level = peak_of(&room_only);
    let worst = worst_scaled_diff(&quarter, &room_only, 0.25);
    assert!(
        worst < level * 1e-3,
        "room 0.2 should be a quarter of room 0.8 everywhere: worst {worst:.6}          against {level:.4}"
    );

    let delay_only = render(
        OrbitSend {
            dry: 0.0,
            delay: 0.8,
            ..Default::default()
        },
        16384,
    );
    assert!(
        peak_of(&delay_only) > 1e-4,
        "dry 0 with delay 0.8 should still produce delay"
    );
    // Reverb and delay are separate paths: neither reproduces the other.
    let cross = worst_scaled_diff(&delay_only, &room_only, 1.0);
    assert!(
        cross > level * 0.1,
        "the delay send should not render as the reverb send"
    );
}

#[test]
fn the_dry_and_wet_paths_sum_rather_than_replace() {
    // With both routed, the output is the sum of the two renderings — that is
    // what pins `+=` in the accumulation loops against a `-=` or a `=`.
    let dry = render(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        16384,
    );
    let wet = render(
        OrbitSend {
            dry: 0.0,
            room: 0.6,
            ..Default::default()
        },
        16384,
    );
    let both = render(
        OrbitSend {
            dry: 1.0,
            room: 0.6,
            ..Default::default()
        },
        16384,
    );
    let level = peak_of(&both);
    let worst = both
        .iter()
        .zip(dry.iter().zip(&wet))
        .fold(0.0f32, |m, (b, (d, w))| {
            m.max((b.0 - (d.0 + w.0)).abs())
                .max((b.1 - (d.1 + w.1)).abs())
        });
    assert!(
        worst < level * 1e-3,
        "dry + wet should sum: worst deviation {worst:.6} against {level:.4}"
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

// --- the choked-voice path --------------------------------------------------
//
// A voice in a `cut` group is choked when a new one in the same group starts:
// instead of stopping, it fades over CHOKE_SECS and is dropped once silent.
// That is a *separate* accumulation loop from the normal one — same routing,
// but per-sample gain — and nothing above reaches it, because none of those
// voices are in a cut group.

fn cut_event(cut: i32, send: OrbitSend) -> NoteEvent {
    let mut ev = routed_event(send);
    ev.cut = Some(cut);
    ev
}

#[test]
fn a_choked_voice_fades_out_instead_of_stopping() {
    // Two voices in the same cut group: starting the second chokes the first,
    // which then ramps down over CHOKE_SECS (10ms) rather than cutting hard.
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(cut_event(
        1,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    let mut warm = vec![(0.0f32, 0.0f32); 2048];
    mixer.render_block(&mut warm);
    assert_eq!(mixer.active_len(), 1, "the first voice is playing");
    let before = peak_of(&warm);
    assert!(before > 0.01, "and audible");

    // The second voice in the group chokes the first.
    mixer.schedule(cut_event(
        1,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    let mut out = vec![(0.0f32, 0.0f32); 2048];
    mixer.render_block(&mut out);
    assert_eq!(
        mixer.active_len(),
        1,
        "the choked voice should be gone and only the new one left"
    );
    // The fade is 10ms, well inside this block, and it is a fade rather than a
    // cut: the block still carries audio.
    assert!(peak_of(&out) > 0.0, "the new voice keeps sounding");
}

#[test]
fn a_choked_voice_is_routed_through_the_same_sends() {
    // The choke branch re-implements the dry/room/delay split with a per-sample
    // gain. Routing a choked voice fully wet still has to reach the reverb, the
    // same way the normal path does.
    let choked_tail = |send: OrbitSend| {
        let mut mixer = OfflineMixer::new(44100.0);
        mixer.schedule(cut_event(2, send));
        let mut warm = vec![(0.0f32, 0.0f32); 1024];
        mixer.render_block(&mut warm);
        // Choke it, then let the tail run.
        mixer.schedule(NoteEvent {
            onset_seconds: 0.0,
            spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams {
                waveform: rudel_dsp::Waveform::Sine,
                freq: 441.0,
                duration: 0.001,
                ..Default::default()
            })),
            fx: rudel_dsp::PostFx::default(),
            cut: Some(2),
            send: OrbitSend {
                dry: 0.0,
                ..Default::default()
            },
            duck: Vec::new(),
            mods: Default::default(),
            tags: Vec::new(),
        });
        let mut out = vec![(0.0f32, 0.0f32); 16384];
        mixer.render_block(&mut out);
        peak_of(&out[8000..])
    };

    let dry_choked = choked_tail(OrbitSend {
        dry: 1.0,
        ..Default::default()
    });
    let wet_choked = choked_tail(OrbitSend {
        dry: 0.0,
        room: 0.9,
        ..Default::default()
    });
    assert!(
        wet_choked > dry_choked * 5.0,
        "a choked voice's room send should still feed the reverb: \
         dry {dry_choked:.6} vs wet {wet_choked:.6}"
    );
}

#[test]
fn a_choked_voice_is_dropped_once_it_has_faded() {
    // The fade decrements by `1 / (sample_rate * CHOKE_SECS)` per sample and
    // returns false at zero. If the step were wrong in either direction the
    // voice would either click off or linger forever.
    let mut mixer = OfflineMixer::new(44100.0);
    for _ in 0..3 {
        mixer.schedule(cut_event(
            3,
            OrbitSend {
                dry: 1.0,
                ..Default::default()
            },
        ));
        let mut out = vec![(0.0f32, 0.0f32); 1024];
        mixer.render_block(&mut out);
    }
    // Each new voice chokes the previous one, and 1024 samples at 44.1kHz is
    // more than the 10ms fade, so only the newest survives each time.
    assert_eq!(mixer.active_len(), 1, "choked voices should not accumulate");
}

// --- the signal-bus send ----------------------------------------------------

#[test]
fn the_bus_send_is_additional_to_the_orbit_routing() {
    // `bus` sends the post-fx output to a numbered signal bus *on top of* the
    // orbit routing, so `dry(0).bus(n)` is a pure modulation source: silent at
    // the master, but audible to a voice reading that bus.
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(routed_event(OrbitSend {
        dry: 0.0,
        bus: Some(1),
        busgain: 1.0,
        ..Default::default()
    }));
    // A reader on the same bus.
    mixer.schedule(NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Bus(rudel_dsp::BusParams {
            bus: 1,
            adsr: rudel_dsp::Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            duration: 10.0,
            gain: 1.0,
            pan: 0.5,
            filters: Default::default(),
        }),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send: OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    });
    let mut out = vec![(0.0f32, 0.0f32); 4096];
    mixer.render_block(&mut out);
    assert!(
        peak_of(&out) > 1e-3,
        "the bus reader should hear the sender even though it is dry 0"
    );

    // Without the send there is nothing on the bus, so the reader is silent.
    let mut quiet = OfflineMixer::new(44100.0);
    quiet.schedule(routed_event(OrbitSend {
        dry: 0.0,
        ..Default::default()
    }));
    quiet.schedule(NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Bus(rudel_dsp::BusParams {
            bus: 1,
            adsr: rudel_dsp::Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            duration: 10.0,
            gain: 1.0,
            pan: 0.5,
            filters: Default::default(),
        }),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send: OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    });
    let mut out2 = vec![(0.0f32, 0.0f32); 4096];
    quiet.render_block(&mut out2);
    assert!(
        peak_of(&out2) < 1e-6,
        "with nothing sent to the bus the reader stays silent"
    );
}

/// Render a voice that gets choked part-way through, so the per-sample fade
/// branch in `mix_sub_block` is the one doing the accumulation. Deterministic:
/// the offline mixer delivers scheduled events on the next render call, so two
/// runs with identical scheduling produce identical output.
fn render_choked(send: OrbitSend, n: usize) -> Vec<(f32, f32)> {
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(cut_event(9, send));
    // Let it start, then choke it with a silent voice in the same group so the
    // only thing in the output is the fading first voice.
    let mut warm = vec![(0.0f32, 0.0f32); 256];
    mixer.render_block(&mut warm);
    mixer.schedule(NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Synth(Box::new(rudel_dsp::VoiceParams {
            waveform: rudel_dsp::Waveform::Sine,
            freq: 441.0,
            duration: 0.0001,
            gain: 0.0,
            ..Default::default()
        })),
        fx: rudel_dsp::PostFx::default(),
        cut: Some(9),
        send: OrbitSend {
            dry: 0.0,
            ..Default::default()
        },
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    });
    let mut out = vec![(0.0f32, 0.0f32); n];
    mixer.render_block(&mut out);
    out
}

#[test]
fn the_choked_path_scales_its_sends_the_same_way() {
    // The fade branch re-implements the dry/room/delay split with a per-sample
    // gain, so it needs its own level checks — the earlier ones only reach the
    // ordinary branch, and bookkeeping assertions (did it drop?) say nothing
    // about what it accumulated on the way down.
    let full = render_choked(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        2048,
    );
    let level = peak_of(&full);
    assert!(
        level > 1e-3,
        "a fading voice should still be audible, got {level}"
    );

    for scale in [0.5f32, 0.25] {
        let scaled = render_choked(
            OrbitSend {
                dry: scale,
                ..Default::default()
            },
            2048,
        );
        let worst = worst_scaled_diff(&scaled, &full, scale);
        assert!(
            worst < level * 1e-3,
            "a choked voice's dry {scale} should scale every sample: \
             worst {worst:.6} against {level:.4}"
        );
    }

    // The fade descends: the signal is quieter later in the ramp than at its
    // start. An accumulation that subtracted, or a step of the wrong sign,
    // would not produce a monotone envelope.
    let env: Vec<f32> = full
        .chunks(64)
        .map(|c| c.iter().fold(0.0f32, |m, (l, _)| m.max(l.abs())))
        .collect();
    let first = env[0];
    let later = env[env.len().min(8) - 1];
    assert!(
        later < first,
        "the choke should fade downward: {first:.5} -> {later:.5}"
    );

    // ...and it reaches silence rather than levelling off.
    assert!(
        env.last().copied().unwrap_or(1.0) < first * 0.01,
        "the fade should reach silence within the block"
    );
}

#[test]
fn a_choked_voices_wet_sends_scale_like_its_dry_one() {
    // Same routing split, same per-sample gain — so a quarter of the room send
    // is a quarter of the wet signal here too.
    let loud = render_choked(
        OrbitSend {
            dry: 0.0,
            room: 0.8,
            ..Default::default()
        },
        8192,
    );
    let level = peak_of(&loud);
    assert!(level > 1e-5, "a choked voice should still feed the reverb");

    let quiet = render_choked(
        OrbitSend {
            dry: 0.0,
            room: 0.2,
            ..Default::default()
        },
        8192,
    );
    let worst = worst_scaled_diff(&quiet, &loud, 0.25);
    assert!(
        worst < level * 1e-3,
        "room 0.2 should be a quarter of room 0.8 while choked: \
         worst {worst:.7} against {level:.5}"
    );

    // The delay send is its own path here too.
    let delayed = render_choked(
        OrbitSend {
            dry: 0.0,
            delay: 0.8,
            ..Default::default()
        },
        8192,
    );
    assert!(
        peak_of(&delayed) > 1e-5,
        "a choked voice's delay send should reach the delay"
    );
    assert!(
        worst_scaled_diff(&delayed, &loud, 1.0) > level * 0.1,
        "delay and reverb stay separate paths while choking"
    );
}

#[test]
fn the_choked_and_open_paths_agree_at_full_gain() {
    // The fade starts at 1.0, so the first sample a choked voice contributes is
    // the same one it would have contributed unchoked. That pins the choke
    // branch's routing against the ordinary branch rather than against itself.
    let choked = render_choked(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        64,
    );
    let mut open_mixer = OfflineMixer::new(44100.0);
    open_mixer.schedule(cut_event(
        11,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    let mut warm = vec![(0.0f32, 0.0f32); 256];
    open_mixer.render_block(&mut warm);
    let mut open = vec![(0.0f32, 0.0f32); 64];
    open_mixer.render_block(&mut open);

    // One sample in, the fade has only stepped once (1/441 of the way), so the
    // two renderings are within a fraction of a percent.
    let level = peak_of(&open).max(1e-9);
    assert!(
        (choked[0].0 - open[0].0).abs() < level * 0.02,
        "the choke should begin at unity: {} vs {}",
        choked[0].0,
        open[0].0
    );
}

// --- the two channels are the same signal -----------------------------------
//
// Every buffer in the mix path is a left/right pair carried through in
// lockstep: the dry / room / delay accumulators, the delay line, the convolver,
// the DJ filter, the signal buses. A test that measures a peak or an RMS over
// both channels cannot see one of a pair go wrong, because the other still
// carries the signal — which is exactly what an operator swap on a single line
// does. A centred voice is the same signal in both channels, so the whole path
// must preserve that *exactly*: no tolerance, no averaging.

/// Peak of one channel on its own — `peak_of` maxes over both, which is the
/// blindness these tests exist to remove.
fn channel_peaks(out: &[(f32, f32)]) -> (f32, f32) {
    out.iter().fold((0.0f32, 0.0f32), |(a, b), (l, r)| {
        (a.max(l.abs()), b.max(r.abs()))
    })
}

/// Assert both channels of `out` are bit-identical, and that it is not silence.
fn assert_channels_agree(what: &str, out: &[(f32, f32)]) {
    let level = peak_of(out);
    assert!(level > 1e-4, "{what}: expected audible output, got {level}");
    for (i, (l, r)) in out.iter().enumerate() {
        assert_eq!(
            l, r,
            "{what}: channels diverged at frame {i} ({l} vs {r}), level {level}"
        );
    }
}

/// Assert each channel carries the signal on its own. Weaker than
/// [`assert_channels_agree`], for the reverb path — its impulse response is
/// deliberately decorrelated between channels, so the two are not the same
/// signal, but neither may be missing.
fn assert_both_channels_audible(what: &str, out: &[(f32, f32)]) {
    let (l, r) = channel_peaks(out);
    let floor = peak_of(out) * 0.05;
    assert!(l > floor && r > floor, "{what}: L {l:.6} R {r:.6}");
}

#[test]
fn a_centred_voice_stays_centred_through_every_send_path() {
    let long_note = 16384;
    for (what, send) in [
        (
            "dry",
            OrbitSend {
                dry: 1.0,
                ..Default::default()
            },
        ),
        (
            "delay",
            OrbitSend {
                dry: 0.0,
                delay: 0.8,
                delay_cfg: DelayConfig {
                    time: 0.02,
                    feedback: 0.6,
                },
                ..Default::default()
            },
        ),
        (
            "djf",
            OrbitSend {
                dry: 1.0,
                djf: Some(0.2),
                ..Default::default()
            },
        ),
    ] {
        assert_channels_agree(what, &render(send, long_note));
    }

    // The per-sample choke branch accumulates separately from the ordinary one,
    // so it gets the same treatment — over a window long enough for the echo to
    // come back, which is the only way its delay accumulation is visible.
    assert_channels_agree(
        "choked",
        &render_choked(
            OrbitSend {
                dry: 1.0,
                delay: 0.5,
                delay_cfg: DelayConfig {
                    time: 0.005,
                    feedback: 0.4,
                },
                ..Default::default()
            },
            4096,
        ),
    );
}

#[test]
fn a_choked_voices_dry_and_wet_paths_sum_rather_than_replace() {
    // Same argument as `the_dry_and_wet_paths_sum_rather_than_replace`, for the
    // per-sample fade branch — which re-implements the whole dry / room / delay
    // split and so needs its own proof that the three add up. The reverb path
    // has no left/right symmetry to lean on, so this is what pins it.
    let with = |dry: f32, room: f32| OrbitSend {
        dry,
        room,
        delay: 0.4,
        delay_cfg: DelayConfig {
            time: 0.005,
            feedback: 0.0,
        },
        ..Default::default()
    };
    let n = 4096;
    let dry_only = render_choked(with(1.0, 0.0), n);
    let wet_only = render_choked(with(0.0, 0.7), n);
    let both = render_choked(with(1.0, 0.7), n);

    let level = peak_of(&both);
    assert!(level > 1e-3, "the choked voice should be audible");
    let worst = both
        .iter()
        .zip(dry_only.iter().zip(&wet_only))
        .fold(0.0f32, |m, (b, (d, w))| {
            m.max((b.0 - (d.0 + w.0)).abs())
                .max((b.1 - (d.1 + w.1)).abs())
        });
    assert!(
        worst < level * 1e-3,
        "a choked voice's dry and wet paths should sum: worst {worst:.6} against {level:.4}"
    );
    // ...and the wet half is really there, so the sum is not two copies of dry.
    assert!(
        peak_of(&wet_only) > level * 0.01,
        "the choked wet sends should carry signal of their own"
    );
}

/// The same voice hard-panned, so exactly one of an orbit's six accumulation
/// buffers carries anything. At 437Hz no sample of the sustain lands exactly on
/// zero, which matters for the `!= 0.0` tests in `mix_into`.
fn panned_event(send: OrbitSend, pan: f32) -> NoteEvent {
    let mut ev = routed_event(send);
    if let rudel_dsp::VoiceSpec::Synth(p) = &mut ev.spec {
        p.pan = pan;
        p.freq = 437.0;
    }
    ev
}

/// An orbit whose reverb and delay both respond within a handful of frames, so
/// a short unit-level render is enough to see them.
fn prompt_orbit() -> OrbitBus {
    OrbitBus::new(
        44100.0,
        &OrbitSend {
            reverb: rudel_dsp::ReverbConfig {
                size: 0.2,
                fade: 0.0, // no fade-in, so the tail starts immediately
                ..Default::default()
            },
            delay_cfg: DelayConfig {
                time: 0.0, // clamps to a one-sample line
                feedback: 0.0,
            },
            ..Default::default()
        },
    )
}

#[test]
fn any_one_of_an_orbits_six_buffers_wakes_it() {
    // An orbit starts idle, and `mix_into` returns without doing any work while
    // it stays that way — so the "did anything arrive" scan gates the entire
    // bus. It is an `||` chain over all six accumulation buffers, and a voice
    // routed to a single send, hard panned, fills exactly one of them. Fed
    // directly here rather than through a voice: a rendered waveform crosses
    // zero, and a buffer containing a zero sample hides half of what this
    // checks.
    const N: usize = 2048; // past the convolver's one-partition latency
    for which in 0..6 {
        let mut bus = prompt_orbit();
        bus.clear(N);
        let buf: &mut Vec<f32> = match which {
            0 => &mut bus.dry_l,
            1 => &mut bus.room_l,
            2 => &mut bus.delay_l,
            3 => &mut bus.dry_r,
            4 => &mut bus.room_r,
            _ => &mut bus.delay_r,
        };
        buf[..N].fill(0.5);
        let mut out = vec![(0.0f32, 0.0f32); N];
        bus.mix_into(&mut out);
        assert!(
            peak_of(&out) > 0.0,
            "buffer {which} on its own should wake the orbit"
        );
    }
}

#[test]
fn an_orbit_keeps_running_while_its_tail_rings_out() {
    // Once nothing is arriving the orbit shuts down to save the work, but only
    // after a window long enough for the reverb and delay to have died away —
    // otherwise every note would have its tail chopped off the moment the
    // source stopped.
    let mut bus = prompt_orbit();
    bus.clear(2048);
    bus.room_l[..2048].fill(0.5);
    bus.room_r[..2048].fill(0.5);
    let mut burst = vec![(0.0f32, 0.0f32); 2048];
    bus.mix_into(&mut burst);
    assert!(peak_of(&burst) > 0.0, "the burst itself should be heard");

    // Two silent blocks later — well inside the 0.2s room's window — the tail
    // is still coming out.
    let mut tail = vec![(0.0f32, 0.0f32); 0];
    for _ in 0..2 {
        bus.clear(512);
        tail = vec![(0.0f32, 0.0f32); 512];
        bus.mix_into(&mut tail);
    }
    assert!(
        peak_of(&tail) > 0.0,
        "the reverb tail should survive the silence, not be cut off at once"
    );
}

#[test]
fn a_cut_group_chokes_only_its_own_group() {
    // `cut` is per group: a new voice silences the voices sharing its group and
    // leaves every other voice alone. Choking across groups would make any two
    // patterns using `cut` interrupt each other.
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(cut_event(
        1,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    mixer.schedule(cut_event(
        2,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    let mut out = vec![(0.0f32, 0.0f32); 256];
    mixer.render_block(&mut out);
    assert_eq!(mixer.active_len(), 2, "both groups should be sounding");

    // A third voice in group 1 chokes only the first.
    mixer.schedule(cut_event(
        1,
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
    ));
    let mut out = vec![(0.0f32, 0.0f32); 2048];
    mixer.render_block(&mut out);
    assert_eq!(
        mixer.active_len(),
        2,
        "the group-2 voice should survive alongside the new group-1 one"
    );
}

#[test]
fn a_duck_lands_on_the_orbit_it_names() {
    // `duckorbit` picks the orbit to duck; the target bus is created if the
    // pattern feeding it has not started yet. Sent to the wrong orbit, a duck
    // would dip whichever bus happened to be first.
    let sustained = |orbit: i32| {
        routed_event(OrbitSend {
            orbit,
            dry: 1.0,
            ..Default::default()
        })
    };
    let level_on_orbit_1 = |duck_orbit: i32| {
        let mut mixer = OfflineMixer::new(44100.0);
        mixer.schedule(sustained(1));
        let mut ducker = routed_event(OrbitSend {
            orbit: 3,
            dry: 0.0,
            ..Default::default()
        });
        ducker.duck = vec![rudel_dsp::Duck {
            orbit: duck_orbit,
            onset: 0.01,
            attack: 0.05,
            depth: 1.0,
        }];
        mixer.schedule(ducker);
        let mut out = vec![(0.0f32, 0.0f32); 4096];
        mixer.render_block(&mut out);
        // Measured at the bottom of the 10ms dip, not over the whole render —
        // a peak taken across the recovery would not see the duck at all.
        peak_of(&out[380..460])
    };
    let ducked = level_on_orbit_1(1);
    let elsewhere = level_on_orbit_1(5);
    assert!(
        ducked < elsewhere * 0.9,
        "ducking orbit 1 should dip it ({ducked:.4}) below the untouched \
         case ({elsewhere:.4})"
    );
}

#[test]
fn the_choke_fade_takes_its_full_ten_milliseconds() {
    // The fade steps by `1 / (sample_rate * CHOKE_SECS)` per sample: 441 steps
    // at 44.1kHz. A step computed any other way either cuts the voice off in a
    // single sample (a click) or leaves it hanging around forever.
    let out = render_choked(
        OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        1024,
    );
    assert!(
        peak_of(&out[200..300]) > 1e-3,
        "5ms in, the voice should still be fading, not gone"
    );
    assert!(
        peak_of(&out[600..]) == 0.0,
        "past 441 samples the voice should be silent and dropped"
    );
}

#[test]
fn the_offline_mixer_steps_frames_the_same_way_it_renders_a_block() {
    // `OfflineMixer::render_frame` is what the benchmarks and the `play`
    // example pull on; it must agree with `render_block` rather than being its
    // own path.
    let send = OrbitSend {
        dry: 1.0,
        ..Default::default()
    };
    let mut by_block = OfflineMixer::new(44100.0);
    by_block.schedule(routed_event(send.clone()));
    let mut block = vec![(0.0f32, 0.0f32); 128];
    by_block.render_block(&mut block);

    let mut by_frame = OfflineMixer::new(44100.0);
    by_frame.schedule(routed_event(send));
    let stepped: Vec<(f32, f32)> = (0..128).map(|_| by_frame.render_frame()).collect();

    assert!(peak_of(&stepped) > 1e-3, "the stepped render should sound");
    assert_eq!(stepped, block, "frame stepping should match the block");
}

#[test]
fn write_frames_lays_out_mono_stereo_and_extra_channels() {
    // The cpal callback's only job: pull frames and fan them out to however
    // many channels the device has. Mono gets the average of the pair, stereo
    // gets it verbatim, anything wider gets the average in the extra channels.
    // Driven with an off-centre voice, so an average cannot pass for a channel.
    let voice = || {
        let (tx, rx) = mpsc::channel::<NoteEvent>();
        tx.send(panned_event(
            OrbitSend {
                dry: 1.0,
                ..Default::default()
            },
            0.85,
        ))
        .unwrap();
        drop(tx);
        test_mixer(rx)
    };
    let want: Vec<(f32, f32)> = {
        let mut m = voice();
        (0..4).map(|_| m.render_frame()).collect()
    };
    assert!(
        want.iter().any(|(l, r)| l != r) && peak_of(&want) > 1e-3,
        "the reference render must be audible and off-centre: {want:?}"
    );

    let mut mono = [0.0f32; 4];
    write_frames(&mut mono, 1, &mut voice());
    for (got, (l, r)) in mono.iter().zip(&want) {
        assert_eq!(*got, (l + r) * 0.5, "mono is the average of the pair");
    }

    let mut stereo = [0.0f32; 8];
    write_frames(&mut stereo, 2, &mut voice());
    for (got, (l, r)) in stereo.chunks(2).zip(&want) {
        assert_eq!((got[0], got[1]), (*l, *r), "stereo is the pair verbatim");
    }

    let mut surround = [0.0f32; 12];
    write_frames(&mut surround, 3, &mut voice());
    for (got, (l, r)) in surround.chunks(3).zip(&want) {
        assert_eq!(
            (got[0], got[1], got[2]),
            (*l, *r, (l + r) * 0.5),
            "extra channels get the average"
        );
    }
}

#[test]
fn a_later_event_retunes_the_orbit_it_lands_on() {
    // An orbit outlives the voice that created it, so the settings a later
    // event carries have to be applied to the existing bus — `getDelay` /
    // `getReverb` in superdough. Left unapplied, `delaytime` and `roomsize`
    // would only ever take effect on the first note to reach an orbit.
    let quiet_with = |send: OrbitSend| {
        let mut ev = routed_event(send);
        if let rudel_dsp::VoiceSpec::Synth(p) = &mut ev.spec {
            p.gain = 0.0;
        }
        ev
    };
    // A silent voice opens the orbit at one setting; a later audible one has to
    // retune it. Each half runs on its own mixer with only the send under test,
    // so nothing else can keep the output alive.
    let open_then = |first: OrbitSend, second: OrbitSend, n: usize| {
        let mut mixer = OfflineMixer::new(44100.0);
        mixer.schedule(quiet_with(first));
        let mut warm = vec![(0.0f32, 0.0f32); 64];
        mixer.render_block(&mut warm);
        let mut ev = routed_event(second);
        // A short note, so what is left afterwards is the orbit's tail.
        if let rudel_dsp::VoiceSpec::Synth(p) = &mut ev.spec {
            p.duration = 0.01;
        }
        mixer.schedule(ev);
        let mut out = vec![(0.0f32, 0.0f32); n];
        mixer.render_block(&mut out);
        out
    };

    // Delay: opened at half a second, retuned to five milliseconds. The retuned
    // echo lands around frame 220; the original would not arrive until 22050.
    let delayed = |time: f32| OrbitSend {
        dry: 0.0,
        delay: 1.0,
        delay_cfg: DelayConfig {
            time,
            feedback: 0.0,
        },
        ..Default::default()
    };
    let out = open_then(delayed(0.5), delayed(0.005), 4096);
    assert!(
        peak_of(&out[220..1200]) > 1e-3,
        "the delay should have been retuned to the later event's time"
    );

    // Reverb: opened at a 50ms room, rebuilt at a 3s one. Measured a second
    // later, where only the long tail can still be ringing.
    let roomy = |size: f32| OrbitSend {
        dry: 0.0,
        room: 0.9,
        reverb: rudel_dsp::ReverbConfig {
            size,
            ..Default::default()
        },
        ..Default::default()
    };
    let out = open_then(roomy(0.05), roomy(3.0), 44100);
    assert!(
        peak_of(&out[40000..]) > 1e-6,
        "the reverb should have been rebuilt at the later event's size"
    );
    // The short room really is short, so that measurement means something.
    let short = open_then(roomy(0.05), roomy(0.05), 44100);
    assert!(
        peak_of(&short[40000..]) < 1e-9,
        "a 50ms room should be silent a second on, got {}",
        peak_of(&short[40000..])
    );
}

/// A voice that is already choking on its very first sample, so *everything*
/// in the output came through the per-sample fade branch.
///
/// [`render_choked`] cannot show that branch on its own: it has to let the
/// voice start before choking it, and those unchoked frames leave a reverb tail
/// that keeps ringing over whatever the fade branch does or does not
/// contribute. Built directly instead — the fade branch is reached by putting a
/// voice in the active list with its choke already under way.
fn render_choked_from_the_start(send: OrbitSend, pan: f32, n: usize) -> Vec<(f32, f32)> {
    let (tx, rx) = mpsc::channel::<NoteEvent>();
    drop(tx);
    let mut mixer = test_mixer(rx);
    let ev = panned_event(send.clone(), pan);
    mixer.active.push(ActiveVoice {
        voice: ev
            .spec
            .into_modulated_voice(mixer.sample_rate, ev.fx, &ev.mods),
        tags: Vec::new(),
        cut: None,
        send,
        choke_gain: Some(1.0),
    });
    let mut out = vec![(0.0f32, 0.0f32); n];
    mixer.render_block(&mut out);
    out
}

#[test]
fn a_choked_voice_still_feeds_its_delay_send() {
    // The fade branch re-implements the dry/room/delay split, so its delay
    // accumulation is its own code. A centred voice through the delay line
    // stays identical in both channels, which no single-channel slip survives.
    let out = render_choked_from_the_start(
        OrbitSend {
            dry: 0.0,
            delay: 0.9,
            delay_cfg: DelayConfig {
                time: 0.002,
                feedback: 0.0,
            },
            ..Default::default()
        },
        0.5,
        1024,
    );
    assert_channels_agree("choked delay send", &out);
}

#[test]
fn a_choked_voice_still_feeds_its_reverb_send_the_right_way_up() {
    // Hard left, reverb only: the orbit's *sole* input is the fade branch's
    // `room_l`, so if that accumulation goes missing the bus never wakes at all.
    let room = |pan: f32| {
        render_choked_from_the_start(
            OrbitSend {
                dry: 0.0,
                room: 0.9,
                reverb: rudel_dsp::ReverbConfig {
                    size: 0.3,
                    fade: 0.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            pan,
            4096,
        )
    };
    let left = room(0.0);
    let right = room(1.0);
    let (ll, lr) = channel_peaks(&left);
    let (rl, rr) = channel_peaks(&right);
    assert!(ll > 1e-5, "a hard-left choked room send should be heard");
    assert!(rr > 1e-5, "and so should a hard-right one");
    assert!(
        lr < ll * 1e-3 && rl < rr * 1e-3,
        "each stays on its own side"
    );

    // ...and the right way up. A `-=` would invert the send, which no level
    // measurement can see — but it anti-correlates with the ordinary path,
    // which is fed by the same voice through the same reverb.
    let open = render(
        OrbitSend {
            dry: 0.0,
            room: 0.9,
            reverb: rudel_dsp::ReverbConfig {
                size: 0.3,
                fade: 0.0,
                ..Default::default()
            },
            ..Default::default()
        },
        4096,
    );
    let correlation = |choked: &[(f32, f32)], pick: fn(&(f32, f32)) -> f32| -> f64 {
        choked
            .iter()
            .zip(&open)
            .map(|(c, o)| pick(c) as f64 * pick(o) as f64)
            .sum()
    };
    // Both channels: each is accumulated on its own line, so each can be
    // inverted on its own.
    assert!(
        correlation(&left, |f| f.0) > 0.0,
        "the choked reverb send should keep the open path's polarity (left)"
    );
    assert!(
        correlation(&right, |f| f.1) > 0.0,
        "the choked reverb send should keep the open path's polarity (right)"
    );
}

#[test]
fn a_choked_voices_bus_send_fades_with_it() {
    // `busgain * choke_gain`: the signal bus sees a choked voice at its current
    // fade level, so a voice that is half faded sends at half its busgain.
    let level_at = |choke: f32| {
        let (tx, rx) = mpsc::channel::<NoteEvent>();
        drop(tx);
        let mut mixer = test_mixer(rx);
        let send = OrbitSend {
            dry: 0.0,
            bus: Some(6),
            busgain: 0.5,
            ..Default::default()
        };
        let ev = panned_event(send.clone(), 0.5);
        mixer.signal_buses.entry(6).or_default();
        mixer.active.push(ActiveVoice {
            voice: ev
                .spec
                .into_modulated_voice(mixer.sample_rate, ev.fx, &ev.mods),
            tags: Vec::new(),
            cut: None,
            send,
            choke_gain: Some(choke),
        });
        let mut out = vec![(0.0f32, 0.0f32); 64];
        mixer.render_block(&mut out);
        let (l, _) = &mixer.signal_buses[&6];
        l[..64].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let full = level_at(1.0);
    let half = level_at(0.5);
    assert!(full > 1e-4, "a sending voice should reach the bus");
    assert!(
        (half / full - 0.5).abs() < 0.05,
        "a half-faded voice should send at half level: {half} vs {full}"
    );
}

#[test]
fn the_reverb_send_reaches_both_channels() {
    // The reverb's impulse response is decorrelated per channel, so the two
    // outputs are not identical — but a send that only fed one of them would
    // still look fine to any measurement taken over the pair.
    let room = OrbitSend {
        dry: 0.0,
        room: 0.8,
        ..Default::default()
    };
    assert_both_channels_audible("room", &render(room.clone(), 16384));
    assert_both_channels_audible("choked room", &render_choked(room, 4096));
}

#[test]
fn a_centred_voice_stays_centred_through_a_signal_bus() {
    // The `bus` send has its own left/right accumulation, read back by the
    // voice on the other side.
    let mut mixer = OfflineMixer::new(44100.0);
    mixer.schedule(routed_event(OrbitSend {
        dry: 0.0,
        bus: Some(4),
        busgain: 0.75,
        ..Default::default()
    }));
    mixer.schedule(NoteEvent {
        onset_seconds: 0.0,
        spec: rudel_dsp::VoiceSpec::Bus(rudel_dsp::BusParams {
            bus: 4,
            adsr: rudel_dsp::Adsr {
                attack: 0.0001,
                decay: 0.0001,
                sustain: 1.0,
                release: 0.01,
            },
            duration: 10.0,
            gain: 1.0,
            pan: 0.5,
            filters: Default::default(),
        }),
        fx: rudel_dsp::PostFx::default(),
        cut: None,
        send: OrbitSend {
            dry: 1.0,
            ..Default::default()
        },
        duck: Vec::new(),
        mods: Default::default(),
        tags: Vec::new(),
    });
    let mut out = vec![(0.0f32, 0.0f32); 4096];
    mixer.render_block(&mut out);
    assert_channels_agree("bus", &out);
}
