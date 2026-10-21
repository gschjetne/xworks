//! The story substrate: a tree of blocks with styles, and its outline and
//! draft views (docs/ARCHITECTURE.md, section 4). M1 has its first form: a
//! story of paragraphs with paragraph styles and character properties, laid
//! out with `text`.
//!
//! - [`StyleSheet`]: the paragraph styles, from `spec/styles/`.
//! - [`spans`]: a paragraph's text as styled spans, its character
//!   properties over its style.
//! - [`lay_out`]: every paragraph of a document's story, laid out to a
//!   width.
//! - [`keys`]: the properties a story reads.
//! - [`document`]: a document made from styled text, through `doc`'s
//!   operations, as importers and tests make them.
//!
//! Invariants:
//!
//! - A paragraph's layout depends only on that paragraph, its style and
//!   the width: editing one paragraph leaves every other's layout as it
//!   was.
//! - A property that's unset, or set to a value of the wrong kind, takes
//!   its style's value; a style that isn't in the sheet is the first one.

use doc::{Document, ObjectId, Op, Paragraph, Props, Run, Value};
use serde::Deserialize;
use text::units::Length;
use text::{Align, Fonts, Language, Span, Style};

/// The names of the properties a story reads, from paragraphs and from runs
/// of text.
pub mod keys {
    /// A paragraph's style, by name (text).
    pub const STYLE: &str = "style";
    /// Bold (on or off).
    pub const BOLD: &str = "bold";
    /// Italic (on or off).
    pub const ITALIC: &str = "italic";
    /// The font family (text).
    pub const FONT: &str = "font";
    /// The font size, in layout units (a number).
    pub const SIZE: &str = "size";
    /// The language, a BCP 47 tag (text), on a run or a whole paragraph.
    pub const LANGUAGE: &str = "language";
}

/// A paragraph style, as `spec/styles/` gives it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ParagraphStyle {
    /// Its name, which paragraphs refer to it by.
    pub name: String,
    /// Its font family.
    pub family: String,
    /// Its font size.
    pub size: Length,
    /// Whether it's bold.
    #[serde(default)]
    pub bold: bool,
    /// Whether it's italic.
    #[serde(default)]
    pub italic: bool,
    /// Space above it.
    #[serde(default = "zero")]
    pub space_before: Length,
    /// Space below it.
    #[serde(default = "zero")]
    pub space_after: Length,
    /// Whether it stays on the page where the next paragraph starts.
    #[serde(default)]
    pub keep_with_next: bool,
    /// How its lines are aligned.
    #[serde(default)]
    pub align: Align,
}

fn zero() -> Length {
    Length::Pt(0.0)
}

/// The paragraph styles of a style sheet, as `spec/styles/` gives them.
/// The file says more, for other substrates; this is what a story reads.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StyleSheet {
    /// The paragraph styles. The first is the default.
    pub paragraph: Vec<ParagraphStyle>,
}

impl StyleSheet {
    /// The style `name`, or the default if there's none by that name.
    ///
    /// # Panics
    ///
    /// If the sheet has no paragraph styles.
    pub fn paragraph_style(&self, name: Option<&str>) -> &ParagraphStyle {
        name.and_then(|n| self.paragraph.iter().find(|s| s.name == n))
            .or(self.paragraph.first())
            .expect("a style sheet has a paragraph style")
    }
}

/// A paragraph of a story, laid out.
#[derive(Clone, Debug)]
pub struct ParagraphLayout {
    /// The paragraph.
    pub id: ObjectId,
    /// Its lines.
    pub layout: text::Layout,
    /// Space above it, in layout units.
    pub space_before: i64,
    /// Space below it, in layout units.
    pub space_after: i64,
    /// Whether it stays on the page where the next paragraph starts.
    pub keep_with_next: bool,
}

