//! The story substrate: a tree of blocks with styles, and its outline and
//! draft views (docs/ARCHITECTURE.md, section 4). M1 has its first form: a
//! story of paragraphs with paragraph styles and character properties, laid
//! out with `text`.
//!
//! - [`StyleSheet`]: the paragraph styles, from `spec/styles/`, and the
//!   document's default tab stops.
//! - [`spans`]: a paragraph's text as styled spans, its character
//!   properties over its style.
//! - [`lay_out`] and [`lay_out_paragraph_with`]: a document's story's
//!   paragraphs laid out to a width, aligned and hyphenated as their
//!   styles and properties say, with their tab stops.
//! - [`word_count`]: a paragraph's words, by its languages' rules.
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
use text::{Align, Fonts, Hyphenation, Language, Setting, Span, Style, TabAlign, TabStop, Tabs};

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
    /// A document's language is its paragraphs': new ones take it from
    /// the paragraph they're split from.
    pub const LANGUAGE: &str = "language";
    /// A paragraph's alignment over its style's: "start", "center", "end"
    /// or "justify" (text).
    pub const ALIGN: &str = "align";
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
    /// Its level as a heading, 1 to 6, if it's a heading's style.
    #[serde(default)]
    pub heading: Option<u8>,
    /// Whether its words are hyphenated at lines' ends, by their
    /// language's patterns; if it doesn't say, they are where its lines
    /// are justified.
    #[serde(default)]
    pub hyphenate: Option<bool>,
    /// Its tab stops, besides the document's.
    #[serde(default)]
    pub tabs: Vec<TabStopSpec>,
}

/// A tab stop, as `spec/styles/` gives it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TabStopSpec {
    /// Its distance from the paragraph's start edge.
    pub at: Length,
    /// How the text after a tab sits at it.
    pub align: TabAlign,
}

/// The tab stops every paragraph of a document has: stops set, and the
/// interval of the default stops after them.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TabSpec {
    /// The stops set.
    #[serde(default)]
    pub stops: Vec<TabStopSpec>,
    /// The distance between default stops.
    pub interval: Length,
}

impl Default for TabSpec {
    fn default() -> TabSpec {
        TabSpec {
            stops: Vec::new(),
            interval: zero(),
        }
    }
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
    /// The document's tab stops, which every paragraph has besides its
    /// style's.
    #[serde(default)]
    pub tabs: TabSpec,
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

    /// The tab stops of a paragraph in `style`: the style's and the
    /// document's, by position, the style's where both have one at the
    /// same place, then the document's default ones.
    pub fn tabs(&self, style: &ParagraphStyle) -> Tabs {
        let stop = |s: &TabStopSpec| TabStop {
            position: s.at.units(),
            align: s.align,
        };
        let mut stops: Vec<TabStop> = style.tabs.iter().map(stop).collect();
        for s in self.tabs.stops.iter().map(stop) {
            if !stops.iter().any(|t| t.position == s.position) {
                stops.push(s);
            }
        }
        stops.sort_by_key(|s| s.position);
        Tabs {
            stops,
            interval: self.tabs.interval.units(),
        }
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
    /// Its level as a heading, if it's one.
    pub heading: Option<u8>,
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

/// How `paragraph`'s lines are aligned: as it says, or as its style does.
pub fn align(paragraph: &Paragraph, sheet: &StyleSheet) -> Align {
    match paragraph.props.get(keys::ALIGN) {
        Some(Value::Text(align)) => match align.as_str() {
            "start" => Align::Start,
            "center" => Align::Center,
            "end" => Align::End,
            "justify" => Align::Justify,
            _ => paragraph_style(paragraph, sheet).align,
        },
        _ => paragraph_style(paragraph, sheet).align,
    }
}

/// How many words `paragraph` has, by its languages' rules: stretches of
/// letters, numbers or ideographs between word boundaries.
pub fn word_count(paragraph: &Paragraph, sheet: &StyleSheet) -> usize {
    let mut languages = Vec::new();
    let mut at = 0;
    for span in spans(paragraph, sheet) {
        languages.push((at..at + span.len, span.style.language));
        at += span.len;
    }
    locale::word_count(&paragraph.text, &languages)
}

/// Paragraph `id` of `document`, laid out to `width`, with no
/// hyphenation patterns: [`lay_out_paragraph_with`] and none.
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
    lay_out_paragraph_with(document, id, sheet, fonts, &Hyphenation::new(), width)
}

/// Paragraph `id` of `document`, laid out to `width`, its words
/// hyphenated, where its style says, by the patterns in `hyphenation`.
/// Languages it has words to hyphenate in but `hyphenation` lacks are in
/// its layout's `unhyphenated`.
///
/// # Panics
///
/// If `id` isn't a paragraph of the document.
pub fn lay_out_paragraph_with(
    document: &Document,
    id: ObjectId,
    sheet: &StyleSheet,
    fonts: &Fonts,
    hyphenation: &Hyphenation,
    width: i64,
) -> ParagraphLayout {
    let paragraph = document.paragraph(id).expect("a paragraph of the document");
    let style = paragraph_style(paragraph, sheet);
    let spans = spans(paragraph, sheet);
    let align = align(paragraph, sheet);
    let tabs = sheet.tabs(style);
    let layout = text::layout_with(
        fonts,
        &text::Paragraph {
            text: &paragraph.text,
            spans: &spans,
            direction: None,
            align,
            width,
        },
        &Setting {
            hyphenate: style.hyphenate.unwrap_or(align == Align::Justify),
            hyphenation: Some(hyphenation),
            tabs: Some(&tabs),
        },
    );
    ParagraphLayout {
        id,
        layout,
        space_before: style.space_before.units(),
        space_after: style.space_after.units(),
        keep_with_next: style.keep_with_next,
        heading: style.heading.filter(|level| (1..=6).contains(level)),
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
