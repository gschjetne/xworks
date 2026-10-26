//! A document's own stylesheet, edited (M2 stage 9; docs/ARCHITECTURE.md,
//! section 6): direct formatting cleared, styles redefined and made from
//! the selection, a style's values, base and name and the page set from
//! the inspector, the stylesheet chosen for a new document, replaced,
//! kept as one's own and updated when offered, styles pasted between
//! documents, and Markdown files kept in Neutral.

use std::sync::{Arc, OnceLock};

use app::{Editor, Effect, Event, Resources};
use chrome::{Arg, Key, Modifiers};
use doc::{ObjectId, Value};
use story::keys;
use text::units::{MILLIMETRE, POINT};

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

/// Resources with documents in a scratch folder of their own, `name`, and
/// the person's own stylesheets in one beside it.
fn res(name: &str) -> Arc<Resources> {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir(name).expect("a folder");
    res.own = Arc::new(app::OwnSheets::load(res.documents.join("stylesheets")));
    Arc::new(res)
}

fn command(editor: &mut Editor, id: &str) -> Vec<Effect> {
    editor.update(Event::Command(id.to_string()))
}

fn with(editor: &mut Editor, id: &str, args: Vec<Arg>) -> Vec<Effect> {
    editor.update(Event::CommandWith(id.to_string(), args))
}

fn key(editor: &mut Editor, key: Key, primary: bool, shift: bool) {
    editor.update(Event::Key {
        key,
        modifiers: Modifiers {
            primary,
            shift,
            ..Modifiers::default()
        },
    });
}

fn select_all(editor: &mut Editor) {
    command(editor, "select-all");
}

/// The first paragraph's own property `key`.
fn own(editor: &Editor, k: usize, key: &str) -> Option<Value> {
    let document = editor.document();
    let id = document.paragraphs()[k];
    document.paragraph(id).unwrap().props.get(key).cloned()
}

/// Whether any run of paragraph `k` sets `key` itself.
fn runs_set(editor: &Editor, k: usize, key: &str) -> bool {
    let document = editor.document();
    let id = document.paragraphs()[k];
    document
        .paragraph(id)
        .unwrap()
        .runs
        .iter()
        .any(|r| r.props.contains_key(key))
}

/// The ID of the editor's paragraph style named `name`.
fn style(editor: &Editor, name: &str) -> ObjectId {
    editor
        .sheet()
        .paragraph
        .iter()
        .find(|s| s.name == name)
        .and_then(|s| s.id)
        .unwrap_or_else(|| panic!("no style {name}"))
}

fn id_arg(id: ObjectId) -> Arg {
    Arg::Int(id.0 as i64)
}

#[test]
fn clear_formatting_takes_away_the_texts_own_formatting_but_not_its_marks() {
    let mut editor = Editor::create(res("app-styles-clear")).expect("a document");
    editor.update(Event::Text("Formatted".to_string()));
    select_all(&mut editor);
    with(&mut editor, "size", vec![Arg::Int(16 * POINT)]);
    with(&mut editor, "space-before", vec![Arg::Int(12 * POINT)]);
    command(&mut editor, "bold");
    assert!(runs_set(&editor, 0, keys::SIZE));
    assert!(editor.state("clear-formatting").enabled);
    command(&mut editor, "clear-formatting");
    assert!(!runs_set(&editor, 0, keys::SIZE));
    assert_eq!(own(&editor, 0, keys::SPACE_BEFORE), None);
    assert!(
        runs_set(&editor, 0, keys::BOLD),
        "strong is a character style"
    );
    assert!(!editor.state("clear-formatting").enabled);
    command(&mut editor, "undo");
    assert!(runs_set(&editor, 0, keys::SIZE));
}

