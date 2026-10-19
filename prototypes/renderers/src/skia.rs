//! tiny-skia: a software renderer, after Skia's CPU backend. Glyphs are
//! drawn from their outlines, read with skrifa, as the other candidates do.

use kurbo::{BezPath, PathEl};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use crate::corpus::{self, Fonts, Item, Page, Rgba};

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

fn path(bez: &BezPath) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for el in bez.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, b) => pb.quad_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32),
            PathEl::CurveTo(a, b, c) => pb.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, c.x as f32, c.y as f32,
            ),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

fn paint(p: &corpus::Paint) -> Paint<'static> {
    let stops = |s: &[(f32, Rgba)]| s.iter().map(|&(t, c)| GradientStop::new(t, color(c))).collect();
    let shader = match p {
        corpus::Paint::Solid(c) => Shader::SolidColor(color(*c)),
        corpus::Paint::Linear { start, end, stops: s } => LinearGradient::new(
            Point::from_xy(start.x as f32, start.y as f32),
            Point::from_xy(end.x as f32, end.y as f32),
            stops(s),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .expect("a valid gradient"),
        corpus::Paint::Radial { center, radius, stops: s } => {
            let c = Point::from_xy(center.x as f32, center.y as f32);
            RadialGradient::new(c, 0.0, c, *radius, stops(s), SpreadMode::Pad, Transform::identity())
                .expect("a valid gradient")
        }
    };
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

/// Collects a glyph's outline, flipped from font units' y-up into the
/// page's y-down and placed at its origin.
struct Pen<'a> {
    pb: &'a mut PathBuilder,
    x: f32,
    y: f32,
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x, self.y - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x, self.y - y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.pb.quad_to(self.x + cx0, self.y - cy0, self.x + x, self.y - y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + cx0, self.y - cy0, self.x + cx1, self.y - cy1, self.x + x, self.y - y,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// Renders `page`; returns RGBA8 pixels, premultiplied.
pub fn render(page: &Page, fonts: &Fonts) -> Vec<u8> {
    let mut pixmap = Pixmap::new(page.width as u32, page.height as u32).expect("a page size");
    for item in &page.items {
        match item {
            Item::Fill { path: p, paint: pt } => {
                if let Some(p) = path(p) {
                    pixmap.fill_path(&p, &paint(pt), FillRule::Winding, Transform::identity(), None);
                }
            }
            Item::Stroke { path: p, width, paint: pt } => {
                if let Some(p) = path(p) {
                    let stroke = Stroke { width: *width as f32, ..Stroke::default() };
                    pixmap.stroke_path(&p, &paint(pt), &stroke, Transform::identity(), None);
                }
            }
            Item::Glyphs { run } => {
                let font = fonts.font(run.font);
                let outlines = font.outline_glyphs();
                let settings = || DrawSettings::unhinted(Size::new(run.size), LocationRef::default());
                let mut pb = PathBuilder::new();
                for &(id, x, y) in &run.glyphs {
                    if let Some(glyph) = outlines.get(GlyphId::new(id)) {
                        let mut pen = Pen { pb: &mut pb, x, y };
                        let _ = glyph.draw(settings(), &mut pen);
                    }
                }
                if let Some(p) = pb.finish() {
                    let pt = paint(&corpus::Paint::Solid(run.color));
                    pixmap.fill_path(&p, &pt, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }
    pixmap.take()
}
