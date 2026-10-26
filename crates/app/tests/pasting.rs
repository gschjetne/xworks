//! Copying and pasting that keep paragraphs' styles and the blocks they
//! sit in (docs/ARCHITECTURE.md, section 6, "What stage 9's follow-up
//! built: pasting"; review sjj5ak5a): a heading pasted is a heading, a
//! quote's paragraphs come in a quote, list items in a list, a table as a
//! table and a rule as a rule; a paragraph copied in part is text; the
//! destination's styles win; pictures come along where they can be had;
//! and Undo takes each paste back in one step.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use app::session::Start;
use app::{Choice, Editor, Effect, Event, Resources};
use chrome::{Key, Modifiers};
use doc::{Document, ObjectId, Props, Value};
use story::{BlockSpec, keys};

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

/// Resources with documents in a scratch folder of their own, `name`.
fn res(name: &str) -> Arc<Resources> {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir(&format!("app-pasting-{name}")).expect("a folder");
    res.locks = Some(testkit::scratch_dir(&format!("app-pasting-{name}-locks")).expect("a folder"));
    Arc::new(res)
}

/// The stylesheet that comes with xWorks named `name`: "Neutral", "Book".
fn sheet(res: &Resources, name: &str) -> story::SheetSpec {
    res.stylesheets
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no stylesheet {name}"))
        .clone()
}

fn p(style: &str, text: &str) -> BlockSpec {
    BlockSpec::Paragraph(style.into(), vec![(text.into(), Props::new())])
}

fn body(text: &str) -> BlockSpec {
    p("Body", text)
}

