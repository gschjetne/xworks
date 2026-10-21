use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::idmap::IdMap;
use crate::props::{self, Props, Run};
use crate::{Error, ObjectId, Op};

/// A story: an ordered list of paragraphs, as M1 has it. Later tracks add
/// lists, tables and embedded parts as blocks of their own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Story {
    /// Its paragraphs, in order. There is always at least one.
    pub paragraphs: Vec<ObjectId>,
}

/// A paragraph: its text, the character properties along it, and its own
/// properties.
///
/// Its runs cover its text exactly, and are normalized: no empty runs, and
/// no two neighbours with equal properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paragraph {
    /// The story it belongs to.
    pub story: ObjectId,
    /// Its text.
    pub text: String,
    /// Its character properties, along the text.
    pub runs: Vec<Run>,
    /// Its own properties, such as its style.
    pub props: Props,
}

impl Paragraph {
    /// Its length, in Unicode scalar values.
    pub fn len(&self) -> u32 {
        props::total(&self.runs)
    }

    /// Whether it has no text.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// An object in a document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Object {
    /// A story.
    Story(Story),
    /// A paragraph.
    Paragraph(Paragraph),
}

/// A document: objects by ID, and the story at its root.
///
/// Cloning is O(1) and gives a snapshot: edits to either copy never show in
/// the other. Two documents are equal when their objects are, whatever
/// edits made them.
#[derive(Clone)]
pub struct Document {
    objects: IdMap<Arc<Object>>,
    root: ObjectId,
    next_id: u64,
}

impl PartialEq for Document {
    fn eq(&self, other: &Document) -> bool {
        self.root == other.root && self.objects.entries() == other.objects.entries()
    }
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.dump())
    }
}

impl Default for Document {
    fn default() -> Document {
        Document::new()
    }
}

/// A document as plain data, for storing: every object by ID, the root, and
/// the next ID to give out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The root story's ID.
    pub root: ObjectId,
    /// The next ID to give out; above every ID in use.
    pub next_id: u64,
    /// Every object, in ID order.
    pub objects: Vec<(ObjectId, Object)>,
}

/// Why a snapshot isn't a document.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    /// Two objects with one ID.
    #[error("{0} is two objects")]
    Duplicate(ObjectId),
    /// An ID at or above the next one to give out.
    #[error("{0} isn't below the next ID")]
    IdTooHigh(ObjectId),
    /// The root isn't a story.
    #[error("the root {0} isn't a story")]
    RootNotAStory(ObjectId),
    /// A story with no paragraphs.
    #[error("{0} has no paragraphs")]
    EmptyStory(ObjectId),
    /// A story lists something that isn't one of its paragraphs.
    #[error("{story} lists {paragraph}, which isn't a paragraph of it")]
    NotItsParagraph {
        /// The story.
        story: ObjectId,
        /// What it lists.
        paragraph: ObjectId,
    },
    /// A paragraph that no story lists, or that one lists twice.
    #[error("{0} isn't in its story exactly once")]
    Stray(ObjectId),
    /// A paragraph whose runs don't cover its text.
    #[error("{0}'s runs don't cover its text")]
    RunsMismatch(ObjectId),
}

impl Document {
    /// The document as plain data.
    pub fn to_snapshot(&self) -> Snapshot {
        Snapshot {
            root: self.root,
            next_id: self.next_id,
            objects: self
                .objects
                .entries()
                .into_iter()
                .map(|(id, object)| (id, Object::clone(object)))
                .collect(),
        }
    }

    /// The document `snapshot` describes, if it holds together: the root
    /// is a story, every story lists its own paragraphs, each once, every
    /// paragraph is in its story, and runs cover their text. Runs are
    /// normalized.
    pub fn from_snapshot(snapshot: Snapshot) -> Result<Document, SnapshotError> {
        let mut objects = IdMap::new();
        let mut ids = BTreeSet::new();
        for (id, object) in snapshot.objects {
            if !ids.insert(id) {
                return Err(SnapshotError::Duplicate(id));
            }
            if id.0 >= snapshot.next_id {
                return Err(SnapshotError::IdTooHigh(id));
            }
            let object = match object {
                Object::Paragraph(mut p) => {
                    if props::total(&p.runs) as usize != p.text.chars().count() {
                        return Err(SnapshotError::RunsMismatch(id));
                    }
                    props::normalize(&mut p.runs);
                    Object::Paragraph(p)
                }
                story => story,
            };
            objects.insert(id, Arc::new(object));
        }
        let document = Document {
            objects,
            root: snapshot.root,
            next_id: snapshot.next_id,
        };
        if document.story(document.root).is_none() {
            return Err(SnapshotError::RootNotAStory(document.root));
        }
        // Every paragraph is listed once, by its own story.
        let mut listed = BTreeSet::new();
        for (id, object) in document.objects.entries() {
            let Object::Story(story) = &**object else {
                continue;
            };
            if story.paragraphs.is_empty() {
                return Err(SnapshotError::EmptyStory(id));
            }
            for &p in &story.paragraphs {
                if document.paragraph(p).is_none_or(|q| q.story != id) {
                    return Err(SnapshotError::NotItsParagraph {
                        story: id,
                        paragraph: p,
                    });
                }
                if !listed.insert(p) {
                    return Err(SnapshotError::Stray(p));
                }
            }
        }
        for (id, object) in document.objects.entries() {
            if matches!(&**object, Object::Paragraph(_)) && !listed.contains(&id) {
                return Err(SnapshotError::Stray(id));
            }
        }
        Ok(document)
    }

