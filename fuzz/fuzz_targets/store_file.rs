//! Document files, as anyone might hand one over: any bytes open as a
//! document or give an error, never a panic, and a document that opens
//! dumps.

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
        drop(opened);
        let _ = store::dump(&path);
    }
    let _ = std::fs::remove_file(&path);
});
