//! Paragraph layout: lines broken first fit, each ordered for display by
//! the bidi algorithm, aligned, and stacked.

use std::ops::Range;

use locale::{Bidi, Direction, Language};

use crate::font::{FaceId, Fonts};
use crate::shape::{self, Item, Shaped};
use crate::units::scale;

/// How a stretch of text is set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    /// The font family, such as "Noto Serif". Characters it lacks come from
    /// its fallbacks ([`Fonts`]).
    pub family: String,
    /// The size of its em, in layout units: 12 points is 152 400.
    pub size: i64,
    /// Its weight, from 1 to 1000: 400 is regular, 700 bold.
    pub weight: u16,
    /// Whether it's italic.
    pub italic: bool,
    /// Its language, which picks the font's forms for it.
    pub language: Language,
}

/// A stretch of a paragraph's text in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Its length in bytes.
    pub len: usize,
    /// Its style.
    pub style: Style,
}

/// Where lines sit between the paragraph's edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Against the edge the paragraph's direction starts from: the left
    /// in a left-to-right paragraph, the right in a right-to-left one.
    #[default]
    Start,
    /// Centred.
    Center,
    /// Against the other edge.
    End,
}

/// A paragraph to lay out.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    /// Its text. Line separators (U+2028) and other hard breaks end lines.
    pub text: &'a str,
    /// Its styles, in order, covering the text exactly. There is always at
    /// least one, so an empty paragraph has a style too.
    pub spans: &'a [Span],
    /// Its direction; `None` takes it from the text's first strong
    /// character.
    pub direction: Option<Direction>,
    /// How its lines are aligned.
    pub align: Align,
    /// How wide it is, in layout units.
    pub width: i64,
}

/// A paragraph laid out: its lines, top to bottom. Positions are in layout
/// units from the paragraph's top left, with y growing down the page.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Its lines, in order. There is always at least one.
    pub lines: Vec<Line>,
    /// The width it was laid out to.
    pub width: i64,
    /// The height of its lines together.
    pub height: i64,
    pub(crate) direction: Direction,
    /// Its text.
    pub(crate) text: String,
    /// Where its graphemes begin and end, from 0 to the text's length.
    pub(crate) graphemes: Vec<usize>,
    /// The offsets just after hard line breaks.
    pub(crate) hard_breaks: Vec<usize>,
}

impl Layout {
    /// The paragraph's direction.
    pub fn direction(&self) -> Direction {
        self.direction
    }
}

/// A line of a paragraph.
#[derive(Clone, Debug)]
pub struct Line {
    /// Its text, as byte offsets into the paragraph, including the spaces
    /// or hard break that end it.
    pub range: Range<usize>,
    /// Its top.
    pub top: i64,
    /// Its baseline.
    pub baseline: i64,
    /// How far its tallest text reaches above the baseline.
    pub ascent: i64,
    /// How far its deepest text reaches below the baseline.
    pub descent: i64,
    /// Its height: its ascent and descent together.
    pub height: i64,
    /// The left edge of its text, not counting spaces that hang past its
    /// end.
    pub x: i64,
    /// The width of its text, not counting spaces that hang past its end.
    pub width: i64,
    /// Its glyphs, in runs from left to right.
    pub runs: Vec<GlyphRun>,
    /// Its graphemes from left to right, where the caret can stand.
    pub(crate) cells: Vec<Cell>,
}

/// Glyphs from one face, in one style and one direction.
#[derive(Clone, Debug)]
pub struct GlyphRun {
    /// The face they come from.
    pub face: FaceId,
    /// The size of its em, in layout units.
    pub size: i64,
    /// The span of the paragraph whose style they have.
    pub span: usize,
    /// The text they show, as byte offsets into the paragraph.
    pub range: Range<usize>,
    /// Its bidi level: even runs read left to right, odd ones right to
    /// left.
    pub level: u8,
    /// Whether its glyphs are to be slanted: its style is italic but its
    /// face isn't, because the family has no italic, as Arabic, Hebrew and
    /// CJK fonts often don't (project owner, 2026-10-21).
    pub oblique: bool,
    /// Its left edge.
    pub x: i64,
    /// Its width: its glyphs' advances together.
    pub width: i64,
    /// Its glyphs, left to right.
    pub glyphs: Vec<Glyph>,
}

