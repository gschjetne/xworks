//! Underlines and lines through text (docs/ROADMAP.md, M2 stage 4): where
//! each run's font puts them, as long as its glyphs but for spaces hanging
//! at a line's end, and only on the stretches asked for.

mod common;

use common::fonts;
use scene::{Color, Item, Path, Point, Rect, Segment};
use skrifa::MetadataProvider;
use text::units::{INCH, POINT};
use text::{Align, Decoration, Layout, Paragraph, Span, Style};

fn style(family: &str) -> Style {
    Style {
        family: family.to_string(),
        size: 12 * POINT,
        weight: 400,
        italic: false,
        language: text::Language::und(),
    }
}

fn lay_out(text: &str, spans: &[Span], width: i64) -> Layout {
    text::layout(
        fonts(),
        &Paragraph {
            text,
            spans,
            direction: None,
            align: Align::Start,
            width,
        },
    )
}

fn one_style(text: &str, width: i64) -> Layout {
    lay_out(
        text,
        &[Span {
            len: text.len(),
            style: style("Noto Serif"),
        }],
        width,
    )
}

/// The rectangle a fill covers.
fn rect(item: &Item) -> Rect {
    let Item::Fill {
        path: Path(segments),
        ..
    } = item
    else {
        panic!("a fill: {item:?}");
    };
    let points: Vec<Point> = segments
        .iter()
        .filter_map(|s| match s {
            Segment::MoveTo(p) | Segment::LineTo(p) => Some(*p),
            _ => None,
        })
        .collect();
    let (x0, y0) = (points[0].x, points[0].y);
    let (x1, y1) = (points[2].x, points[2].y);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Where the face of `run` puts a line, scaled to the run's size: its top
/// below the baseline and its thickness, from the font's own tables.
fn stroke(layout: &Layout, line: usize, run: usize, underline: bool) -> (i64, i64) {
    let run = &layout.lines[line].runs[run];
    let face = fonts().face(run.face);
    let font = skrifa::FontRef::from_index(face.data(), face.index()).expect("a font");
    let metrics = font.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let d = if underline {
        metrics.underline
    } else {
        metrics.strikeout
    }
    .expect("the bundled fonts say where");
    let units = |v: f32| {
        (f64::from(v.round()) * run.size as f64 / f64::from(face.units_per_em())).round() as i64
    };
    (-units(d.offset), units(d.thickness))
}

const UNDER: Decoration = Decoration {
    underline: true,
    strikethrough: false,
};
const THROUGH: Decoration = Decoration {
    underline: false,
    strikethrough: true,
};

#[test]
fn an_underline_is_where_the_font_puts_it_and_as_long_as_the_text() {
    let layout = one_style("Underlined text", 4 * INCH);
    let origin = Point::new(INCH, INCH);
    let items = layout.decorations(fonts(), 0..1, origin, |_| UNDER, |_| Color::BLACK);
    assert_eq!(items.len(), 1);
    let line = &layout.lines[0];
    let (below, thickness) = stroke(&layout, 0, 0, true);
    assert!(below > 0, "Noto Serif's underline is below the baseline");
    assert_eq!(
        rect(&items[0]),
        Rect::new(
            origin.x + line.x,
            origin.y + line.baseline + below,
            line.width,
            thickness
        )
    );
}

#[test]
fn a_line_through_is_above_the_baseline() {
    let layout = one_style("Struck through", 4 * INCH);
    let items = layout.decorations(
        fonts(),
        0..1,
        Point::new(0, 0),
        |_| THROUGH,
        |_| Color::BLACK,
    );
    assert_eq!(items.len(), 1);
    let line = &layout.lines[0];
    let (below, thickness) = stroke(&layout, 0, 0, false);
    assert!(below < 0, "above the baseline");
    let r = rect(&items[0]);
    assert_eq!((r.y, r.height), (line.baseline + below, thickness));
    // Through the lowercase letters, below the capitals' tops.
    assert!(r.y > line.baseline - 12 * POINT / 2 && r.bottom() < line.baseline);
}

#[test]
fn spaces_hanging_at_a_lines_end_arent_underlined() {
    let layout = one_style("first line, then the second", 3 * INCH / 4);
    assert!(layout.lines.len() >= 2, "it breaks");
    let items = layout.decorations(
        fonts(),
        0..layout.lines.len(),
        Point::new(0, 0),
        |_| UNDER,
        |_| Color::BLACK,
    );
    assert_eq!(items.len(), layout.lines.len(), "a line each");
    for (item, line) in items.iter().zip(&layout.lines) {
        let r = rect(item);
        assert_eq!((r.x, r.width), (line.x, line.width), "{:?}", line.range);
    }
}

#[test]
fn only_the_stretches_asked_for_are_decorated() {
    let text = "plain under plain";
    let spans = [
        Span {
            len: 6,
            style: style("Noto Serif"),
        },
        Span {
            len: 5,
            style: style("Noto Serif"),
        },
        Span {
            len: 6,
            style: style("Noto Serif"),
        },
    ];
    let layout = lay_out(text, &spans, 4 * INCH);
    let accent = Color::rgb(160, 30, 40);
    let items = layout.decorations(
        fonts(),
        0..1,
        Point::new(0, 0),
        |span| if span == 1 { UNDER } else { Decoration::NONE },
        |span| if span == 1 { accent } else { Color::BLACK },
    );
    assert_eq!(items.len(), 1);
    let Item::Fill { color, .. } = &items[0] else {
        panic!("a fill");
    };
    assert_eq!(*color, accent);
    let run = layout.lines[0]
        .runs
        .iter()
        .find(|r| r.span == 1)
        .expect("a run of the middle span");
    let r = rect(&items[0]);
    assert_eq!((r.x, r.right()), (run.x, run.x + run.width));
    // Undecorated text has none.
    let none = layout.decorations(
        fonts(),
        0..1,
        Point::new(0, 0),
        |_| Decoration::NONE,
        |_| Color::BLACK,
    );
    assert!(none.is_empty());
}

#[test]
fn each_font_puts_its_own_lines_in_both_directions() {
    let layout = one_style("Hello שלום עולם", 4 * INCH);
    let line = &layout.lines[0];
    assert!(
        line.runs.len() >= 2,
        "Latin and Hebrew from their own faces"
    );
    let both = Decoration {
        underline: true,
        strikethrough: true,
    };
    let items = layout.decorations(fonts(), 0..1, Point::new(0, 0), |_| both, |_| Color::BLACK);
    // In reading order, an underline and a line through for each run.
    let mut runs: Vec<usize> = (0..line.runs.len()).collect();
    runs.sort_by_key(|&i| line.runs[i].range.start);
    let mut expected = Vec::new();
    for &i in &runs {
        let run = &line.runs[i];
        let ends = run
            .glyphs
            .iter()
            .map(|g| (g.x, g.x + g.advance))
            .reduce(|(l, r), (a, b)| (l.min(a), r.max(b)))
            .expect("glyphs");
        for underline in [true, false] {
            let (below, thickness) = stroke(&layout, 0, i, underline);
            // Spaces at the line's end hang; there are none here.
            expected.push(Rect::new(
                ends.0,
                line.baseline + below,
                ends.1 - ends.0,
                thickness,
            ));
        }
    }
    let got: Vec<Rect> = items.iter().map(rect).collect();
    assert_eq!(got, expected);
}
