//! A real document in automatic pages (docs/ROADMAP.md, M1 stage 7): the
//! sample document in M1's styles, poured into A4 pages. Its text stays
//! within the margins, and an edit leaves the pages before it as they were.

use std::sync::OnceLock;

use canvas::{PageStyle, PageStyleSpec, Piece};
use doc::{Document, Op, Props, Run};
use scene::Item;
use serde::Deserialize;
use story::StyleSheet;
use text::Fonts;

#[derive(Deserialize)]
struct Sheet {
    page: PageStyleSpec,
}

fn spec(dir: &str, name: &str) -> String {
    testkit::specs(dir)
        .expect("spec/")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("spec/{dir}/{name}.ron"))
        .1
}

fn styles() -> (StyleSheet, PageStyle) {
    let source = spec("styles", "m1");
    let sheet: StyleSheet = ron::from_str(&source).expect("a style sheet");
    let page: Sheet = ron::from_str(&source).expect("a page style");
    (sheet, PageStyle::from_spec(&page.page))
}

fn sample() -> Document {
    let paragraphs: Vec<(String, Vec<(String, Props)>)> =
        ron::from_str(&spec("documents", "m1-sample")).expect("a document");
    story::document(&paragraphs)
}

/// The sample twice over: several pages in any of the bundled fonts. The
/// sample alone fills two in Source Serif 4.
fn long_sample() -> Document {
    let mut paragraphs: Vec<(String, Vec<(String, Props)>)> =
        ron::from_str(&spec("documents", "m1-sample")).expect("a document");
    paragraphs.extend(paragraphs.clone());
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

/// Where `document`'s pages break: its pieces, page by page.
fn breaks(document: &Document) -> Vec<Vec<Piece>> {
    let (sheet, page) = styles();
    let paragraphs = story::lay_out(document, &sheet, fonts(), page.body_width());
    canvas::paginate(
        page.body_height(),
        &canvas::blocks(&paragraphs, &sheet.blocks),
    )
}

#[test]
fn the_sample_fills_pages_within_their_margins() {
    let (sheet, style) = styles();
    let pages = canvas::automatic(&long_sample(), &sheet, &style, fonts()).pages;
    assert!(pages.len() >= 3, "{} pages", pages.len());
    for (n, page) in pages.iter().enumerate() {
        assert_eq!((page.width, page.height), (style.width, style.height));
        for item in &page.items {
            let Item::Glyphs(run) = item else { continue };
            assert!(
                run.baseline > style.top && run.baseline <= style.height - style.bottom,
                "page {n}: a baseline at {} is outside the margins",
                run.baseline
            );
            for glyph in &run.glyphs {
                // Spaces at a line's end may hang into the margin; letters
                // stay within it.
                let space = run.text[glyph.text.clone()].trim().is_empty();
                assert!(
                    glyph.at.x >= style.left - 1
                        && (space || glyph.at.x < style.width - style.right),
                    "page {n}: {:?} at {} is outside the margins",
                    &run.text[glyph.text.clone()],
                    glyph.at.x
                );
            }
        }
    }
}

#[test]
fn an_edit_leaves_the_pages_before_it() {
    let document = sample();
    let before = breaks(&document);
    let ids = document.paragraphs().to_vec();
    for k in [ids.len() - 1, ids.len() / 2, 3] {
        let mut edited = document.clone();
        let text = " And another sentence, to make it longer.".repeat(6);
        let len = text.chars().count() as u32;
        let end = edited.paragraph(ids[k]).expect("a paragraph").len();
        edited
            .apply(&Op::InsertText {
                paragraph: ids[k],
                offset: end,
                text,
                runs: vec![Run::new(len, Props::new())],
            })
            .expect("the edit applies");
        let after = breaks(&edited);
        assert_ne!(before, after, "editing paragraph {k} changes something");
        // Headings before it move with it, so its page is where the
        // headings above it start.
        let (sheet, _) = styles();
        let mut start = k;
        while start > 0 {
            let style = match edited
                .paragraph(ids[start - 1])
                .and_then(|p| p.props.get(story::keys::STYLE))
            {
                Some(doc::Value::Text(name)) => sheet.paragraph_style(Some(name)).keep_with_next,
                _ => false,
            };
            if !style {
                break;
            }
            start -= 1;
        }
        let page = before
            .iter()
            .position(|p| p.iter().any(|q| q.block == start))
            .expect("the paragraph is on a page");
        assert_eq!(before[..page], after[..page], "editing paragraph {k}");
    }
}

#[test]
fn the_same_document_breaks_the_same_way() {
    assert_eq!(breaks(&sample()), breaks(&sample()));
}

/// The document with every kind of block (spec/documents/m2-blocks.ron).
fn blocks_document() -> Document {
    let blocks: Vec<story::BlockSpec> =
        ron::from_str(&spec("documents", "m2-blocks")).expect("a document");
    story::build(&blocks)
}

/// Lists, quotes, code and tables stay within the margins, and every
/// glyph run, markers' included, points at a block of the structure
/// that holds text.
#[test]
fn blocks_draw_within_the_margins_and_point_into_the_structure() {
    use scene::Role;
    let (sheet, style) = styles();
    let pages = canvas::automatic(&blocks_document(), &sheet, &style, fonts());
    let roles = &pages.structure.blocks;
    for role in [
        Role::List { ordered: false },
        Role::List { ordered: true },
        Role::ListItem,
        Role::Label,
        Role::ListBody,
        Role::Quote,
        Role::Code,
        Role::Table,
        Role::Row,
        Role::Cell { header: true },
        Role::Cell { header: false },
    ] {
        assert!(roles.contains(&role), "{role:?}");
    }
    let mut labels = 0;
    for page in &pages.pages {
        for item in &page.items {
            let Item::Glyphs(run) = item else { continue };
            for glyph in &run.glyphs {
                let space = run.text[glyph.text.clone()].trim().is_empty();
                assert!(
                    glyph.at.x >= style.left - 1
                        && (space || glyph.at.x < style.width - style.right),
                    "{:?} at {} is outside the margins",
                    run.text,
                    glyph.at.x
                );
            }
            let block = run.source.expect("a source").block as usize;
            assert!(
                matches!(
                    roles[block],
                    Role::Paragraph
                        | Role::Heading(_)
                        | Role::Code
                        | Role::Cell { .. }
                        | Role::Label
                ),
                "{:?} points at {:?}",
                run.text,
                roles[block]
            );
            labels += usize::from(roles[block] == Role::Label);
        }
    }
    // Every item has its marker: 3 + 2 bulleted, 3 numbered, 2 quoted.
    assert_eq!(labels, 10);
}

/// A table's row pours as one block, as tall as its tallest cell and the
/// room around it; its header keeps with the row after it.
#[test]
fn a_tables_row_pours_as_one_block() {
    let (sheet, page) = styles();
    let laid = story::lay_out(&blocks_document(), &sheet, fonts(), page.body_width());
    let units = canvas::units(&laid);
    let blocks = canvas::blocks(&laid, &sheet.blocks);
    assert_eq!(units.len(), blocks.len());
    let rows: Vec<usize> = (0..units.len())
        .filter(|&u| laid[units[u].start].place.cell.is_some())
        .collect();
    assert_eq!(rows.len(), 5);
    let pad = sheet.blocks.cell_padding.units();
    for &u in &rows {
        assert_eq!(units[u].len(), 3, "three cells a row");
        let tallest = laid[units[u].clone()]
            .iter()
            .map(|c| c.layout.height)
            .max()
            .unwrap();
        assert_eq!(blocks[u].lines, [tallest + 2 * pad]);
    }
    assert!(blocks[rows[0]].keep_with_next);
    assert_eq!(
        blocks[rows[0]].space_before,
        sheet.blocks.table_space.units()
    );
    assert_eq!(
        blocks[rows[4]].space_after,
        sheet.blocks.table_space.units()
    );
    // Everything else is a paragraph of its own.
    assert_eq!(units.len(), laid.len() - 5 * 2);
}

/// Struck text is drawn with a line through it, as long as its glyphs; a
/// task's box is drawn in its bullet's place, with a tick if it's done.
/// Both are strokes and fills outside the reading order.
#[test]
fn struck_text_and_tasks_draw_their_lines_and_boxes() {
    use story::{BlockSpec, keys, props};
    let (sheet, style) = styles();
    let p = |text: &str| BlockSpec::Paragraph("Body".into(), vec![(text.into(), props(&[]))]);
    let mut document = story::build(&[
        BlockSpec::Paragraph(
            "Body".into(),
            vec![
                ("Kept and ".into(), props(&[])),
                (
                    "struck".into(),
                    props(&[(keys::STRIKETHROUGH, doc::Value::Bool(true))]),
                ),
            ],
        ),
        BlockSpec::List(false, vec![vec![p("to do")], vec![p("done")]]),
    ]);
    let list = document.children(document.root())[1];
    for (item, done) in document
        .children(list)
        .to_vec()
        .into_iter()
        .zip([false, true])
    {
        document
            .apply(&Op::SetBlockProp {
                block: item,
                key: keys::TASK.to_string(),
                value: Some(doc::Value::Bool(done)),
            })
            .expect("it applies");
    }
    let plain = canvas::automatic(
        &story::build(&[p("Kept and struck")]),
        &sheet,
        &style,
        fonts(),
    );
    let pages = canvas::automatic(&document, &sheet, &style, fonts());
    let fills = |pages: &canvas::Pages| {
        pages.pages[0]
            .items
            .iter()
            .filter(|i| matches!(i, Item::Fill { .. }))
            .count()
    };
    assert_eq!(fills(&pages), fills(&plain) + 1, "a line through the text");
    let strokes: Vec<usize> = pages.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Stroke { path, .. } => Some(path.0.len()),
            _ => None,
        })
        .collect();
    // Two boxes, and a tick in the second.
    assert_eq!(strokes, [5, 5, 3]);
    // No bullets: the boxes take their place.
    let bullets = pages.pages[0]
        .items
        .iter()
        .filter(|i| matches!(i, Item::Glyphs(g) if g.text == "•"))
        .count();
    assert_eq!(bullets, 0);
}
