//! What holds of every layout, over random paragraphs of mixed scripts and
//! directions, widths and alignments, justified, hyphenated and tabbed.

mod common;

use common::{fonts, hyphenation, points};
use proptest::prelude::*;
use text::{
    Affinity, Align, Cursor, Direction, Layout, Paragraph, Setting, Span, Style, TabAlign, TabStop,
    Tabs,
};

/// Words in the M1 scripts, with what goes between them.
const WORDS: &[&str] = &[
    "the",
    "quick",
    "fox",
    "Antidisestablishment",
    "hyphenation",
    "soft\u{ad}hyphen",
    "lime-tree",
    "ΓΕΙΆ",
    "κόσμε",
    "Привет",
    "мир",
    "שלום",
    "עולם",
    "مرحبا",
    "بالعالم",
    "日本語の",
    "テキスト",
    "改行",
    "2026",
    "e\u{301}",
    "ﬁne",
    "«",
    "»",
    ",",
    ".",
    "(",
    ")",
];
const GAPS: &[&str] = &[" ", " ", " ", "  ", "\u{2028}", "", "\u{a0}", "\t"];

fn paragraph() -> impl Strategy<Value = (String, f64, Option<Direction>, Align, bool)> {
    let words = prop::collection::vec((0..WORDS.len(), 0..GAPS.len()), 0..24);
    let direction = prop_oneof![
        Just(None),
        Just(Some(Direction::Ltr)),
        Just(Some(Direction::Rtl))
    ];
    let align = prop_oneof![
        Just(Align::Start),
        Just(Align::Center),
        Just(Align::End),
        Just(Align::Justify)
    ];
    (words, 8.0..300.0f64, direction, align, any::<bool>()).prop_map(
        |(words, width, direction, align, bold)| {
            let text: String = words
                .into_iter()
                .map(|(w, g)| format!("{}{}", WORDS[w], GAPS[g]))
                .collect();
            (text, width, direction, align, bold)
        },
    )
}

fn lay_out(
    text: &str,
    width: f64,
    direction: Option<Direction>,
    align: Align,
    bold: bool,
) -> Layout {
    // Bold for the first half, by characters, so spans change mid-text.
    let half = text
        .char_indices()
        .nth(text.chars().count() / 2)
        .map_or(text.len(), |(i, _)| i);
    let style = |bold: bool| Style {
        family: "Noto Serif".to_string(),
        size: points(12.0),
        weight: if bold { 700 } else { 400 },
        italic: false,
        // English, so its words hyphenate where they're bold.
        language: if bold {
            text::Language::parse("en").expect("a tag")
        } else {
            text::Language::und()
        },
    };
    let spans = [
        Span {
            len: half,
            style: style(bold),
        },
        Span {
            len: text.len() - half,
            style: style(false),
        },
    ];
    let tabs = Tabs {
        stops: vec![
            TabStop {
                position: points(40.0),
                align: TabAlign::End,
            },
            TabStop {
                position: points(90.0),
                align: TabAlign::Decimal('.'),
            },
        ],
        interval: points(30.0),
    };
    text::layout_with(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction,
            align,
            width: points(width),
        },
        &Setting {
            hyphenate: bold,
            hyphenation: Some(hyphenation()),
            tabs: Some(&tabs),
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn lines_cover_the_text_and_fit(
        (text, width, direction, align, bold) in paragraph()
    ) {
        let layout = lay_out(&text, width, direction, align, bold);
        let mut at = 0;
        let mut top = 0;
        for line in &layout.lines {
            prop_assert_eq!(line.range.start, at);
            prop_assert_eq!(line.top, top);
            let graphemes = locale::graphemes(&text[line.range.clone()]).len() - 1;
            prop_assert!(
                line.width <= points(width) || graphemes <= 1,
                "a line {} wide in {}: {:?}", line.width, points(width), &text[line.range.clone()]
            );
            let mut x = line.runs.first().map_or(0, |r| r.x);
            for run in &line.runs {
                prop_assert_eq!(run.x, x);
                x += run.width;
            }
            at = line.range.end;
            top += line.height;
        }
        prop_assert_eq!(at, text.len());
    }

    #[test]
    fn carets_are_found_where_they_are_drawn(
        (text, width, direction, align, bold) in paragraph()
    ) {
        let layout = lay_out(&text, width, direction, align, bold);
        let mut cursor = Some(Cursor { offset: 0, affinity: Affinity::Downstream });
        let mut steps = 0;
        while let Some(c) = cursor {
            for c in [c, Cursor { affinity: Affinity::Upstream, ..c }] {
                let caret = layout.caret(c);
                let hit = layout.caret(layout.hit(caret.x, (caret.top + caret.bottom) / 2));
                prop_assert_eq!((hit.x, hit.line), (caret.x, caret.line), "at {:?}", c);
            }
            cursor = layout.next(c);
            steps += 1;
            prop_assert!(steps <= text.len() + 1);
        }
    }

    #[test]
    fn moving_right_then_left_on_a_line_returns(
        (text, width, direction, align, bold) in paragraph()
    ) {
        let layout = lay_out(&text, width, direction, align, bold);
        let mut cursor = Some(Cursor { offset: 0, affinity: Affinity::Downstream });
        while let Some(c) = cursor {
            for c in [c, Cursor { affinity: Affinity::Upstream, ..c }] {
                let caret = layout.caret(c);
                if let Some(r) = layout.right(c)
                    && layout.caret(r).line == caret.line
                {
                    let back = layout.caret(layout.left(r).expect("left of right"));
                    prop_assert_eq!((back.x, back.line), (caret.x, caret.line), "at {:?}", c);
                    prop_assert!(layout.caret(r).x >= caret.x);
                }
            }
            cursor = layout.next(c);
        }
    }

    #[test]
    fn walking_right_crosses_every_line(
        (text, width, direction, align, bold) in paragraph()
    ) {
        // From the end of the text that's leftmost on screen, moving right
        // reaches the other end, through every line, and stops.
        let layout = lay_out(&text, width, direction, align, bold);
        let rtl = layout.direction() == Direction::Rtl;
        let last = layout.lines.len() - 1;
        let start_line = if rtl { last } else { 0 };
        let mut at = layout.hit_line(start_line, i64::MIN / 4);
        let mut lines = vec![layout.caret(at).line];
        let mut steps = 0;
        while let Some(next) = layout.right(at) {
            at = next;
            let line = layout.caret(at).line;
            if lines.last() != Some(&line) {
                lines.push(line);
            }
            steps += 1;
            prop_assert!(steps <= 2 * text.len() + 2 * layout.lines.len());
        }
        let want: Vec<usize> = if rtl { (0..=last).rev().collect() } else { (0..=last).collect() };
        prop_assert_eq!(lines, want);
    }
}

/// Found by `lines_cover_the_text_and_fit`: a Hebrew word broken between
/// letters that kern together came out wider than its line, because lines
/// were measured as the paragraph was shaped whole, not as the broken word
/// is shaped.
#[test]
fn a_word_broken_where_it_kerns_still_fits() {
    let layout = lay_out("עולם ", 9.44200518964192, None, Align::Start, true);
    for line in &layout.lines {
        assert!(line.width <= points(9.44200518964192) || line.range.len() <= 2);
    }
}