/// The paragraph of `document` whose text is `text`.
fn paragraph(document: &Document, text: &str) -> ObjectId {
    document
        .paragraphs()
        .into_iter()
        .find(|&id| document.paragraph(id).is_some_and(|p| p.text == text))
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

/// A window's editor on a new document of `blocks` in the stylesheet
/// `sheet`, with the selection from `from` to `to`: each a paragraph's
/// text and an offset in it. No blocks is one empty paragraph.
fn editor(
    res: &Arc<Resources>,
    sheet_name: &str,
    blocks: &[BlockSpec],
    from: (&str, u32),
    to: (&str, u32),
) -> Editor {
    let document = story::build(&sheet(res, sheet_name), blocks);
    let at = |(text, offset): (&str, u32)| (paragraph(&document, text).0, offset, offset > 0);
    let start = Start {
        document: document.to_snapshot(),
        undo: Vec::new(),
        redo: Vec::new(),
        anchor: at(from),
        focus: at(to),
        scroll: 0,
        goal: None,
        typing: false,
        pending: Props::new(),
        chrome: None,
    };
    Editor::resume(res.clone(), &start, (900.0, 700.0)).expect("a document")
}

/// An editor on a new document of `blocks`, in Neutral, the caret at
/// `at`.
fn at(res: &Arc<Resources>, blocks: &[BlockSpec], at: (&str, u32)) -> Editor {
    editor(res, "Neutral", blocks, at, at)
}

/// An editor on a new, empty document, in Neutral.
fn empty(res: &Arc<Resources>) -> Editor {
    at(res, &[], ("", 0))
}

fn command(editor: &mut Editor, id: &str) -> Vec<Effect> {
    editor.update(Event::Command(id.to_string()))
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

/// Copies the selection: the text put on the clipboard.
fn copy(editor: &mut Editor) -> String {
    match command(editor, "copy").as_slice() {
        [Effect::Copy(text)] => text.clone(),
        effects => panic!("copied: {effects:?}"),
    }
}

fn outline(editor: &Editor) -> Vec<String> {
    story::outline(editor.document(), editor.sheet())
}

/// Pastes `text` into `editor`, checks the outline is `want`, and that
/// Undo takes the paste back in one step, and Redo makes it again.
/// Returns where the caret was after the paste: its paragraph's text and
/// its offset there.
fn paste(editor: &mut Editor, text: &str, want: &[&str]) -> (String, u32) {
    let before = editor.document().clone();
    editor.update(Event::Pasted(text.to_string()));
    let after = editor.document().clone();
    let caret = caret(editor);
    assert_eq!(outline(editor), want, "pasted");
    command(editor, "undo");
    assert_eq!(editor.document(), &before, "undone in one step");
    command(editor, "redo");
    assert_eq!(editor.document(), &after, "redone");
    caret
}

/// The text and offset of the caret's place, with nothing selected.
fn caret(editor: &Editor) -> (String, u32) {
    let (anchor, focus) = editor.selection();
    assert_eq!(anchor, focus, "nothing selected");
    let p = editor
        .document()
        .paragraph(focus.paragraph)
        .expect("a paragraph");
    (p.text.clone(), focus.offset)
}

/// The owner's case (review sjj5ak5a): a heading, a quote and body text,
/// copied whole and pasted into another document, arrive as they were,
/// not as three paragraphs of body text.
#[test]
fn a_heading_a_quote_and_body_text_paste_as_they_were() {
    let res = res("owner");
    let blocks = [
        p("Heading 1", "Title"),
        BlockSpec::Quote(vec![body("Quoted")]),
        body("After"),
    ];
    let mut from = editor(&res, "Neutral", &blocks, ("Title", 0), ("After", 5));
    let text = copy(&mut from);
    assert_eq!(
        text, "Title\nQuoted\nAfter",
        "the clipboard's text is as it was"
    );
    let mut to = empty(&res);
    let caret = paste(
        &mut to,
        &text,
        &["Heading 1: Title", "quote", "  Body: Quoted", "Body: After"],
    );
    assert_eq!(caret, ("After".to_string(), 5));
}

/// A heading copied whole, by itself, is a heading wherever it's pasted:
/// on an empty line, which it replaces; at a paragraph's start, before
/// it; in its middle, between its halves; and at its end, after it.
#[test]
fn a_heading_copied_alone_pastes_as_a_heading() {
    let res = res("heading");
    let mut from = editor(
        &res,
        "Neutral",
        &[p("Heading 1", "Title"), body("After")],
        ("Title", 0),
        ("Title", 5),
    );
    let text = copy(&mut from);
    let mut to = empty(&res);
    let caret = paste(&mut to, &text, &["Heading 1: Title"]);
    assert_eq!(caret, ("Title".to_string(), 5));
    let line = [body("Text here")];
    let mut to = at(&res, &line, ("Text here", 0));
    let caret = paste(&mut to, &text, &["Heading 1: Title", "Body: Text here"]);
    assert_eq!(caret, ("Title".to_string(), 5));
    let mut to = at(&res, &line, ("Text here", 4));
    paste(
        &mut to,
        &text,
        &["Body: Text", "Heading 1: Title", "Body:  here"],
    );
    let mut to = at(&res, &line, ("Text here", 9));
    paste(&mut to, &text, &["Body: Text here", "Heading 1: Title"]);
    // In a list's item, it's in the item.
    let mut to = at(
        &res,
        &[BlockSpec::List(false, vec![vec![body("")]])],
        ("", 0),
    );
    paste(&mut to, &text, &["list", "  item", "    Heading 1: Title"]);
}

/// A paragraph copied in part is text: it goes in at the caret, and the
/// paragraph there keeps its style.
#[test]
fn a_heading_copied_in_part_is_text() {
    let res = res("part");
    let mut from = editor(
        &res,
        "Neutral",
        &[p("Heading 1", "Title")],
        ("Title", 1),
        ("Title", 4),
    );
    let text = copy(&mut from);
    assert_eq!(text, "itl");
    let mut to = at(&res, &[body("Text here")], ("Text here", 4));
    let caret = paste(&mut to, &text, &["Body: Textitl here"]);
    assert_eq!(caret, ("Textitl here".to_string(), 7));
    // From its start, not to its end, it's text too.
    let mut from = editor(
        &res,
        "Neutral",
        &[p("Heading 1", "Title")],
        ("Title", 0),
        ("Title", 3),
    );
    let text = copy(&mut from);
    let mut to = empty(&res);
    paste(&mut to, &text, &["Body: Tit"]);
}

/// The first and last paragraphs copied in part join the text they land
/// beside, in that paragraph's style; the whole ones between come as they
/// were.
#[test]
fn partial_first_and_last_paragraphs_join_the_text_beside_them() {
    let res = res("ends");
    let blocks = [
        body("alpha beta"),
        p("Heading 1", "Middle"),
        BlockSpec::Quote(vec![body("q1"), body("q2 tail")]),
    ];
    let mut from = editor(&res, "Neutral", &blocks, ("alpha beta", 6), ("q2 tail", 2));
    let text = copy(&mut from);
    assert_eq!(text, "beta\nMiddle\nq1\nq2");
    let mut to = at(&res, &[p("Heading 2", "Xx Yy")], ("Xx Yy", 3));
    let caret = paste(
        &mut to,
        &text,
        &[
            "Heading 2: Xx beta",
            "Heading 1: Middle",
            "quote",
            "  Body: q1",
            "Heading 2: q2Yy",
        ],
    );
    assert_eq!(caret, ("q2Yy".to_string(), 2));
    // Two paragraphs, each in part: the caret's paragraph splits, each
    // half taking the text beside it.
    let mut from = editor(&res, "Neutral", &blocks, ("alpha beta", 6), ("Middle", 3));
    let text = copy(&mut from);
    let mut to = at(&res, &[body("Xx Yy")], ("Xx Yy", 3));
    paste(&mut to, &text, &["Body: Xx beta", "Body: MidYy"]);
    // A selection ending at a paragraph's start brings the paragraph
    // break: a paragraph copied whole, then nothing of the next.
    let mut from = editor(&res, "Neutral", &blocks, ("Middle", 0), ("q1", 0));
    let text = copy(&mut from);
    let mut to = at(&res, &[body("Xx Yy")], ("Xx Yy", 3));
    paste(
        &mut to,
        &text,
        &["Body: Xx ", "Heading 1: Middle", "Body: Yy"],
    );
    let mut to = empty(&res);
    paste(&mut to, &text, &["Heading 1: Middle", "Body: "]);
}

/// List items paste in a list of their kind, with their nesting: outside
/// a list as a list, one item as a list of one; inside a list, they join
/// it at the caret's level.
#[test]
fn list_items_paste_in_a_list_with_their_nesting() {
    let res = res("lists");
    let blocks = [
        body("Intro"),
        BlockSpec::List(
            false,
            vec![
                vec![
                    body("one"),
                    BlockSpec::List(false, vec![vec![body("deep")]]),
                ],
                vec![body("two")],
            ],
        ),
        BlockSpec::List(true, vec![vec![body("first")], vec![body("second")]]),
    ];
    // What Copy keeps is what was copied last: each is copied before it's
    // pasted.
    let copied =
        |from: (&str, u32), to: (&str, u32)| copy(&mut editor(&res, "Neutral", &blocks, from, to));
    let whole = || copied(("one", 0), ("two", 3));
    let one = || copied(("two", 0), ("two", 3));
    let nested = || copied(("deep", 0), ("two", 3));
    let numbered = || copied(("first", 0), ("second", 6));
    // Outside a list.
    paste(
        &mut empty(&res),
        &whole(),
        &[
            "list",
            "  item",
            "    Body: one",
            "    list",
            "      item",
            "        Body: deep",
            "  item",
            "    Body: two",
        ],
    );
    paste(
        &mut empty(&res),
        &one(),
        &["list", "  item", "    Body: two"],
    );
    paste(
        &mut empty(&res),
        &nested(),
        &[
            "list",
            "  item",
            "    list",
            "      item",
            "        Body: deep",
            "  item",
            "    Body: two",
        ],
    );
    paste(
        &mut empty(&res),
        &numbered(),
        &[
            "list ordered",
            "  item",
            "    Body: first",
            "  item",
            "    Body: second",
        ],
    );
    // Into a list: at an item's end, after it.
    let list = [BlockSpec::List(
        true,
        vec![vec![body("alpha")], vec![body("beta")]],
    )];
    let mut to = at(&res, &list, ("alpha", 5));
    let caret = paste(
        &mut to,
        &whole(),
        &[
            "list ordered",
            "  item",
            "    Body: alpha",
            "  item",
            "    Body: one",
            "    list",
            "      item",
            "        Body: deep",
            "  item",
            "    Body: two",
            "  item",
            "    Body: beta",
        ],
    );
    assert_eq!(caret, ("two".to_string(), 3));
    // In its middle, between its halves, each in an item.
    let mut to = at(&res, &list, ("alpha", 2));
    paste(
        &mut to,
        &one(),
        &[
            "list ordered",
            "  item",
            "    Body: al",
            "  item",
            "    Body: two",
            "  item",
            "    Body: pha",
            "  item",
            "    Body: beta",
        ],
    );
    // At its start, before it.
    let mut to = at(&res, &list, ("beta", 0));
    paste(
        &mut to,
        &one(),
        &[
            "list ordered",
            "  item",
            "    Body: alpha",
            "  item",
            "    Body: two",
            "  item",
            "    Body: beta",
        ],
    );
    // An empty item is replaced.
    let mut to = at(
        &res,
        &[BlockSpec::List(
            false,
            vec![vec![body("x")], vec![body("")]],
        )],
        ("", 0),
    );
    paste(
        &mut to,
        &numbered(),
        &[
            "list",
            "  item",
            "    Body: x",
            "  item",
            "    Body: first",
            "  item",
            "    Body: second",
        ],
    );
    // At the caret's level, in a list nested in another.
    let mut to = at(
        &res,
        &[BlockSpec::List(
            false,
            vec![vec![
                body("top"),
                BlockSpec::List(false, vec![vec![body("inner")]]),
            ]],
        )],
        ("inner", 5),
    );
    paste(
        &mut to,
        &one(),
        &[
            "list",
            "  item",
            "    Body: top",
            "    list",
            "      item",
            "        Body: inner",
            "      item",
            "        Body: two",
        ],
    );
}

/// A quote pasted into a quote is one quote: its paragraphs join it.
#[test]
fn a_quote_pasted_into_a_quote_joins_it() {
    let res = res("quotes");
    let blocks = [
        BlockSpec::Quote(vec![body("q1"), body("q2")]),
        body("After"),
    ];
    let text = copy(&mut editor(&res, "Neutral", &blocks, ("q1", 0), ("q2", 2)));
    let mut to = at(&res, &[BlockSpec::Quote(vec![body("Xx Yy")])], ("Xx Yy", 3));
    paste(
        &mut to,
        &text,
        &[
            "quote",
            "  Body: Xx ",
            "  Body: q1",
            "  Body: q2",
            "  Body: Yy",
        ],
    );
}

/// A whole table pastes as a table, and rows as a table of those rows,
/// its first row's cells in Table Header; rows pasted into a table's cell
/// are text, as before.
#[test]
fn a_table_pastes_as_a_table_and_rows_into_a_cell_as_text() {
    let res = res("tables");
    let cell = |t: &str| vec![(t.to_string(), Props::new())];
    let table = |rows: &[[&str; 2]]| {
        BlockSpec::Table(
            vec![String::new(), "end".to_string()],
            rows.iter()
                .map(|r| r.iter().map(|t| cell(t)).collect())
                .collect(),
        )
    };
    let blocks = [
        body("Before"),
        table(&[["Name", "Size"], ["Pear", "Big"], ["Fig", "Small"]]),
        body("After"),
    ];
    let whole = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("Name", 0),
        ("Small", 5),
    ));
    paste(
        &mut empty(&res),
        &whole,
        &[
            "table",
            "  row",
            "    Table Header: Name",
            "    Table Header: Size",
            "  row",
            "    Body: Pear",
            "    Body: Big",
            "  row",
            "    Body: Fig",
            "    Body: Small",
        ],
    );
    // Its columns keep their alignment.
    let mut to = empty(&res);
    to.update(Event::Pasted(whole.clone()));
    let size = paragraph(to.document(), "Size");
    assert_eq!(
        to.document()
            .paragraph(size)
            .unwrap()
            .props
            .get(keys::ALIGN),
        Some(&Value::Text("end".to_string()))
    );
    // Rows, outside a table: a table of them, the first a header.
    let rows = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("Pear", 0),
        ("Small", 5),
    ));
    paste(
        &mut empty(&res),
        &rows,
        &[
            "table",
            "  row",
            "    Table Header: Pear",
            "    Table Header: Big",
            "  row",
            "    Body: Fig",
            "    Body: Small",
        ],
    );
    // From inside a table to the text after it: the row copied in part
    // keeps its shape, its cell not copied empty, and the paragraph after
    // joins the text after the caret.
    let out = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("Big", 0),
        ("After", 2),
    ));
    let mut to = at(&res, &[body("xy")], ("xy", 1));
    paste(
        &mut to,
        &out,
        &[
            "Body: x",
            "table",
            "  row",
            "    Table Header: ",
            "    Table Header: Big",
            "  row",
            "    Body: Fig",
            "    Body: Small",
            "Body: Afy",
        ],
    );
    // Rows into a cell: text, its lines run together.
    let mut to = at(&res, &[table(&[["H1", "H2"], ["c", "d"]])], ("c", 1));
    paste(
        &mut to,
        &rows,
        &[
            "table",
            "  row",
            "    Table Header: H1",
            "    Table Header: H2",
            "  row",
            "    Body: cPear Big Fig Small",
            "    Body: d",
        ],
    );
}

