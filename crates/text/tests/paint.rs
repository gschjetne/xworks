//! A laid-out paragraph as display-list items: every character is shown by
//! some glyph's text, each glyph sits on its line, each span keeps its
//! colour, and a hyphen layout adds reads as a soft hyphen.

mod common;

use common::{fonts, points};
use scene::{Color, Item, Point};
use text::{Align, Language, Paragraph, Span, Style};

fn style(bold: bool, language: &str) -> Style {
    Style {
        family: "Noto Serif".to_string(),
        size: points(12.0),
        weight: if bold { 700 } else { 400 },
        italic: false,
        language: Language::parse(language).expect("a tag"),
        ligatures: text::Ligatures::default(),
    }
}

#[test]
fn glyphs_carry_the_text_they_show() {
    let text = "The word שלום, مرحبا and 日本語.\u{2028}Second line ﬁne.";
    let split = text.find("مرحبا").expect("Arabic");
    let spans = [
        Span {
            len: split,
            style: style(false, "en"),
        },
        Span {
            len: text.len() - split,
            style: style(true, "ar"),
        },
    ];
    let layout = text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: points(120.0),
        },
    );
    let origin = Point::new(1000, 2000);
    let colors = [Color::BLACK, Color::rgb(200, 0, 0)];
    let items = layout.paint(fonts(), origin, Some(7), |span| colors[span]);

    // Every character but the line separator is in some glyph's text, and
    // runs together hold the text in order.
    let mut shown = vec![false; text.len()];
    let mut runs: Vec<(usize, &str)> = Vec::new();
    for item in &items {
        let Item::Glyphs(run) = item else {
            panic!("only glyphs");
        };
        let start = text
            .match_indices(run.text.as_str())
            .map(|(i, _)| i)
            .find(|&i| {
                layout
                    .lines
                    .iter()
                    .flat_map(|l| &l.runs)
                    .any(|r| r.range.start == i)
            })
            .expect("a run's text is the paragraph's");
        runs.push((start, &run.text));
        for glyph in &run.glyphs {
            assert!(
                glyph.text.start < glyph.text.end || run.text.is_empty(),
                "{glyph:?}"
            );
            for b in glyph.text.clone() {
                shown[start + b] = true;
            }
        }
        // Spans keep their colours and languages, and runs say where
        // their text starts in the block.
        let span = if start < split { 0 } else { 1 };
        assert_eq!(run.color, colors[span]);
        assert_eq!(run.language.as_deref(), Some(["en", "ar"][span]));
        assert_eq!(
            run.source,
            Some(scene::Source {
                block: 7,
                offset: start
            })
        );
    }
    runs.sort();
    let joined: String = runs.iter().map(|(_, t)| *t).collect();
    assert_eq!(joined, text);
    for (i, c) in text.char_indices() {
        assert_eq!(shown[i], c != '\u{2028}', "{c:?} at {i}");
    }

    // Glyphs sit on their lines' baselines, offset by the origin.
    let baselines: Vec<i64> = layout.lines.iter().map(|l| origin.y + l.baseline).collect();
    for item in &items {
        let Item::Glyphs(run) = item else { continue };
        for glyph in &run.glyphs {
            assert!(
                baselines
                    .iter()
                    .any(|&b| (glyph.at.y - b).abs() < points(6.0)),
                "{glyph:?} is off every baseline {baselines:?}"
            );
            assert!(glyph.at.x >= origin.x - points(1.0));
        }
    }
}

#[test]
fn some_lines_paint_from_their_own_top() {
    // For a paragraph that breaks across pages: its later lines drawn
    // from the top of the next page.
    let text = "One two three four five six seven eight nine ten eleven twelve.";
    let spans = [Span {
        len: text.len(),
        style: style(false, "en"),
    }];
    let layout = text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: points(60.0),
        },
    );
    assert!(layout.lines.len() >= 3);
    let origin = Point::new(0, 1_000_000);
    let items = layout.paint_lines(fonts(), 1..3, origin, None, |_| Color::BLACK);
    let want: Vec<i64> = layout.lines[1..3]
        .iter()
        .map(|l| origin.y + l.baseline - layout.lines[1].top)
        .collect();
    let mut got: Vec<i64> = items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs(run) => Some(run.baseline),
            _ => None,
        })
        .collect();
    got.dedup();
    assert_eq!(got, want);
    assert!(
        layout
            .paint_lines(fonts(), 9..9, origin, None, |_| Color::BLACK)
            .is_empty()
    );
}

/// A hyphen layout adds where it breaks a word is drawn, but it's no
/// character of the text: the run it ends reads it as a soft hyphen, as
/// PDF asks, and the text the runs read is the paragraph's.
#[test]
fn an_added_hyphen_reads_as_a_soft_hyphen() {
    let text = "Notwithstanding extraordinary circumstances, characteristically \
                incomprehensible responsibilities multiplied.";
    let spans = [Span {
        len: text.len(),
        style: style(false, "en-US"),
    }];
    let layout = text::layout_with(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Justify,
            width: points(90.0),
        },
        &text::Setting {
            hyphenate: true,
            hyphenation: Some(common::hyphenation()),
            ..text::Setting::default()
        },
    );
    let hyphenated = layout.lines.iter().filter(|line| line.hyphen).count();
    assert!(hyphenated > 0, "some word is hyphenated");
    let items = layout.paint(fonts(), Point::new(0, 0), None, |_| Color::BLACK);
    let mut read = String::new();
    let mut soft = 0;
    for item in &items {
        if let Item::Glyphs(run) = item {
            soft += usize::from(run.text.ends_with('\u{ad}'));
            read.push_str(run.text.trim_end_matches('\u{ad}'));
        }
    }
    assert_eq!(soft, hyphenated, "each added hyphen reads as a soft hyphen");
    assert!(!read.contains(['-', '\u{2010}']), "{read}");
    let squeeze = |s: &str| s.split_whitespace().collect::<String>();
    assert_eq!(squeeze(&read), squeeze(text));
}
