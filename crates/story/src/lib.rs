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

pub use flow::{Ancestor, Cell, DEEPEST, Entry, Place, flow};

use std::ops::Range;

use doc::{Block, Document, Kind, Object, ObjectId, Op, Paragraph, Props, Run, Value};
use serde::Deserialize;
use text::units::Length;
use text::{Align, Fonts, Hyphenation, Language, Setting, Span, Style, TabAlign, TabStop, Tabs};

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
    /// Its list item's marker, laid out in its first span's style, if it
    /// starts an item.
    pub marker: Option<text::Layout>,
    /// Whether it's a code block.
    pub code: bool,
    /// Which of its spans are links.
    pub links: Vec<bool>,
    /// The byte ranges of its text that are code, in runs.
    pub inline_code: Vec<Range<usize>>,
}

impl ParagraphLayout {
    /// Where its text's lines start, across from the body's start edge.
    pub fn x(&self) -> i64 {
        self.place.indent + self.inset
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
            if let Some(Value::Bool(true)) = run.props.get(keys::CODE) {
                style.family = sheet.blocks.code_family.clone();
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
    let tabs = sheet.tabs(style);
    let inset = if style.code {
        sheet.blocks.code_padding.units()
    } else {
        0
    };
    let layout = text::layout_with(
        fonts,
        &text::Paragraph {
            text: &paragraph.text,
            spans: &spans,
            direction: None,
            align,
            width: (place.width - 2 * inset).max(1),
        },
        &Setting {
            hyphenate: style.hyphenate.unwrap_or(align == Align::Justify),
            hyphenation: Some(hyphenation),
            tabs: Some(&tabs),
        },
    );
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
        if let Some(Value::Bool(true)) = run.props.get(keys::CODE) {
            match inline_code.last_mut() {
                Some(last) if last.end == at => last.end = at + len,
                _ => inline_code.push(at..at + len),
            }
        }
        at += len;
    }
    ParagraphLayout {
        id,
        layout,
        space_before: style.space_before.units(),
        space_after: style.space_after.units(),
        keep_with_next: style.keep_with_next,
        heading: style.heading.filter(|level| (1..=6).contains(level)),
        place: place.clone(),
        inset,
        marker,
        code: style.code,
        links,
        inline_code,
    }
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
