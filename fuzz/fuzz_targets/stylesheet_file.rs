//! Stylesheet files, as xWorks reads people's own from their configuration
//! folder (crates/story's `sheet`; CLAUDE.md, rule 8): any bytes read as a
//! stylesheet or give an error, never a panic, a hang or unbounded memory;
//! one that reads makes a document that holds together, resolves, and
//! writes back as a file that reads as the same stylesheet. Seed it with
//! the sheets that come with xWorks:
//!
//! ```text
//! cargo +nightly fuzz run stylesheet_file corpus/stylesheet_file ../spec/styles
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use story::{SheetSpec, StyleSheet};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(spec) = SheetSpec::parse(text) else {
        return;
    };
    let document = story::new_document(&spec);
    assert_eq!(document.sheet(), Some(doc::ObjectId(story::SHEET_AT)));
    let sheet = StyleSheet::from_spec(&spec);
    assert!(sheet.page.body_width() > 0 && sheet.page.body_height() > 0);
    // Written back as a file, it reads as the same stylesheet, resolved.
    let written = SheetSpec::of(&document).expect("a document's stylesheet writes");
    let again = SheetSpec::parse(&written.write()).expect("what's written reads");
    assert_eq!(StyleSheet::from_spec(&again), sheet);
});
