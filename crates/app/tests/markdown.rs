//! Markdown files as themselves in the app (docs/ROADMAP.md, M2 stage 6):
//! New Markdown Document, a file opened and written back as it's edited,
//! read-only where another program has it or it isn't UTF-8, its named
//! versions while it's open, its pictures linked where they are, and what
//! it becomes when it's made an `.xw` file.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use app::{Editor, Effect, Event, Resources, Window};
use chrome::{Arg, Key, Modifiers};
use doc::Value;
use scene::Item;
use story::{OBJECT, keys};

fn resources(name: &str) -> Arc<Resources> {
    static RES: OnceLock<Resources> = OnceLock::new();
    let res = RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    });
    let mut res = res.clone();
    res.documents = testkit::scratch_dir(&format!("app-markdown-{name}")).expect("a folder");
    Arc::new(res)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pictures")
        .join(name)
}

fn file(res: &Resources, name: &str, text: &[u8]) -> PathBuf {
    let path = res.documents.join(name);
    std::fs::write(&path, text).expect("it writes");
    path
}

fn read(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).expect("it reads")).expect("UTF-8")
}

fn key(editor: &mut Editor, key: Key) {
    editor.update(Event::Key {
        key,
        modifiers: Modifiers::default(),
    });
}

fn command(editor: &mut Editor, id: &str) -> Vec<Effect> {
    editor.update(Event::Command(id.to_string()))
}

/// The pictures the pages draw.
fn drawn(editor: &Editor) -> usize {
    editor
        .pages()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| matches!(item, Item::Image { .. }))
        .count()
}

#[test]
fn new_markdown_documents_are_made_open_as_drafts_and_take_their_names() {
    let res = resources("new");
    let mut editor = Editor::create(res.clone()).expect("a document");
    let effects = command(&mut editor, "new-markdown");
    let path = res.documents.join("Untitled.md");
    assert_eq!(effects, [Effect::Open(Some(path.clone()))]);
    assert_eq!(read(&path), "");
    let mut window = Window::open(res.clone(), &path).expect("it opens");
    assert!(window.editor().is_markdown());
    assert_eq!(window.editor().view_mode(), app::View::Draft);
    window.update(Event::Text("Garden plan".to_string()));
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    window.update(Event::Text("Beans first.".to_string()));
    // Named after its first line, as a new document is, once the caret
    // leaves it.
    let named = res.documents.join("Garden plan.md");
    assert_eq!(window.editor().path(), named);
    assert!(!path.exists());
    assert_eq!(read(&named), "Garden plan\n\nBeans first.\n");
}

#[test]
fn a_markdown_file_another_program_has_or_that_isnt_utf8_opens_read_only() {
    let res = resources("read-only");
    let path = file(&res, "Held.md", b"# Held\n");
    let held = store::acquire(&path);
    assert!(matches!(held, store::Acquired::Held(_)));
    let mut editor = Editor::open(res.clone(), &path).expect("it opens");
    assert!(
        editor.message().starts_with("Read-only: open in"),
        "{}",
        editor.message()
    );
    editor.update(Event::Text("more".to_string()));
    assert_eq!(read(&path), "# Held\n");
    drop(held);

    let path = file(&res, "Latin.md", b"Caf\xe9\n");
    let mut editor = Editor::open(res, &path).expect("it opens");
    assert_eq!(editor.message(), "Read-only: it isn't UTF-8 text");
    editor.update(Event::Text("more".to_string()));
    assert_eq!(std::fs::read(&path).expect("it reads"), b"Caf\xe9\n");
}

#[test]
fn a_markdown_file_is_locked_while_its_open_and_written_whole() {
    let res = resources("locked");
    let path = file(&res, "Open.md", b"\xef\xbb\xbfOne\r\n\r\nTwo\r\n");
    let mut editor = Editor::open(res.clone(), &path).expect("it opens");
    assert!(matches!(store::acquire(&path), store::Acquired::InUse(_)));
    editor.update(Event::Key {
        key: Key::End,
        modifiers: Modifiers {
            primary: true,
            ..Modifiers::default()
        },
    });
    editor.update(Event::Text(" and three".to_string()));
    // Its byte order mark and line endings as they were.
    assert_eq!(
        std::fs::read(&path).expect("it reads"),
        b"\xef\xbb\xbfOne\r\n\r\nTwo and three\r\n"
    );
    // Nothing left beside it but its lock.
    let mut names: Vec<String> = std::fs::read_dir(&res.documents)
        .expect("the folder")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, [".Open.md-lock", "Open.md"]);
    drop(editor);
    assert!(matches!(store::acquire(&path), store::Acquired::Held(_)));
}

