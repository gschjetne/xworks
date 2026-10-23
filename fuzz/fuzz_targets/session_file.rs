//! Session files, as a bug report hands one over (the session recorder
//! writes them, app::recorder): any text reads as a session or gives an
//! error, never a panic; a session that reads writes, and what it writes
//! reads back the same; and the document a recorded session starts from
//! holds together or is refused. Seed it with spec/sessions/:
//!
//! ```text
//! cargo +nightly fuzz run session_file corpus/session_file ../spec/sessions
//! ```

#![no_main]

use app::session;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(read) = session::read(text) else {
        return;
    };
    let written = session::write(&read).expect("a session that reads writes");
    let again = session::read(&written).expect("what's written reads back");
    assert_eq!(
        session::write(&again).expect("it writes again"),
        written,
        "it reads back otherwise"
    );
    if let Some(start) = read.start {
        let _ = doc::Document::from_snapshot(start.document);
        let _ = doc::History::from_stacks(start.undo, start.redo);
    }
});
