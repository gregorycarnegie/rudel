use super::common::*;
use proptest::prelude::*;

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

proptest! {
    #[test]
    fn mtof_octaves_double_frequency(note in -60.0f64..120.0f64) {
        let base = mtof(note);
        let octave = mtof(note + 12.0);

        prop_assert!(base.is_finite());
        prop_assert!(octave.is_finite());
        prop_assert!((octave / base - 2.0).abs() < 0.00001);
    }

    #[test]
    fn mtof_is_monotonic(note in -60.0f64..120.0f64, semitones in 0.0f64..48.0f64) {
        prop_assert!(mtof(note + semitones) >= mtof(note));
    }
}