    /// A document holding a story with one empty paragraph.
    pub fn new() -> Document {
        let root = ObjectId(0);
        let first = ObjectId(1);
        let mut objects = IdMap::new();
        objects.insert(
            root,
            Arc::new(Object::Story(Story {
                paragraphs: vec![first],
            })),
        );
        objects.insert(
            first,
            Arc::new(Object::Paragraph(Paragraph {
                story: root,
                text: String::new(),
                runs: Vec::new(),
                props: Props::new(),
            })),
        );
        Document {
            objects,
            root,
            next_id: 2,
        }
    }

    /// The root story's ID.
    pub fn root(&self) -> ObjectId {
        self.root
    }

    /// A fresh ID, never used in this document, for an object an operation
    /// will create.
    pub fn new_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        id
    }

    /// The object `id`, if there is one.
    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(id).map(|o| &**o)
    }

    /// The story `id`, if there is one.
    pub fn story(&self, id: ObjectId) -> Option<&Story> {
        match self.object(id)? {
            Object::Story(s) => Some(s),
            _ => None,
        }
    }

    /// The paragraph `id`, if there is one.
    pub fn paragraph(&self, id: ObjectId) -> Option<&Paragraph> {
        match self.object(id)? {
            Object::Paragraph(p) => Some(p),
            _ => None,
        }
    }

    /// The root story's paragraphs, in order.
    pub fn paragraphs(&self) -> &[ObjectId] {
        &self
            .story(self.root)
            .expect("the root is a story")
            .paragraphs
    }

    /// The document as text, for tests and diffs: each object by ID, a
    /// paragraph's runs and properties included. Equal documents dump
    /// identically.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "root {}", self.root);
        for (id, object) in self.objects.entries() {
            match &**object {
                Object::Story(s) => {
                    let _ = writeln!(out, "{id} story {:?}", s.paragraphs);
                }
                Object::Paragraph(p) => {
                    let _ = writeln!(
                        out,
                        "{id} paragraph in {} {:?} {:?}",
                        p.story, p.text, p.props
                    );
                    for run in &p.runs {
                        let _ = writeln!(out, "    {} {:?}", run.len, run.props);
                    }
                }
            }
        }
        out
    }

    fn paragraph_mut(&mut self, id: ObjectId) -> Result<&mut Paragraph, Error> {
        match self.objects.get_mut(id).map(Arc::make_mut) {
            Some(Object::Paragraph(p)) => Ok(p),
            _ => Err(Error::NoSuchParagraph(id)),
        }
    }

    fn story_mut(&mut self, id: ObjectId) -> &mut Story {
        match self.objects.get_mut(id).map(Arc::make_mut) {
            Some(Object::Story(s)) => s,
            _ => unreachable!("a paragraph's story exists"),
        }
    }

    fn check_range(&self, paragraph: ObjectId, offset: u32, len: u32) -> Result<&Paragraph, Error> {
        let p = self
            .paragraph(paragraph)
            .ok_or(Error::NoSuchParagraph(paragraph))?;
        let length = p.len();
        if offset.checked_add(len).is_none_or(|end| end > length) {
            return Err(Error::OutOfRange {
                paragraph,
                offset,
                len,
                length,
            });
        }
        Ok(p)
    }

    /// Applies `op`. Returns the operations that undo it, to be applied in
    /// order; nothing changes if it fails.
    pub fn apply(&mut self, op: &Op) -> Result<Vec<Op>, Error> {
        match op {
            Op::InsertText {
                paragraph,
                offset,
                text,
                runs,
            } => {
                self.check_range(*paragraph, *offset, 0)?;
                let chars = text.chars().count() as u32;
                if props::total(runs) != chars {
                    return Err(Error::RunsMismatch {
                        text: chars,
                        runs: props::total(runs),
                    });
                }
                if chars == 0 {
                    return Ok(Vec::new());
                }
                let p = self.paragraph_mut(*paragraph)?;
                let at = byte_index(&p.text, *offset);
                p.text.insert_str(at, text);
                p.runs = props::splice(&p.runs, *offset, 0, runs);
                Ok(vec![Op::DeleteText {
                    paragraph: *paragraph,
                    offset: *offset,
                    len: chars,
                }])
            }
            Op::DeleteText {
                paragraph,
                offset,
                len,
            } => {
                self.check_range(*paragraph, *offset, *len)?;
                if *len == 0 {
                    return Ok(Vec::new());
                }
                let p = self.paragraph_mut(*paragraph)?;
                let (a, b) = (
                    byte_index(&p.text, *offset),
                    byte_index(&p.text, offset + len),
                );
                let removed: String = p.text.drain(a..b).collect();
                let removed_runs = props::slice(&p.runs, *offset, *len);
                p.runs = props::splice(&p.runs, *offset, *len, &[]);
                Ok(vec![Op::InsertText {
                    paragraph: *paragraph,
                    offset: *offset,
                    text: removed,
                    runs: removed_runs,
                }])
            }
            Op::SplitParagraph {
                paragraph,
                offset,
                new,
                new_props,
            } => {
                let story = self.check_range(*paragraph, *offset, 0)?.story;
                if self.objects.get(*new).is_some() {
                    return Err(Error::IdInUse(*new));
                }
                let p = self.paragraph_mut(*paragraph)?;
                let length = p.len();
                let tail = p.text.split_off(byte_index(&p.text, *offset));
                let tail_runs = props::slice(&p.runs, *offset, length - offset);
                p.runs = props::slice(&p.runs, 0, *offset);
                self.objects.insert(
                    *new,
                    Arc::new(Object::Paragraph(Paragraph {
                        story,
                        text: tail,
                        runs: tail_runs,
                        props: new_props.clone(),
                    })),
                );
                let s = self.story_mut(story);
                let at = s
                    .paragraphs
                    .iter()
                    .position(|id| id == paragraph)
                    .expect("in its story");
                s.paragraphs.insert(at + 1, *new);
                self.next_id = self.next_id.max(new.0 + 1);
                Ok(vec![Op::JoinParagraphs {
                    paragraph: *paragraph,
                    offset: *offset,
                    next: *new,
                }])
            }
            Op::JoinParagraphs {
                paragraph,
                offset,
                next,
            } => {
                let p = self
                    .paragraph(*paragraph)
                    .ok_or(Error::NoSuchParagraph(*paragraph))?;
                let q = self.paragraph(*next).ok_or(Error::NoSuchParagraph(*next))?;
                if p.len() != *offset {
                    return Err(Error::OffsetMismatch {
                        paragraph: *paragraph,
                        offset: *offset,
                        length: p.len(),
                    });
                }
                let story = p.story;
                let s = self.story(story).expect("a paragraph's story exists");
                let at = s
                    .paragraphs
                    .iter()
                    .position(|id| id == paragraph)
                    .expect("in its story");
                if q.story != story || s.paragraphs.get(at + 1) != Some(next) {
                    return Err(Error::NotAdjacent {
                        paragraph: *paragraph,
                        next: *next,
                    });
                }
                let q = q.clone();
                let p = self.paragraph_mut(*paragraph)?;
                p.text.push_str(&q.text);
                p.runs.extend(q.runs.iter().cloned());
                props::normalize(&mut p.runs);
                self.objects.remove(*next);
                self.story_mut(story).paragraphs.remove(at + 1);
                Ok(vec![Op::SplitParagraph {
                    paragraph: *paragraph,
                    offset: *offset,
                    new: *next,
                    new_props: q.props,
                }])
            }
            Op::SetCharProp {
                paragraph,
                offset,
                len,
                key,
                value,
            } => {
                let p = self.check_range(*paragraph, *offset, *len)?;
                let previous = props::values(&p.runs, *offset, *len, key);
                let p = self.paragraph_mut(*paragraph)?;
                p.runs = props::set(&p.runs, *offset, *len, key, value);
                let mut inverse = Vec::new();
                let mut at = *offset;
                for (n, old) in previous {
                    inverse.push(Op::SetCharProp {
                        paragraph: *paragraph,
                        offset: at,
                        len: n,
                        key: key.clone(),
                        value: old,
                    });
                    at += n;
                }
                Ok(inverse)
            }
            Op::SetParaProp {
                paragraph,
                key,
                value,
            } => {
                let p = self.paragraph_mut(*paragraph)?;
                let previous = match value {
                    Some(v) => p.props.insert(key.clone(), v.clone()),
                    None => p.props.remove(key),
                };
                Ok(vec![Op::SetParaProp {
                    paragraph: *paragraph,
                    key: key.clone(),
                    value: previous,
                }])
            }
        }
    }

    /// Applies `ops` in order, all or none. Returns the operations that undo
    /// them all, to be applied in order.
    pub fn apply_all(&mut self, ops: &[Op]) -> Result<Vec<Op>, Error> {
        let mut inverses: Vec<Vec<Op>> = Vec::with_capacity(ops.len());
        for op in ops {
            match self.apply(op) {
                Ok(inverse) => inverses.push(inverse),
                Err(e) => {
                    for inverse in inverses.iter().rev() {
                        for undo in inverse {
                            self.apply(undo)
                                .expect("an inverse applies to what it undoes");
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(inverses.into_iter().rev().flatten().collect())
    }
}

/// The byte index of the `chars`th character of `text`; its length past the
/// end.
fn byte_index(text: &str, chars: u32) -> usize {
    text.char_indices()
        .nth(chars as usize)
        .map_or(text.len(), |(i, _)| i)
}
