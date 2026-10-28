//! Markdown files as outside input: a file that isn't UTF-8 is an error;
//! blocks nested deeper than any stack could recurse through are read,
//! written back byte for byte, edited and read back on a small stack, as
//! nothing walks the story by recursion; what Markdown can't hold is
//! refused rather than written away; and reading takes time in a file's
//! size, not in how deep its blocks nest, which doesn't hold yet.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the pure-crate rule is for markdown itself: its tests give it a small stack of its own, and time it"
)]

use std::time::{Duration, Instant};

use doc::{Op, Props, Run};
use markdown::{ReadError, Settings, shape};
use story::StyleSheet;

/// Neutral, the program's stylesheet, which Markdown files take.
fn neutral() -> story::SheetSpec {
    let source = testkit::specs("styles")
        .expect("spec/styles")
        .into_iter()
        .find(|(name, _)| name == "neutral")
        .expect("spec/styles/neutral.ron")
        .1;
    story::SheetSpec::parse(&source).expect("neutral.ron is a stylesheet")
}

fn read(bytes: &[u8]) -> Result<markdown::Read, ReadError> {
    let objects = neutral().objects(story::SHEET_AT);
    let settings = Settings {
        sheet: &objects,
        language: Some("en-US"),
        sizes: &|_| None,
    };
    markdown::read(bytes, &settings)
}

/// Quotes nested `quotes` deep, and lists nested `lists` deep on one
/// line, each around one paragraph.
fn nested(quotes: usize, lists: usize) -> [(&'static str, Vec<u8>); 2] {
    [
        (
            "quotes",
            format!("{}deep\n", "> ".repeat(quotes)).into_bytes(),
        ),
        (
            "lists",
            format!("{}deep\n", "- ".repeat(lists)).into_bytes(),
        ),
    ]
}

#[test]
fn a_file_that_isnt_utf8_is_an_error() {
    assert_eq!(
        read(b"# Heading\n\nText, then \xff.\n").err(),
        Some(ReadError::NotUtf8(22))
    );
    // A byte order mark counts in where it isn't.
    assert_eq!(
        read(b"\xef\xbb\xbfAb\xc3").err(),
        Some(ReadError::NotUtf8(5))
    );
    assert!(read(b"").is_ok());
}

/// On a stack of 256 KiB, an eighth of a test's own, quotes nested 2 000
/// deep and lists 400, which recursion of a few hundred bytes a level
/// would overflow, are read, written back as they were, edited in their
/// deepest paragraph, written and read back as edited. Lists go no deeper
/// only as reading them takes time in the cube of their depth (below).
#[test]
fn nesting_never_exhausts_the_stack() {
    let work = || {
        let sheet = StyleSheet::from_spec(&neutral());
        for (what, bytes) in nested(2000, 400) {
            let mut read = read(&bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
            let written = read.file.write(&read.document, &sheet).expect("it writes");
            assert!(written == bytes, "{what} are written back as they were");
            let deepest = *read.document.paragraphs().last().expect("a paragraph");
            let mut edited = read.document.clone();
            edited
                .apply(&Op::InsertText {
                    paragraph: deepest,
                    offset: 4,
                    text: "er".to_string(),
                    runs: vec![Run::new(2, Props::new())],
                })
                .expect("it applies");
            let written = read.file.write(&edited, &sheet).expect("it writes");
            let back = self::read(&written).expect("it reads back");
            assert_eq!(
                shape(&back.document, &sheet),
                shape(&edited, &sheet),
                "{what} edited read back as edited"
            );
        }
    };
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(work)
        .expect("a thread")
        .join()
        .expect("it reads, writes and edits on a small stack");
}

/// A file a few kilobytes long should read in well under a second however
/// deep it nests (docs/ARCHITECTURE.md, section 11: outside input never
/// hangs). It doesn't: reading takes time in the square of the depth for
/// quotes, and in its cube for lists, so 4 KB of `- - - …` takes a minute
/// and a half in a debug build, though pulldown-cmark itself reads it in
/// a millisecond.
#[test]
#[ignore = "reading takes time in the square or cube of the depth: M2 stage 8 part A's report, markdown"]
fn reading_takes_time_in_the_size_not_the_depth() {
    for (what, bytes) in nested(1000, 1000) {
        let start = Instant::now();
        read(&bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
        let took = start.elapsed();
        assert!(
            took < Duration::from_secs(1),
            "{} bytes of {what} took {took:?}",
            bytes.len()
        );
    }
}

/// What Markdown can't hold is refused, never written away: a font or a
/// size set on text, or a paragraph's own spacing, indents or line height,
/// each says what it is and which block has it.
#[test]
fn what_markdown_cant_hold_is_refused() {
    use doc::Value;
    use markdown::What;
    use story::keys;
    let sheet = StyleSheet::from_spec(&neutral());
    let mut read = read(b"# A heading\n\nA paragraph of text.\n").expect("it reads");
    let paragraph = read.document.paragraphs()[1];
    let on_text = |key: &str, value: Value| Op::SetCharProp {
        paragraph,
        offset: 2,
        len: 9,
        key: key.to_string(),
        value: Some(value),
    };
    let on_paragraph = |key: &str, value: Value| Op::SetParaProp {
        paragraph,
        key: key.to_string(),
        value: Some(value),
    };
    for (op, what) in [
        (on_text(keys::FONT, Value::Text("Inter".into())), What::Font),
        (on_text(keys::SIZE, Value::Int(14 * 12_700)), What::Size),
        (
            on_paragraph(keys::SPACE_BEFORE, Value::Int(0)),
            What::Spacing,
        ),
        (
            on_paragraph(keys::INDENT_START, Value::Int(9_525)),
            What::Indents,
        ),
        (
            on_paragraph(keys::LINE_HEIGHT, Value::Int(1500)),
            What::LineHeight,
        ),
    ] {
        let mut edited = read.document.clone();
        edited.apply(&op).expect("it applies");
        let refused = read.file.write(&edited, &sheet).expect_err("it's refused");
        assert_eq!((refused.what, refused.block), (what, paragraph));
    }
    // Without the edit, the file is as it was.
    let written = read.file.write(&read.document, &sheet).expect("it writes");
    assert_eq!(written, b"# A heading\n\nA paragraph of text.\n");
}