/// A rule pastes as a rule, and a code block as a code block; text pasted
/// into code is code's text, lines and all.
#[test]
fn a_rule_and_a_code_block_paste_as_themselves() {
    let res = res("rules");
    let blocks = [
        body("Above"),
        BlockSpec::Rule,
        BlockSpec::Code("rust".into(), vec!["fn main() {".into(), "}".into()]),
        body("Below"),
    ];
    let all = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("Above", 0),
        ("Below", 5),
    ));
    paste(
        &mut empty(&res),
        &all,
        &[
            "Body: Above",
            "rule",
            "Code: fn main() {\u{2028}}",
            "Body: Below",
        ],
    );
    let code = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("fn main() {\u{2028}}", 0),
        ("fn main() {\u{2028}}", 13),
    ));
    let mut to = at(&res, &[body("xy")], ("xy", 1));
    paste(
        &mut to,
        &code,
        &["Body: x", "Code: fn main() {\u{2028}}", "Body: y"],
    );
    // Its info string comes along.
    let pasted = paragraph(to.document(), "fn main() {\u{2028}}");
    assert_eq!(
        to.document()
            .paragraph(pasted)
            .unwrap()
            .props
            .get(keys::INFO),
        Some(&Value::Text("rust".to_string()))
    );
    // Into code: text.
    let mut to = at(
        &res,
        &[BlockSpec::Code(String::new(), vec!["// ".into()])],
        ("// ", 3),
    );
    paste(
        &mut to,
        &all,
        &["Code: // Above\u{2028}fn main() {\u{2028}}\u{2028}Below"],
    );
}

