//! Writing a story back as Markdown: each block it still holds as it was
//! read, unedited and where it was, is written as its author wrote it,
//! byte for byte; the rest afresh, in the file's habits, and read back to
//! check it says what the story does.
//!
//! The story is walked without recursion. Each container around what's
//! written puts its own marks before each line: a list item its marker on
//! its first line and spaces on the rest, a quote its `>`. A block written
//! as it was read already has them, as its author wrote them, so it's
//! written so only where every container around it is too, as it was.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use doc::{Document, Kind, Object, ObjectId, Paragraph, Props, Value};
use story::{StyleSheet, keys};

use crate::inline::{self, Style, Written};
use crate::read::{self, Settings, line_end, without_ending};
use crate::shape::{Form, Node, canonical, shape_of, text_kind, vanishes};
use crate::{File, Habits, Source, Syntax, TextLine};

/// What a story holds that Markdown can't: what it is, and the block that
/// has it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Markdown can't hold {what}")]
pub struct Unheld {
    /// The block that has it.
    pub block: ObjectId,
    /// What it is.
    pub what: What,
}

/// What Markdown can't hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// A font set on text.
    Font,
    /// A size set on text.
    Size,
    /// A paragraph's own alignment, or a table's cell's that its column
    /// doesn't share.
    Alignment,
    /// A paragraph's own space above or below.
    Spacing,
    /// A paragraph's own indents.
    Indents,
    /// A paragraph's own line height.
    LineHeight,
    /// A paragraph's own tab stops.
    Tabs,
    /// Text in another language than the file's.
    Language,
    /// A style Markdown has no way to write.
    Style(String),
    /// A picture kept in the document rather than linked.
    KeptPicture,
    /// A heading or code in a table's cell.
    BlockInCell,
    /// A table's row with more cells than its header, or a header of none.
    Columns,
    /// A task whose item doesn't start with text.
    TaskStart,
    /// Marks where Markdown can't write them, such as emphasis that starts
    /// with punctuation in the middle of a word.
    Marks,
    /// Another property of the story's.
    Property(String),
    /// A change to the document's stylesheet, which Markdown has no room
    /// for: a Markdown file takes the program's.
    Stylesheet,
}

impl fmt::Display for What {
    /// What it is, as a sentence says it: "a font".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            What::Font => f.write_str("a font"),
            What::Size => f.write_str("a size"),
            What::Alignment => f.write_str("an alignment"),
            What::Spacing => f.write_str("a paragraph's spacing"),
            What::Indents => f.write_str("indents"),
            What::LineHeight => f.write_str("a line height"),
            What::Tabs => f.write_str("tab stops"),
            What::Language => f.write_str("text in another language"),
            What::Style(name) => write!(f, "the style {name}"),
            What::KeptPicture => f.write_str("a picture kept in the document"),
            What::BlockInCell => f.write_str("a heading or code in a table's cell"),
            What::Columns => f.write_str("a row longer than its table's header"),
            What::TaskStart => f.write_str("a task that doesn't start with text"),
            What::Marks => f.write_str("marks where Markdown can't write them"),
            What::Property(key) => write!(f, "{key}"),
            What::Stylesheet => f.write_str("a change to the stylesheet"),
        }
    }
}

/// A line written: one of the file's, as it was, without its line ending,
/// its containers' marks in it, and where its text starts after them; or
/// written afresh, to go after theirs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Whole(Range<usize>, usize),
    Fresh(String),
}

/// What writing a block afresh made, kept so the next writing reuses it
/// while the block stays the same: whether its lines could be the file's,
/// on its first line and the rest, and its lines.
#[derive(Clone, Debug)]
pub(crate) struct Made {
    first: bool,
    rest: bool,
    lines: Vec<Line>,
}

/// What a container being written is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Root,
    List,
    Item,
    Quote,
}

/// A container being written.
struct Open {
    id: ObjectId,
    frame: Frame,
    children: Vec<ObjectId>,
    next: usize,
    /// Whether it, and every container around it, is written as it was
    /// read, so lines of the file inside it can be written as they were.
    original: bool,
    /// What it puts before its first line, and the rest.
    first: String,
    rest: String,
    /// Whether its first line is still to come.
    pending: bool,
    /// Changes to its first line, where that's one of the file's: a list
    /// item's number or box, changed.
    splices: Vec<(Range<usize>, String)>,
    /// The block last written in it, and whether as it was.
    prev: Option<(ObjectId, bool)>,
    /// The marker of a list written just before the next block, which a
    /// list there mustn't share, or it would join it.
    after_list: Option<char>,
    /// A list's bullet or delimiter, whether it's numbered, the number its
    /// next item has, whether its numbers count up, and whether its items
    /// are apart.
    marker: char,
    ordered: bool,
    number: i64,
    counting: bool,
    loose: bool,
}

impl Open {
    fn new(id: ObjectId, frame: Frame, children: Vec<ObjectId>, original: bool) -> Open {
        Open {
            id,
            frame,
            children,
            next: 0,
            original,
            first: String::new(),
            rest: String::new(),
            pending: true,
            splices: Vec::new(),
            prev: None,
            after_list: None,
            marker: '-',
            ordered: false,
            number: 1,
            counting: true,
            loose: false,
        }
    }
}

/// The last writing's made blocks, and the document they were made from.
#[derive(Clone, Debug)]
pub(crate) struct Cache {
    pub document: Document,
    pub made: HashMap<ObjectId, Made>,
}

impl File {
    /// `document`'s story as this file: each block read from it and still
    /// as it was, where it was, written as it was, byte for byte; the rest
    /// written afresh in the file's habits, or else the standard style,
    /// and read back to check. With the file's front matter, line endings
    /// and byte order mark. An error if the story holds what Markdown
    /// can't, which nothing is written for.
    pub fn write(&mut self, document: &Document, sheet: &StyleSheet) -> Result<Vec<u8>, Unheld> {
        let cache = self.cache.take();
        let mut writer = Writer {
            file: self,
            document,
            sheet,
            changed: changed(document, &self.baseline),
            baseline: &self.baseline,
            out: String::new(),
            open: Vec::new(),
            definitions: std::cell::OnceCell::new(),
            last: cache.as_ref(),
            made: HashMap::new(),
        };
        let written = writer.run();
        let (out, made) = (
            std::mem::take(&mut writer.out),
            std::mem::take(&mut writer.made),
        );
        drop(writer);
        match written {
            Ok(()) => {
                self.cache = Some(Cache {
                    document: document.clone(),
                    made,
                });
                let mut bytes = Vec::with_capacity(out.len() + 3);
                if self.bom {
                    bytes.extend_from_slice(b"\xef\xbb\xbf");
                }
                bytes.extend_from_slice(out.as_bytes());
                Ok(bytes)
            }
            Err(e) => {
                self.cache = cache;
                Err(e)
            }
        }
    }

    /// Whether Markdown holds everything `document`'s story does: what
    /// [`File::write`] checks, without writing.
    pub fn holds(&mut self, document: &Document, sheet: &StyleSheet) -> Result<(), Unheld> {
        self.write(document, sheet).map(drop)
    }
}

