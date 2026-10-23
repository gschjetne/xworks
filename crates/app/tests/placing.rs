//! Pictures placed in documents (docs/ROADMAP.md, M2 stage 7): Insert
//! Picture asks the shell for a file, and the file chosen is placed at the
//! caret, kept in the document's file once by its hash, drawn, and
//! printed as a figure with its alternative text; an sRGB JPEG as the file
//! it is. Placed pictures survive reopening, undo and redo.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use app::{Choice, Editor, Effect, Event, Resources};
use doc::Value;
use scene::{Item, Role};
use story::{OBJECT, keys};
use text::units::PIXEL;

fn resources(name: &str) -> Arc<Resources> {
    static RES: OnceLock<Resources> = OnceLock::new();
    let res = RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    });
    let mut res = res.clone();
    res.documents = testkit::scratch_dir(&format!("app-placing-{name}")).expect("a folder");
    Arc::new(res)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pictures")
        .join(name)
}

fn typed(editor: &mut Editor, text: &str) {
    editor.update(Event::Text(text.to_string()));
}

fn command(editor: &mut Editor, id: &str) -> Vec<Effect> {
    editor.update(Event::Command(id.to_string()))
}

/// Insert Picture, and `path` chosen in the dialog it asks for.
fn place(editor: &mut Editor, path: &Path) {
    assert_eq!(
        command(editor, "insert-picture"),
        [Effect::Choose(Choice::Picture)]
    );
    editor.update(Event::Chosen(Choice::Picture, Some(path.to_path_buf())));
}

fn text_of(editor: &Editor) -> Vec<String> {
    let document = editor.document();
    document
        .paragraphs()
        .iter()
        .map(|&id| document.paragraph(id).expect("a paragraph").text.clone())
        .collect()
}

