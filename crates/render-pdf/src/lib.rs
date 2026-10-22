//! PDF output of display lists, with fonts embedded and subset; printing goes
//! through it (docs/ARCHITECTURE.md, section 10). It writes with krilla.
//!
//! [`pdf`] writes [`scene::Page`]s as a tagged PDF document, a page each,
//! with the document's [`scene::Structure`]: its paragraphs and headings in
//! reading order, each holding the text that belongs to it, wherever it's
//! drawn, with its language.
//!
//! Invariants:
//!
//! - Text stays text: glyphs are drawn from fonts embedded in the file,
//!   subset to the glyphs used, with the text each shows, so the PDF can
//!   be searched and copied from.
//! - Text is written in the order it reads, right-to-left runs included,
//!   so it reads back in that order in any reader; fills and strokes are
//!   artifacts, outside the reading order.
//! - Each page is its display list's size, 12 700 layout units to the
//!   point.
//! - The same pages give the same bytes, every time.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::ops::Range;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, Stroke};
use krilla::tagging::{
    Artifact, ArtifactType, ContentTag, Identifier, SpanTag, Tag, TagGroup, TagKind, TagTree,
};
use krilla::text::{Font, GlyphId};
use krilla::{Data, Document};
use scene::{Color, Item, Page, Path, Role, Segment, Structure};

/// Why a PDF couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A page too small or too large for PDF.
    #[error("page {page} is {width} by {height} layout units, which PDF can't hold")]
    PageSize {
        /// The page's index.
        page: usize,
        /// Its width.
        width: i64,
        /// Its height.
        height: i64,
    },
    /// A font that can't be read for embedding.
    #[error("page {page} uses a font that can't be embedded")]
    Font {
        /// The page's index.
        page: usize,
    },
    /// The PDF writer refused the document.
    #[error("writing the PDF: {0}")]
    Pdf(String),
}

/// Layout units in a point.
const POINT: f32 = 12_700.0;

fn pt(units: i64) -> f32 {
    units as f32 / POINT
}

/// `pages` as a tagged PDF document of `structure`, titled `title` if
/// given. Glyph runs whose source isn't in the structure become paragraphs
/// of their own, after it.
pub fn pdf(pages: &[Page], structure: &Structure, title: Option<&str>) -> Result<Vec<u8>, Error> {
    let mut document = Document::new();
    let mut metadata = krilla::metadata::Metadata::new();
    if let Some(title) = title {
        metadata = metadata.title(title.to_string());
    }
    if let Some(language) = &structure.language {
        metadata = metadata.language(language.clone());
    }
    document.set_metadata(metadata);
    // What each block holds, by where in its text it starts.
    let mut blocks: Vec<Vec<(usize, Identifier)>> = vec![Vec::new(); structure.blocks.len()];
    let mut loose: Vec<Identifier> = Vec::new();
    let mut fonts: HashMap<(usize, u32), Font> = HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        let settings =
            PageSettings::from_wh(pt(page.width), pt(page.height)).ok_or(Error::PageSize {
                page: index,
                width: page.width,
                height: page.height,
            })?;
        let mut pdf_page = document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        for item in &page.items {
            match item {
                Item::Fill { path, color } => {
                    let Some(path) = krilla_path(path) else {
                        continue;
                    };
                    surface.start_tagged(ContentTag::Artifact(Artifact::new(
                        ArtifactType::Layout,
                        None,
                    )));
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*color)));
                    surface.draw_path(&path);
                    surface.end_tagged();
                }
                Item::Stroke { path, width, color } => {
                    let Some(path) = krilla_path(path) else {
                        continue;
                    };
                    surface.start_tagged(ContentTag::Artifact(Artifact::new(
                        ArtifactType::Layout,
                        None,
                    )));
                    surface.set_fill(None);
                    surface.set_stroke(Some(Stroke {
                        paint: rgb::Color::new(color.r, color.g, color.b).into(),
                        width: pt(*width),
                        opacity: opacity(*color),
                        ..Stroke::default()
                    }));
                    surface.draw_path(&path);
                    surface.end_tagged();
                }
                Item::Glyphs(run) => {
                    let (Some(left), Some(right)) = (run.glyphs.first(), run.glyphs.last()) else {
                        continue;
                    };
                    let font = match fonts.get(&run.font.key()) {
                        Some(font) => font.clone(),
                        None => {
                            let font = Font::new(Data::from(run.font.data.clone()), run.font.index)
                                .ok_or(Error::Font { page: index })?;
                            fonts.insert(run.font.key(), font.clone());
                            font
                        }
                    };
                    let baseline = pt(run.baseline);
                    // In the order they read: a right-to-left run, whose
                    // text runs backwards along it, is written from its
                    // right end.
                    let mut order: Vec<&scene::Glyph> = run.glyphs.iter().collect();
                    if right.text.start < left.text.start {
                        order.reverse();
                    }
                    let first = order[0];
                    // In ems: krilla scales them by the size itself.
                    let em = run.size.max(1) as f32;
                    let glyphs: Vec<PdfGlyph> = order
                        .iter()
                        .enumerate()
                        .map(|(i, g)| PdfGlyph {
                            id: GlyphId::new(g.id),
                            text: g.text.clone(),
                            advance: order
                                .get(i + 1)
                                .map_or(0.0, |next| (next.at.x - g.at.x) as f32 / em),
                            rise: (run.baseline - g.at.y) as f32 / em,
                        })
                        .collect();
                    let id = surface.start_tagged(ContentTag::Span(SpanTag {
                        lang: run.language.as_deref(),
                        ..SpanTag::empty()
                    }));
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(run.color)));
                    if run.oblique {
                        // Lean the tops right, about the baseline; the page
                        // runs down, as the display list does.
                        let slant = scene::OBLIQUE_SLANT as f32;
                        surface.push_transform(&Transform::from_row(
                            1.0,
                            0.0,
                            -slant,
                            1.0,
                            slant * baseline,
                            0.0,
                        ));
                    }
                    surface.draw_glyphs(
                        Point::from_xy(pt(first.at.x), baseline),
                        &glyphs,
                        font,
                        &run.text,
                        pt(run.size),
                        false,
                    );
                    if run.oblique {
                        surface.pop();
                    }
                    surface.end_tagged();
                    match run.source {
                        Some(source) if (source.block as usize) < blocks.len() => {
                            blocks[source.block as usize].push((source.offset, id));
                        }
                        _ => loose.push(id),
                    }
                }
            }
        }
        surface.finish();
        pdf_page.finish();
    }
    let mut tree = TagTree::new().with_lang(structure.language.clone());
    for (role, mut held) in structure.blocks.iter().zip(blocks) {
        if held.is_empty() {
            continue;
        }
        // In reading order; a stable sort keeps the drawing order of runs
        // that start together.
        held.sort_by_key(|(offset, _)| *offset);
        let kind: TagKind = match role {
            Role::Paragraph => Tag::P.into(),
            Role::Heading(level) => {
                let level =
                    NonZeroU16::new(u16::from(*level).clamp(1, 6)).unwrap_or(NonZeroU16::MIN);
                Tag::Hn(level, None).into()
            }
        };
        let mut group = TagGroup::new(kind);
        for (_, id) in held {
            group.push(id);
        }
        tree.push(group);
    }
    for id in loose {
        tree.push(TagGroup::with_children(Tag::P, vec![id.into()]));
    }
    document.set_tag_tree(tree);
    document.finish().map_err(|e| Error::Pdf(format!("{e:?}")))
}

