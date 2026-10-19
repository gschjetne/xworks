//! The pages every candidate renders: a renderer-neutral description of
//! fills, strokes, gradients and glyph runs, much as `scene` will describe
//! them, and the three pages of the corpus.
//!
//! Text is laid out simply, a glyph per character with its advance and
//! first-fit line breaks: shaping is stage 5's work, and this is enough to
//! load the renderers the way documents will.

use std::path::Path;
use std::sync::Arc;

use kurbo::{BezPath, Circle, Point, Rect, Shape};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

/// Straight-alpha RGBA.
pub type Rgba = [u8; 4];

/// How a shape is painted.
#[derive(Clone)]
pub enum Paint {
    Solid(Rgba),
    Linear {
        start: Point,
        end: Point,
        stops: Vec<(f32, Rgba)>,
    },
    Radial {
        center: Point,
        radius: f32,
        stops: Vec<(f32, Rgba)>,
    },
}

/// One thing drawn.
pub enum Item {
    Fill { path: BezPath, paint: Paint },
    Stroke { path: BezPath, width: f64, paint: Paint },
    Glyphs { run: GlyphRun },
}

/// Glyphs of one font, size and color, positioned on their baselines.
pub struct GlyphRun {
    pub font: usize,
    pub size: f32,
    pub color: Rgba,
    /// Glyph ID, x and y of each glyph's origin.
    pub glyphs: Vec<(u32, f32, f32)>,
}

/// A page to render.
pub struct Page {
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
    pub items: Vec<Item>,
}

impl Page {
    pub fn glyph_count(&self) -> usize {
        self.items
            .iter()
            .map(|item| match item {
                Item::Glyphs { run } => run.glyphs.len(),
                _ => 0,
            })
            .sum()
    }
}

/// The corpus's fonts, by index: serif, sans, sans bold.
pub struct Fonts {
    pub data: Vec<Arc<Vec<u8>>>,
}

pub const SERIF: usize = 0;
pub const SANS: usize = 1;
pub const SANS_BOLD: usize = 2;

impl Fonts {
    pub fn load(dir: &Path) -> std::io::Result<Fonts> {
        let names = [
            "NotoSerif-Regular.ttf",
            "NotoSans-Regular.ttf",
            "NotoSans-Bold.ttf",
        ];
        let mut data = Vec::new();
        for name in names {
            data.push(Arc::new(std::fs::read(dir.join(name))?));
        }
        Ok(Fonts { data })
    }

    pub fn font(&self, index: usize) -> FontRef<'_> {
        FontRef::new(&self.data[index]).expect("the corpus's fonts are valid")
    }

    /// Lays out `text` from (x, y) in lines no wider than `width`, a glyph
    /// per character, breaking at spaces; returns the run and the y below
    /// its last line.
    fn paragraph(
        &self,
        font: usize,
        size: f32,
        color: Rgba,
        text: &str,
        (x, y): (f32, f32),
        width: f32,
        line_height: f32,
    ) -> (GlyphRun, f32) {
        let f = self.font(font);
        let charmap = f.charmap();
        let metrics = f.glyph_metrics(Size::new(size), LocationRef::default());
        let advance = |c: char| {
            let id = charmap.map(c).unwrap_or_default();
            (id.to_u32(), metrics.advance_width(id).unwrap_or(0.0))
        };
        let mut glyphs = Vec::new();
        let mut pen_y = y;
        let mut line: Vec<(u32, f32)> = Vec::new();
        let mut line_width = 0.0;
        let flush = |line: &mut Vec<(u32, f32)>, glyphs: &mut Vec<(u32, f32, f32)>, pen_y: f32| {
            let mut pen_x = x;
            for (id, adv) in line.drain(..) {
                glyphs.push((id, pen_x, pen_y));
                pen_x += adv;
            }
        };
        for word in text.split(' ') {
            let shaped: Vec<_> = word.chars().map(advance).collect();
            let word_width: f32 = shaped.iter().map(|g| g.1).sum();
            let space = advance(' ');
            if !line.is_empty() && line_width + space.1 + word_width > width {
                flush(&mut line, &mut glyphs, pen_y);
                pen_y += line_height;
                line_width = 0.0;
            }
            if !line.is_empty() {
                line.push(space);
                line_width += space.1;
            }
            line.extend(shaped);
            line_width += word_width;
        }
        flush(&mut line, &mut glyphs, pen_y);
        (
            GlyphRun {
                font,
                size,
                color,
                glyphs,
            },
            pen_y + line_height,
        )
    }
}