#[test]
fn applying_a_style_replaces_paragraph_formatting_and_keeps_the_texts() {
    let mut editor = Editor::create(res("app-styles-apply")).expect("a document");
    editor.update(Event::Text("Styled".to_string()));
    select_all(&mut editor);
    with(&mut editor, "size", vec![Arg::Int(16 * POINT)]);
    with(&mut editor, "indent-start", vec![Arg::Int(10 * MILLIMETRE)]);
    command(&mut editor, "style-heading-2");
    assert_eq!(own(&editor, 0, keys::INDENT_START), None);
    assert!(
        runs_set(&editor, 0, keys::SIZE),
        "its text's own size stays"
    );
    // With "clear", the text's own formatting goes too.
    let body = style(&editor, "Body");
    with(
        &mut editor,
        "paragraph-style",
        vec![id_arg(body), Arg::Text("clear".to_string())],
    );
    assert!(!runs_set(&editor, 0, keys::SIZE));
    assert_eq!(
        editor
            .sheet()
            .style_of(
                editor
                    .document()
                    .paragraph(editor.document().paragraphs()[0])
                    .unwrap()
            )
            .name,
        "Body"
    );
}

#[test]
fn a_style_is_redefined_and_made_from_the_selection_and_renamed_in_one_edit() {
    let mut editor = Editor::create(res("app-styles-redefine")).expect("a document");
    editor.update(Event::Text("One".to_string()));
    key(&mut editor, Key::Enter, false, false);
    editor.update(Event::Text("Two".to_string()));
    with(&mut editor, "space-before", vec![Arg::Int(12 * POINT)]);
    command(&mut editor, "redefine-style");
    let body = editor.sheet().default_style().clone();
    assert_eq!(
        body.space_before,
        12 * POINT,
        "Body takes the selection's space"
    );
    assert_eq!(
        own(&editor, 1, keys::SPACE_BEFORE),
        None,
        "which the paragraph no longer sets"
    );
    let laid = editor.pages();
    assert!(!laid.pages.is_empty());
    // A new style from the selection, based on Body.
    with(&mut editor, "indent-first", vec![Arg::Int(5 * MILLIMETRE)]);
    command(&mut editor, "new-style");
    let new = style(&editor, "New Style");
    let made = editor.sheet().paragraph_style(Some(new)).clone();
    assert_eq!(made.base, body.id);
    assert_eq!(made.indents.first, 5 * MILLIMETRE);
    assert_eq!(own(&editor, 1, keys::STYLE), Some(Value::Ref(new)));
    assert_eq!(own(&editor, 1, keys::INDENT_FIRST), None);
    // Renamed: the paragraph still names it by ID.
    let before = editor
        .document()
        .paragraph(editor.document().paragraphs()[1])
        .unwrap()
        .clone();
    with(
        &mut editor,
        "style-rename",
        vec![id_arg(new), Arg::Text("Epigraph".to_string())],
    );
    assert_eq!(editor.sheet().paragraph_style(Some(new)).name, "Epigraph");
    assert_eq!(
        editor
            .document()
            .paragraph(editor.document().paragraphs()[1])
            .unwrap(),
        &before
    );
    // A name another style has is refused.
    with(
        &mut editor,
        "style-rename",
        vec![id_arg(new), Arg::Text("Body".to_string())],
    );
    assert_eq!(editor.sheet().paragraph_style(Some(new)).name, "Epigraph");
}

#[test]
fn a_style_edited_in_the_inspector_restyles_every_paragraph_and_undoes() {
    let mut editor = Editor::create(res("app-styles-edit")).expect("a document");
    editor.update(Event::Text("Body text".to_string()));
    let body = style(&editor, "Body");
    let size = |editor: &Editor| editor.sheet().default_style().size;
    with(
        &mut editor,
        "style-set",
        vec![
            id_arg(body),
            Arg::Text(keys::SIZE.to_string()),
            Arg::Int(14 * POINT),
        ],
    );
    assert_eq!(size(&editor), 14 * POINT);
    assert!(editor.status().contains("14 pt"), "{}", editor.status());
    with(
        &mut editor,
        "style-reset",
        vec![id_arg(body), Arg::Text(keys::SIZE.to_string())],
    );
    assert_eq!(size(&editor), 11 * POINT, "from the defaults again");
    command(&mut editor, "undo");
    assert_eq!(size(&editor), 14 * POINT);
    command(&mut editor, "undo");
    assert_eq!(size(&editor), 11 * POINT);
    // A face, by its part: every style that names it follows.
    let heading = editor
        .sheet()
        .parts
        .iter()
        .find(|p| p.name == "Heading" && p.kind == doc::StyleKind::Face)
        .unwrap()
        .id;
    with(
        &mut editor,
        "style-set",
        vec![
            id_arg(body),
            Arg::Text(keys::FONT.to_string()),
            id_arg(heading),
        ],
    );
    assert_eq!(editor.sheet().default_style().family, "Inter");
    // A base that would loop is refused.
    let h1 = style(&editor, "Heading 1");
    let base = style(&editor, "Heading");
    with(&mut editor, "style-base", vec![id_arg(base), id_arg(h1)]);
    assert_eq!(editor.sheet().paragraph_style(Some(base)).base, None);
    assert!(
        editor.message().contains("based on itself"),
        "{}",
        editor.message()
    );
}

