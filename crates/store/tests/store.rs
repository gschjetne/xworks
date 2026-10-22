//! The `.xw` format (docs/ROADMAP.md, M1 stage 8): reopening gives the same
//! dump and the same renders; snapshots, versions and fonts; a newer schema
//! opens read-only; and what isn't a document, or is damaged, is refused.

use std::path::PathBuf;
use std::sync::OnceLock;

use doc::{Document, History, Op, Props, Run, Value};
use rusqlite::Connection;
use store::{Entry, Error, Store};
use text::Fonts;

fn scratch(name: &str) -> PathBuf {
    testkit::scratch_dir(&format!("store-{name}")).expect("a scratch directory")
}

fn spec(dir: &str, name: &str) -> String {
    testkit::specs(dir)
        .expect("spec/")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("spec/{dir}/{name}.ron"))
        .1
}

fn sample() -> Document {
    let paragraphs: Vec<(String, Vec<(String, Props)>)> =
        ron::from_str(&spec("documents", "m1-sample")).expect("a document");
    story::document(&paragraphs)
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            fonts.add(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let spec: text::FontSpec =
            ron::from_str(&testkit::fonts::spec().expect("fonts.ron")).expect("valid");
        fonts.set_fallbacks(&spec);
        fonts
    })
}

/// The document's pages drawn, as pixels.
fn renders(document: &Document) -> Vec<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct Sheet {
        page: canvas::PageStyleSpec,
    }
    let styles = spec("styles", "m1");
    let sheet: story::StyleSheet = ron::from_str(&styles).expect("a style sheet");
    let page: Sheet = ron::from_str(&styles).expect("a page style");
    let style = canvas::PageStyle::from_spec(&page.page);
    canvas::automatic(document, &sheet, &style, fonts())
        .iter()
        .map(|page| render_cpu::render(page, 36.0, render_cpu::Level::Portable).pixels)
        .collect()
}

/// An edit: `text` typed at the end of paragraph `index`.
fn typing(document: &mut Document, index: usize, text: &str) -> doc::Transaction {
    let paragraph = document.paragraphs()[index];
    let offset = document.paragraph(paragraph).expect("a paragraph").len();
    let len = text.chars().count() as u32;
    document
        .transact(
            "Typing",
            vec![Op::InsertText {
                paragraph,
                offset,
                text: text.to_string(),
                runs: vec![Run::new(len, Props::new())],
            }],
        )
        .expect("typing applies")
}

fn bold(document: &mut Document, index: usize) -> doc::Transaction {
    let paragraph = document.paragraphs()[index];
    let len = document.paragraph(paragraph).expect("a paragraph").len();
    document
        .transact(
            "Bold",
            vec![Op::SetCharProp {
                paragraph,
                offset: 0,
                len,
                key: "bold".to_string(),
                value: Some(Value::Bool(true)),
            }],
        )
        .expect("bold applies")
}

#[test]
fn reopening_gives_the_same_dump_and_the_same_renders() {
    let dir = scratch("reopen");
    let path = dir.join("sample.xw");
    let mut document = sample();
    let mut history = History::new();
    let mut store = Store::create(&path, &document, "story").expect("created");

    let t = typing(&mut document, 1, " Typed after it was made.");
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");
    history.push(t);
    let t = bold(&mut document, 2);
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");
    history.push(t);
    store.name_version("Draft").expect("named");
    // Undo the bold, redo it, undo it again: Redo holds it at the end.
    for undo in [true, false, true] {
        let t = if undo {
            history.undo(&mut document)
        } else {
            history.redo(&mut document)
        }
        .expect("applies")
        .expect("something to undo or redo")
        .clone();
        assert_eq!(t.name, "Bold");
        let entry = if undo {
            Entry::Undo(&t)
        } else {
            Entry::Redo(&t)
        };
        store.append(entry, &document).expect("appended");
    }
    let hash = store
        .embed_font(b"not really a font, but bytes all the same")
        .expect("embedded");
    assert_eq!(hash.len(), 64);
    let dump = store::dump(&path).expect("it dumps");
    drop(store);

    let opened = Store::open(&path).expect("it opens");
    assert_eq!(opened.document, document);
    assert_eq!(opened.history.undo_name(), Some("Typing"));
    assert_eq!(opened.history.redo_name(), Some("Bold"));
    assert_eq!(opened.store.seq(), 5);
    assert_eq!(store::dump(&path).expect("it dumps"), dump);
    assert!(dump.contains("versions\n    2 \"Draft\"\n"), "{dump}");
    assert_eq!(renders(&opened.document), renders(&document));
    // Redo still works after reopening.
    let (mut document, mut history) = (opened.document, opened.history);
    let redone = history
        .redo(&mut document)
        .expect("applies")
        .expect("a redo");
    assert_eq!(redone.name, "Bold");
}