const TEXT: &str = "An office suite is mostly text, set in type, broken into \
lines and pages, and drawn again whenever something moves. Every frame of \
scrolling redraws a screenful of glyphs, so the renderer's speed with text \
decides how fast the whole application feels. Shapes, gradients and images \
matter too, on slides and in drawings, but far less often. A good renderer \
draws all of them the same way on every computer, so a document looks the \
same wherever it is opened, and its pages print as they appear on screen. \
Fast enough is a frame: sixteen milliseconds at sixty frames a second, and \
less on displays that refresh faster than that.";

/// The three pages.
pub fn corpus(fonts: &Fonts) -> Vec<Page> {
    vec![text_page(fonts), grid_page(fonts), slide_page(fonts)]
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    Rect::new(x, y, x + w, y + h).to_path(0.1)
}

/// An A4 page of body text at 192 dpi (twice 96): a heading and eleven
/// paragraphs of 11-point serif, about 5,000 glyphs.
fn text_page(fonts: &Fonts) -> Page {
    let (width, height) = (1588u16, 2245u16);
    let mut items = vec![Item::Fill {
        path: rect(0.0, 0.0, width as f64, height as f64),
        paint: Paint::Solid([255, 255, 255, 255]),
    }];
    let margin = 189.0; // 2.5 cm
    let measure = width as f32 - 2.0 * margin;
    let (heading, mut y) = fonts.paragraph(
        SANS_BOLD,
        48.0,
        [20, 20, 30, 255],
        "The renderer decides how the suite feels",
        (margin, margin + 48.0),
        measure,
        64.0,
    );
    items.push(Item::Glyphs { run: heading });
    y += 24.0;
    for _ in 0..11 {
        let (run, below) = fonts.paragraph(
            SERIF,
            29.33,
            [30, 30, 30, 255],
            TEXT,
            (margin, y),
            measure,
            40.0,
        );
        items.push(Item::Glyphs { run });
        y = below + 16.0;
    }
    Page {
        name: "text",
        width,
        height,
        items,
    }
}

/// A spreadsheet's grid at 1920 by 1080: 26 columns by 50 rows of 1-pixel
/// lines, shaded headers, and a number in every cell, about 9,000 glyphs
/// at 12 pixels.
fn grid_page(fonts: &Fonts) -> Page {
    let (width, height) = (1920u16, 1080u16);
    let mut items = vec![Item::Fill {
        path: rect(0.0, 0.0, width as f64, height as f64),
        paint: Paint::Solid([255, 255, 255, 255]),
    }];
    let (cw, ch, header_w) = (72.0, 21.0, 48.0);
    items.push(Item::Fill {
        path: rect(0.0, 0.0, width as f64, ch),
        paint: Paint::Solid([238, 240, 244, 255]),
    });
    items.push(Item::Fill {
        path: rect(0.0, 0.0, header_w, height as f64),
        paint: Paint::Solid([238, 240, 244, 255]),
    });
    let mut lines = BezPath::new();
    for col in 0..=26 {
        let x = header_w + col as f64 * cw;
        lines.extend(rect(x, 0.0, 1.0, height as f64));
    }
    for row in 0..=51 {
        lines.extend(rect(0.0, row as f64 * ch, width as f64, 1.0));
    }
    items.push(Item::Fill {
        path: lines,
        paint: Paint::Solid([208, 212, 220, 255]),
    });
    let f = fonts.font(SANS);
    let charmap = f.charmap();
    let metrics = f.glyph_metrics(Size::new(12.0), LocationRef::default());
    let mut glyphs = Vec::new();
    let mut seed: u32 = 12345;
    for row in 1..51 {
        for col in 0..26 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let text = format!("{}.{:02}", seed % 100_000, seed / 7 % 100);
            let shaped: Vec<_> = text
                .chars()
                .map(|c| {
                    let id = charmap.map(c).unwrap_or_default();
                    (id.to_u32(), metrics.advance_width(id).unwrap_or(0.0))
                })
                .collect();
            let text_width: f32 = shaped.iter().map(|g| g.1).sum();
            let mut x = (header_w + (col + 1) as f64 * cw) as f32 - 4.0 - text_width;
            let y = (row as f64 * ch + 15.0) as f32;
            for (id, adv) in shaped {
                glyphs.push((id, x, y));
                x += adv;
            }
        }
    }
    items.push(Item::Glyphs {
        run: GlyphRun {
            font: SANS,
            size: 12.0,
            color: [25, 25, 25, 255],
            glyphs,
        },
    });
    Page {
        name: "grid",
        width,
        height,
        items,
    }
}

