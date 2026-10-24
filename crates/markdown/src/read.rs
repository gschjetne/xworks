//! Reading a Markdown file into a story: pulldown-cmark's events, each
//! with its place in the file, made into `doc`'s objects as one snapshot,
//! with what writing them back needs of each.

use std::collections::HashMap;
use std::ops::Range;

use doc::{Block, Document, Kind, Object, ObjectId, Paragraph, Props, Run, Snapshot, Value};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};
use story::{OBJECT, keys};

use crate::{File, Habits, InlineHabits, OPTIONS, Read, Reference, Source, Syntax, TextLine};

/// What reading needs from outside the file.
pub struct Settings<'a> {
    /// The stylesheet the document read takes, as its objects, the sheet
    /// first: the program's, as Markdown can't hold one
    /// ([`story::SheetSpec::objects`]). Headings, code and paragraphs name
    /// its styles by role.
    pub sheet: &'a [(ObjectId, Object)],
    /// The language paragraphs are in where the file's front matter
    /// doesn't say: the system's, as a BCP 47 tag.
    pub language: Option<&'a str>,
    /// The size each picture the file links to is placed at, by its link,
    /// in layout units, where it's known.
    pub sizes: &'a dyn Fn(&str) -> Option<(i64, i64)>,
}

/// Why a file couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// It isn't UTF-8, from this byte on.
    #[error("it isn't UTF-8 text (byte {0} isn't)")]
    NotUtf8(usize),
    /// What was read didn't make a story: a fault in the reader.
    #[error("it couldn't be made a story: {0}")]
    Story(String),
}

/// The links of the pictures the Markdown in `bytes` shows, each once, in
/// the order they come: so they can be found and measured before the file
/// is read ([`Settings::sizes`]).
pub fn links(bytes: &[u8]) -> Vec<String> {
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let Ok(text) = std::str::from_utf8(body) else {
        return Vec::new();
    };
    let start = front_matter(text).map_or(0, |r| r.end);
    let mut out: Vec<String> = Vec::new();
    for event in Parser::new_ext(&text[start..], OPTIONS) {
        if let Event::Start(Tag::Image { dest_url, .. }) = event
            && !out.iter().any(|l| *l == *dest_url)
        {
            out.push(dest_url.to_string());
        }
    }
    out
}