/// Cut then Paste moves a heading and its quote, and Undo takes each back.
#[test]
fn cut_then_paste_moves_blocks() {
    let res = res("cut");
    let blocks = [
        p("Heading 1", "Title"),
        BlockSpec::Quote(vec![body("Quoted")]),
        body("After"),
    ];
    let mut editor = editor(&res, "Neutral", &blocks, ("Title", 0), ("After", 0));
    let before = editor.document().clone();
    let text = match command(&mut editor, "cut").as_slice() {
        [Effect::Copy(text)] => text.clone(),
        effects => panic!("cut: {effects:?}"),
    };
    assert_eq!(outline(&editor), ["Heading 1: After"]);
    key(&mut editor, Key::End, true, false);
    paste(
        &mut editor,
        &text,
        &[
            "Heading 1: After",
            "Heading 1: Title",
            "quote",
            "  Body: Quoted",
            "Heading 1: ",
        ],
    );
    command(&mut editor, "undo");
    command(&mut editor, "undo");
    assert_eq!(editor.document(), &before, "the cut undone too");
}

/// Pasting from a document in another stylesheet: the destination's
/// styles win, matched by role, and those the pasted text uses that it
/// lacks come along, based on the destination's; nothing else comes.
#[test]
fn pasting_from_book_into_neutral_keeps_neutrals_styles() {
    let res = res("book");
    let blocks = [
        p("Heading 1", "Chapter"),
        p("First Paragraph", "Opening"),
        body("Text"),
    ];
    let book = editor(&res, "Book", &blocks, ("Chapter", 0), ("Text", 4));
    let size = |editor: &Editor, name: &str| {
        editor
            .sheet()
            .paragraph
            .iter()
            .find(|s| s.name == name)
            .map(|s| (s.size, s.family.clone()))
    };
    assert_ne!(size(&book, "Heading 1"), size(&empty(&res), "Heading 1"));
    let mut from = book;
    let text = copy(&mut from);
    let mut to = empty(&res);
    let styles = to.document().styles().len();
    paste(
        &mut to,
        &text,
        &[
            "Heading 1: Chapter",
            "First Paragraph: Opening",
            "Body: Text",
        ],
    );
    // Neutral's own Heading 1 and Body, and Book's First Paragraph alone
    // added, based on Neutral's Body.
    assert_eq!(size(&to, "Heading 1"), size(&empty(&res), "Heading 1"));
    assert_eq!(size(&to, "Body"), size(&empty(&res), "Body"));
    assert_eq!(to.document().styles().len(), styles + 1);
    let first = to
        .sheet()
        .paragraph
        .iter()
        .find(|s| s.name == "First Paragraph")
        .expect("First Paragraph came along")
        .clone();
    assert_eq!(first.base, to.sheet().default_style().id);
    // Body text alone brings nothing.
    let mut from = editor(&res, "Book", &blocks, ("Text", 0), ("Text", 4));
    let text = copy(&mut from);
    let mut to = empty(&res);
    paste(&mut to, &text, &["Body: Text"]);
    assert_eq!(to.document().styles().len(), styles);
}

