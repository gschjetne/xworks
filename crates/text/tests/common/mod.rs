//! What the text tests share: the bundled fonts, and styles as specs write
//! them.

#![allow(dead_code, reason = "each test uses what it needs of these")]

use std::sync::{Arc, OnceLock};

use serde::Deserialize;
use text::{FontSpec, Fonts, Hyphenation, Hyphenator, Language, Style, units};

/// The bundled fonts, loaded once, with their fallbacks.
pub(crate) fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            fonts
                .add(bytes)
                .unwrap_or_else(|e| panic!("loading {name}: {e}"));
        }
        let spec: FontSpec =
            ron::from_str(&testkit::fonts::spec().expect("spec/text/fonts.ron")).expect("valid");
        fonts.set_fallbacks(&spec);
        fonts
    })
}

/// Every language's hyphenation patterns (`spec/text/hyphenation.ron`),
/// loaded once.
pub(crate) fn hyphenation() -> &'static Hyphenation {
    static HYPHENATION: OnceLock<Hyphenation> = OnceLock::new();
    HYPHENATION.get_or_init(|| {
        let mut all = Hyphenation::new();
        for loaded in testkit::hyphenation::load().expect("the hyphenation patterns") {
            let l = &loaded.language;
            let hyphenator = Arc::new(Hyphenator::parse(
                &loaded.patterns,
                &loaded.exceptions,
                l.left,
                l.right,
            ));
            for tag in &l.tags {
                all.insert(
                    &Language::parse(tag).expect("a language tag"),
                    hyphenator.clone(),
                );
            }
        }
        all
    })
}

/// A style as a spec writes it: what's left out is 12 points, regular,
/// upright and of undetermined language.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct StyleSpec {
    pub(crate) family: Option<String>,
    pub(crate) size: Option<f64>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) language: Option<String>,
}

impl StyleSpec {
    /// This style, with what it leaves out taken from `base`.
    pub(crate) fn over(&self, base: &StyleSpec) -> StyleSpec {
        StyleSpec {
            family: self.family.clone().or_else(|| base.family.clone()),
            size: self.size.or(base.size),
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
            language: self.language.clone().or_else(|| base.language.clone()),
        }
    }

    pub(crate) fn style(&self) -> Style {
        Style {
            family: self.family.clone().expect("a family"),
            size: points(self.size.unwrap_or(12.0)),
            weight: if self.bold.unwrap_or(false) { 700 } else { 400 },
            italic: self.italic.unwrap_or(false),
            language: locale::Language::parse(self.language.as_deref().unwrap_or("und"))
                .expect("a language tag"),
            ligatures: text::Ligatures::default(),
        }
    }
}

/// A spec, read with optional fields written plainly, without `Some`.
pub(crate) fn parse<T: serde::de::DeserializeOwned>(
    source: &str,
) -> Result<T, ron::error::SpannedError> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
}

/// `pt` points in layout units.
pub(crate) fn points(pt: f64) -> i64 {
    (pt * units::POINT as f64).round() as i64
}

/// The byte offset of the `chars`th character of `text`.
pub(crate) fn byte_offset(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}

/// The character offset of byte `offset` of `text`.
pub(crate) fn char_offset(text: &str, offset: usize) -> usize {
    text[..offset].chars().count()
}
