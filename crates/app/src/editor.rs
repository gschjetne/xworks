//! One document's story, edited: its file, its text and history, where the
//! caret is, and how it's shown, on pages or as a draft. It's the first
//! part behind the canvas contract (`part`): a [`crate::Window`] holds it
//! and draws the chrome around it. It takes [`Event`]s and returns
//! [`Effect`]s; [`Editor::view`] draws its area of the window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{Resources, blocks};
use canvas::Piece;
use chrome::{Arg, CommandState, Key, Modifiers};
use doc::{Document, History, Object, ObjectId, Op, Paragraph, Props, Run, Transaction, Value};
use rayon::prelude::*;
use store::{Entry, Store};
use story::{ParagraphLayout, StyleSheet, keys};
use text::units::{PIXEL, POINT};
use text::{Affinity, Cursor};

mod access;
mod clip;
mod file;
mod format;
mod links;
mod markdown;
mod part;
mod pictures;
mod recording;
mod saver;
mod styling;
mod units;
mod versions;

pub(crate) use clip::Fragment;
pub(crate) use markdown::is_markdown;
pub(crate) use part::view_look;
use units::Unit;

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
    /// window's top left.
    Press {
        /// Across.
        x: f64,
        /// Down.
        y: f64,
        /// Whether Shift is held, to extend the selection.
        shift: bool,
        /// Which press of a run of quick presses in one place it is, as
        /// the shell counts them: 1 for a click, 2 for a double click,
        /// which selects a word, and 3 for a triple click, which selects a
        /// sentence.
        count: u32,
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
    /// A command with its arguments, from the inspector, the ruler, a
    /// submenu or a script.
    CommandWith(String, Vec<Arg>),
    /// Text from the clipboard, after [`Effect::Paste`].
    Pasted(String),
    /// The file a person chose, after [`Effect::Choose`], or `None` if they
    /// chose none.
    Chosen(Choice, Option<PathBuf>),
    /// The system's look changed: dark, or light.
    Theme {
        /// Whether it's dark.
        dark: bool,
    },
    /// The window was maximised, or restored.
    Maximized(bool),
    /// The window came forward: it has the keyboard again, as when the
    /// person switches back to it from another program.
    Focused,
}

/// What a file is chosen for, through the system's own dialog
/// (ARCHITECTURE.md section 8): the shell shows the dialog
/// [`Effect::Choose`] asks for and sends back [`Event::Chosen`]. It's
/// `part`'s, as parts ask for files too.
pub use ::part::Choice;

/// What the window itself does, which only the shell can: xWorks draws its
/// own title bar (review 3ce247c2), so its buttons and edges ask for these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    /// Move the window with the pointer, from a press in the title bar.
    Drag,
    /// Resize it with the pointer from an edge or corner.
    Resize(Edge),
    /// Minimise it.
    Minimize,
    /// Maximise it, or restore it if it's maximised.
    ToggleMaximize,
    /// Close it.
    Close,
}

/// An edge or corner of a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The top.
    North,
    /// The bottom.
    South,
    /// The right.
    East,
    /// The left.
    West,
    /// The top right.
    NorthEast,
    /// The top left.
    NorthWest,
    /// The bottom right.
    SouthEast,
    /// The bottom left.
    SouthWest,
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
    /// Open a window on a named version of the document at this path, as
    /// it was after this entry of its log, read-only
    /// ([`Editor::open_version`]).
    OpenVersion(PathBuf, i64),
    /// Hand this file to the system to open, such as a PDF to print.
    OpenExternally(PathBuf),
    /// Show this file in the system's file manager, such as an export.
    Reveal(PathBuf),
    /// Ask the person for a file, in the system's own dialog, and send it
    /// back as [`Event::Chosen`].
    Choose(Choice),
    /// Quit the app.
    Quit,
    /// Do something to the window itself.
    Window(WindowAction),
    /// Show the pointer as this over the window.
    Pointer(PointerShape),
}

/// What the pointer looks like over a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerShape {
    /// The arrow.
    #[default]
    Arrow,
    /// The text beam, over text that can be edited.
    Text,
    /// Resizing the window from an edge or corner.
    Resize(Edge),
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
    /// The story's paragraphs in reading order, each in its place, and
    /// the containers that placed them.
    entries: Vec<story::Entry>,
    containers: Vec<ObjectId>,
    /// The story's paragraphs in reading order, as laid out.
    ids: Vec<ObjectId>,
    /// The paragraph laid out with a composition in it, if one was.
    composed: Option<ObjectId>,
    index: HashMap<ObjectId, usize>,
    /// What each block poured holds: a paragraph, or a table's row.
    units: Vec<std::ops::Range<usize>>,
    /// The blocks poured.
    blocks: Vec<canvas::Block>,
    pages: Vec<Vec<Piece>>,
    /// Where each paragraph's lines are: (page, top within the page's
    /// body), by paragraph and line.
    places: Vec<Vec<(usize, i64)>>,
    /// Each paragraph's words, for the status bar, counted when it's laid
    /// out.
    words: Vec<usize>,
}

/// A panel of things to choose from, such as documents to open: each
/// row's name, and what choosing it does.
struct Panel {
    title: String,
    rows: Vec<(String, Pick)>,
    selected: usize,
}

/// What choosing a row of a panel does.
#[derive(Clone, Debug)]
enum Pick {
    /// Asks the shell for this.
    Effect(Effect),
    /// Makes the Markdown document an `.xw` file, to make an edit
    /// Markdown can't hold.
    Convert,
    /// Leaves it Markdown, without the edit.
    Keep,
    /// Takes the stylesheet this names, a newer version of the document's.
    Stylesheet(String),
    /// Nothing but closing the panel.
    Nothing,
    /// Keeps the Markdown file as another program changed it, and reads
    /// it again.
    Reread,
    /// Keeps the document as xWorks has it, written over the file another
    /// program changed.
    Overwrite,
    /// Keeps the document as xWorks has it, as a copy beside the file
    /// another program changed.
    Copy,
}

/// One document in one window.
pub struct Editor {
    res: Arc<Resources>,
    path: PathBuf,
    /// The document's file; none only while it's moved, or if it couldn't
    /// be opened again after.
    store: Option<Store>,
    document: Document,
    /// The document's stylesheet, resolved: what its paragraphs are laid
    /// out in.
    sheet: Arc<StyleSheet>,
    /// The document the stylesheet was resolved from, whose objects are
    /// compared with the current one's by identity, so it's resolved
    /// again only when a style changed.
    sheet_of: Document,
    /// Whether direct formatting is tinted in the text (View, Show Direct
    /// Formatting): off until the person turns it on.
    show_formatting: bool,
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
    /// Whether the story is shown on pages or as a draft.
    view: View,
    /// Whether the person named the document; until they do, it's named
    /// after its first heading or line (review a23vvez8).
    named: bool,
    /// A panel of things to choose from, such as named versions, which
    /// the window draws.
    panel: Option<Panel>,
    dragging: bool,
    /// What a double or triple click selected, which dragging extends.
    dragged: Option<(Unit, Position, Position)>,
    message: String,
    /// The pictures the document shows, decoded, by ID.
    pictures: canvas::Pictures,
    /// The named version shown, read-only, if this window shows one.
    version: Option<store::Version>,
    /// The Markdown file the document is, if it's one rather than an
    /// `.xw` file's.
    markdown: Option<Box<markdown::Markdown>>,
}

/// Sizes for Bigger and Smaller, in points.
const SIZES: &[i64] = &[8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72];