/// The pictures fixtures.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pictures")
        .join(name)
}

/// The pictures the pages draw.
fn drawn(editor: &Editor) -> usize {
    editor
        .pages()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| matches!(item, scene::Item::Image { .. }))
        .count()
}

/// The IDs of the pictures `editor`'s document shows, in order.
fn pictures(editor: &Editor) -> Vec<String> {
    story::pictures(editor.document())
}

/// A picture comes along: within a document as it is, and into another
/// `.xw` document kept in its file, by its hash.
#[test]
fn a_picture_comes_along_within_a_document_and_into_another() {
    let res = res("picture");
    let mut from = Editor::create(res.clone()).expect("a document");
    from.update(Event::Text("A".to_string()));
    from.update(Event::Chosen(
        Choice::Picture,
        Some(fixture("gradient.jpg")),
    ));
    from.update(Event::Text("B".to_string()));
    assert_eq!(drawn(&from), 1);
    let id = pictures(&from)[0].clone();
    command(&mut from, "select-all");
    let text = copy(&mut from);
    assert_eq!(text, "AB", "the clipboard has its text without the picture");
    // Within the document.
    key(&mut from, Key::End, true, false);
    key(&mut from, Key::Enter, false, false);
    from.update(Event::Pasted(text.clone()));
    assert_eq!(outline(&from), ["Body: A\u{fffc}B", "Body: A\u{fffc}B"]);
    assert_eq!(drawn(&from), 2);
    // Into another document, which keeps it.
    let mut to = Editor::create(res.clone()).expect("another document");
    to.update(Event::Pasted(text.clone()));
    assert_eq!(outline(&to), ["Body: A\u{fffc}B"]);
    assert_eq!(pictures(&to), std::slice::from_ref(&id));
    assert_eq!(drawn(&to), 1);
    let path = to.path().to_path_buf();
    drop(to);
    let to = Editor::open(res.clone(), &path).expect("it opens");
    assert_eq!(drawn(&to), 1, "the document's file keeps it");
    assert_eq!(to.message(), "");
}

