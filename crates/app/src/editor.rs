//! One document in one window: its file, its text and history, where the
//! caret is, and what the window shows. Shells feed it [`Event`]s and carry
//! out the [`Effect`]s it returns; [`Editor::view`] draws the window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use canvas::Piece;
use chrome::{CommandState, Key, Modifiers};
use doc::{Document, History, Object, ObjectId, Op, Paragraph, Props, Run, Transaction, Value};
use rayon::prelude::*;
use store::{Entry, Store};
use story::{ParagraphLayout, keys};
use text::units::{PIXEL, POINT};
use text::{Affinity, Cursor};
use toolkit::{List, MenuBar, MenuHit};

use crate::Resources;

/// A place in a document: a paragraph, an offset into its text in
/// characters, and whether it belongs with the character before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    /// The paragraph.
    pub paragraph: ObjectId,
    /// The offset, in Unicode scalar values.
    pub offset: u32,
    /// Whether it belongs with the character before it rather than after.
    pub upstream: bool,
}

/// Something that happened to a window.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A key that doesn't type text, with the modifiers held.
    Key {
        /// The key.
        key: Key,
        /// The modifiers.
        modifiers: Modifiers,
    },
    /// Text typed, or committed by an input method.
    Text(String),
    /// An input method's composition in progress; empty when it ends.
    Preedit(String),
    /// The pointer pressed at (`x`, `y`), in logical pixels from the
    /// window's top left; `count` is 2 for a double click.
    Press {
        /// Across.
        x: f64,
        /// Down.
        y: f64,
        /// Whether Shift is held, to extend the selection.
        shift: bool,
    },
    /// The pointer moved, with the button held or not.
    Move {
        /// Across.
        x: f64,
        /// Down.
        y: f64,
    },
    /// The pointer's button let go.
    Release,
    /// The wheel or trackpad scrolled, in logical pixels, down positive.
    Scroll(f64),
    /// The window's size, in logical pixels.
    Resize {
        /// Its width.
        width: f64,
        /// Its height.
        height: f64,
    },
    /// A command, from a menu, a shortcut or a script.
    Command(String),
    /// Text from the clipboard, after [`Effect::Paste`].
    Pasted(String),
}

/// Something the shell does for the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Put this text on the clipboard.
    Copy(String),
    /// Read the clipboard and send it back as [`Event::Pasted`].
    Paste,
    /// Open a window: a new document, or the one at this path.
    Open(Option<PathBuf>),
    /// Hand this file to the system to open, such as a PDF to print.
    OpenExternally(PathBuf),
    /// Show this file in the system's file manager, such as an export.
    Reveal(PathBuf),
    /// Quit the app.
    Quit,
}