/// Reads the Markdown file `bytes`: UTF-8, with or without a byte order
/// mark.
pub fn read(bytes: &[u8], settings: &Settings<'_>) -> Result<Read, ReadError> {
    let (bom, body) = match bytes.strip_prefix(b"\xef\xbb\xbf") {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let text = std::str::from_utf8(body)
        .map_err(|e| ReadError::NotUtf8(e.valid_up_to() + if bom { 3 } else { 0 }))?;
    let front = front_matter(text);
    let language = front
        .clone()
        .and_then(|r| lang(&text[r]))
        .or_else(|| settings.language.map(str::to_string));
    let start = front.as_ref().map_or(0, |r| r.end);
    let mut builder = Builder::new(text, settings, language.clone(), start);
    for (event, range) in Parser::new_ext(&text[start..], OPTIONS).into_offset_iter() {
        builder.event(event, range.start + start..range.end + start);
    }
    let Built {
        document,
        sources,
        inline,
        habits,
    } = builder.finish()?;
    // The line ending the file's first line has.
    let ending = match text.find(['\n', '\r']) {
        Some(i) if text[i..].starts_with("\r\n") => "\r\n",
        Some(i) if text[i..].starts_with('\r') => "\r",
        _ => "\n",
    };
    let definitions = document
        .paragraphs()
        .into_iter()
        .filter(|&id| {
            document
                .paragraph(id)
                .is_some_and(crate::write::is_definition)
        })
        .collect();
    let file = File {
        text: text.to_string(),
        bom,
        ending,
        final_newline: text.is_empty() || text.ends_with(['\n', '\r']),
        front,
        language,
        habits,
        baseline: document.clone(),
        sources,
        inline,
        definitions,
        cache: None,
    };
    Ok(Read { document, file })
}

/// The front matter at the start of `text`, if it has some: YAML between
/// `---` lines (or ended by `...`), or TOML between `+++` lines, as
/// Jekyll, Hugo and Pandoc write it. Its whole lines.
pub(crate) fn front_matter(text: &str) -> Option<Range<usize>> {
    let first_end = line_end(text, 0);
    let fence = text[..first_end].trim_end();
    if fence != "---" && fence != "+++" {
        return None;
    }
    let mut at = first_end;
    while at < text.len() {
        let end = line_end(text, at);
        let line = text[at..end].trim_end();
        if line == fence || (fence == "---" && line == "...") {
            return Some(0..end);
        }
        at = end;
    }
    None
}

/// The language front matter gives, as `lang: de` in YAML or `lang =
/// "de"` in TOML, at its top level.
fn lang(front: &str) -> Option<String> {
    for line in front.lines() {
        let Some(rest) = line.strip_prefix("lang") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else {
            continue;
        };
        let value = value
            .split(" #")
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Whether byte `i` of `bytes` ends a line: a line feed, or a carriage
/// return not followed by one, as CommonMark has it.
fn ends_line(bytes: &[u8], i: usize) -> bool {
    bytes[i] == b'\n' || (bytes[i] == b'\r' && bytes.get(i + 1) != Some(&b'\n'))
}

/// Where the line holding byte `at` of `text` starts, wherever in a
/// character `at` is.
pub(crate) fn line_start(text: &str, at: usize) -> usize {
    let bytes = text.as_bytes();
    (0..at.min(bytes.len()))
        .rev()
        .find(|&i| ends_line(bytes, i))
        .map_or(0, |i| i + 1)
}

/// Where the line holding byte `at` of `text` ends, after its line ending,
/// wherever in a character `at` is.
pub(crate) fn line_end(text: &str, at: usize) -> usize {
    let bytes = text.as_bytes();
    let at = at.min(bytes.len());
    match (at..bytes.len()).find(|&i| ends_line(bytes, i)) {
        Some(i) => i + 1,
        None => bytes.len(),
    }
}

/// `at` back over the line ending of the line it ends, if it ends one.
pub(crate) fn without_ending(text: &str, at: usize) -> usize {
    let s = &text[..at];
    match s.strip_suffix("\r\n") {
        Some(rest) => rest.len(),
        None => s.strip_suffix(['\n', '\r']).unwrap_or(s).len(),
    }
}

/// The whole lines of `range` of `text`, from its first line's start to
/// its last's line ending, blank lines at its end left out.
fn whole_lines(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = line_start(text, range.start);
    let end = text[..range.end.min(text.len())]
        .trim_end_matches([' ', '\t', '\r', '\n'])
        .len()
        .max(start);
    let end = if end == start && range.end <= start {
        start
    } else {
        line_end(text, end.saturating_sub(1).max(start))
    };
    start..end.max(start)
}

/// What reading made.
struct Built {
    document: Document,
    sources: HashMap<ObjectId, Source>,
    inline: HashMap<ObjectId, InlineHabits>,
    habits: Habits,
}

/// What a container being read is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameKind {
    Root,
    List,
    Item,
    Quote,
    Table,
    Row,
}

/// A container being read.
struct Frame {
    id: ObjectId,
    kind: FrameKind,
    children: Vec<ObjectId>,
    props: Props,
    /// Its place in the file.
    range: Range<usize>,
    /// Where what's been read of it so far ends.
    cursor: usize,
    /// Where the lines of its last block end.
    last_end: usize,
    /// How wide what a list item puts before each of its lines is.
    width: usize,
    first_of: Vec<ObjectId>,
    syntax: Syntax,
    /// A table's columns' alignments.
    aligns: Vec<Alignment>,
    /// A list's items' numbers, and whether any of its items holds a
    /// paragraph apart, as a loose list's do.
    numbers: Vec<u64>,
    loose: bool,
}

/// What a paragraph being read is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParaKind {
    Paragraph,
    Heading,
    Code,
    Html,
    Cell,
}

/// The marks on the text being read.
#[derive(Clone, Debug, Default)]
struct Marks {
    bold: u32,
    italic: u32,
    strike: u32,
    /// Links' targets and titles, innermost last, each with how much text
    /// the paragraph had where it starts and its place in the file.
    links: Vec<(String, String, u32, Range<usize>)>,
}

/// A picture being read: its link and title, and its description's text.
struct Image {
    src: String,
    title: String,
    alt: String,
}

/// A paragraph, heading, code block, HTML block or cell being read.
struct Para {
    id: ObjectId,
    kind: ParaKind,
    props: Props,
    text: String,
    runs: Vec<Run>,
    chars: u32,
    marks: Marks,
    images: Vec<Image>,
    range: Range<usize>,
    /// Whether it's a tight list item's text, which has no paragraph of
    /// its own in pulldown-cmark's events.
    implicit: bool,
    lines: Vec<TextLine>,
    /// Where the line being read starts its text, and its first
    /// character.
    line: Option<(usize, u32)>,
    first_of: Vec<ObjectId>,
    syntax: Syntax,
    /// How it writes its marks, line breaks and links.
    habits: InlineHabits,
}

/// How often the file writes things each way, for its habits.
#[derive(Default)]
struct Counts {
    bullets: [usize; 3],
    delimiters: [usize; 2],
    counting: [usize; 2],
    emphasis: [usize; 2],
    strong: [usize; 2],
    fences: [usize; 2],
    padded: [usize; 2],
    /// Paragraphs on more than one line, the longest of their lines, and
    /// the longest paragraph on one line.
    wrapped: usize,
    longest_wrapped: usize,
    longest_single: usize,
}

/// `sheet`'s paragraph style with `role`, or else the first whose property
/// `key` is `value`.
fn style_by(sheet: &[(ObjectId, Object)], role: &str, key: &str, value: Value) -> Option<ObjectId> {
    let styles = || {
        sheet.iter().filter_map(|(id, o)| match o {
            Object::Style(s) if s.kind == doc::StyleKind::Paragraph => Some((*id, s)),
            _ => None,
        })
    };
    styles()
        .find(|(_, s)| s.props.get(keys::ROLE) == Some(&Value::Text(role.to_string())))
        .or_else(|| styles().find(|(_, s)| s.props.get(key) == Some(&value)))
        .map(|(id, _)| id)
}

struct Builder<'a> {
    text: &'a str,
    sheet: &'a [(ObjectId, Object)],
    /// The sheet's styles paragraphs take: body text's, code's, and each
    /// heading level's, found once.
    body: Option<ObjectId>,
    code: Option<ObjectId>,
    headings: [Option<ObjectId>; 6],
    sizes: &'a dyn Fn(&str) -> Option<(i64, i64)>,
    language: Option<String>,
    next: u64,
    objects: Vec<(ObjectId, Object)>,
    sources: HashMap<ObjectId, Source>,
    inline: HashMap<ObjectId, InlineHabits>,
    stack: Vec<Frame>,
    para: Option<Para>,
    /// While inside a block kept whole, as a footnote's definition is: how
    /// deep, and the block.
    skip: Option<(usize, ObjectId, Range<usize>)>,
    counts: Counts,
}

