//! PDF output of display lists, with fonts embedded and subset; printing goes
//! through it (docs/ARCHITECTURE.md, section 10). It writes with krilla.
//!
//! [`pdf`] writes [`scene::Page`]s as a tagged PDF document, a page each,
//! with the document's [`scene::Structure`]: its paragraphs, headings and
//! figures in reading order, each holding the text and images that belong
//! to it, wherever they're drawn, with its language.
//!
//! Invariants:
//!
//! - Text stays text: glyphs are drawn from fonts embedded in the file,
//!   subset to the glyphs used, with the text each shows, so the PDF can
//!   be searched and copied from.
//! - Text is written in the order it reads, right-to-left runs included,
//!   so it reads back in that order in any reader; fills and strokes are
//!   artifacts, outside the reading order.
//! - Images are figures, with their alternative text, written as the same
//!   sRGB pixels the screen draws, so print matches the screen.
//! - Groups are clips and transforms in the PDF too.
//! - Each page is its display list's size, 12 700 layout units to the
//!   point.
//! - The same pages give the same bytes, every time.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU16;
use std::ops::Range;
use std::sync::Arc;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::{BitsPerComponent, CustomImage, ImageColorspace};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, Stroke};
use krilla::surface::Surface;
use krilla::tagging::{
    Artifact, ArtifactType, BBox, ContentTag, Identifier, Node, SpanTag, Tag, TagGroup, TagKind,
    TagTree,
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

/// What a block of the structure holds, by where in its text each part
/// starts.
enum Held {
    /// Text.
    Text(Identifier),
    /// An image, with what it shows in words, and where it is if it's on
    /// one page.
    Picture {
        id: Identifier,
        alt: Option<String>,
        bbox: Option<BBox>,
    },
}

impl Held {
    /// The part as a node of a block of `role`: a picture in a figure is
    /// the figure's own content, and a figure of its own elsewhere.
    fn node(self, role: Option<Role>) -> Node {
        match self {
            Held::Text(id) => id.into(),
            Held::Picture { id, .. } if role == Some(Role::Figure) => id.into(),
            Held::Picture { id, alt, bbox } => {
                TagGroup::with_children(Tag::Figure(alt).with_bbox(bbox), vec![id.into()]).into()
            }
        }
    }
}

/// What writing the pages gathers as it goes.
struct Writer {
    blocks: Vec<Vec<(usize, Held)>>,
    loose: Vec<Held>,
    fonts: HashMap<(usize, u32), Font>,
    images: HashMap<usize, krilla::image::Image>,
    page: usize,
}

/// `pages` as a tagged PDF document of `structure`, titled `title` if
/// given. Glyph runs and images whose source isn't in the structure become
/// paragraphs and figures of their own, after it.
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
    let mut writer = Writer {
        blocks: (0..structure.blocks.len()).map(|_| Vec::new()).collect(),
        loose: Vec::new(),
        fonts: HashMap::new(),
        images: HashMap::new(),
        page: 0,
    };
    for (index, page) in pages.iter().enumerate() {
        let settings =
            PageSettings::from_wh(pt(page.width), pt(page.height)).ok_or(Error::PageSize {
                page: index,
                width: page.width,
                height: page.height,
            })?;
        let mut pdf_page = document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        writer.page = index;
        writer.items(&mut surface, &page.items, scene::Transform::IDENTITY)?;
        surface.finish();
        pdf_page.finish();
    }
    let mut tree = TagTree::new().with_lang(structure.language.clone());
    for (role, mut held) in structure.blocks.iter().zip(writer.blocks) {
        if held.is_empty() {
            continue;
        }
        // In reading order; a stable sort keeps the drawing order of parts
        // that start together.
        held.sort_by_key(|(offset, _)| *offset);
        let kind: TagKind = match role {
            Role::Paragraph => Tag::P.into(),
            Role::Heading(level) => {
                let level =
                    NonZeroU16::new(u16::from(*level).clamp(1, 6)).unwrap_or(NonZeroU16::MIN);
                Tag::Hn(level, None).into()
            }
            Role::Figure => {
                let picture = held.iter().find_map(|(_, h)| match h {
                    Held::Picture { alt, bbox, .. } => Some((alt.clone(), *bbox)),
                    Held::Text(_) => None,
                });
                let (alt, bbox) = picture.unwrap_or_default();
                Tag::Figure(alt).with_bbox(bbox).into()
            }
        };
        let mut group = TagGroup::new(kind);
        for (_, h) in held {
            group.push(h.node(Some(*role)));
        }
        tree.push(group);
    }
    for h in writer.loose {
        match h {
            Held::Text(id) => tree.push(TagGroup::with_children(Tag::P, vec![id.into()])),
            picture => tree.push(picture.node(None)),
        }
    }
    document.set_tag_tree(tree);
    document.finish().map_err(|e| Error::Pdf(format!("{e:?}")))
}