/// A positioned glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// Its ID in its run's face.
    pub id: u32,
    /// Its origin's distance from the paragraph's left edge.
    pub x: i64,
    /// Its origin's distance below the line's baseline.
    pub y: i64,
    /// How far it moves the pen.
    pub advance: i64,
    /// Where its cluster of the text starts, as a byte offset into the
    /// paragraph.
    pub cluster: usize,
}

/// A grapheme on a line, for caret placement: its text and its extent.
#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub range: Range<usize>,
    pub left: i64,
    pub right: i64,
    pub rtl: bool,
}

/// Lays out `paragraph` in `fonts`.
///
/// # Panics
///
/// If the spans don't cover the text exactly, a span doesn't end on a
/// character boundary, or there are no fonts.
pub fn layout(fonts: &Fonts, paragraph: &Paragraph<'_>) -> Layout {
    let text = paragraph.text;
    assert!(!paragraph.spans.is_empty(), "a paragraph needs a style");
    assert_eq!(
        paragraph.spans.iter().map(|s| s.len).sum::<usize>(),
        text.len(),
        "a paragraph's spans cover its text"
    );
    let bidi = Bidi::new(text, paragraph.direction);
    let graphemes = locale::graphemes(text);
    let items = shape::itemize(fonts, text, paragraph.spans, &bidi, &graphemes);

    // Each cluster's advance, at the offset where it starts, and their
    // running sums: the width of a stretch of text as shaped whole.
    let mut advance = vec![0; text.len() + 1];
    for glyph in items.iter().flat_map(|i| &i.glyphs) {
        advance[glyph.cluster] += glyph.advance;
    }
    let mut position = Vec::with_capacity(advance.len() + 1);
    position.push(0);
    for a in &advance {
        position.push(position.last().copied().unwrap_or(0) + a);
    }
    let shaping = Shaping {
        fonts,
        paragraph,
        items,
        position,
    };

    let hard_breaks: Vec<usize> = text
        .char_indices()
        .filter(|&(_, c)| shape::is_hard_break(c))
        .map(|(i, c)| i + c.len_utf8())
        .collect();
    let mut lines = Vec::new();
    let mut top = 0;
    let lines_at = break_lines(text, &graphemes, paragraph.width, |r| shaping.width(r));
    for range in lines_at {
        let line = build_line(&shaping, &bidi, &graphemes, range, top);
        top += line.height;
        lines.push(line);
    }
    Layout {
        lines,
        width: paragraph.width,
        height: top,
        direction: bidi.direction(),
        text: text.to_string(),
        graphemes,
        hard_breaks,
    }
}

/// Where `text`'s lines start and end: first fit, breaking where UAX #14
/// allows, with spaces at a line's end hanging past it. A word too wide
/// for a line on its own is broken between graphemes.
/// `measure` gives a line's width, less the spaces hanging at its end.
fn break_lines(
    text: &str,
    graphemes: &[usize],
    width: i64,
    measure: impl Fn(Range<usize>) -> i64,
) -> Vec<Range<usize>> {
    let breaks = locale::line_breaks(text);
    let fits = |range: Range<usize>| measure(range) <= width;
    let mut lines = Vec::new();
    let mut start = 0;
    let mut next = 0;
    loop {
        let mut fit = None;
        let mut overflow = text.len();
        while let Some(b) = breaks.get(next) {
            if b.offset <= start {
                next += 1;
            } else if fits(start..b.offset) {
                fit = Some(*b);
                next += 1;
                if b.mandatory {
                    break;
                }
            } else {
                overflow = b.offset;
                break;
            }
        }
        let end = match fit {
            Some(b) => b.offset,
            None => {
                // Graphemes, as many as fit, and at least one.
                let first = graphemes
                    .get(graphemes.partition_point(|&g| g <= start))
                    .copied()
                    .unwrap_or(text.len());
                let mut end = first;
                for &g in between(graphemes, first, overflow) {
                    if !fits(start..g) {
                        break;
                    }
                    end = g;
                }
                end
            }
        };
        lines.push(start..end);
        start = end;
        if start >= text.len() {
            // A hard break at the very end leaves an empty last line.
            if fit.is_some_and(|b| b.mandatory) {
                lines.push(start..start);
            }
            break;
        }
    }
    lines
}

