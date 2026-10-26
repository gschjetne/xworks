//! The story substrate: a tree of blocks with styles, and its outline and
//! draft views (docs/ARCHITECTURE.md, section 4). M2 has its tree: lists
//! of items, quotes, code blocks and tables around paragraphs, with the
//! marks Markdown needs on their text, laid out with `text`.
//!
//! - A document's own stylesheet (M2's stage 9): [`SheetSpec`], a
//!   stylesheet as a file gives it, made the document's own by
//!   [`new_document`] and [`SheetSpec::objects`]; [`StyleSheet`], its
//!   styles resolved along their chains, with the named parts they refer
//!   to, the defaults, the page, its lists and how its blocks look
//!   ([`Blocks`]); and [`styles::find`], where a value comes from.
//!   [`merge`] brings another document's styles in, matched by role and
//!   name, as pasting and replacing a sheet do.
//! - [`flow`]: the story's paragraphs in reading order, each with its
//!   [`Place`] in the tree: indent, list marker (numbers computed, not
//!   stored), a numbered heading's number, quotes, table cell, the
//!   containers around it, and the rules across the text before it
//!   ([`Rule`], Markdown's thematic break), which have no text and go
//!   with the paragraph after them.
//! - [`spans`] and [`spans_at`]: a paragraph's text as styled spans: its
//!   style, its run's character style and marks, and its own formatting
//!   over them.
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
//!   was, unless it moves others in the tree or changes the stylesheet.
//! - A property that's unset, or set to a value of the wrong kind or past
//!   its limits, takes its style's value; a style that isn't in the sheet
//!   is the default one; a value the sheet lacks is the program's.
//! - Neutral, the sheet new documents take unless the person chooses, lays
//!   a story out as M2 did before documents held their own styles.

mod flow;
mod merge;
pub mod sheet;
pub mod styles;

pub use flow::{Ancestor, Cell, DEEPEST, Entry, Flow, Place, Rule, flow};
pub use merge::{Change, changes, merge};
pub use sheet::{SheetError, SheetSpec};
pub use styles::{
    Blocks, CharValues, CharacterStyle, DocumentTabs, Level, ListStyle, PageStyle, ParagraphStyle,
    Part, Rgb, Source, StyleSheet,
};

use std::ops::Range;

use doc::{Block, Document, Kind, Object, ObjectId, Op, Paragraph, Props, Run, Snapshot, Value};
use serde::{Deserialize, Serialize};
use text::units::Length;
use text::{Align, Fonts, Hyphenation, Language, Setting, Span, Style, TabAlign, TabStop, Tabs};

