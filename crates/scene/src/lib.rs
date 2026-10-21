//! The display list: what a view draws, as data, for the renderers to draw
//! (docs/ARCHITECTURE.md, section 10). M1 has fills, strokes and glyph
//! runs; images, clips and transforms come with the tracks that need them.
//!
//! - [`Page`]: a page's size and its [`Item`]s, drawn in order, each over
//!   the ones before.
//! - [`Glyphs`]: glyphs from one font, with the text they show, so PDF
//!   output can be searched and copied and screen readers can read it.
//! - [`Path`], [`Color`], [`Font`].
//!
//! Invariants:
//!
//! - Positions and sizes are in layout units, 914 400 to the inch, with y
//!   growing down the page from its top left corner.
//! - A display list holds no references into the document or the view that
//!   made it: it can be drawn on any thread, by any renderer, any number of
//!   times, and draws the same each time.

use std::ops::Range;
use std::sync::Arc;

/// The bytes of a font file, shared.
pub type FontData = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// A face of a font file, for drawing its glyphs.
#[derive(Clone)]
pub struct Font {
    /// The file.
    pub data: FontData,
    /// The face's index in the file; 0 unless it's a collection.
    pub index: u32,
}

impl Font {
    /// Whether `self` and `other` are the same face of the same file, by
    /// identity, not by comparing bytes.
    pub fn same(&self, other: &Font) -> bool {
        Arc::ptr_eq(&self.data, &other.data) && self.index == other.index
    }

    /// A key identifying the face while its data lives, for renderers to
    /// cache what they make of it.
    pub fn key(&self) -> (usize, u32) {
        (Arc::as_ptr(&self.data).cast::<u8>() as usize, self.index)
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("bytes", &(*self.data).as_ref().len())
            .field("index", &self.index)
            .finish()
    }
}

/// A colour: sRGB, with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Opacity: 0 is transparent, 255 opaque.
    pub a: u8,
}

impl Color {
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
}

/// A point, in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Point {
    /// Across, from the left.
    pub x: i64,
    /// Down, from the top.
    pub y: i64,
}

impl Point {
    /// The point (`x`, `y`).
    pub const fn new(x: i64, y: i64) -> Point {
        Point { x, y }
    }
}

/// A step of a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    /// Starts a new subpath at a point.
    MoveTo(Point),
    /// A straight line to a point.
    LineTo(Point),
    /// A quadratic Bézier curve: a control point, then the end.
    QuadTo(Point, Point),
    /// A cubic Bézier curve: two control points, then the end.
    CubicTo(Point, Point, Point),
    /// A line back to the subpath's start, closing it.
    Close,
}

/// A shape: subpaths of lines and curves. Fills use the nonzero winding
/// rule.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Path(pub Vec<Segment>);

impl Path {
    /// The rectangle `width` by `height` with its top left at (`x`, `y`).
    pub fn rect(x: i64, y: i64, width: i64, height: i64) -> Path {
        Path(vec![
            Segment::MoveTo(Point::new(x, y)),
            Segment::LineTo(Point::new(x + width, y)),
            Segment::LineTo(Point::new(x + width, y + height)),
            Segment::LineTo(Point::new(x, y + height)),
            Segment::Close,
        ])
    }
}

/// How far a synthetic oblique slants: the tangent of 12°, the slant
/// FreeType gives faces it obliques. Each glyph's top leans right of its
/// foot by this much of its height above the baseline.
pub const OBLIQUE_SLANT: f64 = 0.212_556_56;

/// A positioned glyph.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Glyph {
    /// Its ID in its run's font.
    pub id: u32,
    /// Its origin, on the baseline.
    pub at: Point,
    /// The part of its run's text it shows, as a byte range. Glyphs of one
    /// cluster, such as a letter and its marks, have the same range; a
    /// ligature's covers every character it joins.
    pub text: Range<usize>,
}

/// Glyphs from one font, at one size and in one colour.
#[derive(Clone, Debug)]
pub struct Glyphs {
    /// The font.
    pub font: Font,
    /// The size of its em, in layout units.
    pub size: i64,
    /// Their colour.
    pub color: Color,
    /// Whether to slant them by [`OBLIQUE_SLANT`], for italic text in a
    /// face that has none.
    pub oblique: bool,
    /// The baseline they sit on, but for their own offsets; a slant leans
    /// about it.
    pub baseline: i64,
    /// The text they show, in the order it reads.
    pub text: String,
    /// The glyphs, left to right.
    pub glyphs: Vec<Glyph>,
}

/// One thing drawn.
#[derive(Clone, Debug)]
pub enum Item {
    /// A shape filled with a colour.
    Fill {
        /// The shape.
        path: Path,
        /// The colour.
        color: Color,
    },
    /// A shape's outline, stroked with a colour.
    Stroke {
        /// The shape.
        path: Path,
        /// The line's width, in layout units.
        width: i64,
        /// The colour.
        color: Color,
    },
    /// Glyphs.
    Glyphs(Glyphs),
}

