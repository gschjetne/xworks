//! The reference renderer: a display list drawn to pixels in software, on
//! `vello_cpu` (D7, docs/ARCHITECTURE.md, section 10).
//!
//! [`render`] draws a [`scene::Page`] at any resolution, and [`Renderer`]
//! does too, keeping what it makes of the page's images from one render
//! to the next, and drawing a part of a page alone, for a window that
//! draws only what changed. At [`Level::Portable`] it draws with scalar
//! code, and its pixels are the same on every machine and platform: tests
//! and golden images use it. At [`Level::Fastest`] it uses the machine's
//! best SIMD instructions, which draw slightly different pixels on some
//! machines (within a few of 255).
//!
//! Invariants:
//!
//! - The same page at the same resolution and level draws the same pixels,
//!   every time, whatever was drawn before.
//! - Pixels are straight RGBA, 8 bits a channel, row by row; nothing is
//!   drawn under the page's items, so what they don't cover is transparent.
//! - Items outside groups are drawn at their coordinates scaled, with no
//!   transform; only a group's items go through one.
//! - An image is drawn from its smallest level at least as large as it's
//!   drawn, sampled bilinearly.
//! - No threads: the app runs renders on its own workers.

use std::collections::HashMap;
use std::sync::Arc;

use scene::{Color, Item, Page, Path, PixelRect, Segment};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{self, Affine, BezPath, Stroke};
use vello_cpu::peniko::{Blob, Extend, Fill, FontData, ImageAlphaType, ImageQuality, ImageSampler};
use vello_cpu::{ImageSource, PixelMetadata, Pixmap, RenderContext, RenderSettings, Resources};

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
    Renderer::new().render(page, ppi, level)
}

/// Draws pages, keeping the pixmaps it makes of their images while they're
/// drawn: an image's level is copied for vello_cpu once, not at every
/// render.
#[derive(Default)]
pub struct Renderer {
    images: HashMap<(usize, usize), Cached>,
    /// How many renders it has done, for forgetting images no longer drawn.
    renders: u64,
}

/// An image level as vello_cpu draws it.
struct Cached {
    /// The image, kept so its key can't come to mean another.
    _image: scene::Image,
    pixmap: Arc<Pixmap>,
    used: u64,
}

/// Renders after which an image not drawn in them is forgotten.
const FORGET_AFTER: u64 = 32;

impl Renderer {
    /// A renderer with nothing kept.
    pub fn new() -> Renderer {
        Renderer::default()
    }

    /// `page` drawn at `ppi` pixels per inch.
    pub fn render(&mut self, page: &Page, ppi: f64, level: Level) -> Image {
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: u32::from(pixels(page.width, ppi)),
            height: u32::from(pixels(page.height, ppi)),
        };
        self.render_rect(page, ppi, level, rect)
    }

    /// The pixels of `rect` of `page` drawn at `ppi`: only the items that
    /// reach into it are drawn, and its pixels are those of the whole page
    /// drawn, but where anti-aliasing rounds the other way.
    pub fn render_rect(&mut self, page: &Page, ppi: f64, level: Level, rect: PixelRect) -> Image {
        self.renders += 1;
        let (width, height) = (
            rect.width.clamp(1, u32::from(u16::MAX)) as u16,
            rect.height.clamp(1, u32::from(u16::MAX)) as u16,
        );
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
        let whole = (rect.x, rect.y) == (0, 0)
            && rect.width >= u32::from(pixels(page.width, ppi))
            && rect.height >= u32::from(pixels(page.height, ppi));
        // The rectangle in layout units, and a pixel around it, to leave
        // out the items that don't reach it.
        let area = (!whole).then(|| {
            let units = |v: u32| (f64::from(v) / scale).floor() as i64;
            let pixel = (1.0 / scale).ceil() as i64;
            scene::Rect::new(
                units(rect.x),
                units(rect.y),
                units(rect.width),
                units(rect.height),
            )
            .outset(2 * pixel)
        });
        ctx.set_fill_rule(Fill::NonZero);
        let mut drawer = Drawer {
            ctx: &mut ctx,
            resources: &mut resources,
            images: &mut self.images,
            renders: self.renders,
            fonts: HashMap::new(),
            scale,
            offset: (f64::from(rect.x), f64::from(rect.y)),
        };
        for item in &page.items {
            if let Some(area) = area
                && !item
                    .bounds()
                    .is_some_and(|b| b.intersection(area).is_some())
            {
                continue;
            }
            drawer.item(item, None);
        }
        ctx.flush();
        let mut pixmap = Pixmap::new(width, height);
        ctx.render(&mut pixmap, &mut resources);
        let renders = self.renders;
        self.images
            .retain(|_, cached| renders - cached.used < FORGET_AFTER);
        // The pixmap's own bytes, made straight where they're translucent:
        // no copy of the whole image, which costs more than drawing it.
        let mut pixels = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
        for pixel in pixels.as_chunks_mut::<4>().0 {
            if !matches!(pixel[3], 0 | 255) {
                *pixel = straight(*pixel);
            }
        }
        Image {
            width: u32::from(width),
            height: u32::from(height),
            pixels,
        }
    }
}

