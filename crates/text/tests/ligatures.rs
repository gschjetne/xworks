//! Ligatures, class by class (ARCHITECTURE.md section 9; review dcn5ejrd):
//! a style turns each of OpenType's classes on or off, and code turns them
//! all off, so it shows exactly what was typed.

mod common;

use common::fonts;
use scene::{Item, Point};
use text::units::{INCH, POINT};
use text::{Align, Ligatures, Paragraph, Span, Style};

/// The glyphs `text` is drawn with, in `family`, with `ligatures`.
fn glyphs(text: &str, family: &str, ligatures: Ligatures) -> Vec<u32> {
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: family.to_string(),
            size: 12 * POINT,
            weight: 400,
            italic: false,
            language: text::Language::und(),
            ligatures,
        },
    }];
    let layout = text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: 6 * INCH,
        },
    );
    layout
        .paint(fonts(), Point::new(0, 0), None, |_| scene::Color::BLACK)
        .iter()
        .flat_map(|item| match item {
            Item::Glyphs(g) => g.glyphs.iter().map(|glyph| glyph.id).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn standard_ligatures_join_letters_unless_turned_off() {
    let on = glyphs("office", "Source Serif 4", Ligatures::default());
    let off = glyphs(
        "office",
        "Source Serif 4",
        Ligatures {
            standard: false,
            ..Ligatures::default()
        },
    );
    // "ffi" is one glyph with standard ligatures, three without.
    assert_eq!(off.len(), 6);
    assert!(on.len() < off.len(), "{on:?} against {off:?}");
}

#[test]
fn code_shows_what_was_typed() {
    // JetBrains Mono draws "->" as an arrow through its contextual
    // alternates; with no ligatures it's a hyphen and a greater-than sign.
    let on = glyphs("a -> b", "JetBrains Mono", Ligatures::default());
    let none = glyphs("a -> b", "JetBrains Mono", Ligatures::NONE);
    let plain: Vec<u32> = glyphs("-", "JetBrains Mono", Ligatures::NONE)
        .into_iter()
        .chain(glyphs(">", "JetBrains Mono", Ligatures::NONE))
        .collect();
    assert_ne!(on, none);
    assert_eq!(&none[2..4], plain.as_slice());
}

#[test]
fn the_default_is_opentypes() {
    assert_eq!(
        Ligatures::default(),
        Ligatures {
            standard: true,
            contextual: true,
            discretionary: false,
            historical: false,
        }
    );
}