/// The names of the properties a story reads, from paragraphs, from runs
/// of text, from containers, and from the stylesheet's styles, parts and
/// defaults. A style sets a value under the same key a paragraph or run
/// sets it under, as its own formatting.
pub mod keys {
    /// A paragraph's style, by ID (a reference).
    pub const STYLE: &str = "style";
    /// A run's character style, by ID (a reference).
    pub const CHAR_STYLE: &str = "char-style";
    /// A style's or a part's name (text).
    pub const NAME: &str = "name";
    /// What a style or a part is for (text), by which another sheet's
    /// matches it: "body", "heading-1", "emphasis", "text".
    pub const ROLE: &str = "role";
    /// Text's colour (text, `#rrggbb`, or a reference to a colour).
    pub const COLOR: &str = "color";
    /// Whether text is underlined (on or off).
    pub const UNDERLINE: &str = "underline";
    /// The classes of ligature text is set with (text): those on, by name,
    /// separated by spaces.
    pub const LIGATURES: &str = "ligatures";
    /// Whether a paragraph stays on the page where the next one starts (on
    /// or off).
    pub const KEEP_WITH_NEXT: &str = "keep-with-next";
    /// A paragraph style's level as a heading, 1 to 6; 0 for none (a
    /// number).
    pub const HEADING: &str = "heading";
    /// Whether a paragraph's words are hyphenated (on or off); unset, they
    /// are where its lines are justified.
    pub const HYPHENATE: &str = "hyphenate";
    /// Whether a paragraph style is numbered, as a technical document's
    /// headings are (on or off).
    pub const NUMBERED: &str = "numbered";
    /// A face's font family (text).
    pub const FAMILY: &str = "family";
    /// A list definition's bullets, by depth, separated by spaces (text).
    pub const BULLETS: &str = "bullets";
    /// How a list definition's numbers read, `1` standing for the number
    /// (text).
    pub const NUMBERS: &str = "numbers";
    /// How far each level of a list indents its items (a number).
    pub const LIST_INDENT: &str = "list-indent";
    /// The room between a list item's marker and its text (a number).
    pub const MARKER_GAP: &str = "marker-gap";
    /// The list definition a list takes, if not its sheet's for lists
    /// numbered or not (a reference).
    pub const LIST: &str = "list";
    /// A page style's width (a number).
    pub const PAGE_WIDTH: &str = "page-width";
    /// A page style's height (a number).
    pub const PAGE_HEIGHT: &str = "page-height";
    /// A page style's margin at the top (a number).
    pub const MARGIN_TOP: &str = "margin-top";
    /// A page style's margin at the bottom (a number).
    pub const MARGIN_BOTTOM: &str = "margin-bottom";
    /// A page style's margin at the left (a number).
    pub const MARGIN_LEFT: &str = "margin-left";
    /// A page style's margin at the right (a number).
    pub const MARGIN_RIGHT: &str = "margin-right";
    /// The stylesheet a document's was copied from (text).
    pub const ORIGIN: &str = "origin";
    /// That stylesheet's version (a number).
    pub const ORIGIN_VERSION: &str = "origin-version";
    /// The distance between a document's default tab stops (a number).
    pub const TAB_INTERVAL: &str = "tab-interval";
    /// The tab stops every paragraph of a document has besides its style's
    /// (text, as [`TABS`] holds them, or a reference to a tab layout).
    pub const DOCUMENT_TABS: &str = "document-tabs";
    /// How far a quote indents its blocks, its bar included (a number).
    pub const QUOTE_INDENT: &str = "quote-indent";
    /// How wide a quote's bar is (a number).
    pub const QUOTE_BAR: &str = "quote-bar";
    /// The colour of a quote's bar (as [`COLOR`]).
    pub const QUOTE_COLOR: &str = "quote-color";
    /// The space above and below a quote, besides its first and last
    /// blocks' own (a number); none where it's unset.
    pub const QUOTE_SPACE: &str = "quote-space";
    /// The tint behind code (as [`COLOR`]).
    pub const CODE_TINT: &str = "code-tint";
    /// How far a code block's tint reaches around its text (a number).
    pub const CODE_PADDING: &str = "code-padding";
    /// The space above and below a table (a number).
    pub const TABLE_SPACE: &str = "table-space";
    /// The room around a cell's text (a number).
    pub const CELL_PADDING: &str = "cell-padding";
    /// The width of a table's rules above and below it and under its
    /// header (a number).
    pub const RULE: &str = "rule";
    /// The width of the rules between a table's rows (a number).
    pub const HAIRLINE: &str = "hairline";
    /// The colour of a table's rules (as [`COLOR`]).
    pub const RULE_COLOR: &str = "rule-color";
    /// The colour of the rules between its rows (as [`COLOR`]).
    pub const HAIRLINE_COLOR: &str = "hairline-color";
    /// The room above and below a rule across the text, Markdown's
    /// thematic break, which is drawn a hairline wide in the rules' colour
    /// (a number).
    pub const RULE_SPACE: &str = "rule-space";
    /// The colour drawn where a picture can't be (as [`COLOR`]).
    pub const MISSING_PICTURE: &str = "missing-picture";
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
    /// "definition", "footnote", "metadata", "alert" and the like): what a
    /// story doesn't hold, such as HTML, a link reference definition or
    /// GitHub's alert's `[!NOTE]`, as its source. On a code block it's a
    /// block of it, on a run of text it's inline, as inline HTML is;
    /// either way it's shown as code is, and written back as it is.
    pub const SOURCE: &str = "source";
    /// Whether a list item is a task, and done (on or off): its box,
    /// checked or not, as GitHub's task lists' `- [ ]` and `- [x]`. Unset,
    /// it's an item as any other.
    pub const TASK: &str = "task";
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

/// `paragraph`'s indents: its own, where it sets them, and otherwise its
/// style's; those past what makes sense are taken as unset.
pub fn indents(paragraph: &Paragraph, sheet: &StyleSheet) -> Indents {
    let style = sheet.style_of(paragraph).indents;
    let get = |key, or| number(paragraph, key, -MOST_SPACE..=MOST_SPACE).unwrap_or(or);
    Indents {
        start: get(keys::INDENT_START, style.start),
        end: get(keys::INDENT_END, style.end),
        first: get(keys::INDENT_FIRST, style.first),
    }
}

/// `paragraph`'s indents at `place`, as [`indents`] gives them, but where
/// its container lends it its geometry (project owner, 2026-10-24, review
/// 30dmn1js, "containers lend geometry only"): a table's cell takes only
/// its own indents, as the table places its text, and a paragraph that
/// starts a list item, or is numbered, only its own first-line indent,
/// as its marker stands where that indent would.
pub fn indents_at(paragraph: &Paragraph, sheet: &StyleSheet, place: &Place) -> Indents {
    let own = |key| number(paragraph, key, -MOST_SPACE..=MOST_SPACE);
    if place.cell.is_some() {
        return Indents {
            start: own(keys::INDENT_START).unwrap_or(0),
            end: own(keys::INDENT_END).unwrap_or(0),
            first: own(keys::INDENT_FIRST).unwrap_or(0),
        };
    }
    let mut indents = indents(paragraph, sheet);
    if place.marker.is_some() || place.task.is_some() {
        indents.first = own(keys::INDENT_FIRST).unwrap_or(0);
    }
    indents
}

/// How `paragraph`'s lines are aligned at `place`, as [`align`] gives it,
/// but a table's cell as it says, as its column's alignment, or else to
/// the start: the table places its cells' text.
pub fn align_at(paragraph: &Paragraph, sheet: &StyleSheet, place: &Place) -> Align {
    if place.cell.is_some() {
        return match paragraph.props.get(keys::ALIGN) {
            Some(Value::Text(align)) => sheet::align_of(align).unwrap_or(Align::Start),
            _ => Align::Start,
        };
    }
    align(paragraph, sheet)
}

/// How tall `paragraph`'s lines are, in thousandths of their font size,
/// from a quarter of the font to ten times it: its own, where it sets it,
/// or its style's; none, as tall as their fonts make them, where it's 0
/// or neither says.
pub fn line_height(paragraph: &Paragraph, sheet: &StyleSheet) -> Option<i64> {
    match paragraph.props.get(keys::LINE_HEIGHT) {
        Some(Value::Int(0)) => None,
        _ => number(paragraph, keys::LINE_HEIGHT, 250..=10_000)
            .or(sheet.style_of(paragraph).line_height),
    }
}

/// The space above and below `paragraph`: its own where it sets them,
/// otherwise its style's.
pub fn spacing(paragraph: &Paragraph, sheet: &StyleSheet) -> (i64, i64) {
    let style = sheet.style_of(paragraph);
    (
        number(paragraph, keys::SPACE_BEFORE, 0..=MOST_SPACE).unwrap_or(style.space_before),
        number(paragraph, keys::SPACE_AFTER, 0..=MOST_SPACE).unwrap_or(style.space_after),
    )
}

/// The tab stops `paragraph`'s tabs go to, measured from its place's
/// start edge: its own, where it sets them (stops, or a layout of them
/// the sheet has), or its style's and the document's; then the document's
/// default ones.
pub fn paragraph_tabs(paragraph: &Paragraph, sheet: &StyleSheet) -> Tabs {
    let own = match paragraph.props.get(keys::TABS) {
        Some(Value::Text(stops)) => parse_tabs(stops),
        Some(Value::Ref(id)) => sheet
            .part(*id)
            .filter(|p| p.kind == doc::StyleKind::Tabs)
            .and_then(|p| parse_tabs(&p.value)),
        _ => None,
    };
    match own {
        Some(stops) => Tabs {
            stops,
            interval: sheet.tabs.interval,
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

/// A tab stop, as a stylesheet file gives it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct TabStopSpec {
    /// Its distance from the paragraph's start edge.
    pub at: Length,
    /// How the text after a tab sits at it.
    pub align: TabAlign,
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
    /// The colour of each of its spans.
    pub colors: Vec<Rgb>,
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
/// with the run's character style, marks and own formatting over it.
pub fn spans(paragraph: &Paragraph, sheet: &StyleSheet) -> Vec<Span> {
    spans_at(paragraph, sheet, None)
}

/// `paragraph`'s text as spans, as [`spans`] gives them, for a paragraph
/// at `place`: a table's header cells are bold.
pub fn spans_at(paragraph: &Paragraph, sheet: &StyleSheet, place: Option<&Place>) -> Vec<Span> {
    styled(paragraph, sheet, place).0
}

/// How a span of text looks besides its letters: its colour, and the lines
/// drawn with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// Its colour.
    pub color: Rgb,
    /// The lines drawn with it.
    pub decoration: text::Decoration,
}

/// `paragraph`'s text as spans, and how each looks besides its letters:
/// its paragraph style's values; then for each run its character style's,
/// each of its marks' character style's, in [`styles::MARKS`]' order, and
/// its own formatting's ([`styles::CHARACTER_FORMATTING`]), each over the
/// last; then its language.
pub fn styled(
    paragraph: &Paragraph,
    sheet: &StyleSheet,
    place: Option<&Place>,
) -> (Vec<Span>, Vec<Look>) {
    let (base, look) = base_style(paragraph, sheet, place);
    let mut chars = paragraph.text.chars();
    let mut spans = Vec::with_capacity(paragraph.runs.len().max(1));
    let mut looks = Vec::with_capacity(paragraph.runs.len().max(1));
    for run in &paragraph.runs {
        let len = chars
            .by_ref()
            .take(run.len as usize)
            .map(char::len_utf8)
            .sum();
        // Most runs set nothing: they're the paragraph's.
        if run.props.is_empty() {
            spans.push(Span {
                len,
                style: base.clone(),
            });
            looks.push(look);
            continue;
        }
        let mut style = base.clone();
        let mut seen = look;
        if let Some(Value::Ref(id)) = run.props.get(keys::CHAR_STYLE)
            && let Some(c) = sheet.character_style(*id)
        {
            apply(&mut style, &mut seen, &c.values);
        }
        for (k, (key, _)) in styles::MARKS.iter().enumerate() {
            if styles::has_mark(&run.props, key) {
                apply(&mut style, &mut seen, sheet.mark(k));
            }
        }
        apply(&mut style, &mut seen, &direct(&run.props, sheet));
        if let Some(language) = language(run.props.get(keys::LANGUAGE)) {
            style.language = language;
        }
        spans.push(Span { len, style });
        looks.push(seen);
    }
    if spans.is_empty() {
        spans.push(Span {
            len: 0,
            style: base,
        });
        looks.push(look);
    }
    (spans, looks)
}

/// What `values` set, over `style` and `look`.
fn apply(style: &mut Style, look: &mut Look, values: &CharValues) {
    if let Some(family) = &values.family {
        style.family = family.clone();
    }
    if let Some(size) = values.size {
        style.size = size;
    }
    if let Some(bold) = values.bold {
        style.weight = if bold { 700 } else { 400 };
    }
    if let Some(italic) = values.italic {
        style.italic = italic;
    }
    if let Some(ligatures) = values.ligatures {
        style.ligatures = ligatures;
    }
    if let Some(color) = values.color {
        look.color = color;
    }
    if let Some(underline) = values.underline {
        look.decoration.underline = underline;
    }
    if let Some(strikethrough) = values.strikethrough {
        look.decoration.strikethrough = strikethrough;
    }
}

/// What a run with `props` sets itself, its direct formatting, parts it
/// names taken from `sheet`.
pub fn direct(props: &Props, sheet: &StyleSheet) -> CharValues {
    let part = |value: Option<&Value>, kind: doc::StyleKind| match value {
        Some(Value::Text(t)) => Some(t.clone()),
        Some(Value::Ref(id)) => sheet
            .part(*id)
            .filter(|p| p.kind == kind)
            .map(|p| p.value.clone()),
        _ => None,
    };
    let ok = |key: &str, v: &Value| styles::valid(key)(v);
    CharValues {
        family: part(props.get(keys::FONT), doc::StyleKind::Face)
            .filter(|f| ok(keys::FONT, &Value::Text(f.clone()))),
        size: match props.get(keys::SIZE) {
            Some(v @ Value::Int(n)) if ok(keys::SIZE, v) => Some(*n),
            _ => None,
        },
        color: part(props.get(keys::COLOR), doc::StyleKind::Colour)
            .and_then(|c| sheet::parse_colour(&c)),
        underline: match props.get(keys::UNDERLINE) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        },
        ..CharValues::default()
    }
}

/// The lines drawn with a run of text with `props` that sets them itself:
/// through it, where it's struck through.
pub fn decoration(props: &Props) -> text::Decoration {
    text::Decoration {
        underline: matches!(props.get(keys::UNDERLINE), Some(Value::Bool(true))),
        strikethrough: matches!(props.get(keys::STRIKETHROUGH), Some(Value::Bool(true))),
    }
}

/// The style of `paragraph`'s text before its runs' own properties, and
/// how it looks: its paragraph style's, and bold in a table's header.
fn base_style(paragraph: &Paragraph, sheet: &StyleSheet, place: Option<&Place>) -> (Style, Look) {
    let style = sheet.style_of(paragraph);
    let header = place
        .and_then(|p| p.cell)
        .is_some_and(|cell| cell.index == 0);
    (
        Style {
            family: style.family.clone(),
            size: style.size,
            weight: if style.bold || header { 700 } else { 400 },
            italic: style.italic,
            language: language(paragraph.props.get(keys::LANGUAGE)).unwrap_or_default(),
            ligatures: style.ligatures,
        },
        Look {
            color: style.color,
            decoration: text::Decoration {
                underline: style.underline,
                strikethrough: style.strikethrough,
            },
        },
    )
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
        Some(Value::Text(align)) => {
            sheet::align_of(align).unwrap_or_else(|| sheet.style_of(paragraph).align)
        }
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
    let (spans, looks) = styled(paragraph, sheet, Some(place));
    let align = align_at(paragraph, sheet, place);
    let inset = if style.code {
        sheet.blocks.code_padding
    } else {
        0
    };
    // Its own indents narrow it, but never to less than a quarter of its
    // room; its tab stops stay where they are on the ruler, measured from
    // its place's start edge; its pictures fit what's left.
    let indents = indents_at(paragraph, sheet, place);
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
            line_height: line_height(paragraph, sheet),
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
        let style = base_style(paragraph, sheet, Some(place)).0;
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
    let colors = looks.iter().map(|l| l.color).collect();
    let decorations = looks.iter().map(|l| l.decoration).collect();
    let mut inline_code: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    let mut chars = paragraph.text.chars();
    for run in &paragraph.runs {
        let len: usize = chars
            .by_ref()
            .take(run.len as usize)
            .map(char::len_utf8)
            .sum();
        links.push(styles::has_mark(&run.props, keys::LINK));
        if styles::has_mark(&run.props, keys::CODE) {
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
        colors,
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

/// Where a new document's stylesheet's objects start, a Markdown file's
/// read among them: #2, after the story and its first paragraph.
pub const SHEET_AT: u64 = 2;

/// A new document in the stylesheet `spec` gives, holding one empty
/// paragraph in its default style: the story at #0, the paragraph at #1,
/// and the stylesheet's objects from [`SHEET_AT`] on.
pub fn new_document(spec: &SheetSpec) -> Document {
    let (root, first) = (ObjectId(0), ObjectId(1));
    let sheet = spec.objects(SHEET_AT);
    let next_id = SHEET_AT + sheet.len() as u64;
    let body = default_style_id(&sheet);
    let mut props = Props::new();
    if let Some(body) = body {
        props.insert(keys::STYLE.to_string(), Value::Ref(body));
    }
    let mut objects = vec![
        (
            root,
            Object::Story(doc::Story {
                blocks: vec![first],
            }),
        ),
        (
            first,
            Object::Paragraph(Paragraph {
                parent: root,
                text: String::new(),
                runs: Vec::new(),
                props,
            }),
        ),
    ];
    objects.extend(sheet);
    Document::from_snapshot(Snapshot {
        root,
        next_id,
        objects,
    })
    .unwrap_or_default()
}

/// `document`'s stylesheet as objects, the sheet first and then its styles
/// and parts in order, each by its ID: none if it has no sheet.
pub fn sheet_objects(document: &Document) -> Vec<(ObjectId, Object)> {
    let Some(sheet) = document.sheet() else {
        return Vec::new();
    };
    std::iter::once(sheet)
        .chain(document.styles().iter().copied())
        .filter_map(|id| Some((id, document.object(id)?.clone())))
        .collect()
}

/// The paragraph style among a sheet's objects that paragraphs naming
/// none take: the one with the role "body", or the first.
pub fn default_style_id(objects: &[(ObjectId, Object)]) -> Option<ObjectId> {
    let paragraph_styles = || {
        objects.iter().filter_map(|(id, o)| match o {
            Object::Style(s) if s.kind == doc::StyleKind::Paragraph => Some((*id, s)),
            _ => None,
        })
    };
    paragraph_styles()
        .find(|(_, s)| s.props.get(keys::ROLE) == Some(&Value::Text("body".into())))
        .or_else(|| paragraph_styles().next())
        .map(|(id, _)| id)
}

/// The ID of `document`'s paragraph style named `name`, if it has one.
pub fn style_named(document: &Document, name: &str) -> Option<ObjectId> {
    document.styles().iter().copied().find(|&id| {
        document.style(id).is_some_and(|s| {
            s.kind == doc::StyleKind::Paragraph
                && s.props.get(keys::NAME) == Some(&Value::Text(name.to_string()))
        })
    })
}

/// The ID of `document`'s paragraph style with role `role`, if it has one.
pub fn style_with_role(document: &Document, role: &str) -> Option<ObjectId> {
    document.styles().iter().copied().find(|&id| {
        document.style(id).is_some_and(|s| {
            s.kind == doc::StyleKind::Paragraph
                && s.props.get(keys::ROLE) == Some(&Value::Text(role.to_string()))
        })
    })
}

/// A document in the stylesheet `spec` gives, whose story holds
/// `paragraphs`, each a style's name and its text in runs, each run with
/// its character properties. It's made with `doc`'s operations, as an
/// importer would make it; a name the sheet hasn't is the default style.
pub fn document<S: AsRef<str>, T: AsRef<str>>(
    spec: &SheetSpec,
    paragraphs: &[(S, Vec<(T, Props)>)],
) -> Document {
    let mut document = new_document(spec);
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
        let id = style_named(&document, style.as_ref());
        apply(
            &mut document,
            Op::SetParaProp {
                paragraph: current,
                key: keys::STYLE.to_string(),
                value: id.map(Value::Ref),
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
    /// A rule across the text, as Markdown's thematic break.
    Rule,
}

/// A document in the stylesheet `spec` gives, whose story holds
/// `blocks`, made with `doc`'s operations as an importer would make it:
/// each block inserted whole. Paragraphs name styles by name, a name the
/// sheet hasn't the default style; code blocks take the sheet's style
/// with the role "code".
pub fn build(spec: &SheetSpec, blocks: &[BlockSpec]) -> Document {
    let mut document = new_document(spec);
    let root = document.root();
    let first = document.paragraphs()[0];
    let mut after = Some(first);
    for block in blocks {
        let mut objects = Vec::new();
        let id = objects_of(&mut document, block, root, &mut objects);
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
            let style = style_named(document, style);
            out.push((id, paragraph_object(parent, style, runs, Props::new())));
        }
        BlockSpec::Code(info, lines) => {
            let mut props = Props::new();
            if !info.is_empty() {
                props.insert(keys::INFO.to_string(), Value::Text(info.clone()));
            }
            let text = lines.join("\u{2028}");
            let style = style_with_role(document, "code");
            out.push((
                id,
                paragraph_object(parent, style, &[(text, Props::new())], props),
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
                    out.push((cell, paragraph_object(row, None, runs, props)));
                    children.push(cell);
                }
                set_children(&mut out[row_at].1, children);
                push_child(&mut out[at].1, row);
            }
        }
        BlockSpec::Rule => out.push((id, container(Kind::Rule, Props::new()))),
    }
    id
}

/// A paragraph in `parent` in style `style`, if it names one, of `runs`.
fn paragraph_object<T: AsRef<str>>(
    parent: ObjectId,
    style: Option<ObjectId>,
    runs: &[(T, Props)],
    mut props: Props,
) -> Object {
    if let Some(style) = style {
        props.insert(keys::STYLE.to_string(), Value::Ref(style));
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
