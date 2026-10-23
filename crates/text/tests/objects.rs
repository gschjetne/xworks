//! Inline objects (docs/ROADMAP.md, M2 stage 7): pictures set in a
//! paragraph's text as characters are, each standing for its character,
//! U+FFFC, as a box on the baseline. Lines break around them, the caret
//! stands either side, lines grow to hold them, and no font draws them.

mod common;

use common::{fonts, points};
use text::units::INCH;
use text::{Affinity, Align, Cursor, Inline, Paragraph, Setting, Span, Style};

const OBJECT: char = '\u{fffc}';

fn style(family: &str, language: &str) -> Style {
    Style {
        family: family.to_string(),
        size: points(12.0),
        weight: 400,
        italic: false,
        language: text::Language::parse(language).expect("a tag"),
        ligatures: text::Ligatures::default(),
    }
}

/// `text` in one style, `width` wide, aligned as `align`, with `objects`.
fn lay_out(text: &str, width: i64, align: Align, objects: &[Inline]) -> text::Layout {
    let spans = [Span {
        len: text.len(),
        style: style("Source Serif 4", "en"),
    }];
    text::layout_with(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align,
            width,
        },
        &Setting {
            objects,
            ..Setting::default()
        },
    )
}

fn at(text: &str, c: char) -> usize {
    text.find(c).expect("the character")
}

#[test]
fn an_object_takes_its_room_in_the_line_and_lifts_it() {
    let text = format!("Before {OBJECT} after.");
    let offset = at(&text, OBJECT);
    let object = Inline {
        offset,
        width: 2 * INCH,
        height: INCH,
    };
    let plain = lay_out(&text, 8 * INCH, Align::Start, &[]);
    let laid = lay_out(&text, 8 * INCH, Align::Start, &[object]);
    assert_eq!(laid.lines.len(), 1);
    let line = &laid.lines[0];
    assert_eq!(line.objects.len(), 1);
    let placed = line.objects[0];
    assert_eq!(placed.offset, offset);
    assert_eq!((placed.width, placed.height), (2 * INCH, INCH));
    // It starts where the text before it ends, and the text after it
    // starts where it ends.
    let before_end = line
        .runs
        .iter()
        .flat_map(|r| &r.glyphs)
        .filter(|g| g.cluster < offset)
        .map(|g| g.x + g.advance)
        .max()
        .expect("text before");
    assert_eq!(placed.x, before_end);
    let after_start = line
        .runs
        .iter()
        .flat_map(|r| &r.glyphs)
        .filter(|g| g.cluster > offset)
        .map(|g| g.x)
        .min()
        .expect("text after");
    assert_eq!(after_start, placed.x + placed.width);
    // The line stands as tall as the object above its baseline, and no
    // glyph draws it.
    assert!(line.ascent >= INCH && line.ascent > plain.lines[0].ascent);
    assert_eq!(line.descent, plain.lines[0].descent);
    assert!(
        line.runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .all(|g| g.cluster != offset)
    );
    // Fonts needn't have its character.
    assert!(!laid.missing);
}

#[test]
fn the_caret_stands_either_side_of_an_object() {
    let text = format!("ab{OBJECT}cd");
    let offset = at(&text, OBJECT);
    let laid = lay_out(
        &text,
        8 * INCH,
        Align::Start,
        &[Inline {
            offset,
            width: INCH,
            height: INCH / 2,
        }],
    );
    let placed = laid.lines[0].objects[0];
    let before = Cursor {
        offset,
        affinity: Affinity::Downstream,
    };
    let after = laid.next(before).expect("a place after it");
    assert_eq!(after.offset, offset + OBJECT.len_utf8());
    assert_eq!(laid.caret(before).x, placed.x);
    assert_eq!(laid.caret(after).x, placed.x + placed.width);
    // A click on its right half lands after it.
    let hit = laid.hit(placed.x + placed.width * 3 / 4, laid.lines[0].baseline - 10);
    assert_eq!(hit.offset, after.offset);
    // Selected, it's highlighted whole.
    let rects = laid.selection(offset..after.offset);
    assert_eq!(rects.len(), 1);
    assert_eq!((rects[0].x, rects[0].width), (placed.x, placed.width));
}