#[test]
fn the_page_is_the_documents_own_and_kept() {
    let mut editor = Editor::create(res("app-styles-page")).expect("a document");
    editor.update(Event::Text("Paged".to_string()));
    with(&mut editor, "page-size", vec![Arg::Text("A5".to_string())]);
    with(
        &mut editor,
        "page-margin",
        vec![Arg::Text("left".to_string()), Arg::Int(30 * MILLIMETRE)],
    );
    let page = editor.sheet().page;
    assert_eq!(
        (page.width, page.height, page.left),
        (148 * MILLIMETRE, 210 * MILLIMETRE, 30 * MILLIMETRE)
    );
    assert_eq!(editor.pages().pages[0].width, 148 * MILLIMETRE);
    // Margins that leave no room are refused.
    with(
        &mut editor,
        "page-margin",
        vec![Arg::Text("right".to_string()), Arg::Int(120 * MILLIMETRE)],
    );
    assert_eq!(editor.sheet().page.right, 25 * MILLIMETRE);
    let path = editor.path().to_path_buf();
    drop(editor);
    let reopened = Editor::open(resources().clone().into(), &path).expect("it opens");
    assert_eq!(reopened.sheet().page.width, 148 * MILLIMETRE);
}

/// The version of stylesheet `name` that comes with xWorks.
fn version(res: &Resources, name: &str) -> u32 {
    res.stylesheets
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .version
}

/// The version of Neutral that comes with xWorks.
fn neutral_version(res: &Resources) -> u32 {
    version(res, "Neutral")
}

#[test]
fn a_new_document_copies_the_stylesheet_chosen_and_can_take_another() {
    let res = res("app-styles-new");
    let mut editor = Editor::create(res.clone()).expect("a document");
    assert_eq!(
        editor.sheet().origin,
        Some(("Neutral".to_string(), neutral_version(&res)))
    );
    let effects = with(
        &mut editor,
        "new-with-stylesheet",
        vec![Arg::Text("app:Book".to_string())],
    );
    let Some(Effect::Open(Some(path))) = effects.first() else {
        panic!("a new document to open: {effects:?}");
    };
    let mut book = Editor::open(res.clone(), path).expect("it opens");
    assert_eq!(
        book.sheet().origin,
        Some(("Book".to_string(), version(&res, "Book")))
    );
    assert_eq!(book.sheet().default_style().family, "EB Garamond");
    // Replaced by Modern: matched by role, so Body is still Body, and a
    // style of the document's own stays.
    book.update(Event::Text("Kept".to_string()));
    command(&mut book, "new-style");
    with(
        &mut book,
        "replace-stylesheet",
        vec![Arg::Text("app:Modern".to_string())],
    );
    assert_eq!(book.sheet().default_style().family, "Inter");
    assert_eq!(
        book.sheet().origin,
        Some(("Modern".to_string(), version(&res, "Modern")))
    );
    assert!(book.sheet().paragraph.iter().any(|s| s.name == "New Style"));
    command(&mut book, "undo");
    assert_eq!(book.sheet().default_style().family, "EB Garamond");
}

