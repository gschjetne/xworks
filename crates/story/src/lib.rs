//! The story substrate: a tree of blocks with styles, and its outline and
//! draft views (docs/ARCHITECTURE.md, section 4). M2 has its tree: lists
//! of items, quotes, code blocks and tables around paragraphs, with the
//! marks Markdown needs on their text, laid out with `text`.
//!
//! - [`StyleSheet`]: the paragraph styles, the document's default tab
//!   stops, and how lists, quotes, code and tables look ([`Blocks`]), from
//!   `spec/styles/`.
//! - [`flow`]: the story's paragraphs in reading order, each with its
//!   [`Place`] in the tree: indent, list marker (numbers computed, not
//!   stored), quotes, table cell, and the containers around it.
//! - [`spans`] and [`spans_at`]: a paragraph's text as styled spans, its
//!   character properties over its style.
//! - [`lay_out`], [`lay_out_paragraph_with`] and [`lay_out_at`]: a
//!   document's story's paragraphs laid out at their places, aligned and
//!   hyphenated as their styles and properties say, with their tab stops.
//! - Pictures are in paragraphs' text, as Markdown's `![alt](src)` is:
//!   each an [`OBJECT`] character whose run names the picture
//!   ([`keys::PICTURE`]), its alternative text and the size it's placed
//!   at, laid out as an inline object scaled down to fit its paragraph
//!   ([`ParagraphLayout::pictures`]). A paragraph of one picture alone is
//!   a figure. [`pictures`] lists those a document shows.
//! - GitHub's strikethrough, a line through text ([`decoration`]), and
//!   task lists, items with a box in their bullet's place
//!   ([`Place::task`]); and Markdown a story doesn't hold, kept as it was
//!   written and shown as code ([`keys::SOURCE`]).
//! - [`word_count`]: a paragraph's words, by its languages' rules.
//! - [`keys`]: the properties a story reads.
//! - [`document`] and [`build`]: documents made from styled text, through
//!   `doc`'s operations, as importers and tests make them.
//! - [`outline`]: a document's tree as indented lines, for tests.
//!
//! Invariants:
//!
//! - A paragraph's layout depends only on that paragraph, its style and
//!   its place: editing one paragraph leaves every other's layout as it
//!   was, unless it moves others in the tree.
//! - A property that's unset, or set to a value of the wrong kind, takes
//!   its style's value; a style that isn't in the sheet is the first one.
//! - A story of paragraphs alone lays out as M1's did.

mod flow;

pub use flow::{Ancestor, Cell, DEEPEST, Entry, Flow, Place, flow};

use std::ops::Range;

use doc::{Block, Document, Kind, Object, ObjectId, Op, Paragraph, Props, Run, Value};
use serde::Deserialize;
use text::units::Length;
use text::{
    Align, Fonts, Hyphenation, Language, Ligatures, Setting, Span, Style, TabAlign, TabStop, Tabs,
};