impl<'a> Builder<'a> {
    fn new(
        text: &'a str,
        settings: &'a Settings<'a>,
        language: Option<String>,
        start: usize,
    ) -> Builder<'a> {
        Builder {
            text,
            sheet: settings.sheet,
            body: story::default_style_id(settings.sheet),
            code: style_by(settings.sheet, "code", keys::CODE, Value::Bool(true)),
            headings: std::array::from_fn(|k| {
                let level = k as i64 + 1;
                style_by(
                    settings.sheet,
                    &format!("heading-{level}"),
                    keys::HEADING,
                    Value::Int(level),
                )
            }),
            sizes: settings.sizes,
            language,
            next: settings
                .sheet
                .iter()
                .map(|(id, _)| id.0 + 1)
                .max()
                .unwrap_or(1)
                .max(1),
            objects: Vec::new(),
            sources: HashMap::new(),
            inline: HashMap::new(),
            stack: vec![Frame {
                id: ObjectId(0),
                kind: FrameKind::Root,
                children: Vec::new(),
                props: Props::new(),
                range: start..text.len(),
                cursor: start,
                last_end: start,
                width: 0,
                first_of: Vec::new(),
                syntax: Syntax::Made,
                aligns: Vec::new(),
                numbers: Vec::new(),
                loose: false,
            }],
            para: None,
            skip: None,
            counts: Counts::default(),
        }
    }

    fn id(&mut self) -> ObjectId {
        let id = ObjectId(self.next);
        self.next += 1;
        id
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("the story is always open")
    }

    /// How far the marker of a list item at `at` is indented, after what
    /// the containers around its list put on its line; none where that
    /// can't be told, as with tabs.
    fn indent_of(&self, at: usize) -> Option<usize> {
        let text = self.text;
        let mut pos = line_start(text, at);
        let n = self.stack.len();
        for frame in &self.stack[1..n.saturating_sub(1)] {
            match frame.kind {
                FrameKind::Quote => {
                    let rest = text.get(pos..at)?;
                    let lead = rest.len() - rest.trim_start_matches(' ').len();
                    if lead > 3 {
                        return None;
                    }
                    pos += lead;
                    if !text[pos..].starts_with('>') {
                        return None;
                    }
                    pos += 1;
                    if text[pos..].starts_with('\t') {
                        return None;
                    }
                    if text[pos..at].starts_with(' ') {
                        pos += 1;
                    }
                }
                FrameKind::Item => {
                    pos += frame.width;
                    if pos > at || !text.is_char_boundary(pos) {
                        return None;
                    }
                }
                _ => {}
            }
        }
        let before = text.get(pos..at)?;
        before.bytes().all(|b| b == b' ').then_some(before.len())
    }

    /// The items whose markers are on the first line of a block starting
    /// at `at` in the innermost container: those it's the first block of,
    /// and whose marker is on its line, innermost first.
    fn first_of(&self, at: usize) -> Vec<ObjectId> {
        let line = line_start(self.text, at);
        let mut out = Vec::new();
        for frame in self.stack.iter().rev() {
            if !frame.children.is_empty() || frame.kind == FrameKind::Root {
                break;
            }
            if frame.kind == FrameKind::Item {
                if line_start(self.text, frame.range.start) != line {
                    break;
                }
                out.push(frame.id);
            }
        }
        out
    }

    /// The style a heading of `level` takes.
    fn heading_style(&self, level: u8) -> Option<ObjectId> {
        self.headings
            .get(usize::from(level).saturating_sub(1))
            .copied()
            .flatten()
    }

    fn body_style(&self) -> Option<ObjectId> {
        self.body
    }

    fn code_style(&self) -> Option<ObjectId> {
        self.code
    }

    /// A paragraph's own properties: its style, if it names one, and the
    /// file's language.
    fn para_props(&self, style: Option<ObjectId>) -> Props {
        let mut props = Props::new();
        if let Some(style) = style {
            props.insert(keys::STYLE.to_string(), Value::Ref(style));
        }
        if let Some(language) = &self.language {
            props.insert(keys::LANGUAGE.to_string(), Value::Text(language.clone()));
        }
        props
    }

    /// Starts reading a paragraph-like block of `kind` at `range`.
    fn open(&mut self, kind: ParaKind, style: Option<ObjectId>, range: Range<usize>) {
        let id = self.id();
        let first_of = self.first_of(range.start);
        let props = self.para_props(style);
        self.para = Some(Para {
            id,
            kind,
            props,
            text: String::new(),
            runs: Vec::new(),
            chars: 0,
            marks: Marks::default(),
            images: Vec::new(),
            range,
            implicit: false,
            lines: Vec::new(),
            line: None,
            first_of,
            syntax: Syntax::Paragraph(Vec::new()),
            habits: InlineHabits::default(),
        });
    }

    /// Adds `object` with `source` to the innermost container, as its
    /// last child.
    fn add(
        &mut self,
        id: ObjectId,
        object: Object,
        lines: Range<usize>,
        first_of: Vec<ObjectId>,
        syntax: Syntax,
    ) {
        let frame = self.stack.last_mut().expect("the story is always open");
        let prev = frame.children.last().copied();
        let parent = frame.id;
        frame.children.push(id);
        if lines.end > frame.cursor {
            frame.cursor = lines.end;
        }
        frame.last_end = frame.last_end.max(lines.end);
        self.sources.insert(
            id,
            Source {
                lines,
                parent,
                prev,
                first_of,
                syntax,
            },
        );
        self.objects.push((id, object));
    }

    /// A paragraph in the innermost container, kept as it was written: its
    /// whole lines, with `text` as the story shows it.
    fn raw(&mut self, kind: &str, text: String, lines: Range<usize>) {
        let id = self.id();
        let first_of = self.first_of(lines.start);
        let mut props = self.para_props(self.code_style());
        props.insert(keys::SOURCE.to_string(), Value::Text(kind.to_string()));
        let parent = self.stack.last().expect("open").id;
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\u{2028}");
        let runs = runs_of(&text);
        let object = Object::Paragraph(Paragraph {
            parent,
            text,
            runs,
            props,
        });
        self.add(id, object, lines, first_of, Syntax::Raw);
    }

    /// Reads what's between the end of what the innermost container has
    /// read and `upto`: blank lines, or Markdown no event shows, such as
    /// link reference definitions, which are kept as they were written,
    /// each run of lines as a block of its own.
    fn gap(&mut self, upto: usize) {
        let frame = self.stack.last().expect("open");
        // A list holds only items: what pulldown-cmark counts as its, after
        // its last, such as link reference definitions, is the next
        // container's to read.
        if matches!(
            frame.kind,
            FrameKind::Table | FrameKind::Row | FrameKind::List
        ) || self.skip.is_some()
        {
            return;
        }
        let from = frame.cursor;
        if upto <= from {
            return;
        }
        let depth = self
            .stack
            .iter()
            .filter(|f| matches!(f.kind, FrameKind::Quote))
            .count();
        let mut at = from;
        let mut run: Option<(usize, usize, String)> = None;
        let mut found: Vec<(Range<usize>, String)> = Vec::new();
        while at < upto {
            let end = line_end(self.text, at).min(upto);
            let line = &self.text[at..without_ending(self.text, end).max(at)];
            let content = strip_prefixes(line, depth);
            // Blank, or what pulldown-cmark takes for blank, such as a line
            // of vertical tabs.
            if content.trim().is_empty() {
                if let Some((start, last, text)) = run.take() {
                    found.push((line_start(self.text, start)..last, text));
                }
            } else {
                match &mut run {
                    Some((_, last, text)) => {
                        text.push('\n');
                        text.push_str(content.trim_matches([' ', '\t']));
                        *last = end;
                    }
                    None => run = Some((at, end, content.trim_matches([' ', '\t']).to_string())),
                }
            }
            at = end;
        }
        if let Some((start, last, text)) = run {
            found.push((line_start(self.text, start)..last, text));
        }
        for (lines, text) in found {
            self.raw("definition", text, lines);
        }
        let frame = self.top();
        if upto > frame.cursor {
            frame.cursor = upto;
        }
    }

    /// Ends the paragraph a tight list item's text makes, if one is open.
    fn close_implicit(&mut self) {
        if self.para.as_ref().is_some_and(|p| p.implicit) {
            self.close();
        }
    }

    /// Opens a tight list item's paragraph for inline content at `range`,
    /// if nothing is open to take it.
    fn inline_at(&mut self, range: &Range<usize>) {
        if self.para.is_none() {
            let top = self.stack.last().expect("open");
            if top.kind != FrameKind::Item {
                return;
            }
            self.gap(
                line_start(self.text, range.start).max(self.stack.last().expect("open").cursor),
            );
            let style = self.body_style();
            self.open(ParaKind::Paragraph, style, range.clone());
            if let Some(p) = &mut self.para {
                p.implicit = true;
            }
        }
        if let Some(p) = &mut self.para {
            if p.implicit {
                p.range.end = p.range.end.max(range.end);
            }
            if p.line.is_none() && p.images.is_empty() {
                p.line = Some((range.start, p.chars));
            }
        }
    }

    /// The marks on the text being read, as a run's properties.
    fn marks(&self) -> Props {
        let mut props = Props::new();
        let Some(p) = &self.para else {
            return props;
        };
        if p.marks.bold > 0 {
            props.insert(keys::BOLD.to_string(), Value::Bool(true));
        }
        if p.marks.italic > 0 {
            props.insert(keys::ITALIC.to_string(), Value::Bool(true));
        }
        if p.marks.strike > 0 {
            props.insert(keys::STRIKETHROUGH.to_string(), Value::Bool(true));
        }
        if let Some((url, title, _, _)) = p.marks.links.last() {
            props.insert(keys::LINK.to_string(), Value::Text(url.clone()));
            if !title.is_empty() {
                props.insert(keys::LINK_TITLE.to_string(), Value::Text(title.clone()));
            }
        }
        props
    }

    /// Adds `text` to the paragraph being read, with the marks on it and
    /// `extra`; to the description of a picture, if one's being read.
    fn push(&mut self, text: &str, extra: Props) {
        if text.is_empty() {
            return;
        }
        let mut props = self.marks();
        let Some(p) = &mut self.para else {
            return;
        };
        if let Some(image) = p.images.last_mut() {
            image.alt.push_str(text);
            return;
        }
        props.extend(extra);
        let n = text.chars().count() as u32;
        p.text.push_str(text);
        p.chars += n;
        match p.runs.last_mut() {
            Some(last) if *last.props == props => last.len += n,
            _ => p.runs.push(Run::new(n, props)),
        }
    }

    /// Ends the line of text being read, after a soft or a hard break.
    fn end_line(&mut self, soft: bool) {
        let text = self.text;
        let Some(p) = &mut self.para else {
            return;
        };
        if let Some((content, first)) = p.line.take() {
            let whole = line_start(text, content)..without_ending(text, line_end(text, content));
            p.lines.push(TextLine {
                whole,
                content,
                chars: first..p.chars,
                soft,
            });
        }
    }

    /// Ends the paragraph-like block being read, adding it to its
    /// container.
    fn close(&mut self) {
        self.end_line(false);
        let Some(mut p) = self.para.take() else {
            return;
        };
        match p.kind {
            ParaKind::Code | ParaKind::Html => {
                let mut text = p.text.replace("\r\n", "\n").replace('\r', "\n");
                if text.ends_with('\n') {
                    text.pop();
                }
                // Blank lines an HTML block ends in, where it ends with
                // its container, are the blank line after it, which a
                // block written after it has anyway.
                if p.kind == ParaKind::Html {
                    while let Some(end) = text.rfind('\n')
                        && text[end + 1..].trim_matches([' ', '\t']).is_empty()
                    {
                        text.truncate(end);
                    }
                }
                p.text = text.replace('\n', "\u{2028}");
                p.runs = runs_of(&p.text);
            }
            _ => {}
        }
        let lines = whole_lines(self.text, &p.range);
        if p.kind == ParaKind::Paragraph {
            self.alert(&mut p);
        }
        let syntax = match p.kind {
            ParaKind::Paragraph => {
                if p.lines.len() > 1 {
                    self.counts.wrapped += 1;
                    for line in &p.lines {
                        let n = self.text[line.whole.clone()].chars().count();
                        self.counts.longest_wrapped = self.counts.longest_wrapped.max(n);
                    }
                } else if let Some(line) = p.lines.first() {
                    let n = self.text[line.whole.clone()].chars().count();
                    self.counts.longest_single = self.counts.longest_single.max(n);
                }
                Syntax::Paragraph(std::mem::take(&mut p.lines))
            }
            ParaKind::Cell => Syntax::Paragraph(Vec::new()),
            _ => std::mem::replace(&mut p.syntax, Syntax::Made),
        };
        let parent = self.stack.last().expect("open").id;
        let object = Object::Paragraph(Paragraph {
            parent,
            text: p.text,
            runs: p.runs,
            props: p.props,
        });
        if p.habits != InlineHabits::default() {
            self.inline.insert(p.id, p.habits);
        }
        self.add(p.id, object, lines, p.first_of, syntax);
    }

    /// GitHub's alert, `[!NOTE]` and the like alone on the first line of
    /// a quote's first paragraph, kept as Markdown kept as written
    /// (review jy4a2tx5), as the extensions not taken are: shown as
    /// source, and written back as it was.
    fn alert(&self, p: &mut Para) {
        let Some(frame) = self.stack.last() else {
            return;
        };
        let Some(first) = p.lines.first() else {
            return;
        };
        if frame.kind != FrameKind::Quote
            || !frame.children.is_empty()
            || line_start(self.text, frame.range.start) != line_start(self.text, first.content)
        {
            return;
        }
        let line = self.text[first.content.min(first.whole.end)..first.whole.end].trim_end();
        let is_alert = line
            .strip_prefix("[!")
            .and_then(|rest| rest.strip_suffix(']'))
            .is_some_and(|kind| {
                ["note", "tip", "important", "warning", "caution"]
                    .iter()
                    .any(|k| kind.eq_ignore_ascii_case(k))
            });
        if is_alert && p.text.starts_with(line) {
            let n = line.chars().count() as u32;
            p.runs = with_prop(&p.runs, 0, n, keys::SOURCE, Value::Text("alert".into()));
        }
    }

    /// Starts reading a container of `kind` at `range`.
    fn push_frame(&mut self, kind: FrameKind, range: Range<usize>, props: Props, syntax: Syntax) {
        let id = self.id();
        let first_of = self.first_of(range.start);
        let cursor = match kind {
            FrameKind::Item => item_content(self.text, range.start),
            _ => line_start(self.text, range.start),
        };
        self.stack.push(Frame {
            id,
            kind,
            children: Vec::new(),
            props,
            range,
            cursor,
            last_end: cursor,
            width: 0,
            first_of,
            syntax,
            aligns: Vec::new(),
            numbers: Vec::new(),
            loose: false,
        });
    }

    /// Ends the innermost container, adding it to the one around it.
    fn pop_frame(&mut self) {
        self.close_implicit();
        // What's between its last block and its end; a container's end
        // can be part way along the line its next block starts on.
        let end = self.stack.last().expect("open").range.end;
        self.gap(end);
        let mut frame = self.stack.pop().expect("a container to end");
        if matches!(frame.kind, FrameKind::Item | FrameKind::Quote) && frame.children.is_empty() {
            // An empty item or quote holds an empty paragraph, to type in.
            let id = self.id();
            let props = self.para_props(self.body_style());
            self.objects.push((
                id,
                Object::Paragraph(Paragraph {
                    parent: frame.id,
                    text: String::new(),
                    runs: Vec::new(),
                    props,
                }),
            ));
            self.sources.insert(
                id,
                Source {
                    lines: frame.cursor..frame.cursor,
                    parent: frame.id,
                    prev: None,
                    first_of: Vec::new(),
                    syntax: Syntax::Made,
                },
            );
            frame.children.push(id);
        }
        let kind = match frame.kind {
            FrameKind::List => Kind::List,
            FrameKind::Item => Kind::Item,
            FrameKind::Quote => Kind::Quote,
            FrameKind::Table => Kind::Table,
            FrameKind::Row => Kind::Row,
            FrameKind::Root => unreachable!("the story isn't ended"),
        };
        if frame.kind == FrameKind::List {
            let counting = frame.numbers.len() < 2 || frame.numbers[1] != frame.numbers[0];
            if frame.numbers.len() >= 2 {
                self.counts.counting[usize::from(counting)] += 1;
            }
            if let Syntax::List {
                loose, counting: c, ..
            } = &mut frame.syntax
            {
                *loose = frame.loose;
                *c = counting;
            }
        }
        if let (FrameKind::Table, Syntax::Table { widths, .. }) = (frame.kind, &frame.syntax) {
            self.counts.padded[usize::from(widths.is_some())] += 1;
        }
        // A container's lines are its first and its blocks': where it ends
        // is the line its next block may start on.
        let lines = match frame.kind {
            FrameKind::Table | FrameKind::Row => whole_lines(self.text, &frame.range),
            FrameKind::List => {
                let start = line_start(self.text, frame.range.start);
                start..line_end(self.text, frame.range.start).max(frame.last_end)
            }
            _ => {
                let r = &frame.range;
                let end = if r.end < self.text.len() && !self.text[..r.end].ends_with(['\n', '\r'])
                {
                    line_start(self.text, r.end).max(r.start)
                } else {
                    r.end
                };
                let lines = whole_lines(self.text, &(r.start..end));
                lines.start
                    ..lines
                        .end
                        .max(frame.last_end)
                        .max(line_end(self.text, r.start))
            }
        };
        let parent = self.stack.last().expect("open").id;
        let object = Object::Block(Block {
            kind,
            parent,
            children: frame.children,
            props: frame.props,
        });
        // A list's looseness is its items' business: an item holding a
        // paragraph apart makes its list loose.
        self.add(frame.id, object, lines, frame.first_of, frame.syntax);
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        if let Some((depth, id, whole)) = &mut self.skip {
            match event {
                Event::Start(_) => *depth += 1,
                Event::End(_) if *depth == 0 => {
                    let (id, whole) = (*id, whole.clone());
                    self.skip = None;
                    self.end_kept(id, whole);
                }
                Event::End(_) => *depth -= 1,
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => {
                if self
                    .para
                    .as_ref()
                    .is_some_and(|p| matches!(p.kind, ParaKind::Code | ParaKind::Html))
                {
                    if let Some(p) = &mut self.para {
                        p.text.push_str(&text);
                    }
                    return;
                }
                self.inline_at(&range);
                self.push(&text, Props::new());
            }
            Event::Code(text) => {
                self.inline_at(&range);
                self.push(&text, one(keys::CODE, Value::Bool(true)));
            }
            Event::InlineHtml(html) => {
                self.inline_at(&range);
                self.push(&html, one(keys::SOURCE, Value::Text("html".into())));
            }
            Event::Html(html) => {
                if let Some(p) = &mut self.para {
                    p.text.push_str(&html);
                }
            }
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                self.inline_at(&range);
                let kind = if matches!(event, Event::FootnoteReference(_)) {
                    "footnote"
                } else {
                    "math"
                };
                let source = self.text[range.clone()].to_string();
                self.push(&source, one(keys::SOURCE, Value::Text(kind.into())));
            }
            Event::SoftBreak => {
                self.inline_at(&range);
                if self.para.as_ref().is_some_and(|p| !p.images.is_empty()) {
                    self.push(" ", Props::new());
                    return;
                }
                self.end_line(true);
                self.push(" ", Props::new());
            }
            Event::HardBreak => {
                self.inline_at(&range);
                if self.text[range.clone()].starts_with(' ')
                    && let Some(p) = &mut self.para
                {
                    p.habits.spaced_breaks = true;
                }
                if self.para.as_ref().is_some_and(|p| !p.images.is_empty()) {
                    self.push(" ", Props::new());
                    return;
                }
                self.push("\u{2028}", Props::new());
                self.end_line(false);
            }
            Event::Rule => {
                // A thematic break: a rule, a block the story holds
                // (review jy4a2tx5).
                self.block_start(&range);
                let id = self.id();
                let first_of = self.first_of(range.start);
                let parent = self.stack.last().expect("open").id;
                let rule = Object::Block(Block {
                    kind: Kind::Rule,
                    parent,
                    children: Vec::new(),
                    props: Props::new(),
                });
                self.add(
                    id,
                    rule,
                    whole_lines(self.text, &range),
                    first_of,
                    Syntax::Rule,
                );
            }
            Event::TaskListMarker(done) => {
                if let Some(item) = self
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|f| f.kind == FrameKind::Item)
                {
                    item.props.insert(keys::TASK.to_string(), Value::Bool(done));
                    if let Syntax::Item { task, .. } = &mut item.syntax {
                        *task = Some(range.clone());
                    }
                    item.cursor = item.cursor.max(after_spaces(self.text, range.end));
                }
            }
        }
    }

    /// What starting a block does to what's open: a tight item's text
    /// ends, and what's between the last block and this one is read.
    fn block_start(&mut self, range: &Range<usize>) {
        self.close_implicit();
        let at = line_start(self.text, range.start).max(self.stack.last().expect("open").cursor);
        self.gap(at);
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Paragraph => {
                self.block_start(&range);
                // A paragraph of its own in an item makes its list loose.
                let n = self.stack.len();
                if n >= 2
                    && self.stack[n - 1].kind == FrameKind::Item
                    && self.stack[n - 2].kind == FrameKind::List
                {
                    self.stack[n - 2].loose = true;
                }
                let style = self.body_style();
                self.open(ParaKind::Paragraph, style, range);
            }
            Tag::Heading { level, .. } => {
                self.block_start(&range);
                let style = self.heading_style(level as u8);
                let setext = !self.text[range.start..].trim_start().starts_with('#');
                self.open(ParaKind::Heading, style, range.clone());
                let underline = setext
                    .then(|| {
                        let end = without_ending(self.text, whole_lines(self.text, &range).end);
                        let last = &self.text[line_start(self.text, end.saturating_sub(1))..end];
                        last.trim().chars().next()
                    })
                    .flatten();
                if let Some(p) = &mut self.para {
                    p.syntax = Syntax::Heading(underline);
                }
            }
            Tag::CodeBlock(kind) => {
                self.block_start(&range);
                let style = self.code_style();
                let fence = match &kind {
                    CodeBlockKind::Fenced(_) => {
                        let rest = &self.text[range.start..];
                        let c = rest.chars().next().unwrap_or('`');
                        let n = rest.chars().take_while(|&x| x == c).count();
                        Some((c, n))
                    }
                    CodeBlockKind::Indented => None,
                };
                self.counts.fences[usize::from(matches!(fence, Some(('~', _))))] +=
                    usize::from(fence.is_some());
                self.open(ParaKind::Code, style, range);
                if let Some(p) = &mut self.para {
                    if let CodeBlockKind::Fenced(info) = kind
                        && !info.trim().is_empty()
                    {
                        p.props
                            .insert(keys::INFO.to_string(), Value::Text(info.trim().to_string()));
                    }
                    p.syntax = Syntax::Code(fence);
                }
            }
            Tag::HtmlBlock => {
                self.block_start(&range);
                let style = self.code_style();
                self.open(ParaKind::Html, style, range);
                if let Some(p) = &mut self.para {
                    p.props
                        .insert(keys::SOURCE.to_string(), Value::Text("html".into()));
                    p.syntax = Syntax::Raw;
                }
            }
            Tag::FootnoteDefinition(_)
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {
                self.block_start(&range);
                let id = self.id();
                self.skip = Some((0, id, range));
            }
            Tag::List(start) => {
                self.block_start(&range);
                let marker = item_marker(self.text, range.start);
                let c = self.text[marker.clone()].chars().last().unwrap_or('-');
                match c {
                    '-' => self.counts.bullets[0] += 1,
                    '*' => self.counts.bullets[1] += 1,
                    '+' => self.counts.bullets[2] += 1,
                    '.' => self.counts.delimiters[0] += 1,
                    _ => self.counts.delimiters[1] += 1,
                }
                let mut props = Props::new();
                if let Some(n) = start {
                    props.insert(keys::ORDERED.to_string(), Value::Bool(true));
                    if n != 1 {
                        props.insert(
                            keys::START.to_string(),
                            Value::Int(n.min(i64::MAX as u64) as i64),
                        );
                    }
                }
                self.push_frame(
                    FrameKind::List,
                    range,
                    props,
                    Syntax::List {
                        marker: c,
                        loose: false,
                        counting: true,
                    },
                );
            }
            Tag::Item => {
                self.block_start(&range);
                let marker = item_marker(self.text, range.start);
                let number = self.text[marker.clone()]
                    .trim_end_matches(['.', ')'])
                    .parse::<u64>()
                    .ok();
                if let Some(list) = self.stack.last_mut()
                    && let Some(n) = number
                {
                    list.numbers.push(n);
                }
                let rest = &self.text
                    [marker.end..without_ending(self.text, line_end(self.text, marker.end))];
                let spaces = rest.chars().take_while(|&c| c == ' ').count();
                let tabbed = rest[spaces..].starts_with('\t');
                // An item whose text starts on the line after its marker is
                // written with one space after it, when it's written anew;
                // one whose text is code after five or more, with one.
                let spaces = if spaces == rest.len() || spaces > 4 {
                    1
                } else {
                    spaces.max(1)
                };
                let indent = self.indent_of(marker.start).filter(|_| !tabbed);
                self.push_frame(
                    FrameKind::Item,
                    range,
                    Props::new(),
                    Syntax::Item {
                        marker: marker.clone(),
                        indent,
                        spaces,
                        task: None,
                    },
                );
                self.top().width = indent.unwrap_or(0) + marker.len() + spaces;
            }
            Tag::BlockQuote(_) => {
                self.block_start(&range);
                let gt = self.text[range.start..]
                    .find('>')
                    .map_or(range.start, |i| range.start + i);
                let after = &self.text[(gt + 1).min(self.text.len())..];
                let prefix = if after.starts_with(' ') {
                    Some("> ".to_string())
                } else if after.starts_with('\t') {
                    None
                } else {
                    Some(">".to_string())
                };
                self.push_frame(
                    FrameKind::Quote,
                    range.clone(),
                    Props::new(),
                    Syntax::Quote(prefix),
                );
            }
            Tag::Table(aligns) => {
                self.block_start(&range);
                let lines = whole_lines(self.text, &range);
                let header = line_end(self.text, lines.start);
                let delimiter = header..line_end(self.text, header);
                let widths = padded_widths(self.text, &lines);
                self.push_frame(
                    FrameKind::Table,
                    range,
                    Props::new(),
                    Syntax::Table { delimiter, widths },
                );
                self.top().aligns = aligns;
            }
            Tag::TableHead | Tag::TableRow => {
                self.push_frame(FrameKind::Row, range, Props::new(), Syntax::Row);
            }
            Tag::TableCell => {
                let column = self.stack.last().map_or(0, |f| f.children.len());
                let align = self
                    .stack
                    .iter()
                    .rev()
                    .find(|f| f.kind == FrameKind::Table)
                    .and_then(|t| t.aligns.get(column).copied());
                self.open(ParaKind::Cell, None, range);
                if let Some(p) = &mut self.para {
                    let align = match align {
                        Some(Alignment::Left) => Some("start"),
                        Some(Alignment::Center) => Some("center"),
                        Some(Alignment::Right) => Some("end"),
                        _ => None,
                    };
                    if let Some(align) = align {
                        p.props
                            .insert(keys::ALIGN.to_string(), Value::Text(align.to_string()));
                    }
                }
            }
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {
                self.inline_at(&range);
                let c = self.text[range.start..].chars().next().unwrap_or('*');
                let Some(p) = &mut self.para else { return };
                match tag {
                    Tag::Emphasis => {
                        p.marks.italic += 1;
                        p.habits.emphasis.get_or_insert(c);
                        self.counts.emphasis[usize::from(c == '_')] += 1;
                    }
                    Tag::Strong => {
                        p.marks.bold += 1;
                        p.habits.strong.get_or_insert(c);
                        self.counts.strong[usize::from(c == '_')] += 1;
                    }
                    _ => p.marks.strike += 1,
                }
            }
            Tag::Superscript | Tag::Subscript => {
                self.inline_at(&range);
            }
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => {
                self.inline_at(&range);
                let url = if link_type == LinkType::Email {
                    format!("mailto:{dest_url}")
                } else {
                    dest_url.to_string()
                };
                if let Some(p) = &mut self.para {
                    if matches!(
                        link_type,
                        LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut
                    ) {
                        p.habits.references.push(Reference {
                            url: url.clone(),
                            title: title.to_string(),
                            label: id.to_string(),
                            shortcut: link_type != LinkType::Reference,
                        });
                    }
                    p.marks
                        .links
                        .push((url, title.to_string(), p.chars, range.clone()));
                }
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                self.inline_at(&range);
                if let Some(p) = &mut self.para {
                    p.images.push(Image {
                        src: dest_url.to_string(),
                        title: title.to_string(),
                        alt: String::new(),
                    });
                }
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.mark(|m| m.italic = m.italic.saturating_sub(1)),
            TagEnd::Strong => self.mark(|m| m.bold = m.bold.saturating_sub(1)),
            TagEnd::Strikethrough => self.mark(|m| m.strike = m.strike.saturating_sub(1)),
            TagEnd::Link => {
                let Some(p) = &mut self.para else { return };
                let Some((_, _, chars, range)) = p.marks.links.pop() else {
                    return;
                };
                // A link of no text is kept as it was written.
                if p.chars == chars && p.images.is_empty() {
                    let source = self.text[range].to_string();
                    self.push(&source, one(keys::SOURCE, Value::Text("link".into())));
                }
            }
            TagEnd::Image => {
                let Some(p) = &mut self.para else { return };
                let Some(image) = p.images.pop() else { return };
                match p.images.last_mut() {
                    // A picture in a picture's description gives it its
                    // description.
                    Some(outer) => outer.alt.push_str(&image.alt),
                    None => self.picture(image),
                }
            }
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::TableCell => self.close(),
            TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow => self.pop_frame(),
            TagEnd::FootnoteDefinition
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }

    /// Changes the marks on the text being read.
    fn mark(&mut self, change: impl FnOnce(&mut Marks)) {
        if let Some(p) = &mut self.para {
            change(&mut p.marks);
        }
    }

    /// Adds `image`, read, to the paragraph's text as a picture.
    fn picture(&mut self, image: Image) {
        let mut props = Props::new();
        props.insert(keys::PICTURE.to_string(), Value::Text(image.src.clone()));
        props.insert(
            keys::PICTURE_SRC.to_string(),
            Value::Text(image.src.clone()),
        );
        if !image.alt.is_empty() {
            props.insert(keys::PICTURE_ALT.to_string(), Value::Text(image.alt));
        }
        if !image.title.is_empty() {
            props.insert(keys::PICTURE_TITLE.to_string(), Value::Text(image.title));
        }
        if let Some((w, h)) = (self.sizes)(&image.src) {
            props.insert(keys::PICTURE_WIDTH.to_string(), Value::Int(w));
            props.insert(keys::PICTURE_HEIGHT.to_string(), Value::Int(h));
        }
        self.push(&OBJECT.to_string(), props);
    }

    /// Ends a block kept whole, as a footnote's definition is.
    fn end_kept(&mut self, _id: ObjectId, range: Range<usize>) {
        let lines = whole_lines(self.text, &range);
        let depth = self
            .stack
            .iter()
            .filter(|f| matches!(f.kind, FrameKind::Quote))
            .count();
        let text: Vec<&str> = self.text[lines.clone()]
            .lines()
            .map(|l| strip_prefixes(l, depth).trim_end())
            .collect();
        let kind = if self.text[range.start..].starts_with("[^") {
            "footnote"
        } else {
            "metadata"
        };
        self.raw(kind, text.join("\n"), lines);
    }

    fn finish(mut self) -> Result<Built, ReadError> {
        if self.skip.is_some() {
            let (_, id, range) = self.skip.take().expect("checked");
            self.end_kept(id, range.start..self.text.len());
        }
        if self.para.is_some() {
            self.close();
        }
        while self.stack.len() > 1 {
            let end = self.text.len();
            self.top().range.end = end;
            self.pop_frame();
        }
        self.gap(self.text.len());
        let has_paragraph = self
            .objects
            .iter()
            .any(|(_, o)| matches!(o, Object::Paragraph(_)));
        if !has_paragraph {
            // A story always has a paragraph.
            let id = self.id();
            let props = self.para_props(self.body_style());
            self.objects.push((
                id,
                Object::Paragraph(Paragraph {
                    parent: ObjectId(0),
                    text: String::new(),
                    runs: Vec::new(),
                    props,
                }),
            ));
            let at = self.text.len();
            self.sources.insert(
                id,
                Source {
                    lines: at..at,
                    parent: ObjectId(0),
                    prev: self.stack[0].children.last().copied(),
                    first_of: Vec::new(),
                    syntax: Syntax::Made,
                },
            );
            self.stack[0].children.push(id);
        }
        let root = self.stack.pop().expect("the story");
        let mut objects = self.objects;
        objects.extend(self.sheet.iter().cloned());
        objects.push((
            ObjectId(0),
            Object::Story(doc::Story {
                blocks: root.children,
            }),
        ));
        objects.sort_by_key(|(id, _)| *id);
        let document = Document::from_snapshot(Snapshot {
            root: ObjectId(0),
            next_id: self.next,
            objects,
        })
        .map_err(|e| ReadError::Story(e.to_string()))?;
        let c = &self.counts;
        let most = |counts: &[usize], chars: &[char], default: char| {
            let best = counts.iter().copied().max().unwrap_or(0);
            if best == 0 {
                return default;
            }
            chars[counts.iter().position(|&n| n == best).unwrap_or(0)]
        };
        let width = (c.wrapped >= 2
            && (40..=120).contains(&c.longest_wrapped)
            && c.longest_single <= c.longest_wrapped)
            .then_some(c.longest_wrapped);
        let habits = Habits {
            bullet: most(&c.bullets, &['-', '*', '+'], '-'),
            delimiter: most(&c.delimiters, &['.', ')'], '.'),
            counting: c.counting[0] <= c.counting[1],
            emphasis: most(&c.emphasis, &['*', '_'], '*'),
            strong: most(&c.strong, &['*', '_'], '*'),
            width,
            fence: most(&c.fences, &['`', '~'], '`'),
            padded: c.padded[1] >= c.padded[0],
        };
        Ok(Built {
            document,
            sources: self.sources,
            inline: self.inline,
            habits,
        })
    }
}

