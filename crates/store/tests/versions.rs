//! Pictures kept in the file, once each by their hash (spec/format/
//! schema-2.sql), and named versions copied out as documents of their own
//! (docs/ROADMAP.md, M2 stage 7).

use std::path::PathBuf;

use doc::{Document, Op, Props, Run};
use rusqlite::Connection;
use store::{Entry, Error, ReadOnly, Store};

fn scratch(name: &str) -> PathBuf {
    testkit::scratch_dir(&format!("store-versions-{name}")).expect("a scratch directory")
}

fn typing(document: &mut Document, text: &str) -> doc::Transaction {
    let paragraph = document.paragraphs()[0];
    let offset = document.paragraph(paragraph).expect("a paragraph").len();
    document
        .transact(
            "Typing",
            vec![Op::InsertText {
                paragraph,
                offset,
                text: text.to_string(),
                runs: vec![Run::new(text.chars().count() as u32, Props::new())],
            }],
        )
        .expect("typing applies")
}

#[test]
fn pictures_are_kept_once_by_their_hash() {
    let dir = scratch("images");
    let path = dir.join("pictures.xw");
    let document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    let png = b"\x89PNG\r\n\x1a\nnot a whole picture, but bytes to keep".to_vec();
    let jpeg = b"\xff\xd8\xffanother".to_vec();
    let a = store.add_image("png", &png).expect("kept");
    let again = store.add_image("png", &png).expect("kept again");
    let b = store.add_image("jpeg", &jpeg).expect("kept");
    assert_eq!(a, again, "the same bytes, the same picture");
    assert_eq!(a.len(), 64);
    assert_ne!(a, b);
    let mut ids = store.image_ids().expect("listed");
    ids.sort();
    let mut want = vec![a.clone(), b.clone()];
    want.sort();
    assert_eq!(ids, want);
    let image = store.image(&a).expect("read").expect("there");
    assert_eq!((image.format.as_str(), image.data), ("png", png.clone()));
    assert_eq!(store.image(&"0".repeat(64)).expect("read"), None);
    assert!(matches!(
        store.add_image("tiff", b"II*\0"),
        Err(Error::NotAPicture(_))
    ));
    let dump = store::dump(&path).expect("it dumps");
    assert!(dump.contains("\nimages\n"), "{dump}");
    assert!(dump.contains(&format!("    {b} jpeg 10 bytes\n")), "{dump}");
    assert!(dump.contains(&format!("    {a} png {} bytes\n", png.len())));
    drop(store);

    // Bytes that aren't what their ID says are damage, not a picture.
    let connection = Connection::open(&path).expect("SQLite opens it");
    connection
        .execute(
            "UPDATE image SET data = x'00' WHERE id = ?1",
            rusqlite::params![a],
        )
        .expect("damaged");
    drop(connection);
    let opened = Store::open(&path).expect("it opens");
    assert!(matches!(opened.store.image(&a), Err(Error::Damaged(_))));
    assert!(opened.store.image(&b).expect("read").is_some());
}

/// A font the document carries is kept once, by the SHA-256 of its bytes,
/// however often it's embedded, and reads back as it was.
#[test]
fn fonts_are_kept_once_by_their_hash() {
    let dir = scratch("fonts");
    let path = dir.join("fonts.xw");
    let mut store = Store::create(&path, &Document::new(), "story").expect("created");
    let a = store.embed_font(b"one font's bytes").expect("embedded");
    let again = store
        .embed_font(b"one font's bytes")
        .expect("embedded again");
    let b = store.embed_font(b"another font's bytes").expect("embedded");
    assert_eq!(a, again, "the same bytes, the same font");
    assert_ne!(a, b);
    assert_eq!(a, store::image_id(b"one font's bytes"), "its SHA-256");
    drop(store);
    let opened = Store::open(&path).expect("it opens");
    let mut want = vec![
        (a, b"one font's bytes".to_vec()),
        (b, b"another font's bytes".to_vec()),
    ];
    want.sort();
    assert_eq!(opened.store.fonts().expect("listed"), want);
}

#[test]
fn a_document_without_pictures_dumps_as_before() {
    let dir = scratch("no-images");
    let path = dir.join("plain.xw");
    drop(Store::create(&path, &Document::new(), "story").expect("created"));
    let dump = store::dump(&path).expect("it dumps");
    assert!(!dump.contains("images"), "{dump}");
}

#[test]
fn a_named_version_is_copied_out_as_a_document_of_its_own() {
    let dir = scratch("copy");
    let path = dir.join("history.xw");
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    let font = store.embed_font(b"a font's bytes").expect("embedded");
    let kept = store.add_image("png", b"\x89PNG kept").expect("kept");
    let left = store.add_image("gif", b"GIF89a left").expect("kept");
    for text in ["One. ", "Two. "] {
        let t = typing(&mut document, text);
        store
            .append(Entry::Commit(&t), &document)
            .expect("appended");
    }
    store.name_version("Draft").expect("named");
    let draft = document.clone();
    let t = typing(&mut document, "Three.");
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");

    let version = store.versions().expect("versions")[0].clone();
    assert_eq!(version.name, "Draft");
    let at = store.document_at(version.seq).expect("the version");
    assert_eq!(at, draft);
    let copy = dir.join("history (Draft).xw");
    store
        .copy_out(&copy, &at, std::slice::from_ref(&kept))
        .expect("copied out");
    assert!(matches!(
        store.copy_out(&copy, &at, &[]),
        Err(Error::Exists(_))
    ));
    drop(store);

    let opened = Store::open(&copy).expect("the copy opens");
    assert_eq!(opened.store.read_only(), None);
    assert_eq!(opened.document, draft);
    assert_eq!(opened.store.seq(), 0, "no history");
    assert_eq!(opened.history.undo_name(), None);
    assert!(opened.store.versions().expect("versions").is_empty());
    assert_eq!(
        opened.store.fonts().expect("fonts"),
        [(font, b"a font's bytes".to_vec())]
    );
    assert_eq!(opened.store.image_ids().expect("pictures"), [kept]);
    assert!(opened.store.image(&left).expect("read").is_none());

    // The original is as it was.
    let original = Store::open(&path).expect("it opens");
    assert_eq!(original.document, document);
    assert_eq!(original.store.seq(), 3);
}

#[test]
fn a_version_opened_to_read_cant_be_written() {
    let dir = scratch("read");
    let path = dir.join("read.xw");
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story").expect("created");
    let t = typing(&mut document, "Read me.");
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");
    store.name_version("First").expect("named");
    let mut read = Store::open_to_read(&path).expect("it opens to read");
    assert_eq!(read.store.read_only(), Some(&ReadOnly::Asked));
    let seq = read.store.versions().expect("versions")[0].seq;
    assert_eq!(read.store.document_at(seq).expect("the version"), document);
    assert!(matches!(
        read.store.name_version("No"),
        Err(Error::OpenToRead)
    ));
    let t = typing(&mut read.document, " Not here.");
    assert!(matches!(
        read.store.append(Entry::Commit(&t), &read.document),
        Err(Error::OpenToRead)
    ));
    // Meanwhile the writer goes on writing.
    let t = typing(&mut document, " Still writing.");
    store
        .append(Entry::Commit(&t), &document)
        .expect("appended");
}