/// Why a document couldn't be opened or made.
#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    /// The file couldn't be read or written.
    #[error(transparent)]
    Store(#[from] store::Error),
    /// The documents folder couldn't be made.
    #[error("{path}: {source}")]
    Io {
        /// The folder.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// The paragraphs as laid out, for the document they were laid out from.
struct Laid {
    /// The document laid out; its objects are compared with the current
    /// one's by identity, so unchanged paragraphs keep their layouts.
    document: Document,
    paragraphs: Vec<ParagraphLayout>,
    index: HashMap<ObjectId, usize>,
    pages: Vec<Vec<Piece>>,
    /// Where each paragraph's lines are: (page, top within the page's
    /// body), by paragraph and line.
    places: Vec<Vec<(usize, i64)>>,
    /// Each paragraph's words, for the status bar, counted when it's laid
    /// out.
    words: Vec<usize>,
}

/// The Open panel: documents to choose from.
struct Panel {
    paths: Vec<PathBuf>,
    selected: usize,
}

/// One document in one window.
pub struct Editor {
    res: Arc<Resources>,
    path: PathBuf,
    store: Store,
    document: Document,
    history: History,
    laid: Laid,
    anchor: Position,
    focus: Position,
    /// Where Up and Down aim, across lines.
    goal: Option<i64>,
    /// Where a run of typing goes on, if one is going.
    typing: Option<Position>,
    /// Properties for the next text typed, set by Bold and the like with no
    /// selection.
    pending: Props,
    composition: String,
    width: i64,
    height: i64,
    scroll: i64,
    menu: Option<usize>,
    hover: Option<MenuHit>,
    panel: Option<Panel>,
    dragging: bool,
    message: String,
}

/// Sizes for Bigger and Smaller, in points.
const SIZES: &[i64] = &[8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72];
/// Space between pages, and around them.
const GAP: i64 = 16 * PIXEL;

fn chars(text: &str) -> u32 {
    text.chars().count() as u32
}

/// The byte offset of character `offset` of `text`.
fn byte(text: &str, offset: u32) -> usize {
    text.char_indices()
        .nth(offset as usize)
        .map_or(text.len(), |(i, _)| i)
}

/// The character offset of byte `offset` of `text`.
fn char_at(text: &str, offset: usize) -> u32 {
    chars(&text[..offset.min(text.len())])
}

impl Editor {
    /// A new, empty document, made at once as "Untitled.xw" (or
    /// "Untitled 2.xw" and so on) in the documents folder.
    pub fn create(res: Arc<Resources>) -> Result<Editor, EditorError> {
        std::fs::create_dir_all(&res.documents).map_err(|source| EditorError::Io {
            path: res.documents.clone(),
            source,
        })?;
        let mut n = 1;
        let path = loop {
            let name = if n == 1 {
                "Untitled.xw".to_string()
            } else {
                format!("Untitled {n}.xw")
            };
            let path = res.documents.join(name);
            if !path.exists() {
                break path;
            }
            n += 1;
        };
        let mut document = Document::new();
        let first = document.paragraphs()[0];
        document
            .apply(&Op::SetParaProp {
                paragraph: first,
                key: keys::STYLE.to_string(),
                value: Some(Value::Text(res.sheet.paragraph_style(None).name.clone())),
            })
            .map_err(|e| EditorError::Store(store::Error::Damaged(e.to_string())))?;
        let store = Store::create(&path, &document, "story")?;
        Ok(Editor::with(res, path, store, document, History::new()))
    }

    /// The document at `path`.
    pub fn open(res: Arc<Resources>, path: &Path) -> Result<Editor, EditorError> {
        let opened = Store::open(path)?;
        let mut editor = Editor::with(
            res,
            path.to_path_buf(),
            opened.store,
            opened.document,
            opened.history,
        );
        if let Some(schema) = editor.store.read_only() {
            editor.message = format!("Read-only: made by a newer xWorks (schema {schema})");
        }
        Ok(editor)
    }

    fn with(
        res: Arc<Resources>,
        path: PathBuf,
        store: Store,
        document: Document,
        history: History,
    ) -> Editor {
        let first = document.paragraphs()[0];
        let start = Position {
            paragraph: first,
            offset: 0,
            upstream: false,
        };
        let mut editor = Editor {
            res,
            path,
            store,
            laid: Laid {
                document: Document::new(),
                paragraphs: Vec::new(),
                index: HashMap::new(),
                pages: Vec::new(),
                places: Vec::new(),
                words: Vec::new(),
            },
            document,
            history,
            anchor: start,
            focus: start,
            goal: None,
            typing: None,
            pending: Props::new(),
            composition: String::new(),
            width: 800 * PIXEL,
            height: 600 * PIXEL,
            scroll: 0,
            menu: None,
            hover: None,
            panel: None,
            dragging: false,
            message: String::new(),
        };
        editor.relayout(true);
        editor
    }

    /// The document's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The window's title: the document's name.
    pub fn title(&self) -> String {
        self.path.file_stem().map_or_else(
            || "Untitled".to_string(),
            |s| s.to_string_lossy().into_owned(),
        )
    }

    /// The document.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The selection: where it started and where it ends, which is where
    /// the caret is.
    pub fn selection(&self) -> (Position, Position) {
        (self.anchor, self.focus)
    }

    /// The document's text, paragraphs ending in newlines, and the
    /// selection in it as character offsets: for accessibility.
    pub fn text(&self) -> (String, usize, usize) {
        let mut out = String::new();
        let (mut anchor, mut focus) = (0, 0);
        for &id in self.document.paragraphs() {
            let text = &self.paragraph(id).text;
            let base = out.chars().count();
            if self.anchor.paragraph == id {
                anchor = base + self.anchor.offset as usize;
            }
            if self.focus.paragraph == id {
                focus = base + self.focus.offset as usize;
            }
            out.push_str(text);
            out.push('\n');
        }
        (out, anchor, focus)
    }

    fn paragraph(&self, id: ObjectId) -> &Paragraph {
        self.document
            .paragraph(id)
            .expect("a paragraph of the document")
    }

    fn index_of(&self, id: ObjectId) -> usize {
        self.document
            .paragraphs()
            .iter()
            .position(|&p| p == id)
            .unwrap_or(0)
    }

    // ---- layout

    /// Lays out what changed, and pours the story into pages again. With
    /// `all`, lays out every paragraph afresh.
    fn relayout(&mut self, all: bool) {
        let width = self.res.page.body_width();
        let mut old: HashMap<ObjectId, (ParagraphLayout, usize)> =
            std::mem::take(&mut self.laid.paragraphs)
                .into_iter()
                .zip(std::mem::take(&mut self.laid.words))
                .map(|(p, words)| (p.id, (p, words)))
                .collect();
        let composing = !self.composition.is_empty();
        let composed = composing.then(|| self.composed());
        // What each paragraph keeps: its layout, if it's unchanged, and its
        // word count, which is the document's, not the composition's.
        let ids = self.document.paragraphs();
        let kept: Vec<(Option<ParagraphLayout>, Option<usize>)> = ids
            .iter()
            .map(|&id| {
                let same = !all
                    && match (self.laid.document.object(id), self.document.object(id)) {
                        (Some(a), Some(b)) => std::ptr::eq(a as *const Object, b as *const Object),
                        _ => false,
                    };
                let reuse = same && !(composing && id == self.focus.paragraph);
                match old.remove(&id) {
                    Some((layout, count)) if same => (reuse.then_some(layout), Some(count)),
                    _ => (None, None),
                }
            })
            .collect();
        // The rest laid out and counted afresh: on the worker pool when
        // there are several, as when a document opens, and here when
        // there's one, as when typing, which waking the pool would slow.
        let todo: Vec<(ObjectId, Option<usize>)> = ids
            .iter()
            .zip(&kept)
            .filter(|(_, (layout, _))| layout.is_none())
            .map(|(&id, &(_, count))| (id, count))
            .collect();
        let (document, res, focus) = (&self.document, &self.res, self.focus.paragraph);
        let lay_out = |&(id, count): &(ObjectId, Option<usize>)| {
            let source = match &composed {
                Some(c) if id == focus => c,
                _ => document,
            };
            let layout = story::lay_out_paragraph(source, id, &res.sheet, res.fonts(), width);
            let count = count.unwrap_or_else(|| {
                let p = document.paragraph(id).expect("a paragraph of the document");
                p.text.split_whitespace().count()
            });
            (layout, count)
        };
        let mut fresh: Vec<(ParagraphLayout, usize)> = if todo.len() > 1 {
            todo.par_iter().map(lay_out).collect()
        } else {
            todo.iter().map(lay_out).collect()
        };
        // A character the fonts lack brings in those kept until text needs
        // them, such as the CJK faces, and its paragraphs are laid out again.
        if fresh.iter().any(|(p, _)| p.layout.missing) {
            match res.load_on_demand() {
                Ok(true) => {
                    for (todo, fresh) in todo.iter().zip(&mut fresh) {
                        if fresh.0.layout.missing {
                            *fresh = lay_out(todo);
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => self.message = format!("Couldn't load fonts: {e}"),
            }
        }
        let mut fresh = fresh.into_iter();
        let (paragraphs, words): (Vec<ParagraphLayout>, Vec<usize>) = kept
            .into_iter()
            .map(|kept| match kept {
                (Some(layout), Some(count)) => (layout, count),
                _ => fresh
                    .next()
                    .expect("a fresh layout for each paragraph not kept"),
            })
            .unzip();
        let pages = canvas::paginate(self.res.page.body_height(), &canvas::blocks(&paragraphs));
        let mut places: Vec<Vec<(usize, i64)>> = paragraphs
            .iter()
            .map(|p| vec![(0, 0); p.layout.lines.len()])
            .collect();
        for (n, pieces) in pages.iter().enumerate() {
            for piece in pieces {
                let lines = &paragraphs[piece.block].layout.lines;
                let top = lines[piece.lines.start].top;
                for l in piece.lines.clone() {
                    places[piece.block][l] = (n, piece.y + lines[l].top - top);
                }
            }
        }
        self.laid = Laid {
            document: self.document.clone(),
            index: paragraphs
                .iter()
                .enumerate()
                .map(|(i, p)| (p.id, i))
                .collect(),
            paragraphs,
            pages,
            places,
            words,
        };
    }

    /// The document with the composition typed in at the caret, for showing
    /// it before it's committed.
    fn composed(&self) -> Document {
        let mut composed = self.document.clone();
        let at = self.ordered().0;
        let props = self.typing_props(at);
        let _ = composed.apply(&Op::InsertText {
            paragraph: at.paragraph,
            offset: at.offset,
            text: self.composition.clone(),
            runs: vec![Run::new(chars(&self.composition), props)],
        });
        composed
    }

    fn layout_of(&self, id: ObjectId) -> &ParagraphLayout {
        &self.laid.paragraphs[self.laid.index[&id]]
    }

    // ---- geometry: positions on pages, in the window

    /// The top of the area pages show in.
    fn top(&self) -> i64 {
        MenuBar::height(&self.res.theme)
    }

    /// The height of the area pages show in.
    fn view_height(&self) -> i64 {
        (self.height - self.top() - self.res.theme.bar_height).max(PIXEL)
    }

    /// Page `n`'s top left, in document coordinates.
    fn page_origin(&self, n: usize) -> (i64, i64) {
        let page = &self.res.page;
        let x = ((self.width - page.width) / 2).max(GAP);
        (x, GAP + n as i64 * (page.height + GAP))
    }

    fn document_height(&self) -> i64 {
        GAP + self.laid.pages.len() as i64 * (self.res.page.height + GAP)
    }

    /// Where line `line` of paragraph `k` starts, in window coordinates:
    /// its left and top.
    fn line_origin(&self, k: usize, line: usize) -> (i64, i64) {
        let (n, y) = self.laid.places[k][line];
        let (px, py) = self.page_origin(n);
        let page = &self.res.page;
        let top = self.laid.paragraphs[k].layout.lines[line].top;
        (
            px + page.left,
            self.top() - self.scroll + py + page.top + y - top,
        )
    }

    fn cursor(&self, p: Position) -> (usize, Cursor) {
        let k = self.laid.index[&p.paragraph];
        let text = &self.paragraph(p.paragraph).text;
        let offset = byte(text, p.offset);
        let affinity = if p.upstream {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        (k, Cursor { offset, affinity })
    }

    fn position(&self, k: usize, cursor: Cursor) -> Position {
        let id = self.laid.paragraphs[k].id;
        let text = &self.paragraph(id).text;
        Position {
            paragraph: id,
            offset: char_at(text, cursor.offset),
            upstream: cursor.affinity == Affinity::Upstream,
        }
    }

    /// The caret's rectangle in the window: its x, top and bottom.
    fn caret_rect(&self) -> (i64, i64, i64) {
        let (k, cursor) = if self.composition.is_empty() {
            self.cursor(self.focus)
        } else {
            // After the composition, which is laid out in the text but not
            // in the document yet.
            let (k, mut cursor) = self.cursor(self.ordered().0);
            cursor.offset += self.composition.len();
            cursor.affinity = Affinity::Upstream;
            (k, cursor)
        };
        let layout = &self.laid.paragraphs[k].layout;
        let caret = layout.caret(cursor);
        let (x, y) = self.line_origin(k, caret.line);
        (x + caret.x, y + caret.top, y + caret.bottom)
    }

    /// The position at (`x`, `y`) in the window.
    fn hit(&self, x: i64, y: i64) -> Position {
        let page = &self.res.page;
        let doc_y = y - self.top() + self.scroll;
        let stride = page.height + GAP;
        let last = self.laid.pages.len().saturating_sub(1);
        let n = (((doc_y - GAP).max(0)) / stride).min(last as i64) as usize;
        let (px, py) = self.page_origin(n);
        let body_y = doc_y - py - page.top;
        let body_x = x - px - page.left;
        // The line nearest that height on that page.
        let mut best: Option<(i64, usize, usize)> = None;
        for piece in &self.laid.pages[n] {
            let lines = &self.laid.paragraphs[piece.block].layout.lines;
            let top = lines[piece.lines.start].top;
            for l in piece.lines.clone() {
                let line_top = piece.y + lines[l].top - top;
                let bottom = line_top + lines[l].height;
                let distance = if body_y < line_top {
                    line_top - body_y
                } else if body_y >= bottom {
                    body_y - bottom + 1
                } else {
                    0
                };
                if best.is_none_or(|(d, _, _)| distance < d) {
                    best = Some((distance, piece.block, l));
                }
            }
        }
        let (k, l) = best.map_or((0, 0), |(_, k, l)| (k, l));
        let cursor = self.laid.paragraphs[k].layout.hit_line(l, body_x);
        self.position(k, cursor)
    }

    /// Scrolls so the caret is in view.
    fn reveal(&mut self) {
        let (_, top, bottom) = self.caret_rect();
        let (view_top, view_bottom) = (self.top(), self.top() + self.view_height());
        if top < view_top {
            self.scroll -= view_top - top + GAP;
        } else if bottom > view_bottom {
            self.scroll += bottom - view_bottom + GAP;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let most = (self.document_height() - self.view_height()).max(0);
        self.scroll = self.scroll.clamp(0, most);
    }

    // ---- the selection

    fn order(&self, p: Position) -> (usize, u32) {
        (self.index_of(p.paragraph), p.offset)
    }

    /// The selection's start and end, in the text's order.
    fn ordered(&self) -> (Position, Position) {
        if self.order(self.anchor) <= self.order(self.focus) {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    fn collapsed(&self) -> bool {
        self.order(self.anchor) == self.order(self.focus)
    }

    /// Moves the caret to `p`, extending the selection with `extend`.
    fn go(&mut self, p: Position, extend: bool) {
        self.focus = p;
        if !extend {
            self.anchor = p;
        }
        self.typing = None;
        self.pending.clear();
    }

    /// The selected text, paragraphs ending in newlines but the last.
    fn selected_text(&self) -> String {
        let (start, end) = self.ordered();
        let ids = self.document.paragraphs();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let mut out = String::new();
        for (i, &id) in ids.iter().enumerate().take(b + 1).skip(a) {
            let text = &self.paragraph(id).text;
            let from = if i == a { byte(text, start.offset) } else { 0 };
            let to = if i == b {
                byte(text, end.offset)
            } else {
                text.len()
            };
            out.push_str(&text[from..to.max(from)]);
            if i < b {
                out.push('\n');
            }
        }
        out
    }

    // ---- moving

    fn step(&self, p: Position, right: bool) -> Option<Position> {
        let (k, cursor) = self.cursor(p);
        let layout = &self.laid.paragraphs[k].layout;
        let moved = if right {
            layout.right(cursor)
        } else {
            layout.left(cursor)
        };
        if let Some(c) = moved {
            return Some(self.position(k, c));
        }
        // Past the paragraph's edge: on to the next paragraph in its
        // reading direction, or back to the one before.
        let forward = right == (layout.direction() == text::Direction::Ltr);
        let ids = self.document.paragraphs();
        if forward {
            let next = *ids.get(k + 1)?;
            Some(Position {
                paragraph: next,
                offset: 0,
                upstream: false,
            })
        } else {
            let previous = *ids.get(k.checked_sub(1)?)?;
            Some(Position {
                paragraph: previous,
                offset: self.paragraph(previous).len(),
                upstream: true,
            })
        }
    }

    fn vertical(&self, p: Position, down: bool, goal: i64) -> Option<Position> {
        let (k, cursor) = self.cursor(p);
        let layout = &self.laid.paragraphs[k].layout;
        let line = layout.line_of(cursor);
        let (k, line) = if down {
            if line + 1 < layout.lines.len() {
                (k, line + 1)
            } else if k + 1 < self.laid.paragraphs.len() {
                (k + 1, 0)
            } else {
                return None;
            }
        } else if line > 0 {
            (k, line - 1)
        } else if k > 0 {
            (k - 1, self.laid.paragraphs[k - 1].layout.lines.len() - 1)
        } else {
            return None;
        };
        let cursor = self.laid.paragraphs[k].layout.hit_line(line, goal);
        Some(self.position(k, cursor))
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        if let Some(command) = self.res.commands.for_key(key, modifiers) {
            let id = command.id.clone();
            return self.command(&id);
        }
        if self.menu.is_some() {
            if key == Key::Escape {
                self.menu = None;
            }
            return Vec::new();
        }
        if self.panel.is_some() {
            return self.panel_key(key);
        }
        let extend = modifiers.shift;
        let goal = self.goal.take();
        match key {
            Key::Left | Key::Right => {
                let right = key == Key::Right;
                let target = if !extend && !self.collapsed() {
                    let (start, end) = self.ordered();
                    let ltr = self.layout_of(self.focus.paragraph).layout.direction()
                        == text::Direction::Ltr;
                    if right == ltr { end } else { start }
                } else {
                    self.step(self.focus, right).unwrap_or(self.focus)
                };
                self.go(target, extend);
            }
            Key::Up | Key::Down => {
                let (x, _, _) = self.caret_rect();
                let goal = goal.unwrap_or_else(|| {
                    let (k, cursor) = self.cursor(self.focus);
                    let line = self.laid.paragraphs[k].layout.line_of(cursor);
                    x - self.line_origin(k, line).0
                });
                let target = self.vertical(self.focus, key == Key::Down, goal);
                let target = target.unwrap_or_else(|| self.document_edge(key == Key::Down));
                self.go(target, extend);
                self.goal = Some(goal);
            }
            Key::Home | Key::End if modifiers.primary => {
                let target = self.document_edge(key == Key::End);
                self.go(target, extend);
            }
            Key::Home | Key::End => {
                let (k, cursor) = self.cursor(self.focus);
                let layout = &self.laid.paragraphs[k].layout;
                let line = layout.line_of(cursor);
                let c = if key == Key::Home {
                    layout.line_start(line)
                } else {
                    layout.line_end(line)
                };
                let target = self.position(k, c);
                self.go(target, extend);
            }
            Key::PageUp | Key::PageDown => {
                let (x, top, _) = self.caret_rect();
                let by = self.view_height() - GAP * 2;
                let y = if key == Key::PageDown {
                    top + by
                } else {
                    top - by
                };
                self.scroll += if key == Key::PageDown { by } else { -by };
                self.clamp_scroll();
                let target = self.hit(x, y.clamp(self.top(), self.top() + self.view_height() - 1));
                self.go(target, extend);
            }
            Key::Backspace | Key::Delete => self.delete(key == Key::Delete),
            Key::Enter if modifiers.shift => self.insert_text("\u{2028}"),
            Key::Enter => self.split(),
            Key::Escape => {
                let focus = self.focus;
                self.go(focus, false);
            }
            Key::Tab | Key::Char(_) => {}
        }
        self.reveal();
        Vec::new()
    }

    fn document_edge(&self, end: bool) -> Position {
        let ids = self.document.paragraphs();
        if end {
            let last = *ids.last().expect("a paragraph");
            Position {
                paragraph: last,
                offset: self.paragraph(last).len(),
                upstream: true,
            }
        } else {
            Position {
                paragraph: ids[0],
                offset: 0,
                upstream: false,
            }
        }
    }

    // ---- editing

    /// Applies `ops` as a transaction named `name`, records it in the file
    /// (continuing the last one with `continuing`), and lays out what
    /// changed. Returns whether it applied.
    fn commit(&mut self, name: &str, ops: Vec<Op>, continuing: bool) -> bool {
        if ops.is_empty() {
            return false;
        }
        if let Some(schema) = self.store.read_only() {
            self.message = format!("Read-only: made by a newer xWorks (schema {schema})");
            return false;
        }
        let t = match self.document.transact(name, ops) {
            Ok(t) => t,
            Err(e) => {
                self.message = format!("That didn't work: {e}");
                return false;
            }
        };
        let entry = if continuing {
            Entry::Continue(&t)
        } else {
            Entry::Commit(&t)
        };
        match self.store.append(entry, &self.document) {
            Ok(_) => self.message.clear(),
            Err(e) => self.message = format!("Couldn't save: {e}"),
        }
        if !(continuing && self.history.continue_last(t.clone())) {
            self.history.push(t);
        }
        self.relayout(false);
        true
    }

    /// The operations that delete the selection, from a document where it
    /// stands, in order; and the selection's start, where the caret goes.
    fn deletion(&self) -> (Vec<Op>, Position) {
        let (start, end) = self.ordered();
        let mut start = start;
        start.upstream = false;
        if self.collapsed() {
            return (Vec::new(), start);
        }
        let ids = self.document.paragraphs();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let mut ops = Vec::new();
        if a == b {
            ops.push(Op::DeleteText {
                paragraph: start.paragraph,
                offset: start.offset,
                len: end.offset - start.offset,
            });
            return (ops, start);
        }
        // The end of the first paragraph, all of those between, and the
        // start of the last; then join them, the first taking the last's
        // text.
        let first_len = self.paragraph(start.paragraph).len();
        ops.push(Op::DeleteText {
            paragraph: start.paragraph,
            offset: start.offset,
            len: first_len - start.offset,
        });
        for &id in &ids[a + 1..b] {
            ops.push(Op::DeleteText {
                paragraph: id,
                offset: 0,
                len: self.paragraph(id).len(),
            });
        }
        ops.push(Op::DeleteText {
            paragraph: end.paragraph,
            offset: 0,
            len: end.offset,
        });
        for &id in &ids[a + 1..=b] {
            ops.push(Op::JoinParagraphs {
                paragraph: start.paragraph,
                offset: start.offset,
                next: id,
            });
        }
        (ops, start)
    }

    /// The properties text typed at `at` takes: the character's before it,
    /// or after it at a paragraph's start, with any pending ones over them.
    fn typing_props(&self, at: Position) -> Props {
        let paragraph = self.paragraph(at.paragraph);
        let probe = if at.offset > 0 { at.offset - 1 } else { 0 };
        let mut pos = 0;
        let mut props = Props::new();
        for run in &paragraph.runs {
            if probe < pos + run.len {
                props = (*run.props).clone();
                break;
            }
            pos += run.len;
        }
        for (k, v) in &self.pending {
            match v {
                Value::Bool(false) => {
                    props.remove(k);
                }
                v => {
                    props.insert(k.clone(), v.clone());
                }
            }
        }
        props
    }

    fn insert_text(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|&c| !c.is_control() || c == '\u{2028}')
            .collect();
        if text.is_empty() {
            return;
        }
        let continuing = self.collapsed() && self.typing == Some(self.focus);
        let (mut ops, at) = self.deletion();
        let props = self.typing_props(at);
        let len = chars(&text);
        ops.push(Op::InsertText {
            paragraph: at.paragraph,
            offset: at.offset,
            text,
            runs: vec![Run::new(len, props)],
        });
        let pending = std::mem::take(&mut self.pending);
        if self.commit("Typing", ops, continuing) {
            let after = Position {
                paragraph: at.paragraph,
                offset: at.offset + len,
                upstream: true,
            };
            self.anchor = after;
            self.focus = after;
            self.typing = Some(after);
            self.pending = pending;
        }
    }

    fn split(&mut self) {
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return;
        }
        let paragraph = scratch.paragraph(at.paragraph).expect("a paragraph");
        let mut props = paragraph.props.clone();
        // After a heading's last character comes the default style.
        let style = match props.get(keys::STYLE) {
            Some(Value::Text(name)) => Some(name.as_str()),
            _ => None,
        };
        if at.offset == paragraph.len() && self.res.sheet.paragraph_style(style).heading.is_some() {
            props.insert(
                keys::STYLE.to_string(),
                Value::Text(self.res.sheet.paragraph_style(None).name.clone()),
            );
        }
        let new = self.document.new_id();
        ops.push(Op::SplitParagraph {
            paragraph: at.paragraph,
            offset: at.offset,
            new,
            new_props: props,
        });
        if self.commit("New Paragraph", ops, false) {
            let after = Position {
                paragraph: new,
                offset: 0,
                upstream: false,
            };
            self.go(after, false);
        }
    }

    fn delete(&mut self, forward: bool) {
        if !self.collapsed() {
            let (ops, at) = self.deletion();
            if self.commit("Delete", ops, false) {
                self.go(at, false);
            }
            return;
        }
        let at = self.focus;
        let text = self.paragraph(at.paragraph).text.clone();
        let boundaries = locale::graphemes(&text);
        let here = byte(&text, at.offset);
        let ids = self.document.paragraphs().to_vec();
        let k = self.index_of(at.paragraph);
        let (ops, caret) = if forward {
            match boundaries.iter().copied().find(|&g| g > here) {
                Some(g) => (
                    vec![Op::DeleteText {
                        paragraph: at.paragraph,
                        offset: at.offset,
                        len: char_at(&text, g) - at.offset,
                    }],
                    at,
                ),
                None => match ids.get(k + 1) {
                    Some(&next) => (
                        vec![Op::JoinParagraphs {
                            paragraph: at.paragraph,
                            offset: at.offset,
                            next,
                        }],
                        at,
                    ),
                    None => return,
                },
            }
        } else {
            match boundaries.iter().rev().copied().find(|&g| g < here) {
                Some(g) => {
                    let from = char_at(&text, g);
                    (
                        vec![Op::DeleteText {
                            paragraph: at.paragraph,
                            offset: from,
                            len: at.offset - from,
                        }],
                        Position {
                            paragraph: at.paragraph,
                            offset: from,
                            upstream: from > 0,
                        },
                    )
                }
                None => match k.checked_sub(1).map(|i| ids[i]) {
                    Some(previous) => {
                        let len = self.paragraph(previous).len();
                        (
                            vec![Op::JoinParagraphs {
                                paragraph: previous,
                                offset: len,
                                next: at.paragraph,
                            }],
                            Position {
                                paragraph: previous,
                                offset: len,
                                upstream: true,
                            },
                        )
                    }
                    None => return,
                },
            }
        };
        if self.commit("Delete", ops, false) {
            self.go(caret, false);
        }
    }

    fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return;
        }
        let props = self.typing_props(at);
        let mut caret = at;
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                let new = scratch.new_id();
                let new_props = scratch
                    .paragraph(caret.paragraph)
                    .map(|p| p.props.clone())
                    .unwrap_or_default();
                let op = Op::SplitParagraph {
                    paragraph: caret.paragraph,
                    offset: caret.offset,
                    new,
                    new_props,
                };
                if scratch.apply(&op).is_err() {
                    return;
                }
                ops.push(op);
                caret = Position {
                    paragraph: new,
                    offset: 0,
                    upstream: false,
                };
            }
            let line: String = line.chars().filter(|c| !c.is_control()).collect();
            if line.is_empty() {
                continue;
            }
            let len = chars(&line);
            let op = Op::InsertText {
                paragraph: caret.paragraph,
                offset: caret.offset,
                text: line,
                runs: vec![Run::new(len, props.clone())],
            };
            if scratch.apply(&op).is_err() {
                return;
            }
            ops.push(op);
            caret.offset += len;
            caret.upstream = true;
        }
        if self.commit("Paste", ops, false) {
            self.go(caret, false);
        }
    }

    /// Sets character property `key` to `value` over the selection.
    fn set_char_prop(&mut self, name: &str, key: &str, value: Option<Value>) {
        let (start, end) = self.ordered();
        let ids = self.document.paragraphs().to_vec();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let mut ops = Vec::new();
        for (i, &id) in ids.iter().enumerate().take(b + 1).skip(a) {
            let len = self.paragraph(id).len();
            let from = if i == a { start.offset } else { 0 };
            let to = if i == b { end.offset } else { len };
            if to > from {
                ops.push(Op::SetCharProp {
                    paragraph: id,
                    offset: from,
                    len: to - from,
                    key: key.to_string(),
                    value: value.clone(),
                });
            }
        }
        self.commit(name, ops, false);
    }

    /// Whether every selected character has `key` set to true; at a caret,
    /// whether text typed there would.
    fn all(&self, key: &str) -> bool {
        if self.collapsed() {
            return self.typing_props(self.focus).get(key) == Some(&Value::Bool(true));
        }
        let (start, end) = self.ordered();
        let ids = self.document.paragraphs();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        ids.iter().enumerate().take(b + 1).skip(a).all(|(i, &id)| {
            let p = self.paragraph(id);
            let from = if i == a { start.offset } else { 0 };
            let to = if i == b { end.offset } else { p.len() };
            let mut pos = 0;
            p.runs.iter().all(|run| {
                let (s, e) = (pos, pos + run.len);
                pos = e;
                e <= from || s >= to || run.props.get(key) == Some(&Value::Bool(true))
            })
        })
    }

    fn toggle(&mut self, name: &str, key: &str) {
        let on = !self.all(key);
        if self.collapsed() {
            self.pending.insert(key.to_string(), Value::Bool(on));
            self.typing = None;
        } else {
            self.set_char_prop(name, key, on.then_some(Value::Bool(true)));
        }
    }

    /// The font size at the selection's start, in layout units.
    fn size(&self) -> i64 {
        let at = self.ordered().0;
        match self.typing_props(at).get(keys::SIZE) {
            Some(Value::Int(size)) if *size > 0 => *size,
            _ => {
                let p = self.paragraph(at.paragraph);
                let style = match p.props.get(keys::STYLE) {
                    Some(Value::Text(name)) => Some(name.as_str()),
                    _ => None,
                };
                self.res.sheet.paragraph_style(style).size.units()
            }
        }
    }

    fn set_size(&mut self, size: i64) {
        if self.collapsed() {
            self.pending
                .insert(keys::SIZE.to_string(), Value::Int(size));
            self.typing = None;
        } else {
            self.set_char_prop("Size", keys::SIZE, Some(Value::Int(size)));
        }
    }

    fn set_style(&mut self, style: &str) {
        let (start, end) = self.ordered();
        let ids = self.document.paragraphs().to_vec();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let ops = ids[a..=b]
            .iter()
            .map(|&id| Op::SetParaProp {
                paragraph: id,
                key: keys::STYLE.to_string(),
                value: Some(Value::Text(style.to_string())),
            })
            .collect();
        self.commit(style, ops, false);
    }

    /// Where the caret goes after `ops` were applied: the end of the last
    /// change.
    fn after(&self, ops: &[Op]) -> Option<Position> {
        let at = |paragraph: ObjectId, offset: u32| {
            self.document.paragraph(paragraph).map(|p| Position {
                paragraph,
                offset: offset.min(p.len()),
                upstream: true,
            })
        };
        match ops.last()? {
            Op::InsertText {
                paragraph,
                offset,
                text,
                ..
            } => at(*paragraph, offset + chars(text)),
            Op::DeleteText {
                paragraph, offset, ..
            }
            | Op::JoinParagraphs {
                paragraph, offset, ..
            } => at(*paragraph, *offset),
            Op::SplitParagraph { new, .. } => at(*new, 0),
            Op::SetCharProp {
                paragraph,
                offset,
                len,
                ..
            } => at(*paragraph, offset + len),
            Op::SetParaProp { paragraph, .. } => at(*paragraph, 0),
        }
    }

    fn undo(&mut self, redo: bool) {
        if let Some(schema) = self.store.read_only() {
            self.message = format!("Read-only: made by a newer xWorks (schema {schema})");
            return;
        }
        let done = if redo {
            self.history.redo(&mut self.document)
        } else {
            self.history.undo(&mut self.document)
        };
        let t: Transaction = match done {
            Ok(Some(t)) => t.clone(),
            Ok(None) => return,
            Err(e) => {
                self.message = format!("That didn't work: {e}");
                return;
            }
        };
        let entry = if redo {
            Entry::Redo(&t)
        } else {
            Entry::Undo(&t)
        };
        if let Err(e) = self.store.append(entry, &self.document) {
            self.message = format!("Couldn't save: {e}");
        }
        self.relayout(false);
        let ops = if redo { &t.ops } else { &t.inverse };
        let caret = self.after(ops).unwrap_or_else(|| self.document_edge(false));
        self.go(caret, false);
        self.message = format!("{} {}", if redo { "Redid" } else { "Undid" }, t.name);
    }

    // ---- commands

    /// Whether command `id` can run now, and whether it's on.
    pub fn state(&self, id: &str) -> CommandState {
        let writable = self.store.read_only().is_none();
        let enabled = match id {
            "undo" => writable && self.history.undo_name().is_some(),
            "redo" => writable && self.history.redo_name().is_some(),
            "cut" | "copy" => !self.collapsed(),
            "save-version" | "paste" | "bold" | "italic" => writable,
            _ => true,
        };
        let checked = match id {
            "bold" => self.all(keys::BOLD),
            "italic" => self.all(keys::ITALIC),
            _ => id
                .strip_prefix("style-")
                .is_some_and(|style| self.style_id() == style),
        };
        CommandState { enabled, checked }
    }

    /// The caret's paragraph's style, as a command ID ends: "body",
    /// "heading-1".
    fn style_id(&self) -> String {
        let p = self.paragraph(self.focus.paragraph);
        let name = match p.props.get(keys::STYLE) {
            Some(Value::Text(name)) => Some(name.as_str()),
            _ => None,
        };
        self.res
            .sheet
            .paragraph_style(name)
            .name
            .to_lowercase()
            .replace(' ', "-")
    }

    fn command(&mut self, id: &str) -> Vec<Effect> {
        self.menu = None;
        if !self.state(id).enabled {
            return Vec::new();
        }
        let mut effects = Vec::new();
        match id {
            "new" => effects.push(Effect::Open(None)),
            "open" => self.open_panel(),
            "quit" => effects.push(Effect::Quit),
            "save-version" => {
                let n = self.store.versions().map_or(0, |v| v.len()) + 1;
                let name = format!("Version {n}");
                self.message = match self.store.name_version(&name) {
                    Ok(()) => format!("Saved {name}"),
                    Err(e) => format!("Couldn't save a version: {e}"),
                };
            }
            "export-pdf" => {
                let path = self.path.with_extension("pdf");
                match self.export(&path) {
                    Ok(()) => {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        self.message = format!("Exported {name}");
                        effects.push(Effect::Reveal(path));
                    }
                    Err(e) => self.message = format!("Couldn't export: {e}"),
                }
            }
            "print" => {
                let path = std::env::temp_dir().join(format!("{} (print).pdf", self.title()));
                match self.export(&path) {
                    Ok(()) => effects.push(Effect::OpenExternally(path)),
                    Err(e) => self.message = format!("Couldn't print: {e}"),
                }
            }
            "undo" => self.undo(false),
            "redo" => self.undo(true),
            "copy" => effects.push(Effect::Copy(self.selected_text())),
            "cut" => {
                effects.push(Effect::Copy(self.selected_text()));
                let (ops, at) = self.deletion();
                if self.commit("Cut", ops, false) {
                    self.go(at, false);
                }
            }
            "paste" => effects.push(Effect::Paste),
            "select-all" => {
                self.anchor = self.document_edge(false);
                self.focus = self.document_edge(true);
                self.typing = None;
            }
            "bold" => self.toggle("Bold", keys::BOLD),
            "italic" => self.toggle("Italic", keys::ITALIC),
            "size-bigger" | "size-smaller" => {
                let pt = self.size() / POINT;
                let next = if id == "size-bigger" {
                    SIZES.iter().copied().find(|&s| s > pt).unwrap_or(pt)
                } else {
                    SIZES.iter().rev().copied().find(|&s| s < pt).unwrap_or(pt)
                };
                self.set_size(next * POINT);
            }
            _ => {
                if let Some(pt) = id.strip_prefix("size-").and_then(|n| n.parse::<i64>().ok()) {
                    self.set_size(pt * POINT);
                } else if let Some(style) = id.strip_prefix("style-") {
                    let name = self
                        .res
                        .sheet
                        .paragraph
                        .iter()
                        .find(|s| s.name.to_lowercase().replace(' ', "-") == style)
                        .map(|s| s.name.clone());
                    if let Some(name) = name {
                        self.set_style(&name);
                    }
                }
            }
        }
        self.reveal();
        effects
    }

    /// The document's pages and structure, as drawn and printed.
    fn pages(&self) -> canvas::Pages {
        canvas::Pages {
            pages: canvas::draw(
                &self.res.page,
                &self.laid.paragraphs,
                &self.laid.pages,
                self.res.fonts(),
            ),
            structure: canvas::structure(&self.laid.paragraphs),
        }
    }

    fn export(&self, path: &Path) -> Result<(), String> {
        let pages = self.pages();
        let bytes = render_pdf::pdf(&pages.pages, &pages.structure, Some(&self.title()))
            .map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    // ---- the Open panel

    fn open_panel(&mut self) {
        let mut found: Vec<(std::time::SystemTime, PathBuf)> =
            std::fs::read_dir(&self.res.documents)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|e| e == "xw") && p != &self.path)
                        .map(|p| {
                            let modified = std::fs::metadata(&p)
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::UNIX_EPOCH);
                            (modified, p)
                        })
                        .collect()
                })
                .unwrap_or_default();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        self.panel = Some(Panel {
            paths: found.into_iter().map(|(_, p)| p).collect(),
            selected: 0,
        });
    }

    fn list(&self) -> Option<List> {
        let panel = self.panel.as_ref()?;
        let names = panel
            .paths
            .iter()
            .map(|p| {
                p.file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
            })
            .collect();
        Some(List::new(
            &self.res.theme,
            "Open a document",
            names,
            self.width,
            self.height,
        ))
    }

    fn panel_key(&mut self, key: Key) -> Vec<Effect> {
        let Some(panel) = &mut self.panel else {
            return Vec::new();
        };
        match key {
            Key::Up => panel.selected = panel.selected.saturating_sub(1),
            Key::Down => {
                panel.selected = (panel.selected + 1).min(panel.paths.len().saturating_sub(1))
            }
            Key::Enter => {
                let chosen = panel.paths.get(panel.selected).cloned();
                self.panel = None;
                if let Some(path) = chosen {
                    return vec![Effect::Open(Some(path))];
                }
            }
            Key::Escape => self.panel = None,
            _ => {}
        }
        Vec::new()
    }

    // ---- events

    /// Handles `event`, returning what the shell should do.
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let px = |v: f64| (v * PIXEL as f64).round() as i64;
        match event {
            Event::Key { key, modifiers } => return self.key(key, modifiers),
            Event::Text(text) => {
                if self.panel.is_none() && self.menu.is_none() {
                    self.composition.clear();
                    self.insert_text(&text);
                    self.reveal();
                }
            }
            Event::Preedit(text) => {
                self.composition = text;
                self.relayout(false);
                self.reveal();
            }
            Event::Command(id) => return self.command(&id),
            Event::Pasted(text) => {
                self.paste(&text);
                self.reveal();
            }
            Event::Resize { width, height } => {
                self.width = px(width).max(PIXEL);
                self.height = px(height).max(PIXEL);
                self.clamp_scroll();
            }
            Event::Scroll(dy) => {
                self.scroll += px(dy);
                self.clamp_scroll();
            }
            Event::Press { x, y, shift } => return self.press(px(x), px(y), shift),
            Event::Move { x, y } => self.pointer_move(px(x), px(y)),
            Event::Release => self.dragging = false,
        }
        Vec::new()
    }

    fn menu_bar(&self) -> MenuBar {
        let bar = self
            .res
            .commands
            .menu_bar(self.res.mac, |id| self.state(id));
        MenuBar::new(
            self.res.fonts(),
            &self.res.theme,
            bar,
            self.width,
            self.menu,
        )
    }

    fn press(&mut self, x: i64, y: i64, shift: bool) -> Vec<Effect> {
        let theme = self.res.theme.clone();
        if let Some(list) = self.list() {
            match list.hit(&theme, x, y) {
                Some(Some(row)) => {
                    let chosen = self.panel.as_ref().and_then(|p| p.paths.get(row).cloned());
                    self.panel = None;
                    return chosen
                        .map(|p| vec![Effect::Open(Some(p))])
                        .unwrap_or_default();
                }
                Some(None) => return Vec::new(),
                None => {
                    self.panel = None;
                    return Vec::new();
                }
            }
        }
        match self.menu_bar().hit(&theme, x, y) {
            Some(MenuHit::Title(i)) => {
                self.menu = if self.menu == Some(i) { None } else { Some(i) };
                return Vec::new();
            }
            Some(MenuHit::Command(id)) => return self.command(&id),
            Some(MenuHit::Menu) => return Vec::new(),
            None if self.menu.is_some() => {
                self.menu = None;
                return Vec::new();
            }
            None => {}
        }
        if y < self.top() || y >= self.top() + self.view_height() {
            return Vec::new();
        }
        let target = self.hit(x, y);
        self.go(target, shift);
        self.dragging = true;
        Vec::new()
    }

    fn pointer_move(&mut self, x: i64, y: i64) {
        let theme = self.res.theme.clone();
        let bar = self.menu_bar();
        self.hover = bar.hit(&theme, x, y);
        if let (Some(_), Some(MenuHit::Title(i))) = (self.menu, &self.hover) {
            self.menu = Some(*i);
        }
        if self.dragging && self.menu.is_none() && self.panel.is_none() {
            let target = self.hit(x, y);
            self.go(target, true);
        }
        if let (Some(list), Some(panel)) = (self.list(), &mut self.panel)
            && let Some(Some(row)) = list.hit(&theme, x, y)
        {
            panel.selected = row;
        }
    }

    // ---- drawing

    /// The window as a display list, in layout units from its top left,
    /// with its size: 9 525 to a logical pixel.
    pub fn view(&self) -> scene::Page {
        crate::view::draw(self)
    }

    /// Where an input method's candidate window should go: below the caret,
    /// as x, y, width and height in logical pixels.
    pub fn ime_area(&self) -> (f64, f64, f64, f64) {
        let (x, top, bottom) = self.caret_rect();
        let px = |v: i64| v as f64 / PIXEL as f64;
        (px(x), px(top), 1.0, px(bottom - top))
    }
}