/// The blocks of `document` that aren't as `baseline` has them, with
/// everything in them, and the containers they're in: found past every
/// object the two share, so it takes time in what was edited.
fn changed(document: &Document, baseline: &Document) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    for id in document.changed_since(baseline) {
        // Undone, an edit leaves an object as it was, if not the very one.
        if let (Some(a), Some(b)) = (document.object(id), baseline.object(id))
            && a == b
        {
            continue;
        }
        let mut at = Some(id);
        while let Some(x) = at {
            if !out.insert(x) {
                break;
            }
            at = document.parent(x).or_else(|| baseline.parent(x));
        }
    }
    out
}

struct Writer<'a> {
    file: &'a File,
    document: &'a Document,
    sheet: &'a StyleSheet,
    /// What's been edited since the file was read, and the containers
    /// it's in.
    changed: HashSet<ObjectId>,
    baseline: &'a Document,
    out: String,
    open: Vec<Open>,
    /// The story's link reference definitions, which text read back to
    /// check it is read with, as the file reads it.
    definitions: std::cell::OnceCell<Vec<(Option<String>, String)>>,
    last: Option<&'a Cache>,
    made: HashMap<ObjectId, Made>,
}

impl<'a> Writer<'a> {
    fn text(&self) -> &'a str {
        &self.file.text
    }

    /// Whether `id` is as the file had it, with everything in it.
    fn unchanged(&self, id: ObjectId) -> bool {
        !self.changed.contains(&id) && self.baseline.object(id).is_some()
    }

    fn habits(&self) -> &Habits {
        &self.file.habits
    }

    fn ending(&self) -> &'static str {
        self.file.ending
    }

    fn run(&mut self) -> Result<(), Unheld> {
        if let Some(front) = self.file.front.clone() {
            let text = self.text()[front].to_string();
            self.out.push_str(&text);
        }
        let root = self.document.root();
        let children = self.document.children(root).to_vec();
        self.open.push(Open::new(root, Frame::Root, children, true));
        while let Some(top) = self.open.last_mut() {
            if top.next >= top.children.len() {
                self.close()?;
                continue;
            }
            let child = top.children[top.next];
            top.next += 1;
            self.child(child)?;
        }
        if !self.file.final_newline {
            if self.out.ends_with("\r\n") {
                self.out.truncate(self.out.len() - 2);
            } else if self.out.ends_with(['\n', '\r']) {
                self.out.pop();
            }
        }
        // A thematic break first would be read as front matter's start,
        // if another follows: a blank line first.
        if self.file.front.is_none() && read::front_matter(&self.out).is_some() {
            self.out.insert_str(0, self.file.ending);
        }
        Ok(())
    }

    fn top(&mut self) -> &mut Open {
        self.open.last_mut().expect("a container is open")
    }

    /// The list items whose first line is still to come, innermost first.
    fn pending_items(&self) -> Vec<ObjectId> {
        self.open
            .iter()
            .rev()
            .take_while(|o| o.pending)
            .filter(|o| o.frame == Frame::Item)
            .map(|o| o.id)
            .collect()
    }

    /// Their changes to their first line.
    fn pending_splices(&self) -> Vec<(Range<usize>, String)> {
        self.open
            .iter()
            .filter(|o| o.pending && o.frame == Frame::Item)
            .flat_map(|o| o.splices.iter().cloned())
            .collect()
    }

    // ---- lines

    /// What goes before a line written afresh: each container's, its first
    /// line's if that's still to come.
    fn prefix(&mut self) -> String {
        let mut s = String::new();
        for o in &mut self.open {
            s.push_str(if o.pending { &o.first } else { &o.rest });
            o.pending = false;
        }
        s
    }

    /// A line written afresh, after its containers' marks; a blank line if
    /// it's empty.
    fn fresh(&mut self, content: &str) {
        let ending = self.ending();
        if content.is_empty() && !self.open.iter().any(|o| o.pending && !o.first.is_empty()) {
            // What every container puts before its lines, less the spaces
            // that would end it: a quote's `>` in a list item keeps the
            // item's indent.
            let blank: String = self.open.iter().map(|o| o.rest.as_str()).collect();
            self.out.push_str(blank.trim_end());
            self.out.push_str(ending);
            return;
        }
        let prefix = self.prefix();
        let line = format!("{prefix}{content}");
        self.out.push_str(if content.is_empty() {
            line.trim_end()
        } else {
            &line
        });
        self.out.push_str(ending);
    }

    /// The file's lines `range`, as they were but for `splices` to them,
    /// ending in a line ending.
    fn whole(&mut self, range: Range<usize>, splices: &[(Range<usize>, String)]) {
        let text = self.text();
        let mut splices: Vec<&(Range<usize>, String)> = splices
            .iter()
            .filter(|(r, _)| r.start >= range.start && r.end <= range.end)
            .collect();
        splices.sort_by_key(|(r, _)| r.start);
        let mut at = range.start;
        for (r, s) in splices {
            if r.start < at {
                continue;
            }
            self.out.push_str(&text[at..r.start]);
            self.out.push_str(s);
            at = r.end;
        }
        self.out.push_str(&text[at..range.end]);
        if !self.out.ends_with(['\n', '\r']) && !self.out.is_empty() {
            self.out.push_str(self.file.ending);
        }
        for o in &mut self.open {
            o.pending = false;
        }
    }

    /// `lines`, a block's, written.
    fn lines(&mut self, lines: &[Line]) {
        for (i, line) in lines.iter().enumerate() {
            match line {
                Line::Whole(range, _) => {
                    let splices = if i == 0 {
                        self.pending_splices()
                    } else {
                        Vec::new()
                    };
                    let end = line_end(self.text(), range.end);
                    self.whole(range.start..end, &splices);
                }
                Line::Fresh(content) => self.fresh(content),
            }
        }
    }

    // ---- blocks

    /// Writes `id`, a child of the innermost container.
    fn child(&mut self, id: ObjectId) -> Result<(), Unheld> {
        let document = self.document;
        let Some(object) = document.object(id) else {
            return Ok(());
        };
        // What Markdown writes as nothing; but what the file had, as it
        // had it.
        let whole = self.whole_plan(id);
        match object {
            Object::Paragraph(p) if whole.is_none() && vanishes(p, self.sheet) => {
                self.check_paragraph(id, p, false)?;
                return Ok(());
            }
            Object::Block(b)
                if matches!(b.kind, Kind::List | Kind::Table) && b.children.is_empty() =>
            {
                return Ok(());
            }
            _ => {}
        }
        let place = self.may_be_whole(id);
        self.separate(id, whole.is_some());
        let after_list = self.top().after_list.take();
        self.top().prev = Some((id, whole.is_some()));
        if let Some(splices) = whole {
            let lines = self.file.sources[&id].lines.clone();
            self.whole(lines, &splices);
            if let Object::Block(b) = object
                && b.kind == Kind::List
                && let Some(Syntax::List { marker, .. }) =
                    self.file.sources.get(&id).map(|s| &s.syntax)
            {
                self.top().after_list = Some(*marker);
            }
            if let Object::Block(b) = object
                && b.kind == Kind::Item
            {
                self.top().number += 1;
            }
            return Ok(());
        }
        match object {
            Object::Paragraph(p) => {
                let lines = self.paragraph(id, p, place)?;
                self.lines(&lines);
            }
            Object::Block(b) => match b.kind {
                Kind::List => self.open_list(id, after_list)?,
                Kind::Item => self.open_item(id)?,
                Kind::Quote => self.open_quote(id)?,
                Kind::Table => self.table(id)?,
                Kind::Row => {}
                Kind::Rule => self.rule(id)?,
            },
            Object::Story(_) | Object::Sheet(_) | Object::Style(_) => {}
        }
        Ok(())
    }

    /// The changes to the first line of `id`, if it's to be written as the
    /// file had it: as it was read, where it was, its containers written
    /// as they were.
    fn whole_plan(&self, id: ObjectId) -> Option<Vec<(Range<usize>, String)>> {
        let src = self.file.sources.get(&id)?;
        if !self.unchanged(id) || !self.in_place(src) {
            return None;
        }
        let top = self.open.last()?;
        if self.pending_items() != src.first_of {
            return None;
        }
        let mut splices = self.pending_splices();
        match &src.syntax {
            // An indented code block only after what it was after: after a
            // list it would be the list's.
            Syntax::Code(None) if top.prev.map(|p| p.0) != src.prev => return None,
            // A list after a list with its marker would join it.
            Syntax::List { marker, .. } if top.after_list == Some(*marker) => return None,
            Syntax::Item { marker, .. } => {
                // Its number, if its list counts and it's moved.
                let want = self.marker_text(top);
                let had = &self.text()[marker.clone()];
                if want != had {
                    if want.len() != had.len() {
                        return None;
                    }
                    splices.push((marker.clone(), want));
                }
            }
            _ => {}
        }
        Some(splices)
    }

    /// The separator before `id`, which is to be written as the file had
    /// it with `whole`: what the file had there, if it had it there, or a
    /// blank line where blocks need one.
    fn separate(&mut self, id: ObjectId, whole: bool) {
        let top = self.open.last().expect("open");
        let src = self.file.sources.get(&id);
        let Some((prev, prev_whole)) = top.prev else {
            // The first in its container.
            if top.frame == Frame::Root {
                let start = self.file.front.as_ref().map_or(0, |f| f.end);
                match src {
                    Some(src) if top.original && src.prev.is_none() && !src.lines.is_empty() => {
                        let gap = &self.text()[start..src.lines.start];
                        self.out.push_str(gap);
                    }
                    _ if self.file.front.is_some() => self.fresh(""),
                    _ => {}
                }
            }
            return;
        };
        if let (Some(src), Some(had)) = (src, self.file.sources.get(&prev))
            && top.original
            && src.prev == Some(prev)
            && src.parent == top.id
            && !had.lines.is_empty()
            && had.lines.end <= src.lines.start
        {
            let gap = &self.text()[had.lines.end..src.lines.start];
            if gap.contains(['\n', '\r']) || (prev_whole && whole) {
                self.out.push_str(gap);
                return;
            }
        }
        match top.frame {
            Frame::List if !top.loose => {}
            Frame::Item => {
                // In a tight list, a list right under its item's text.
                let list = &self.open[self.open.len() - 2];
                let interrupts = match self.document.block(id) {
                    Some(b) if b.kind == Kind::List => {
                        !matches!(b.props.get(keys::START), Some(Value::Int(n)) if *n != 1)
                    }
                    _ => false,
                };
                let after_text = self
                    .document
                    .paragraph(prev)
                    .is_some_and(|p| text_kind(p, self.sheet).is_empty());
                if list.loose || !interrupts || !after_text {
                    self.fresh("");
                }
            }
            _ => self.fresh(""),
        }
    }

    /// The marker the next item of list `list` has: its bullet, or its
    /// number and delimiter.
    fn marker_text(&self, list: &Open) -> String {
        if list.ordered {
            format!("{}{}", list.number, list.marker)
        } else {
            list.marker.to_string()
        }
    }

    /// Ends the innermost container.
    fn close(&mut self) -> Result<(), Unheld> {
        let open = self.open.last().expect("open");
        match open.frame {
            Frame::Root => {
                // What the file had after its last block, if that's last:
                // the last it had lines for, past a paragraph the reader
                // made to end a story of rules, which is written as
                // nothing.
                let base = &self.file.baseline;
                let had_last = base.children(base.root()).iter().rev().find(|c| {
                    self.file
                        .sources
                        .get(c)
                        .is_some_and(|s| !s.lines.is_empty())
                });
                if let Some((last, _)) = open.prev
                    && let Some(src) = self.file.sources.get(&last)
                    && open.original
                    && !src.lines.is_empty()
                    && had_last == Some(&last)
                {
                    let tail = &self.file.text[src.lines.end..];
                    if tail.trim().is_empty() {
                        let tail = tail.to_string();
                        self.out.push_str(&tail);
                    }
                } else if open.prev.is_none() && self.unchanged(open.id) {
                    // A file of blank lines, as it was.
                    let start = self.file.front.as_ref().map_or(0, |f| f.end);
                    let rest = &self.file.text[start..];
                    if rest.trim().is_empty() {
                        let rest = rest.to_string();
                        self.out.push_str(&rest);
                    }
                }
            }
            Frame::Item | Frame::Quote if open.pending => {
                // Nothing in it: its marker alone.
                self.fresh("");
            }
            _ => {}
        }
        let closed = self.open.pop().expect("open");
        if let Some(parent) = self.open.last_mut() {
            match closed.frame {
                Frame::List => parent.after_list = Some(closed.marker),
                Frame::Item => parent.number += 1,
                _ => {}
            }
        }
        Ok(())
    }

    fn open_list(&mut self, id: ObjectId, after_list: Option<char>) -> Result<(), Unheld> {
        let b = self.document.block(id).expect("a list");
        check_props(id, &b.props, &[keys::ORDERED, keys::START])?;
        let ordered = matches!(b.props.get(keys::ORDERED), Some(Value::Bool(true)));
        let start = match b.props.get(keys::START) {
            Some(Value::Int(n)) if ordered => (*n).clamp(0, 999_999_999),
            _ => 1,
        };
        let habits = self.habits().clone();
        let parent = self.open.last().expect("open");
        let src = self.file.sources.get(&id).filter(|s| s.parent == parent.id);
        let had = src.and_then(|s| match s.syntax {
            Syntax::List {
                marker,
                loose,
                counting,
            } => Some((marker, loose, counting)),
            _ => None,
        });
        let (mut marker, loose, counting) = had.unwrap_or((
            if ordered {
                habits.delimiter
            } else {
                habits.bullet
            },
            false,
            habits.counting,
        ));
        if ordered != matches!(marker, '.' | ')') {
            marker = if ordered {
                habits.delimiter
            } else {
                habits.bullet
            };
        }
        if after_list == Some(marker) {
            marker = match marker {
                '-' => '*',
                '*' => '+',
                '+' => '-',
                '.' => ')',
                _ => '.',
            };
        }
        // Lists begun on one line, with three of one bullet and nothing
        // after them, would read as a thematic break: one that's changed
        // takes `+`, which never makes one, after two of its own.
        if !ordered && marker != '+' && !self.unchanged(id) {
            let line: String = self
                .open
                .iter()
                .map(|o| if o.pending { &o.first } else { &o.rest }.as_str())
                .collect();
            if line.matches(marker).count() >= 2 {
                marker = '+';
            }
        }
        let parent = self.open.last().expect("open");
        let original = parent.original && had.is_some_and(|h| h.0 == marker);
        let children = b.children.clone();
        let mut open = Open::new(id, Frame::List, children, original);
        open.marker = marker;
        open.ordered = ordered;
        open.number = start;
        open.counting = counting;
        open.loose = loose;
        self.open.push(open);
        Ok(())
    }

    fn open_item(&mut self, id: ObjectId) -> Result<(), Unheld> {
        let document = self.document;
        let b = document.block(id).expect("an item");
        check_props(id, &b.props, &[keys::TASK])?;
        let task = match b.props.get(keys::TASK) {
            Some(Value::Bool(done)) => Some(*done),
            _ => None,
        };
        if task.is_some() {
            // A task's box goes before text.
            let first = b.children.iter().find(|&&c| match document.object(c) {
                Some(Object::Paragraph(p)) => !vanishes(p, self.sheet),
                _ => true,
            });
            if let Some(&first) = first
                && !document
                    .paragraph(first)
                    .is_some_and(|p| text_kind(p, self.sheet).is_empty())
            {
                return Err(Unheld {
                    block: id,
                    what: What::TaskStart,
                });
            }
        }
        let list = self.open.last().expect("its list");
        let marker = self.marker_text(list);
        let src = self.file.sources.get(&id).filter(|s| s.parent == list.id);
        let text = self.text();
        let (indent, spaces, splices, original) = match src.map(|s| &s.syntax) {
            Some(Syntax::Item {
                marker: had,
                indent: Some(indent),
                spaces,
                task: had_task,
            }) if list.original && text[had.clone()].len() == marker.len() => {
                let mut splices = Vec::new();
                if text[had.clone()] != marker {
                    splices.push((had.clone(), marker.clone()));
                }
                let content = had.end + spaces;
                let line = without_ending(text, line_end(text, had.end));
                match (had_task, task) {
                    (Some(range), Some(done)) => {
                        let was = text[range.clone()].eq_ignore_ascii_case("[x]");
                        if was != done {
                            splices.push((range.clone(), box_of(done).to_string()));
                        }
                    }
                    (None, Some(done)) if content <= line => {
                        splices.push((content..content, format!("{} ", box_of(done))));
                    }
                    (Some(range), None) => {
                        let end = text[range.end..line]
                            .chars()
                            .take_while(|&c| c == ' ')
                            .count()
                            + range.end;
                        splices.push((range.start..end, String::new()));
                    }
                    _ => {}
                }
                (*indent, *spaces, splices, true)
            }
            _ => (0, 1, Vec::new(), false),
        };
        let mut first = format!("{}{marker}{}", " ".repeat(indent), " ".repeat(spaces));
        if let Some(done) = task {
            first.push_str(box_of(done));
            first.push(' ');
        }
        let rest = " ".repeat(indent + marker.len() + spaces);
        let children = b.children.clone();
        let mut open = Open::new(id, Frame::Item, children, original);
        open.first = first;
        open.rest = rest;
        open.splices = splices;
        self.open.push(open);
        Ok(())
    }

    fn open_quote(&mut self, id: ObjectId) -> Result<(), Unheld> {
        let b = self.document.block(id).expect("a quote");
        check_props(id, &b.props, &[])?;
        let parent = self.open.last().expect("open");
        let src = self.file.sources.get(&id).filter(|s| s.parent == parent.id);
        let had = match src.map(|s| &s.syntax) {
            Some(Syntax::Quote(prefix)) => prefix.clone(),
            _ => None,
        };
        let original = parent.original && had.is_some();
        let prefix = had.unwrap_or_else(|| "> ".to_string());
        let children = b.children.clone();
        let mut open = Open::new(id, Frame::Quote, children, original);
        open.first = prefix.clone();
        open.rest = prefix;
        self.open.push(open);
        Ok(())
    }

    /// Rule `id`, a thematic break written afresh: `---`, after the blank
    /// line every block written afresh has before it, so it's never read
    /// as a heading's underline; or, on the first line of a list item
    /// whose marker or another's on that line is a `-`, of a character
    /// none of them is, as the line would otherwise be a thematic break
    /// itself, not an item holding one.
    fn rule(&mut self, id: ObjectId) -> Result<(), Unheld> {
        let b = self.document.block(id).expect("a rule");
        check_props(id, &b.props, &[])?;
        let markers: String = self
            .open
            .iter()
            .filter(|o| o.pending && o.frame == Frame::Item)
            .map(|o| o.first.as_str())
            .collect();
        let c = ['-', '*', '_']
            .into_iter()
            .find(|c| !markers.contains(*c))
            .unwrap_or('_');
        self.fresh(&c.to_string().repeat(3));
        Ok(())
    }

    // ---- paragraphs

    /// Whether `id`'s lines may be the file's: the rest of them, and its
    /// first.
    fn may_be_whole(&self, id: ObjectId) -> (bool, bool) {
        let Some(src) = self.file.sources.get(&id) else {
            return (false, false);
        };
        let rest = self.in_place(src);
        (rest && self.pending_items() == src.first_of, rest)
    }

    /// Whether a block read as `src` is where the file had it, so its
    /// lines can be the file's: in the container it was in, written as it
    /// was, and not after a list written afresh where it starts indented,
    /// which might make it the list's.
    fn in_place(&self, src: &Source) -> bool {
        let Some(top) = self.open.last() else {
            return false;
        };
        let after_fresh_list = matches!(top.prev, Some((prev, false))
            if self.document.block(prev).is_some_and(|b| b.kind == Kind::List));
        !src.lines.is_empty()
            && top.original
            && src.parent == top.id
            && !(after_fresh_list && self.text()[src.lines.start..].starts_with([' ', '\t']))
    }

    /// Paragraph `id`'s lines.
    fn paragraph(
        &mut self,
        id: ObjectId,
        p: &Paragraph,
        (first, rest): (bool, bool),
    ) -> Result<Vec<Line>, Unheld> {
        self.check_paragraph(id, p, false)?;
        if let Some(last) = self.last
            && let (Some(a), Some(b)) = (last.document.object(id), self.document.object(id))
            && std::ptr::eq(a, b)
            && let Some(made) = last.made.get(&id)
            && made.first == first
            && made.rest == rest
        {
            let lines = made.lines.clone();
            self.made.insert(id, made.clone());
            return Ok(lines);
        }
        let kind = text_kind(p, self.sheet);
        let lines = match kind.as_str() {
            "code" => code_lines(self.file, id, p),
            "source" => p
                .text
                .split('\u{2028}')
                .map(|l| Line::Fresh(l.to_string()))
                .collect(),
            "" => self.text_lines(id, p, first, rest)?,
            heading => {
                let level = heading[1..].parse().unwrap_or(1);
                self.heading_lines(id, p, level)?
            }
        };
        self.made.insert(
            id,
            Made {
                first,
                rest,
                lines: lines.clone(),
            },
        );
        Ok(lines)
    }

    /// What Markdown can't hold of paragraph `id`, if anything, `cell` if
    /// it's a table's.
    fn check_paragraph(&self, id: ObjectId, p: &Paragraph, cell: bool) -> Result<(), Unheld> {
        let unheld = |what| Err(Unheld { block: id, what });
        let style = self.sheet.style_of(p);
        // Markdown holds the program's sheet's styles that it has: body
        // text, headings and code.
        let markdown = |role: &str| {
            role == "body" || role == "code" || role.strip_prefix("heading-").is_some()
        };
        if style.id.is_some() && !style.role.as_deref().is_some_and(markdown) {
            return unheld(What::Style(style.name.clone()));
        }
        if cell && (style.code || style.heading.is_some()) {
            return unheld(What::BlockInCell);
        }
        let language = p.props.get(keys::LANGUAGE);
        for (key, value) in &p.props {
            let what = match key.as_str() {
                keys::STYLE | keys::INFO | keys::SOURCE => continue,
                keys::LANGUAGE if self.file_language(value) => continue,
                keys::ALIGN if cell => continue,
                keys::LANGUAGE => What::Language,
                keys::ALIGN => What::Alignment,
                keys::SPACE_BEFORE | keys::SPACE_AFTER => What::Spacing,
                keys::LINE_HEIGHT => What::LineHeight,
                keys::INDENT_START | keys::INDENT_END | keys::INDENT_FIRST => What::Indents,
                keys::TABS => What::Tabs,
                _ => What::Property(key.clone()),
            };
            return unheld(what);
        }
        let plain = style.code;
        for run in &p.runs {
            for (key, value) in run.props.iter() {
                if matches!(value, Value::Bool(false)) {
                    continue;
                }
                let what = match key.as_str() {
                    keys::LANGUAGE if Some(value) == language || self.file_language(value) => {
                        continue;
                    }
                    keys::LANGUAGE => What::Language,
                    keys::FONT => What::Font,
                    keys::SIZE => What::Size,
                    _ if plain => What::Property(key.clone()),
                    keys::BOLD
                    | keys::ITALIC
                    | keys::STRIKETHROUGH
                    | keys::CODE
                    | keys::LINK
                    | keys::LINK_TITLE
                    | keys::SOURCE
                    | keys::PICTURE_ALT
                    | keys::PICTURE_WIDTH
                    | keys::PICTURE_HEIGHT
                    | keys::PICTURE_SRC
                    | keys::PICTURE_TITLE => continue,
                    keys::PICTURE if run.props.contains_key(keys::PICTURE_SRC) => continue,
                    keys::PICTURE => What::KeptPicture,
                    _ => What::Property(key.clone()),
                };
                return unheld(what);
            }
        }
        Ok(())
    }

    /// Whether `value` is the file's language.
    fn file_language(&self, value: &Value) -> bool {
        match (value, &self.file.language) {
            (Value::Text(tag), Some(language)) => tag.eq_ignore_ascii_case(language),
            _ => false,
        }
    }

    /// The inline style the file writes paragraph `id` with.
    fn style(&self, id: ObjectId, careful: bool, cell: bool) -> Style<'_> {
        let own = self.file.inline.get(&id);
        Style {
            emphasis: own
                .and_then(|o| o.emphasis)
                .unwrap_or(self.habits().emphasis),
            strong: own.and_then(|o| o.strong).unwrap_or(self.habits().strong),
            careful,
            cell,
            literal_breaks: cell,
            spaced_breaks: own.is_some_and(|o| o.spaced_breaks) && !careful,
            references: own.map_or(&[], |o| o.references.as_slice()),
            angled: own.map_or(&[], |o| o.angled.as_slice()),
        }
    }

    /// How wide a paragraph's lines may be, after its containers' marks.
    fn width(&self) -> Option<usize> {
        let prefix: usize = self.open.iter().map(|o| o.rest.chars().count()).sum();
        self.habits()
            .width
            .map(|w| w.saturating_sub(prefix).max(20))
    }

    /// A body paragraph's lines: those it had that its edit left as they
    /// were, if it was read and they may be the file's, and the rest
    /// afresh; or all afresh, broken where it was and at the file's width.
    fn text_lines(
        &mut self,
        id: ObjectId,
        p: &Paragraph,
        first: bool,
        rest: bool,
    ) -> Result<Vec<Line>, Unheld> {
        let (text, props) = canonical(p);
        let chars: Vec<(char, Props)> = text.chars().zip(props).collect();
        let expected = shape_of(self.document, self.sheet, id);
        // A quote's first paragraph that starts with a GitHub alert's
        // `[!NOTE]` reads back as one only in its quote.
        let alert = self.alert_len(id, &chars);
        let within = if alert.is_some() {
            Within::Quote
        } else {
            Within::Story
        };
        let had = match self.file.sources.get(&id).map(|s| &s.syntax) {
            Some(Syntax::Paragraph(lines)) if !lines.is_empty() => Some(lines.clone()),
            _ => None,
        };
        let base = self.file.baseline.paragraph(id).map(|b| {
            let (text, props) = canonical(b);
            (b.text == text, text.chars().zip(props).collect::<Vec<_>>())
        });
        if rest
            && let (Some(lines), Some((true, base))) = (&had, &base)
            && let Some(out) = self.reuse(id, &chars, base, lines, first)
            && self.reads_back(&self.content(&out), &expected, within)
        {
            return Ok(out);
        }
        // Afresh: broken where the file broke it, as far as the edit left
        // those places, and where it's too wide.
        let mut kept = BTreeSet::new();
        if let (Some(lines), Some((_, base))) = (&had, &base) {
            let (n0, n1) = (base.len(), chars.len());
            let p = common_prefix(base, &chars);
            let s = common_suffix(base, &chars, p);
            for line in lines.iter().filter(|l| l.soft) {
                let b = line.chars.end as usize;
                let at = if b < p {
                    Some(b)
                } else if b >= n0 - s {
                    Some(b + n1 - n0)
                } else {
                    None
                };
                if let Some(at) = at.filter(|&at| breakable(&chars, at)) {
                    kept.insert(at);
                }
            }
        }
        // An alert's `[!NOTE]` stays alone on its line, as GitHub reads it.
        let alert = alert.filter(|&n| chars.get(n).is_some_and(|(c, p)| *c == ' ' && p.is_empty()));
        kept.extend(alert);
        let width = self.width();
        for (careful, breaks, width) in [
            (false, kept.clone(), width),
            (true, kept, width),
            (true, alert.into_iter().collect(), None),
        ] {
            let written = wrap(
                &chars,
                0,
                chars.len(),
                breaks,
                self.style(id, careful, false),
                width,
            );
            let out: Vec<Line> = written
                .iter()
                .map(|w| Line::Fresh(inline::guard_start(w)))
                .collect();
            if self.reads_back(&self.content(&out), &expected, within) {
                return Ok(out);
            }
        }
        Err(Unheld {
            block: id,
            what: What::Marks,
        })
    }

    /// How many of the characters `chars` of paragraph `id` starts with
    /// are a GitHub alert's `[!NOTE]` kept as written, if it starts with
    /// one as its quote's first block.
    fn alert_len(&self, id: ObjectId, chars: &[(char, Props)]) -> Option<usize> {
        let quote = self.document.parent(id)?;
        if self.document.block(quote)?.kind != Kind::Quote
            || self.document.children(quote).first() != Some(&id)
        {
            return None;
        }
        let is_alert =
            |p: &Props| matches!(p.get(keys::SOURCE), Some(Value::Text(k)) if k == "alert");
        let n = chars.iter().take_while(|(_, p)| is_alert(p)).count();
        (n > 0).then_some(n)
    }

    /// A paragraph's lines, of the file's `lines` it had, as `base`, those
    /// before and after what's changed in it as they were, and those
    /// between afresh: if its marks don't reach across from what's changed
    /// into what isn't.
    fn reuse(
        &self,
        id: ObjectId,
        chars: &[(char, Props)],
        base: &[(char, Props)],
        lines: &[TextLine],
        first: bool,
    ) -> Option<Vec<Line>> {
        let (n0, n1) = (base.len(), chars.len());
        let k = lines.len();
        let p = common_prefix(base, chars);
        let s = common_suffix(base, chars, p);
        let sep = |i: usize| usize::from(lines[i].soft);
        let mut i0 = 0;
        while i0 < k
            && (i0 > 0 || first)
            && lines[i0].chars.end as usize + sep(i0) <= p
            && (i0 + 1 < k || n0 == n1 && p == n0)
        {
            i0 += 1;
        }
        if i0 == k {
            return Some(lines.iter().map(whole_line).collect());
        }
        let mut j0 = k;
        while j0 > i0.max(1) {
            let j = j0 - 1;
            let start = lines[j].chars.start as usize;
            if start - sep(j - 1) >= n0 - s {
                j0 = j;
            } else {
                break;
            }
        }
        let m0 = if i0 == 0 {
            0
        } else {
            lines[i0 - 1].chars.end as usize + sep(i0 - 1)
        };
        let m1 = if j0 == k {
            n1
        } else {
            (lines[j0].chars.start as usize - sep(j0 - 1) + n1).checked_sub(n0)?
        };
        if m0 >= m1 || m1 > n1 {
            return None;
        }
        let crosses = |at: usize| at > 0 && at < n1 && shares_marks(&chars[at - 1].1, &chars[at].1);
        if crosses(m0) || crosses(m1) {
            return None;
        }
        let mut written = wrap(
            chars,
            m0,
            m1,
            BTreeSet::new(),
            self.style(id, false, false),
            self.width(),
        );
        if written.len() > 1 && written.last().is_some_and(|w| w.text.is_empty()) {
            written.pop();
        }
        let mut out: Vec<Line> = lines[..i0].iter().map(whole_line).collect();
        out.extend(written.iter().map(|w| Line::Fresh(inline::guard_start(w))));
        out.extend(lines[j0..].iter().map(whole_line));
        Some(out)
    }

    /// `lines` as one block's Markdown, without what its containers put
    /// before them, to read back.
    fn content(&self, lines: &[Line]) -> String {
        let text = self.text();
        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            match line {
                Line::Whole(range, content) => {
                    // The text after its containers' marks.
                    out.push_str(&text[(*content).min(range.end)..range.end]);
                }
                Line::Fresh(s) => out.push_str(s),
            }
        }
        out
    }

    /// Whether `markdown`, read back as the file would read it, is one
    /// block of `expected`'s shape: on its own, as a table of one column
    /// whose header cell it is, or as a quote's first block.
    fn reads_back(&self, markdown: &str, expected: &[Node], within: Within) -> bool {
        let cell = within == Within::Cell;
        let mut text = match within {
            Within::Story => markdown.to_string(),
            Within::Cell => format!("| {markdown} |\n| --- |\n"),
            Within::Quote => markdown.split('\n').map(|l| format!("> {l}\n")).collect(),
        };
        // The definitions it might name: all of them, or of many, those
        // whose labels it has, case folded.
        let definitions = self.definitions();
        let folded = fold(&text);
        let named: Vec<&str> = definitions
            .iter()
            .filter(|(label, _)| {
                definitions.len() <= MANY_DEFINITIONS
                    || label
                        .as_ref()
                        .is_none_or(|l| folded.contains(&format!("[{l}")))
            })
            .map(|(_, d)| d.as_str())
            .collect();
        if !named.is_empty() {
            text.push_str("\n\n");
            text.push_str(&named.join("\n"));
        }
        let sheet = story::sheet_objects(self.document);
        let settings = Settings {
            sheet: &sheet,
            language: None,
            sizes: &|_| None,
        };
        let Ok(read) = read::read(text.as_bytes(), &settings) else {
            return false;
        };
        let document = &read.document;
        let Some(&first) = document.children(document.root()).first() else {
            return false;
        };
        let first = match within {
            Within::Story => first,
            Within::Cell => {
                let Some(&cell) = document
                    .children(first)
                    .first()
                    .and_then(|&row| document.children(row).first())
                else {
                    return false;
                };
                cell
            }
            Within::Quote => {
                let Some(&inner) = document.children(first).first() else {
                    return false;
                };
                inner
            }
        };
        let mut got = shape_of(document, self.sheet, first);
        let mut expected = expected.to_vec();
        if cell {
            // A cell's alignment is its column's, which the row says.
            for node in got.iter_mut().chain(expected.iter_mut()) {
                if let Form::Text { kind, detail, .. } = &mut node.form {
                    kind.clear();
                    detail.clear();
                }
            }
        }
        got == expected
    }

    /// The story's link reference definitions and footnotes, as kept,
    /// each with its label, folded, where that can be told.
    fn definitions(&self) -> &[(Option<String>, String)] {
        self.definitions.get_or_init(|| {
            // Those the file had and still has as it had them, and those
            // edited since; found in what's changed, not in the whole
            // story.
            let mut texts: Vec<&str> = self
                .file
                .definitions
                .iter()
                .filter(|id| !self.changed.contains(id))
                .filter_map(|&id| self.document.paragraph(id).map(|p| p.text.as_str()))
                .collect();
            let mut edited: Vec<ObjectId> = self.changed.iter().copied().collect();
            edited.sort_unstable();
            for id in edited {
                if let Some(p) = self.document.paragraph(id)
                    && is_definition(p)
                {
                    texts.push(&p.text);
                }
            }
            let mut out = Vec::new();
            for text in texts {
                for definition in text.split('\u{2028}') {
                    let label = definition
                        .trim_start()
                        .strip_prefix('[')
                        .and_then(|rest| rest.split_once("]:"))
                        .map(|(label, _)| fold(label));
                    out.push((label, definition.to_string()));
                }
            }
            out
        })
    }

    /// A heading's lines: `#`s and its text, or its text underlined where
    /// it was, or where it has line breaks and is of the first two levels.
    fn heading_lines(
        &mut self,
        id: ObjectId,
        p: &Paragraph,
        level: usize,
    ) -> Result<Vec<Line>, Unheld> {
        let (text, props) = canonical(p);
        let chars: Vec<(char, Props)> = text.chars().zip(props).collect();
        let had = match self.file.sources.get(&id).map(|s| &s.syntax) {
            Some(Syntax::Heading(Some(c))) => Some(*c),
            _ => None,
        };
        let underline = if level == 1 { '=' } else { '-' };
        // Underlined as it was; its line breaks the characters they are.
        let setext = level <= 2 && !chars.is_empty() && had == Some(underline);
        let expected = shape_of(self.document, self.sheet, id);
        for careful in [false, true] {
            let style = Style {
                literal_breaks: true,
                ..self.style(id, careful, false)
            };
            let written = inline::write(&chars, 0, chars.len(), &BTreeSet::new(), style);
            let lines: Vec<Line> = if setext {
                let mut lines: Vec<Line> = written
                    .iter()
                    .map(|w| Line::Fresh(inline::guard_start(w)))
                    .collect();
                let width = written.last().map_or(3, |w| w.text.chars().count()).max(3);
                lines.push(Line::Fresh(underline.to_string().repeat(width)));
                lines
            } else {
                let hashes = "#".repeat(level);
                match written.first().map(|w| w.text.as_str()) {
                    None | Some("") => vec![Line::Fresh(hashes)],
                    Some(content) => {
                        vec![Line::Fresh(format!("{hashes} {}", guard_closing(content)))]
                    }
                }
            };
            if self.reads_back(&self.content(&lines), &expected, Within::Story) {
                return Ok(lines);
            }
        }
        Err(Unheld {
            block: id,
            what: What::Marks,
        })
    }

    // ---- tables

    /// Table `id`: its header, delimiter row and rows, each row as the file
    /// had it if it's unchanged and the table's columns are as they were.
    fn table(&mut self, id: ObjectId) -> Result<(), Unheld> {
        let document = self.document;
        let b = document.block(id).expect("a table");
        check_props(id, &b.props, &[])?;
        let rows = b.children.clone();
        let columns = document.children(rows[0]).len();
        if columns == 0 {
            return Err(Unheld {
                block: id,
                what: What::Columns,
            });
        }
        let align_of = |cell: ObjectId| match document
            .paragraph(cell)
            .and_then(|p| p.props.get(keys::ALIGN))
        {
            Some(Value::Text(a)) => a.trim().to_string(),
            _ => String::new(),
        };
        let aligns: Vec<String> = document
            .children(rows[0])
            .iter()
            .map(|&c| align_of(c))
            .collect();
        for &row in &rows {
            let r = document.block(row).expect("a row");
            check_props(row, &r.props, &[])?;
            if r.children.len() > columns {
                return Err(Unheld {
                    block: row,
                    what: What::Columns,
                });
            }
            for (column, &cell) in r.children.iter().enumerate() {
                let p = document.paragraph(cell).expect("a cell");
                self.check_paragraph(cell, p, true)?;
                let align = align_of(cell);
                if align != aligns[column]
                    || !["", "start", "center", "end"].contains(&align.as_str())
                {
                    return Err(Unheld {
                        block: cell,
                        what: What::Alignment,
                    });
                }
            }
        }
        let top = self.open.last().expect("open");
        let src = self.file.sources.get(&id).filter(|s| s.parent == top.id);
        let original = top.original && src.is_some();
        let base = &self.file.baseline;
        let same_columns = original
            && base.block(id).is_some_and(|t| {
                t.children.first().is_some_and(|&h| {
                    let had: Vec<String> = base
                        .children(h)
                        .iter()
                        .map(
                            |&c| match base.paragraph(c).and_then(|p| p.props.get(keys::ALIGN)) {
                                Some(Value::Text(a)) => a.trim().to_string(),
                                _ => String::new(),
                            },
                        )
                        .collect();
                    had == aligns
                })
            });
        let (delimiter, had_widths) = match src.map(|s| &s.syntax) {
            Some(Syntax::Table { delimiter, widths }) => (Some(delimiter.clone()), widths.clone()),
            _ => (None, None),
        };
        // Each row's cells, written, where it isn't as the file had it.
        let mut written: Vec<Option<Vec<String>>> = Vec::new();
        for (i, &row) in rows.iter().enumerate() {
            let whole = same_columns && self.row_whole(row, i == 0);
            if whole {
                written.push(None);
                continue;
            }
            let mut cells = Vec::new();
            for &cell in document.children(row) {
                cells.push(self.cell(cell)?);
            }
            while cells.len() < columns {
                cells.push(String::new());
            }
            written.push(Some(cells));
        }
        let padded = if original {
            had_widths.is_some()
        } else {
            self.habits().padded
        };
        let widths: Vec<usize> = match (&had_widths, same_columns) {
            (Some(w), true) if w.len() == columns => w.clone(),
            _ if padded => (0..columns)
                .map(|c| {
                    written
                        .iter()
                        .flatten()
                        .map(|cells| cells[c].chars().count())
                        .max()
                        .unwrap_or(0)
                        .max(3)
                })
                .collect(),
            _ => vec![0; columns],
        };
        // Each cell padded to its column's width on the side away from its
        // alignment.
        let row_line = |cells: &[String]| {
            let parts: Vec<String> = cells
                .iter()
                .zip(widths.iter().zip(&aligns))
                .map(|(c, (&w, align))| {
                    let pad = w.saturating_sub(c.chars().count());
                    match align.as_str() {
                        "end" => format!("{}{c}", " ".repeat(pad)),
                        "center" => {
                            format!("{}{c}{}", " ".repeat(pad / 2), " ".repeat(pad - pad / 2))
                        }
                        _ => format!("{c}{}", " ".repeat(pad)),
                    }
                })
                .collect();
            format!("| {} |", parts.join(" | "))
        };
        for (i, &row) in rows.iter().enumerate() {
            match &written[i] {
                None => {
                    let src = &self.file.sources[&row];
                    let splices = self.pending_splices();
                    self.whole(src.lines.clone(), &splices);
                }
                Some(cells) => {
                    let line = row_line(cells);
                    self.fresh(&line);
                }
            }
            if i == 0 {
                match &delimiter {
                    Some(range) if same_columns => self.whole(range.clone(), &[]),
                    _ => {
                        let cells: Vec<String> = aligns
                            .iter()
                            .zip(&widths)
                            .map(|(a, &w)| {
                                let w = w.max(3);
                                match a.as_str() {
                                    "start" => format!(":{}", "-".repeat(w - 1)),
                                    "center" => format!(":{}:", "-".repeat(w - 2)),
                                    "end" => format!("{}:", "-".repeat(w - 1)),
                                    _ => "-".repeat(w),
                                }
                            })
                            .collect();
                        let line = format!("| {} |", cells.join(" | "));
                        self.fresh(&line);
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether table row `row` can be written as the file had it.
    fn row_whole(&self, row: ObjectId, header: bool) -> bool {
        let Some(src) = self.file.sources.get(&row) else {
            return false;
        };
        let table = self.document.parent(row);
        !src.lines.is_empty()
            && self.unchanged(row)
            && Some(src.parent) == table
            && (!header || self.pending_items() == src.first_of)
            && (header || self.file.baseline.children(src.parent).first() != Some(&row))
    }

    /// A table's cell's text, written, read back to check.
    fn cell(&mut self, id: ObjectId) -> Result<String, Unheld> {
        let p = self.document.paragraph(id).expect("a cell");
        let (text, props) = canonical(p);
        let chars: Vec<(char, Props)> = text.chars().zip(props).collect();
        let expected = vec![Node {
            depth: 0,
            form: match crate::shape::shape_of(self.document, self.sheet, id).pop() {
                Some(node) => node.form,
                None => return Ok(String::new()),
            },
        }];
        for careful in [false, true] {
            let written = inline::write(
                &chars,
                0,
                chars.len(),
                &BTreeSet::new(),
                self.style(id, careful, true),
            );
            let line = written.first().map(|w| w.text.clone()).unwrap_or_default();
            if self.reads_back(&line, &expected, Within::Cell) {
                return Ok(line);
            }
        }
        Err(Unheld {
            block: id,
            what: What::Marks,
        })
    }
}

/// Where a block written afresh is read back, to check it: on its own, as
/// a table's cell, or as a quote's first block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Within {
    Story,
    Cell,
    Quote,
}

/// A task's box: `[x]` when it's done, `[ ]` when not.
fn box_of(done: bool) -> &'static str {
    if done { "[x]" } else { "[ ]" }
}

/// What Markdown can't hold of a container's properties: any but `held`.
fn check_props(id: ObjectId, props: &Props, held: &[&str]) -> Result<(), Unheld> {
    match props.keys().find(|k| !held.contains(&k.as_str())) {
        Some(key) => Err(Unheld {
            block: id,
            what: What::Property(key.clone()),
        }),
        None => Ok(()),
    }
}

/// A code block's lines: fenced, in the fence it had or the file's habit,
/// long enough that no line of its ends it; or indented, as it was, if it
/// still can be.
fn code_lines(file: &File, id: ObjectId, p: &Paragraph) -> Vec<Line> {
    let lines: Vec<&str> = if p.text.is_empty() {
        Vec::new()
    } else {
        p.text.split('\u{2028}').collect()
    };
    let info = match p.props.get(keys::INFO) {
        Some(Value::Text(info)) => info.trim().to_string(),
        _ => String::new(),
    };
    let had = match file.sources.get(&id).map(|s| &s.syntax) {
        Some(Syntax::Code(fence)) => Some(*fence),
        _ => None,
    };
    let blank = |l: &&str| l.trim_matches([' ', '\t']).is_empty();
    if had == Some(None)
        && info.is_empty()
        && !lines.is_empty()
        && !lines.first().is_some_and(blank)
        && !lines.last().is_some_and(blank)
    {
        return lines
            .iter()
            .map(|l| {
                if l.is_empty() {
                    Line::Fresh(String::new())
                } else {
                    Line::Fresh(format!("    {l}"))
                }
            })
            .collect();
    }
    let (mut c, mut n) = had.flatten().unwrap_or((file.habits.fence, 3));
    if c == '`' && info.contains('`') {
        c = '~';
    }
    for line in &lines {
        let rest = line.trim_start_matches(' ');
        if line.len() - rest.len() <= 3 {
            n = n.max(rest.chars().take_while(|&x| x == c).count() + 1);
        }
    }
    let fence = c.to_string().repeat(n.max(3));
    let mut out = vec![Line::Fresh(format!("{fence}{info}"))];
    out.extend(lines.iter().map(|l| Line::Fresh(l.to_string())));
    out.push(Line::Fresh(fence));
    out
}

/// A heading's text with a `#` run at its end escaped, which would be
/// taken for its closing sequence.
fn guard_closing(content: &str) -> String {
    let trimmed = content.trim_end_matches('#');
    if trimmed.len() < content.len() && (trimmed.is_empty() || trimmed.ends_with([' ', '\t'])) {
        format!("{trimmed}\\{}", &content[trimmed.len()..])
    } else {
        content.to_string()
    }
}

/// How many characters `a` and `b` share at their start, marks and all.
fn common_prefix(a: &[(char, Props)], b: &[(char, Props)]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// How many they share at their end, not reaching into the first `p`.
fn common_suffix(a: &[(char, Props)], b: &[(char, Props)], p: usize) -> usize {
    let most = a.len().min(b.len()) - p;
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take(most)
        .take_while(|(x, y)| x == y)
        .count()
}

/// Whether two neighbouring characters share a mark that reaches across
/// them.
fn shares_marks(a: &Props, b: &Props) -> bool {
    [
        keys::BOLD,
        keys::ITALIC,
        keys::STRIKETHROUGH,
        keys::CODE,
        keys::LINK,
        keys::SOURCE,
    ]
    .iter()
    .any(|k| a.get(*k).is_some() && a.get(*k) == b.get(*k))
}

/// Whether a line may break at character `at`: a space between two
/// characters that aren't, outside code and kept Markdown.
fn breakable(chars: &[(char, Props)], at: usize) -> bool {
    let plain = |p: &Props| !p.contains_key(keys::CODE) && !p.contains_key(keys::SOURCE);
    let text = |i: usize| {
        chars
            .get(i)
            .is_some_and(|(c, p)| !c.is_whitespace() && plain(p))
    };
    at > 0
        && chars.get(at).is_some_and(|(c, p)| *c == ' ' && plain(p))
        && text(at - 1)
        && text(at + 1)
}

/// `chars[from..to]` written as lines, broken at `breaks`, and where a
/// line is wider than `width` at the last space it can break at that
/// fits, or the first after.
fn wrap(
    chars: &[(char, Props)],
    from: usize,
    to: usize,
    mut breaks: BTreeSet<usize>,
    style: Style<'_>,
    width: Option<usize>,
) -> Vec<Written> {
    loop {
        let lines = inline::write(chars, from, to, &breaks, style);
        let Some(width) = width else {
            return lines;
        };
        let mut added = false;
        for line in &lines {
            if line.text.chars().count() <= width {
                continue;
            }
            let fits: Vec<&(usize, usize)> = line
                .spaces
                .iter()
                .filter(|(_, at)| breakable(chars, *at))
                .collect();
            let best = fits
                .iter()
                .rev()
                .find(|(offset, _)| line.text[..*offset].chars().count() <= width)
                .or(fits.first());
            if let Some(&&(_, at)) = best
                && breaks.insert(at)
            {
                added = true;
            }
        }
        if !added {
            return lines;
        }
    }
}

/// A paragraph's line as the file had it.
fn whole_line(line: &TextLine) -> Line {
    Line::Whole(line.whole.clone(), line.content)
}

/// Whether `p` is Markdown kept as written that defines what text may
/// name: link reference definitions, and footnotes.
pub(crate) fn is_definition(p: &Paragraph) -> bool {
    matches!(p.props.get(keys::SOURCE), Some(Value::Text(kind)) if kind == "definition" || kind == "footnote")
}

/// How many link reference definitions a story may have before text read
/// back to check it is read with only those its labels name.
const MANY_DEFINITIONS: usize = 64;

/// `s` case folded, nearly as CommonMark matches labels: lowercase, and
/// `ß` and `ẞ` as `ss`.
fn fold(s: &str) -> String {
    s.to_lowercase().to_uppercase().to_lowercase()
}