/// Where `range` of `text` ends, less the spaces and hard break at its end,
/// which hang.
fn trim(text: &str, range: Range<usize>) -> usize {
    let line = &text[range.clone()];
    range.start + line.trim_end_matches(hangs).len()
}

fn hangs(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

/// A paragraph shaped whole, and what shapes parts of it again.
struct Shaping<'a> {
    fonts: &'a Fonts,
    paragraph: &'a Paragraph<'a>,
    items: Vec<Item>,
    /// The running sums of clusters' advances, by byte offset: the width of
    /// a stretch of text as shaped whole.
    position: Vec<i64>,
}

impl Shaping<'_> {
    /// The parts of items in `range`, in order: each item's index, and the
    /// part of its text in the range.
    fn parts(&self, range: Range<usize>) -> impl Iterator<Item = (usize, Range<usize>)> + '_ {
        let first = self.items.partition_point(|i| i.range.end <= range.start);
        self.items[first..]
            .iter()
            .take_while(move |i| i.range.start < range.end)
            .enumerate()
            .map(move |(k, i)| {
                (
                    first + k,
                    i.range.start.max(range.start)..i.range.end.min(range.end),
                )
            })
    }

    /// Whether `part` of item `index` keeps the glyphs the item was shaped
    /// with: it's cut, if at all, where breaking is safe.
    fn keeps(&self, index: usize, part: &Range<usize>) -> bool {
        let item = &self.items[index];
        can_cut(&item.glyphs, part.start, &item.range)
            && can_cut(&item.glyphs, part.end, &item.range)
    }

    /// The glyphs of `part` of item `index` at bidi `level`, left to right:
    /// the item's own where they hold, otherwise shaped again.
    fn glyphs(&self, index: usize, part: Range<usize>, level: u8) -> Vec<Shaped> {
        let item = &self.items[index];
        if level == item.level && self.keeps(index, &part) {
            if part == item.range {
                item.glyphs.clone()
            } else {
                item.glyphs
                    .iter()
                    .filter(|g| part.contains(&g.cluster))
                    .copied()
                    .collect()
            }
        } else {
            shape::shape(
                self.fonts,
                self.paragraph.text,
                part,
                &self.paragraph.spans[item.span],
                item.face,
                level,
                item.script,
            )
        }
    }

    /// How wide `range` is as a line, less the spaces hanging at its end:
    /// with its edges shaped again where breaking there changes them.
    fn width(&self, range: Range<usize>) -> i64 {
        let end = trim(self.paragraph.text, range.clone());
        if self
            .parts(range.clone())
            .all(|(i, part)| self.keeps(i, &part))
        {
            return self.position[end] - self.position[range.start];
        }
        self.parts(range)
            .map(|(i, part)| {
                self.glyphs(i, part, self.items[i].level)
                    .iter()
                    .filter(|g| g.cluster < end)
                    .map(|g| g.advance)
                    .sum::<i64>()
            })
            .sum()
    }
}

/// A piece of an item on a line, at one level.
struct Piece {
    range: Range<usize>,
    level: u8,
    item: usize,
    glyphs: Vec<Shaped>,
}

