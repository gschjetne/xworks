//! Named versions (docs/ROADMAP.md, M2 stage 7): Versions lists those
//! named, newest first, and one chosen opens read-only, as it was, while
//! the document goes on being written; Copy Out makes it a document of its
//! own, with its pictures. A document is never open to write twice.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use app::{Choice, Editor, Effect, Event, Resources};
use chrome::{Key, Modifiers};
use scene::Item;
use story::OBJECT;

fn resources(name: &str) -> Arc<Resources> {
    static RES: OnceLock<Resources> = OnceLock::new();
    let res = RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    });
    let mut res = res.clone();
    res.documents = testkit::scratch_dir(&format!("app-versions-{name}")).expect("a folder");
    Arc::new(res)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pictures")
        .join(name)
}

fn key(editor: &mut Editor, key: Key) -> Vec<Effect> {
    editor.update(Event::Key {
        key,
        modifiers: Modifiers::default(),
    })
}

/// The caret to the document's end, as Ctrl+End puts it.
fn to_end(editor: &mut Editor) {
    editor.update(Event::Key {
        key: Key::End,
        modifiers: Modifiers {
            primary: true,
            ..Modifiers::default()
        },
    });
}

fn command(editor: &mut Editor, id: &str) -> Vec<Effect> {
    editor.update(Event::Command(id.to_string()))
}

fn text(editor: &Editor) -> String {
    let document = editor.document();
    document
        .paragraphs()
        .iter()
        .map(|&id| document.paragraph(id).expect("a paragraph").text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn pictures_drawn(editor: &Editor) -> usize {
    editor
        .pages()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| matches!(item, Item::Image { .. }))
        .count()
}

#[test]
fn a_version_opens_as_it_was_and_copies_out_with_its_pictures() {
    let res = resources("copy");
    let mut editor = Editor::create(res.clone()).expect("a document");
    // With none named, Versions says how to name one.
    command(&mut editor, "versions");
    assert!(editor.message().starts_with("No versions yet"));
    editor.update(Event::Text("A picture: ".to_string()));
    command(&mut editor, "insert-picture");
    editor.update(Event::Chosen(
        Choice::Picture,
        Some(fixture("gradient.jpg")),
    ));
    command(&mut editor, "save-version");
    editor.update(Event::Text(" Then more.".to_string()));
    command(&mut editor, "save-version");
    editor.update(Event::Text(" And the last.".to_string()));
    let path = editor.path().to_path_buf();

    // Versions, newest first: Down to the first, Enter.
    assert!(command(&mut editor, "versions").is_empty());
    assert!(key(&mut editor, Key::Down).is_empty());
    let effects = key(&mut editor, Key::Enter);
    let [Effect::OpenVersion(at, seq)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(at, &path);
    let mut first = Editor::open_version(res.clone(), &path, *seq).expect("the version");
    assert_eq!(text(&first), format!("A picture: {OBJECT}"));
    assert_eq!(first.version().map(|v| v.name.as_str()), Some("Version 1"));
    assert_eq!(first.title(), "Untitled (Version 1)");
    assert_eq!(pictures_drawn(&first), 1);
    // Read-only: nothing changes it, nor names it.
    first.update(Event::Text("No.".to_string()));
    command(&mut first, "save-version");
    assert_eq!(text(&first), format!("A picture: {OBJECT}"));
    assert!(!first.state("save-version").enabled);
    assert!(!first.state("insert-picture").enabled);
    assert!(!editor.state("copy-out").enabled);
    // Meanwhile the document goes on.
    editor.update(Event::Text(" Still.".to_string()));
    assert!(text(&editor).ends_with("And the last. Still."));

    // Copied out, it's a document of its own, beside the original, with
    // its picture and no history.
    let effects = command(&mut first, "copy-out");
    let [Effect::Open(Some(copy))] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        copy.file_name().and_then(|n| n.to_str()),
        Some("Untitled (Version 1).xw")
    );
    assert_eq!(copy.parent(), path.parent());
    let mut copied = Editor::open(res.clone(), copy).expect("the copy opens");
    assert_eq!(text(&copied), format!("A picture: {OBJECT}"));
    assert_eq!(pictures_drawn(&copied), 1);
    assert!(!copied.state("undo").enabled);
    to_end(&mut copied);
    copied.update(Event::Text(" Mine now.".to_string()));
    assert_eq!(text(&copied), format!("A picture: {OBJECT} Mine now."));
    // Copied again, it takes another name.
    let effects = command(&mut first, "copy-out");
    let [Effect::Open(Some(again))] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        again.file_name().and_then(|n| n.to_str()),
        Some("Untitled (Version 1) 2.xw")
    );
}

#[test]
fn a_document_open_to_write_opens_read_only_again_and_says_why() {
    let res = resources("twice");
    let mut editor = Editor::create(res.clone()).expect("a document");
    editor.update(Event::Text("Mine.".to_string()));
    let path = editor.path().to_path_buf();
    let mut second = Editor::open(res.clone(), &path).expect("it opens");
    assert!(
        second.message().starts_with("Read-only: open in xWorks")
            || second.message() == "Read-only: open in another program",
        "{}",
        second.message()
    );
    second.update(Event::Text("Not mine.".to_string()));
    assert_eq!(text(&second), "Mine.");
    assert!(!second.state("undo").enabled);
    drop(second);
    drop(editor);
    let mut again = Editor::open(res, &path).expect("it opens");
    to_end(&mut again);
    again.update(Event::Text(" Again.".to_string()));
    assert_eq!(text(&again), "Mine. Again.");
}
