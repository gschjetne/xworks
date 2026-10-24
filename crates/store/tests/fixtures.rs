//! Files older xWorks wrote (spec/format/fixtures/): each opens with the
//! same text and history it was written with. A copy is opened, never the
//! fixture itself, since opening an older file upgrades it.
//!
//! Since schema 3 a document holds its own stylesheet, and its paragraphs
//! name their styles by ID: an older file gains the stylesheet it was laid
//! out with, frozen in `spec/format/upgrade-2.ron`. So a dump older code
//! made is compared with today's with the stylesheet's objects left out
//! and each style named by its name again: the same document, log,
//! versions and history.

use std::collections::BTreeMap;
use std::path::PathBuf;

use doc::{Object, ObjectId, Value};
use store::Store;

fn fixture(name: &str) -> PathBuf {
    testkit::workspace_root()
        .join("spec/format/fixtures")
        .join(name)
}

/// A copy of fixture `name` in a scratch folder of its own.
fn copy(name: &str, scratch: &str) -> PathBuf {
    let dir = testkit::scratch_dir(&format!("store-fixture-{scratch}")).expect("a folder");
    let path = dir.join(name);
    std::fs::copy(fixture(name), &path).expect("the fixture copies");
    path
}

/// The dump of the file at `path`, as older code would have made it: its
/// stylesheet's lines left out, and each reference to one of its styles
/// written as the style's name, as schema 2 named them. Returns it and
/// the schema line.
fn as_before(path: &std::path::Path) -> (String, String) {
    let dump = store::dump(path).expect("the file opens");
    let opened = Store::open(path).expect("it opens again");
    let document = &opened.document;
    let mut names: BTreeMap<ObjectId, String> = BTreeMap::new();
    let mut sheet: Vec<ObjectId> = document.sheet().into_iter().collect();
    for &id in document.styles() {
        sheet.push(id);
        if let Some(Value::Text(name)) = document.style(id).and_then(|s| s.props.get("name")) {
            names.insert(id, name.clone());
        }
    }
    let (first, rest) = dump.split_once('\n').expect("a dump");
    let mut out = String::new();
    for line in rest.lines() {
        if sheet.iter().any(|id| line.starts_with(&format!("{id} "))) {
            continue;
        }
        let mut line = line.to_string();
        for (id, name) in &names {
            line = line.replace(&format!("Ref({id})"), &format!("Text({name:?})"));
        }
        out.push_str(&line);
        out.push('\n');
    }
    (first.to_string(), out)
}

/// The dump older code made of its own file (`NAME.dump`) is the dump of
/// the file as opened now, as before.
fn opens_with_its_own_dump(name: &str, schema: i64) {
    let path = copy(&format!("{name}.xw"), name);
    let (first, rest) = as_before(&path);
    let expected = std::fs::read_to_string(fixture(&format!("{name}.dump"))).expect("the dump");
    let (old_first, old_rest) = expected.split_once('\n').expect("a dump");
    assert_eq!(old_first, format!("xworks schema {schema}"));
    assert_eq!(first, format!("xworks schema {}", store::SCHEMA));
    assert_eq!(
        rest, old_rest,
        "the same document, log, versions and history"
    );
}

