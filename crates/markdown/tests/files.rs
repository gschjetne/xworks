//! Markdown files of our own, as people write them
//! (`spec/markdown/files/`): each written back untouched, byte for byte;
//! with a word edited, one line changed and the rest as they were; and
//! what's written anew written in each file's habits.

use doc::{Document, ObjectId, Op, Props, Run, Value};
use markdown::{Read, Settings, shape};
use story::{StyleSheet, keys};

fn sheet() -> StyleSheet {
    let source = testkit::specs("styles")
        .expect("spec/styles")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("spec/styles/m1.ron")
        .1;
    ron::from_str(&source).expect("m1.ron is a style sheet")
}

/// Every file, by name, as its bytes.
fn files() -> Vec<(String, Vec<u8>)> {
    let out = testkit::spec_bytes("markdown/files", "md").expect("spec/markdown/files");
    assert!(out.len() >= 6, "the files are there");
    out
}

fn file(name: &str) -> Vec<u8> {
    files()
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name}"))
        .1
}

fn read(bytes: &[u8], sheet: &StyleSheet) -> Read {
    let settings = Settings {
        sheet,
        language: Some("en-US"),
        sizes: &|_| None,
    };
    markdown::read(bytes, &settings).expect("it reads")
}

/// The first paragraph whose text has `needle`, and where it starts in
/// characters.
fn find(document: &Document, needle: &str) -> (ObjectId, u32) {
    document
        .paragraphs()
        .into_iter()
        .find_map(|id| {
            let text = &document.paragraph(id)?.text;
            text.find(needle)
                .map(|b| (id, text[..b].chars().count() as u32))
        })
        .unwrap_or_else(|| panic!("{needle:?} isn't in the document"))
}

/// `document` with the first `old` replaced by `new`, which takes the
/// properties of the text it replaces.
fn replace(document: &Document, old: &str, new: &str) -> Document {
    let (id, at) = find(document, old);
    let p = document.paragraph(id).expect("a paragraph");
    let mut pos = 0;
    let props = p
        .runs
        .iter()
        .find_map(|r| {
            let found = (at < pos + r.len).then(|| (*r.props).clone());
            pos += r.len;
            found
        })
        .unwrap_or_default();
    let mut document = document.clone();
    document
        .apply_all(&[
            Op::DeleteText {
                paragraph: id,
                offset: at,
                len: old.chars().count() as u32,
            },
            Op::InsertText {
                paragraph: id,
                offset: at,
                text: new.to_string(),
                runs: vec![Run::new(new.chars().count() as u32, props)],
            },
        ])
        .expect("it applies");
    document
}

/// The lines of `a` and `b` that differ, where they have as many.
fn changed(a: &[u8], b: &[u8]) -> Vec<(String, String)> {
    let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('\n').collect(), b.split('\n').collect());
    assert_eq!(a.len(), b.len(), "as many lines");
    a.iter()
        .zip(&b)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

#[test]
fn untouched_files_are_written_back_byte_for_byte() {
    let sheet = sheet();
    for (name, bytes) in files() {
        let mut read = read(&bytes, &sheet);
        let written = read.file.write(&read.document, &sheet).expect("it writes");
        assert!(
            written == bytes,
            "{name} was written back as\n{}",
            String::from_utf8_lossy(&written)
        );
    }
}

/// Every word of four letters or more in every file, but in code, made
/// capitals in turn: the file changes in that word's line alone, and
/// reads back as the story edited.
#[test]
fn a_one_word_edit_changes_one_line() {
    let sheet = sheet();
    let mut edits = 0;
    for (name, bytes) in files() {
        let read = read(&bytes, &sheet);
        for id in read.document.paragraphs() {
            let p = read.document.paragraph(id).expect("a paragraph");
            if p.props.contains_key(keys::SOURCE) || sheet.style_of(p).code {
                continue;
            }
            for word in p.text.split(|c: char| !c.is_ascii_alphabetic()) {
                if word.len() < 4 || word.chars().all(|c| c.is_ascii_uppercase()) {
                    continue;
                }
                let edited = replace(&read.document, word, &word.to_ascii_uppercase());
                let mut file = read.file.clone();
                let written = file
                    .write(&edited, &sheet)
                    .unwrap_or_else(|e| panic!("{name}: {word:?} to capitals: {e:?}"));
                let changes = changed(&bytes, &written);
                assert_eq!(
                    changes.len(),
                    1,
                    "{name}: {word:?} to capitals changed {changes:#?}"
                );
                assert_eq!(
                    changes[0].1,
                    changes[0].0.replacen(word, &word.to_ascii_uppercase(), 1),
                    "{name}: {word:?}"
                );
                let again = self::read(&written, &sheet);
                assert_eq!(
                    shape(&edited, &sheet),
                    shape(&again.document, &sheet),
                    "{name}: {word:?}"
                );
                edits += 1;
                // One word a paragraph is enough.
                break;
            }
        }
    }
    assert!(edits >= 40, "only {edits} edits");
}

