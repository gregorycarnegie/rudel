use super::*;
use crate::Frac;

/// The MIDI numbers a pattern produces. `voicing` sets the `note` control (as
/// upstream's `.note()` does) and `root_notes` yields a note *name*, so read
/// through the control and resolve a name if that is what is there.
fn notes(pat: &Pattern) -> Vec<i32> {
    let midi = |v: &Value| match v {
        Value::Str(s) => crate::tonal::note_to_midi(s).map(|m| m as f64),
        other => other.as_f64(),
    };
    let mut v: Vec<i32> = pat
        .query_arc(Frac::zero(), Frac::one())
        .into_iter()
        .map(|h| match &h.value {
            Value::Map(m) => midi(m.get("note").unwrap()).unwrap() as i32,
            other => midi(other).unwrap() as i32,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn lefthand_cmaj7() {
    // C^7 lefthand, anchor a4 -> rootless voicing B3 D4 E4 G4
    let opts = VoicingOpts {
        dict: "lefthand".to_string(),
        ..Default::default()
    };
    assert_eq!(render_voicing("C^7", &opts), Some(vec![59, 62, 64, 67]));
}

#[test]
fn triad_c_major() {
    let opts = VoicingOpts {
        dict: "triads".to_string(),
        ..Default::default()
    };
    // C major triad below the default c5 anchor -> E4 G4 C5.
    assert_eq!(render_voicing("C", &opts), Some(vec![64, 67, 72]));
}

#[test]
fn voicing_pattern_stacks_notes() {
    // default dictionary is now `ireal`: C -> E3 C4 E4 G4 C5.
    let pat = pure(Value::Str("C".into())).voicing();
    assert_eq!(notes(&pat), vec![52, 60, 64, 67, 72]);
}

#[test]
fn voicings_named_dictionary() {
    let pat = pure(Value::Str("C^7".into())).voicings("lefthand");
    assert_eq!(notes(&pat), vec![59, 62, 64, 67]);
}

#[test]
fn voicing_reads_list_backed_chord_symbol() {
    // mini spells `c:maj7` as ["c", "maj7"]; voicing joins it to "Cmaj7".
    let pat = pure(Value::List(vec![
        Value::Str("C".into()),
        Value::Str("maj7".into()),
    ]))
    .voicings("lefthand");
    let from_symbol = pure(Value::Str("C^7".into())).voicings("lefthand");
    assert_eq!(notes(&pat), notes(&from_symbol));
}

#[test]
fn voicing_reads_dictionary_control_key() {
    // a map carrying chord + the `dictionary` control key (from `dict()`).
    let mut m = ValueMap::new();
    m.insert("chord".to_string(), Value::Str("C^7".into()));
    m.insert("dictionary".to_string(), Value::Str("lefthand".into()));
    let pat = pure(Value::Map(m)).voicing();
    assert_eq!(notes(&pat), vec![59, 62, 64, 67]);
}

#[test]
fn duck_mode_voices_against_an_anchor_event() {
    // `.anchor(melody).mode('duck')` is how a tune keeps a comping chord out of
    // the melody's way: the voicing is aligned to the anchor note, then any
    // tone landing *on* it is dropped. The anchor arrives as a whole event
    // (`{anchor: {note: …}}`), not a scalar, so reading it means looking inside.
    let voiced = |anchor: Value| {
        let mut m = ValueMap::new();
        m.insert("chord".to_string(), Value::Str("C^7".into()));
        m.insert("dictionary".to_string(), Value::Str("lefthand".into()));
        m.insert("mode".to_string(), Value::Str("duck".into()));
        m.insert("anchor".to_string(), anchor);
        notes(&pure(Value::Map(m)).voicing())
    };
    // Anchored on B4 (71), the lefthand C^7 voicing sits below it: B3 D4 E4 G4.
    let mut anchor_event = ValueMap::new();
    anchor_event.insert("note".to_string(), Value::F64(71.0));
    assert_eq!(voiced(Value::Map(anchor_event)), vec![59, 62, 64, 67]);
    // A bare number and a note name anchor the same way...
    assert_eq!(voiced(Value::F64(71.0)), vec![59, 62, 64, 67]);
    assert_eq!(voiced(Value::Str("B4".into())), vec![59, 62, 64, 67]);
    // ...and a tone that collides with the anchor is ducked out: anchored on
    // G4 (67), the G4 in the voicing goes.
    let mut collides = ValueMap::new();
    collides.insert("note".to_string(), Value::F64(67.0));
    let ducked = voiced(Value::Map(collides));
    assert!(
        !ducked.contains(&67),
        "the anchor's own note is dropped: {ducked:?}"
    );
    assert_eq!(ducked.len(), 3);
}

#[test]
fn root_notes_reads_list_backed_chord() {
    let pat = pure(Value::List(vec![
        Value::Str("A".into()),
        Value::Str("m7".into()),
    ]))
    .root_notes(3);
    assert_eq!(notes(&pat), vec![57]); // A3
}

#[test]
fn root_notes_maps_to_octave() {
    let pat = pure(Value::Str("C^7".into())).root_notes(2);
    assert_eq!(notes(&pat), vec![36]); // C2
    let pat = pure(Value::Str("Am7".into())).root_notes(3);
    assert_eq!(notes(&pat), vec![57]); // A3
}

#[test]
fn voicing_with_n_plays_like_scale() {
    // n selects a single note from the voicing, octaving overshoots.
    // triads C below the c5 anchor is [E4 G4 C5] = [64, 67, 72].
    let opts = VoicingOpts {
        dict: "triads".to_string(),
        n: Some(0),
        ..Default::default()
    };
    assert_eq!(render_voicing("C", &opts), Some(vec![64]));
    let opts = VoicingOpts {
        dict: "triads".to_string(),
        n: Some(3), // wraps to the next octave of note 0
        ..Default::default()
    };
    assert_eq!(render_voicing("C", &opts), Some(vec![76]));
}

#[test]
fn unknown_chord_is_silent() {
    let pat = pure(Value::Str("Zwurble".into())).voicing();
    assert!(pat.query_arc(Frac::zero(), Frac::one()).is_empty());
}
