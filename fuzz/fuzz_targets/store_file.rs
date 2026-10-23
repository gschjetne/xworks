//! Document files, as anyone might hand one over: any bytes open as a
//! document or give an error, never a panic, and a document that opens
//! dumps, and its pictures read and decode or give errors. Seed it with
//! the files older xWorks wrote, so it reaches the upgrade from older
//! schemas:
//!
//! ```text
//! cargo +nightly fuzz run store_file corpus/store_file ../spec/format/fixtures
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!("xworks-fuzz-{}.xw", std::process::id()));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    if let Ok(opened) = store::Store::open(&path) {
        let _ = opened.store.versions();
        let _ = opened.store.fonts();
        // Its pictures, as the editor reads and decodes them, and those
        // its story names.
        for id in opened.store.image_ids().unwrap_or_default() {
            if let Ok(Some(image)) = opened.store.image(&id) {
                let _ = app::picture::decode(&image.data, xworks_fuzz::limits());
            }
        }
        let _ = story::pictures(&opened.document);
        drop(opened);
        let _ = store::dump(&path);
    }
    let _ = std::fs::remove_file(&path);
});