/// A paragraph added to a list is written in the file's habits: its
/// bullets and its emphasis.
#[test]
fn new_items_follow_the_files_habits() {
    let sheet = sheet();
    for (name, after, bullet, emphasis) in [
        ("notes.md", "Support stays on the forum for now.", "* ", "_"),
        (
            "changelog.md",
            "Plans can be printed in Norwegian and German.",
            "+ ",
            "*",
        ),
        ("readme.md", "for other tools.", "- ", "*"),
    ] {
        let bytes = file(name);
        let read = read(&bytes, &sheet);
        let (id, _) = find(&read.document, after);
        let item = read.document.parent(id).expect("an item");
        let list = read.document.parent(item).expect("a list");
        let mut document = read.document.clone();
        let (new_item, new_p) = (document.new_id(), document.new_id());
        let italic = Props::from([(keys::ITALIC.to_string(), Value::Bool(true))]);
        let tag = read.file.language().expect("a language").to_string();
        let language = Props::from([(keys::LANGUAGE.to_string(), Value::Text(tag))]);
        let mut paragraph_props = language;
        paragraph_props.insert(keys::STYLE.to_string(), Value::Text("Body".into()));
        document
            .apply(&Op::InsertBlock {
                parent: list,
                after: Some(item),
                objects: vec![
                    (
                        new_item,
                        doc::Object::Block(doc::Block {
                            kind: doc::Kind::Item,
                            parent: list,
                            children: vec![new_p],
                            props: Props::new(),
                        }),
                    ),
                    (
                        new_p,
                        doc::Object::Paragraph(doc::Paragraph {
                            parent: new_item,
                            text: "A new one, quite new.".into(),
                            runs: vec![
                                Run::new(11, Props::new()),
                                Run::new(5, italic),
                                Run::new(5, Props::new()),
                            ],
                            props: paragraph_props,
                        }),
                    ),
                ],
            })
            .expect("it applies");
        let mut file = read.file.clone();
        let written =
            String::from_utf8(file.write(&document, &sheet).expect("it writes")).expect("UTF-8");
        let line = format!("{bullet}A new one, {emphasis}quite{emphasis} new.");
        assert!(written.contains(&line), "{name}: no {line:?} in\n{written}");
        // And nothing else changed: it's one line more.
        let before = String::from_utf8_lossy(&bytes).lines().count();
        assert_eq!(written.lines().count(), before + 1, "{name}:\n{written}");
    }
}

/// A paragraph edited in a file wrapped at a width is wrapped there, and
/// the file's line endings and byte order mark stay.
#[test]
fn edits_keep_the_files_width_line_endings_and_mark() {
    let sheet = sheet();
    let bytes = file("notes.md");
    let read = read(&bytes, &sheet);
    let longer = replace(
        &read.document,
        "Ingrid chaired;",
        "Ingrid chaired, as she has since the spring, and kept us to time;",
    );
    let mut file = read.file.clone();
    let written =
        String::from_utf8(file.write(&longer, &sheet).expect("it writes")).expect("UTF-8");
    let width = String::from_utf8_lossy(&bytes)
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap();
    assert!(
        written.lines().all(|l| l.chars().count() <= width),
        "wider than {width}:\n{written}"
    );
    assert_eq!(
        shape(&longer, &sheet),
        shape(&self::read(written.as_bytes(), &sheet).document, &sheet)
    );

    let bytes = self::file("windows.md");
    let read = self::read(&bytes, &sheet);
    assert!(read.file.bom() && read.file.line_ending() == "\r\n");
    let edited = replace(&read.document, "Bread", "Rye bread");
    let mut file = read.file.clone();
    let written = file.write(&edited, &sheet).expect("it writes");
    assert!(written.starts_with(b"\xef\xbb\xbf"));
    let text = String::from_utf8(written[3..].to_vec()).expect("UTF-8");
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "a bare line feed in\n{text}"
    );
    assert!(text.contains("2. Rye bread\r\n"), "{text}");
}

/// Toggling a task changes its box alone.
#[test]
fn a_task_toggled_changes_its_box_alone() {
    let sheet = sheet();
    let bytes = file("notes.md");
    let read = read(&bytes, &sheet);
    let (id, _) = find(&read.document, "Draft the release notes");
    let item = read.document.parent(id).expect("an item");
    let mut document = read.document.clone();
    document
        .apply(&Op::SetBlockProp {
            block: item,
            key: keys::TASK.to_string(),
            value: Some(Value::Bool(true)),
        })
        .expect("it applies");
    let mut file = read.file.clone();
    let written = file.write(&document, &sheet).expect("it writes");
    assert_eq!(
        changed(&bytes, &written),
        [(
            "- [ ] Draft the release notes (Tomás)".to_string(),
            "- [x] Draft the release notes (Tomás)".to_string()
        )]
    );
}

/// The file's front matter is kept, and its language is the paragraphs'.
#[test]
fn front_matter_is_kept_and_gives_the_language() {
    let sheet = sheet();
    let read = read(&file("notes.md"), &sheet);
    assert_eq!(read.file.language(), Some("en-GB"));
    assert!(
        read.file
            .front_matter()
            .is_some_and(|f| f.contains("tags: [planning, q4]"))
    );
    for id in read.document.paragraphs() {
        let p = read.document.paragraph(id).expect("a paragraph");
        assert_eq!(
            p.props.get(keys::LANGUAGE),
            Some(&Value::Text("en-GB".into()))
        );
    }
}

/// Lists begun on one line, their bullets alone on it once the text after
/// them is gone, aren't written as a thematic break, which three of one
/// bullet would read as.
#[test]
fn lists_begun_on_one_line_and_emptied_stay_lists() {
    let sheet = sheet();
    for bytes in [&b"- - - Beans\n"[..], b"* * * Beans\n", b"1. - - - Beans\n"] {
        let read = read(bytes, &sheet);
        let (id, at) = find(&read.document, "Beans");
        let mut document = read.document.clone();
        document
            .apply(&Op::DeleteText {
                paragraph: id,
                offset: at,
                len: 5,
            })
            .expect("it applies");
        let edited = read
            .file
            .clone()
            .write(&document, &sheet)
            .expect("it writes");
        let fresh = markdown::File::empty(read.file.language())
            .write(&document, &sheet)
            .expect("it writes");
        for written in [edited, fresh] {
            let again = self::read(&written, &sheet);
            assert_eq!(
                shape(&again.document, &sheet),
                shape(&document, &sheet),
                "{}",
                String::from_utf8_lossy(&written)
            );
        }
    }
}
