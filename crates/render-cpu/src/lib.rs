//! The reference renderer: a display list drawn to pixels in software, on
//! `vello_cpu` (D7, docs/ARCHITECTURE.md, section 10).
//!
//! [`render`] draws a [`scene::Page`] at any resolution. At
//! [`Level::Portable`] it draws with scalar code, and its pixels are the
//! same on every machine and platform: tests and golden images use it. At
//! [`Level::Fastest`] it uses the machine's best SIMD instructions, which
//! draw slightly different pixels on some machines (within a few of 255).
//!
//! Invariants:
//!
//! - The same page at the same resolution and level draws the same pixels,
//!   every time.
//! - Pixels are straight RGBA, 8 bits a channel, row by row; nothing is
//!   drawn under the page's items, so what they don't cover is transparent.
//! - No threads: the app runs renders on its own workers.

use std::collections::HashMap;
use std::sync::Arc;

use scene::{Color, Item, Page, Path, Segment};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{Affine, BezPath, Stroke};
use vello_cpu::peniko::{Blob, Fill, FontData};
use vello_cpu::{Pixmap, RenderContext, RenderSettings, Resources};

/// Which of the machine's instructions to draw with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Level {
    /// Scalar code, the same pixels everywhere: for tests and goldens.
    #[default]
    Portable,
    /// The machine's best SIMD: for the screen.
    Fastest,
}

/// A rendered image: straight RGBA, 8 bits a channel, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Its width, in pixels.
    pub width: u32,
    /// Its height, in pixels.
    pub height: u32,
    /// Its pixels, four bytes each.
    pub pixels: Vec<u8>,
}

/// The size in pixels of a `units`-long side at `ppi` pixels per inch,
/// rounded up, between 1 and 65 535.
pub fn pixels(units: i64, ppi: f64) -> u16 {
    let n = (units as f64 * ppi / scene_inch()).ceil();
    n.clamp(1.0, f64::from(u16::MAX)) as u16
}

fn scene_inch() -> f64 {
    914_400.0
}

/// `page` drawn at `ppi` pixels per inch.
pub fn render(page: &Page, ppi: f64, level: Level) -> Image {
    let (width, height) = (pixels(page.width, ppi), pixels(page.height, ppi));
    let settings = RenderSettings {
        level: match level {
            Level::Portable => vello_cpu::Level::Fallback(fearless_simd::Fallback::new()),
            Level::Fastest => vello_cpu::Level::new(),
        },
        num_threads: 0,
    };
    let mut ctx = RenderContext::new_with(width, height, settings);
    let mut resources = Resources::new();
    let scale = ppi / scene_inch();
    let at = |v: i64| v as f64 * scale;
    let mut fonts: HashMap<(usize, u32), FontData> = HashMap::new();
    ctx.set_fill_rule(Fill::NonZero);
    for item in &page.items {
        match item {
            Item::Fill { path, color } => {
                ctx.set_paint(paint(*color));
                ctx.fill_path(&bez(path, scale));
            }
            Item::Stroke { path, width, color } => {
                ctx.set_paint(paint(*color));
                ctx.set_stroke(Stroke::new(at(*width)));
                ctx.stroke_path(&bez(path, scale));
            }
            Item::Glyphs(run) => {
                let font = fonts
                    .entry(run.font.key())
                    .or_insert_with(|| {
                        FontData::new(Blob::new(Arc::clone(&run.font.data)), run.font.index)
                    })
                    .clone();
                ctx.set_paint(paint(run.color));
                let mut builder = ctx
                    .glyph_run(&mut resources, &font)
                    .font_size(at(run.size) as f32)
                    .hint(false);
                if run.oblique {
                    // Glyph space runs down the page, so leaning the tops
                    // right takes x from y negatively.
                    builder = builder.glyph_transform(Affine::skew(-scene::OBLIQUE_SLANT, 0.0));
                }
                // A run whose glyphs can't be drawn draws nothing; the
                // rest of the page still does.
                let _ = builder.fill_glyphs(run.glyphs.iter().map(|g| vello_cpu::Glyph {
                    id: g.id,
                    x: at(g.at.x) as f32,
                    y: at(g.at.y) as f32,
                }));
            }
        }
    }
    ctx.flush();
    let mut pixmap = Pixmap::new(width, height);
    ctx.render(&mut pixmap, &mut resources);
    Image {
        width: u32::from(width),
        height: u32::from(height),
        pixels: pixmap
            .data()
            .iter()
            .flat_map(|p| straight([p.r, p.g, p.b, p.a]))
            .collect(),
    }
}

fn paint(c: Color) -> AlphaColor<vello_cpu::color::Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// A premultiplied pixel made straight, rounding to the nearest.
fn straight([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 || a == 255 {
        return [r, g, b, a];
    }
    let a32 = u32::from(a);
    let un = |c: u8| ((u32::from(c) * 255 + a32 / 2) / a32).min(255) as u8;
    [un(r), un(g), un(b), a]
}

/// `path` in pixels.
fn bez(path: &Path, scale: f64) -> BezPath {
    let p = |q: scene::Point| (q.x as f64 * scale, q.y as f64 * scale);
    let mut out = BezPath::new();
    for segment in &path.0 {
        match *segment {
            Segment::MoveTo(a) => out.move_to(p(a)),
            Segment::LineTo(a) => out.line_to(p(a)),
            Segment::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            Segment::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            Segment::Close => out.close_path(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Layout units in a pixel at 96 pixels per inch.
    const PX: i64 = 9_525;

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_rectangle_on_pixel_edges_fills_exactly_its_pixels() {
        let mut page = Page::new(64 * PX, 48 * PX);
        page.fill_background(Color::WHITE);
        page.items.push(Item::Fill {
            path: Path::rect(16 * PX, 16 * PX, 32 * PX, 16 * PX),
            color: Color::rgb(32, 48, 96),
        });
        let image = render(&page, 96.0, Level::Portable);
        assert_eq!((image.width, image.height), (64, 48));
        for y in 0..48 {
            for x in 0..64 {
                let inside = (16..48).contains(&x) && (16..32).contains(&y);
                let want = if inside { [32, 48, 96, 255] } else { [255; 4] };
                assert_eq!(pixel(&image, x, y), want, "({x}, {y})");
            }
        }
    }

    #[test]
    fn nothing_drawn_is_transparent_and_half_alpha_stays_straight() {
        let mut page = Page::new(4 * PX, 4 * PX);
        page.items.push(Item::Fill {
            path: Path::rect(0, 0, 2 * PX, 4 * PX),
            color: Color {
                r: 200,
                g: 100,
                b: 50,
                a: 128,
            },
        });
        let image = render(&page, 96.0, Level::Portable);
        assert_eq!(pixel(&image, 3, 0), [0, 0, 0, 0]);
        let [r, g, b, a] = pixel(&image, 0, 0);
        assert_eq!(a, 128);
        for (got, want) in [(r, 200), (g, 100), (b, 50)] {
            assert!(got.abs_diff(want) <= 1, "{got} for {want}");
        }
    }

    #[test]
    fn resolution_scales_the_page() {
        let page = Page::new(64 * PX, 48 * PX);
        let image = render(&page, 192.0, Level::Portable);
        assert_eq!((image.width, image.height), (128, 96));
        assert_eq!(pixels(1, 96.0), 1);
    }

    #[test]
    fn straight_alpha_undoes_premultiplying() {
        assert_eq!(straight([100, 50, 0, 128]), [199, 100, 0, 128]);
        assert_eq!(straight([0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(straight([255, 255, 255, 255]), [255, 255, 255, 255]);
    }
}
