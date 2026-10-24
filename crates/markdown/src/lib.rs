//! Markdown files as themselves (docs/ARCHITECTURE.md, section 6; review
//! ys9vyag1): a `.md` file read into a structure-only story, and the story
//! written back as Markdown.
//!
//! - [`read`]: CommonMark with GitHub's tables, strikethrough and task
//!   lists, read with pulldown-cmark into a [`Document`]: headings are the
//!   Heading styles; paragraphs, lists, quotes, code blocks and tables are
//!   stage 2's blocks, and thematic breaks are rules; emphasis, strong,
//!   code, links and lines through text are marks, and GitHub's autolinks,
//!   bare web and e-mail addresses, are links ([`autolinks`]); images are
//!   pictures in the text, linked by their paths
//!   (`story::keys::PICTURE_SRC`). What a
//!   story doesn't hold, such as HTML, link reference definitions,
//!   footnotes and the `[!NOTE]` of GitHub's alerts, which are quotes, is
//!   kept as it was written (`story::keys::SOURCE`). YAML or TOML front
//!   matter is kept as it was, outside the story, and its `lang` is the
//!   paragraphs' language.
//! - [`File`]: what the file said, kept for writing it back:
//!   [`File::write`] writes each block the story still holds as it was
//!   read byte for byte as its author wrote it, and those edited or new in
//!   the file's own habits where they're clear (its bullets, emphasis,
//!   numbering and wrapping width) or else a standard style: `#` headings,
//!   `-` bullets, `1.` numbers, `*emphasis*`, `**strong**`, fenced code
//!   with backticks, and a paragraph on one line. A link whose text is its
//!   address is written bare, as GitHub reads it, where that reads back
//!   the same, and plain text that would read as one has a character of
//!   it escaped. Line endings, the byte order mark and the front matter
//!   stay as they were, but for the front matter's `lang`, which all the
//!   text put in another language changes, or adds, with front matter
//!   where there's none.
//! - [`Unheld`]: what Markdown can't hold, such as a font or a paragraph's
//!   spacing, which [`File::write`] refuses so the app can ask before the
//!   document becomes an `.xw` file.
//! - [`shape`]: a story as Markdown holds it, for comparing what's written
//!   with what's read back.
//!
//! Invariants:
//!
//! - Reading never fails on UTF-8 and never panics; a file that isn't
//!   UTF-8 is an error.
//! - A file read and written back without edits is the same bytes.
//! - What's written reads back as the same story, as [`shape`] sees it:
//!   each edited block is read back as it's written, and written more
//!   plainly, with more escapes, if it doesn't read back the same.
//! - Nothing walks the story by recursion, so no file can exhaust the
//!   stack.

mod autolink;
mod inline;
mod read;
mod shape;
mod write;

use std::collections::HashMap;
use std::ops::Range;

use doc::{Document, ObjectId};

pub use autolink::{Autolink, AutolinkKind, address, autolinks};
pub use read::{ReadError, Settings, links, read};
pub use shape::{Form, Node, shape};
pub use write::{Unheld, What};

/// The extensions read, besides CommonMark: GitHub's tables,
/// strikethrough, task lists and footnotes. Footnotes are read so they're
/// kept as they were written rather than taken for links. GitHub's alerts
/// are read as the quotes they are, their `[!NOTE]` kept as written
/// (review jy4a2tx5).
pub(crate) const OPTIONS: pulldown_cmark::Options = pulldown_cmark::Options::ENABLE_TABLES
    .union(pulldown_cmark::Options::ENABLE_STRIKETHROUGH)
    .union(pulldown_cmark::Options::ENABLE_TASKLISTS)
    .union(pulldown_cmark::Options::ENABLE_FOOTNOTES);

/// A Markdown file, read: its story, and what writing it back needs.
#[derive(Clone, Debug)]
pub struct Read {
    /// The story.
    pub document: Document,
    /// The file.
    pub file: File,
}

/// What a Markdown file said, for writing its story back: its text, each
/// block's place in it and how it was written, the file's habits, and its
/// story as it was read, to tell which blocks were edited.
#[derive(Clone, Debug)]
pub struct File {
    /// Its text, without a byte order mark.
    text: String,
    /// Whether it began with a byte order mark.
    bom: bool,
    /// Its line ending, for lines written afresh.
    ending: &'static str,
    /// Whether its last line ended in a line ending.
    final_newline: bool,
    /// Its front matter's lines, kept as they were.
    front: Option<Range<usize>>,
    /// The language its paragraphs are in: its front matter's `lang`, or
    /// the one it was read in.
    language: Option<String>,
    /// How it writes what it has.
    habits: Habits,
    /// The story as it was read.
    baseline: Document,
    /// Each object read from it, but the story, by ID.
    sources: HashMap<ObjectId, Source>,
    /// How each paragraph wrote its marks, line breaks and links, where
    /// it wrote them its own way, by its ID.
    inline: HashMap<ObjectId, InlineHabits>,
    /// The paragraphs that keep its link reference definitions and
    /// footnotes.
    definitions: Vec<ObjectId>,
    /// What [`File::write`] last made of blocks written afresh, and the
    /// document it wrote, whose objects it was made from.
    cache: Option<write::Cache>,
}