#[test]
fn lines_break_around_objects() {
    // Three objects each two inches wide, in a paragraph three inches
    // wide: a line each, and one wider than the paragraph gets a line
    // of its own all the same.
    let text = format!("{OBJECT} {OBJECT} {OBJECT}");
    let offsets: Vec<usize> = text.match_indices(OBJECT).map(|(i, _)| i).collect();
    let objects: Vec<Inline> = offsets
        .iter()
        .enumerate()
        .map(|(k, &offset)| Inline {
            offset,
            width: if k == 2 { 4 * INCH } else { 2 * INCH },
            height: INCH,
        })
        .collect();
    for align in [Align::Start, Align::Justify] {
        let laid = lay_out(&text, 3 * INCH, align, &objects);
        let placed: Vec<Vec<usize>> = laid
            .lines
            .iter()
            .map(|l| l.objects.iter().map(|o| o.offset).collect())
            .collect();
        assert_eq!(
            placed,
            offsets.iter().map(|&o| vec![o]).collect::<Vec<_>>(),
            "{align:?}"
        );
        for line in &laid.lines {
            assert!(line.height > INCH);
        }
    }
}

#[test]
fn objects_draw_nothing_as_text() {
    let text = format!("A picture {OBJECT} in a line.");
    let offset = at(&text, OBJECT);
    let laid = lay_out(
        &text,
        8 * INCH,
        Align::Start,
        &[Inline {
            offset,
            width: INCH,
            height: INCH,
        }],
    );
    let items = laid.paint(fonts(), scene::Point::new(0, 0), Some(0), |_| {
        scene::Color::BLACK
    });
    let drawn: String = items
        .iter()
        .filter_map(|item| match item {
            scene::Item::Glyphs(g) => Some(g.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(drawn, "A picture  in a line.");
}

#[test]
fn an_object_sits_in_right_to_left_text_in_its_order() {
    let text = format!("שלום {OBJECT} עולם");
    let offset = at(&text, OBJECT);
    let spans = [Span {
        len: text.len(),
        style: style("Noto Serif Hebrew", "he"),
    }];
    let laid = text::layout_with(
        fonts(),
        &Paragraph {
            text: &text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: 8 * INCH,
        },
        &Setting {
            objects: &[Inline {
                offset,
                width: INCH,
                height: INCH,
            }],
            ..Setting::default()
        },
    );
    let line = &laid.lines[0];
    let placed = line.objects[0];
    // The first word is right of it, the second left.
    let x_of = |from: usize, to: usize| {
        line.runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .filter(|g| (from..to).contains(&g.cluster))
            .map(|g| g.x)
            .collect::<Vec<_>>()
    };
    assert!(
        x_of(0, offset)
            .iter()
            .all(|&x| x >= placed.x + placed.width)
    );
    assert!(
        x_of(offset + OBJECT.len_utf8(), text.len())
            .iter()
            .all(|&x| x < placed.x)
    );
}

#[test]
fn objects_that_dont_stand_for_characters_are_ignored() {
    let text = "é and more";
    let bad = [
        // Inside a character, past the end, and twice in one place.
        Inline {
            offset: 1,
            width: INCH,
            height: INCH,
        },
        Inline {
            offset: 100,
            width: INCH,
            height: INCH,
        },
        Inline {
            offset: 3,
            width: -5,
            height: -5,
        },
        Inline {
            offset: 3,
            width: INCH,
            height: INCH,
        },
    ];
    let laid = lay_out(text, 8 * INCH, Align::Start, &bad);
    let placed: Vec<_> = laid.lines.iter().flat_map(|l| &l.objects).collect();
    // The space at 3 stands for an object, as wide as the first one given
    // there says: none.
    assert_eq!(placed.len(), 1);
    assert_eq!((placed[0].offset, placed[0].width), (3, 0));
}
