//! The display list: what a view draws, as data, for the renderers to draw
//! (docs/ARCHITECTURE.md, section 10). Views make pages of items; render-cpu,
//! render-gpu and render-pdf draw them, and none of them looks back at the
//! document that made them.
//!
//! - [`Page`]: a page's size and its [`Item`]s, drawn in order, each over
//!   the ones before: fills, strokes, glyph runs, images, and groups of
//!   items clipped and transformed.
//! - [`Glyphs`]: glyphs from one font, with the text they show, so PDF
//!   output can be searched and copied and screen readers can read it.
//! - [`Image`]: a raster image, sRGB and premultiplied, at most
//!   [`MAX_SIDE`] pixels a side, with smaller copies of itself for drawing
//!   it small, and, where it's given, the sRGB JPEG file it was decoded
//!   from, for PDF to hold as it is.
//! - [`Group`]: items drawn through a [`Transform`], within a clip.
//! - [`Path`], [`Rect`], [`Color`], [`Font`].
//! - [`Structure`]: a document's blocks in reading order, nested as lists,
//!   quotes and tables nest them, which glyph runs and images point into
//!   ([`Source`]), for tagged PDF and accessibility.
//! - [`changed`] and [`redraw`]: where two display lists draw differently,
//!   so a window draws only that again.
//!
//! # What a caller may rely on
//!
//! - **Pure.** No I/O, threads, clocks or environment (Clippy's
//!   `crates/clippy.toml`), and it builds for WebAssembly (CI).
//! - **Units:** positions and sizes are integers in layout units, 914 400
//!   to the inch, with y growing down the page from its top left corner;
//!   colours are sRGB with straight alpha. The renderers draw by them
//!   (render-cpu's unit tests
//!   `a_rectangle_on_pixel_edges_fills_exactly_its_pixels` and
//!   `resolution_scales_the_page`; this crate's unit test
//!   `a_rect_is_closed_and_clockwise_on_screen`).
//! - **A display list stands alone.** It holds no references into the
//!   document or the view that made it, sharing only fonts' and images'
//!   bytes, so it can be drawn on any thread, by any renderer, any number
//!   of times, and draws the same each time (this crate's unit test
//!   `display_lists_go_between_threads`; render-cpu's
//!   `tests/damage.rs`, `a_renderer_draws_the_same_whatever_it_drew_before`).
//! - **Bounds hold what's drawn:** an item's [`Item::bounds`] hold every
//!   pixel it draws, anti-aliasing, glyphs, strokes, images and groups
//!   included (render-cpu's `tests/damage.rs`,
//!   `every_item_draws_within_its_bounds`; this crate's unit test
//!   `a_group_is_bounded_by_its_transformed_items_and_its_clip`).
//! - **Damage is enough:** drawing every item of a new list over the
//!   rectangles [`changed`] and [`redraw`] give, over the old list's
//!   pixels, gives the new list's pixels; nothing changed is no damage
//!   (render-cpu's `tests/damage.rs`,
//!   `drawing_the_damage_over_the_old_frame_gives_the_new_one`; `damage`'s
//!   unit tests).
//! - **Images are the same everywhere:** at most [`MAX_SIDE`] pixels a
//!   side, a larger one scaled to fit keeping its proportions, with
//!   levels halved down to a pixel by an area-averaging resampler in
//!   integers, so every platform has the same pixels; pixels that don't
//!   fill the size given are an [`ImageError`], never a panic (`image`'s
//!   unit tests, such as `levels_halve_down_to_a_pixel` and
//!   `an_image_needs_pixels_and_the_right_number_of_them`).
//! - **Structure:** a block is inside an earlier block or none;
//!   [`Structure::parent`] takes any other parent as none, so no structure
//!   loops (this crate's unit test `a_parent_is_an_earlier_block`).
//!
//! # What it doesn't promise
//!
//! - That every item is visible: items may lie off the page, and paths
//!   may be empty.
//! - Exact turns: [`Transform::rotate`] by other than quarter turns takes
//!   its sine and cosine from the platform, which may differ in the last
//!   bit, so goldens turn by quarters or give the matrix.
//! - Tight bounds: they're generous, glyphs taken to fill their font's
//!   bounding box and strokes to reach twice their width.
//! - That [`Glyphs::text`] and [`Glyph::text`] agree with the glyphs, or
//!   that sources point into blocks that exist: the view that made them
//!   keeps that, and renderers take what they're given.

