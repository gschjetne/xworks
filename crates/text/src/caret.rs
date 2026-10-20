//! Carets and selections on a laid-out paragraph: where the caret stands,
//! how it moves, and what a click hits.
//!
//! The caret stands between graphemes. Where directions meet, one place in
//! the text is two places on screen: between "abc" and a Hebrew word after
//! it, offset 3 is both after "c" and, at the far end of the Hebrew, before
//! its first letter. A [`Cursor`]'s affinity says which: the caret keeps to
//! the character it belongs with, the one just typed or just moved past.

use crate::layout::{Cell, Layout, Line};

/// Which character a cursor belongs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Affinity {
    /// The character before it, as after typing that character or moving
    /// forward past it.
    Upstream,
    /// The character after it, as after moving back past that character.
    Downstream,
}

/// A place between graphemes of a paragraph: an offset into its text and
/// the character it belongs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cursor {
    /// A byte offset into the paragraph's text, at a grapheme boundary.
    pub offset: usize,
    /// Which character it belongs with.
    pub affinity: Affinity,
}

impl Cursor {
    /// The cursor at `offset` belonging with the character before it.
    pub fn upstream(offset: usize) -> Cursor {
        Cursor {
            offset,
            affinity: Affinity::Upstream,
        }
    }

    /// The cursor at `offset` belonging with the character after it.
    pub fn downstream(offset: usize) -> Cursor {
        Cursor {
            offset,
            affinity: Affinity::Downstream,
        }
    }
}

/// Where the caret is drawn: a vertical bar on a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caret {
    /// Its distance from the paragraph's left edge.
    pub x: i64,
    /// The top of its line.
    pub top: i64,
    /// The bottom of its line.
    pub bottom: i64,
    /// Its line's index.
    pub line: usize,
    /// Whether the character it belongs with reads right to left, so the
    /// next one typed in the same direction appears to its left.
    pub rtl: bool,
}

/// A rectangle in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Its left edge.
    pub x: i64,
    /// Its top.
    pub y: i64,
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
}

/// The cursor at a cell's left edge.
fn left_of(cell: &Cell) -> Cursor {
    if cell.rtl {
        Cursor::upstream(cell.range.end)
    } else {
        Cursor::downstream(cell.range.start)
    }
}

/// The cursor at a cell's right edge.
fn right_of(cell: &Cell) -> Cursor {
    if cell.rtl {
        Cursor::downstream(cell.range.start)
    } else {
        Cursor::upstream(cell.range.end)
    }
}

impl Layout {
    /// `cursor` made exact: at a grapheme boundary within the text, and
    /// belonging with the character after it at the start of the text or
    /// of a line begun by a hard break.
    fn normalize(&self, cursor: Cursor) -> Cursor {
        let end = self.graphemes.last().copied().unwrap_or(0);
        let offset = match self.graphemes.binary_search(&cursor.offset.min(end)) {
            Ok(i) => self.graphemes[i],
            Err(i) => self.graphemes[i.saturating_sub(1)],
        };
        let affinity = if offset == 0 || self.hard_breaks.binary_search(&offset).is_ok() {
            Affinity::Downstream
        } else {
            cursor.affinity
        };
        Cursor { offset, affinity }
    }

    /// The line `cursor` is on: where a line wraps, the end of one line
    /// and the start of the next are the same offset, and its affinity
    /// says which.
    pub fn line_of(&self, cursor: Cursor) -> usize {
        let cursor = self.normalize(cursor);
        let last = self.lines.len() - 1;
        let i = self
            .lines
            .iter()
            .position(|l| l.range.end > cursor.offset)
            .unwrap_or(last);
        if i > 0
            && self.lines[i].range.start == cursor.offset
            && cursor.affinity == Affinity::Upstream
        {
            i - 1
        } else {
            i
        }
    }

    /// Which gap between `line`'s cells `cursor` stands in: 0 before the
    /// first, up to the number of cells after the last.
    fn gap(&self, line: &Line, cursor: Cursor) -> usize {
        let cells = &line.cells;
        let find = |c: Cursor| {
            (0..=cells.len()).find(|&k| {
                (k > 0 && right_of(&cells[k - 1]) == c)
                    || (k < cells.len() && left_of(&cells[k]) == c)
            })
        };
        let flipped = Cursor {
            affinity: match cursor.affinity {
                Affinity::Upstream => Affinity::Downstream,
                Affinity::Downstream => Affinity::Upstream,
            },
            ..cursor
        };
        find(cursor).or_else(|| find(flipped)).unwrap_or(0)
    }

    /// Where the caret is drawn for `cursor`.
    pub fn caret(&self, cursor: Cursor) -> Caret {
        let cursor = self.normalize(cursor);
        let i = self.line_of(cursor);
        let line = &self.lines[i];
        let k = self.gap(line, cursor);
        let cells = &line.cells;
        let (x, rtl) = if cells.is_empty() {
            (line.x, self.direction == locale::Direction::Rtl)
        } else if k > 0 && right_of(&cells[k - 1]) == cursor {
            (cells[k - 1].right, cells[k - 1].rtl)
        } else if k < cells.len() && left_of(&cells[k]) == cursor {
            (cells[k].left, cells[k].rtl)
        } else if k > 0 {
            (cells[k - 1].right, cells[k - 1].rtl)
        } else {
            (cells[0].left, cells[0].rtl)
        };
        Caret {
            x,
            top: line.top,
            bottom: line.top + line.height,
            line: i,
            rtl,
        }
    }

