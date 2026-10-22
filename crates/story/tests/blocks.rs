//! A story's tree laid out (docs/ROADMAP.md, M2 stage 2): its paragraphs in
//! reading order, each in its place among lists, quotes, code and tables,
//! with the marks Markdown needs.

use std::sync::OnceLock;

use doc::{Kind, Value};
use story::{BlockSpec, StyleSheet, keys, props};
use text::Fonts;
use text::units::POINT;

fn sheet() -> StyleSheet {
    let source = testkit::specs("styles")
        .expect("spec/styles")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("spec/styles/m1.ron")
        .1;
    ron::from_str(&source).expect("m1.ron is a style sheet")
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

fn p(text: &str) -> BlockSpec {
    BlockSpec::Paragraph("Body".into(), vec![(text.into(), props(&[]))])
}

fn cell(text: &str) -> Vec<(String, doc::Props)> {
    vec![(text.into(), props(&[]))]
}

const WIDTH: i64 = 400 * POINT;

/// A document with every kind of block.
fn every_kind() -> Vec<BlockSpec> {
    vec![
        p("Intro"),
        BlockSpec::List(
            false,
            vec![
                vec![p("one"), BlockSpec::List(false, vec![vec![p("one.a")]])],
                vec![p("two")],
            ],
        ),
        BlockSpec::List(true, vec![vec![p("first")], vec![p("second")]]),
        BlockSpec::Quote(vec![p("quoted"), BlockSpec::Quote(vec![p("deeper")])]),
        BlockSpec::Code("rust".into(), vec!["fn main() {".into(), "}".into()]),
        BlockSpec::Table(
            vec!["".into(), "end".into()],
            vec![vec![cell("Name"), cell("Size")], vec![cell("a"), cell("1")]],
        ),
    ]
}

#[test]
fn the_tree_reads_in_order_with_its_places() {
    let sheet = sheet();
    let document = story::build(&every_kind());
    let flow = story::flow(&document, &sheet, WIDTH);
    let texts: Vec<&str> = flow
        .iter()
        .map(|e| document.paragraph(e.id).unwrap().text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "Intro",
            "one",
            "one.a",
            "two",
            "first",
            "second",
            "quoted",
            "deeper",
            "fn main() {\u{2028}}",
            "Name",
            "Size",
            "a",
            "1"
        ]
    );
    let list = sheet.blocks.list_indent.units();
    let quote = sheet.blocks.quote_indent.units();
    let at = |k: usize| &flow[k].place;
    assert_eq!(*at(0), story::Place::top(WIDTH));
    // Bullets by depth, numbers counted, not stored.
    let markers: Vec<Option<&str>> = (1..6).map(|k| at(k).marker.as_deref()).collect();
    assert_eq!(
        markers,
        [Some("•"), Some("–"), Some("•"), Some("1."), Some("2.")]
    );
    assert_eq!((at(1).indent, at(1).width), (list, WIDTH - list));
    assert_eq!((at(2).indent, at(2).gutter), (2 * list, 2 * list));
    assert_eq!(
        at(2).path.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [Kind::List, Kind::Item, Kind::List, Kind::Item]
    );
    assert!(at(4).path[0].ordered);
    // Quotes, each with its bar.
    assert_eq!(at(6).indent, quote);
    assert_eq!(at(7).indent, 2 * quote);
    assert_eq!(
        at(7).quotes.iter().map(|q| q.1).collect::<Vec<_>>(),
        [0, quote]
    );
    // Cells, a column each.
    let cells: Vec<(u32, u32, i64)> = (9..13)
        .map(|k| {
            let c = at(k).cell.unwrap();
            (c.index, c.column, c.x)
        })
        .collect();
    assert_eq!(
        cells,
        [(0, 0, 0), (0, 1, WIDTH / 2), (1, 0, 0), (1, 1, WIDTH / 2)]
    );
    assert!(at(11).cell.unwrap().last && !at(9).cell.unwrap().last);
}

