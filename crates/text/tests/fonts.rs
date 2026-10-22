//! Loading the bundled fonts, and choosing faces from them.

mod common;

use common::fonts;
use text::{Align, FontError, Fonts, Language, Paragraph, Span, Style, units};

#[test]
fn every_bundled_face_loads_with_its_family_weight_and_slant() {
    let mut faces: Vec<(String, u16, bool)> = fonts()
        .faces()
        .map(|(_, f)| (f.family().to_string(), f.weight(), f.italic()))
        .collect();
    faces.sort();
    let mut want = Vec::new();
    for family in ["Noto Sans", "Noto Serif"] {
        for weight in [400, 700] {
            for italic in [false, true] {
                want.push((family.to_string(), weight, italic));
            }
        }
    }
    for family in [
        "Noto Naskh Arabic",
        "Noto Sans Arabic",
        "Noto Sans CJK SC",
        "Noto Sans Hebrew",
        "Noto Serif CJK SC",
        "Noto Serif Hebrew",
    ] {
        for weight in [400, 700] {
            want.push((family.to_string(), weight, false));
        }
    }
    // The interface's font: regular, medium and semibold.
    for weight in [400, 500, 600] {
        want.push(("Inter".to_string(), weight, false));
    }
    want.sort();
    assert_eq!(faces, want);
}

#[test]
fn the_nearest_face_is_chosen() {
    let fonts = fonts();
    let pick = |family: &str, weight: u16, italic: bool| {
        fonts
            .select(family, weight, italic)
            .map(|id| (fonts.face(id).weight(), fonts.face(id).italic()))
    };
    assert_eq!(pick("Noto Serif", 700, true), Some((700, true)));
    assert_eq!(pick("Noto Serif", 400, false), Some((400, false)));
    // 600 is nearer bold than regular; 550 is as near either, and the
    // heavier wins.
    assert_eq!(pick("Noto Serif", 600, false), Some((700, false)));
    assert_eq!(pick("Noto Serif", 550, false), Some((700, false)));
    assert_eq!(pick("Noto Serif", 450, false), Some((400, false)));
    // Hebrew has no italic: the upright face stands in.
    assert_eq!(pick("Noto Serif Hebrew", 400, true), Some((400, false)));
    assert_eq!(pick("No Such Family", 400, false), None);
}

#[test]
fn what_isnt_a_font_is_refused() {
    let mut fonts = Fonts::new();
    assert!(matches!(fonts.add(Vec::new()), Err(FontError::NotAFont(_))));
    assert!(matches!(
        fonts.add(b"not a font, only some text".to_vec()),
        Err(FontError::NotAFont(_))
    ));
    assert_eq!(fonts.faces().count(), 0);
    // A real font, cut short, refuses rather than loads half-read.
    let (_, bytes) = testkit::fonts::load()
        .expect("the bundled fonts")
        .into_iter()
        .find(|(name, _)| name == "NotoSerif-Regular.ttf")
        .expect("Noto Serif");
    assert!(fonts.add(bytes[..200].to_vec()).is_err());
    assert_eq!(fonts.faces().count(), 0);
}

#[test]
fn italic_without_an_italic_face_is_slanted() {
    let fonts = fonts();
    let text = "Hello שלום";
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: "Noto Serif".to_string(),
            size: 12 * units::POINT,
            weight: 400,
            italic: true,
            language: Language::und(),
        },
    }];
    let layout = text::layout(
        fonts,
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: 300 * units::POINT,
        },
    );
    let runs: Vec<(&str, &str, bool)> = layout.lines[0]
        .runs
        .iter()
        .map(|r| {
            (
                &text[r.range.clone()],
                fonts.face(r.face).family(),
                r.oblique,
            )
        })
        .collect();
    // Noto Serif has an italic, so Latin needs no slant; Noto Serif Hebrew
    // has none, so the Hebrew is slanted. The space after "Hello" runs the
    // Latin way, in its face.
    assert_eq!(
        runs,
        [
            ("Hello ", "Noto Serif", false),
            ("שלום", "Noto Serif Hebrew", true),
        ]
    );
}

#[test]
fn characters_no_face_has_are_reported() {
    // Noto Serif alone: it has Latin, and no Chinese.
    let mut fonts = Fonts::new();
    let (_, bytes) = testkit::fonts::load()
        .expect("the bundled fonts")
        .into_iter()
        .find(|(name, _)| name == "NotoSerif-Regular.ttf")
        .expect("Noto Serif");
    fonts.add(bytes).expect("a font");
    let lay_out = |fonts: &Fonts, text: &str| {
        let spans = [Span {
            len: text.len(),
            style: Style {
                family: "Noto Serif".to_string(),
                size: 12 * units::POINT,
                weight: 400,
                italic: false,
                language: Language::und(),
            },
        }];
        text::layout(
            fonts,
            &Paragraph {
                text,
                spans: &spans,
                direction: None,
                align: Align::Start,
                width: 300 * units::POINT,
            },
        )
    };
    assert!(!lay_out(&fonts, "Hello, world").missing);
    assert!(lay_out(&fonts, "Hello, 世界").missing);
    // With every bundled face, Chinese has one.
    assert!(!lay_out(common::fonts(), "Hello, 世界").missing);
    // A blank paragraph lacks nothing.
    assert!(!lay_out(&fonts, "").missing);
}