#[test]
fn snapshots_bound_the_replay_and_versions_go_back() {
    let dir = scratch("snapshots");
    let path = dir.join("long.xw");
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    let mut states = vec![document.clone()];
    for i in 0..250 {
        let t = typing(&mut document, 0, &format!("{i} "));
        store
            .append(Entry::Commit(&t), &document)
            .expect("appended");
        states.push(document.clone());
        if i == 149 {
            store.name_version("Halfway").expect("named");
        }
    }
    drop(store);
    let connection = Connection::open(&path).expect("SQLite opens it");
    let snapshots: Vec<i64> = connection
        .prepare("SELECT seq FROM snapshot ORDER BY seq")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(snapshots, [0, 100, 200]);
    drop(connection);

    let opened = Store::open(&path).expect("it opens");
    assert_eq!(opened.document, document);
    let versions = opened.store.versions().expect("versions");
    assert_eq!(
        versions,
        [store::Version {
            name: "Halfway".to_string(),
            seq: 150
        }]
    );
    for seq in [0, 1, 99, 100, 101, 150, 199, 200, 250] {
        assert_eq!(
            opened.store.document_at(seq).expect("a version"),
            states[seq as usize],
            "after entry {seq}"
        );
    }
}

#[test]
fn a_newer_schema_opens_read_only() {
    let dir = scratch("newer");
    let path = dir.join("newer.xw");
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    let t = typing(&mut document, 0, "Written by an older xWorks.");
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");
    drop(store);
    let connection = Connection::open(&path).expect("SQLite opens it");
    connection
        .pragma_update(None, "user_version", 2)
        .expect("set");
    drop(connection);

    let mut opened = Store::open(&path).expect("it opens");
    assert_eq!(opened.store.read_only(), Some(2));
    assert_eq!(opened.document, document);
    let t = typing(&mut document, 0, " More.");
    assert!(matches!(
        opened.store.append(Entry::Commit(&t), &document),
        Err(Error::ReadOnly { schema: 2 })
    ));
    assert!(matches!(
        opened.store.name_version("No"),
        Err(Error::ReadOnly { schema: 2 })
    ));
    assert!(
        store::dump(&path)
            .expect("it dumps")
            .starts_with("xworks schema 2\n")
    );
}

#[test]
fn what_isnt_a_document_is_refused() {
    let dir = scratch("refused");
    let missing = dir.join("missing.xw");
    assert!(matches!(Store::open(&missing), Err(Error::NotADocument(_))));

    let text = dir.join("text.xw");
    std::fs::write(&text, "Not a database, only some text.").unwrap();
    assert!(Store::open(&text).is_err());

    let other = dir.join("other.xw");
    let connection = Connection::open(&other).unwrap();
    connection
        .execute_batch("CREATE TABLE t (x INTEGER);")
        .unwrap();
    drop(connection);
    assert!(matches!(Store::open(&other), Err(Error::NotADocument(_))));

    let path = dir.join("taken.xw");
    Store::create(&path, &Document::new(), "story").expect("created");
    assert!(matches!(
        Store::create(&path, &Document::new(), "story"),
        Err(Error::Exists(_))
    ));
}

