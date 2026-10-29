//! Tab stops are measured from the paragraph's start edge: a first-line
//! indent moves where the first line's text starts, not where its stops
//! are, so text after a tab stands in the same place on every line, as a
//! ruler shows it. Found writing M2's exit document, whose costs, after a
//! first-line indent, stood an indent to the right of the next line's.

mod common;

use common::fonts;
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

fn lay_out(text: &str, indent: i64, tabs: &Tabs) -> Layout {
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
            tabs: Some(tabs),
            indent,
            ..Setting::default()
        },
    )
}

/// Where the text after the tab on line `k` starts.
fn after_tab(layout: &Layout, text: &str, k: usize) -> i64 {
    let line = &layout.lines[k];
    let tab = text[line.range.clone()].find('\t').expect("a tab") + line.range.start;
    line.runs
        .iter()
        .flat_map(|r| &r.glyphs)
        .filter(|g| g.cluster > tab)
        .map(|g| g.x)
        .min()
        .expect("text after the tab")
}

#[test]
fn a_first_line_indent_leaves_the_stops_where_they_are() {
    let text = "One\tfirst\u{2028}Two\tsecond";
    let stop = Tabs {
        stops: vec![TabStop {
            position: 200 * POINT,
            align: TabAlign::Start,
            leader: None,
        }],
        interval: 0,
    };
    let every = Tabs {
        stops: Vec::new(),
        interval: 72 * POINT,
    };
    for tabs in [&stop, &every] {
        let flat = lay_out(text, 0, tabs);
        assert_eq!(flat.lines.len(), 2);
        let at = after_tab(&flat, text, 0);
        assert_eq!(after_tab(&flat, text, 1), at);
        // Indented, or hanging: the first line starts elsewhere, and its
        // tab still reaches the same stop.
        for indent in [36 * POINT, -18 * POINT] {
            let layout = lay_out(text, indent, tabs);
            assert_eq!(after_tab(&layout, text, 0), at, "indent {indent}");
            assert_eq!(after_tab(&layout, text, 1), at, "indent {indent}");
        }
    }
}
