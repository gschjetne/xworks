//! Copying and pasting with structure and styles (docs/ARCHITECTURE.md,
//! section 6): Copy puts the selection's text on the system's clipboard,
//! as ever, and keeps beside it, in the program, a fragment of the story's
//! tree: the stretches of the paragraphs copied, with their runs and own
//! properties, the blocks wholly between them, rules and tables among
//! them, and the containers around them all, with their own properties;
//! whether the first and last paragraphs were copied whole; and the
//! document it came from, for its stylesheet and pictures. Paste uses the
//! fragment while the clipboard still holds the text it was kept with, in
//! any window of this xWorks; text from anywhere else pastes as text.
//!
//! What a paste does with it (`blocks::paste`):
//!
//! - A paragraph copied in part is text: its runs go in at the caret, and
//!   the paragraph there keeps its style. So is anything pasted into code
//!   or a table's cell, which take text as text, as before.
//! - A paragraph copied whole comes as a paragraph, in its own style, in
//!   the containers it was in: a quote's paragraphs in a quote, a list's
//!   items in a list of their kind with their nesting, a table as a table
//!   and a rule as a rule. A copy that begins or ends partway into a
//!   container brings the container around what it copied: one item pastes
//!   as a list of one item, and rows as a table of those rows, whose first
//!   row's cells take Table Header, as Insert Table makes them.
//! - The first and last paragraphs, copied in part, join the text before
//!   and after the caret, in that paragraph's style.
//! - Pasted inside a container, blocks nest there, but for list items
//!   pasted into a list, which join it at the caret's level, and a quote
//!   pasted into a quote, whose blocks join it.
//!
//! The destination's styles win (project owner, 2026-10-24, review
//! 30dmn1js): a pasted paragraph's style, and its text's character styles
//! and parts, are matched with the destination's by role and then by name,
//! and those it uses that the destination lacks come along with their
//! chains, as `story::merge_used` brings them. Nothing is renamed "Body 2".
//!
//! Pictures come along: in the same document as they are; from an `.xw`
//! file with their bytes, which Copy reads from its file, kept in the
//! destination's; from a Markdown file by the files they link to, kept in
//! an `.xw` file, or linked from where the destination is. A Markdown file
//! can't hold a picture an `.xw` file keeps, so it asks, as for anything
//! else it can't hold, and the picture comes along if the person makes it
//! an `.xw` file. A picture whose file can't be had stays behind, and the
//! status bar says so.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use doc::{Block, Document, Kind, Object, ObjectId, Paragraph, Props, Run, Snapshot, Story, Value};
use story::{OBJECT, keys};

use super::{Editor, byte};
use crate::{blocks, picture};

/// What Copy kept beside the text it put on the clipboard.
#[derive(Clone, Debug)]
pub(crate) struct Fragment {
    /// The text it put on the system's clipboard, by which a paste is
    /// known to be of it.
    text: String,
    /// What was copied, as a document of its own: its story holds the
    /// stretches of the paragraphs copied, the blocks between them and the
    /// containers around them, by their IDs where they were copied from;
    /// and its stylesheet is the document's.
    copied: Document,
    /// The first and last paragraphs copied.
    first: ObjectId,
    last: ObjectId,
    /// Whether the first paragraph was copied whole, from its start; and
    /// the last, to its end.
    first_whole: bool,
    last_whole: bool,
    /// The file of the document it came from.
    path: PathBuf,
    /// Whether that's a Markdown file.
    markdown: bool,
    /// The pictures it shows, where they can be had: from an `.xw` file,
    /// each picture its file keeps, by ID; from a Markdown file, the file
    /// each link names, by link.
    kept: Arc<HashMap<String, store::Image>>,
    linked: Arc<HashMap<String, PathBuf>>,
}