/// The names of the properties a story reads, from paragraphs, from runs
/// of text, and from containers.
pub mod keys {
    /// A paragraph's style, by name (text).
    pub const STYLE: &str = "style";
    /// Bold (on or off): Markdown's strong emphasis.
    pub const BOLD: &str = "bold";
    /// Italic (on or off): Markdown's emphasis.
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
    /// or "justify" (text). A table's column alignment is its cells'.
    pub const ALIGN: &str = "align";
    /// Code, in a run (on or off): set in the code face, as Markdown's
    /// backticks set it.
    pub const CODE: &str = "code";
    /// A link's target, a URL (text), on the run of text that's the link.
    pub const LINK: &str = "link";
    /// A link's title (text), where it has one.
    pub const LINK_TITLE: &str = "link-title";
    /// A code block's info string (text), such as the language of its
    /// code: what follows a Markdown fence.
    pub const INFO: &str = "info";
    /// Whether a list is numbered (on or off).
    pub const ORDERED: &str = "ordered";
    /// The number a numbered list starts from (a number); 1 if unset.
    pub const START: &str = "start";
    /// The space above a paragraph, in layout units (a number), over its
    /// style's.
    pub const SPACE_BEFORE: &str = "space-before";
    /// The space below a paragraph, in layout units (a number), over its
    /// style's.
    pub const SPACE_AFTER: &str = "space-after";
    /// How tall a paragraph's lines are, in thousandths of their largest
    /// font size (a number): 1 200 is 1.2 times the font. Unset, lines are
    /// as tall as their fonts.
    pub const LINE_HEIGHT: &str = "line-height";
    /// How far a paragraph's text is indented from its place's start
    /// edge, in layout units (a number): the left in left-to-right text.
    pub const INDENT_START: &str = "indent-start";
    /// How far from its place's end edge, in layout units (a number).
    pub const INDENT_END: &str = "indent-end";
    /// How much further a paragraph's first line is indented than the
    /// rest, in layout units (a number); negative hangs it out.
    pub const INDENT_FIRST: &str = "indent-first";
    /// A paragraph's own tab stops (text), in place of its style's and
    /// the document's: [`crate::format_tabs`] writes them and
    /// [`crate::parse_tabs`] reads them. The document's default stops
    /// still follow them.
    pub const TABS: &str = "tabs";
    /// A picture, by its ID in the document's file (text), on the run of
    /// [`super::OBJECT`] characters that show it: Markdown's `![alt](src)`.
    pub const PICTURE: &str = "picture";
    /// What a picture shows, in words (text), for screen readers and
    /// tagged PDF: Markdown's alt.
    pub const PICTURE_ALT: &str = "picture-alt";
    /// How wide a picture is placed, in layout units (a number), before
    /// it's scaled down to fit its paragraph.
    pub const PICTURE_WIDTH: &str = "picture-width";
    /// How tall a picture is placed, in layout units (a number).
    pub const PICTURE_HEIGHT: &str = "picture-height";
    /// Where a linked picture is, as its document links it (text): a path
    /// relative to the document's folder, an absolute path, or a URL, as
    /// Markdown's `![alt](src)` writes it. A Markdown file's pictures are
    /// linked, not kept in it, and each one's [`PICTURE`] is its link.
    pub const PICTURE_SRC: &str = "picture-src";
    /// A picture's title (text), as Markdown's `![alt](src "title")`
    /// gives it.
    pub const PICTURE_TITLE: &str = "picture-title";

    /// The keys that make a run a picture, which text typed after one
    /// doesn't take from it.
    pub const PICTURES: [&str; 6] = [
        PICTURE,
        PICTURE_ALT,
        PICTURE_WIDTH,
        PICTURE_HEIGHT,
        PICTURE_SRC,
        PICTURE_TITLE,
    ];

    /// A line through the text (on or off): GitHub's `~~strikethrough~~`.
    pub const STRIKETHROUGH: &str = "strikethrough";
    /// Markdown kept as it was written (text, saying what it is: "html",
    /// "rule", "definition", "footnote" or "metadata"): what a story
    /// doesn't hold, such as HTML, a thematic break or a link reference
    /// definition, as its source. On a code block it's a block of it, on a
    /// run of text it's inline, as inline HTML is; either way it's shown as
    /// code is, and written back as it is.
    pub const SOURCE: &str = "source";
    /// Whether a list item is a task, and done (on or off): its box,
    /// checked or not, as GitHub's task lists' `- [ ]` and `- [x]`. Unset,
    /// it's an item as any other.
    pub const TASK: &str = "task";
    /// A quote's kind (text), as GitHub's alerts give it: "NOTE", "TIP",
    /// "IMPORTANT", "WARNING" or "CAUTION".
    pub const ALERT: &str = "alert";
}

/// The most a paragraph's space above or below, or an indent, may be: ten
/// inches. Larger values, as from a damaged file, are taken as unset.
const MOST_SPACE: i64 = 10 * text::units::INCH;

/// The most a tab stop's position may be: a hundred inches.
const MOST_TAB: i64 = 100 * text::units::INCH;

/// A paragraph's own indents, in layout units: from its place's start and
/// end edges, and its first line's further in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Indents {
    /// From the start edge: the left in left-to-right text.
    pub start: i64,
    /// From the end edge.
    pub end: i64,
    /// The first line's, further in than `start`; negative hangs it out.
    pub first: i64,
}