    /// The cursor one grapheme to the right on screen, or `None` at the
    /// paragraph's edge. Past a line's right end, it goes on at the line
    /// that continues the text that way: the next one in a left-to-right
    /// paragraph, the one before in a right-to-left one.
    pub fn right(&self, cursor: Cursor) -> Option<Cursor> {
        let cursor = self.normalize(cursor);
        let i = self.line_of(cursor);
        let line = &self.lines[i];
        let k = self.gap(line, cursor);
        if k < line.cells.len() {
            return Some(right_of(&line.cells[k]));
        }
        let next = match self.direction {
            locale::Direction::Ltr => i.checked_add(1).filter(|&n| n < self.lines.len()),
            locale::Direction::Rtl => i.checked_sub(1),
        }?;
        let cells = &self.lines[next].cells;
        let Some(first) = cells.first() else {
            return Some(self.line_start(next));
        };
        let landing = left_of(first);
        Some(if landing.offset == cursor.offset {
            right_of(first)
        } else {
            landing
        })
    }

    /// The cursor one grapheme to the left on screen, or `None` at the
    /// paragraph's edge; the mirror of [`Layout::right`].
    pub fn left(&self, cursor: Cursor) -> Option<Cursor> {
        let cursor = self.normalize(cursor);
        let i = self.line_of(cursor);
        let line = &self.lines[i];
        let k = self.gap(line, cursor);
        if k > 0 {
            return Some(left_of(&line.cells[k - 1]));
        }
        let next = match self.direction {
            locale::Direction::Ltr => i.checked_sub(1),
            locale::Direction::Rtl => i.checked_add(1).filter(|&n| n < self.lines.len()),
        }?;
        let cells = &self.lines[next].cells;
        let Some(last) = cells.last() else {
            return Some(self.line_start(next));
        };
        let landing = right_of(last);
        Some(if landing.offset == cursor.offset {
            left_of(last)
        } else {
            landing
        })
    }

    /// The cursor after the next grapheme in the text, or `None` at its
    /// end.
    pub fn next(&self, cursor: Cursor) -> Option<Cursor> {
        let cursor = self.normalize(cursor);
        let offset = self
            .graphemes
            .get(self.graphemes.partition_point(|&g| g <= cursor.offset))?;
        Some(self.normalize(Cursor::upstream(*offset)))
    }

    /// The cursor before the previous grapheme in the text, or `None` at
    /// its start.
    pub fn previous(&self, cursor: Cursor) -> Option<Cursor> {
        let cursor = self.normalize(cursor);
        let before = self.graphemes.partition_point(|&g| g < cursor.offset);
        let offset = self.graphemes[..before].last()?;
        Some(Cursor::downstream(*offset))
    }

    /// The cursor at the start of line `i` in the text's order.
    pub fn line_start(&self, i: usize) -> Cursor {
        self.normalize(Cursor::downstream(self.lines[i].range.start))
    }

    /// The cursor at the end of line `i` in the text's order: before the
    /// hard break that ends it, if one does.
    pub fn line_end(&self, i: usize) -> Cursor {
        let range = &self.lines[i].range;
        let end = if self.hard_breaks.binary_search(&range.end).is_ok() {
            let before = self.graphemes.partition_point(|&g| g < range.end);
            self.graphemes[..before]
                .last()
                .copied()
                .unwrap_or(range.start)
        } else {
            range.end
        };
        self.normalize(Cursor::upstream(end))
    }

    /// The cursor nearest the point (`x`, `y`): on the line at that height,
    /// or the first or last line above or below them all.
    pub fn hit(&self, x: i64, y: i64) -> Cursor {
        let i = self
            .lines
            .iter()
            .position(|l| y < l.top + l.height)
            .unwrap_or(self.lines.len() - 1);
        self.hit_line(i, x)
    }

    /// The cursor on line `i` nearest `x`: the nearer edge of the grapheme
    /// at `x`, or the line's near end.
    pub fn hit_line(&self, i: usize, x: i64) -> Cursor {
        let cells = &self.lines[i].cells;
        let (Some(first), Some(last)) = (cells.first(), cells.last()) else {
            return self.line_start(i);
        };
        if x < first.left {
            return left_of(first);
        }
        if x >= last.right {
            return right_of(last);
        }
        let cell = cells
            .iter()
            .find(|c| c.left <= x && x < c.right)
            .unwrap_or(last);
        if 2 * x < cell.left + cell.right {
            left_of(cell)
        } else {
            right_of(cell)
        }
    }

    /// The rectangles that highlight `range` of the text, a byte range at
    /// grapheme boundaries: on each line, one for each stretch of it that
    /// is together on screen.
    pub fn selection(&self, range: std::ops::Range<usize>) -> Vec<Rect> {
        let mut rects = Vec::new();
        for line in &self.lines {
            let mut spans: Vec<(i64, i64)> = Vec::new();
            for cell in &line.cells {
                if cell.range.start >= range.start && cell.range.end <= range.end {
                    match spans.last_mut() {
                        Some((_, right)) if *right == cell.left => *right = cell.right,
                        _ => spans.push((cell.left, cell.right)),
                    }
                }
            }
            rects.extend(spans.into_iter().map(|(left, right)| Rect {
                x: left,
                y: line.top,
                width: right - left,
                height: line.height,
            }));
        }
        rects
    }
}
