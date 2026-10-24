//! CommonMark's own examples (docs/ROADMAP.md, M2 stage 6): each one
//! read, and written back untouched, which gives it byte for byte; and
//! written afresh, as a new file would be, which reads back as the same
//! story. The examples are fetched by hash with the fonts, `cargo run -p
//! fetch-fonts` (spec/markdown/commonmark.ron).

use markdown::{File, Settings, shape};
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

/// Neutral resolved, as a document read from Markdown has it.
fn sheet() -> StyleSheet {
    StyleSheet::from_spec(&neutral())
}

/// Neutral's objects, as reading Markdown puts them in a document.
fn objects() -> &'static [(doc::ObjectId, doc::Object)] {
    static OBJECTS: std::sync::OnceLock<Vec<(doc::ObjectId, doc::Object)>> =
        std::sync::OnceLock::new();
    OBJECTS.get_or_init(|| neutral().objects(story::SHEET_AT))
}

fn examples() -> Vec<testkit::commonmark::Example> {
    testkit::commonmark::load().unwrap_or_else(|e| panic!("{e}"))
}

fn show(markdown: &str) -> String {
    format!("{markdown:?}")
}

#[test]
fn every_example_is_written_back_untouched_byte_for_byte() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: None,
        sizes: &|_| None,
    };
    let examples = examples();
    assert_eq!(examples.len(), 652);
    let mut failures = Vec::new();
    for e in &examples {
        let read = match markdown::read(e.markdown.as_bytes(), &settings) {
            Ok(read) => read,
            Err(err) => {
                failures.push(format!("example {}: {err}", e.number));
                continue;
            }
        };
        let mut file = read.file.clone();
        match file.write(&read.document, &sheet) {
            Ok(bytes) if bytes == e.markdown.as_bytes() => {}
            Ok(bytes) => failures.push(format!(
                "example {} ({}): {} written back as {}",
                e.number,
                e.section,
                show(&e.markdown),
                show(&String::from_utf8_lossy(&bytes))
            )),
            Err(err) => failures.push(format!("example {}: {err}", e.number)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} examples:\n{}",
        failures.len(),
        examples.len(),
        failures.join("\n")
    );
}

#[test]
fn every_example_written_afresh_reads_back_as_the_same_story() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: None,
        sizes: &|_| None,
    };
    let examples = examples();
    let mut failures = Vec::new();
    for e in &examples {
        let read = markdown::read(e.markdown.as_bytes(), &settings).expect("it reads");
        let written = match File::empty(None).write(&read.document, &sheet) {
            Ok(bytes) => bytes,
            Err(err) => {
                failures.push(format!(
                    "example {} ({}): {err}: {}",
                    e.number,
                    e.section,
                    show(&e.markdown)
                ));
                continue;
            }
        };
        let again = markdown::read(&written, &settings).expect("what's written reads");
        let (want, got) = (
            shape(&read.document, &sheet),
            shape(&again.document, &sheet),
        );
        if want != got {
            failures.push(format!(
                "example {} ({}): {} written as {}\n  read as {want:?}\n  read back as {got:?}",
                e.number,
                e.section,
                show(&e.markdown),
                show(&String::from_utf8_lossy(&written)),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} examples:\n{}",
        failures.len(),
        examples.len(),
        failures.join("\n")
    );
}

/// Each example with each of its paragraphs edited in turn, a word added
/// to its end: what's written reads back as the story edited, the rest
/// of it as the file had it.
#[test]
fn every_example_edited_reads_back_as_edited() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: None,
        sizes: &|_| None,
    };
    let examples = examples();
    let mut failures = Vec::new();
    let mut edits = 0;
    for e in &examples {
        let read = markdown::read(e.markdown.as_bytes(), &settings).expect("it reads");
        for id in read.document.paragraphs() {
            let p = read.document.paragraph(id).expect("a paragraph");
            if story::keys::SOURCE.is_empty() || p.props.contains_key(story::keys::SOURCE) {
                continue;
            }
            let mut document = read.document.clone();
            let len = p.len();
            document
                .apply(&doc::Op::InsertText {
                    paragraph: id,
                    offset: len,
                    text: " more".to_string(),
                    runs: vec![doc::Run::new(5, doc::Props::new())],
                })
                .expect("it applies");
            edits += 1;
            let mut file = read.file.clone();
            let written = match file.write(&document, &sheet) {
                Ok(bytes) => bytes,
                Err(err) => {
                    failures.push(format!(
                        "example {} ({}), {id}: {err}: {}",
                        e.number,
                        e.section,
                        show(&e.markdown)
                    ));
                    continue;
                }
            };
            let again = markdown::read(&written, &settings).expect("what's written reads");
            let (want, got) = (shape(&document, &sheet), shape(&again.document, &sheet));
            if want != got {
                failures.push(format!(
                    "example {} ({}), {id}: {} written as {}\n  want {want:?}\n  got  {got:?}",
                    e.number,
                    e.section,
                    show(&e.markdown),
                    show(&String::from_utf8_lossy(&written)),
                ));
            }
        }
    }
    assert!(edits > 500, "only {edits} edits");
    assert!(
        failures.is_empty(),
        "{} of {edits} edits:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