mod damage;
mod image;

pub use damage::{PixelRect, changed, merge, redraw};
pub use image::{FileData, Image, ImageError, Level, MAX_SIDE};

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

/// A rectangle, in layout units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Its left edge.
    pub x: i64,
    /// Its top edge.
    pub y: i64,
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
}

impl Rect {
    /// Larger than any page: what an item covers when it can't say.
    pub const EVERYWHERE: Rect = Rect {
        x: -(1 << 60),
        y: -(1 << 60),
        width: 1 << 61,
        height: 1 << 61,
    };

    /// The rectangle `width` by `height` with its top left at (`x`, `y`).
    pub const fn new(x: i64, y: i64, width: i64, height: i64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Its right edge.
    pub fn right(&self) -> i64 {
        self.x + self.width
    }

    /// Its bottom edge.
    pub fn bottom(&self) -> i64 {
        self.y + self.height
    }

    /// Whether it covers nothing.
    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// The smallest rectangle holding both.
    pub fn union(self, other: Rect) -> Rect {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let (r, b) = (
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        );
        Rect::new(x, y, r - x, b - y)
    }

    /// Where the two overlap, if they do.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let (r, b) = (
            self.right().min(other.right()),
            self.bottom().min(other.bottom()),
        );
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    /// The rectangle grown by `by` on every side.
    pub fn outset(self, by: i64) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.width + 2 * by,
            self.height + 2 * by,
        )
    }
}

/// An affine transform, as PDF and kurbo write one: a point (x, y) goes to
/// (a·x + c·y + e, b·x + d·y + f), for `[a, b, c, d, e, f]`, with e and f
/// in layout units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform(pub [f64; 6]);

impl Transform {
    /// Leaves everything where it is.
    pub const IDENTITY: Transform = Transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// Moves everything by (`x`, `y`).
    pub fn translate(x: i64, y: i64) -> Transform {
        Transform([1.0, 0.0, 0.0, 1.0, x as f64, y as f64])
    }

