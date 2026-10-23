//! Pictures in pages (docs/ROADMAP.md, M2 stage 7): each drawn where its
//! character stands, on its line's baseline, with its alternative text and
//! its place in the document's structure; a paragraph of one picture
//! alone is a figure; a picture that can't be drawn is a tint.

use std::sync::OnceLock;

use canvas::{PageStyle, PageStyleSpec, Pictures};
use doc::{Props, Value};
use scene::{Item, Role};
use serde::Deserialize;
use story::{OBJECT, StyleSheet, keys, props};
use text::Fonts;
use text::units::INCH;

#[derive(Deserialize)]
struct Sheet {
    page: PageStyleSpec,
}

fn styles() -> (StyleSheet, PageStyle) {
    let source = testkit::specs("styles")
        .expect("spec/")
        .into_iter()
        .find(|(n, _)| n == "m1")
        .expect("spec/styles/m1.ron")
        .1;
    let sheet: StyleSheet = ron::from_str(&source).expect("a style sheet");
    let page: Sheet = ron::from_str(&source).expect("a page style");
    (sheet, PageStyle::from_spec(&page.page))
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

fn picture(id: &str, alt: &str, width: i64, height: i64) -> Props {
    props(&[
        (keys::PICTURE, Value::Text(id.to_string())),
        (keys::PICTURE_ALT, Value::Text(alt.to_string())),
        (keys::PICTURE_WIDTH, Value::Int(width)),
        (keys::PICTURE_HEIGHT, Value::Int(height)),
    ])
}

fn solid(rgba: [u8; 4]) -> scene::Image {
    scene::Image::new(2, 2, rgba.repeat(4)).expect("an image")
}

#[test]
fn pictures_are_drawn_where_their_characters_stand() {
    let object = OBJECT.to_string();
    let document = story::document(&[
        (
            "Body",
            vec![
                ("Text with ".to_string(), Props::new()),
                (object.clone(), picture("inline", "A dot", 2 * INCH, INCH)),
                (" in it.".to_string(), Props::new()),
            ],
        ),
        (
            "Body",
            vec![(
                object.clone(),
                picture("figure", "A figure", 3 * INCH, 2 * INCH),
            )],
        ),
        (
            "Body",
            vec![(object.clone(), picture("missing", "", INCH, INCH))],
        ),
    ]);
    let (sheet, style) = styles();
    let mut pictures = Pictures::new();
    pictures.insert("inline".to_string(), solid([200, 0, 0, 255]));
    pictures.insert("figure".to_string(), solid([0, 0, 200, 255]));
    let pages = canvas::automatic_with(&document, &sheet, &style, fonts(), &pictures);
    let items = &pages.pages[0].items;
    let images: Vec<(scene::Rect, Option<String>, Option<scene::Source>)> = items
        .iter()
        .filter_map(|item| match item {
            Item::Image {
                rect, alt, source, ..
            } => Some((*rect, alt.clone(), *source)),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);
    let (inline, figure) = (&images[0], &images[1]);
    assert_eq!((inline.0.width, inline.0.height), (2 * INCH, INCH));
    assert_eq!(inline.1.as_deref(), Some("A dot"));
    assert_eq!((figure.0.width, figure.0.height), (3 * INCH, 2 * INCH));
    assert_eq!(figure.1.as_deref(), Some("A figure"));
    // The inline one stands on its line's baseline, after the text
    // before it.
    let baseline = items
        .iter()
        .find_map(|item| match item {
            Item::Glyphs(g) if g.text.starts_with("Text with") => Some(g.baseline),
            _ => None,
        })
        .expect("the first line's text");
    assert_eq!(inline.0.bottom(), baseline);
    // Each is in its paragraph's block of the structure, where its
    // character is: the first in a paragraph, the second a figure.
    let structure = &pages.structure;
    let inline_source = inline.2.expect("a source");
    assert_eq!(
        structure.blocks[inline_source.block as usize],
        Role::Paragraph
    );
    assert_eq!(inline_source.offset, "Text with ".len());
    let figure_source = figure.2.expect("a source");
    assert_eq!(structure.blocks[figure_source.block as usize], Role::Figure);
    // The one the file lacks is a tint of its size.
    let tint = sheet.blocks.missing_picture;
    let tints = items
        .iter()
        .filter(|item| {
            matches!(item, Item::Fill { color, path }
                if *color == scene::Color::rgb(tint.0, tint.1, tint.2)
                    && item.bounds().is_some_and(|b| (b.width, b.height) == (INCH, INCH))
                    && !path.0.is_empty())
        })
        .count();
    assert_eq!(tints, 1);
    // Without the pictures, each is a tint.
    let bare = canvas::automatic(&document, &sheet, &style, fonts());
    assert!(
        !bare.pages[0]
            .items
            .iter()
            .any(|item| matches!(item, Item::Image { .. }))
    );
}