/// `key`'s value on `paragraph` as a number within `range`, if it's one.
fn number(paragraph: &Paragraph, key: &str, range: std::ops::RangeInclusive<i64>) -> Option<i64> {
    match paragraph.props.get(key) {
        Some(Value::Int(n)) if range.contains(n) => Some(*n),
        _ => None,
    }
}

/// `paragraph`'s own indents; those unset, or past what makes sense, are
/// 0.
pub fn indents(paragraph: &Paragraph) -> Indents {
    let get = |key| number(paragraph, key, -MOST_SPACE..=MOST_SPACE).unwrap_or(0);
    Indents {
        start: get(keys::INDENT_START),
        end: get(keys::INDENT_END),
        first: get(keys::INDENT_FIRST),
    }
}

/// How tall `paragraph`'s lines are, in thousandths of their font size,
/// if it says: from a quarter of the font to ten times it.
pub fn line_height(paragraph: &Paragraph) -> Option<i64> {
    number(paragraph, keys::LINE_HEIGHT, 250..=10_000)
}

/// The space above and below `paragraph`: its own where it sets them,
/// otherwise its style's.
pub fn spacing(paragraph: &Paragraph, sheet: &StyleSheet) -> (i64, i64) {
    let style = sheet.style_of(paragraph);
    (
        number(paragraph, keys::SPACE_BEFORE, 0..=MOST_SPACE)
            .unwrap_or_else(|| style.space_before.units()),
        number(paragraph, keys::SPACE_AFTER, 0..=MOST_SPACE)
            .unwrap_or_else(|| style.space_after.units()),
    )
}

/// The tab stops `paragraph`'s tabs go to, measured from its place's
/// start edge: its own, where it sets them, or its style's and the
/// document's; then the document's default ones.
pub fn paragraph_tabs(paragraph: &Paragraph, sheet: &StyleSheet) -> Tabs {
    let own = match paragraph.props.get(keys::TABS) {
        Some(Value::Text(stops)) => parse_tabs(stops),
        _ => None,
    };
    match own {
        Some(stops) => Tabs {
            stops,
            interval: sheet.tabs.interval.units(),
        },
        None => sheet.tabs(sheet.style_of(paragraph)),
    }
}

