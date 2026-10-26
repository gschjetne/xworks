//! Tab leaders (review sjj5ak5a): a stop with a leader fills the room a
//! tab to it opens with its character, repeated, as a table of contents
//! has dots from a heading to its page's number. They sit on a grid from
//! the paragraph's start, so leaders on lines one under another line up;
//! they keep clear of the text either side; and they're drawn with the
//! text, in its face and colour, but aren't part of it.

mod common;

use common::fonts;
use scene::Item;
use text::units::POINT;
use text::{Align, Layout, Paragraph, Setting, Span, Style, TabAlign, TabStop, Tabs};

fn style() -> Style {
    Style {
        family: "Noto Serif".to_string(),
        size: 12 * POINT,
        weight: 400,
        italic: false,
        language: text::Language::und(),
        ligatures: text::Ligatures::default(),
    }
}

fn lay_out(text: &str, leader: Option<char>) -> Layout {
    let tabs = Tabs {
        stops: vec![TabStop {
            position: 300 * POINT,
            align: TabAlign::End,
            leader,
        }],
        interval: 0,
    };
    text::layout_with(
        fonts(),
        &Paragraph {
            text,
            spans: &[Span {
                len: text.len(),
                style: style(),
            }],
            direction: None,
            align: Align::Start,
            width: 400 * POINT,
        },
        &Setting {
            tabs: Some(&tabs),
            ..Setting::default()
        },
    )
}

/// Where the text before and after the first line's tab, in `text`, ends
/// and starts.
fn gap(layout: &Layout, text: &str) -> (i64, i64) {
    let line = &layout.lines[0];
    let tab = text.find('\t').expect("a tab");
    let glyphs: Vec<_> = line.runs.iter().flat_map(|r| &r.glyphs).collect();
    let before = glyphs
        .iter()
        .filter(|g| g.cluster < tab)
        .map(|g| g.x + g.advance)
        .max()
        .unwrap_or(0);
    let after = glyphs
        .iter()
        .filter(|g| g.cluster > tab)
        .map(|g| g.x)
        .min()
        .unwrap_or(0);
    (before, after)
}

#[test]
fn a_leader_fills_the_room_a_tab_opens() {
    let text = "Introduction\t12";
    let layout = lay_out(text, Some('.'));
    let line = &layout.lines[0];
    assert_eq!(line.leaders.len(), 1, "one tab, one leader");
    let leader = &line.leaders[0];
    let (before, after) = gap(&layout, text);
    let dots: Vec<i64> = leader.glyphs.iter().map(|g| g.x).collect();
    assert!(dots.len() > 20, "{} dots", dots.len());
    // Clear of the text either side, at an even pitch.
    assert!(dots[0] > before, "{} after {before}", dots[0]);
    let last = leader.glyphs.last().unwrap();
    assert!(last.x + last.advance < after, "{} before {after}", last.x);
    let pitch = dots[1] - dots[0];
    assert!(dots.windows(2).all(|w| w[1] - w[0] == pitch));
    // On a grid from the paragraph's start, so they line up across lines.
    assert_eq!(dots[0] % pitch, 0);
    let other = lay_out("Appendix A\t3", Some('.'));
    let first = other.lines[0].leaders[0].glyphs[0].x;
    assert_eq!((first - dots[0]) % pitch, 0);
    // The text's caret positions and width are as without a leader.
    let plain = lay_out("Introduction\t12", None);
    assert!(plain.lines[0].leaders.is_empty());
    assert_eq!(plain.lines[0].width, line.width);
    // Drawn with the text, a run of glyphs more, in its font.
    let drawn = layout.paint(fonts(), scene::Point::new(0, 0), Some(0), |_| {
        scene::Color::BLACK
    });
    let runs = |items: &[Item]| {
        items
            .iter()
            .filter(|i| matches!(i, Item::Glyphs(_)))
            .count()
    };
    let without = plain.paint(fonts(), scene::Point::new(0, 0), Some(0), |_| {
        scene::Color::BLACK
    });
    assert_eq!(runs(&drawn), runs(&without) + 1);
    let Some(Item::Glyphs(dots)) = drawn.iter().rfind(|i| matches!(i, Item::Glyphs(_))) else {
        panic!("the leader drawn");
    };
    assert!(dots.text.is_empty() && dots.source.is_none(), "not text");
}
