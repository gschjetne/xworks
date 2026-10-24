//! GitHub's autolinks (review jy4a2tx5), by our own cases after the
//! behaviour of its spec's examples (spec/markdown/autolinks.ron): each
//! read as the links it says, written back untouched byte for byte, and
//! written afresh, bare where they read back the same, reading back as
//! the same story.

use doc::{Document, Value};
use markdown::{File, Settings, shape};
use serde::Deserialize;
use story::{StyleSheet, keys};

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

#[derive(Deserialize)]
struct Case {
    markdown: String,
    links: Vec<(String, String)>,
}

fn cases() -> Vec<Case> {
    let source = testkit::specs("markdown")
        .expect("spec/markdown")
        .into_iter()
        .find(|(name, _)| name == "autolinks")
        .expect("spec/markdown/autolinks.ron")
        .1;
    ron::from_str(&source).expect("the cases read")
}

/// Every link in `document`, in reading order: its text and its address.
fn links(document: &Document) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for id in document.paragraphs() {
        let p = document.paragraph(id).expect("a paragraph");
        let mut chars = p.text.chars();
        let mut last: Option<String> = None;
        for run in &p.runs {
            let text: String = chars.by_ref().take(run.len as usize).collect();
            match run.props.get(keys::LINK) {
                Some(Value::Text(url)) if last.as_ref() == Some(url) => {
                    let (t, _) = out.last_mut().expect("the link");
                    let t: &mut String = t;
                    t.push_str(&text);
                }
                Some(Value::Text(url)) => {
                    out.push((text, url.clone()));
                    last = Some(url.clone());
                }
                _ => last = None,
            }
        }
    }
    out
}

#[test]
fn every_case_reads_as_its_links_and_writes_back() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: None,
        sizes: &|_| None,
    };
    let cases = cases();
    assert!(cases.len() >= 30, "{} cases", cases.len());
    let mut failures = Vec::new();
    for case in &cases {
        let read = markdown::read(case.markdown.as_bytes(), &settings).expect("it reads");
        let got = links(&read.document);
        if got != case.links {
            failures.push(format!(
                "{:?} reads as links {got:?}, not {:?}",
                case.markdown, case.links
            ));
            continue;
        }
        let untouched = read
            .file
            .clone()
            .write(&read.document, &sheet)
            .expect("it writes");
        if untouched != case.markdown.as_bytes() {
            failures.push(format!(
                "{:?} written back as {:?}",
                case.markdown,
                String::from_utf8_lossy(&untouched)
            ));
        }
        let fresh = match File::empty(None).write(&read.document, &sheet) {
            Ok(fresh) => fresh,
            Err(e) => {
                failures.push(format!("{:?} isn't written afresh: {e}", case.markdown));
                continue;
            }
        };
        let again = markdown::read(&fresh, &settings).expect("it reads");
        if shape(&again.document, &sheet) != shape(&read.document, &sheet) {
            failures.push(format!(
                "{:?} written afresh as {:?} reads back otherwise",
                case.markdown,
                String::from_utf8_lossy(&fresh)
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A link whose text is its address is written afresh bare, where that
/// reads back the same; in angle brackets where the file wrote it so; and
/// as a link of text and target where bare it would read as more.
#[test]
fn links_whose_text_is_their_address_are_written_bare() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: None,
        sizes: &|_| None,
    };
    for (markdown, fresh) in [
        ("Visit www.example.net.\n", "Visit www.example.net.\n"),
        (
            "Mail [me@example.net](mailto:me@example.net)\n",
            "Mail me@example.net\n",
        ),
        (
            "[https://example.net](https://example.net)x\n",
            "<https://example.net>x\n",
        ),
        (
            "[www.example.net](http://www.example.net)x\n",
            "[www.example.net](http://www.example.net)x\n",
        ),
        ("Plain www\\.example.net\n", "Plain www\\.example.net\n"),
    ] {
        let read = markdown::read(markdown.as_bytes(), &settings).expect("it reads");
        let written = File::empty(None)
            .write(&read.document, &sheet)
            .expect("it writes");
        assert_eq!(String::from_utf8_lossy(&written), fresh, "{markdown:?}");
    }
    // Edited, a paragraph keeps its angle brackets.
    let read = markdown::read(b"See <https://example.net> now\n", &settings).expect("it reads");
    let id = read.document.paragraphs()[0];
    let mut edited = read.document.clone();
    edited
        .apply(&doc::Op::DeleteText {
            paragraph: id,
            offset: 0,
            len: 4,
        })
        .expect("it applies");
    let written = read.file.clone().write(&edited, &sheet).expect("it writes");
    assert_eq!(
        String::from_utf8_lossy(&written),
        "<https://example.net> now\n"
    );
}