/// `text` with its line endings as a paste reads them.
fn lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `props` with references to the fragment's styles and parts turned into
/// the destination's, by `map`; a reference to anything else is left out.
fn mapped(props: &Props, map: &HashMap<ObjectId, ObjectId>) -> Props {
    props
        .iter()
        .filter_map(|(k, v)| match v {
            Value::Ref(id) => map.get(id).map(|to| (k.clone(), Value::Ref(*to))),
            other => Some((k.clone(), other.clone())),
        })
        .collect()
}

/// The references in `props`.
fn refs(props: &Props) -> impl Iterator<Item = ObjectId> + '_ {
    props.values().filter_map(|v| match v {
        Value::Ref(id) => Some(*id),
        _ => None,
    })
}

/// The picture a run's `props` show, by the link a Markdown file has it
/// by, or its ID in an `.xw` file.
fn picture_link(props: &Props) -> Option<&str> {
    match props.get(keys::PICTURE_SRC).or(props.get(keys::PICTURE)) {
        Some(Value::Text(link)) => Some(link),
        _ => None,
    }
}

/// `p`'s text from character `from` to `to`, with its runs.
fn stretch(p: &Paragraph, from: u32, to: u32) -> Paragraph {
    let to = to.max(from);
    let mut runs = Vec::new();
    let mut pos = 0;
    for run in &p.runs {
        let (a, b) = (pos.max(from), (pos + run.len).min(to));
        if a < b {
            runs.push(Run {
                len: b - a,
                props: run.props.clone(),
            });
        }
        pos += run.len;
    }
    Paragraph {
        parent: p.parent,
        text: p.text[byte(&p.text, from)..byte(&p.text, to)].to_string(),
        runs,
        props: p.props.clone(),
    }
}

/// What a paste brought of its pictures.
#[derive(Default)]
struct Brought {
    /// The pictures for the destination's file to keep.
    kept: Vec<store::Image>,
    /// How many stayed behind.
    left: usize,
}

impl Fragment {
    /// The styles and parts its blocks name.
    fn used(&self) -> BTreeSet<ObjectId> {
        let mut out = BTreeSet::new();
        let doc = &self.copied;
        for id in doc.subtree(doc.root()) {
            match doc.object(id) {
                Some(Object::Paragraph(p)) => {
                    out.extend(refs(&p.props));
                    for run in &p.runs {
                        out.extend(refs(&run.props));
                    }
                }
                Some(Object::Block(b)) => out.extend(refs(&b.props)),
                _ => {}
            }
        }
        out
    }

    /// Whether `id` is a table's cell in it.
    fn is_cell(&self, id: ObjectId) -> bool {
        self.copied
            .parent(id)
            .and_then(|r| self.copied.block(r))
            .is_some_and(|b| b.kind == Kind::Row)
    }
}

impl Editor {
    /// Keeps the selection as the program's fragment, beside `text`, what
    /// goes on the system's clipboard.
    pub(super) fn keep_fragment(&self, text: &str) {
        let Some(fragment) = self.fragment(text) else {
            return;
        };
        let mut clip = self
            .res
            .clip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *clip = Some(fragment);
    }