/// Runs covering `text` with no properties.
fn runs_of(text: &str) -> Vec<Run> {
    let n = text.chars().count() as u32;
    if n == 0 {
        Vec::new()
    } else {
        vec![Run::new(n, Props::new())]
    }
}

/// `runs` with `key` set to `value` on characters `from..to`, the runs
/// split where that starts and ends and joined where they come out the
/// same.
pub(crate) fn with_prop(runs: &[Run], from: u32, to: u32, key: &str, value: Value) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len() + 2);
    let mut push = |len: u32, props: Props| {
        if len == 0 {
            return;
        }
        match out.last_mut() {
            Some(last) if *last.props == props => last.len += len,
            _ => out.push(Run::new(len, props)),
        }
    };
    let mut at = 0;
    for run in runs {
        let (start, end) = (at, at + run.len);
        at = end;
        let (a, b) = (from.clamp(start, end), to.clamp(start, end));
        if a >= b {
            push(run.len, (*run.props).clone());
            continue;
        }
        push(a - start, (*run.props).clone());
        let mut marked = (*run.props).clone();
        marked.insert(key.to_string(), value.clone());
        push(b - a, marked);
        push(end - b, (*run.props).clone());
    }
    out
}

/// Properties of one key.
fn one(key: &str, value: Value) -> Props {
    Props::from([(key.to_string(), value)])
}

