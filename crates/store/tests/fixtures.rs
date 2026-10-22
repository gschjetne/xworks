//! Files older xWorks wrote (spec/format/fixtures/): each opens with the
//! same text and history it was written with. A copy is opened, never the
//! fixture itself, since opening an older file upgrades it.

use std::path::PathBuf;

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

/// The dump M1 made of its own file (`m1.dump`) is the dump of the file
/// as opened now, but for the schema it names.
#[test]
fn m1s_file_opens_with_its_text_and_history() {
    let path = copy("m1.xw", "m1");
    let dump = store::dump(&path).expect("M1's file opens");
    let expected = std::fs::read_to_string(fixture("m1.dump")).expect("m1.dump");
    let (first, rest) = dump.split_once('\n').expect("a dump");
    let (m1_first, m1_rest) = expected.split_once('\n').expect("a dump");
    assert_eq!(m1_first, "xworks schema 1");
    assert_eq!(first, format!("xworks schema {}", store::SCHEMA));
    assert_eq!(
        rest, m1_rest,
        "the same document, log, versions and history"
    );

    // The named versions go back, one from each snapshot.
    let opened = Store::open(&path).expect("it opens again");
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

/// Opening M1's file upgrades it, once: it has a new file's tables, and
/// is today's schema, and opening it again finds it so.
#[test]
fn m1s_file_upgrades_to_a_new_files_tables() {
    let path = copy("m1.xw", "m1-upgrade");
    assert_eq!(schema(&path), 1);
    let dump = store::dump(&path).expect("it opens");
    assert_eq!(schema(&path), store::SCHEMA);
    let fresh = path.with_file_name("fresh.xw");
    drop(Store::create(&fresh, &doc::Document::new(), "story").expect("created"));
    assert_eq!(tables(&path), tables(&fresh));
    assert!(tables(&path).iter().any(|(name, _)| name == "image"));
    assert_eq!(store::dump(&path).expect("it opens again"), dump);
}

/// A schema 1 file that doesn't decode isn't upgraded: it's left as it
/// was, and opening it is an error.
#[test]
fn a_damaged_m1_file_is_left_as_it_was() {
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
}