impl Editor {
    pub(crate) fn resources(&self) -> &Resources {
        &self.res
    }

    pub(crate) fn view_parts(&self) -> ViewParts<'_> {
        let (start, end) = self.ordered();
        ViewParts {
            width: self.width,
            height: self.height,
            top: self.top(),
            view_height: self.view_height(),
            pages: self.laid.pages.len(),
            paragraphs: &self.laid.paragraphs,
            pieces: &self.laid.pages,
            selection: (!self.collapsed()).then(|| {
                let a = self.cursor(start);
                let b = self.cursor(end);
                (a.0, a.1.offset, b.0, b.1.offset)
            }),
            caret: (self.collapsed() && self.menu.is_none() && self.panel.is_none())
                .then(|| self.caret_rect()),
            composition: (!self.composition.is_empty()).then(|| {
                let (k, c) = self.cursor(start);
                (k, c.offset, c.offset + self.composition.len())
            }),
            menu: self.menu_bar(),
            hover: self.hover.clone(),
            list: self
                .list()
                .map(|l| (l, self.panel.as_ref().map_or(0, |p| p.selected))),
            status: self.status(),
            message: if self.message.is_empty() {
                "Saved".to_string()
            } else {
                self.message.clone()
            },
        }
    }

    pub(crate) fn origin_of_page(&self, n: usize) -> (i64, i64) {
        let (x, y) = self.page_origin(n);
        (x, self.top() - self.scroll + y)
    }

    pub(crate) fn line_origin_of(&self, k: usize, line: usize) -> (i64, i64) {
        self.line_origin(k, line)
    }

    /// What the status bar says: the page, the style and size at the
    /// caret, and the document's words.
    pub fn status(&self) -> String {
        let (k, cursor) = self.cursor(self.focus);
        let line = self.laid.paragraphs[k].layout.line_of(cursor);
        let page = self.laid.places[k][line].0 + 1;
        let words: usize = self.laid.words.iter().sum();
        let p = self.paragraph(self.focus.paragraph);
        let name = match p.props.get(keys::STYLE) {
            Some(Value::Text(name)) => Some(name.as_str()),
            _ => None,
        };
        let style = &self.res.sheet.paragraph_style(name).name;
        format!(
            "Page {page} of {}   ·   {style}   ·   {} pt   ·   {words} words",
            self.laid.pages.len(),
            self.size() as f64 / POINT as f64
        )
    }
}

/// Everything the view needs, gathered.
pub(crate) struct ViewParts<'a> {
    pub width: i64,
    pub height: i64,
    pub top: i64,
    pub view_height: i64,
    pub pages: usize,
    pub paragraphs: &'a [ParagraphLayout],
    pub pieces: &'a [Vec<Piece>],
    /// The selection, as (paragraph, byte) at its start and end.
    pub selection: Option<(usize, usize, usize, usize)>,
    pub caret: Option<(i64, i64, i64)>,
    /// The composition: its paragraph and byte range.
    pub composition: Option<(usize, usize, usize)>,
    pub menu: MenuBar,
    pub hover: Option<MenuHit>,
    pub list: Option<(List, usize)>,
    pub status: String,
    pub message: String,
}