/// `line` without what `depth` quotes put before it: spaces, and a `>`
/// with a space after it, each.
pub(crate) fn strip_prefixes(line: &str, depth: usize) -> &str {
    let mut rest = line;
    for _ in 0..depth {
        let trimmed = rest.trim_start_matches([' ', '\t']);
        match trimmed.strip_prefix('>') {
            Some(after) => rest = after.strip_prefix(' ').unwrap_or(after),
            None => break,
        }
    }
    rest
}

/// The marker of the list item at `at`: `-`, `*` or `+`, or a number and
/// `.` or `)`.
pub(crate) fn item_marker(text: &str, at: usize) -> Range<usize> {
    let rest = &text[at..];
    let lead = rest.len() - rest.trim_start_matches(' ').len();
    let start = at + lead;
    let rest = &text[start..];
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let end = if digits > 0 {
        start + digits + usize::from(rest[digits..].starts_with(['.', ')']))
    } else {
        start + rest.chars().next().map_or(0, char::len_utf8)
    };
    start..end
}

/// Where the text of the list item at `at` starts: after its marker and
/// the spaces after it, or the line's end.
fn item_content(text: &str, at: usize) -> usize {
    let marker = item_marker(text, at);
    after_spaces(text, marker.end)
}

/// `at` past the spaces and tabs after it, on its line.
fn after_spaces(text: &str, at: usize) -> usize {
    let rest = &text[at.min(text.len())..];
    at + (rest.len() - rest.trim_start_matches([' ', '\t']).len())
}