    /// Scales everything about the origin.
    pub fn scale(x: f64, y: f64) -> Transform {
        Transform([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    /// Turns everything `degrees` clockwise on the page about the origin.
    /// Quarter turns are exact; others take their sine and cosine from
    /// the platform, which may differ in the last bit between platforms,
    /// so goldens turn by quarters or give the matrix itself.
    pub fn rotate(degrees: f64) -> Transform {
        let quarter = degrees / 90.0;
        let (sin, cos) = if quarter.fract() == 0.0 {
            match quarter.rem_euclid(4.0) as u8 {
                0 => (0.0, 1.0),
                1 => (1.0, 0.0),
                2 => (0.0, -1.0),
                _ => (-1.0, 0.0),
            }
        } else {
            degrees.to_radians().sin_cos()
        };
        Transform([cos, sin, -sin, cos, 0.0, 0.0])
    }

    /// `self`, then `next`: what applying one and then the other does.
    pub fn then(self, next: Transform) -> Transform {
        let [a, b, c, d, e, f] = self.0;
        let [na, nb, nc, nd, ne, nf] = next.0;
        Transform([
            na * a + nc * b,
            nb * a + nd * b,
            na * c + nc * d,
            nb * c + nd * d,
            na * e + nc * f + ne,
            nb * e + nd * f + nf,
        ])
    }

    /// Where the point (`x`, `y`) goes.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

impl Default for Transform {
    fn default() -> Transform {
        Transform::IDENTITY
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
    /// The language of their text, a BCP 47 tag, if it's known.
    pub language: Option<String>,
    /// Where their text belongs in the document's structure, if anywhere.
    pub source: Option<Source>,
    /// The text they show, in the order it reads.
    pub text: String,
    /// The glyphs, left to right.
    pub glyphs: Vec<Glyph>,
}

/// Where a glyph run's text belongs in a document: which block of its
/// [`Structure`], and where in that block's text it starts. Runs are drawn
/// left to right, but read in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Source {
    /// The block's index in the structure.
    pub block: u32,
    /// The byte offset of the run's text in the block's.
    pub offset: usize,
}

/// What a block of a document is, for accessibility and tagged PDF. Some
/// hold text, which glyph runs point into; the others hold blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// A paragraph.
    Paragraph,
    /// A heading, of level 1 (the highest) to 6.
    Heading(u8),
    /// A figure: a picture on its own, which its image items' alternative
    /// text describes.
    Figure,
    /// A list of items, numbered or not.
    List {
        /// Whether its items are numbered.
        ordered: bool,
    },
    /// An item of a list: its label, then its body.
    ListItem,
    /// A list item's label, its bullet or number: text.
    Label,
    /// A list item's body: its blocks.
    ListBody,
    /// A quotation: its blocks.
    Quote,
    /// A block of code: text.
    Code,
    /// A table: its rows.
    Table,
    /// A row of a table: its cells.
    Row,
    /// A cell of a table, a header's or not: text.
    Cell {
        /// Whether it's in the table's header.
        header: bool,
    },
}

/// A document's logical structure: its blocks in reading order, whatever
/// pages they fall on, nested, and its language.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Structure {
    /// The document's language, a BCP 47 tag, if it's known.
    pub language: Option<String>,
    /// Its blocks, in reading order, each before the blocks inside it.
    pub blocks: Vec<Role>,
    /// The block each block is inside, by index, if it's inside one: an
    /// earlier one. Blocks past its end are inside none, so a structure of
    /// blocks one after another needs none.
    pub parents: Vec<Option<u32>>,
}

impl Structure {
    /// The block `block` is inside, if it's inside one.
    pub fn parent(&self, block: usize) -> Option<usize> {
        self.parents
            .get(block)
            .copied()
            .flatten()
            .map(|p| p as usize)
            .filter(|&p| p < block)
    }

    /// Adds `other`'s blocks after this one's, as `other` nests them.
    /// Returns the index its first block has now, for its glyph runs'
    /// sources.
    pub fn append(&mut self, other: &Structure) -> u32 {
        let first = self.blocks.len() as u32;
        self.parents.resize(self.blocks.len(), None);
        self.blocks.extend(&other.blocks);
        self.parents
            .extend((0..other.blocks.len()).map(|i| other.parent(i).map(|p| p as u32 + first)));
        first
    }
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
    /// An image, stretched to fill a rectangle.
    Image {
        /// The image.
        image: Image,
        /// Where it's drawn: its top left pixel's corner at the
        /// rectangle's, its bottom right's at the rectangle's.
        rect: Rect,
        /// What it shows, in words, for screen readers and tagged PDF.
        alt: Option<String>,
        /// Where it belongs in the document's structure, if anywhere: in a
        /// figure block, or in a block of text at an offset.
        source: Option<Source>,
    },
    /// Items drawn through a transform, within a clip.
    Group(Group),
}

/// Items drawn together: through a transform, and only within a clip.
#[derive(Clone, Debug, Default)]
pub struct Group {
    /// Where its items show, in the coordinates the group is drawn in,
    /// before its transform: nothing outside it shows. `None` clips
    /// nothing.
    pub clip: Option<Path>,
    /// What its items' coordinates go through on the way to the group's.
    pub transform: Transform,
    /// Its items, back to front.
    pub items: Vec<Item>,
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
        self.items
            .extend(other.items.iter().map(|item| shifted(item, at)));
    }

    /// Every font its glyphs use, each once, in the order first used.
    pub fn fonts(&self) -> Vec<Font> {
        let mut fonts: Vec<Font> = Vec::new();
        fn walk(items: &[Item], fonts: &mut Vec<Font>) {
            for item in items {
                match item {
                    Item::Glyphs(g) if !fonts.iter().any(|f| f.same(&g.font)) => {
                        fonts.push(g.font.clone());
                    }
                    Item::Group(group) => walk(&group.items, fonts),
                    _ => {}
                }
            }
        }
        walk(&self.items, &mut fonts);
        fonts
    }
}

/// `item` moved by `at`.
fn shifted(item: &Item, at: Point) -> Item {
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
    match item {
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
        Item::Image {
            image,
            rect,
            alt,
            source,
        } => Item::Image {
            image: image.clone(),
            rect: Rect::new(rect.x + at.x, rect.y + at.y, rect.width, rect.height),
            alt: alt.clone(),
            source: *source,
        },
        // The group's items stay in its coordinates; the group moves.
        Item::Group(group) => Item::Group(Group {
            clip: group.clip.as_ref().map(path),
            transform: group.transform.then(Transform::translate(at.x, at.y)),
            items: group.items.clone(),
        }),
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
    fn transforms_compose_in_order() {
        let t = Transform::scale(2.0, 3.0).then(Transform::translate(10, 20));
        assert_eq!(t.apply(1.0, 1.0), (12.0, 23.0));
        let t = Transform::translate(10, 20).then(Transform::scale(2.0, 3.0));
        assert_eq!(t.apply(1.0, 1.0), (22.0, 63.0));
        // A quarter turn clockwise on the page, exactly.
        assert_eq!(Transform::rotate(90.0).apply(10.0, 0.0), (0.0, 10.0));
        assert_eq!(Transform::rotate(-90.0).apply(10.0, 0.0), (0.0, -10.0));
        assert_eq!(Transform::rotate(180.0).apply(1.0, 2.0), (-1.0, -2.0));
        assert_eq!(
            Transform::rotate(90.0).then(Transform::rotate(270.0)),
            Transform::IDENTITY
        );
    }

    #[test]
    fn rects_meet_and_join() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.union(b), Rect::new(0, 0, 15, 15));
        assert_eq!(a.intersection(b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.intersection(Rect::new(10, 0, 5, 5)), None);
        assert_eq!(a.outset(2), Rect::new(-2, -2, 14, 14));
    }

    #[test]
    fn a_placed_page_moves_images_and_groups() {
        let image = Image::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        let mut inner = Page::new(10, 10);
        inner.items.push(Item::Image {
            image: image.clone(),
            rect: Rect::new(1, 2, 3, 4),
            alt: None,
            source: None,
        });
        inner.items.push(Item::Group(Group {
            clip: Some(Path::rect(0, 0, 5, 5)),
            transform: Transform::scale(2.0, 2.0),
            items: vec![Item::Fill {
                path: Path::rect(1, 1, 1, 1),
                color: Color::BLACK,
            }],
        }));
        let mut sheet = Page::new(100, 100);
        sheet.place(&inner, Point::new(50, 60));
        let Item::Image { rect, .. } = &sheet.items[0] else {
            panic!("an image");
        };
        assert_eq!(*rect, Rect::new(51, 62, 3, 4));
        let Item::Group(group) = &sheet.items[1] else {
            panic!("a group");
        };
        assert_eq!(group.clip, Some(Path::rect(50, 60, 5, 5)));
        assert_eq!(group.transform.apply(1.0, 1.0), (52.0, 62.0));
        assert_eq!(sheet.items[1].bounds(), Some(Rect::new(52, 62, 2, 2)));
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
                language: None,
                source: None,
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

    /// A block is inside an earlier block or none: a parent that's the
    /// block itself, a later one or past the end is none, so no structure
    /// loops. Appending one structure to another keeps its nesting.
    #[test]
    fn a_parent_is_an_earlier_block() {
        let structure = Structure {
            language: None,
            blocks: vec![Role::Quote, Role::Paragraph, Role::Paragraph, Role::Quote],
            parents: vec![Some(3), Some(0), Some(2), Some(9)],
        };
        let parents: Vec<_> = (0..5).map(|b| structure.parent(b)).collect();
        assert_eq!(parents, [None, Some(0), None, None, None]);
        let mut whole = Structure {
            language: None,
            blocks: vec![Role::Paragraph],
            parents: Vec::new(),
        };
        assert_eq!(whole.append(&structure), 1);
        let parents: Vec<_> = (0..5).map(|b| whole.parent(b)).collect();
        assert_eq!(parents, [None, None, Some(1), None, None]);
    }

    /// A display list owns what it holds, sharing fonts' and images' bytes
    /// by reference counts, so it goes to any thread and lives as long as
    /// it's needed; the compiler checks it.
    #[test]
    fn display_lists_go_between_threads() {
        fn shared<T: Send + Sync + 'static>() {}
        shared::<Page>();
        shared::<Item>();
        shared::<Image>();
        shared::<Structure>();
    }
}