#[test]
fn damage_is_an_error_not_a_document() {
    let dir = scratch("damaged");
    let path = dir.join("damaged.xw");
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    for text in ["one ", "two ", "three"] {
        let t = typing(&mut document, 0, text);
        store
            .append(Entry::Commit(&t), &document)
            .expect("appended");
    }
    drop(store);

    let damage = |sql: &str, value: Vec<u8>| {
        let copy = dir.join("copy.xw");
        std::fs::copy(&path, &copy).unwrap();
        let connection = Connection::open(&copy).unwrap();
        if sql.contains("?1") {
            connection.execute(sql, rusqlite::params![value]).unwrap();
        } else {
            connection.execute_batch(sql).unwrap();
        }
        drop(connection);
        let result = Store::open(&copy);
        std::fs::remove_file(&copy).unwrap();
        result
    };
    let none = Vec::new;
    // Operations that aren't operations, or don't apply.
    assert!(matches!(
        damage(
            "UPDATE log SET ops = ?1 WHERE seq = 2",
            vec![0xff, 0xff, 0xff]
        ),
        Err(Error::Damaged(_))
    ));
    let impossible = store::encode(&vec![Op::DeleteText {
        paragraph: doc::ObjectId(1),
        offset: 100,
        len: 1,
    }])
    .unwrap();
    assert!(matches!(
        damage("UPDATE log SET ops = ?1 WHERE seq = 2", impossible),
        Err(Error::Damaged(_))
    ));
    // A gap in the log, a snapshot that doesn't hold together, no kind.
    assert!(matches!(
        damage("DELETE FROM log WHERE seq = 2", none()),
        Err(Error::Damaged(_))
    ));
    let empty = store::encode(&doc::Snapshot {
        root: doc::ObjectId(1),
        next_id: 2,
        objects: Vec::new(),
    })
    .unwrap();
    assert!(matches!(
        damage("UPDATE snapshot SET document = ?1 WHERE seq = 0", empty),
        Err(Error::Damaged(_))
    ));
    assert!(matches!(
        damage("DELETE FROM meta", none()),
        Err(Error::Damaged(_))
    ));
    // An undo with nothing to undo.
    assert!(matches!(
        damage("UPDATE log SET kind = 'undo' WHERE seq = 1", none()),
        Err(Error::Damaged(_))
    ));
    // Untouched, it opens.
    assert!(Store::open(&path).is_ok());
}

/// postcard writes enums by their variants' positions, so the encoding of
/// operations is part of the format (spec/format/README.md). These bytes
/// change only with a new schema.
#[test]
fn the_encoding_is_pinned() {
    let ops = vec![
        Op::InsertText {
            paragraph: doc::ObjectId(1),
            offset: 2,
            text: "hé".to_string(),
            runs: vec![Run::new(
                2,
                Props::from([("bold".to_string(), Value::Bool(true))]),
            )],
        },
        Op::SetParaProp {
            paragraph: doc::ObjectId(300),
            key: "style".to_string(),
            value: Some(Value::Text("Body".to_string())),
        },
    ];
    let bytes = store::encode(&ops).unwrap();
    assert_eq!(
        bytes,
        [
            2, // two operations
            0, 1, 2, 3, b'h', 0xc3, 0xa9, // InsertText: #1 at 2, "hé" (3 bytes)
            1, 2, 1, 4, b'b', b'o', b'l', b'd', 0, 1, // a run of 2: bold = Bool(true)
            5, 0xac, 0x02, 5, b's', b't', b'y', b'l', b'e', // SetParaProp: #300, "style"
            1, 2, 4, b'B', b'o', b'd', b'y', // Some(Text("Body"))
        ]
    );
    assert_eq!(store::decode::<Vec<Op>>(&bytes).unwrap(), ops);
}