/// Pictures a Markdown file links to come along: into an `.xw` document
/// kept by their hashes, and into a Markdown file elsewhere linked from
/// there. A picture an `.xw` file keeps can't be linked from a Markdown
/// file, which asks; made an `.xw` file, it keeps the picture.
#[test]
fn pictures_come_along_between_markdown_and_xw_documents() {
    let res = res("markdown-pictures");
    let docs = res.documents.join("docs");
    let notes = res.documents.join("notes");
    std::fs::create_dir_all(docs.join("images")).unwrap();
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::copy(fixture("gradient.jpg"), docs.join("images/gradient.jpg")).unwrap();
    let source = docs.join("Source.md");
    std::fs::write(
        &source,
        "# Pictures\n\n![A gradient](images/gradient.jpg)\n",
    )
    .unwrap();
    let mut from = Editor::open(res.clone(), &source).expect("it opens");
    assert_eq!(drawn(&from), 1);
    command(&mut from, "select-all");
    let text = copy(&mut from);
    // Into an .xw document: kept, by its hash.
    let mut xw = Editor::create(res.clone()).expect("a document");
    xw.update(Event::Pasted(text.clone()));
    assert_eq!(outline(&xw), ["Heading 1: Pictures", "Body: \u{fffc}"]);
    let hash = store::image_id(&std::fs::read(fixture("gradient.jpg")).unwrap());
    assert_eq!(pictures(&xw), std::slice::from_ref(&hash));
    assert_eq!(drawn(&xw), 1);
    // Into a Markdown file in another folder: linked from there.
    let other = notes.join("Other.md");
    std::fs::write(&other, "").unwrap();
    let mut md = Editor::open(res.clone(), &other).expect("it opens");
    md.update(Event::Pasted(text.clone()));
    assert_eq!(drawn(&md), 1);
    md.flush();
    assert_eq!(
        std::fs::read_to_string(&other).unwrap(),
        "# Pictures\n\n![A gradient](../docs/images/gradient.jpg)\n"
    );
    // From the .xw document into a Markdown file: asked, and kept once
    // it's made an .xw file.
    command(&mut xw, "select-all");
    let text = copy(&mut xw);
    let third = notes.join("Third.md");
    std::fs::write(&third, "").unwrap();
    let mut md = Editor::open(res.clone(), &third).expect("it opens");
    md.update(Event::Pasted(text));
    assert!(md.panel().is_some(), "it asks: {}", md.message());
    md.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    assert!(!md.is_markdown(), "{}", md.message());
    assert_eq!(pictures(&md), [hash]);
    assert_eq!(drawn(&md), 1);
}