fn build_line(
    shaping: &Shaping<'_>,
    bidi: &Bidi<'_>,
    graphemes: &[usize],
    range: Range<usize>,
    top: i64,
) -> Line {
    let (fonts, paragraph) = (shaping.fonts, shaping.paragraph);
    let text = paragraph.text;
    let levels = bidi.line_levels(range.clone());
    let level_at = |offset: usize| levels[offset - range.start];

    // The items' parts on this line, shaped as `Shaping::width` measured
    // them, then split where rule L1 changed levels: whitespace that now
    // runs the paragraph's way is shaped again in that direction.
    let mut pieces: Vec<Piece> = Vec::new();
    for (index, part) in shaping.parts(range.clone()) {
        let item = &shaping.items[index];
        let glyphs = shaping.glyphs(index, part.clone(), item.level);
        let mut from = part.start;
        for &g in between(graphemes, part.start, part.end) {
            if g == part.end || level_at(g) != level_at(from) {
                let (sub, level) = (from..g, level_at(from));
                let glyphs = if level != item.level {
                    shape::shape(
                        fonts,
                        text,
                        sub.clone(),
                        &paragraph.spans[item.span],
                        item.face,
                        level,
                        item.script,
                    )
                } else if sub == part {
                    glyphs.clone()
                } else {
                    glyphs
                        .iter()
                        .filter(|g| sub.contains(&g.cluster))
                        .copied()
                        .collect()
                };
                pieces.push(Piece {
                    range: sub,
                    level,
                    item: index,
                    glyphs,
                });
                from = g;
            }
        }
    }

    // Left to right, and how wide: the text, and the spaces hanging at its
    // end.
    let order = locale::visual_order(&pieces.iter().map(|p| p.level).collect::<Vec<_>>());
    let end = trim(text, range.clone());
    let total: i64 = pieces
        .iter()
        .flat_map(|p| &p.glyphs)
        .map(|g| g.advance)
        .sum();
    let hang: i64 = pieces
        .iter()
        .flat_map(|p| &p.glyphs)
        .filter(|g| g.cluster >= end)
        .map(|g| g.advance)
        .sum();
    let width = total - hang;
    let rtl = bidi.paragraph_level() % 2 == 1;
    let free = paragraph.width - width;
    let text_x = match (paragraph.align, rtl) {
        (Align::Start, false) | (Align::End, true) => 0,
        (Align::Start, true) | (Align::End, false) => free,
        (Align::Center, _) => free.div_euclid(2),
    };
    // Hanging spaces are at the line's end: the right in a left-to-right
    // paragraph, the left in a right-to-left one.
    let mut pen = if rtl { text_x - hang } else { text_x };

    let mut runs = Vec::new();
    let mut cells = Vec::new();
    for &i in &order {
        let piece = &pieces[i];
        let item = &shaping.items[piece.item];
        let x = pen;
        let mut glyphs = Vec::with_capacity(piece.glyphs.len());
        for g in &piece.glyphs {
            if !g.hidden {
                glyphs.push(Glyph {
                    id: g.id,
                    x: pen + g.x_offset,
                    y: -g.y_offset,
                    advance: g.advance,
                    cluster: g.cluster,
                });
            }
            pen += g.advance;
        }
        cells.extend(piece_cells(text, graphemes, piece, x));
        let style = &paragraph.spans[item.span].style;
        runs.push(GlyphRun {
            face: item.face,
            size: style.size,
            span: item.span,
            range: piece.range.clone(),
            level: piece.level,
            oblique: style.italic && !fonts.face(item.face).italic(),
            x,
            width: pen - x,
            glyphs,
        });
    }

    // As tall as its fonts: each face's ascent and descent, with half its
    // line gap above and half below.
    let mut faces: Vec<(FaceId, i64)> = runs.iter().map(|r| (r.face, r.size)).collect();
    if faces.is_empty() {
        let span = span_at(paragraph.spans, range.start);
        let style = &paragraph.spans[span].style;
        let face = fonts.candidates(&style.family, style.weight, style.italic)[0];
        faces.push((face, style.size));
    }
    let (mut ascent, mut descent) = (0, 0);
    for (face, size) in faces {
        let face = fonts.face(face);
        let upem = face.units_per_em;
        let gap = scale(face.line_gap, size, upem);
        ascent = ascent.max(scale(face.ascent, size, upem) + gap / 2);
        descent = descent.max(scale(face.descent, size, upem) + gap - gap / 2);
    }
    Line {
        range,
        top,
        baseline: top + ascent,
        ascent,
        descent,
        height: ascent + descent,
        x: text_x,
        width,
        runs,
        cells,
    }
}