/// A slide at 1920 by 1080: a gradient background, a radial highlight,
/// translucent curved shapes, strokes, a title and three bullets.
fn slide_page(fonts: &Fonts) -> Page {
    let (width, height) = (1920u16, 1080u16);
    let mut items = vec![Item::Fill {
        path: rect(0.0, 0.0, width as f64, height as f64),
        paint: Paint::Linear {
            start: Point::new(0.0, 0.0),
            end: Point::new(1920.0, 1080.0),
            stops: vec![(0.0, [18, 32, 74, 255]), (1.0, [64, 18, 82, 255])],
        },
    }];
    items.push(Item::Fill {
        path: Circle::new((1500.0, 300.0), 420.0).to_path(0.1),
        paint: Paint::Radial {
            center: Point::new(1500.0, 300.0),
            radius: 420.0,
            stops: vec![(0.0, [255, 200, 120, 160]), (1.0, [255, 200, 120, 0])],
        },
    });
    for i in 0..12 {
        let t = i as f64;
        let mut blob = BezPath::new();
        blob.move_to((200.0 + t * 120.0, 900.0));
        blob.curve_to(
            (260.0 + t * 120.0, 600.0 - t * 20.0),
            (420.0 + t * 110.0, 700.0),
            (480.0 + t * 120.0, 960.0),
        );
        blob.quad_to((340.0 + t * 120.0, 1060.0), (200.0 + t * 120.0, 900.0));
        blob.close_path();
        items.push(Item::Fill {
            path: blob,
            paint: Paint::Solid([
                (80 + i * 14) as u8,
                (200 - i * 9) as u8,
                (230 - i * 6) as u8,
                110,
            ]),
        });
    }
    for i in 0..8 {
        let t = i as f64;
        let mut wave = BezPath::new();
        wave.move_to((0.0, 520.0 + t * 18.0));
        wave.curve_to(
            (480.0, 380.0 + t * 30.0),
            (1200.0, 760.0 - t * 25.0),
            (1920.0, 560.0 + t * 12.0),
        );
        items.push(Item::Stroke {
            path: wave,
            width: 2.0 + t * 0.75,
            paint: Paint::Solid([255, 255, 255, 60 + i as u8 * 12]),
        });
    }
    let (title, _) = fonts.paragraph(
        SANS_BOLD,
        84.0,
        [255, 255, 255, 255],
        "One renderer for every page",
        (140.0, 220.0),
        1600.0,
        100.0,
    );
    items.push(Item::Glyphs { run: title });
    let mut y = 360.0;
    for bullet in [
        "Text, shapes and gradients drawn alike",
        "The same pixels on every computer",
        "Fast enough to redraw every frame",
    ] {
        let (run, below) = fonts.paragraph(
            SANS,
            44.0,
            [230, 232, 245, 255],
            bullet,
            (180.0, y),
            1500.0,
            60.0,
        );
        items.push(Item::Glyphs { run });
        y = below + 10.0;
    }
    Page {
        name: "slide",
        width,
        height,
        items,
    }
}