#[test]
fn m1s_file_opens_with_its_text_and_history() {
    opens_with_its_own_dump("m1", 1);
    let path = copy("m1.xw", "m1-history");

    // The named versions go back, one from each snapshot.
    let opened = Store::open(&path).expect("it opens");
    let versions = opened.store.versions().expect("versions");
    let texts = |seq: i64| -> Vec<String> {
        let document = opened.store.document_at(seq).expect("a version");
        document
            .paragraphs()
            .iter()
            .map(|&p| document.paragraph(p).expect("a paragraph").text.clone())
            .collect()
    };
    assert_eq!(versions.len(), 2);
    assert_eq!(
        texts(versions[0].seq),
        [
            "The M1 fixture",
            "A document written by M1, kept to show that later versions open its text, its formatting and its history.",
        ]
    );
    let second = texts(versions[1].seq);
    assert_eq!(second.len(), 7);
    assert!(second[6].ends_with("never really theirs to keep."));

    // Undo and Redo pick up where M1 left them.
    let (mut document, mut history) = (opened.document, opened.history);
    let last = document.paragraphs().len();
    let redone = history
        .redo(&mut document)
        .expect("applies")
        .expect("a redo")
        .name
        .clone();
    assert_eq!(redone, "New Paragraph");
    assert_eq!(document.paragraphs().len(), last + 1);
    // Undoing it and then the run of typing a word at a time empties the
    // paragraph that run filled.
    history.undo(&mut document).expect("applies");
    history.undo(&mut document).expect("applies");
    let paragraphs = document.paragraphs().to_vec();
    assert_eq!(paragraphs.len(), last);
    assert_eq!(document.paragraph(paragraphs[last - 1]).unwrap().text, "");
}

/// The text and the style of each paragraph of `document`, as its sheet
/// names it.
fn styled(document: &doc::Document) -> Vec<(String, String)> {
    document
        .paragraphs()
        .iter()
        .map(|&id| {
            let p = document.paragraph(id).expect("a paragraph");
            let style = match p.props.get("style") {
                Some(Value::Ref(s)) => match document.style(*s).and_then(|s| s.props.get("name")) {
                    Some(Value::Text(name)) => name.clone(),
                    _ => "?".to_string(),
                },
                _ => String::new(),
            };
            (style, p.text.clone())
        })
        .collect()
}

/// The file M2 wrote before its stage 9, schema 2's last, opens with its
/// own dump, its named versions and its history: blocks, marks,
/// paragraphs' own properties and a picture.
#[test]
fn m2s_file_opens_with_its_text_and_history() {
    opens_with_its_own_dump("m2", 2);
    let path = copy("m2.xw", "m2-history");
    let opened = Store::open(&path).expect("it opens");
    let document = &opened.document;
    // Its stylesheet is Neutral's first version, which it was laid out in.
    let sheet = document.stylesheet().expect("a stylesheet");
    assert_eq!(
        sheet.props.get("origin"),
        Some(&Value::Text("Neutral".into()))
    );
    assert_eq!(sheet.props.get("origin-version"), Some(&Value::Int(1)));
    let styles = styled(document);
    assert_eq!(
        styles[0],
        ("Heading 1".to_string(), "The M2 fixture".to_string())
    );
    assert!(
        styles
            .iter()
            .any(|(s, t)| s == "Code" && t.starts_with("let answer"))
    );
    // Table cells name no style, as before.
    assert!(styles.iter().any(|(s, t)| s.is_empty() && t == "Pear"));

    let versions = opened.store.versions().expect("versions");
    assert_eq!(versions.len(), 2);
    let first = opened
        .store
        .document_at(versions[0].seq)
        .expect("a version");
    assert_eq!(
        styled(&first),
        [
            ("Heading 1".to_string(), "The M2 fixture".to_string()),
            (
                "Body".to_string(),
                "A document written by M2, kept to show that later versions open its blocks, its marks, its code, struck text and its history.".to_string()
            ),
        ]
    );
    let second = opened
        .store
        .document_at(versions[1].seq)
        .expect("a version");
    assert!(
        styled(&second)
            .last()
            .unwrap()
            .1
            .ends_with("the same history.")
    );

    // Undo and Redo pick up where M2 left them, styles and all.
    let (mut document, mut history) = (opened.document.clone(), opened.history);
    let before = styled(&document);
    let redone = history
        .redo(&mut document)
        .expect("applies")
        .expect("a redo")
        .name
        .clone();
    assert_eq!(redone, "New Paragraph");
    assert_eq!(styled(&document).len(), before.len() + 1);
    assert_eq!(styled(&document).last().unwrap().0, "Body");
    while history.undo(&mut document).expect("applies").is_some() {}
    assert_eq!(document.paragraphs().len(), 1);
    // A document every step of whose history held together reads back.
    assert!(doc::Document::from_snapshot(document.to_snapshot()).is_ok());
}

