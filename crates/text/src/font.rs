use std::collections::BTreeMap;
use std::sync::Arc;

use harfrust::font::Blob;
use serde::Deserialize;
use skrifa::raw::FileRef;
use skrifa::{MetadataProvider, string::StringId};

/// Why a font file couldn't be loaded.
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    /// The data isn't a font or a font collection.
    #[error("not a font: {0}")]
    NotAFont(String),
    /// A face in the file can't be read.
    #[error("face {index}: {reason}")]
    BadFace {
        /// The face's index in the file.
        index: u32,
        /// What's wrong with it.
        reason: String,
    },
}

/// A face in a [`Fonts`], by the order it was added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceId(u32);

impl FaceId {
    /// Its index in its [`Fonts`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One face of a font file: what layout needs from it, and its data, for
/// drawing and embedding. Cloning shares the data.
#[derive(Clone)]
pub struct Face {
    data: scene::FontData,
    blob: Blob,
    index: u32,
    pub(crate) font: harfrust::Font,
    family: String,
    weight: u16,
    italic: bool,
    pub(crate) units_per_em: u16,
    /// Above the baseline, in font units.
    pub(crate) ascent: i32,
    /// Below the baseline, in font units, positive.
    pub(crate) descent: i32,
    pub(crate) line_gap: i32,
    /// Where an underline goes, in font units.
    pub(crate) underline: Stroke,
    /// Where a line through the text goes, in font units.
    pub(crate) strikeout: Stroke,
}

/// A line drawn with text, as a font places it, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stroke {
    /// Its top edge's height above the baseline; below it if negative.
    pub(crate) top: i32,
    /// How thick it is.
    pub(crate) thickness: i32,
}

impl Face {
    /// Its typographic family name, such as "Noto Serif".
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Its weight, from 1 to 1000: 400 is regular, 700 bold.
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Whether it's italic or oblique.
    pub fn italic(&self) -> bool {
        self.italic
    }

    /// The font file it's in.
    pub fn data(&self) -> &[u8] {
        &self.blob
    }

    /// The face, for drawing: its file, shared, and its index in it.
    pub fn font(&self) -> scene::Font {
        scene::Font {
            data: self.data.clone(),
            index: self.index,
        }
    }

    /// Its index in its file, which is 0 unless the file is a collection.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Its em, in font units.
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Whether it has a glyph for `c`.
    pub fn has(&self, c: char) -> bool {
        self.font.charmap().map_unicode(c).is_some()
    }
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("family", &self.family)
            .field("weight", &self.weight)
            .field("italic", &self.italic)
            .field("index", &self.index)
            .finish()
    }
}

/// How families fall back for characters they lack, as
/// `spec/text/fonts.ron` gives it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FontSpec {
    /// The families, each with its fallbacks.
    pub families: Vec<FamilySpec>,
}

/// A family and the families its missing characters come from, in order.
#[derive(Clone, Debug, Deserialize)]
pub struct FamilySpec {
    /// The family.
    pub family: String,
    /// Its fallbacks, first choice first.
    pub fallback: Vec<String>,
}

/// The fonts text can be set in: faces, and how families fall back.
///
/// A character comes from the face its style asks for if that has it, or
/// else from the first of the family's fallbacks that does, in the weight
/// and slant nearest the style's.
///
/// Faces are only ever added, so a [`FaceId`] stays the same face, and
/// layouts made before more faces were added still draw with them.
/// Cloning shares the faces' data.
#[derive(Clone, Debug, Default)]
pub struct Fonts {
    faces: Vec<Face>,
    fallbacks: BTreeMap<String, Vec<String>>,
}

impl Fonts {
    /// No fonts.
    pub fn new() -> Fonts {
        Fonts::default()
    }

    /// Adds every face in `data`, a font file or collection. Returns their
    /// IDs; nothing is added if any face can't be read.
    pub fn add(&mut self, data: Vec<u8>) -> Result<Vec<FaceId>, FontError> {
        let shared: scene::FontData = Arc::new(data);
        let blob = Blob::from(shared.clone());
        let count = match FileRef::new(&blob).map_err(|e| FontError::NotAFont(e.to_string()))? {
            FileRef::Font(_) => 1,
            FileRef::Collection(c) => c.len(),
        };
        let faces = (0..count)
            .map(|index| load(&shared, &blob, index))
            .collect::<Result<Vec<_>, _>>()?;
        let first = self.faces.len() as u32;
        self.faces.extend(faces);
        Ok((first..self.faces.len() as u32).map(FaceId).collect())
    }