#[test]
fn a_stylesheet_kept_as_ones_own_is_offered_for_new_documents() {
    let res = res("app-styles-own");
    let mut editor = Editor::create(res.clone()).expect("a document");
    let body = style(&editor, "Body");
    with(
        &mut editor,
        "style-set",
        vec![
            id_arg(body),
            Arg::Text(keys::SIZE.to_string()),
            Arg::Int(13 * POINT),
        ],
    );
    assert!(editor.state("save-stylesheet").enabled);
    command(&mut editor, "save-stylesheet");
    assert!(
        editor.message().contains("My Neutral"),
        "{}",
        editor.message()
    );
    let (own, bad) = res.own.list();
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].spec.name, "My Neutral");
    assert_eq!(editor.sheet().origin, Some(("My Neutral".to_string(), 1)));
    let effects = with(
        &mut editor,
        "new-with-stylesheet",
        vec![Arg::Text("own:My Neutral".to_string())],
    );
    let Some(Effect::Open(Some(path))) = effects.first() else {
        panic!("a new document to open: {effects:?}");
    };
    let made = Editor::open(res.clone(), path).expect("it opens");
    assert_eq!(made.sheet().default_style().size, 13 * POINT);
    // A file in the folder that isn't a stylesheet is left out, and said.
    std::fs::write(res.documents.join("stylesheets/Broken.ron"), "(name: ").unwrap();
    let (own, bad) = res.own.list();
    assert_eq!((own.len(), bad.len()), (1, 1));
}

#[test]
fn a_newer_version_of_the_stylesheet_is_offered_never_applied() {
    let res = res("app-styles-update");
    let mut editor = Editor::create(res.clone()).expect("a document");
    editor.update(Event::Text("Offered".to_string()));
    let path = editor.path().to_path_buf();
    drop(editor);
    // A newer Neutral, whose body is a point larger.
    let mut newer = (*res).clone();
    let mut neutral = newer.stylesheets[0].clone();
    neutral.version += 1;
    neutral.paragraph[0].size = Some(text::units::Length::Pt(12.0));
    let mut sheets = (*newer.stylesheets).clone();
    sheets[0] = neutral;
    newer.stylesheets = Arc::new(sheets);
    let mut editor = Editor::open(Arc::new(newer), &path).expect("it opens");
    assert!(
        editor.message().contains("Neutral has a newer version"),
        "{}",
        editor.message()
    );
    assert_eq!(
        editor.sheet().default_style().size,
        11 * POINT,
        "nothing changes by itself"
    );
    assert!(editor.state("update-stylesheet").enabled);
    command(&mut editor, "update-stylesheet");
    let (title, rows, _) = editor.panel().expect("what changed");
    assert!(title.contains("Neutral has changed"), "{title}");
    assert!(
        rows.iter()
            .any(|r| r.contains("Body") && r.contains("size 12 pt")),
        "{rows:?}"
    );
    key(&mut editor, Key::Enter, false, false);
    assert_eq!(editor.sheet().default_style().size, 12 * POINT);
    assert_eq!(
        editor.sheet().origin,
        Some(("Neutral".to_string(), neutral_version(&res) + 1))
    );
    assert!(!editor.state("update-stylesheet").enabled);
}

#[test]
fn pasting_from_another_document_brings_its_styles_matched() {
    let res = res("app-styles-paste");
    let mut from = Editor::create(res.clone()).expect("a document");
    from.update(Event::Text("Captioned".to_string()));
    command(&mut from, "new-style");
    let caption = style(&from, "New Style");
    with(
        &mut from,
        "style-rename",
        vec![id_arg(caption), Arg::Text("Caption".to_string())],
    );
    with(
        &mut from,
        "style-set",
        vec![
            id_arg(caption),
            Arg::Text(keys::ITALIC.to_string()),
            Arg::Int(1),
        ],
    );
    select_all(&mut from);
    let effects = command(&mut from, "copy");
    let Some(Effect::Copy(text)) = effects.first() else {
        panic!("copied: {effects:?}");
    };
    let mut to = Editor::create(res.clone()).expect("another document");
    to.update(Event::Text("Before. ".to_string()));
    to.update(Event::Pasted(text.clone()));
    let caption = style(&to, "Caption");
    let made = to.sheet().paragraph_style(Some(caption)).clone();
    assert!(made.italic);
    assert_eq!(
        made.base,
        Some(style(&to, "Body")),
        "based on the destination's Body"
    );
    assert_eq!(
        to.sheet()
            .paragraph
            .iter()
            .filter(|s| s.name == "Body")
            .count(),
        1
    );
    // Text pasted from elsewhere is text.
    to.update(Event::Pasted("Plain".to_string()));
    assert_eq!(
        to.sheet()
            .paragraph
            .iter()
            .filter(|s| s.name == "Caption")
            .count(),
        1
    );
}