/// The tables of the file at `path`, as SQLite records their definitions.
fn tables(path: &std::path::Path) -> Vec<(String, String)> {
    let connection = rusqlite::Connection::open(path).expect("SQLite opens it");
    let mut statement = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'table' ORDER BY name")
        .expect("a query");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the tables")
        .collect::<Result<_, _>>()
        .expect("the tables")
}

fn schema(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .expect("SQLite opens it")
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("a version")
}

/// Opening an older file upgrades it, once: it has a new file's tables,
/// and is today's schema, and opening it again finds it so.
#[test]
fn older_files_upgrade_to_a_new_files_tables() {
    for (name, was) in [("m1", 1), ("m2", 2)] {
        let path = copy(&format!("{name}.xw"), &format!("{name}-upgrade"));
        assert_eq!(schema(&path), was);
        let dump = store::dump(&path).expect("it opens");
        assert_eq!(schema(&path), store::SCHEMA);
        let fresh = path.with_file_name("fresh.xw");
        drop(Store::create(&fresh, &doc::Document::new(), "story").expect("created"));
        assert_eq!(tables(&path), tables(&fresh));
        assert!(tables(&path).iter().any(|(name, _)| name == "image"));
        assert_eq!(store::dump(&path).expect("it opens again"), dump);
    }
}

/// An older file that doesn't decode isn't upgraded: it's left as it
/// was, and opening it is an error.
#[test]
fn a_damaged_older_file_is_left_as_it_was() {
    let path = copy("m1.xw", "m1-damaged");
    let connection = rusqlite::Connection::open(&path).expect("SQLite opens it");
    connection
        .execute(
            "UPDATE snapshot SET document = ?1 WHERE seq = 100",
            rusqlite::params![vec![0xffu8; 8]],
        )
        .expect("damaged");
    drop(connection);
    assert!(matches!(Store::open(&path), Err(store::Error::Damaged(_))));
    assert_eq!(schema(&path), 1);
    assert!(!tables(&path).iter().any(|(name, _)| name == "image"));

    let path = copy("m2.xw", "m2-damaged");
    let connection = rusqlite::Connection::open(&path).expect("SQLite opens it");
    connection
        .execute(
            "UPDATE log SET ops = ?1 WHERE seq = 50",
            rusqlite::params![vec![0xffu8; 8]],
        )
        .expect("damaged");
    drop(connection);
    let before = std::fs::read(&path).expect("the file");
    assert!(matches!(Store::open(&path), Err(store::Error::Damaged(_))));
    assert_eq!(schema(&path), 2);
    assert_eq!(std::fs::read(&path).expect("the file"), before);
}

/// The stylesheet a schema 2 document gains is Neutral as it was at
/// version 1, which schema 2's documents were laid out in: the frozen
/// copy is exactly what the sheet that comes with xWorks makes, while
/// that sheet is still at version 1.
#[test]
fn schema_2s_stylesheet_is_neutral_at_version_1() {
    let source = std::fs::read_to_string(testkit::workspace_root().join("spec/styles/neutral.ron"))
        .expect("neutral.ron");
    let neutral = story::SheetSpec::parse(&source).expect("a stylesheet");
    let frozen_path = testkit::workspace_root().join("spec/format/upgrade-2.ron");
    let frozen: Vec<(ObjectId, Object)> =
        ron::from_str(&std::fs::read_to_string(&frozen_path).expect("upgrade-2.ron"))
            .expect("the frozen sheet reads");
    if neutral.version != 1 {
        return;
    }
    let made = neutral.objects(0);
    if made != frozen {
        let written =
            ron::ser::to_string_pretty(&made, ron::ser::PrettyConfig::new()).expect("it writes");
        let out = testkit::scratch_dir("store-upgrade-2")
            .expect("a folder")
            .join("upgrade-2.ron");
        std::fs::write(&out, written).expect("written");
        panic!(
            "spec/format/upgrade-2.ron isn't Neutral at version 1; what it makes is in {}",
            out.display()
        );
    }
}