#[test]
fn blocks_lay_out_in_their_places() {
    let sheet = sheet();
    let document = story::build(&every_kind());
    let laid = story::lay_out(&document, &sheet, fonts(), WIDTH);
    // Markers are laid out; a code block is code, in its face, its lines
    // broken where its line separators are.
    assert!(laid[1].marker.is_some() && laid[0].marker.is_none());
    let code = &laid[8];
    assert!(code.code);
    assert_eq!(code.layout.lines.len(), 2);
    let face = fonts().face(code.layout.lines[0].runs[0].face);
    assert_eq!(face.family(), "JetBrains Mono");
    // A header's cells are bold; a cell set to end its lines does.
    let header = fonts().face(laid[9].layout.lines[0].runs[0].face);
    assert_eq!(header.weight(), 700);
    let body = fonts().face(laid[11].layout.lines[0].runs[0].face);
    assert_eq!(body.weight(), 400);
    let size = &laid[12].layout.lines[0];
    assert_eq!(size.x + size.width, laid[12].place.width);
    // Nothing is missing from the fonts.
    assert!(laid.iter().all(|p| !p.layout.missing));
    assert!(
        laid.iter()
            .filter_map(|p| p.marker.as_ref())
            .all(|m| !m.missing)
    );
}

#[test]
fn marks_set_code_and_links_apart() {
    let sheet = sheet();
    let document = story::build(&[BlockSpec::Paragraph(
        "Body".into(),
        vec![
            ("Call ".into(), props(&[])),
            ("main()".into(), props(&[(keys::CODE, Value::Bool(true))])),
            (" or read ".into(), props(&[])),
            (
                "the guide".into(),
                props(&[(keys::LINK, Value::Text("https://example.com".into()))]),
            ),
        ],
    )]);
    let id = document.paragraphs()[0];
    let spans = story::spans(document.paragraph(id).unwrap(), &sheet);
    let families: Vec<&str> = spans.iter().map(|s| s.style.family.as_str()).collect();
    assert_eq!(
        families,
        [
            "Source Serif 4",
            "JetBrains Mono",
            "Source Serif 4",
            "Source Serif 4"
        ]
    );
    let laid = story::lay_out(&document, &sheet, fonts(), WIDTH);
    assert_eq!(laid[0].links, [false, false, false, true]);
    assert_eq!(
        laid[0].inline_code,
        vec![std::ops::Range { start: 5, end: 11 }]
    );
}

#[test]
fn a_story_of_paragraphs_lays_out_as_m1s_did() {
    let sheet = sheet();
    let document = story::document(&[
        ("Heading 1", vec![("A heading", props(&[]))]),
        ("Body", vec![("Some text.", props(&[]))]),
    ]);
    for (laid, id) in story::lay_out(&document, &sheet, fonts(), WIDTH)
        .iter()
        .zip(document.paragraphs())
    {
        let alone = story::lay_out_paragraph(&document, id, &sheet, fonts(), WIDTH);
        assert_eq!(laid.id, id);
        assert_eq!(laid.place, story::Place::top(WIDTH));
        assert_eq!(format!("{:?}", laid.layout), format!("{:?}", alone.layout));
    }
}

#[test]
fn the_outline_shows_the_tree() {
    let document = story::build(&every_kind());
    let outline = story::outline(&document, &sheet());
    assert_eq!(
        outline[..9],
        [
            "Body: Intro",
            "list",
            "  item",
            "    Body: one",
            "    list",
            "      item",
            "        Body: one.a",
            "  item",
            "    Body: two",
        ]
    );
    assert!(outline.contains(&"list ordered".to_string()));
    assert!(outline.contains(&"Code: fn main() {\u{2028}}".to_string()));
    assert!(outline.contains(&"  row".to_string()));
}

#[test]
fn nesting_past_the_deepest_lays_out_as_if_it_stopped() {
    use doc::{Block, Document, Object, ObjectId, Op, Paragraph, Props};
    let sheet = sheet();
    // A quote in a quote, a hundred thousand deep, in one operation.
    let mut document = Document::new();
    let root = document.root();
    let depth = 100_000u64;
    let mut objects: Vec<(ObjectId, Object)> = (0..depth)
        .map(|i| {
            let id = ObjectId(10 + i);
            let parent = if i == 0 { root } else { ObjectId(9 + i) };
            let block = Block {
                kind: Kind::Quote,
                parent,
                children: vec![ObjectId(11 + i)],
                props: Props::new(),
            };
            (id, Object::Block(block))
        })
        .collect();
    objects.push((
        ObjectId(10 + depth),
        Object::Paragraph(Paragraph {
            parent: ObjectId(9 + depth),
            text: String::new(),
            runs: Vec::new(),
            props: Props::new(),
        }),
    ));
    document
        .apply(&Op::InsertBlock {
            parent: root,
            after: None,
            objects,
        })
        .expect("it applies");
    let first = document.paragraphs()[1];
    document
        .apply(&Op::DeleteBlock { block: first })
        .expect("it goes");
    let flow = story::flow(&document, &sheet, WIDTH);
    assert_eq!(flow.len(), 1);
    assert_eq!(flow[0].place.path.len(), story::DEEPEST);
}
