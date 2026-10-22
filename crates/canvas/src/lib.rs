//! The canvas substrate: pages, masters and the open pasteboard, frames, and
//! automatic pagination (docs/ARCHITECTURE.md, section 4). M1 has its first
//! form: automatic pages from one page style, the mode the word processor
//! grows from.
//!
//! - [`PageStyle`]: a page's size and margins.
//! - [`paginate`]: blocks of lines poured into pages, breaking paragraphs
//!   between lines.
//! - [`automatic`]: a document's story poured into automatic pages and
//!   drawn, page by page, as display lists.
//!
//! The rules where a page breaks:
//!
//! - A paragraph that doesn't fit breaks between lines, with at least two
//!   of its lines at the foot of a page (no orphans) and two at the top of
//!   the next (no widows), where it has that many.
//! - A block that keeps with the next, as headings do, moves to the next
//!   page rather than end a page without the start of the block after it;
//!   a run of such blocks moves together.
//! - Space before a block is dropped at the top of a page, and space after
//!   one at the foot.
//! - Where a page is too short to keep those rules, it takes what fits;
//!   a line taller than the page still goes on a page of its own.
//!
//! Invariants:
//!
//! - Every line of every block is placed once, in order.
//! - Where a page breaks depends only on what comes before the break and
//!   the run of kept-together blocks it ends in: editing a block leaves
//!   every page before that run's page as it was.

use std::ops::Range;

use doc::Document;
use scene::{Color, Page, Point, Role, Structure};
use serde::Deserialize;
use story::{ParagraphLayout, StyleSheet};
use text::Fonts;
use text::units::Length;

/// A page's size and margins, as `spec/styles/` gives them.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct PageStyleSpec {
    /// Its width.
    pub width: Length,
    /// Its height.
    pub height: Length,
    /// The margin at the top.
    pub top: Length,
    /// The margin at the bottom.
    pub bottom: Length,
    /// The margin at the left.
    pub left: Length,
    /// The margin at the right.
    pub right: Length,
}

/// A page's size and margins, in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageStyle {
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
    /// The margin at the top.
    pub top: i64,
    /// The margin at the bottom.
    pub bottom: i64,
    /// The margin at the left.
    pub left: i64,
    /// The margin at the right.
    pub right: i64,
}

impl PageStyle {
    /// The page style `spec` describes.
    pub fn from_spec(spec: &PageStyleSpec) -> PageStyle {
        PageStyle {
            width: spec.width.units(),
            height: spec.height.units(),
            top: spec.top.units(),
            bottom: spec.bottom.units(),
            left: spec.left.units(),
            right: spec.right.units(),
        }
    }

    /// The width of the text between the margins.
    pub fn body_width(&self) -> i64 {
        self.width - self.left - self.right
    }

    /// The height of the text between the margins.
    pub fn body_height(&self) -> i64 {
        self.height - self.top - self.bottom
    }
}

/// Something to pour onto pages: a paragraph's lines, by height, and the
/// space around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Its lines' heights, top to bottom. There is at least one.
    pub lines: Vec<i64>,
    /// Space above it, dropped at the top of a page.
    pub space_before: i64,
    /// Space below it, dropped at the foot of a page.
    pub space_after: i64,
    /// Whether it stays on the page where the next block starts.
    pub keep_with_next: bool,
}

/// Lines of one block placed on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The block's index.
    pub block: usize,
    /// Which of its lines.
    pub lines: Range<usize>,
    /// Where the first of them starts, down from the top of the page's body.
    pub y: i64,
}

/// The fewest lines of a paragraph left at the foot of a page.
pub const ORPHANS: usize = 2;
/// The fewest lines of a paragraph carried to the top of a page.
pub const WIDOWS: usize = 2;