/// Pasting into a Markdown file writes what it holds; a style it can't
/// hold is asked about, as anything else is. A Markdown file's blocks
/// copied into an `.xw` document come as they were.
#[test]
fn pasting_into_and_out_of_markdown_files() {
    let res = res("markdown");
    let blocks = [
        p("Heading 1", "Title"),
        BlockSpec::Quote(vec![body("Quoted")]),
        BlockSpec::List(false, vec![vec![body("one")], vec![body("two")]]),
        BlockSpec::Rule,
        body("End"),
    ];
    let text = copy(&mut editor(
        &res,
        "Neutral",
        &blocks,
        ("Title", 0),
        ("End", 3),
    ));
    let path = res.documents.join("Notes.md");
    std::fs::write(&path, "").unwrap();
    let mut md = Editor::open(res.clone(), &path).expect("it opens");
    md.update(Event::Pasted(text));
    md.flush();
    let written = "# Title\n\n> Quoted\n\n- one\n- two\n\n---\n\nEnd\n";
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
    // Undo takes it back, and the file has it back as it was.
    command(&mut md, "undo");
    md.flush();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    command(&mut md, "redo");
    md.flush();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
    // Copied back out of it, into an .xw document.
    command(&mut md, "select-all");
    let text = copy(&mut md);
    paste(
        &mut empty(&res),
        &text,
        &[
            "Heading 1: Title",
            "quote",
            "  Body: Quoted",
            "list",
            "  item",
            "    Body: one",
            "  item",
            "    Body: two",
            "rule",
            "Body: End",
        ],
    );
    // A style Markdown can't hold: asked, and not made until answered.
    let mut from = Editor::create(res.clone()).expect("a document");
    from.update(Event::Text("Captioned".to_string()));
    command(&mut from, "new-style");
    command(&mut from, "select-all");
    let text = copy(&mut from);
    let before = md.document().clone();
    key(&mut md, Key::End, true, false);
    md.update(Event::Pasted(text));
    assert!(md.panel().is_some(), "it asks: {}", md.message());
    assert_eq!(md.document(), &before);
}

/// The characters of `editor`'s document's paragraphs.
fn characters(editor: &Editor) -> usize {
    let document = editor.document();
    document
        .paragraphs()
        .into_iter()
        .map(|id| document.paragraph(id).unwrap().len() as usize)
        .sum()
}