/// The widths of a table's columns, if its rows' pipes line up: each the
/// width of its delimiter row's cell, less the space either side.
fn padded_widths(text: &str, lines: &Range<usize>) -> Option<Vec<usize>> {
    let rows: Vec<&str> = text[lines.clone()].lines().map(str::trim_end).collect();
    if rows.len() < 2 {
        return None;
    }
    let pipes = |row: &str| -> Vec<usize> {
        let mut out = Vec::new();
        let mut escaped = false;
        for (i, c) in row.chars().enumerate() {
            if c == '|' && !escaped {
                out.push(i);
            }
            escaped = c == '\\' && !escaped;
        }
        out
    };
    let first = pipes(rows[1]);
    if first.len() < 2 || rows.iter().any(|r| pipes(r) != first) {
        return None;
    }
    Some(
        first
            .windows(2)
            .map(|w| (w[1] - w[0]).saturating_sub(3))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_found_and_gives_the_language() {
        let text = "---\ntitle: Notes\nlang: nb\n---\n# Notes\n";
        let front = front_matter(text).expect("front matter");
        assert_eq!(&text[front.clone()], "---\ntitle: Notes\nlang: nb\n---\n");
        assert_eq!(lang(&text[front]), Some("nb".to_string()));
        assert_eq!(lang("+++\nlang = \"de\"\n+++\n"), Some("de".to_string()));
        assert_eq!(front_matter("--- \nno end\n"), None);
        assert_eq!(front_matter("# Title\n---\n"), None);
    }

    #[test]
    fn lines_are_whole() {
        let text = "> a\n> b\n\nc";
        assert_eq!(whole_lines(text, &(2..8)), 0..8);
        assert_eq!(whole_lines(text, &(9..10)), 9..10);
        assert_eq!(item_marker("  12. x", 0), 2..5);
        assert_eq!(item_marker("- x", 0), 0..1);
        assert_eq!(item_content("-   x", 0), 4);
    }
}