fn opacity(color: Color) -> NormalizedF32 {
    NormalizedF32::new(f32::from(color.a) / 255.0).unwrap_or(NormalizedF32::ONE)
}

fn fill(color: Color) -> Fill {
    Fill {
        paint: rgb::Color::new(color.r, color.g, color.b).into(),
        opacity: opacity(color),
        rule: FillRule::NonZero,
    }
}

/// `path` in points, or `None` if it's empty.
fn krilla_path(path: &Path) -> Option<krilla::geom::Path> {
    let mut builder = PathBuilder::new();
    for segment in &path.0 {
        let p = |q: scene::Point| (pt(q.x), pt(q.y));
        match *segment {
            Segment::MoveTo(a) => {
                let (x, y) = p(a);
                builder.move_to(x, y);
            }
            Segment::LineTo(a) => {
                let (x, y) = p(a);
                builder.line_to(x, y);
            }
            Segment::QuadTo(a, b) => {
                let ((x1, y1), (x, y)) = (p(a), p(b));
                builder.quad_to(x1, y1, x, y);
            }
            Segment::CubicTo(a, b, c) => {
                let ((x1, y1), (x2, y2), (x, y)) = (p(a), p(b), p(c));
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            Segment::Close => builder.close(),
        }
    }
    builder.finish()
}

/// A glyph as krilla takes it: advances and offsets in ems, which it
/// scales by the size it asks at.
struct PdfGlyph {
    id: GlyphId,
    text: Range<usize>,
    /// To the next glyph's origin.
    advance: f32,
    /// Above the run's baseline.
    rise: f32,
}

impl krilla::text::Glyph for PdfGlyph {
    fn glyph_id(&self) -> GlyphId {
        self.id
    }

    fn text_range(&self) -> Range<usize> {
        self.text.clone()
    }

    fn x_advance(&self, size: f32) -> f32 {
        self.advance * size
    }

    fn x_offset(&self, _size: f32) -> f32 {
        0.0
    }

    fn y_offset(&self, size: f32) -> f32 {
        self.rise * size
    }

    fn y_advance(&self, _size: f32) -> f32 {
        0.0
    }

    fn location(&self) -> Option<krilla::surface::Location> {
        None
    }
}
