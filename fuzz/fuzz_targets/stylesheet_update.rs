//! Taking a newer version of a stylesheet three-way (crates/story's
//! `update`; CLAUDE.md, rule 8), from stylesheet files as people's own are
//! read, the versions kept when they're kept again among them: two files,
//! split at the first NUL byte, the version a document copied and a newer
//! one, each read or refused, never a panic. Where both read, a document
//! copied from either takes the newer one: its operations apply to the
//! document as it was, Undo takes them back whole, and the sheet it ends
//! with resolves and says it came from the newer version. Seed it with
//! the sheets that come with xWorks and their kept versions:
//!
//! ```text
//! cargo +nightly fuzz run stylesheet_update corpus/stylesheet_update ../spec/styles ../spec/styles/versions
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use story::{SheetSpec, StyleSheet};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (a, b) = text.split_once('\0').unwrap_or((text, text));
    let (Ok(base), Ok(newer)) = (SheetSpec::parse(a), SheetSpec::parse(b)) else {
        return;
    };
    let (from, to) = (story::new_document(&base), story::new_document(&newer));
    let program = StyleSheet::from_spec(&newer);
    for start in [&base, &newer] {
        let document = story::new_document(start);
        for copied in [Some(&from), None] {
            let mut updated = document.clone();
            let update = story::update(&mut updated, copied, &to);
            let mut again = document.clone();
            let undo = again
                .apply_all(&update.ops)
                .expect("an update applies to the document as it was");
            assert_eq!(again.to_snapshot(), updated.to_snapshot());
            again.apply_all(&undo).expect("and is undone");
            assert_eq!(again.to_snapshot().objects, document.to_snapshot().objects);
            let sheet = StyleSheet::of(&updated, &program);
            assert_eq!(sheet.origin, Some((newer.name.clone(), newer.version)));
            assert!(sheet.page.body_width() > 0 && sheet.page.body_height() > 0);
        }
    }
});
