use super::common::*;

#[test]
fn drum_names_resolve() {
    assert_eq!(DrumKind::from_name("bd"), Some(DrumKind::Bd));
    assert_eq!(DrumKind::from_name("hh"), Some(DrumKind::Hh));
    assert_eq!(DrumKind::from_name("oh"), Some(DrumKind::Oh));
    assert_eq!(DrumKind::from_name("rim"), Some(DrumKind::Rim));
    assert_eq!(DrumKind::from_name("sawtooth"), None);
}

#[test]
fn drum_produces_sound_then_finishes() {
    for kind in [
        DrumKind::Bd,
        DrumKind::Sd,
        DrumKind::Hh,
        DrumKind::Oh,
        DrumKind::Rim,
        DrumKind::Clap,
        DrumKind::Lt,
        DrumKind::Mt,
        DrumKind::Ht,
        DrumKind::Rd,
        DrumKind::Cr,
    ] {
        let mut v = DrumVoice::new(DrumParams::new(kind), 44100.0);
        let mut out = Vec::new();
        let mut ticks = 0;
        for _ in 0..(44100 * 2) {
            let (l, _r) = v.tick();
            out.push(l);
            ticks += 1;
            if v.is_done() {
                break;
            }
        }
        assert_is_signal(&out, &format!("{kind:?}"));
        assert!(v.is_done(), "{kind:?} should finish");
        assert!(ticks < 44100 * 2, "{kind:?} should finish within 2s");
    }
}