/// Tab stops as [`keys::TABS`] holds them: each its kind (`start`, `end`,
/// `center`, or `decimal:` and the character it lines up on) and its
/// position in layout units, separated by semicolons, in order of
/// position: `"start 1260000; decimal:. 4950000"`.
pub fn format_tabs(stops: &[TabStop]) -> String {
    let mut stops = stops.to_vec();
    stops.sort_by_key(|s| s.position);
    stops
        .iter()
        .map(|s| {
            let kind = match s.align {
                TabAlign::Start => "start".to_string(),
                TabAlign::End => "end".to_string(),
                TabAlign::Center => "center".to_string(),
                TabAlign::Decimal(c) => format!("decimal:{c}"),
            };
            format!("{kind} {}", s.position)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The tab stops [`format_tabs`] wrote, in order of position, one at each
/// place; `None` if any of them isn't one.
pub fn parse_tabs(text: &str) -> Option<Vec<TabStop>> {
    let mut stops = Vec::new();
    for entry in text.split(';').map(str::trim).filter(|e| !e.is_empty()) {
        let mut parts = entry.split_whitespace();
        let (kind, position) = (parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let align = match kind {
            "start" => TabAlign::Start,
            "end" => TabAlign::End,
            "center" => TabAlign::Center,
            _ => {
                let mut c = kind.strip_prefix("decimal:")?.chars();
                match (c.next(), c.next()) {
                    (Some(c), None) => TabAlign::Decimal(c),
                    _ => return None,
                }
            }
        };
        let position: i64 = position.parse().ok()?;
        if !(0..=MOST_TAB).contains(&position) {
            return None;
        }
        stops.push(TabStop { position, align });
    }
    stops.sort_by_key(|s| s.position);
    stops.dedup_by_key(|s| s.position);
    Some(stops)
}

/// The character a picture in a paragraph's text stands for, U+FFFC
/// OBJECT REPLACEMENT CHARACTER: one for each picture its run shows
/// ([`keys::PICTURE`]).
pub const OBJECT: char = '\u{fffc}';

/// The largest a picture is drawn either way, in layout units: 100
/// inches, so no file's numbers carry layout past what it can add up.
pub const LARGEST_PICTURE: i64 = 100 * text::units::INCH;

/// A picture placed in a paragraph's text, as its run's properties say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The byte offset of its character in the paragraph's text.
    pub offset: usize,
    /// Its ID in the document's file.
    pub id: String,
    /// What it shows, in words, if that's said.
    pub alt: Option<String>,
}

/// The picture a run with properties `props` shows, if it shows one: its
/// ID, its alternative text, and the size it's placed at, at least a unit
/// and at most [`LARGEST_PICTURE`] each way.
pub fn picture_of(props: &Props) -> Option<(String, Option<String>, i64, i64)> {
    let Some(Value::Text(id)) = props.get(keys::PICTURE) else {
        return None;
    };
    let alt = match props.get(keys::PICTURE_ALT) {
        Some(Value::Text(alt)) if !alt.is_empty() => Some(alt.clone()),
        _ => None,
    };
    let side = |key: &str| match props.get(key) {
        Some(Value::Int(n)) => (*n).clamp(1, LARGEST_PICTURE),
        _ => text::units::INCH,
    };
    Some((
        id.clone(),
        alt,
        side(keys::PICTURE_WIDTH),
        side(keys::PICTURE_HEIGHT),
    ))
}

/// The IDs of the pictures `document` shows, each once, in the order
/// they're first shown.
pub fn pictures(document: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in document.paragraphs() {
        let Some(p) = document.paragraph(id) else {
            continue;
        };
        for run in &p.runs {
            if let Some(Value::Text(picture)) = run.props.get(keys::PICTURE)
                && !out.contains(picture)
            {
                out.push(picture.clone());
            }
        }
    }
    out
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
    /// Whether it's a code block's style: its text is code, drawn on a
    /// tint, and Enter breaks its lines rather than the paragraph.
    #[serde(default)]
    pub code: bool,
    /// Which classes of ligature its text is set with (project owner,
    /// 2026-10-23, review dcn5ejrd); OpenType's defaults where it doesn't
    /// say.
    #[serde(default)]
    pub ligatures: Ligatures,
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

/// How blocks other than paragraphs look, as `spec/styles/` gives it.
/// Colours are sRGB.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Blocks {
    /// How far each level of list indents its items.
    pub list_indent: Length,
    /// The bullets of lists that aren't numbered, by depth, from the
    /// outermost, over again when nesting goes deeper.
    pub bullets: Vec<String>,
    /// The room between a list item's marker and its text.
    pub marker_gap: Length,
    /// How far a quote indents its blocks, its bar included.
    pub quote_indent: Length,
    /// How wide a quote's bar is.
    pub quote_bar: Length,
    /// The colour of a quote's bar.
    pub quote_color: (u8, u8, u8),
    /// The family code is set in, in runs as in code blocks' styles.
    pub code_family: String,
    /// The ligatures code in runs is set with: none, so it shows exactly
    /// what was typed (project owner, 2026-10-23, review dcn5ejrd).
    pub code_ligatures: Ligatures,
    /// The tint behind code.
    pub code_tint: (u8, u8, u8),
    /// How far a code block's tint reaches around its text.
    pub code_padding: Length,
    /// The space above and below a table.
    pub table_space: Length,
    /// The room around a cell's text.
    pub cell_padding: Length,
    /// The width of a table's rules above and below it, and under its
    /// header.
    pub rule: Length,
    /// The width of the rules between its rows.
    pub hairline: Length,
    /// The colour of a table's rules.
    pub rule_color: (u8, u8, u8),
    /// The colour of the rules between its rows.
    pub hairline_color: (u8, u8, u8),
    /// The colour of links' text.
    pub link_color: (u8, u8, u8),
    /// The colour drawn where a picture can't be, as when the document's
    /// file lacks it.
    pub missing_picture: (u8, u8, u8),
}

/// The paragraph styles of a style sheet, and how other blocks look, as
/// `spec/styles/` gives them. The file says more, for other substrates;
/// this is what a story reads.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct StyleSheet {
    /// The paragraph styles. The first is the default.
    pub paragraph: Vec<ParagraphStyle>,
    /// The document's tab stops, which every paragraph has besides its
    /// style's.
    #[serde(default)]
    pub tabs: TabSpec,
    /// How lists, quotes, code and tables look.
    pub blocks: Blocks,
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

    /// The style `paragraph` names, or the default.
    pub fn style_of(&self, paragraph: &Paragraph) -> &ParagraphStyle {
        let name = match paragraph.props.get(keys::STYLE) {
            Some(Value::Text(name)) => Some(name.as_str()),
            _ => None,
        };
        self.paragraph_style(name)
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
    /// Where it sits in the story's tree.
    pub place: Place,
    /// How far its text sits in from its place on each side, as a code
    /// block's does on its tint.
    pub inset: i64,
    /// How far its own indents move its text in from its place's left:
    /// its start indent in left-to-right text, its end indent in
    /// right-to-left text.
    pub margin: i64,
    /// Its list item's marker, laid out in its first span's style, if it
    /// starts an item.
    pub marker: Option<text::Layout>,
    /// Whether it's a code block.
    pub code: bool,
    /// Which of its spans are links.
    pub links: Vec<bool>,
    /// The lines drawn with each of its spans, such as through it.
    pub decorations: Vec<text::Decoration>,
    /// The byte ranges of its text that are code, in runs, or Markdown
    /// kept as it was written.
    pub inline_code: Vec<Range<usize>>,
    /// The pictures in its text, in order; its layout's lines say where
    /// each went.
    pub pictures: Vec<Picture>,
    /// Whether it's a figure: its text is one picture and nothing else,
    /// as Markdown's `![alt](src)` alone in a paragraph is.
    pub figure: bool,
}

impl ParagraphLayout {
    /// Where its text's lines start, across from the body's start edge.
    pub fn x(&self) -> i64 {
        self.place.indent + self.inset + self.margin
    }
}

/// `paragraph`'s text as spans: each run of its characters in its style,
/// with the run's own properties over it.
pub fn spans(paragraph: &Paragraph, sheet: &StyleSheet) -> Vec<Span> {
    spans_at(paragraph, sheet, None)
}

/// `paragraph`'s text as spans, as [`spans`] gives them, for a paragraph
/// at `place`: a table's header cells are bold.
pub fn spans_at(paragraph: &Paragraph, sheet: &StyleSheet, place: Option<&Place>) -> Vec<Span> {
    let base = base_style(paragraph, sheet, place);
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
            if is_code(&run.props) {
                style.family = sheet.blocks.code_family.clone();
                style.ligatures = sheet.blocks.code_ligatures;
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

/// Whether a run with `props` is shown as code: code, or Markdown kept as
/// it was written ([`keys::SOURCE`]).
fn is_code(props: &Props) -> bool {
    matches!(props.get(keys::CODE), Some(Value::Bool(true)))
        || matches!(props.get(keys::SOURCE), Some(Value::Text(_)))
}

/// The lines drawn with a run of text with `props`: through it, where it's
/// struck through.
pub fn decoration(props: &Props) -> text::Decoration {
    text::Decoration {
        underline: false,
        strikethrough: matches!(props.get(keys::STRIKETHROUGH), Some(Value::Bool(true))),
    }
}

/// The style of `paragraph`'s text before its runs' own properties: its
/// paragraph style's, and bold in a table's header.
fn base_style(paragraph: &Paragraph, sheet: &StyleSheet, place: Option<&Place>) -> Style {
    let style = sheet.style_of(paragraph);
    let header = place
        .and_then(|p| p.cell)
        .is_some_and(|cell| cell.index == 0);
    Style {
        family: style.family.clone(),
        size: style.size.units(),
        weight: if style.bold || header { 700 } else { 400 },
        italic: style.italic,
        language: language(paragraph.props.get(keys::LANGUAGE)).unwrap_or_default(),
        ligatures: style.ligatures,
    }
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
            _ => sheet.style_of(paragraph).align,
        },
        _ => sheet.style_of(paragraph).align,
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

/// Paragraph `id` of `document`, straight in the story, laid out to
/// `width`, with no hyphenation patterns: [`lay_out_paragraph_with`] and
/// none.
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

/// Paragraph `id` of `document`, straight in the story, laid out to
/// `width`, as [`lay_out_at`] lays it out.
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
    lay_out_at(document, id, &Place::top(width), sheet, fonts, hyphenation)
}

/// Paragraph `id` of `document`, laid out at `place`: to its width, with
/// its marker if it starts a list item, its words hyphenated, where its
/// style says, by the patterns in `hyphenation`. Languages it has words to
/// hyphenate in but `hyphenation` lacks are in its layout's
/// `unhyphenated`.
///
/// # Panics
///
/// If `id` isn't a paragraph of the document.
pub fn lay_out_at(
    document: &Document,
    id: ObjectId,
    place: &Place,
    sheet: &StyleSheet,
    fonts: &Fonts,
    hyphenation: &Hyphenation,
) -> ParagraphLayout {
    let paragraph = document.paragraph(id).expect("a paragraph of the document");
    let style = sheet.style_of(paragraph);
    let spans = spans_at(paragraph, sheet, Some(place));
    let align = align(paragraph, sheet);
    let inset = if style.code {
        sheet.blocks.code_padding.units()
    } else {
        0
    };
    // Its own indents narrow it, but never to less than a quarter of its
    // room; its tab stops stay where they are on the ruler, measured from
    // its place's start edge; its pictures fit what's left.
    let indents = indents(paragraph);
    let room = (place.width - 2 * inset).max(1);
    let width = (room - indents.start - indents.end).max(room / 4).max(1);
    let mut tabs = paragraph_tabs(paragraph, sheet);
    for stop in &mut tabs.stops {
        stop.position -= indents.start;
    }
    let (pictures, objects) = placed_pictures(paragraph, width);
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
            line_height: line_height(paragraph),
            indent: indents.first.clamp(-width, width),
            objects: &objects,
        },
    );
    let margin = if layout.direction() == text::Direction::Rtl {
        room - width - indents.start
    } else {
        indents.start
    };
    let figure = pictures.len() == 1 && paragraph.text.chars().eq([OBJECT]);
    // The marker is in the paragraph's style, whatever its runs say.
    let marker = place.marker.as_ref().map(|marker| {
        let style = base_style(paragraph, sheet, Some(place));
        text::layout(
            fonts,
            &text::Paragraph {
                text: marker,
                spans: &[Span {
                    len: marker.len(),
                    style,
                }],
                direction: None,
                align: Align::Start,
                width: i64::MAX / 4,
            },
        )
    });
    let mut links = Vec::with_capacity(paragraph.runs.len());
    let mut decorations = Vec::with_capacity(paragraph.runs.len());
    let mut inline_code: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    let mut chars = paragraph.text.chars();
    for run in &paragraph.runs {
        let len: usize = chars
            .by_ref()
            .take(run.len as usize)
            .map(char::len_utf8)
            .sum();
        links.push(matches!(run.props.get(keys::LINK), Some(Value::Text(_))));
        decorations.push(decoration(&run.props));
        if is_code(&run.props) {
            match inline_code.last_mut() {
                Some(last) if last.end == at => last.end = at + len,
                _ => inline_code.push(at..at + len),
            }
        }
        at += len;
    }
    let (space_before, space_after) = spacing(paragraph, sheet);
    ParagraphLayout {
        id,
        layout,
        space_before,
        space_after,
        keep_with_next: style.keep_with_next,
        heading: style.heading.filter(|level| (1..=6).contains(level)),
        place: place.clone(),
        inset,
        margin,
        marker,
        code: style.code,
        links,
        decorations,
        inline_code,
        pictures,
        figure,
    }
}

/// The pictures in `paragraph`'s text, and the objects that stand for
/// them in its layout, `width` wide: each [`OBJECT`] character of a run
/// that shows a picture, at the size it's placed at, scaled down to the
/// width if it's wider, keeping its proportions.
fn placed_pictures(paragraph: &Paragraph, width: i64) -> (Vec<Picture>, Vec<text::Inline>) {
    let mut pictures = Vec::new();
    let mut objects = Vec::new();
    let mut chars = paragraph.text.char_indices();
    for run in &paragraph.runs {
        let shown = picture_of(&run.props);
        for _ in 0..run.len {
            let Some((offset, c)) = chars.next() else {
                break;
            };
            let Some((id, alt, w, h)) = &shown else {
                continue;
            };
            if c != OBJECT {
                continue;
            }
            let (w, h) = if *w > width {
                // In i128: both may be as large as LARGEST_PICTURE.
                let scaled = i128::from(*h) * i128::from(width) / i128::from(*w);
                (width, (scaled as i64).max(1))
            } else {
                (*w, *h)
            };
            pictures.push(Picture {
                offset,
                id: id.clone(),
                alt: alt.clone(),
            });
            objects.push(text::Inline {
                offset,
                width: w,
                height: h,
            });
        }
    }
    (pictures, objects)
}

/// Every paragraph of `document`'s story, in reading order, laid out at
/// its place in a body `width` wide.
pub fn lay_out(
    document: &Document,
    sheet: &StyleSheet,
    fonts: &Fonts,
    width: i64,
) -> Vec<ParagraphLayout> {
    let hyphenation = Hyphenation::new();
    flow(document, sheet, width)
        .entries
        .iter()
        .map(|entry| lay_out_at(document, entry.id, &entry.place, sheet, fonts, &hyphenation))
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

/// A block of a document to build ([`build`]), as `spec/documents/` and
/// tests give them. Text is in runs, each with its character properties.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum BlockSpec {
    /// A paragraph: its style's name and its text.
    Paragraph(String, Vec<(String, Props)>),
    /// A code block: its info string, and its lines.
    Code(String, Vec<String>),
    /// A list: whether it's numbered, and its items, each of blocks.
    List(bool, Vec<Vec<BlockSpec>>),
    /// A quote of blocks.
    Quote(Vec<BlockSpec>),
    /// A table: its columns' alignment ("start", "center", "end" or ""),
    /// and its rows, the header first, each of its cells' text.
    Table(Vec<String>, Vec<Vec<Vec<(String, Props)>>>),
}

/// The name of the style of code blocks, in M2's sheet.
pub const CODE_STYLE: &str = "Code";

/// A document whose story holds `blocks`, made with `doc`'s operations as
/// an importer would make it: each block inserted whole.
pub fn build(blocks: &[BlockSpec]) -> Document {
    let mut document = Document::new();
    let root = document.root();
    let first = document.paragraphs()[0];
    let mut after = Some(first);
    for spec in blocks {
        let mut objects = Vec::new();
        let id = objects_of(&mut document, spec, root, &mut objects);
        document
            .apply(&Op::InsertBlock {
                parent: root,
                after,
                objects,
            })
            .expect("building a document applies");
        after = Some(id);
    }
    if !blocks.is_empty() {
        document
            .apply(&Op::DeleteBlock { block: first })
            .expect("the first paragraph goes");
    }
    document
}

/// Adds `spec`'s objects, in `parent`, to `out`, it first; returns its ID.
fn objects_of(
    document: &mut Document,
    spec: &BlockSpec,
    parent: ObjectId,
    out: &mut Vec<(ObjectId, Object)>,
) -> ObjectId {
    let id = document.new_id();
    let container = |kind: Kind, props: Props| {
        Object::Block(Block {
            kind,
            parent,
            children: Vec::new(),
            props,
        })
    };
    let at = out.len();
    match spec {
        BlockSpec::Paragraph(style, runs) => {
            out.push((id, styled(parent, style, runs, Props::new())));
        }
        BlockSpec::Code(info, lines) => {
            let mut props = Props::new();
            if !info.is_empty() {
                props.insert(keys::INFO.to_string(), Value::Text(info.clone()));
            }
            let text = lines.join("\u{2028}");
            out.push((
                id,
                styled(parent, CODE_STYLE, &[(text, Props::new())], props),
            ));
        }
        BlockSpec::List(ordered, items) => {
            let mut props = Props::new();
            if *ordered {
                props.insert(keys::ORDERED.to_string(), Value::Bool(true));
            }
            out.push((id, container(Kind::List, props)));
            for blocks in items {
                let item = document.new_id();
                let item_at = out.len();
                out.push((
                    item,
                    Object::Block(Block {
                        kind: Kind::Item,
                        parent: id,
                        children: Vec::new(),
                        props: Props::new(),
                    }),
                ));
                let children: Vec<ObjectId> = blocks
                    .iter()
                    .map(|b| objects_of(document, b, item, out))
                    .collect();
                set_children(&mut out[item_at].1, children);
                push_child(&mut out[at].1, item);
            }
        }
        BlockSpec::Quote(blocks) => {
            out.push((id, container(Kind::Quote, Props::new())));
            let children: Vec<ObjectId> = blocks
                .iter()
                .map(|b| objects_of(document, b, id, out))
                .collect();
            set_children(&mut out[at].1, children);
        }
        BlockSpec::Table(align, rows) => {
            out.push((id, container(Kind::Table, Props::new())));
            for cells in rows {
                let row = document.new_id();
                let row_at = out.len();
                out.push((
                    row,
                    Object::Block(Block {
                        kind: Kind::Row,
                        parent: id,
                        children: Vec::new(),
                        props: Props::new(),
                    }),
                ));
                let mut children = Vec::new();
                for (column, runs) in cells.iter().enumerate() {
                    let cell = document.new_id();
                    let mut props = Props::new();
                    if let Some(a) = align.get(column).filter(|a| !a.is_empty()) {
                        props.insert(keys::ALIGN.to_string(), Value::Text(a.clone()));
                    }
                    // Cells name no style: they're in the default one.
                    out.push((cell, styled(row, "", runs, props)));
                    children.push(cell);
                }
                set_children(&mut out[row_at].1, children);
                push_child(&mut out[at].1, row);
            }
        }
    }
    id
}

/// A paragraph in `parent` in style `style` (none if empty), of `runs`.
fn styled<T: AsRef<str>>(
    parent: ObjectId,
    style: &str,
    runs: &[(T, Props)],
    mut props: Props,
) -> Object {
    if !style.is_empty() {
        props.insert(keys::STYLE.to_string(), Value::Text(style.to_string()));
    }
    let mut text = String::new();
    let mut out: Vec<Run> = Vec::new();
    for (t, p) in runs {
        let t = t.as_ref();
        text.push_str(t);
        let n = t.chars().count() as u32;
        if n > 0 {
            out.push(Run::new(n, p.clone()));
        }
    }
    Object::Paragraph(Paragraph {
        parent,
        text,
        runs: out,
        props,
    })
}

fn set_children(object: &mut Object, children: Vec<ObjectId>) {
    if let Object::Block(b) = object {
        b.children = children;
    }
}

fn push_child(object: &mut Object, child: ObjectId) {
    if let Object::Block(b) = object {
        b.children.push(child);
    }
}

/// `document`'s tree as lines, one an object, indented two spaces a
/// level: a paragraph as its style and text, a container as its kind and
/// properties. For tests and sessions to say what a document should be.
pub fn outline(document: &Document, sheet: &StyleSheet) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<(ObjectId, usize)> = document
        .children(document.root())
        .iter()
        .rev()
        .map(|&c| (c, 0))
        .collect();
    while let Some((id, depth)) = stack.pop() {
        let pad = "  ".repeat(depth);
        match document.object(id) {
            Some(Object::Paragraph(p)) => {
                out.push(format!("{pad}{}: {}", sheet.style_of(p).name, p.text));
            }
            Some(Object::Block(b)) => {
                let mut line = format!("{pad}{}", b.kind.name());
                for (key, value) in &b.props {
                    match value {
                        Value::Bool(true) => line.push_str(&format!(" {key}")),
                        Value::Bool(false) => {}
                        Value::Int(n) => line.push_str(&format!(" {key} {n}")),
                        Value::Text(t) => line.push_str(&format!(" {key} {t:?}")),
                        Value::Ref(id) => line.push_str(&format!(" {key} {id}")),
                    }
                }
                out.push(line);
                stack.extend(b.children.iter().rev().map(|&c| (c, depth + 1)));
            }
            _ => {}
        }
    }
    out
}

/// Character properties: `pairs` of a key and a value.
pub fn props(pairs: &[(&str, Value)]) -> Props {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