impl Writer {
    /// Writes `items`, which `transform` takes to the page's coordinates.
    fn items(
        &mut self,
        surface: &mut Surface<'_>,
        items: &[Item],
        transform: scene::Transform,
    ) -> Result<(), Error> {
        for item in items {
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
                Item::Glyphs(run) => self.glyphs(surface, run)?,
                Item::Image {
                    image,
                    rect,
                    alt,
                    source,
                } => {
                    let Some(size) = Size::from_wh(pt(rect.width), pt(rect.height)) else {
                        continue;
                    };
                    let picture = self
                        .images
                        .entry(image.key())
                        .or_insert_with(|| picture(image))
                        .clone();
                    let id = surface.start_tagged(ContentTag::Other);
                    surface.push_transform(&Transform::from_translate(pt(rect.x), pt(rect.y)));
                    surface.draw_image(picture, size);
                    surface.pop();
                    surface.end_tagged();
                    let on_page = transform.bounds(*rect);
                    let bbox = Rect::from_xywh(
                        pt(on_page.x),
                        pt(on_page.y),
                        pt(on_page.width),
                        pt(on_page.height),
                    )
                    .map(|r| BBox::new(self.page, r));
                    let held = Held::Picture {
                        id,
                        alt: alt.clone(),
                        bbox,
                    };
                    match source {
                        Some(source) if (source.block as usize) < self.blocks.len() => {
                            self.blocks[source.block as usize].push((source.offset, held));
                        }
                        _ => self.loose.push(held),
                    }
                }
                Item::Group(group) => {
                    let clip = group.clip.as_ref().and_then(krilla_path);
                    if let Some(clip) = &clip {
                        surface.push_clip_path(clip, &FillRule::NonZero);
                    }
                    let [a, b, c, d, e, f] = group.transform.0;
                    surface.push_transform(&Transform::from_row(
                        a as f32,
                        b as f32,
                        c as f32,
                        d as f32,
                        (e / f64::from(POINT)) as f32,
                        (f / f64::from(POINT)) as f32,
                    ));
                    self.items(surface, &group.items, group.transform.then(transform))?;
                    surface.pop();
                    if clip.is_some() {
                        surface.pop();
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes a glyph run as a span of text, in the order it reads.
    fn glyphs(&mut self, surface: &mut Surface<'_>, run: &scene::Glyphs) -> Result<(), Error> {
        let (Some(left), Some(right)) = (run.glyphs.first(), run.glyphs.last()) else {
            return Ok(());
        };
        let font = match self.fonts.get(&run.font.key()) {
            Some(font) => font.clone(),
            None => {
                let font = Font::new(Data::from(run.font.data.clone()), run.font.index)
                    .ok_or(Error::Font { page: self.page })?;
                self.fonts.insert(run.font.key(), font.clone());
                font
            }
        };
        let baseline = pt(run.baseline);
        // In the order they read: a right-to-left run, whose text runs
        // backwards along it, is written from its right end.
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
            // Lean the tops right, about the baseline; the page runs down,
            // as the display list does.
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
            Some(source) if (source.block as usize) < self.blocks.len() => {
                self.blocks[source.block as usize].push((source.offset, Held::Text(id)));
            }
            _ => self.loose.push(Held::Text(id)),
        }
        Ok(())
    }
}

/// `image` for krilla: its largest level, straight, with an alpha channel
/// unless it's opaque, sampled smoothly as the screen does.
fn picture(image: &scene::Image) -> krilla::image::Image {
    let (rgb, alpha) = image.straight();
    let pixels = Pixels {
        width: image.width(),
        height: image.height(),
        rgb: Arc::new(rgb),
        alpha: alpha.map(Arc::new),
    };
    krilla::image::Image::from_custom(pixels, true).unwrap_or_else(|_| {
        // from_custom refuses nothing; a picture it did refuse would still
        // be drawn, as one transparent pixel.
        krilla::image::Image::from_rgba8(vec![0; 4], 1, 1)
    })
}

/// An image's pixels as krilla writes them: sRGB, and alpha apart.
#[derive(Clone)]
struct Pixels {
    width: u32,
    height: u32,
    rgb: Arc<Vec<u8>>,
    alpha: Option<Arc<Vec<u8>>>,
}

impl Hash for Pixels {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // krilla hashes an image once, to tell images apart by content.
        self.width.hash(state);
        self.height.hash(state);
        self.rgb.hash(state);
        self.alpha.hash(state);
    }
}

impl CustomImage for Pixels {
    fn color_channel(&self) -> &[u8] {
        &self.rgb
    }

    fn alpha_channel(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }

    fn bits_per_component(&self) -> BitsPerComponent {
        BitsPerComponent::Eight
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }

    fn color_space(&self) -> ImageColorspace {
        ImageColorspace::Rgb
    }
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