/// What drawing a page's items needs.
struct Drawer<'a> {
    ctx: &'a mut RenderContext,
    resources: &'a mut Resources,
    images: &'a mut HashMap<(usize, usize), Cached>,
    renders: u64,
    fonts: HashMap<(usize, u32), FontData>,
    /// Pixels to a layout unit.
    scale: f64,
    /// Where the context's origin is on the page, in pixels. Outside
    /// groups, coordinates are moved by it themselves; inside them, the
    /// transform moves them.
    offset: (f64, f64),
}

impl Drawer<'_> {
    /// Draws `item`. `transform` is the group's it's in, if any: from its
    /// coordinates in pixels to the context's.
    fn item(&mut self, item: &Item, transform: Option<Affine>) {
        let scale = self.scale;
        let at = |v: i64| v as f64 * scale;
        let shift = if transform.is_some() {
            (0.0, 0.0)
        } else {
            self.offset
        };
        match item {
            Item::Fill { path, color } => {
                self.ctx.set_paint(paint(*color));
                self.ctx.fill_path(&bez(path, scale, shift));
            }
            Item::Stroke { path, width, color } => {
                self.ctx.set_paint(paint(*color));
                self.ctx.set_stroke(Stroke::new(at(*width)));
                self.ctx.stroke_path(&bez(path, scale, shift));
            }
            Item::Glyphs(run) => {
                let font = self
                    .fonts
                    .entry(run.font.key())
                    .or_insert_with(|| {
                        FontData::new(Blob::new(Arc::clone(&run.font.data)), run.font.index)
                    })
                    .clone();
                self.ctx.set_paint(paint(run.color));
                let mut builder = self
                    .ctx
                    .glyph_run(self.resources, &font)
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
                    x: (at(g.at.x) - shift.0) as f32,
                    y: (at(g.at.y) - shift.1) as f32,
                }));
            }
            Item::Image { image, rect, .. } => {
                if rect.is_empty() {
                    return;
                }
                let (x, y) = (at(rect.x) - shift.0, at(rect.y) - shift.1);
                let (w, h) = (at(rect.width), at(rect.height));
                // How large it's drawn on the canvas, through the group's
                // transform if it's in one.
                let [a, b, c, d, _, _] = transform.unwrap_or(Affine::IDENTITY).as_coeffs();
                let index = image.level_for(w * a.hypot(b), h * c.hypot(d));
                let renders = self.renders;
                let cached = self.images.entry((image.key(), index)).or_insert_with(|| {
                    let level = &image.levels()[index];
                    Cached {
                        _image: image.clone(),
                        pixmap: Arc::new(Pixmap::from_parts(
                            level.pixels.clone(),
                            level.width as u16,
                            level.height as u16,
                            PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, !image.opaque()),
                        )),
                        used: renders,
                    }
                });
                cached.used = renders;
                let pixmap = cached.pixmap.clone();
                let (lw, lh) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                self.ctx.set_paint(vello_cpu::Image {
                    image: ImageSource::Pixmap(pixmap),
                    sampler: sampler(),
                });
                self.ctx.set_paint_transform(
                    Affine::translate((x, y)) * Affine::scale_non_uniform(w / lw, h / lh),
                );
                self.ctx.fill_rect(&kurbo::Rect::new(x, y, x + w, y + h));
                self.ctx.reset_paint_transform();
            }
            Item::Group(group) => {
                let outer = transform.unwrap_or(Affine::IDENTITY);
                let [ga, gb, gc, gd, ge, gf] = group.transform.0;
                let own = Affine::new([ga, gb, gc, gd, ge * scale, gf * scale]);
                // Inside the group the offset goes into the transform.
                let base = transform.unwrap_or(Affine::translate((-self.offset.0, -self.offset.1)));
                let inner = base * own;
                if let Some(clip) = &group.clip {
                    self.ctx.set_transform(outer);
                    self.ctx.push_clip_path(&bez(clip, scale, shift));
                }
                for item in &group.items {
                    self.ctx.set_transform(inner);
                    self.item(item, Some(inner));
                }
                self.ctx.set_transform(outer);
                if group.clip.is_some() {
                    self.ctx.pop_clip();
                }
            }
        }
    }
}