    /// Sets the families' fallbacks, replacing any set before.
    pub fn set_fallbacks(&mut self, spec: &FontSpec) {
        self.fallbacks = spec
            .families
            .iter()
            .map(|f| (f.family.clone(), f.fallback.clone()))
            .collect();
    }

    /// The face `id`.
    ///
    /// # Panics
    ///
    /// If `id` isn't from these fonts.
    pub fn face(&self, id: FaceId) -> &Face {
        &self.faces[id.index()]
    }

    /// Every face, by ID.
    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces
            .iter()
            .enumerate()
            .map(|(i, f)| (FaceId(i as u32), f))
    }

    /// The face of `family` nearest `weight` and `italic`: the right slant
    /// first, then the nearest weight, the heavier of two equally near.
    pub fn select(&self, family: &str, weight: u16, italic: bool) -> Option<FaceId> {
        self.faces()
            .filter(|(_, f)| f.family == family)
            .min_by_key(|(_, f)| {
                (
                    f.italic != italic,
                    f.weight.abs_diff(weight),
                    std::cmp::Reverse(f.weight),
                )
            })
            .map(|(id, _)| id)
    }

    /// The faces a character of `family`, `weight` and `italic` may come
    /// from, first choice first: the family's own face, then its
    /// fallbacks'. If the family has no face, the first face of all leads,
    /// so text always has a font while there is one.
    pub(crate) fn candidates(&self, family: &str, weight: u16, italic: bool) -> Vec<FaceId> {
        let mut out = Vec::new();
        out.extend(self.select(family, weight, italic));
        if out.is_empty() && !self.faces.is_empty() {
            out.push(FaceId(0));
        }
        for fallback in self.fallbacks.get(family).into_iter().flatten() {
            if let Some(id) = self.select(fallback, weight, italic)
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
        out
    }
}

/// Face `index` of `blob`.
fn load(data: &scene::FontData, blob: &Blob, index: u32) -> Result<Face, FontError> {
    let bad = |reason: String| FontError::BadFace { index, reason };
    let font_ref = skrifa::FontRef::from_index(blob, index).map_err(|e| bad(e.to_string()))?;
    let family = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| font_ref.localized_strings(id).english_or_first())
        .map(|name| name.to_string())
        .ok_or_else(|| bad("no family name".to_string()))?;
    let attributes = font_ref.attributes();
    let weight = attributes.weight.value().round().clamp(1.0, 1000.0) as u16;
    let italic = attributes.style != skrifa::attribute::Style::Normal;
    let font = harfrust::Font::new(blob.clone(), index)
        .ok_or_else(|| bad("not a font HarfRust can shape with".to_string()))?;
    let line = font.metrics().h_line().unwrap_or_default();
    let units_per_em = font.units_per_em();
    if units_per_em == 0 {
        return Err(bad("an em of 0 units".to_string()));
    }
    let (underline, strikeout) = strokes(&font_ref, units_per_em);
    Ok(Face {
        data: data.clone(),
        blob: blob.clone(),
        index,
        family,
        weight,
        italic,
        units_per_em,
        ascent: line.ascender.to_i32(),
        descent: -line.descender.to_i32(),
        line_gap: line.line_gap.to_i32().max(0),
        underline,
        strikeout,
        font,
    })
}

/// Where a face's underline and strikeout go, from its `post` and `OS/2`
/// tables. A face without them, or with a stroke of no thickness, gets
/// what's usual: an underline a tenth of an em below the baseline, and a
/// strikeout whose middle is three tenths of an em above it, each a
/// twentieth of an em thick.
fn strokes(font: &skrifa::FontRef<'_>, units_per_em: u16) -> (Stroke, Stroke) {
    let metrics = font.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let em = i32::from(units_per_em);
    let thin = (em / 20).max(1);
    let stroke = |decoration: Option<skrifa::metrics::Decoration>, top: i32| match decoration
        .filter(|d| d.thickness > 0.0)
    {
        Some(d) => Stroke {
            top: d.offset.round() as i32,
            thickness: d.thickness.round().max(1.0) as i32,
        },
        None => Stroke {
            top,
            thickness: thin,
        },
    };
    (
        stroke(metrics.underline, -em / 10),
        stroke(metrics.strikeout, em * 3 / 10 + thin / 2),
    )
}