/// Whether `glyphs`, shaping `item`, can be cut at `offset` without shaping
/// again: at the item's edges, or where a cluster starts with a glyph safe
/// to break at.
fn can_cut(glyphs: &[Shaped], offset: usize, item: &Range<usize>) -> bool {
    if offset == item.start || offset == item.end {
        return true;
    }
    let mut at = glyphs.iter().filter(|g| g.cluster == offset).peekable();
    at.peek().is_some() && at.all(|g| !g.unsafe_to_break)
}

/// The index of the span holding `offset`; at the end, the last span.
fn span_at(spans: &[Span], offset: usize) -> usize {
    let mut end = 0;
    for (i, span) in spans.iter().enumerate() {
        end += span.len;
        if offset < end {
            return i;
        }
    }
    spans.len() - 1
}

/// The boundaries in `graphemes`, which are sorted, after `start` and up to
/// `end`.
fn between(graphemes: &[usize], start: usize, end: usize) -> &[usize] {
    let from = graphemes.partition_point(|&g| g <= start);
    let to = graphemes.partition_point(|&g| g <= end);
    &graphemes[from..to.max(from)]
}

/// `piece`'s graphemes, left to right, with the piece's left edge at `x`.
/// A cluster of several graphemes, such as a ligature, is shared evenly
/// between them. Hard breaks have no cell.
fn piece_cells(text: &str, graphemes: &[usize], piece: &Piece, x: i64) -> Vec<Cell> {
    let rtl = piece.level % 2 == 1;
    // Clusters left to right, with their extents.
    let mut clusters: Vec<(usize, i64, i64)> = Vec::new();
    let mut pen = x;
    for g in &piece.glyphs {
        match clusters.last_mut() {
            Some((cluster, _, right)) if *cluster == g.cluster => *right += g.advance,
            _ => clusters.push((g.cluster, pen, pen + g.advance)),
        }
        pen += g.advance;
    }
    let mut starts: Vec<usize> = clusters.iter().map(|c| c.0).collect();
    starts.sort_unstable();
    let mut cells = Vec::new();
    for (cluster, left, right) in clusters {
        let end = starts
            .get(starts.partition_point(|&s| s <= cluster))
            .copied()
            .unwrap_or(piece.range.end);
        let mut bounds = vec![cluster];
        bounds.extend(between(graphemes, cluster, end));
        if bounds.last() != Some(&end) {
            bounds.push(end);
        }
        let n = (bounds.len() - 1) as i64;
        let width = right - left;
        let mut parts: Vec<Cell> = bounds
            .windows(2)
            .enumerate()
            .map(|(k, pair)| {
                let k = k as i64;
                let (from, to) = if rtl { (n - 1 - k, n - k) } else { (k, k + 1) };
                Cell {
                    range: pair[0]..pair[1],
                    left: left + width * from / n,
                    right: left + width * to / n,
                    rtl,
                }
            })
            .collect();
        if rtl {
            parts.reverse();
        }
        cells.extend(parts.into_iter().filter(|c| {
            !text[c.range.clone()]
                .chars()
                .next()
                .is_some_and(shape::is_hard_break)
        }));
    }
    cells
}
