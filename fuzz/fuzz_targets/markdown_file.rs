//! Markdown files, as xWorks opens them (crates/markdown; CLAUDE.md, rule
//! 8): any bytes read as a story or give an error, never a panic, a hang
//! or unbounded memory; a story read is written back untouched as the
//! same bytes; and written afresh, as a new file, it reads back as the
//! same story. Seed it with spec/markdown/files/:
//!
//! ```text
//! cargo +nightly fuzz run markdown_file corpus/markdown_file ../spec/markdown/files
//! ```

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use markdown::{File, Settings, What};
use story::StyleSheet;

fn sheet() -> &'static StyleSheet {
    static SHEET: OnceLock<StyleSheet> = OnceLock::new();
    SHEET.get_or_init(|| ron::from_str(include_str!("../../spec/styles/m1.ron")).expect("m1.ron"))
}

fuzz_target!(|data: &[u8]| {
    let sheet = sheet();
    let settings = Settings {
        sheet,
        language: Some("en-US"),
        sizes: &|_| Some((9525 * 64, 9525 * 48)),
    };
    let Ok(read) = markdown::read(data, &settings) else {
        return;
    };
    let mut file = read.file.clone();
    let written = file
        .write(&read.document, sheet)
        .expect("a story read writes back");
    assert!(
        written == data,
        "a story read untouched writes back as it was"
    );
    // What pulldown-cmark reads as nothing is kept as it was written, but
    // where it's written afresh it may read as something: only stories
    // without it are checked to read back the same.
    let kept_nothing = read.document.paragraphs().into_iter().any(|id| {
        read.document.paragraph(id).is_some_and(|p| {
            p.props.get(story::keys::SOURCE) == Some(&doc::Value::Text("definition".to_string()))
                && !p.text.trim_start().starts_with('[')
        })
    });
    // Nor are files with lines ended by carriage returns alone, which
    // pulldown-cmark takes for blank inside HTML blocks and nowhere else.
    let lone_cr = data.windows(2).any(|w| w[0] == b'\r' && w[1] != b'\n') || data.ends_with(b"\r");
    // Nor are those with a line separator in Markdown kept as written: the
    // story shows it as a line's end, and as one it's written afresh, which
    // may end an HTML block sooner.
    let separator = data.windows(3).any(|w| w == "\u{2028}".as_bytes())
        && read.document.paragraphs().into_iter().any(|id| {
            read.document
                .paragraph(id)
                .is_some_and(|p| p.props.contains_key(story::keys::SOURCE))
        });
    if kept_nothing || lone_cr || separator {
        return;
    }
    let fresh = match File::empty(read.file.language()).write(&read.document, sheet) {
        Ok(fresh) => fresh,
        // Marks no Markdown writes there, as emphasis starting with
        // punctuation inside a word, which nothing read has.
        Err(e) if e.what == What::Marks => return,
        Err(e) => panic!("a story read can be written afresh: {e:?}"),
    };
    let again = markdown::read(&fresh, &settings).expect("what's written reads");
    assert_eq!(
        markdown::shape(&read.document, sheet),
        markdown::shape(&again.document, sheet),
        "what's written afresh reads back as the same story"
    );
});