/// Selections from paragraphs' starts, middles and ends to others', in a
/// document of every kind of block, pasted at paragraphs' edges and
/// middles of every kind: the document holds together, no text is lost
/// or doubled, and Undo takes each paste back.
#[test]
fn every_selection_pastes_anywhere_and_undoes() {
    let res = res("every");
    let cell = |t: &str| vec![(t.to_string(), Props::new())];
    let table = |rows: &[[&str; 2]]| {
        BlockSpec::Table(
            vec![String::new(); 2],
            rows.iter()
                .map(|r| r.iter().map(|t| cell(t)).collect())
                .collect(),
        )
    };
    let source = [
        p("Heading 1", "Head"),
        BlockSpec::List(
            false,
            vec![
                vec![body("one"), BlockSpec::List(true, vec![vec![body("deep")]])],
                vec![body("two")],
            ],
        ),
        BlockSpec::Quote(vec![body("q1"), BlockSpec::Rule, body("q2")]),
        table(&[["ta", "tb"], ["tc", "td"]]),
        BlockSpec::Code(String::new(), vec!["co".into(), "de".into()]),
        BlockSpec::Rule,
        body("End"),
    ];
    let mut from = editor(&res, "Neutral", &source, ("Head", 0), ("Head", 0));
    // The places a step at a time from the start, kept at paragraphs'
    // edges and one character in.
    let mut places = vec![0];
    let mut step = 0;
    let mut previous = from.selection().1;
    loop {
        key(&mut from, Key::Right, false, false);
        let now = from.selection().1;
        if now == previous {
            break;
        }
        step += 1;
        let len = from.document().paragraph(now.paragraph).unwrap().len();
        if now.offset <= 1 || now.offset == len {
            places.push(step);
        }
        previous = now;
    }
    let list = || BlockSpec::List(false, vec![vec![body("Li")], vec![body("")]]);
    let destinations: Vec<(Vec<BlockSpec>, (&str, u32))> = vec![
        (Vec::new(), ("", 0)),
        (vec![body("Xy")], ("Xy", 0)),
        (vec![body("Xy")], ("Xy", 1)),
        (vec![body("Xy")], ("Xy", 2)),
        (vec![list()], ("Li", 2)),
        (vec![list()], ("", 0)),
        (vec![BlockSpec::Quote(vec![body("Qu")])], ("Qu", 1)),
        (vec![table(&[["Ca", "Cb"]])], ("Ca", 1)),
        (
            vec![BlockSpec::Code(String::new(), vec!["Co".into()])],
            ("Co", 2),
        ),
    ];
    let mut to: Vec<Editor> = destinations
        .iter()
        .map(|(blocks, caret)| at(&res, blocks, *caret))
        .collect();
    let mut pasted = 0;
    for (i, &a) in places.iter().enumerate() {
        // A quarter of the selections, spread over them all, as each
        // paste is saved and laid out.
        for (j, &b) in places.iter().enumerate().skip(i + 1) {
            if (i + j) % 4 != 0 {
                continue;
            }
            key(&mut from, Key::Home, true, false);
            for _ in 0..a {
                key(&mut from, Key::Right, false, false);
            }
            for _ in a..b {
                key(&mut from, Key::Right, false, true);
            }
            let text = copy(&mut from);
            let lines = text.chars().filter(|&c| c != '\n').count();
            for (k, editor) in to.iter_mut().enumerate() {
                let before = editor.document().clone();
                let count = characters(editor);
                editor.update(Event::Pasted(text.clone()));
                let what = format!("{text:?} pasted into destination {k}");
                assert!(
                    Document::from_snapshot(editor.document().to_snapshot()).is_ok(),
                    "{what} holds together"
                );
                // A cell or code takes the text's line breaks as spaces or
                // line breaks of its own.
                let added = if k >= 7 { text.chars().count() } else { lines };
                assert_eq!(
                    characters(editor),
                    count + added,
                    "{what}: {:?}",
                    outline(editor)
                );
                command(editor, "undo");
                assert_eq!(editor.document(), &before, "{what}, undone");
                pasted += 1;
            }
        }
    }
    assert!(pasted > 500, "{pasted}");
}