    /// The selection, as a fragment kept beside `text`.
    fn fragment(&self, text: &str) -> Option<Fragment> {
        let doc = &self.document;
        let (start, end) = self.ordered();
        let (first, last) = (start.paragraph, end.paragraph);
        let root = doc.root();
        // The blocks between, whole; the first and last paragraphs; and
        // the containers around them.
        let mut included: BTreeSet<ObjectId> = BTreeSet::new();
        if first != last {
            for block in blocks::between(doc, first, last) {
                included.extend(doc.subtree(block));
            }
        }
        for p in [first, last] {
            included.insert(p);
            let mut at = doc.parent(p);
            while let Some(x) = at
                && x != root
            {
                included.insert(x);
                at = doc.parent(x);
            }
        }
        // A row copied in part keeps its shape: its cells not copied come
        // empty.
        let mut emptied = BTreeSet::new();
        for p in [first, last] {
            let row = doc
                .parent(p)
                .filter(|&r| doc.block(r).is_some_and(|b| b.kind == Kind::Row));
            for &cell in row.map_or(&[][..], |r| doc.children(r)) {
                if included.insert(cell) {
                    emptied.insert(cell);
                }
            }
        }
        let held = |children: &[ObjectId]| -> Vec<ObjectId> {
            children
                .iter()
                .copied()
                .filter(|c| included.contains(c))
                .collect()
        };
        let mut objects = vec![(
            root,
            Object::Story(Story {
                blocks: held(doc.children(root)),
            }),
        )];
        let mut kept = HashMap::new();
        let mut linked = HashMap::new();
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for &id in &included {
            let object = match doc.object(id)? {
                Object::Paragraph(p) => {
                    let (from, to) = if emptied.contains(&id) {
                        (0, 0)
                    } else {
                        (
                            if id == first { start.offset } else { 0 },
                            if id == last { end.offset } else { p.len() },
                        )
                    };
                    let p = stretch(p, from, to);
                    for run in &p.runs {
                        self.keep_picture(&run.props, &folder, &mut kept, &mut linked);
                    }
                    Object::Paragraph(p)
                }
                Object::Block(b) => Object::Block(Block {
                    children: held(&b.children),
                    ..b.clone()
                }),
                _ => continue,
            };
            objects.push((id, object));
        }
        objects.extend(story::sheet_objects(doc));
        let next_id = objects.iter().map(|(id, _)| id.0).max().unwrap_or(0) + 1;
        let copied = Document::from_snapshot(Snapshot {
            root,
            next_id,
            objects,
        })
        .ok()?;
        let many = first != last;
        let last_len = doc.paragraph(last)?.len();
        Some(Fragment {
            text: lines(text),
            copied,
            first,
            last,
            first_whole: start.offset == 0 && (many || end.offset == last_len),
            last_whole: end.offset == last_len && end.offset > 0 && (many || start.offset == 0),
            path: self.path.clone(),
            markdown: self.is_markdown(),
            kept: Arc::new(kept),
            linked: Arc::new(linked),
        })
    }

    /// Keeps where the picture a run's `props` show can be had, if they
    /// show one: from the document's file, its bytes; from a Markdown
    /// file's `folder`, the file its link names.
    fn keep_picture(
        &self,
        props: &Props,
        folder: &Path,
        kept: &mut HashMap<String, store::Image>,
        linked: &mut HashMap<String, PathBuf>,
    ) {
        if !props.contains_key(keys::PICTURE) {
            return;
        }
        if self.is_markdown() {
            if let Some(link) = picture_link(props)
                && !linked.contains_key(link)
                && let Some(path) = super::markdown::resolve(folder, link).filter(|p| p.is_file())
            {
                let path = std::path::absolute(&path).unwrap_or(path);
                linked.insert(link.to_string(), path);
            }
        } else if let Some(Value::Text(id)) = props.get(keys::PICTURE)
            && !kept.contains_key(id)
            && let Some(Ok(Some(image))) = self.store.as_ref().map(|s| s.image(id))
        {
            kept.insert(id.clone(), image);
        }
    }

