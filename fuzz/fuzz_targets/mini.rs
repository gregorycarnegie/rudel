#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &str| {
    let parsed = rudel_mini::parse(source);
    let locations = rudel_mini::leaf_locations(source);
    assert_eq!(parsed.is_ok(), locations.is_ok());
    if let Ok(locations) = locations {
        let mut previous_end = 0;
        for (start, end) in locations {
            assert!(previous_end <= start && start < end);
            assert!(source.get(start..end).is_some());
            previous_end = end;
        }
    }
    // Building with an editor offset must accept exactly the same grammar.
    assert_eq!(
        parsed.is_ok(),
        rudel_mini::parse_with_offset(source, 17).is_ok()
    );
});