/// The pictures the pages draw: each one's size and alternative text.
fn drawn(editor: &Editor) -> Vec<(i64, i64, Option<String>)> {
    editor
        .pages()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Image { rect, alt, .. } => Some((rect.width, rect.height, alt.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_placed_picture_survives_reopening_undo_and_redo() {
    let res = resources("survives");
    let mut editor = Editor::create(res.clone()).expect("a document");
    editor.update(Event::Resize {
        width: 900.0,
        height: 700.0,
    });
    typed(&mut editor, "A gradient: ");
    let jpeg = fixture("gradient.jpg");
    place(&mut editor, &jpeg);
    typed(&mut editor, " and more.");
    assert_eq!(
        text_of(&editor),
        [format!("A gradient: {OBJECT} and more.")]
    );
    // Its run names it, at its own size at 96 pixels to the inch.
    let document = editor.document();
    let p = document
        .paragraph(document.paragraphs()[0])
        .expect("a paragraph");
    let run = p
        .runs
        .iter()
        .find(|r| r.props.contains_key(keys::PICTURE))
        .expect("a picture's run");
    assert_eq!(run.len, 1);
    let Some(Value::Text(id)) = run.props.get(keys::PICTURE).cloned() else {
        panic!("a picture's ID");
    };
    assert_eq!(id.len(), 64);
    assert_eq!(
        run.props.get(keys::PICTURE_WIDTH),
        Some(&Value::Int(32 * PIXEL))
    );
    assert_eq!(
        run.props.get(keys::PICTURE_HEIGHT),
        Some(&Value::Int(24 * PIXEL))
    );
    let shown = vec![(32 * PIXEL, 24 * PIXEL, Some("gradient".to_string()))];
    assert_eq!(drawn(&editor), shown);

    // Undone, it goes; redone, it's back, and drawn.
    command(&mut editor, "undo");
    command(&mut editor, "undo");
    assert_eq!(text_of(&editor), ["A gradient: "]);
    assert!(drawn(&editor).is_empty());
    command(&mut editor, "redo");
    assert_eq!(text_of(&editor), [format!("A gradient: {OBJECT}")]);
    assert_eq!(drawn(&editor), shown);
    command(&mut editor, "redo");

    // Kept in the file, once; reopened, it's drawn from there.
    let path = editor.path().to_path_buf();
    let dump = editor.document().dump();
    drop(editor);
    let file = store::dump(&path).expect("it dumps");
    assert!(
        file.contains(&format!(
            "images\n    {id} jpeg {} bytes\n",
            std::fs::metadata(&jpeg).expect("the JPEG").len()
        )),
        "{file}"
    );
    let mut editor = Editor::open(res.clone(), &path).expect("it reopens");
    assert_eq!(editor.document().dump(), dump);
    assert_eq!(drawn(&editor), shown);

    // Undone and closed, the document doesn't show it; reopened and
    // redone, it's back from the file.
    command(&mut editor, "undo");
    command(&mut editor, "undo");
    assert!(drawn(&editor).is_empty());
    drop(editor);
    let mut editor = Editor::open(res, &path).expect("it reopens");
    assert!(drawn(&editor).is_empty());
    command(&mut editor, "redo");
    assert_eq!(drawn(&editor), shown);
}

#[test]
fn an_srgb_jpeg_prints_as_the_file_it_is_and_a_figure_is_tagged() {
    let res = resources("pdf");
    let mut editor = Editor::create(res).expect("a document");
    typed(&mut editor, "Below, a figure.");
    editor.update(Event::Key {
        key: chrome::Key::Enter,
        modifiers: chrome::Modifiers::default(),
    });
    let jpeg = fixture("gradient.jpg");
    place(&mut editor, &jpeg);
    let pages = editor.pages();
    assert!(pages.structure.blocks.contains(&Role::Figure));
    let effects = command(&mut editor, "export-pdf");
    let [Effect::Reveal(pdf)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let pdf = std::fs::read(pdf).expect("the PDF");
    let bytes = std::fs::read(&jpeg).expect("the JPEG");
    assert!(
        pdf.windows(bytes.len()).any(|w| w == bytes.as_slice()),
        "the JPEG isn't in the PDF unchanged"
    );
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let alts: Vec<String> = document
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok()?.get(b"Alt").ok().cloned())
        .filter_map(|alt| lopdf::decode_text_string(&alt).ok())
        .collect();
    assert_eq!(alts, ["gradient"]);
    // Its character is no text: the page reads as typed.
    let text = testkit::pdf::text(&pdf).expect("text reads");
    assert!(!text.contains(OBJECT), "{text:?}");
    assert!(text.contains("Below, a figure."));
}

#[test]
fn what_cant_be_placed_says_why_and_changes_nothing() {
    let res = resources("refused");
    let dir = res.documents.clone();
    let mut editor = Editor::create(res.clone()).expect("a document");
    typed(&mut editor, "Text.");
    let before = editor.document().dump();
    // Not a picture.
    let notes = dir.join("notes.txt");
    std::fs::write(&notes, "Only words.").expect("written");
    place(&mut editor, &notes);
    assert_eq!(editor.document().dump(), before);
    assert!(
        editor.message().starts_with("Couldn't place notes.txt"),
        "{}",
        editor.message()
    );
    // Not there.
    place(&mut editor, &dir.join("gone.png"));
    assert_eq!(editor.document().dump(), before);
    // None chosen.
    editor.update(Event::Chosen(Choice::Picture, None));
    assert_eq!(editor.document().dump(), before);
    // Open read-only, as a second window would have it, there's nowhere
    // to place one.
    let path = editor.path().to_path_buf();
    let mut second = Editor::open(res, &path).expect("it opens");
    assert!(!second.state("insert-picture").enabled);
    second.update(Event::Chosen(
        Choice::Picture,
        Some(fixture("gradient.jpg")),
    ));
    assert_eq!(second.document().dump(), before);
    assert!(second.message().starts_with("Read-only"));
}

#[test]
fn text_typed_after_a_picture_is_text() {
    let res = resources("typed-after");
    let mut editor = Editor::create(res).expect("a document");
    place(&mut editor, &fixture("gradient.jpg"));
    typed(&mut editor, "Caption");
    let document = editor.document();
    let p = document
        .paragraph(document.paragraphs()[0])
        .expect("a paragraph");
    assert_eq!(p.text, format!("{OBJECT}Caption"));
    assert_eq!(p.runs.len(), 2);
    assert!(!p.runs[1].props.contains_key(keys::PICTURE));
    // Copied, it's the text alone; the object character never types.
    command(&mut editor, "select-all");
    let effects = command(&mut editor, "copy");
    assert_eq!(effects, [Effect::Copy("Caption".to_string())]);
    command(&mut editor, "select-all");
    editor.update(Event::Pasted(format!("a{OBJECT}b")));
    assert_eq!(text_of(&editor), ["ab"]);
}
