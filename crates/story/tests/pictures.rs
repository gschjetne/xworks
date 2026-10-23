//! Pictures in a story (docs/ROADMAP.md, M2 stage 7): each an OBJECT
//! character in a paragraph's text whose run names the picture, as
//! Markdown's `![alt](src)` sits in a paragraph's text; a paragraph of one
//! picture alone is a figure.

use std::sync::OnceLock;

use doc::{Props, Value};
use story::{OBJECT, StyleSheet, keys, props};
use text::Fonts;
use text::units::INCH;

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

/// A picture's run: `id`, `alt`, placed `width` by `height`.
fn picture(id: &str, alt: &str, width: i64, height: i64) -> Props {
    props(&[
        (keys::PICTURE, Value::Text(id.to_string())),
        (keys::PICTURE_ALT, Value::Text(alt.to_string())),
        (keys::PICTURE_WIDTH, Value::Int(width)),
        (keys::PICTURE_HEIGHT, Value::Int(height)),
    ])
}

#[test]
fn a_picture_in_text_is_placed_scaled_to_its_paragraph() {
    let object = OBJECT.to_string();
    let document = story::document(&[
        (
            "Body",
            vec![
                ("Before ".to_string(), Props::new()),
                (
                    object.clone(),
                    picture("a1", "A wide view", 10 * INCH, 5 * INCH),
                ),
                (" after.".to_string(), Props::new()),
            ],
        ),
        (
            "Body",
            vec![(object.clone(), picture("b2", "", 2 * INCH, INCH))],
        ),
        // Text that names a picture but isn't its character shows none.
        (
            "Body",
            vec![("x".to_string(), picture("c3", "", INCH, INCH))],
        ),
    ]);
    let laid = story::lay_out(&document, &sheet(), fonts(), 6 * INCH);
    let first = &laid[0];
    assert_eq!(first.pictures.len(), 1);
    let placed = first.pictures[0].clone();
    assert_eq!(placed.offset, "Before ".len());
    assert_eq!(placed.id, "a1");
    assert_eq!(placed.alt.as_deref(), Some("A wide view"));
    assert!(!first.figure);
    // Ten inches wide, in six: scaled to six by three.
    let objects: Vec<text::Placed> = first
        .layout
        .lines
        .iter()
        .flat_map(|l| l.objects.iter().copied())
        .collect();
    assert_eq!(objects.len(), 1);
    assert_eq!((objects[0].width, objects[0].height), (6 * INCH, 3 * INCH));
    // A picture alone is a figure, and an empty alt is none.
    let second = &laid[1];
    assert!(second.figure);
    assert_eq!(second.pictures[0].alt, None);
    let line = &second.layout.lines[0];
    assert_eq!(
        (line.objects[0].width, line.objects[0].height),
        (2 * INCH, INCH)
    );
    assert!(line.ascent >= INCH);
    assert!(laid[2].pictures.is_empty() && !laid[2].figure);
    // The document shows a1 and b2, and names c3, which it doesn't show
    // but keeps, as its run does.
    assert_eq!(story::pictures(&document), ["a1", "b2", "c3"]);
}

#[test]
fn a_pictures_size_is_kept_within_bounds() {
    let mut huge = picture("big", "", i64::MAX, -4);
    huge.remove(keys::PICTURE_ALT);
    assert_eq!(
        story::picture_of(&huge),
        Some(("big".to_string(), None, story::LARGEST_PICTURE, 1))
    );
    assert_eq!(story::picture_of(&Props::new()), None);
    // Without a size, an inch square.
    let bare = props(&[(keys::PICTURE, Value::Text("x".to_string()))]);
    assert_eq!(
        story::picture_of(&bare),
        Some(("x".to_string(), None, INCH, INCH))
    );
}