/// A Markdown file's thematic break is a rule (review jy4a2tx5): drawn in
/// the draft, a hairline across the column, and printed, where it's drawn
/// and not text.
#[test]
fn a_rule_is_drawn_and_printed_but_isnt_text() {
    let res = resources("rule");
    let path = file(&res, "Rule.md", b"Above\n\n---\n\nBelow\n");
    let editor = Editor::open(res.clone(), &path).expect("it opens");
    let look = &editor.sheet().blocks;
    let rule = scene::Color::rgb(look.rule_color.0, look.rule_color.1, look.rule_color.2);
    let pages = editor.pages();
    let fills = pages.pages[0]
        .items
        .iter()
        .filter(|item| matches!(item, Item::Fill { color, .. } if *color == rule))
        .count();
    assert_eq!(fills, 1, "a hairline across the column");
    let pdf = render_pdf::pdf(&pages.pages, &pages.structure, None).expect("a PDF");
    let text = testkit::pdf::text(&pdf).expect("its text");
    assert!(text.contains("Above") && text.contains("Below"), "{text:?}");
    assert!(!text.contains('-'), "{text:?}");
    // The draft draws it too, in the look's ink.
    let drawn = editor.view().items.len();
    let mut without =
        Editor::open(res.clone(), &file(&res, "Plain.md", b"Above\n\nBelow\n")).expect("it opens");
    without.update(Event::Resize {
        width: 800.0,
        height: 600.0,
    });
    let mut with = editor;
    with.update(Event::Resize {
        width: 800.0,
        height: 600.0,
    });
    assert!(drawn > 0);
    assert_eq!(with.view().items.len(), without.view().items.len() + 1);
}

#[test]
fn versions_of_a_markdown_file_last_while_its_open() {
    let res = resources("versions");
    let path = file(&res, "Draft.md", b"First thoughts.\n");
    let mut window = Window::open(res.clone(), &path).expect("it opens");
    window.update(Event::Command("save-version".to_string()));
    assert_eq!(
        window.editor().message(),
        "Saved Version 1, kept while it's open"
    );
    window.update(Event::Text("Second ".to_string()));
    assert_eq!(read(&path), "Second First thoughts.\n");
    // The version opens in a window of its own, as it was, read-only.
    let version = Window::open_version(res.clone(), &path, 0).expect("the version opens");
    let p = version.editor().document().paragraphs()[0];
    assert_eq!(
        version
            .editor()
            .document()
            .paragraph(p)
            .expect("a paragraph")
            .text,
        "First thoughts."
    );
    assert!(!version.editor().state("bold").enabled);
    // Closed, they go.
    window.close();
    drop(window);
    assert!(Window::open_version(res.clone(), &path, 0).is_err());
}

#[test]
fn pictures_a_markdown_file_links_to_are_drawn_from_where_they_point() {
    let res = resources("linked");
    std::fs::create_dir_all(res.documents.join("images")).expect("a folder");
    std::fs::copy(
        fixture("gradient.jpg"),
        res.documents.join("images/gradient.jpg"),
    )
    .expect("it copies");
    let path = file(
        &res,
        "Pictures.md",
        b"![A gradient](images/gradient.jpg)\n\n![Gone](images/gone.png)\n",
    );
    let editor = Editor::open(res, &path).expect("it opens");
    assert_eq!(drawn(&editor), 1);
    assert_eq!(editor.message(), "A picture couldn't be shown");
    // Placed at its own size, as a picture placed in an .xw file is.
    let document = editor.document();
    let p = document
        .paragraph(document.paragraphs()[0])
        .expect("a paragraph");
    assert_eq!(p.text, OBJECT.to_string());
    assert_eq!(
        p.runs[0].props.get(keys::PICTURE_WIDTH),
        Some(&Value::Int(32 * text::units::PIXEL))
    );
}

