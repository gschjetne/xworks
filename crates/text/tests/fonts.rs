//! Loading the bundled fonts, and choosing faces from them.

mod common;

use common::fonts;
use text::{FontError, Fonts};

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
        "Noto Serif Hebrew",
    ] {
        for weight in [400, 700] {
            want.push((family.to_string(), weight, false));
        }
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