/// Insert, Table makes a table whose header row's cells are in Table
/// Header, as headings are in theirs (review sjj5ak5a), and whose other
/// cells name no style; a copied table's cells paste as paragraphs, those
/// of its header in the default style, as Table Header is a header row's.
#[test]
fn a_tables_header_row_is_in_table_header() {
    let res = res("app-styles-table");
    let mut editor = Editor::create(res.clone()).expect("a document");
    editor.update(Event::Text("Before".to_string()));
    command(&mut editor, "insert-table");
    for (k, text) in ["Name", "Size", "Pear"].into_iter().enumerate() {
        if k > 0 {
            key(&mut editor, Key::Tab, false, false);
        }
        editor.update(Event::Text(text.to_string()));
    }
    let header = style(&editor, "Table Header");
    assert_eq!(own(&editor, 1, keys::STYLE), Some(Value::Ref(header)));
    assert_eq!(own(&editor, 2, keys::STYLE), Some(Value::Ref(header)));
    assert_eq!(own(&editor, 3, keys::STYLE), None);
    select_all(&mut editor);
    let effects = command(&mut editor, "copy");
    let Some(Effect::Copy(text)) = effects.first() else {
        panic!("copied: {effects:?}");
    };
    let mut to = Editor::create(res.clone()).expect("another document");
    to.update(Event::Text("Here: ".to_string()));
    to.update(Event::Pasted(text.clone()));
    let document = to.document();
    let names: Vec<(String, String)> = document
        .paragraphs()
        .iter()
        .map(|&id| {
            let p = document.paragraph(id).unwrap();
            (to.sheet().style_of(p).name.clone(), p.text.clone())
        })
        .collect();
    assert!(names.iter().any(|(_, text)| text == "Name"), "{names:?}");
    assert!(names.iter().all(|(style, _)| style == "Body"), "{names:?}");
}

#[test]
fn a_markdown_file_keeps_neutral_and_asks_before_a_style_changes() {
    let res = res("app-styles-markdown");
    let path = res.documents.join("Notes.md");
    std::fs::write(&path, "# Notes\n\nA line.\n").unwrap();
    let mut editor = Editor::open(res.clone(), &path).expect("it opens");
    assert_eq!(
        editor.sheet().origin,
        Some(("Neutral".to_string(), neutral_version(&res)))
    );
    let body = style(&editor, "Body");
    with(
        &mut editor,
        "style-set",
        vec![
            id_arg(body),
            Arg::Text(keys::SIZE.to_string()),
            Arg::Int(14 * POINT),
        ],
    );
    assert!(editor.panel().is_some(), "it asks first");
    assert_eq!(
        editor.sheet().default_style().size,
        11 * POINT,
        "and doesn't change"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# Notes\n\nA line.\n"
    );
}

#[test]
fn direct_formatting_is_tinted_only_when_asked() {
    let mut editor = Editor::create(res("app-styles-tint")).expect("a document");
    editor.update(Event::Text("Tinted".to_string()));
    select_all(&mut editor);
    with(&mut editor, "size", vec![Arg::Int(16 * POINT)]);
    let tint = resources().light.formatting;
    let tinted = |editor: &Editor| {
        editor
            .view()
            .items
            .iter()
            .any(|item| matches!(item, scene::Item::Fill { color, .. } if *color == tint))
    };
    assert!(!editor.state("show-formatting").checked);
    assert!(!tinted(&editor));
    command(&mut editor, "show-formatting");
    assert!(editor.state("show-formatting").checked);
    assert!(tinted(&editor));
}
