//! What's typed into the inspector's lengths, and a paragraph's own tab
//! stops as a file holds them: any text reads without a panic. A length
//! read is within its bounds and shows as text that reads back as itself;
//! tab stops read are in order, one at each place, and write back as text
//! that reads as them again.

#![no_main]

use chrome::{Length, Unit};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    for unit in [Unit::Point, Unit::Centimetre, Unit::Millimetre, Unit::Inch] {
        if let Some(length) = Length::parse(text, unit) {
            assert!(length.value.abs() <= 1000 * 914_400);
            let shown = length.to_string();
            let back = Length::parse(&shown, Unit::Point).expect("what's shown reads");
            assert_eq!(back.to_string(), shown);
        }
    }
    if let Some(stops) = story::parse_tabs(text) {
        assert!(stops.windows(2).all(|w| w[0].position < w[1].position));
        let written = story::format_tabs(&stops);
        assert_eq!(story::parse_tabs(&written), Some(stops));
    }
});
