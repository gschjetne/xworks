//! A laid-out paragraph as display-list items: every character is shown by
//! some glyph's text, each glyph sits on its line, and each span keeps its
//! colour.

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
    let items = layout.paint(fonts(), origin, |span| colors[span]);

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
        // Spans keep their colours.
        let span = if start < split { 0 } else { 1 };
        assert_eq!(run.color, colors[span]);
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
