//! The layout cases in `spec/text/layout/` (spec/text/README.md): line
//! breaks, visual order, fonts, positions and caret movement, and what
//! holds of every layout.

mod common;

use common::{StyleSpec, byte_offset, char_offset, fonts, parse, points};
use serde::Deserialize;
use text::{Affinity, Align, Cursor, Direction, Layout, Paragraph, Span};

#[derive(Debug, Deserialize)]
struct Case {
    text: String,
    style: StyleSpec,
    #[serde(default)]
    spans: Vec<SpanSpec>,
    width: f64,
    #[serde(default)]
    direction: DirectionSpec,
    #[serde(default)]
    align: AlignSpec,
    #[serde(default)]
    paragraph: Dir,
    fonts: Option<Vec<(String, String)>>,
    lines: Vec<LineSpec>,
    #[serde(default)]
    carets: Vec<Walk>,
    #[serde(default)]
    selections: Vec<Selection>,
}

#[derive(Debug, Deserialize)]
struct Selection {
    from: usize,
    to: usize,
    rects: usize,
}

#[derive(Debug, Default, Deserialize)]
struct SpanSpec {
    chars: Option<usize>,
    #[serde(flatten)]
    style: StyleSpec,
}

#[derive(Debug, Default, Deserialize)]
enum DirectionSpec {
    #[default]
    Auto,
    Ltr,
    Rtl,
}