/// `blocks` poured into pages whose bodies are `height` tall: each page's
/// pieces, top to bottom. There is always at least one page.
pub fn paginate(height: i64, blocks: &[Block]) -> Vec<Vec<Piece>> {
    let mut pages: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut y = 0;
    let mut b = 0;
    let mut first = 0;
    while let Some(block) = blocks.get(b) {
        let n = block.lines.len();
        let page = pages.last_mut().expect("a page");
        let at_top = page.is_empty();
        let before = if first == 0 && !at_top {
            block.space_before
        } else {
            0
        };

        // The lines that fit, and how many to take of them.
        let mut fit = 0;
        let mut height_of = before;
        while first + fit < n && y + height_of + block.lines[first + fit] <= height {
            height_of += block.lines[first + fit];
            fit += 1;
        }
        let left = n - first;
        let mut take = fit;
        if take < left {
            if left - take < WIDOWS {
                take = take.saturating_sub(WIDOWS - (left - take));
            }
            if first == 0 && take < ORPHANS.min(n) {
                take = 0;
            }
        } else if block.keep_with_next && !at_top {
            // All of it fits: does the start of the next block, too?
            if let Some(next) = blocks.get(b + 1) {
                let start: i64 = next.lines.iter().take(ORPHANS).sum();
                if y + height_of + block.space_after + next.space_before + start > height {
                    take = 0;
                }
            }
        }

        if take == 0 && !at_top {
            // This block starts on the next page, and so do the blocks at
            // the foot of this one that keep with it, unless one of them
            // already starts this page.
            let mut restart = b;
            while page.len() > 1 {
                let last = page.last().expect("a piece");
                let kept = &blocks[last.block];
                if last.block + 1 == restart
                    && kept.keep_with_next
                    && last.lines == (0..kept.lines.len())
                {
                    restart = last.block;
                    page.pop();
                } else {
                    break;
                }
            }
            if restart != b {
                first = 0;
            }
            b = restart;
            pages.push(Vec::new());
            y = 0;
            continue;
        }
        // At the top of a page, moving on wouldn't help: if the rules left
        // nothing, the page takes what fits, and at least one line, however
        // tall.
        let take = if take == 0 { fit.max(1) } else { take }.min(left);
        let lines_height: i64 = block.lines[first..first + take].iter().sum();
        page.push(Piece {
            block: b,
            lines: first..first + take,
            y: y + before,
        });
        y += before + lines_height;
        first += take;
        if first == n {
            y += block.space_after;
            b += 1;
            first = 0;
        } else {
            pages.push(Vec::new());
            y = 0;
        }
    }
    pages
}

/// A story's laid-out paragraphs as blocks to pour.
pub fn blocks(paragraphs: &[ParagraphLayout]) -> Vec<Block> {
    paragraphs
        .iter()
        .map(|p| Block {
            lines: p.layout.lines.iter().map(|l| l.height).collect(),
            space_before: p.space_before,
            space_after: p.space_after,
            keep_with_next: p.keep_with_next,
        })
        .collect()
}

/// `pages` of `paragraphs`, as [`paginate`] placed them, drawn on white
/// pages of `style`.
pub fn draw(
    style: &PageStyle,
    paragraphs: &[ParagraphLayout],
    pages: &[Vec<Piece>],
    fonts: &Fonts,
) -> Vec<Page> {
    pages
        .iter()
        .map(|pieces| {
            let mut page = Page::new(style.width, style.height);
            page.fill_background(Color::WHITE);
            for piece in pieces {
                let origin = Point::new(style.left, style.top + piece.y);
                page.items
                    .extend(paragraphs[piece.block].layout.paint_lines(
                        fonts,
                        piece.lines.clone(),
                        origin,
                        Some(piece.block as u32),
                        |_| Color::BLACK,
                    ));
            }
            page
        })
        .collect()
}

/// The structure of `paragraphs`: each a paragraph or a heading, in order.
pub fn structure(paragraphs: &[ParagraphLayout]) -> Structure {
    Structure {
        language: None,
        blocks: paragraphs
            .iter()
            .map(|p| p.heading.map_or(Role::Paragraph, Role::Heading))
            .collect(),
    }
}

/// A document drawn in pages, and its structure, which the pages' glyph
/// runs point into.
#[derive(Clone, Debug)]
pub struct Pages {
    /// The pages.
    pub pages: Vec<Page>,
    /// The document's blocks in reading order.
    pub structure: Structure,
}