    /// Pastes the program's fragment in place of the selection, if the
    /// clipboard holds the text it was kept with, `text`. False if it
    /// doesn't, or the selection is in code or a table's cell, which take
    /// text as text.
    pub(super) fn paste_fragment(&mut self, text: &str) -> bool {
        let fragment = {
            let clip = self
                .res
                .clip
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match clip.as_ref() {
                Some(f) if f.text == lines(text) => f.clone(),
                _ => return false,
            }
        };
        let at = self.ordered().0.paragraph;
        let in_cell = self
            .document
            .parent(at)
            .and_then(|p| self.document.block(p))
            .is_some_and(|b| b.kind == Kind::Row);
        if self.is_code(at) || in_cell {
            return false;
        }
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return true;
        }
        let (styles, map) = story::merge_used(&mut scratch, &fragment.copied, &fragment.used());
        ops.extend(styles);
        let mut brought = Brought::default();
        let header = story::style_with_role(&scratch, story::TABLE_HEADER);
        let Some(pasted) = self.pasted(&fragment, &map, header, &mut brought) else {
            return true;
        };
        // A cell is never joined to text outside its table; a cell alone
        // is text.
        let alone = fragment.first == fragment.last;
        let text_only = |whole: bool, id: ObjectId| {
            if fragment.is_cell(id) { alone } else { !whole }
        };
        let slice = blocks::Slice {
            blocks: &pasted,
            first: fragment.first,
            last: fragment.last,
            open_start: text_only(fragment.first_whole, fragment.first),
            open_end: text_only(fragment.last_whole, fragment.last),
        };
        let Some((more, caret)) = blocks::paste(&scratch, at, &slice) else {
            return true;
        };
        ops.extend(more);
        let unkept = self.keep_brought(&mut brought);
        let committed = self.commit("Paste", ops, false);
        self.pasted_pictures.clear();
        if committed {
            self.go(caret, false);
            match (unkept, brought.left) {
                (Some(e), _) => self.message = format!("Couldn't keep a picture pasted: {e}"),
                (None, 0) => {}
                (None, 1) => {
                    self.message = "Pasted without a picture whose file can't be had".to_string();
                }
                (None, n) => {
                    self.message = format!("Pasted without {n} pictures whose files can't be had");
                }
            }
        }
        true
    }

    /// The fragment's blocks as they paste here: references to its styles
    /// and parts made this document's by `map`, and its pictures brought
    /// along, or left out, as [`Editor::bring_picture`] says. A table's
    /// first row's cells take this document's Table Header, `header`,
    /// where they name no style or the default one, as Insert Table makes
    /// them.
    fn pasted(
        &self,
        fragment: &Fragment,
        map: &HashMap<ObjectId, ObjectId>,
        header: Option<ObjectId>,
        brought: &mut Brought,
    ) -> Option<Document> {
        let from = &fragment.copied;
        let root = from.root();
        let body = self.sheet.default_style().id;
        let mut objects: Vec<(ObjectId, Object)> = Vec::new();
        let mut index: HashMap<ObjectId, usize> = HashMap::new();
        let mut tables = Vec::new();
        for id in from.subtree(root) {
            let object = match from.object(id)? {
                Object::Paragraph(p) => {
                    Object::Paragraph(self.pasted_paragraph(fragment, p, map, brought))
                }
                Object::Block(b) => {
                    if b.kind == Kind::Table {
                        tables.push(id);
                    }
                    Object::Block(Block {
                        props: mapped(&b.props, map),
                        ..b.clone()
                    })
                }
                other => other.clone(),
            };
            index.insert(id, objects.len());
            objects.push((id, object));
        }
        if let Some(header) = header {
            for table in tables {
                let row = from.children(table).first().copied();
                for &cell in row.map_or(&[][..], |r| from.children(r)) {
                    if let Some(&at) = index.get(&cell)
                        && let Object::Paragraph(p) = &mut objects[at].1
                    {
                        let style = match p.props.get(keys::STYLE) {
                            Some(Value::Ref(id)) => Some(*id),
                            _ => None,
                        };
                        if style.is_none() || style == body {
                            p.props.insert(keys::STYLE.to_string(), Value::Ref(header));
                        }
                    }
                }
            }
        }
        let next_id = objects.iter().map(|(id, _)| id.0).max().unwrap_or(0) + 1;
        Document::from_snapshot(Snapshot {
            root,
            next_id,
            objects,
        })
        .ok()
    }

    /// Paragraph `p` of the fragment as it pastes here: its own properties
    /// and its runs' mapped by `map`; its pictures brought along, or left
    /// out with their characters; and control characters but tabs and line
    /// breaks left out.
    fn pasted_paragraph(
        &self,
        fragment: &Fragment,
        p: &Paragraph,
        map: &HashMap<ObjectId, ObjectId>,
        brought: &mut Brought,
    ) -> Paragraph {
        let mut text = String::new();
        let mut runs: Vec<Run> = Vec::new();
        let mut letters = p.text.chars();
        for run in &p.runs {
            let piece: Vec<char> = letters.by_ref().take(run.len as usize).collect();
            let mut props = mapped(&run.props, map);
            let mut picture = false;
            if props.contains_key(keys::PICTURE) {
                let objects = piece.iter().filter(|&&c| c == OBJECT).count();
                match self.bring_picture(fragment, &props, brought) {
                    Some(with) => {
                        props = with;
                        picture = true;
                    }
                    None => {
                        brought.left += objects;
                        for key in keys::PICTURES {
                            props.remove(key);
                        }
                    }
                }
            }
            let piece: String = piece
                .into_iter()
                .filter(|&c| {
                    (c != OBJECT || picture) && (!c.is_control() || c == '\t' || c == '\u{2028}')
                })
                .collect();
            let n = piece.chars().count() as u32;
            if n > 0 {
                text.push_str(&piece);
                runs.push(Run::new(n, props));
            }
        }
        Paragraph {
            parent: p.parent,
            text,
            runs,
            props: mapped(&p.props, map),
        }
    }

    /// A picture's run's `props` as they paste here, if it comes along:
    /// as they are in the document it came from; from an `.xw` file, kept
    /// in this one's file, or, in a Markdown file, which asks first, if
    /// it becomes one; from a Markdown file, linked from where this one is
    /// or, in an `.xw` file, kept by its hash. None if the picture's file
    /// can't be had.
    fn bring_picture(
        &self,
        fragment: &Fragment,
        props: &Props,
        brought: &mut Brought,
    ) -> Option<Props> {
        if fragment.path == self.path {
            return Some(props.clone());
        }
        let mut props = props.clone();
        if !fragment.markdown {
            let Some(Value::Text(id)) = props.get(keys::PICTURE) else {
                return None;
            };
            let image = fragment.kept.get(id)?;
            brought.kept.push(image.clone());
            // Its link, if it ever had one, is the other file's.
            props.remove(keys::PICTURE_SRC);
            return Some(props);
        }
        let link = picture_link(&props)?.to_string();
        let file = fragment.linked.get(&link);
        if self.is_markdown() {
            // Linked from here; a link that names no file here, such as a
            // web address, as it was.
            if let Some(file) = file {
                let folder = self
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                let (link, _) = super::markdown::link_to(&folder, file);
                props.insert(keys::PICTURE.to_string(), Value::Text(link.clone()));
                props.insert(keys::PICTURE_SRC.to_string(), Value::Text(link));
            }
            return Some(props);
        }
        let data = std::fs::read(file?).ok()?;
        let format = super::pictures::format_name(picture::format(&data)?);
        let id = store::image_id(&data);
        brought.kept.push(store::Image {
            id: id.clone(),
            format: format.to_string(),
            data,
        });
        props.insert(keys::PICTURE.to_string(), Value::Text(id));
        props.remove(keys::PICTURE_SRC);
        Some(props)
    }

    /// Keeps the pictures a paste `brought` in the document's file, before
    /// the paste is made, as Insert Picture does; in a Markdown file, which
    /// can't keep them, holds them until it's asked whether to become an
    /// `.xw` file, which then keeps them. Says why one couldn't be kept.
    fn keep_brought(&mut self, brought: &mut Brought) -> Option<store::Error> {
        let mut images = std::mem::take(&mut brought.kept);
        images.sort_by(|a, b| a.id.cmp(&b.id));
        images.dedup_by(|a, b| a.id == b.id);
        if self.is_markdown() {
            self.pasted_pictures = images;
            return None;
        }
        let store = self.store.as_mut()?;
        images
            .iter()
            .find_map(|image| store.add_image(&image.format, &image.data).err())
    }
}