/// A page: its size, and what's drawn on it, back to front. Nothing is
/// drawn under the items; a page that should be white says so with a fill.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Its width, in layout units.
    pub width: i64,
    /// Its height, in layout units.
    pub height: i64,
    /// What's drawn on it, back to front.
    pub items: Vec<Item>,
}

impl Page {
    /// An empty page `width` by `height`.
    pub fn new(width: i64, height: i64) -> Page {
        Page {
            width,
            height,
            items: Vec::new(),
        }
    }

    /// Fills the whole page with `color`.
    pub fn fill_background(&mut self, color: Color) {
        self.items.push(Item::Fill {
            path: Path::rect(0, 0, self.width, self.height),
            color,
        });
    }

    /// Draws `other`'s items on this page with `other`'s top left corner at
    /// `at`: for sheets of pages, side by side.
    pub fn place(&mut self, other: &Page, at: Point) {
        let shift = |p: Point| Point::new(p.x + at.x, p.y + at.y);
        let path = |path: &Path| {
            Path(
                path.0
                    .iter()
                    .map(|segment| match *segment {
                        Segment::MoveTo(a) => Segment::MoveTo(shift(a)),
                        Segment::LineTo(a) => Segment::LineTo(shift(a)),
                        Segment::QuadTo(a, b) => Segment::QuadTo(shift(a), shift(b)),
                        Segment::CubicTo(a, b, c) => Segment::CubicTo(shift(a), shift(b), shift(c)),
                        Segment::Close => Segment::Close,
                    })
                    .collect(),
            )
        };
        for item in &other.items {
            self.items.push(match item {
                Item::Fill { path: p, color } => Item::Fill {
                    path: path(p),
                    color: *color,
                },
                Item::Stroke {
                    path: p,
                    width,
                    color,
                } => Item::Stroke {
                    path: path(p),
                    width: *width,
                    color: *color,
                },
                Item::Glyphs(run) => Item::Glyphs(Glyphs {
                    baseline: run.baseline + at.y,
                    glyphs: run
                        .glyphs
                        .iter()
                        .map(|g| Glyph {
                            at: shift(g.at),
                            ..g.clone()
                        })
                        .collect(),
                    ..run.clone()
                }),
            });
        }
    }

    /// Every font its glyphs use, each once, in the order first used.
    pub fn fonts(&self) -> Vec<Font> {
        let mut fonts: Vec<Font> = Vec::new();
        for item in &self.items {
            if let Item::Glyphs(g) = item
                && !fonts.iter().any(|f| f.same(&g.font))
            {
                fonts.push(g.font.clone());
            }
        }
        fonts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_is_closed_and_clockwise_on_screen() {
        let Path(segments) = Path::rect(10, 20, 30, 40);
        assert_eq!(
            segments,
            [
                Segment::MoveTo(Point::new(10, 20)),
                Segment::LineTo(Point::new(40, 20)),
                Segment::LineTo(Point::new(40, 60)),
                Segment::LineTo(Point::new(10, 60)),
                Segment::Close,
            ]
        );
    }

    #[test]
    fn a_placed_page_moves_everything() {
        let mut inner = Page::new(10, 10);
        inner.items.push(Item::Fill {
            path: Path::rect(1, 2, 3, 4),
            color: Color::BLACK,
        });
        let mut sheet = Page::new(100, 100);
        sheet.place(&inner, Point::new(50, 60));
        let Item::Fill { path, .. } = &sheet.items[0] else {
            panic!("a fill");
        };
        assert_eq!(path, &Path::rect(51, 62, 3, 4));
    }

    #[test]
    fn fonts_are_listed_once_by_identity() {
        let a: FontData = Arc::new(vec![1u8, 2, 3]);
        let b: FontData = Arc::new(vec![1u8, 2, 3]);
        let run = |data: &FontData, index| {
            Item::Glyphs(Glyphs {
                font: Font {
                    data: data.clone(),
                    index,
                },
                size: 1,
                color: Color::BLACK,
                oblique: false,
                baseline: 0,
                text: String::new(),
                glyphs: Vec::new(),
            })
        };
        let mut page = Page::new(100, 100);
        page.fill_background(Color::WHITE);
        page.items
            .extend([run(&a, 0), run(&b, 0), run(&a, 0), run(&a, 1)]);
        let fonts = page.fonts();
        assert_eq!(fonts.len(), 3, "the same bytes in two files are two fonts");
        assert!(fonts[0].same(&Font {
            data: a.clone(),
            index: 0
        }));
        assert!(fonts[1].same(&Font { data: b, index: 0 }));
        assert!(fonts[2].same(&Font { data: a, index: 1 }));
    }
}