/// `document`'s story in automatic pages of `style`: laid out, poured and
/// drawn.
pub fn automatic(
    document: &Document,
    sheet: &StyleSheet,
    style: &PageStyle,
    fonts: &Fonts,
) -> Pages {
    let paragraphs = story::lay_out(document, sheet, fonts, style.body_width());
    let pages = paginate(style.body_height(), &blocks(&paragraphs));
    Pages {
        pages: draw(style, &paragraphs, &pages, fonts),
        structure: structure(&paragraphs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(lines: usize) -> Block {
        Block {
            lines: vec![10; lines],
            space_before: 0,
            space_after: 0,
            keep_with_next: false,
        }
    }

    fn heading() -> Block {
        Block {
            lines: vec![20],
            space_before: 5,
            space_after: 5,
            keep_with_next: true,
        }
    }

    /// Each page's pieces as (block, first line, end line, y).
    fn shape(pages: &[Vec<Piece>]) -> Vec<Vec<(usize, usize, usize, i64)>> {
        pages
            .iter()
            .map(|p| {
                p.iter()
                    .map(|q| (q.block, q.lines.start, q.lines.end, q.y))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn what_fits_stays_on_one_page() {
        let pages = paginate(100, &[para(3), para(4)]);
        assert_eq!(shape(&pages), [vec![(0, 0, 3, 0), (1, 0, 4, 30)]]);
        assert_eq!(shape(&paginate(100, &[])), [Vec::new()]);
    }

    #[test]
    fn a_paragraph_breaks_between_lines() {
        // Ten lines on a page that holds six.
        let pages = paginate(60, &[para(10)]);
        assert_eq!(shape(&pages), [vec![(0, 0, 6, 0)], vec![(0, 6, 10, 0)]]);
    }

    #[test]
    fn no_widows_no_orphans() {
        // A page that holds five: three lines used, two left. A paragraph
        // of three would leave one line for the next page: it all moves.
        let pages = paginate(50, &[para(3), para(3)]);
        assert_eq!(shape(&pages), [vec![(0, 0, 3, 0)], vec![(1, 0, 3, 0)]]);
        // Four lines used, one left: a paragraph never starts with one line
        // at the foot of a page.
        let pages = paginate(50, &[para(4), para(4)]);
        assert_eq!(shape(&pages), [vec![(0, 0, 4, 0)], vec![(1, 0, 4, 0)]]);
        // A paragraph of six on a page that holds five: four and two, not
        // five and one.
        let pages = paginate(50, &[para(6)]);
        assert_eq!(shape(&pages), [vec![(0, 0, 4, 0)], vec![(0, 4, 6, 0)]]);
    }

    #[test]
    fn headings_keep_with_what_follows() {
        // Six lines used of eight; the heading would fit, but nothing of
        // the paragraph after it would.
        let pages = paginate(80, &[para(6), heading(), para(4)]);
        assert_eq!(
            shape(&pages),
            [vec![(0, 0, 6, 0)], vec![(1, 0, 1, 0), (2, 0, 4, 25)]]
        );
        // Two headings in a row move together: both fit, but the second
        // would end the page.
        let pages = paginate(100, &[para(4), heading(), heading(), para(4)]);
        assert_eq!(
            shape(&pages),
            [
                vec![(0, 0, 4, 0)],
                vec![(1, 0, 1, 0), (2, 0, 1, 30), (3, 0, 4, 55)]
            ]
        );
    }

    #[test]
    fn a_heading_at_the_top_of_a_page_stays() {
        // Nothing of what follows fits after it, but moving it on wouldn't
        // help: it already starts its page. And a paragraph of three on
        // pages of two can't keep both rules: it fills the page.
        let pages = paginate(20, &[heading(), para(3)]);
        assert_eq!(
            shape(&pages),
            [vec![(0, 0, 1, 0)], vec![(1, 0, 2, 0)], vec![(1, 2, 3, 0)]]
        );
    }

    #[test]
    fn space_before_goes_at_the_top_of_a_page() {
        let spaced = Block {
            space_before: 15,
            ..para(2)
        };
        let pages = paginate(40, &[para(2), spaced.clone(), spaced]);
        assert_eq!(
            shape(&pages),
            [vec![(0, 0, 2, 0)], vec![(1, 0, 2, 0)], vec![(2, 0, 2, 0)]]
        );
    }

    #[test]
    fn a_line_taller_than_the_page_gets_one() {
        let tall = Block {
            lines: vec![500],
            ..para(1)
        };
        let pages = paginate(100, &[para(2), tall, para(1)]);
        assert_eq!(
            shape(&pages),
            [vec![(0, 0, 2, 0)], vec![(1, 0, 1, 0)], vec![(2, 0, 1, 0)]]
        );
    }
}