#[test]
fn a_markdown_file_made_an_xw_file_keeps_its_history_and_pictures() {
    let res = resources("convert");
    std::fs::copy(fixture("gradient.jpg"), res.documents.join("gradient.jpg")).expect("it copies");
    let path = file(
        &res,
        "Report.md",
        b"# Report\n\n![gradient](gradient.jpg)\n",
    );
    let mut window = Window::open(res.clone(), &path).expect("it opens");
    window.update(Event::Key {
        key: Key::End,
        modifiers: Modifiers::default(),
    });
    window.update(Event::Text(" in brief".to_string()));
    window.update(Event::Command("save-version".to_string()));
    window.update(Event::Key {
        key: Key::Home,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    });
    window.update(Event::CommandWith(
        "font".to_string(),
        vec![Arg::Text("Inter".to_string())],
    ));
    assert!(
        window
            .editor()
            .message()
            .starts_with("Markdown can't hold a font")
    );
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    let xw = res.documents.join("Report.xw");
    assert_eq!(window.editor().path(), xw);
    assert!(!window.editor().is_markdown());
    assert!(
        window.editor().message().starts_with("Now Report.xw"),
        "{}",
        window.editor().message()
    );
    assert_eq!(
        read(&path),
        "# Report in brief\n\n![gradient](gradient.jpg)\n"
    );
    assert_eq!(drawn(window.editor()), 1);
    window.close();
    drop(window);
    // Reopened, it has its history, its version and its picture, kept.
    let opened = store::Store::open(&xw).expect("it opens");
    let names: Vec<String> = opened
        .store
        .versions()
        .expect("versions")
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(names, ["Version 1"]);
    assert_eq!(opened.store.image_ids().expect("images").len(), 1);
    assert_eq!(opened.history.undo_name(), Some("Font"));
    drop(opened);
    let mut editor = Editor::open(res, &xw).expect("it opens");
    assert_eq!(drawn(&editor), 1);
    command(&mut editor, "undo");
    command(&mut editor, "undo");
    let p = editor.document().paragraphs()[0];
    assert_eq!(
        editor.document().paragraph(p).expect("a heading").text,
        "Report"
    );
    key(&mut editor, Key::Down);
}

/// A task's box, clicked, ticks or unticks it, and the file changes in
/// the box alone.
#[test]
fn a_tasks_box_clicked_ticks_it() {
    let res = resources("click");
    let path = file(
        &res,
        "Tasks.md",
        b"- [ ] Sow the beans\n- [x] Dig the bed\n",
    );
    let mut editor = Editor::open(res, &path).expect("it opens");
    editor.update(Event::Resize {
        width: 900.0,
        height: 700.0,
    });
    // The boxes, as drawn: outlines of four sides.
    let boxes: Vec<scene::Rect> = editor
        .view()
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Stroke { path, .. } if path.0.len() == 5 => {
                let points: Vec<scene::Point> = path
                    .0
                    .iter()
                    .filter_map(|s| match s {
                        scene::Segment::MoveTo(p) | scene::Segment::LineTo(p) => Some(*p),
                        _ => None,
                    })
                    .collect();
                Some(scene::Rect::new(
                    points[0].x,
                    points[0].y,
                    points[2].x - points[0].x,
                    points[2].y - points[0].y,
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2, "two boxes");
    let px = |v: i64| v as f64 / text::units::PIXEL as f64;
    let click = |editor: &mut Editor, r: &scene::Rect| {
        editor.update(Event::Press {
            x: px(r.x + r.width / 2),
            y: px(r.y + r.height / 2),
            shift: false,
            count: 1,
        });
        editor.update(Event::Release);
    };
    click(&mut editor, &boxes[0]);
    assert_eq!(read(&path), "- [x] Sow the beans\n- [x] Dig the bed\n");
    click(&mut editor, &boxes[1]);
    assert_eq!(read(&path), "- [x] Sow the beans\n- [ ] Dig the bed\n");
    // Undone, as any edit.
    command(&mut editor, "undo");
    assert_eq!(read(&path), "- [x] Sow the beans\n- [x] Dig the bed\n");
}