/// How a story is shown (ARCHITECTURE.md section 8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// On pages, as it prints.
    #[default]
    Pages,
    /// As a draft: one column of readable width, without pages, on the
    /// desk.
    Draft,
}

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
    /// "Untitled 2.xw" and so on) in the documents folder, in a copy of
    /// the first of the stylesheets that come with xWorks, Neutral.
    pub fn create(res: Arc<Resources>) -> Result<Editor, EditorError> {
        let spec = res.stylesheets[0].clone();
        Editor::create_in(res, &spec)
    }

    /// A new, empty document, made as [`Editor::create`] makes one, in a
    /// copy of the stylesheet `spec` gives.
    pub fn create_in(res: Arc<Resources>, spec: &story::SheetSpec) -> Result<Editor, EditorError> {
        let path = Editor::untitled(&res)?;
        let mut document = story::new_document(spec);
        let first = document.paragraphs()[0];
        let mut ops = Vec::new();
        // A document is in the system's language (project owner,
        // 2026-10-22, review pysc65hp); its paragraphs pass it on.
        if res.language != text::Language::und() {
            ops.push(Op::SetParaProp {
                paragraph: first,
                key: keys::LANGUAGE.to_string(),
                value: Some(Value::Text(res.language.to_string())),
            });
        }
        document
            .apply_all(&ops)
            .map_err(|e| EditorError::Store(store::Error::Damaged(e.to_string())))?;
        let store = Store::create(&path, &document, "story")?;
        Ok(Editor::with(
            res,
            path,
            Some(store),
            document,
            History::new(),
        ))
    }

    /// Where a new document goes: "Untitled.xw" in the documents folder,
    /// or "Untitled 2.xw" and so on, the folder made if it isn't there.
    fn untitled(res: &Resources) -> Result<PathBuf, EditorError> {
        std::fs::create_dir_all(&res.documents).map_err(|source| EditorError::Io {
            path: res.documents.clone(),
            source,
        })?;
        let mut n = 1;
        loop {
            let name = if n == 1 {
                "Untitled.xw".to_string()
            } else {
                format!("Untitled {n}.xw")
            };
            let path = res.documents.join(name);
            if !path.exists() {
                return Ok(path);
            }
            n += 1;
        }
    }

    /// The document at `path`: an `.xw` file, or a Markdown file, which
    /// opens as itself.
    pub fn open(res: Arc<Resources>, path: &Path) -> Result<Editor, EditorError> {
        if is_markdown(path) {
            return Editor::open_markdown(res, path);
        }
        let opened = Store::open(path)?;
        let mut editor = Editor::with(
            res,
            path.to_path_buf(),
            Some(opened.store),
            opened.document,
            opened.history,
        );
        editor.load_pictures(story::pictures(&editor.document));
        editor.unwritable();
        editor.offer_update();
        Ok(editor)
    }

    fn with(
        res: Arc<Resources>,
        path: PathBuf,
        store: Option<Store>,
        document: Document,
        history: History,
    ) -> Editor {
        Editor::with_view(res, path, store, document, history, View::Pages)
    }

    /// An editor on `document`, shown as `view`, laid out once, so.
    fn with_view(
        res: Arc<Resources>,
        path: PathBuf,
        store: Option<Store>,
        document: Document,
        history: History,
        view: View,
    ) -> Editor {
        let first = document.paragraphs()[0];
        let start = Position {
            paragraph: first,
            offset: 0,
            upstream: false,
        };
        let sheet = Arc::new(StyleSheet::of(&document, &res.program));
        let unloaded = res.load_families(&sheet.families()).err();
        let mut editor = Editor {
            res,
            path,
            store,
            sheet,
            sheet_of: document.clone(),
            show_formatting: false,
            laid: Laid {
                document: Document::new(),
                paragraphs: Vec::new(),
                entries: Vec::new(),
                containers: Vec::new(),
                ids: Vec::new(),
                composed: None,
                index: HashMap::new(),
                units: Vec::new(),
                blocks: Vec::new(),
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
            view,
            named: false,
            panel: None,
            dragging: false,
            dragged: None,
            message: String::new(),
            pictures: canvas::Pictures::new(),
            version: None,
            markdown: None,
        };
        editor.named = !editor.auto_named();
        editor.relayout(true);
        if let Some(e) = unloaded {
            editor.message = format!("Couldn't load fonts: {e}");
        }
        editor
    }

    /// The document's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The window's title: the document's name, and the version's if it
    /// shows one: "Report (Version 2)".
    pub fn title(&self) -> String {
        let name = self.path.file_stem().map_or_else(
            || "Untitled".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        match &self.version {
            Some(version) => format!("{name} ({})", version.name),
            None => name,
        }
    }

    /// How the story is shown.
    pub fn view_mode(&self) -> View {
        self.view
    }

    /// The document.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// What the status bar says of the last thing done, such as why the
    /// document is read-only; empty once all is saved.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Says `message` in the status bar.
    pub(crate) fn say(&mut self, message: &str) {
        self.message = message.to_string();
    }

    /// Whether the document can be changed: its file is open to write.
    pub(crate) fn writable(&self) -> bool {
        match self.markdown_read_only() {
            Some(why) => why.is_none(),
            None => self.store.as_ref().is_some_and(|s| s.read_only().is_none()),
        }
    }

    /// Whether the document can't be changed, saying why if so.
    pub(super) fn unwritable(&mut self) -> bool {
        if let Some(why) = self.markdown_read_only() {
            return match why {
                Some(why) => {
                    self.message = format!("Read-only: {why}");
                    true
                }
                None => false,
            };
        }
        match &self.store {
            Some(store) => match store.read_only() {
                Some(why) => {
                    self.message = format!("Read-only: {why}");
                    true
                }
                None => false,
            },
            None => {
                self.message = "Read-only: the document's file can't be reached".to_string();
                true
            }
        }
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
        for &id in &self.laid.ids {
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

    /// Paragraph `id`'s place in the story's reading order.
    fn index_of(&self, id: ObjectId) -> usize {
        self.laid.index.get(&id).copied().unwrap_or(0)
    }

    /// The story's paragraphs in reading order.
    fn ids(&self) -> &[ObjectId] {
        &self.laid.ids
    }

    // ---- layout

    /// Lays out what changed, and pours the story into pages again. With
    /// `all`, lays out every paragraph afresh.
    ///
    /// Typing changes one paragraph and no container, so then the flow is
    /// kept, every other paragraph keeps its layout where it is, and only
    /// the changed paragraph's block is made again before pouring.
    fn relayout(&mut self, all: bool) {
        let all = self.restyle() || all;
        let width = self.page_style().body_width();
        // Whether `id` is the very object it was when last laid out.
        let was = self.laid.document.clone();
        let now = &self.document;
        let unchanged = |id: ObjectId| match (was.object(id), now.object(id)) {
            (Some(a), Some(b)) => std::ptr::eq(a as *const Object, b as *const Object),
            _ => false,
        };
        // The story's paragraphs in reading order, each in its place:
        // walked again only where a container changed. The order first, so
        // the selection is ordered by it from here on.
        // A paragraph whose style changed may number the headings after
        // it otherwise.
        let restyled = |id: &ObjectId| match (was.paragraph(*id), now.paragraph(*id)) {
            (Some(a), Some(b)) => a.props.get(keys::STYLE) != b.props.get(keys::STYLE),
            _ => false,
        };
        let same_flow = !all
            && !self.laid.containers.is_empty()
            && self.laid.containers.iter().all(|&c| unchanged(c))
            && !now.changed_since(&was).iter().any(restyled);
        if !same_flow {
            let flow = story::flow(now, &self.sheet, width);
            self.laid.ids = flow.entries.iter().map(|e| e.id).collect();
            self.laid.index = self
                .laid
                .ids
                .iter()
                .enumerate()
                .map(|(i, &id)| (id, i))
                .collect();
            self.laid.entries = flow.entries;
            self.laid.containers = flow.containers;
        }
        let entries = std::mem::take(&mut self.laid.entries);
        let composing = !self.composition.is_empty();
        let composed = composing.then(|| self.composed());
        let focus = self.focus.paragraph;
        // A paragraph is laid out again if it changed, if a composition is
        // in it, or if one was, though it's gone now.
        let was_composed = self.laid.composed;
        let fresh_for =
            |id: ObjectId| !unchanged(id) || (composing && id == focus) || was_composed == Some(id);
        // Each paragraph's layout and word count, the word count being the
        // document's, not the composition's: kept where they are when the
        // flow is, and otherwise found by ID, kept if the paragraph and its
        // place are unchanged. What isn't kept is laid out below.
        let mut paragraphs: Vec<ParagraphLayout> = Vec::new();
        let mut slots: Vec<Option<ParagraphLayout>> = Vec::new();
        let (mut words, todo): (Vec<Option<usize>>, Vec<usize>);
        if same_flow {
            paragraphs = std::mem::take(&mut self.laid.paragraphs);
            words = std::mem::take(&mut self.laid.words)
                .into_iter()
                .enumerate()
                .map(|(k, count)| unchanged(entries[k].id).then_some(count))
                .collect();
            todo = (0..entries.len())
                .filter(|&k| fresh_for(entries[k].id))
                .collect();
        } else {
            let mut old: HashMap<ObjectId, (ParagraphLayout, usize)> =
                std::mem::take(&mut self.laid.paragraphs)
                    .into_iter()
                    .zip(std::mem::take(&mut self.laid.words))
                    .map(|(p, words)| (p.id, (p, words)))
                    .collect();
            let mut kept_words = Vec::with_capacity(entries.len());
            slots = entries
                .iter()
                .map(|entry| {
                    let id = entry.id;
                    let same = !all && unchanged(id);
                    match old.remove(&id) {
                        Some((layout, count)) if same => {
                            kept_words.push(Some(count));
                            (!fresh_for(id) && layout.place == entry.place).then_some(layout)
                        }
                        _ => {
                            kept_words.push(None);
                            None
                        }
                    }
                })
                .collect();
            words = kept_words;
            todo = (0..entries.len()).filter(|&k| slots[k].is_none()).collect();
        }
        // The rest laid out and counted afresh: on the worker pool when
        // there are several, as when a document opens, and here when
        // there's one, as when typing, which waking the pool would slow.
        let (document, res, sheet) = (&self.document, &self.res, &*self.sheet);
        let counted = &words;
        let lay_out = |&k: &usize| {
            let entry = &entries[k];
            let source = match &composed {
                Some(c) if entry.id == focus => c,
                _ => document,
            };
            let hyphenation = res.hyphenation();
            let layout = story::lay_out_at(
                source,
                entry.id,
                &entry.place,
                sheet,
                res.fonts(),
                &hyphenation,
            );
            let count = counted[k].unwrap_or_else(|| {
                let p = document.paragraph(entry.id).expect("a paragraph");
                story::word_count(p, sheet)
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
                    for (k, fresh) in todo.iter().zip(&mut fresh) {
                        if fresh.0.layout.missing {
                            *fresh = lay_out(k);
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => self.message = format!("Couldn't load fonts: {e}"),
            }
        }
        // Words in a language whose hyphenation patterns aren't loaded
        // bring them in, and their paragraphs are laid out again.
        let wanted: Vec<text::Language> = fresh
            .iter()
            .flat_map(|(p, _)| p.layout.unhyphenated.iter().cloned())
            .collect();
        if !wanted.is_empty() {
            match res.load_hyphenation(&wanted) {
                Ok(true) => {
                    for (k, fresh) in todo.iter().zip(&mut fresh) {
                        if !fresh.0.layout.unhyphenated.is_empty() {
                            *fresh = lay_out(k);
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => self.message = format!("Couldn't load hyphenation: {e}"),
            }
        }
        for (&k, (layout, count)) in todo.iter().zip(fresh) {
            if same_flow {
                paragraphs[k] = layout;
            } else {
                slots[k] = Some(layout);
            }
            words[k] = Some(count);
        }
        if !same_flow {
            paragraphs = slots
                .into_iter()
                .map(|p| p.expect("every paragraph laid out"))
                .collect();
        }
        let words: Vec<usize> = words.into_iter().map(|w| w.unwrap_or(0)).collect();
        // The blocks to pour: those of changed paragraphs made again, when
        // the flow is kept.
        let look = &self.sheet.blocks;
        let (units, blocks) = if same_flow && self.laid.blocks.len() == self.laid.units.len() {
            let units = std::mem::take(&mut self.laid.units);
            let mut blocks = std::mem::take(&mut self.laid.blocks);
            for &k in &todo {
                let u = units.partition_point(|r| r.end <= k);
                blocks[u] = canvas::block(&paragraphs, units[u].clone(), look);
            }
            (units, blocks)
        } else {
            (
                canvas::units(&paragraphs),
                canvas::blocks(&paragraphs, look),
            )
        };
        let pages = canvas::paginate(self.page_style().body_height(), &blocks);
        let mut places: Vec<Vec<(usize, i64)>> = paragraphs
            .iter()
            .map(|p| vec![(0, 0); p.layout.lines.len()])
            .collect();
        let pad = look.cell_padding;
        for (n, pieces) in pages.iter().enumerate() {
            for piece in pieces {
                let unit = units[piece.block].clone();
                // A rule across the text has no lines of text.
                if unit.is_empty() {
                    continue;
                }
                if paragraphs[unit.start].place.cell.is_some() {
                    // A table's row: every line of each of its cells.
                    for k in unit {
                        for (l, line) in paragraphs[k].layout.lines.iter().enumerate() {
                            places[k][l] = (n, piece.y + pad + line.top);
                        }
                    }
                    continue;
                }
                let k = unit.start;
                let lines = &paragraphs[k].layout.lines;
                let top = lines[piece.lines.start].top;
                for l in piece.lines.clone() {
                    places[k][l] = (n, piece.y + lines[l].top - top);
                }
            }
        }
        self.laid = Laid {
            document: self.document.clone(),
            entries,
            containers: std::mem::take(&mut self.laid.containers),
            ids: std::mem::take(&mut self.laid.ids),
            composed: composing.then_some(focus),
            index: std::mem::take(&mut self.laid.index),
            units,
            blocks,
            paragraphs,
            pages,
            places,
            words,
        };
    }

    /// Resolves the document's stylesheet again if a style or the sheet
    /// changed since it was last resolved, and brings in the fonts it sets
    /// text in that aren't loaded. True if it changed, so every paragraph
    /// is laid out again.
    fn restyle(&mut self) -> bool {
        let (was, now) = (&self.sheet_of, &self.document);
        let same = |id: ObjectId| match (was.object(id), now.object(id)) {
            (Some(a), Some(b)) => std::ptr::eq(a as *const Object, b as *const Object),
            (None, None) => true,
            _ => false,
        };
        let changed = was.sheet() != now.sheet()
            || now
                .sheet()
                .is_some_and(|sheet| !same(sheet) || now.styles().iter().any(|&id| !same(id)));
        if !changed {
            return false;
        }
        self.sheet = Arc::new(StyleSheet::of(&self.document, &self.res.program));
        self.sheet_of = self.document.clone();
        if let Err(e) = self.res.load_families(&self.sheet.families()) {
            self.message = format!("Couldn't load fonts: {e}");
        }
        true
    }

    /// The document's stylesheet, resolved.
    pub fn sheet(&self) -> &StyleSheet {
        &self.sheet
    }

    /// The operations that take paragraph `id`'s own paragraph formatting
    /// away, and with `text`, its text's own formatting too: all of it, or
    /// along `range` of it if one is given.
    pub(crate) fn clearing(&self, id: ObjectId, range: Option<(u32, u32)>, text: bool) -> Vec<Op> {
        let p = self.paragraph(id);
        let mut ops = Vec::new();
        if range.is_none() || text {
            for key in story::styles::PARAGRAPH_FORMATTING {
                if range.is_none() && p.props.contains_key(key) {
                    ops.push(Op::SetParaProp {
                        paragraph: id,
                        key: key.to_string(),
                        value: None,
                    });
                }
            }
        }
        if text {
            let (from, to) = range.unwrap_or((0, p.len()));
            for key in story::styles::CHARACTER_FORMATTING {
                let mut pos = 0;
                let any = p.runs.iter().any(|run| {
                    let (a, b) = (pos, pos + run.len);
                    pos = b;
                    b > from && a < to && run.props.contains_key(key)
                });
                if any && to > from {
                    ops.push(Op::SetCharProp {
                        paragraph: id,
                        offset: from,
                        len: to - from,
                        key: key.to_string(),
                        value: None,
                    });
                }
            }
        }
        ops
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

    /// The top of its area: its own coordinates start there.
    fn top(&self) -> i64 {
        0
    }

    /// The height of its area.
    fn view_height(&self) -> i64 {
        self.height.max(PIXEL)
    }

    /// The room above the first page, and between pages.
    fn gaps(&self) -> (i64, i64) {
        let s = &self.res.sizes;
        match self.view {
            View::Pages => (s.page_top.units(), s.page_gap.units()),
            View::Draft => (s.draft_top.units(), s.page_gap.units()),
        }
    }

    /// How wide the draft's column is: as wide as reads well, or the area
    /// less a gap at each side.
    fn column(&self) -> i64 {
        let s = &self.res.sizes;
        let gap = s.page_gap.units();
        s.draft_column
            .units()
            .min(self.width - 2 * gap)
            .max(160 * PIXEL)
    }

    /// The page the story is poured into: the page style's, or as a
    /// draft, one column as tall as it needs, without margins.
    pub(crate) fn page_style(&self) -> canvas::PageStyle {
        match self.view {
            View::Pages => self.sheet.page,
            View::Draft => canvas::PageStyle {
                width: self.column(),
                height: i64::MAX / 8,
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            },
        }
    }

    /// How tall the draft's text is.
    fn draft_height(&self) -> i64 {
        let Some(pieces) = self.laid.pages.first() else {
            return 0;
        };
        pieces.last().map_or(0, |piece| {
            let block = &self.laid.blocks[piece.block];
            piece.y + block.lines[piece.lines.clone()].iter().sum::<i64>()
        })
    }

    /// Page `n`'s top left, in document coordinates: centred across the
    /// area, on a whole pixel, so its edges are sharp.
    fn page_origin(&self, n: usize) -> (i64, i64) {
        let (top, gap) = self.gaps();
        let whole = |x: i64| (x + PIXEL / 2).div_euclid(PIXEL) * PIXEL;
        match self.view {
            View::Pages => {
                let page = &self.sheet.page;
                let x = whole((self.width - page.width) / 2).max(gap);
                (x, top + n as i64 * (page.height + gap))
            }
            View::Draft => (whole((self.width - self.column()) / 2), top),
        }
    }

    fn document_height(&self) -> i64 {
        let (top, gap) = self.gaps();
        match self.view {
            View::Pages => top + self.laid.pages.len() as i64 * (self.sheet.page.height + gap),
            View::Draft => 2 * top + self.draft_height(),
        }
    }

    /// Where line `line` of paragraph `k` starts, in window coordinates:
    /// its left and top.
    fn line_origin(&self, k: usize, line: usize) -> (i64, i64) {
        let (n, y) = self.laid.places[k][line];
        let (px, py) = self.page_origin(n);
        let page = &self.page_style();
        let p = &self.laid.paragraphs[k];
        let top = p.layout.lines[line].top;
        (
            px + page.left + p.x(),
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
        let page = &self.page_style();
        let doc_y = y - self.top() + self.scroll;
        let (top, gap) = self.gaps();
        let stride = page.height + gap;
        let last = self.laid.pages.len().saturating_sub(1);
        let n = (((doc_y - top).max(0)) / stride).min(last as i64) as usize;
        let (px, py) = self.page_origin(n);
        let body_y = doc_y - py - page.top;
        let body_x = x - px - page.left;
        // The line nearest that height on that page; in a table's row, in
        // the cell across from it.
        let mut best: Option<(i64, usize, usize)> = None;
        for piece in &self.laid.pages[n] {
            let unit = self.laid.units[piece.block].clone();
            if unit.is_empty() {
                // A rule: the caret goes to the text around it.
                continue;
            }
            let cells = unit.len() > 1 || self.laid.paragraphs[unit.start].place.cell.is_some();
            for k in unit {
                let p = &self.laid.paragraphs[k];
                if let Some(cell) = p.place.cell
                    && cells
                    && !(cell.x..cell.x + cell.width).contains(&body_x)
                    && !(cell.column == 0 && body_x < cell.x)
                    && !(cell.column + 1 == cell.columns && body_x >= cell.x)
                {
                    continue;
                }
                let lines = &p.layout.lines;
                let range = if cells {
                    0..lines.len()
                } else {
                    piece.lines.clone()
                };
                for l in range {
                    let line_top = self.laid.places[k][l].1;
                    let bottom = line_top + lines[l].height;
                    let distance = if body_y < line_top {
                        line_top - body_y
                    } else if body_y >= bottom {
                        body_y - bottom + 1
                    } else {
                        0
                    };
                    if best.is_none_or(|(d, _, _)| distance < d) {
                        best = Some((distance, k, l));
                    }
                }
            }
        }
        let (k, l) = best.map_or((0, 0), |(_, k, l)| (k, l));
        let x = body_x - self.laid.paragraphs[k].x();
        let cursor = self.laid.paragraphs[k].layout.hit_line(l, x);
        self.position(k, cursor)
    }

    /// Scrolls so the caret is in view.
    fn reveal(&mut self) {
        let (_, top, bottom) = self.caret_rect();
        let (view_top, view_bottom) = (self.top(), self.top() + self.view_height());
        let gap = self.gaps().1;
        if top < view_top {
            self.scroll -= view_top - top + gap;
        } else if bottom > view_bottom {
            self.scroll += bottom - view_bottom + gap;
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

    /// The selected text, paragraphs ending in newlines but the last, as
    /// the clipboard takes it: plain text, without its pictures'
    /// characters.
    fn selected_text(&self) -> String {
        let (start, end) = self.ordered();
        let ids = self.ids();
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
            out.extend(
                text[from..to.max(from)]
                    .chars()
                    .filter(|&c| c != story::OBJECT),
            );
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
        let ids = self.ids();
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

    /// The line above or below `p`'s, at `goal` across from the page
    /// body's left: in the same paragraph, or the block above or below; in
    /// a table's row, the cell under `goal`.
    fn vertical(&self, p: Position, down: bool, goal: i64) -> Option<Position> {
        let (k, cursor) = self.cursor(p);
        let layout = &self.laid.paragraphs[k].layout;
        let line = layout.line_of(cursor);
        let units = &self.laid.units;
        let u = units.partition_point(|r| r.end <= k);
        // The paragraph of a block that's under `goal`.
        let under = |u: usize| {
            let unit = units[u].clone();
            unit.clone()
                .find(|&c| {
                    self.laid.paragraphs[c]
                        .place
                        .cell
                        .is_some_and(|cell| goal < cell.x + cell.width)
                })
                .unwrap_or(unit.end - 1)
        };
        // The blocks below and above that have text: past rules.
        let below = (u + 1..units.len()).find(|&v| !units[v].is_empty());
        let above = (0..u).rev().find(|&v| !units[v].is_empty());
        let (k, line) = if down {
            if line + 1 < layout.lines.len() {
                (k, line + 1)
            } else {
                (under(below?), 0)
            }
        } else if line > 0 {
            (k, line - 1)
        } else {
            let k = under(above?);
            (k, self.laid.paragraphs[k].layout.lines.len() - 1)
        };
        let p = &self.laid.paragraphs[k];
        let cursor = p.layout.hit_line(line, goal - p.x());
        Some(self.position(k, cursor))
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        if self.panel.is_some() {
            return self.panel_key(key);
        }
        if let Some(command) = self.res.commands.for_key(key, modifiers, self.res.mac) {
            let id = command.id.clone();
            return self.command(&id);
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
                    let p = &self.laid.paragraphs[k];
                    let line = p.layout.line_of(cursor);
                    x - self.line_origin(k, line).0 + p.x()
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
                let by = self.view_height() - self.gaps().1 * 2;
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
            // In a list item or a table's cell, Tab moves through the tree;
            // elsewhere it types a tab.
            Key::Tab if !modifiers.primary && !modifiers.alt => {
                let ids = self.ids().to_vec();
                let outcome = blocks::tab(&self.document, &ids, self.focus, modifiers.shift);
                if !self.carry_out(outcome) {
                    self.insert_text("\t");
                }
            }
            Key::Tab | Key::Char(_) | Key::Alt | Key::F10 => {}
        }
        self.reveal();
        Vec::new()
    }

    fn document_edge(&self, end: bool) -> Position {
        let ids = self.ids();
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
        if self.unwritable() {
            return false;
        }
        let t = match self.document.transact(name, ops) {
            Ok(t) => t,
            Err(e) => {
                self.message = format!("That didn't work: {e}");
                return false;
            }
        };
        if self.is_markdown() {
            // Written whole, if Markdown holds it; if not, not made, and
            // the person asked.
            self.message.clear();
            let logged = if continuing {
                markdown::Logged::Continue
            } else {
                markdown::Logged::Commit
            };
            if let Err(unheld) = self.save_markdown(logged, &t) {
                self.document
                    .apply_all(&t.inverse)
                    .expect("an edit's inverse applies to what it made");
                self.typing = None;
                self.ask_to_convert(name, t.ops, unheld);
                return false;
            }
        }
        let entry = if continuing {
            Entry::Continue(&t)
        } else {
            Entry::Commit(&t)
        };
        match self.store.as_mut().map(|s| s.append(entry, &self.document)) {
            Some(Ok(_)) => self.message.clear(),
            Some(Err(e)) => self.message = format!("Couldn't save: {e}"),
            None => {}
        }
        let placed = Editor::pictures_in(&t.ops);
        if !(continuing && self.history.continue_last(t.clone())) {
            self.history.push(t);
        }
        self.relayout(false);
        self.load_pictures(placed);
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
        let ids = self.ids();
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
        // Across containers, or a rule between, blocks between go whole.
        if ids[a..=b]
            .windows(2)
            .any(|w| self.document.next(w[0]) != Some(w[1]))
        {
            let ops = blocks::deletion(&self.document, ids, start, end).unwrap_or_default();
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
        // Text typed after a picture is text.
        for key in keys::PICTURES {
            props.remove(key);
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
        // A picture's character comes only with its picture.
        let text: String = text
            .chars()
            .filter(|&c| (!c.is_control() || c == '\u{2028}' || c == '\t') && c != story::OBJECT)
            .collect();
        if text.is_empty() {
            return;
        }
        let collapsed = self.collapsed();
        let continuing = collapsed && self.typing == Some(self.focus);
        let (mut ops, at) = self.deletion();
        let props = self.typing_props(at);
        let len = chars(&text);
        ops.push(Op::InsertText {
            paragraph: at.paragraph,
            offset: at.offset,
            text: text.clone(),
            runs: vec![Run::new(len, props)],
        });
        // Typing on in an address linked as typed takes the link along.
        if collapsed {
            let relinked = self.relink(&self.document, &ops, at, len);
            ops.extend(relinked);
        }
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
            // An address it ends becomes a link, an edit of its own.
            self.link_typed(&text);
        }
    }

    /// Carries out what a key does to the story's tree; false if it leaves
    /// it to the key's plain meaning.
    fn carry_out(&mut self, outcome: Option<blocks::Outcome>) -> bool {
        match outcome {
            Some(blocks::Outcome::Edit { name, ops, caret }) => {
                if self.commit(name, ops, false) {
                    self.go(caret, false);
                }
            }
            Some(blocks::Outcome::Go(caret)) => self.go(caret, false),
            Some(blocks::Outcome::Nothing) => {}
            None => return false,
        }
        true
    }

    /// Whether paragraph `id` is a code block.
    fn is_code(&self, id: ObjectId) -> bool {
        self.sheet.style_of(self.paragraph(id)).code
    }

    fn split(&mut self) {
        if self.collapsed() {
            let enter = blocks::enter(&self.document, &self.sheet, self.focus);
            if enter.is_none() && self.is_code(self.focus.paragraph) {
                // Code breaks its lines, not the paragraph.
                self.insert_text("\u{2028}");
                return;
            }
            let at = self.focus;
            if self.carry_out(enter) {
                // An address the paragraph ended in becomes a link.
                self.link_ending(at.paragraph, at.offset as usize, at.offset as usize);
                return;
            }
        }
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return;
        }
        let paragraph = scratch.paragraph(at.paragraph).expect("a paragraph");
        let mut props = paragraph.props.clone();
        // After a heading's last character comes the default style, and
        // its own paragraph formatting stays behind with it.
        if at.offset == paragraph.len() && self.sheet.style_of(paragraph).heading.is_some() {
            match self.sheet.default_style().id {
                Some(body) => props.insert(keys::STYLE.to_string(), Value::Ref(body)),
                None => props.remove(keys::STYLE),
            };
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
            // An address the paragraph ended in becomes a link.
            self.link_ending(at.paragraph, at.offset as usize, at.offset as usize);
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
        let ids = self.ids().to_vec();
        let k = self.index_of(at.paragraph);
        // At a paragraph's edge, its place in the tree may say otherwise.
        let edge = if forward {
            here == text.len()
        } else {
            here == 0
        };
        if edge && self.carry_out(blocks::edge(&self.document, &ids, k, forward)) {
            return;
        }
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
        // What Copy kept in the program, structure and styles, while the
        // clipboard holds what it put there.
        if self.paste_fragment(text) {
            return;
        }
        let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
        // Code takes lines as lines, and a table's cell as one.
        let at = self.ordered().0.paragraph;
        if self.is_code(at) {
            text = text.replace('\n', "\u{2028}");
        } else if self
            .document
            .parent(at)
            .and_then(|p| self.document.block(p))
            .is_some_and(|b| b.kind == doc::Kind::Row)
        {
            text = text.replace('\n', " ");
        }
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return;
        }
        let props = self.typing_props(at);
        let mut caret = at;
        // What the paste put in each paragraph, for linking addresses.
        let mut pasted = Vec::new();
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
            let line: String = line
                .chars()
                .filter(|&c| (!c.is_control() || c == '\t') && c != story::OBJECT)
                .collect();
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
            pasted.push((
                caret.paragraph,
                caret.offset as usize,
                (caret.offset + len) as usize,
            ));
            caret.offset += len;
            caret.upstream = true;
        }
        if self.commit("Paste", ops, false) {
            self.go(caret, false);
            self.link_pasted(&pasted);
        }
    }

    /// Sets character property `key` to `value` over the selection.
    fn set_char_prop(&mut self, name: &str, key: &str, value: Option<Value>) {
        let (start, end) = self.ordered();
        let ids = self.ids().to_vec();
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
        let ids = self.ids();
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

    /// The list item the caret's in, if it's a task, and whether it's
    /// done.
    fn task_at_caret(&self) -> Option<(ObjectId, bool)> {
        let item = self.document.parent(self.focus.paragraph)?;
        let b = self.document.block(item)?;
        match (b.kind, b.props.get(keys::TASK)) {
            (doc::Kind::Item, Some(Value::Bool(done))) => Some((item, *done)),
            _ => None,
        }
    }

    /// Checks task `item` off, or back on: its box ticked, or not.
    fn check(&mut self, item: ObjectId, done: bool) {
        let name = if done { "Check Off" } else { "Uncheck" };
        self.commit(
            name,
            vec![Op::SetBlockProp {
                block: item,
                key: keys::TASK.to_string(),
                value: Some(Value::Bool(done)),
            }],
            false,
        );
    }

    /// Checks the caret's task off, or back on.
    fn check_off(&mut self) {
        if let Some((item, done)) = self.task_at_caret() {
            self.check(item, !done);
        }
    }

    /// Makes the items the selection's paragraphs are in tasks, each with
    /// its box unticked, or, if they all are, items again; paragraphs in
    /// no list are made a bulleted list of tasks.
    fn toggle_tasks(&mut self) {
        let (start, end) = self.ordered();
        let ids = self.ids().to_vec();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let selected: Vec<ObjectId> = ids[a..=b].to_vec();
        let items_in = |document: &Document| -> Vec<ObjectId> {
            let mut items: Vec<ObjectId> = Vec::new();
            for &p in &selected {
                if let Some(item) = document.parent(p)
                    && document
                        .block(item)
                        .is_some_and(|b| b.kind == doc::Kind::Item)
                    && !items.contains(&item)
                {
                    items.push(item);
                }
            }
            items
        };
        let mut scratch = self.document.clone();
        let mut ops = Vec::new();
        let mut caret = None;
        if items_in(&scratch).is_empty()
            && let Some(blocks::Outcome::Edit {
                ops: made,
                caret: at,
                ..
            }) = blocks::toggle_list(&self.document, &ids, start, end, false)
        {
            if scratch.apply_all(&made).is_err() {
                return;
            }
            ops = made;
            caret = Some(at);
        }
        let items = items_in(&scratch);
        let task = |i: ObjectId| {
            scratch
                .block(i)
                .and_then(|b| b.props.get(keys::TASK))
                .is_some()
        };
        let all = !items.is_empty() && items.iter().all(|&i| task(i));
        for &item in &items {
            if all || !task(item) {
                ops.push(Op::SetBlockProp {
                    block: item,
                    key: keys::TASK.to_string(),
                    value: (!all).then_some(Value::Bool(false)),
                });
            }
        }
        if self.commit("Task List", ops, false)
            && let Some(at) = caret
        {
            self.go(at, false);
        }
    }

    /// The task whose box is at (`x`, `y`) in the window, if one is, and
    /// whether it's done.
    fn task_box_at(&self, x: i64, y: i64) -> Option<(ObjectId, bool)> {
        for (k, p) in self.laid.paragraphs.iter().enumerate() {
            let (Some(done), Some(line)) = (p.place.task, p.layout.lines.first()) else {
                continue;
            };
            if self.laid.places.get(k).is_none_or(|l| l.is_empty()) {
                continue;
            }
            let (text_x, top) = self.line_origin(k, 0);
            let side = (line.ascent * 7 / 10).max(PIXEL);
            let right = text_x - p.x() + p.place.gutter - p.place.gap;
            let baseline = top + line.baseline;
            let slack = 3 * PIXEL;
            if (right - side - slack..=right + slack).contains(&x)
                && (baseline - side - slack..=baseline + slack).contains(&y)
            {
                let item = self.document.parent(p.id)?;
                return Some((item, done));
            }
        }
        None
    }

    /// The font size at the selection's start, in layout units.
    fn size(&self) -> i64 {
        let at = self.ordered().0;
        match self.typing_props(at).get(keys::SIZE) {
            Some(Value::Int(size)) if *size > 0 => *size,
            _ => {
                let p = self.paragraph(at.paragraph);
                story::styled(p, &self.sheet, None)
                    .0
                    .first()
                    .map_or(self.sheet.style_of(p).size, |s| s.style.size)
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

    /// Applies paragraph style `style` to the paragraphs selected: each
    /// takes it in place of its own paragraph formatting, but keeps its
    /// text's own formatting, unless `clear` says to take that away too
    /// (project owner, 2026-10-24, review 30dmn1js).
    pub(crate) fn set_style(&mut self, style: ObjectId, clear: bool) {
        let ids = self.selected_paragraphs();
        let mut ops = Vec::new();
        for &id in &ids {
            ops.push(Op::SetParaProp {
                paragraph: id,
                key: keys::STYLE.to_string(),
                value: Some(Value::Ref(style)),
            });
            ops.extend(self.clearing(id, None, clear));
        }
        let name = self.sheet.paragraph_style(Some(style)).name.clone();
        self.commit(&name, ops, false);
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
            // The start of the first paragraph in a block put somewhere;
            // where one was deleted, wherever the operations before say.
            Op::InsertBlock { objects, .. } => objects
                .first()
                .and_then(|(id, _)| self.first_paragraph(*id))
                .and_then(|p| at(p, 0))
                .or_else(|| self.after(&ops[..ops.len() - 1])),
            Op::MoveBlock { block: id, .. } | Op::SetBlockProp { block: id, .. } => self
                .first_paragraph(*id)
                .and_then(|p| at(p, 0))
                .or_else(|| self.after(&ops[..ops.len() - 1])),
            Op::DeleteBlock { .. } => self.after(&ops[..ops.len() - 1]),
            // Styles move no text: the caret goes where the operations
            // before say.
            Op::InsertStyle { .. }
            | Op::DeleteStyle { .. }
            | Op::SetStyleProp { .. }
            | Op::SetBase { .. } => self.after(&ops[..ops.len() - 1]),
        }
    }

    /// The first paragraph in `block`, in reading order; for a block with
    /// none, as a rule has, the first after it.
    fn first_paragraph(&self, block: ObjectId) -> Option<ObjectId> {
        let first_in = |block: ObjectId| {
            self.document
                .subtree(block)
                .into_iter()
                .find(|&id| self.document.paragraph(id).is_some())
        };
        if let Some(p) = first_in(block) {
            return Some(p);
        }
        let mut at = block;
        loop {
            match self.document.next(at) {
                Some(next) => match first_in(next) {
                    Some(p) => return Some(p),
                    None => at = next,
                },
                None => at = self.document.parent(at)?,
            }
        }
    }

    fn undo(&mut self, redo: bool) {
        if self.unwritable() {
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
        if let Some(Err(e)) = self.store.as_mut().map(|s| s.append(entry, &self.document)) {
            self.message = format!("Couldn't save: {e}");
        }
        let logged = if redo {
            markdown::Logged::Redo
        } else {
            markdown::Logged::Undo
        };
        if let Err(unheld) = self.save_markdown(logged, &t) {
            self.message = format!("Couldn't save: {unheld}");
        }
        self.relayout(false);
        let ops = if redo { &t.ops } else { &t.inverse };
        // Where the change was; or, where that can't be told, as when a
        // rule came or went, where the caret is, if that's still there.
        let focus = self.focus;
        let kept = self.document.paragraph(focus.paragraph).map(|p| Position {
            offset: focus.offset.min(p.len()),
            ..focus
        });
        // An address linked as typed, undone, leaves the caret where it
        // was, after what ended it.
        let caret = if t.name == links::LINK {
            kept
        } else {
            self.after(ops)
        };
        let caret = caret.or(kept).unwrap_or_else(|| self.document_edge(false));
        self.go(caret, false);
        self.message = format!("{} {}", if redo { "Redid" } else { "Undid" }, t.name);
        self.load_pictures(Editor::pictures_in(ops));
    }

    // ---- commands

    /// Whether command `id` can run now, and whether it's on.
    pub fn state(&self, id: &str) -> CommandState {
        let writable = self.writable();
        let enabled = match id {
            "undo" => writable && self.history.undo_name().is_some(),
            "redo" => writable && self.history.redo_name().is_some(),
            "cut" | "copy" => !self.collapsed(),
            "copy-out" => self.version.is_some(),
            "save-version" | "paste" | "bold" | "italic" | "strikethrough" | "code"
            | "list-bullets" | "list-numbers" | "list-tasks" | "quote" | "insert-table"
            | "insert-rule" | "insert-picture" => writable,
            "task-toggle" => writable && self.task_at_caret().is_some(),
            "clear-formatting" => writable && self.has_direct_formatting(),
            "redefine-style" | "new-style" | "replace-stylesheet" => {
                writable && self.document.sheet().is_some()
            }
            "save-stylesheet" => self.document.sheet().is_some() && self.res.own.folder().is_some(),
            "update-stylesheet" => writable && self.newer_stylesheet().is_some(),
            "style-set" | "style-toggle" | "style-reset" | "style-base" | "style-rename"
            | "page-size" | "page-margin" => writable,
            _ if id.starts_with("language-") || id.starts_with("align-") => writable,
            _ => true,
        };
        let checked = match id {
            "bold" => self.all(keys::BOLD),
            "italic" => self.all(keys::ITALIC),
            "strikethrough" => self.all(keys::STRIKETHROUGH),
            "list-tasks" => self.task_at_caret().is_some(),
            "task-toggle" => self.task_at_caret().is_some_and(|(_, done)| done),
            "view-draft" => self.view == View::Draft,
            "view-pages" => self.view == View::Pages,
            "show-formatting" => self.show_formatting,
            _ => {
                id.strip_prefix("style-")
                    .is_some_and(|style| self.style_id() == style)
                    || self.unit_checked(id)
            }
        };
        CommandState { enabled, checked }
    }

    /// The caret's paragraph's style's role, as a command ID ends:
    /// "body", "heading-1".
    fn style_id(&self) -> String {
        let p = self.paragraph(self.focus.paragraph);
        self.sheet.style_of(p).role.clone().unwrap_or_default()
    }

    fn command(&mut self, id: &str) -> Vec<Effect> {
        if !self.state(id).enabled {
            return Vec::new();
        }
        if self.unit_command(id) {
            self.reveal();
            return Vec::new();
        }
        let mut effects = Vec::new();
        match id {
            "new" => effects.push(Effect::Open(None)),
            "new-markdown" => match Editor::new_markdown(&self.res) {
                Ok(path) => effects.push(Effect::Open(Some(path))),
                Err(e) => self.message = format!("Couldn't make a Markdown document: {e}"),
            },
            "open" => effects.push(Effect::Choose(Choice::Open)),
            "move-to" => effects.push(Effect::Choose(Choice::MoveTo)),
            "quit" => effects.push(Effect::Quit),
            "show-formatting" => self.show_formatting = !self.show_formatting,
            "view-draft" | "view-pages" => {
                let view = if id == "view-draft" {
                    View::Draft
                } else {
                    View::Pages
                };
                self.set_view(view);
            }
            "save-version" if self.is_markdown() => self.name_markdown_version(),
            "save-version" => {
                if let Some(store) = &mut self.store {
                    let n = store.versions().map_or(0, |v| v.len()) + 1;
                    let name = format!("Version {n}");
                    self.message = match store.name_version(&name) {
                        Ok(()) => format!("Saved {name}"),
                        Err(e) => format!("Couldn't save a version: {e}"),
                    };
                }
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
            "copy" => {
                let text = self.selected_text();
                self.keep_fragment(&text);
                effects.push(Effect::Copy(text));
            }
            "cut" => {
                let text = self.selected_text();
                self.keep_fragment(&text);
                effects.push(Effect::Copy(text));
                let (ops, at) = self.deletion();
                if self.commit("Cut", ops, false) {
                    self.go(at, false);
                }
            }
            "paste" => effects.push(Effect::Paste),
            "insert-picture" => effects.push(Effect::Choose(Choice::Picture)),
            "versions" => self.versions_panel(),
            "copy-out" => effects.extend(self.copy_out()),
            "select-all" => {
                self.anchor = self.document_edge(false);
                self.focus = self.document_edge(true);
                self.typing = None;
            }
            "bold" => self.toggle("Bold", keys::BOLD),
            "italic" => self.toggle("Italic", keys::ITALIC),
            "strikethrough" => self.toggle("Strikethrough", keys::STRIKETHROUGH),
            "code" => self.toggle("Code", keys::CODE),
            "list-tasks" => self.toggle_tasks(),
            "clear-formatting" => self.clear_formatting(),
            "redefine-style" => self.redefine_style(),
            "new-style" => self.new_style(),
            "save-stylesheet" => self.save_stylesheet(),
            "update-stylesheet" => self.update_panel(),
            "task-toggle" => self.check_off(),
            "list-bullets" | "list-numbers" | "quote" | "insert-table" | "insert-rule" => {
                let (start, end) = self.ordered();
                let ids = self.ids().to_vec();
                let outcome = match id {
                    "quote" => blocks::toggle_quote(&self.document, &ids, start, end),
                    "insert-table" => blocks::insert_table(&self.document, start),
                    "insert-rule" => blocks::insert_rule(&self.document, &self.sheet, start),
                    _ => {
                        blocks::toggle_list(&self.document, &ids, start, end, id == "list-numbers")
                    }
                };
                self.carry_out(outcome);
            }
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
                } else if let Some(role) = id.strip_prefix("style-") {
                    // The paragraph styles in the menus are by role, so
                    // they're the same commands in every stylesheet.
                    if let Some(style) = self.sheet.by_role(role).and_then(|s| s.id) {
                        self.set_style(style, false);
                    }
                }
            }
        }
        self.reveal();
        effects
    }

    /// The document's pages and structure, as drawn and printed.
    /// On pages, they're those shown; as a draft, the story is laid out
    /// on pages afresh, as it prints.
    pub fn pages(&self) -> canvas::Pages {
        let look = &self.sheet.blocks;
        if self.view == View::Draft {
            let page = &self.sheet.page;
            let flow = story::flow(&self.document, &self.sheet, page.body_width());
            let hyphenation = self.res.hyphenation();
            let (document, sheet, fonts) = (&self.document, &self.sheet, self.res.fonts());
            let paragraphs: Vec<ParagraphLayout> = flow
                .entries
                .par_iter()
                .map(|e| story::lay_out_at(document, e.id, &e.place, sheet, fonts, &hyphenation))
                .collect();
            let blocks = canvas::blocks(&paragraphs, look);
            let pages = canvas::paginate(page.body_height(), &blocks);
            return canvas::Pages {
                pages: canvas::draw(
                    page,
                    look,
                    &paragraphs,
                    &pages,
                    self.res.fonts(),
                    &self.pictures,
                ),
                structure: canvas::structure(&paragraphs),
            };
        }
        canvas::Pages {
            pages: canvas::draw(
                &self.sheet.page,
                look,
                &self.laid.paragraphs,
                &self.laid.pages,
                self.res.fonts(),
                &self.pictures,
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

    // ---- a panel of things to choose from, which the window draws

    /// The panel open, if one is: its title, its rows' names, and the row
    /// lit.
    pub fn panel(&self) -> Option<(&str, Vec<String>, usize)> {
        let panel = self.panel.as_ref()?;
        let names = panel.rows.iter().map(|(name, _)| name.clone()).collect();
        Some((panel.title.as_str(), names, panel.selected))
    }

    /// Closes the panel, choosing nothing: an edit Markdown can't hold,
    /// asked about, isn't made; a change another program made, asked
    /// about, is asked about again later, nothing written meanwhile.
    pub(crate) fn close_panel(&mut self) {
        self.panel = None;
        self.keep_markdown();
        self.defer_disk();
    }

    /// What choosing row `row` of the panel does, which closes it.
    pub(crate) fn choose_row(&mut self, row: usize) -> Vec<Effect> {
        let chosen = self
            .panel
            .take()
            .and_then(|mut p| (row < p.rows.len()).then(|| p.rows.swap_remove(row).1));
        match chosen {
            Some(Pick::Effect(effect)) => vec![effect],
            Some(Pick::Convert) => self.convert(),
            Some(Pick::Stylesheet(key)) => {
                self.take_update(&key);
                Vec::new()
            }
            Some(Pick::Nothing) => Vec::new(),
            Some(Pick::Reread) => {
                self.read_again();
                Vec::new()
            }
            Some(Pick::Overwrite) => {
                self.overwrite_disk();
                Vec::new()
            }
            Some(Pick::Copy) => {
                self.copy_disk();
                Vec::new()
            }
            Some(Pick::Keep) | None => {
                self.keep_markdown();
                Vec::new()
            }
        }
    }

    pub(crate) fn panel_key(&mut self, key: Key) -> Vec<Effect> {
        let Some(panel) = &mut self.panel else {
            return Vec::new();
        };
        match key {
            Key::Up => panel.selected = panel.selected.saturating_sub(1),
            Key::Down => {
                panel.selected = (panel.selected + 1).min(panel.rows.len().saturating_sub(1))
            }
            Key::Enter => {
                let row = panel.selected;
                return self.choose_row(row);
            }
            Key::Escape => self.close_panel(),
            _ => {}
        }
        Vec::new()
    }

    // ---- events

    /// Handles `event`, returning what the shell should do. (A window's
    /// recorder notes what it's given: [`crate::Window::record`].)
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let px = |v: f64| (v * PIXEL as f64).round() as i64;
        match event {
            Event::Key { key, modifiers } => return self.key(key, modifiers),
            Event::Text(text) => {
                self.composition.clear();
                self.insert_text(&text);
                self.reveal();
            }
            Event::Preedit(text) => {
                self.composition = text;
                self.relayout(false);
                self.reveal();
            }
            Event::Command(id) => return self.command(&id),
            Event::CommandWith(id, args) => return self.command_with(&id, &args),
            Event::Pasted(text) => {
                self.paste(&text);
                self.reveal();
            }
            Event::Chosen(Choice::Open, Some(path)) => return vec![Effect::Open(Some(path))],
            Event::Chosen(Choice::MoveTo, Some(path)) => self.move_to_chosen(&path),
            Event::Chosen(choice, path) => {
                self.chosen(choice, path.as_deref());
                self.reveal();
            }
            Event::Theme { .. } | Event::Maximized(_) => {}
            Event::Focused => self.came_forward(),
            Event::Resize { width, height } => {
                let column = self.column();
                self.width = px(width).max(PIXEL);
                self.height = px(height).max(PIXEL);
                if self.view == View::Draft && self.column() != column {
                    self.relayout(true);
                }
                self.clamp_scroll();
            }
            Event::Scroll(dy) => {
                self.scroll += px(dy);
                self.clamp_scroll();
            }
            Event::Press { x, y, shift, count } => return self.press(px(x), px(y), shift, count),
            Event::Move { x, y } => self.pointer_move(px(x), px(y)),
            Event::Release => self.dragging = false,
        }
        Vec::new()
    }

    fn press(&mut self, x: i64, y: i64, shift: bool, count: u32) -> Vec<Effect> {
        if y < self.top() || y >= self.top() + self.view_height() {
            return Vec::new();
        }
        // A task's box, clicked, ticks or unticks it.
        if count <= 1
            && !shift
            && self.writable()
            && let Some((item, done)) = self.task_box_at(x, y)
        {
            self.check(item, !done);
            return Vec::new();
        }
        let target = self.hit(x, y);
        match count {
            0 | 1 => {
                self.go(target, shift);
                self.dragged = None;
            }
            2 => self.select_unit(target, Unit::Word),
            _ => self.select_unit(target, Unit::Sentence),
        }
        self.dragging = true;
        Vec::new()
    }

    fn pointer_move(&mut self, x: i64, y: i64) {
        if self.dragging {
            let target = self.hit(x, y);
            if self.dragged.is_some() {
                self.drag_unit(target);
            } else {
                self.go(target, true);
            }
        }
    }

    // ---- drawing

    /// The window came forward: a change another program made to the
    /// document's file, if it's a Markdown file's, is read or asked about.
    pub fn came_forward(&mut self) {
        self.markdown_came_forward();
    }

    /// Writes what's waiting to be written of the document's file at
    /// once, as closing it does: a Markdown file's, which is otherwise
    /// written a moment after an edit. An `.xw` file has each edit written
    /// as it's made.
    pub fn flush(&mut self) {
        self.flush_markdown();
    }

    /// What the work done for the document away from the window has to
    /// say, said, as a Markdown file's writing failing, or another
    /// program's change to it, asked about: whether there was anything,
    /// so the window is drawn again. The shell asks before it waits.
    pub fn poll(&mut self) -> bool {
        self.poll_markdown()
    }

    /// Its area of the window as a display list, in layout units from the
    /// area's top left, with its size: 9 525 to a logical pixel. The
    /// window draws the chrome around it.
    pub fn view(&self) -> scene::Page {
        crate::view::draw(self, &view_look(&self.res, false))
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
            units: &self.laid.units,
            blocks: &self.laid.blocks,
            pieces: &self.laid.pages,
            pictures: &self.pictures,
            selection: (!self.collapsed()).then(|| {
                let a = self.cursor(start);
                let b = self.cursor(end);
                (a.0, a.1.offset, b.0, b.1.offset)
            }),
            caret: self.collapsed().then(|| self.caret_rect()),
            composition: (!self.composition.is_empty()).then(|| {
                let (k, c) = self.cursor(start);
                (k, c.offset, c.offset + self.composition.len())
            }),
            draft: self.view == View::Draft,
        }
    }

    /// Whether direct formatting is tinted in the text (View, Show Direct
    /// Formatting).
    pub(crate) fn shows_formatting(&self) -> bool {
        self.show_formatting
    }

    /// What of laid-out paragraph `k` is its own formatting: the byte
    /// ranges of its text that set their own character values, and
    /// whether it sets paragraph values of its own.
    pub(crate) fn direct_formatting(&self, k: usize) -> (Vec<std::ops::Range<usize>>, bool) {
        let p = self.paragraph(self.laid.paragraphs[k].id);
        let whole = story::styles::PARAGRAPH_FORMATTING
            .iter()
            .any(|key| p.props.contains_key(*key));
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let mut at = 0;
        let mut chars = p.text.chars();
        for run in &p.runs {
            let len: usize = chars
                .by_ref()
                .take(run.len as usize)
                .map(char::len_utf8)
                .sum();
            if story::styles::CHARACTER_FORMATTING
                .iter()
                .any(|key| run.props.contains_key(*key))
            {
                match ranges.last_mut() {
                    Some(last) if last.end == at => last.end = at + len,
                    _ => ranges.push(at..at + len),
                }
            }
            at += len;
        }
        (ranges, whole)
    }

    /// How tall the draft's text is, for drawing what's in view.
    pub(crate) fn draft_bottom(&self) -> i64 {
        self.draft_height()
    }

    pub(crate) fn origin_of_page(&self, n: usize) -> (i64, i64) {
        let (x, y) = self.page_origin(n);
        (x, self.top() - self.scroll + y)
    }

    pub(crate) fn line_origin_of(&self, k: usize, line: usize) -> (i64, i64) {
        self.line_origin(k, line)
    }

    /// What the status bar says: the page, or that it's a draft, the
    /// style and size at the caret, and the document's words.
    pub fn status(&self) -> String {
        let (k, cursor) = self.cursor(self.focus);
        let line = self.laid.paragraphs[k].layout.line_of(cursor);
        let page = self.laid.places[k][line].0 + 1;
        let words: usize = self.laid.words.iter().sum();
        let p = self.paragraph(self.focus.paragraph);
        let style = &self.sheet.style_of(p).name;
        let place = match self.view {
            View::Pages => format!("Page {page} of {}", self.laid.pages.len()),
            View::Draft => "Draft".to_string(),
        };
        format!(
            "{place}   ·   {style}   ·   {} pt   ·   {words} words",
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
    pub units: &'a [std::ops::Range<usize>],
    pub blocks: &'a [canvas::Block],
    pub pieces: &'a [Vec<Piece>],
    pub pictures: &'a canvas::Pictures,
    /// The selection, as (paragraph, byte) at its start and end.
    pub selection: Option<(usize, usize, usize, usize)>,
    pub caret: Option<(i64, i64, i64)>,
    /// The composition: its paragraph and byte range.
    pub composition: Option<(usize, usize, usize)>,
    /// Whether it's a draft, on the desk, rather than pages.
    pub draft: bool,
}
