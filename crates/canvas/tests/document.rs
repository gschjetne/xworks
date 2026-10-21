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
    canvas::paginate(page.body_height(), &canvas::blocks(&paragraphs))
}

#[test]
fn the_sample_fills_pages_within_their_margins() {
    let (sheet, style) = styles();
    let pages = canvas::automatic(&sample(), &sheet, &style, fonts());
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