impl File {
    /// An empty file, as a new Markdown document is: its paragraphs in
    /// `language`, its lines ending in line feeds.
    pub fn empty(language: Option<&str>) -> File {
        File {
            text: String::new(),
            bom: false,
            ending: "\n",
            final_newline: true,
            front: None,
            language: language.map(str::to_string),
            habits: Habits::default(),
            baseline: Document::new(),
            sources: HashMap::new(),
            inline: HashMap::new(),
            definitions: Vec::new(),
            cache: None,
        }
    }

    /// The language its paragraphs are in, if it says or was read in one.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Its front matter, as it was written, if it has some.
    pub fn front_matter(&self) -> Option<&str> {
        self.front.clone().map(|r| &self.text[r])
    }

    /// Its line ending: "\n" or "\r\n".
    pub fn line_ending(&self) -> &str {
        self.ending
    }

    /// Whether it began with a byte order mark.
    pub fn bom(&self) -> bool {
        self.bom
    }
}

/// How a file writes what it has, where it's clear; otherwise the
/// standard style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Habits {
    /// Bullets: `-`, `*` or `+`.
    pub bullet: char,
    /// What follows an ordered list's numbers: `.` or `)`.
    pub delimiter: char,
    /// Whether ordered lists count up, rather than repeat their first
    /// number.
    pub counting: bool,
    /// Emphasis: `*` or `_`.
    pub emphasis: char,
    /// Strong emphasis: `*` or `_`, doubled.
    pub strong: char,
    /// The width paragraphs are wrapped at, if they are.
    pub width: Option<usize>,
    /// The character code fences are made of: `` ` `` or `~`.
    pub fence: char,
    /// Whether tables pad their cells so their columns line up.
    pub padded: bool,
}

impl Default for Habits {
    fn default() -> Habits {
        Habits {
            bullet: '-',
            delimiter: '.',
            counting: true,
            emphasis: '*',
            strong: '*',
            width: None,
            fence: '`',
            padded: true,
        }
    }
}

/// How a paragraph wrote its marks, line breaks and links: those it
/// writes again when it's edited, over the file's habits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InlineHabits {
    /// Emphasis: `*` or `_`.
    pub emphasis: Option<char>,
    /// Strong emphasis: `*` or `_`.
    pub strong: Option<char>,
    /// Whether its line breaks were two spaces at a line's end, rather
    /// than a backslash.
    pub spaced_breaks: bool,
    /// The links it wrote by reference.
    pub references: Vec<Reference>,
    /// The addresses it wrote as links in angle brackets, `<…>`, which it
    /// writes so again rather than bare.
    pub angled: Vec<String>,
}

/// A link a paragraph wrote by reference to a link reference definition:
/// its target and title, as the definition gives them, the definition's
/// label as the link wrote it, and whether the link was its label alone,
/// `[label]`, rather than `[text][label]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub url: String,
    pub title: String,
    pub label: String,
    pub shortcut: bool,
}

/// Where a block read from a file was in it, and how it was written.
#[derive(Clone, Debug)]
pub(crate) struct Source {
    /// Its whole lines: from its first line's start to its last's line
    /// ending, blank lines after it left out. Empty for a block the reader
    /// made, such as the paragraph in an empty list item.
    pub lines: Range<usize>,
    /// The container it was in.
    pub parent: ObjectId,
    /// The block before it there, if any.
    pub prev: Option<ObjectId>,
    /// The list items whose markers are on its first line, innermost
    /// first: those it's the first block of.
    pub first_of: Vec<ObjectId>,
    /// How it was written.
    pub syntax: Syntax,
}

/// How a block was written.
#[derive(Clone, Debug)]
pub(crate) enum Syntax {
    /// A paragraph, or a table's cell: its lines.
    Paragraph(Vec<TextLine>),
    /// A heading: underlined (setext) with this character, or with `#`s.
    Heading(Option<char>),
    /// A code block: fenced with this many of this character, or
    /// indented.
    Code(Option<(char, usize)>),
    /// Markdown kept as it was written.
    Raw,
    /// A thematic break, which is a rule.
    Rule,
    /// A list: its bullet or delimiter, whether its items are apart, with
    /// blank lines between them, and whether its numbers count up.
    List {
        marker: char,
        loose: bool,
        counting: bool,
    },
    /// A list item: its marker (`-` or `12.`), the spaces before it, after
    /// what the containers around it put there, and after it, and its
    /// task's box, `[ ]` or `[x]`, if it's a task. With no spaces before
    /// it, where they're tabs or what's there can't be told apart.
    Item {
        marker: Range<usize>,
        indent: Option<usize>,
        spaces: usize,
        task: Option<Range<usize>>,
    },
    /// A quote: what starts each of its lines, `> ` or `>`; none if it's
    /// a tab after the `>`, whose width depends on where it is.
    Quote(Option<String>),
    /// A table: its delimiter row's line, and its columns' widths if it's
    /// padded.
    Table {
        delimiter: Range<usize>,
        widths: Option<Vec<usize>>,
    },
    /// A table's row.
    Row,
    /// Made by the reader, not in the file.
    Made,
}

/// A line of a paragraph's text as the file had it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextLine {
    /// The whole line, without its line ending.
    pub whole: Range<usize>,
    /// Where its text starts, after what the containers around it put
    /// there.
    pub content: usize,
    /// The paragraph's characters it holds: from its first, to the soft
    /// break's space after it, or after its hard break.
    pub chars: Range<u32>,
    /// Whether a soft break ends it, which the paragraph's text holds as a
    /// space.
    pub soft: bool,
}
