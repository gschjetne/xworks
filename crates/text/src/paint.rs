//! A laid-out paragraph as display-list items.

use scene::{Color, Glyph, Glyphs, Item, Point};

use crate::font::Fonts;
use crate::layout::Layout;
use crate::shape::is_hard_break;

impl Layout {
    /// The paragraph drawn with its top left at `origin`: a glyph run for
    /// each of its lines' runs, in `color` of the run's span. Each glyph
    /// carries the text its cluster shows.
    pub fn paint(&self, fonts: &Fonts, origin: Point, color: impl Fn(usize) -> Color) -> Vec<Item> {
        let mut items = Vec::new();
        for line in &self.lines {
            for run in &line.runs {
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
                    text: self.text[run.range.clone()].to_string(),
                    glyphs,
                }));
            }
        }
        items
    }
}
