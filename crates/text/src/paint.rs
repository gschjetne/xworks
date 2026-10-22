//! A laid-out paragraph as display-list items.

use scene::{Color, Glyph, Glyphs, Item, Point, Source};

use crate::font::Fonts;
use crate::layout::Layout;
use crate::shape::is_hard_break;

impl Layout {
    /// The paragraph drawn with its top left at `origin`: a glyph run for
    /// each of its lines' runs, in reading order, in `color` of the run's
    /// span. Each glyph
    /// carries the text its cluster shows, and each run its language and,
    /// for a paragraph that's `block` of a document's structure, where its
    /// text belongs.
    pub fn paint(
        &self,
        fonts: &Fonts,
        origin: Point,
        block: Option<u32>,
        color: impl Fn(usize) -> Color,
    ) -> Vec<Item> {
        self.paint_lines(fonts, 0..self.lines.len(), origin, block, color)
    }

    /// `lines` of the paragraph, drawn with the first one's top left at
    /// `origin`, as [`Layout::paint`] draws them all: for a paragraph that
    /// breaks across pages.
    pub fn paint_lines(
        &self,
        fonts: &Fonts,
        lines: std::ops::Range<usize>,
        origin: Point,
        block: Option<u32>,
        color: impl Fn(usize) -> Color,
    ) -> Vec<Item> {
        let mut items = Vec::new();
        let Some(top) = self.lines.get(lines.start).map(|l| l.top) else {
            return items;
        };
        let origin = Point::new(origin.x, origin.y - top);
        for line in &self.lines[lines] {
            // In the order they read: they don't overlap, so it changes
            // nothing drawn, but text written to PDF reads back in order.
            let mut runs: Vec<_> = line.runs.iter().collect();
            runs.sort_by_key(|run| run.range.start);
            for run in runs {
                if run.glyphs.is_empty() {
                    continue;
                }
                // The run's text, less a hard break at its end, which no
                // glyph shows.
                let end = run.range.start
                    + self.text[run.range.clone()]
                        .trim_end_matches(is_hard_break)
                        .len();
                let mut starts: Vec<usize> = run.glyphs.iter().map(|g| g.cluster).collect();
                starts.sort_unstable();
                starts.dedup();
                let glyphs = run
                    .glyphs
                    .iter()
                    .map(|g| {
                        let next = starts
                            .get(starts.partition_point(|&s| s <= g.cluster))
                            .copied()
                            .unwrap_or(end)
                            .min(end)
                            .max(g.cluster);
                        Glyph {
                            id: g.id,
                            at: Point::new(origin.x + g.x, origin.y + line.baseline + g.y),
                            text: g.cluster - run.range.start..next - run.range.start,
                        }
                    })
                    .collect();
                items.push(Item::Glyphs(Glyphs {
                    font: fonts.face(run.face).font(),
                    size: run.size,
                    color: color(run.span),
                    oblique: run.oblique,
                    baseline: origin.y + line.baseline,
                    language: self.languages.get(run.span).cloned().flatten(),
                    source: block.map(|block| Source {
                        block,
                        offset: run.range.start,
                    }),
                    text: self.text[run.range.clone()].to_string(),
                    glyphs,
                }));
            }
        }
        items
    }
}