/// How images are sampled: bilinearly, their edge pixels held beyond them.
fn sampler() -> ImageSampler {
    ImageSampler {
        x_extend: Extend::Pad,
        y_extend: Extend::Pad,
        quality: ImageQuality::Medium,
        alpha: 1.0,
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

/// `path` in pixels, less `shift`.
fn bez(path: &Path, scale: f64, shift: (f64, f64)) -> BezPath {
    let p = |q: scene::Point| (q.x as f64 * scale - shift.0, q.y as f64 * scale - shift.1);
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
    use scene::{Group, Rect, Transform};

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

    /// A 2 by 2 image: red, green, blue and white.
    fn four() -> scene::Image {
        let rgba = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ]
        .concat();
        scene::Image::new(2, 2, rgba).unwrap()
    }

    #[test]
    fn an_image_fills_its_rectangle() {
        let mut page = Page::new(20 * PX, 20 * PX);
        page.fill_background(Color::BLACK);
        page.items.push(Item::Image {
            image: four(),
            rect: Rect::new(4 * PX, 4 * PX, 8 * PX, 8 * PX),
            alt: None,
            source: None,
        });
        let image = render(&page, 96.0, Level::Portable);
        // Each image pixel covers four by four; their middles are its own
        // colour, bilinear sampling blends only where they meet.
        assert_eq!(pixel(&image, 5, 5), [255, 0, 0, 255]);
        assert_eq!(pixel(&image, 10, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, 5, 10), [0, 0, 255, 255]);
        assert_eq!(pixel(&image, 10, 10), [255, 255, 255, 255]);
        // Nothing outside it.
        for (x, y) in [(3, 3), (12, 8), (8, 12), (3, 8)] {
            assert_eq!(pixel(&image, x, y), [0, 0, 0, 255], "({x}, {y})");
        }
    }

    #[test]
    fn a_group_clips_and_transforms_its_items() {
        let square = Item::Fill {
            path: Path::rect(0, 0, 4 * PX, 4 * PX),
            color: Color::WHITE,
        };
        let mut page = Page::new(20 * PX, 20 * PX);
        page.fill_background(Color::BLACK);
        page.items.push(Item::Group(Group {
            clip: Some(Path::rect(0, 0, 20 * PX, 10 * PX)),
            transform: Transform::scale(2.0, 4.0).then(Transform::translate(2 * PX, 2 * PX)),
            items: vec![square],
        }));
        let image = render(&page, 96.0, Level::Portable);
        // The square is 8 by 16 at (2, 2), cut at y = 10.
        for y in 0..20 {
            for x in 0..20 {
                let inside = (2..10).contains(&x) && (2..10).contains(&y);
                let want = if inside { [255; 4] } else { [0, 0, 0, 255] };
                assert_eq!(pixel(&image, x, y), want, "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_rect_of_the_page_draws_what_the_whole_does_there() {
        let mut page = Page::new(40 * PX, 30 * PX);
        page.fill_background(Color::WHITE);
        page.items.push(Item::Fill {
            path: Path::rect(3 * PX + 17, 5 * PX, 20 * PX, 7 * PX + 300),
            color: Color::rgb(200, 30, 60),
        });
        page.items.push(Item::Group(Group {
            clip: Some(Path::rect(PX, PX, 30 * PX, 25 * PX)),
            transform: Transform([0.8, 0.6, -0.6, 0.8, 20.0 * PX as f64, 2.0 * PX as f64]),
            items: vec![Item::Image {
                image: four(),
                rect: Rect::new(0, 0, 12 * PX, 12 * PX),
                alt: None,
                source: None,
            }],
        }));
        let mut renderer = Renderer::new();
        let whole = renderer.render(&page, 96.0, Level::Portable);
        let rect = PixelRect {
            x: 7,
            y: 4,
            width: 21,
            height: 13,
        };
        let part = renderer.render_rect(&page, 96.0, Level::Portable, rect);
        assert_eq!((part.width, part.height), (21, 13));
        for y in 0..rect.height {
            for x in 0..rect.width {
                let (a, b) = (pixel(&part, x, y), pixel(&whole, rect.x + x, rect.y + y));
                let worst = a.iter().zip(b).map(|(a, b)| a.abs_diff(b)).max().unwrap();
                assert!(worst <= 1, "({x}, {y}): {a:?} against {b:?}");
            }
        }
    }
}