#[derive(Debug, Default, Deserialize)]
enum AlignSpec {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
enum Dir {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
enum Fragment {
    Ltr(String),
    Rtl(String),
}

#[derive(Debug, Deserialize)]
struct LineSpec {
    text: String,
    visual: Option<Vec<Fragment>>,
    width: Option<i64>,
    baseline: Option<i64>,
    height: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Walk {
    from: String,
    by: Move,
    visits: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum Move {
    Right,
    Left,
    Next,
    Previous,
}

#[test]
fn layout_cases() {
    let cases = testkit::specs("text/layout").expect("spec/text/layout");
    assert!(!cases.is_empty());
    let mut failures = Vec::new();
    for (name, source) in &cases {
        let case: Case = parse(source).unwrap_or_else(|e| panic!("{name}.ron: {e}"));
        for problem in check(&case) {
            failures.push(format!("{name}: {problem}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn lay_out(case: &Case) -> Layout {
    let text = &case.text;
    let mut spans = Vec::new();
    let mut at = 0;
    for span in &case.spans {
        let end = match span.chars {
            Some(n) => byte_offset(text, char_offset(text, at) + n),
            None => text.len(),
        };
        spans.push(Span {
            len: end - at,
            style: span.style.over(&case.style).style(),
        });
        at = end;
    }
    if spans.is_empty() {
        spans.push(Span {
            len: text.len(),
            style: case.style.style(),
        });
    } else if at < text.len() {
        spans.last_mut().expect("a span").len += text.len() - at;
    }
    text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: match case.direction {
                DirectionSpec::Auto => None,
                DirectionSpec::Ltr => Some(Direction::Ltr),
                DirectionSpec::Rtl => Some(Direction::Rtl),
            },
            align: match case.align {
                AlignSpec::Start => Align::Start,
                AlignSpec::Center => Align::Center,
                AlignSpec::End => Align::End,
            },
            width: points(case.width),
        },
    )
}

/// Everything wrong with `case`'s layout.
fn check(case: &Case) -> Vec<String> {
    let layout = lay_out(case);
    let text = &case.text;
    let mut problems = Vec::new();

    let rtl = layout.direction() == Direction::Rtl;
    if rtl != (case.paragraph == Dir::Rtl) {
        problems.push(format!("the paragraph is {:?}", layout.direction()));
    }

    let got: Vec<&str> = layout
        .lines
        .iter()
        .map(|l| &text[l.range.clone()])
        .collect();
    let want: Vec<&str> = case.lines.iter().map(|l| l.text.as_str()).collect();
    if got != want {
        problems.push(format!("lines {got:?}, expected {want:?}"));
        return problems;
    }

    for (i, (line, spec)) in layout.lines.iter().zip(&case.lines).enumerate() {
        if let Some(visual) = &spec.visual {
            let got = fragments(text, line);
            if &got != visual {
                problems.push(format!(
                    "line {i} reads {got:?} on screen, expected {visual:?}"
                ));
            }
        }
        for (what, got, want) in [
            ("width", line.width, spec.width),
            ("baseline", line.baseline, spec.baseline),
            ("height", line.height, spec.height),
        ] {
            if let Some(want) = want
                && got != want
            {
                problems.push(format!("line {i}'s {what} is {got}, expected {want}"));
            }
        }
    }

    if let Some(want) = &case.fonts {
        let got = families(text, &layout);
        if &got != want {
            problems.push(format!("fonts {got:?}, expected {want:?}"));
        }
    }

    problems.extend(invariants(case, &layout));

    for walk in &case.carets {
        let from = parse_cursor(text, &walk.from);
        let mut at = from;
        let mut visited = Vec::new();
        while let Some(next) = step(&layout, at, walk.by) {
            visited.push(show_cursor(text, next));
            at = next;
            if visited.len() > 4 * text.len() + 4 {
                break;
            }
        }
        if visited != walk.visits {
            problems.push(format!(
                "moving {:?} from {} visits {visited:?}, expected {:?}",
                walk.by, walk.from, walk.visits
            ));
        }
    }
    for selection in &case.selections {
        let range = byte_offset(text, selection.from)..byte_offset(text, selection.to);
        let rects = layout.selection(range);
        let width: i64 = rects.iter().map(|r| r.width).sum();
        if rects.len() != selection.rects || width <= 0 {
            problems.push(format!(
                "selecting {} to {} highlights {rects:?}, expected {} stretches",
                selection.from, selection.to, selection.rects
            ));
        }
    }
    problems
}

fn step(layout: &Layout, at: Cursor, by: Move) -> Option<Cursor> {
    match by {
        Move::Right => layout.right(at),
        Move::Left => layout.left(at),
        Move::Next => layout.next(at),
        Move::Previous => layout.previous(at),
    }
}

/// The line from left to right, in stretches of one direction.
fn fragments(text: &str, line: &text::Line) -> Vec<Fragment> {
    let mut out: Vec<(bool, Vec<std::ops::Range<usize>>)> = Vec::new();
    for run in &line.runs {
        let rtl = run.level % 2 == 1;
        match out.last_mut() {
            Some((r, ranges)) if *r == rtl => ranges.push(run.range.clone()),
            _ => out.push((rtl, vec![run.range.clone()])),
        }
    }
    out.into_iter()
        .map(|(rtl, mut ranges)| {
            ranges.sort_by_key(|r| r.start);
            let s: String = ranges.iter().map(|r| &text[r.clone()]).collect();
            if rtl {
                Fragment::Rtl(s)
            } else {
                Fragment::Ltr(s)
            }
        })
        .collect()
}

/// The family each stretch of the text comes from, in logical order.
fn families(text: &str, layout: &Layout) -> Vec<(String, String)> {
    let mut runs: Vec<(usize, usize, String)> = layout
        .lines
        .iter()
        .flat_map(|l| &l.runs)
        .map(|r| {
            (
                r.range.start,
                r.range.end,
                fonts().face(r.face).family().to_string(),
            )
        })
        .collect();
    runs.sort();
    let mut out: Vec<(String, String)> = Vec::new();
    for (start, end, family) in runs {
        match out.last_mut() {
            Some((s, f)) if *f == family => s.push_str(&text[start..end]),
            _ => out.push((text[start..end].to_string(), family)),
        }
    }
    out
}

/// What holds of every layout.
fn invariants(case: &Case, layout: &Layout) -> Vec<String> {
    let text = &case.text;
    let width = points(case.width);
    let mut problems = Vec::new();

    // Lines cover the text, in order, one after another down the page.
    let mut at = 0;
    let mut top = 0;
    for (i, line) in layout.lines.iter().enumerate() {
        if line.range.start != at {
            problems.push(format!("line {i} starts at {}, not {at}", line.range.start));
        }
        if line.top != top
            || line.baseline != top + line.ascent
            || line.height != line.ascent + line.descent
        {
            problems.push(format!("line {i}'s top, baseline and height don't add up"));
        }
        at = line.range.end;
        top += line.height;
    }
    if at != text.len() {
        problems.push(format!("the lines end at {at}, not {}", text.len()));
    }
    if layout.height != top {
        problems.push(format!(
            "the paragraph is {} high, its lines {top}",
            layout.height
        ));
    }

    for (i, line) in layout.lines.iter().enumerate() {
        // Each fits, unless it's one character too wide on its own.
        let graphemes = locale::graphemes(&text[line.range.clone()]).len() - 1;
        if line.width > width && graphemes > 1 {
            problems.push(format!(
                "line {i} is {} wide, more than {width}",
                line.width
            ));
        }
        // Runs sit side by side, and glyphs within them.
        let mut x = line.runs.first().map_or(line.x, |r| r.x);
        for run in &line.runs {
            if run.x != x {
                problems.push(format!("line {i} has a run at {}, not {x}", run.x));
            }
            let advances: i64 = run.glyphs.iter().map(|g| g.advance).sum();
            if advances != run.width {
                problems.push(format!(
                    "line {i} has a run whose glyphs don't add up to its width"
                ));
            }
            x += run.width;
        }
        // Aligned as asked: the line's text, less spaces hanging at its end.
        let rtl = layout.direction() == Direction::Rtl;
        let (left, right) = (line.x, width - (line.x + line.width));
        let aligned = match (&case.align, rtl) {
            (AlignSpec::Start, false) | (AlignSpec::End, true) => left == 0,
            (AlignSpec::Start, true) | (AlignSpec::End, false) => right == 0,
            (AlignSpec::Center, _) => (left - right).abs() <= 1,
        };
        if !aligned {
            problems.push(format!(
                "line {i} isn't aligned {:?}: {left} to its left, {right} to its right",
                case.align
            ));
        }
    }

    // Every place the caret stands, hit-testing finds again, and moving
    // to the next place and back returns.
    let mut cursor = Some(Cursor {
        offset: 0,
        affinity: Affinity::Downstream,
    });
    while let Some(c) = cursor {
        let caret = layout.caret(c);
        let y = (caret.top + caret.bottom) / 2;
        let hit = layout.hit(caret.x, y);
        if layout.caret(hit).x != caret.x || layout.caret(hit).line != caret.line {
            problems.push(format!(
                "hit-testing the caret at {} finds {}",
                show_cursor(text, c),
                show_cursor(text, hit)
            ));
        }
        cursor = layout.next(c);
        if let Some(n) = cursor
            && layout.previous(n).map(|p| p.offset) != Some(c.offset)
        {
            problems.push(format!(
                "moving back from {} doesn't return to {}",
                show_cursor(text, n),
                c.offset
            ));
        }
    }
    problems
}

/// A cursor as specs write it, "3<" or "3>", in characters.
fn parse_cursor(text: &str, s: &str) -> Cursor {
    let (n, affinity) = match s.strip_suffix('<') {
        Some(n) => (n, Affinity::Upstream),
        None => (
            s.strip_suffix('>')
                .unwrap_or_else(|| panic!("{s:?} isn't a cursor")),
            Affinity::Downstream,
        ),
    };
    let n: usize = n.parse().unwrap_or_else(|_| panic!("{s:?} isn't a cursor"));
    Cursor {
        offset: byte_offset(text, n),
        affinity,
    }
}

fn show_cursor(text: &str, c: Cursor) -> String {
    let mark = match c.affinity {
        Affinity::Upstream => '<',
        Affinity::Downstream => '>',
    };
    format!("{}{mark}", char_offset(text, c.offset))
}
