use super::common::*;

#[test]
fn note_names() {
    assert_eq!(note_name_to_midi("a4"), Some(69));
    assert_eq!(note_name_to_midi("c4"), Some(60));
    assert_eq!(note_name_to_midi("c3"), Some(48));
    assert_eq!(note_name_to_midi("c#4"), Some(61));
    assert_eq!(note_name_to_midi("eb3"), Some(51));
    assert_eq!(note_name_to_midi("c"), Some(48)); // default octave 3
}

#[test]
fn mtof_a4() {
    assert!((mtof(69.0) - 440.0).abs() < 0.001);
}