/// `paragraph`'s text as spans: each run of its characters in its style,
/// with the run's own properties over it.
pub fn spans(paragraph: &Paragraph, sheet: &StyleSheet) -> Vec<Span> {
    let style = paragraph_style(paragraph, sheet);
    let base = Style {
        family: style.family.clone(),
        size: style.size.units(),
        weight: if style.bold { 700 } else { 400 },
        italic: style.italic,
        language: language(paragraph.props.get(keys::LANGUAGE)).unwrap_or_default(),
    };
    let mut chars = paragraph.text.chars();
    let mut spans: Vec<Span> = paragraph
        .runs
        .iter()
        .map(|run| {
            let len = chars
                .by_ref()
                .take(run.len as usize)
                .map(char::len_utf8)
                .sum();
            let mut style = base.clone();
            if let Some(Value::Bool(bold)) = run.props.get(keys::BOLD) {
                style.weight = if *bold { 700 } else { 400 };
            }
            if let Some(Value::Bool(italic)) = run.props.get(keys::ITALIC) {
                style.italic = *italic;
            }
            if let Some(Value::Text(family)) = run.props.get(keys::FONT) {
                style.family = family.clone();
            }
            if let Some(Value::Int(size)) = run.props.get(keys::SIZE)
                && *size > 0
            {
                style.size = *size;
            }
            if let Some(language) = language(run.props.get(keys::LANGUAGE)) {
                style.language = language;
            }
            Span { len, style }
        })
        .collect();
    if spans.is_empty() {
        spans.push(Span {
            len: 0,
            style: base,
        });
    }
    spans
}

/// The style `paragraph` names, or the default.
fn paragraph_style<'a>(paragraph: &Paragraph, sheet: &'a StyleSheet) -> &'a ParagraphStyle {
    let name = match paragraph.props.get(keys::STYLE) {
        Some(Value::Text(name)) => Some(name.as_str()),
        _ => None,
    };
    sheet.paragraph_style(name)
}

fn language(value: Option<&Value>) -> Option<Language> {
    match value {
        Some(Value::Text(tag)) => Language::parse(tag).ok(),
        _ => None,
    }
}

/// Paragraph `id` of `document`, laid out to `width`.
///
/// # Panics
///
/// If `id` isn't a paragraph of the document.
pub fn lay_out_paragraph(
    document: &Document,
    id: ObjectId,
    sheet: &StyleSheet,
    fonts: &Fonts,
    width: i64,
) -> ParagraphLayout {
    let paragraph = document.paragraph(id).expect("a paragraph of the document");
    let style = paragraph_style(paragraph, sheet);
    let spans = spans(paragraph, sheet);
    let layout = text::layout(
        fonts,
        &text::Paragraph {
            text: &paragraph.text,
            spans: &spans,
            direction: None,
            align: style.align,
            width,
        },
    );
    ParagraphLayout {
        id,
        layout,
        space_before: style.space_before.units(),
        space_after: style.space_after.units(),
        keep_with_next: style.keep_with_next,
    }
}

/// Every paragraph of `document`'s story, in order, laid out to `width`.
pub fn lay_out(
    document: &Document,
    sheet: &StyleSheet,
    fonts: &Fonts,
    width: i64,
) -> Vec<ParagraphLayout> {
    document
        .paragraphs()
        .iter()
        .map(|&id| lay_out_paragraph(document, id, sheet, fonts, width))
        .collect()
}

/// A document whose story holds `paragraphs`, each a style's name and its
/// text in runs, each run with its character properties. It's made with
/// `doc`'s operations, as an importer would make it.
pub fn document<S: AsRef<str>, T: AsRef<str>>(paragraphs: &[(S, Vec<(T, Props)>)]) -> Document {
    let mut document = Document::new();
    let mut current = document.paragraphs()[0];
    let apply = |document: &mut Document, op: Op| {
        document.apply(&op).expect("building a document applies");
    };
    for (i, (style, runs)) in paragraphs.iter().enumerate() {
        if i > 0 {
            let new = document.new_id();
            let offset = document.paragraph(current).map_or(0, Paragraph::len);
            apply(
                &mut document,
                Op::SplitParagraph {
                    paragraph: current,
                    offset,
                    new,
                    new_props: Props::new(),
                },
            );
            current = new;
        }
        apply(
            &mut document,
            Op::SetParaProp {
                paragraph: current,
                key: keys::STYLE.to_string(),
                value: Some(Value::Text(style.as_ref().to_string())),
            },
        );
        let mut offset = 0;
        for (text, props) in runs {
            let text = text.as_ref();
            let len = text.chars().count() as u32;
            apply(
                &mut document,
                Op::InsertText {
                    paragraph: current,
                    offset,
                    text: text.to_string(),
                    runs: vec![Run::new(len, props.clone())],
                },
            );
            offset += len;
        }
    }
    document
}

/// Character properties: `pairs` of a key and a value.
pub fn props(pairs: &[(&str, Value)]) -> Props {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
