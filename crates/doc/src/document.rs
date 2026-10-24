use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::idmap::IdMap;
use crate::props::{self, Props, Run};
use crate::{Error, ObjectId, Op};

/// A story: a tree of blocks. Its own children are paragraphs and
/// containers ([`Block`]), and containers hold more of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Story {
    /// Its blocks, in order. Somewhere among them there is always at least
    /// one paragraph.
    pub blocks: Vec<ObjectId>,
}

/// A paragraph: its text, the character properties along it, and its own
/// properties. Headings and code blocks are paragraphs too, set apart by
/// their style.
///
/// Its runs cover its text exactly, and are normalized: no empty runs, and
/// no two neighbours with equal properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paragraph {
    /// The container it's in: the story, or a block.
    pub parent: ObjectId,
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

/// What a container is, and so what it can hold. Files store kinds by
/// their variants' positions (spec/format/README.md): new kinds go at the
/// end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    /// A list: items only. Its properties say whether it's numbered, and
    /// from what.
    List,
    /// An item of a list: blocks, as a story holds them.
    Item,
    /// A quotation: blocks, as a story holds them.
    Quote,
    /// A table: rows only. The first row is its header.
    Table,
    /// A row of a table: its cells, each one paragraph.
    Row,
    /// A rule across the text, as Markdown's thematic break: it holds
    /// nothing.
    Rule,
}

impl Kind {
    /// Its name in dumps: "list", "item" and so on.
    pub fn name(self) -> &'static str {
        match self {
            Kind::List => "list",
            Kind::Item => "item",
            Kind::Quote => "quote",
            Kind::Table => "table",
            Kind::Row => "row",
            Kind::Rule => "rule",
        }
    }
}

/// A container of blocks: a list, an item, a quote, a table or a row,
/// which may be empty; or a rule, which is always empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// What it is.
    pub kind: Kind,
    /// The container it's in.
    pub parent: ObjectId,
    /// What it holds, in order.
    pub children: Vec<ObjectId>,
    /// Its properties, such as a list's numbering.
    pub props: Props,
}

/// What a style styles, or what a named part names (docs/ARCHITECTURE.md,
/// section 6). Files store kinds by their variants' positions
/// (spec/format/README.md): new kinds go at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StyleKind {
    /// A paragraph style, which paragraphs name.
    Paragraph,
    /// A character style, which runs of text name, as marks do.
    Character,
    /// A page style: a page's size and margins.
    Page,
    /// A named part: a face, its font family.
    Face,
    /// A named part: a colour.
    Colour,
    /// A named part: a layout of tab stops.
    Tabs,
    /// A named part: how a list is marked and indented.
    List,
}

impl StyleKind {
    /// Its name in dumps: "paragraph style", "face" and so on.
    pub fn name(self) -> &'static str {
        match self {
            StyleKind::Paragraph => "paragraph style",
            StyleKind::Character => "character style",
            StyleKind::Page => "page style",
            StyleKind::Face => "face",
            StyleKind::Colour => "colour",
            StyleKind::Tabs => "tab layout",
            StyleKind::List => "list definition",
        }
    }

    /// Whether it's a named part rather than a style: parts aren't based
    /// on anything; styles refer to them.
    pub fn is_part(self) -> bool {
        matches!(
            self,
            StyleKind::Face | StyleKind::Colour | StyleKind::Tabs | StyleKind::List
        )
    }
}

/// A style, or a named part styles refer to, in a document's stylesheet.
///
/// A style sets only what differs from the style it's based on, which is
/// of its own kind; parts are based on nothing. Its name and role are
/// among its properties, so renaming it is one edit and paragraphs, which
/// name it by ID, never notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// The stylesheet it's in.
    pub sheet: ObjectId,
    /// What it is.
    pub kind: StyleKind,
    /// The style it's based on, if it's based on one.
    pub base: Option<ObjectId>,
    /// What it sets: its name, its role, and its values.
    pub props: Props,
}

/// A document's stylesheet: its styles and parts, in order, and its
/// properties: the defaults every chain of styles ends in, and what the
/// whole document shares, such as where the sheet came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    /// Its styles and parts, in order.
    pub styles: Vec<ObjectId>,
    /// Its defaults, and the document's own settings.
    pub props: Props,
}

/// An object in a document. Files store objects by their variants'
/// positions (spec/format/README.md): new variants go at the end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Object {
    /// A story.
    Story(Story),
    /// A paragraph.
    Paragraph(Paragraph),
    /// A container.
    Block(Block),
    /// The stylesheet.
    Sheet(Sheet),
    /// A style or a named part.
    Style(Style),
}

impl Object {
    /// The container it's in: a block's, or a style's stylesheet; `None`
    /// for a story or the stylesheet.
    pub fn parent(&self) -> Option<ObjectId> {
        match self {
            Object::Story(_) | Object::Sheet(_) => None,
            Object::Paragraph(p) => Some(p.parent),
            Object::Block(b) => Some(b.parent),
            Object::Style(s) => Some(s.sheet),
        }
    }

    /// What it holds, in order: a container's blocks, or the stylesheet's
    /// styles; nothing for a paragraph or a style.
    pub fn children(&self) -> &[ObjectId] {
        match self {
            Object::Story(s) => &s.blocks,
            Object::Paragraph(_) | Object::Style(_) => &[],
            Object::Block(b) => &b.children,
            Object::Sheet(s) => &s.styles,
        }
    }

    /// Whether it holds blocks, or styles: a story, a container or the
    /// stylesheet.
    pub fn is_container(&self) -> bool {
        !matches!(self, Object::Paragraph(_) | Object::Style(_))
    }

    /// Whether it's part of a story: the story, a paragraph or a
    /// container, which the operations on blocks act on.
    fn in_story(&self) -> bool {
        matches!(
            self,
            Object::Story(_) | Object::Paragraph(_) | Object::Block(_)
        )
    }

    /// Whether it can hold `child`: a story, an item or a quote holds
    /// paragraphs, lists, quotes, tables and rules; a list holds items; a
    /// table holds rows; a row holds paragraphs; a rule holds nothing; and
    /// the stylesheet holds styles.
    pub fn holds(&self, child: &Object) -> bool {
        let flow = matches!(
            child,
            Object::Paragraph(_)
                | Object::Block(Block {
                    kind: Kind::List | Kind::Quote | Kind::Table | Kind::Rule,
                    ..
                })
        );
        match self {
            Object::Story(_) => flow,
            Object::Paragraph(_) | Object::Style(_) => false,
            Object::Sheet(_) => matches!(child, Object::Style(_)),
            Object::Block(b) => match b.kind {
                Kind::Item | Kind::Quote => flow,
                Kind::List => matches!(
                    child,
                    Object::Block(Block {
                        kind: Kind::Item,
                        ..
                    })
                ),
                Kind::Table => matches!(
                    child,
                    Object::Block(Block {
                        kind: Kind::Row,
                        ..
                    })
                ),
                Kind::Row => matches!(child, Object::Paragraph(_)),
                Kind::Rule => false,
            },
        }
    }

    fn set_parent(&mut self, parent: ObjectId) {
        match self {
            Object::Story(_) | Object::Sheet(_) | Object::Style(_) => {}
            Object::Paragraph(p) => p.parent = parent,
            Object::Block(b) => b.parent = parent,
        }
    }

    fn children_mut(&mut self) -> Option<&mut Vec<ObjectId>> {
        match self {
            Object::Story(s) => Some(&mut s.blocks),
            Object::Paragraph(_) | Object::Style(_) => None,
            Object::Block(b) => Some(&mut b.children),
            Object::Sheet(s) => Some(&mut s.styles),
        }
    }
}

/// A document: objects by ID, the story at its root, and its stylesheet,
/// if it has one.
///
/// Cloning is O(1) and gives a snapshot: edits to either copy never show in
/// the other. Two documents are equal when their objects are, whatever
/// edits made them.
#[derive(Clone)]
pub struct Document {
    objects: IdMap<Arc<Object>>,
    root: ObjectId,
    next_id: u64,
    /// How many paragraphs it has.
    paragraphs: usize,
    /// Its stylesheet, if it has one.
    sheet: Option<ObjectId>,
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
    /// A story or container lists something that isn't a child of it: no
    /// object, or one whose parent is another.
    #[error("{story} lists {paragraph}, which isn't a child of it")]
    NotItsParagraph {
        /// The story or container.
        story: ObjectId,
        /// What it lists.
        paragraph: ObjectId,
    },
    /// An object that nothing lists, that's listed twice, or that isn't in
    /// the root's tree.
    #[error("{0} isn't in the story exactly once")]
    Stray(ObjectId),
    /// A paragraph whose runs don't cover its text.
    #[error("{0}'s runs don't cover its text")]
    RunsMismatch(ObjectId),
    /// A container holding what it can't hold, such as a list holding a
    /// paragraph.
    #[error("{parent} can't hold {child}")]
    WrongKind {
        /// The container.
        parent: ObjectId,
        /// What it holds.
        child: ObjectId,
    },
    /// More than one stylesheet.
    #[error("{0} is a second stylesheet")]
    TwoSheets(ObjectId),
    /// A style based on what isn't a style of its kind, or a part based on
    /// anything.
    #[error("{style} can't be based on {base}")]
    WrongBase {
        /// The style.
        style: ObjectId,
        /// What it's based on.
        base: ObjectId,
    },
    /// A style based on itself, through a chain of bases.
    #[error("{0} is based on itself")]
    Loop(ObjectId),
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
    /// is a story; every container lists its own children, each once,
    /// each of a kind it can hold; there's at most one stylesheet, which
    /// lists its styles likewise; every other object is in the root's tree
    /// or the stylesheet; there's a paragraph; runs cover their text; and
    /// every style is based on a style of its kind, never on itself, and
    /// parts on nothing. Runs are normalized.
    pub fn from_snapshot(snapshot: Snapshot) -> Result<Document, SnapshotError> {
        let mut objects = IdMap::new();
        let mut ids = BTreeSet::new();
        let mut paragraphs = 0;
        let mut sheet = None;
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
                    paragraphs += 1;
                    Object::Paragraph(p)
                }
                Object::Sheet(s) => {
                    if sheet.replace(id).is_some() {
                        return Err(SnapshotError::TwoSheets(id));
                    }
                    Object::Sheet(s)
                }
                other => other,
            };
            objects.insert(id, Arc::new(object));
        }
        let document = Document {
            objects,
            root: snapshot.root,
            next_id: snapshot.next_id,
            paragraphs,
            sheet,
        };
        let Some(story) = document.story(document.root) else {
            return Err(SnapshotError::RootNotAStory(document.root));
        };
        if story.blocks.is_empty() {
            return Err(SnapshotError::EmptyStory(document.root));
        }
        // Every object is listed once, by its own parent, which can hold it.
        let mut listed = BTreeSet::new();
        for (id, object) in document.objects.entries() {
            for &c in object.children() {
                let Some(child) = document.object(c).filter(|o| o.parent() == Some(id)) else {
                    return Err(SnapshotError::NotItsParagraph {
                        story: id,
                        paragraph: c,
                    });
                };
                if !object.holds(child) {
                    return Err(SnapshotError::WrongKind {
                        parent: id,
                        child: c,
                    });
                }
                if !listed.insert(c) {
                    return Err(SnapshotError::Stray(c));
                }
            }
        }
        for &id in &ids {
            if id != document.root && Some(id) != document.sheet && !listed.contains(&id) {
                return Err(SnapshotError::Stray(id));
            }
        }
        // And every object is in the root's tree or the stylesheet: none
        // is in a ring of containers holding each other.
        let mut reached: BTreeSet<ObjectId> = document.subtree(document.root).into_iter().collect();
        if let Some(sheet) = document.sheet {
            if reached.contains(&sheet) {
                return Err(SnapshotError::Stray(sheet));
            }
            reached.extend(document.subtree(sheet));
        }
        if let Some(&stray) = ids.iter().find(|id| !reached.contains(id)) {
            return Err(SnapshotError::Stray(stray));
        }
        if document.paragraphs == 0 {
            return Err(SnapshotError::EmptyStory(document.root));
        }
        // Every style is based on one of its kind, parts on none, and no
        // chain comes back to where it started: each chain is walked until
        // it meets one already found to end, so it takes time in how many
        // styles there are.
        for &id in document.styles() {
            let style = document.style(id).expect("a stylesheet lists styles");
            if let Some(base) = style.base
                && !document.can_base(style.kind, base)
            {
                return Err(SnapshotError::WrongBase { style: id, base });
            }
        }
        let mut ends: BTreeSet<ObjectId> = BTreeSet::new();
        for &id in document.styles() {
            let mut path = BTreeSet::new();
            let mut at = Some(id);
            while let Some(x) = at {
                if ends.contains(&x) {
                    break;
                }
                if !path.insert(x) {
                    return Err(SnapshotError::Loop(x));
                }
                at = document.style(x).and_then(|s| s.base);
            }
            ends.extend(path);
        }
        Ok(document)
    }

    /// Whether a style of `kind` can be based on `base`: it's a style, not
    /// a part, and `base` is a style of the same kind.
    fn can_base(&self, kind: StyleKind, base: ObjectId) -> bool {
        !kind.is_part() && self.style(base).is_some_and(|b| b.kind == kind)
    }

    /// The document's stylesheet, if it has one.
    pub fn sheet(&self) -> Option<ObjectId> {
        self.sheet
    }

    /// The stylesheet itself, if there is one.
    pub fn stylesheet(&self) -> Option<&Sheet> {
        match self.object(self.sheet?)? {
            Object::Sheet(s) => Some(s),
            _ => None,
        }
    }

    /// The stylesheet's styles and parts, in order: none without one.
    pub fn styles(&self) -> &[ObjectId] {
        self.stylesheet().map_or(&[], |s| &s.styles)
    }

    /// The style or part `id`, if there is one.
    pub fn style(&self, id: ObjectId) -> Option<&Style> {
        match self.object(id)? {
            Object::Style(s) => Some(s),
            _ => None,
        }
    }

    /// Style `id` and the styles it's based on, in order, up its chain:
    /// one more than the stylesheet has styles if the chain loops, which
    /// no document's does. Empty if there's no such style.
    pub fn chain(&self, id: ObjectId) -> Vec<ObjectId> {
        let most = self.styles().len() + 1;
        let mut out = Vec::new();
        let mut at = self.style(id).map(|_| id);
        while let Some(x) = at {
            if out.len() >= most {
                break;
            }
            out.push(x);
            at = self.style(x).and_then(|s| s.base);
        }
        out
    }

    /// A document holding a story with one empty paragraph.
    pub fn new() -> Document {
        let root = ObjectId(0);
        let first = ObjectId(1);
        let mut objects = IdMap::new();
        objects.insert(
            root,
            Arc::new(Object::Story(Story {
                blocks: vec![first],
            })),
        );
        objects.insert(
            first,
            Arc::new(Object::Paragraph(Paragraph {
                parent: root,
                text: String::new(),
                runs: Vec::new(),
                props: Props::new(),
            })),
        );
        Document {
            objects,
            root,
            next_id: 2,
            paragraphs: 1,
            sheet: None,
        }
    }

    /// The objects that aren't the very ones `other` has, as where this
    /// document was edited since it was `other`: those either has and the
    /// other doesn't, or holds otherwise, or holds again as they were, in
    /// ID order. It takes time in how much differs, not in how much there
    /// is, as the two share all the rest.
    pub fn changed_since(&self, other: &Document) -> Vec<ObjectId> {
        self.objects.differing(&other.objects, Arc::ptr_eq)
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

    /// The container `id`, if there is one.
    pub fn block(&self, id: ObjectId) -> Option<&Block> {
        match self.object(id)? {
            Object::Block(b) => Some(b),
            _ => None,
        }
    }

    /// The container `id` is in, if it's in one.
    pub fn parent(&self, id: ObjectId) -> Option<ObjectId> {
        self.object(id)?.parent()
    }

    /// What `id` holds, in order: nothing if it's a paragraph or there's
    /// no such object.
    pub fn children(&self, id: ObjectId) -> &[ObjectId] {
        self.object(id).map_or(&[], Object::children)
    }

    /// The block before `id` in its container, if there's one.
    pub fn previous(&self, id: ObjectId) -> Option<ObjectId> {
        let siblings = self.children(self.parent(id)?);
        let at = siblings.iter().position(|&c| c == id)?;
        at.checked_sub(1).map(|i| siblings[i])
    }

    /// The block after `id` in its container, if there's one.
    pub fn next(&self, id: ObjectId) -> Option<ObjectId> {
        let siblings = self.children(self.parent(id)?);
        let at = siblings.iter().position(|&c| c == id)?;
        siblings.get(at + 1).copied()
    }

    /// Whether `id` is `ancestor` or inside it.
    pub fn contains(&self, ancestor: ObjectId, id: ObjectId) -> bool {
        let mut at = Some(id);
        while let Some(x) = at {
            if x == ancestor {
                return true;
            }
            at = self.parent(x);
        }
        false
    }

    /// `id` and everything inside it, in reading order.
    pub fn subtree(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            if self.object(x).is_none() {
                continue;
            }
            out.push(x);
            stack.extend(self.children(x).iter().rev());
        }
        out
    }

    /// Every paragraph of the story, in reading order: headings, list
    /// items' paragraphs and tables' cells, row by row, included.
    pub fn paragraphs(&self) -> Vec<ObjectId> {
        let mut out = Vec::with_capacity(self.paragraphs);
        let mut stack = vec![self.root];
        while let Some(x) = stack.pop() {
            match self.object(x) {
                Some(Object::Paragraph(_)) => out.push(x),
                Some(object) => stack.extend(object.children().iter().rev()),
                None => {}
            }
        }
        out
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
                    let _ = writeln!(out, "{id} story {:?}", s.blocks);
                }
                Object::Paragraph(p) => {
                    let _ = writeln!(
                        out,
                        "{id} paragraph in {} {:?} {:?}",
                        p.parent, p.text, p.props
                    );
                    for run in &p.runs {
                        let _ = writeln!(out, "    {} {:?}", run.len, run.props);
                    }
                }
                Object::Block(b) => {
                    let _ = writeln!(
                        out,
                        "{id} {} in {} {:?} {:?}",
                        b.kind.name(),
                        b.parent,
                        b.children,
                        b.props
                    );
                }
                Object::Sheet(s) => {
                    let _ = writeln!(out, "{id} sheet {:?} {:?}", s.styles, s.props);
                }
                Object::Style(s) => {
                    let base = s
                        .base
                        .map_or_else(String::new, |b| format!(" based on {b}"));
                    let _ = writeln!(
                        out,
                        "{id} {} in {}{base} {:?}",
                        s.kind.name(),
                        s.sheet,
                        s.props
                    );
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

    /// The children of container `id`, which exists.
    fn children_mut(&mut self, id: ObjectId) -> &mut Vec<ObjectId> {
        self.objects
            .get_mut(id)
            .map(Arc::make_mut)
            .and_then(Object::children_mut)
            .expect("a container")
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

    /// Where a block placed after `after` in container `parent` goes, as
    /// an index into its children.
    fn place(&self, parent: ObjectId, after: Option<ObjectId>) -> Result<usize, Error> {
        let container = self
            .object(parent)
            .filter(|o| o.is_container())
            .ok_or(Error::NoSuchBlock(parent))?;
        match after {
            None => Ok(0),
            Some(a) => container
                .children()
                .iter()
                .position(|&c| c == a)
                .map(|i| i + 1)
                .ok_or(Error::NotAChild { parent, child: a }),
        }
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
                let parent = self.check_range(*paragraph, *offset, 0)?.parent;
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
                        parent,
                        text: tail,
                        runs: tail_runs,
                        props: new_props.clone(),
                    })),
                );
                let siblings = self.children_mut(parent);
                let at = siblings
                    .iter()
                    .position(|id| id == paragraph)
                    .expect("in its container");
                siblings.insert(at + 1, *new);
                self.next_id = self.next_id.max(new.0 + 1);
                self.paragraphs += 1;
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
                let parent = p.parent;
                let siblings = self.children(parent);
                let at = siblings
                    .iter()
                    .position(|id| id == paragraph)
                    .expect("in its container");
                if q.parent != parent || siblings.get(at + 1) != Some(next) {
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
                self.children_mut(parent).remove(at + 1);
                self.paragraphs -= 1;
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
            Op::InsertBlock {
                parent,
                after,
                objects,
            } => self.insert_block(*parent, *after, objects),
            Op::DeleteBlock { block } => self.delete_block(*block),
            Op::MoveBlock {
                block,
                parent,
                after,
            } => self.move_block(*block, *parent, *after),
            Op::SetBlockProp { block, key, value } => {
                let b = match self.objects.get_mut(*block).map(Arc::make_mut) {
                    Some(Object::Block(b)) => b,
                    _ => return Err(Error::NoSuchBlock(*block)),
                };
                let previous = match value {
                    Some(v) => b.props.insert(key.clone(), v.clone()),
                    None => b.props.remove(key),
                };
                Ok(vec![Op::SetBlockProp {
                    block: *block,
                    key: key.clone(),
                    value: previous,
                }])
            }
            Op::InsertStyle {
                sheet,
                after,
                id,
                style,
            } => self.insert_style(*sheet, *after, *id, style),
            Op::DeleteStyle { style } => self.delete_style(*style),
            Op::SetStyleProp { style, key, value } => {
                let props = match self.objects.get_mut(*style).map(Arc::make_mut) {
                    Some(Object::Style(s)) => &mut s.props,
                    Some(Object::Sheet(s)) => &mut s.props,
                    _ => return Err(Error::NoSuchStyle(*style)),
                };
                let previous = match value {
                    Some(v) => props.insert(key.clone(), v.clone()),
                    None => props.remove(key),
                };
                Ok(vec![Op::SetStyleProp {
                    style: *style,
                    key: key.clone(),
                    value: previous,
                }])
            }
            Op::SetBase { style, base } => self.set_base(*style, *base),
        }
    }

    /// Whether basing `style` on `base` would base it on itself.
    fn loops(&self, style: ObjectId, base: ObjectId) -> bool {
        self.chain(base).contains(&style)
    }

    fn insert_style(
        &mut self,
        sheet: ObjectId,
        after: Option<ObjectId>,
        id: ObjectId,
        style: &Style,
    ) -> Result<Vec<Op>, Error> {
        if self.sheet != Some(sheet) {
            return Err(Error::NoSuchStyle(sheet));
        }
        if style.sheet != sheet {
            return Err(Error::Malformed(id));
        }
        if self.objects.get(id).is_some() {
            return Err(Error::IdInUse(id));
        }
        if let Some(base) = style.base
            && !self.can_base(style.kind, base)
        {
            return Err(Error::WrongBase { style: id, base });
        }
        let at = self.place(sheet, after)?;
        self.objects
            .insert(id, Arc::new(Object::Style(style.clone())));
        self.children_mut(sheet).insert(at, id);
        self.next_id = self.next_id.max(id.0 + 1);
        Ok(vec![Op::DeleteStyle { style: id }])
    }

    fn delete_style(&mut self, id: ObjectId) -> Result<Vec<Op>, Error> {
        let style = self.style(id).ok_or(Error::NoSuchStyle(id))?.clone();
        if let Some(&by) = self
            .styles()
            .iter()
            .find(|&&s| self.style(s).is_some_and(|s| s.base == Some(id)))
        {
            return Err(Error::BasedOn { style: id, by });
        }
        let after = self.previous(id);
        self.children_mut(style.sheet).retain(|&s| s != id);
        self.objects.remove(id);
        Ok(vec![Op::InsertStyle {
            sheet: style.sheet,
            after,
            id,
            style,
        }])
    }

    fn set_base(&mut self, id: ObjectId, base: Option<ObjectId>) -> Result<Vec<Op>, Error> {
        let style = self.style(id).ok_or(Error::NoSuchStyle(id))?;
        let kind = style.kind;
        if let Some(b) = base {
            if !self.can_base(kind, b) {
                return Err(Error::WrongBase { style: id, base: b });
            }
            if self.loops(id, b) {
                return Err(Error::Loop { style: id, base: b });
            }
        }
        let Some(Object::Style(style)) = self.objects.get_mut(id).map(Arc::make_mut) else {
            return Err(Error::NoSuchStyle(id));
        };
        let previous = std::mem::replace(&mut style.base, base);
        Ok(vec![Op::SetBase {
            style: id,
            base: previous,
        }])
    }

    fn insert_block(
        &mut self,
        parent: ObjectId,
        after: Option<ObjectId>,
        objects: &[(ObjectId, Object)],
    ) -> Result<Vec<Op>, Error> {
        if !self.object(parent).is_some_and(Object::in_story) {
            return Err(Error::NoSuchBlock(parent));
        }
        let at = self.place(parent, after)?;
        let Some((root, top)) = objects.first() else {
            return Err(Error::Malformed(parent));
        };
        let container = self.object(parent).expect("placed in it");
        if !container.holds(top) {
            return Err(Error::WrongKind {
                parent,
                child: *root,
            });
        }
        if top.parent() != Some(parent) {
            return Err(Error::Malformed(*root));
        }
        // Fresh IDs, each once, and runs that cover their text.
        let mut set: BTreeMap<ObjectId, &Object> = BTreeMap::new();
        for (id, object) in objects {
            if self.objects.get(*id).is_some() {
                return Err(Error::IdInUse(*id));
            }
            if set.insert(*id, object).is_some()
                || !object.in_story()
                || matches!(object, Object::Story(_))
            {
                return Err(Error::Malformed(*root));
            }
            if let Object::Paragraph(p) = object {
                let chars = p.text.chars().count() as u32;
                if props::total(&p.runs) != chars {
                    return Err(Error::RunsMismatch {
                        text: chars,
                        runs: props::total(&p.runs),
                    });
                }
            }
        }
        // Each held once, by the container it names, which can hold it;
        // and all of them in the one block.
        let mut held = BTreeSet::new();
        for (id, object) in objects {
            for c in object.children() {
                let child = set.get(c).ok_or(Error::Malformed(*root))?;
                if child.parent() != Some(*id) || !held.insert(*c) {
                    return Err(Error::Malformed(*root));
                }
                if !object.holds(child) {
                    return Err(Error::WrongKind {
                        parent: *id,
                        child: *c,
                    });
                }
            }
        }
        // Nothing holds the block itself, so the walk from it ends: any
        // ring of containers would hold something twice.
        if held.contains(root) {
            return Err(Error::Malformed(*root));
        }
        let mut reached = 0;
        let mut stack = vec![*root];
        while let Some(x) = stack.pop() {
            reached += 1;
            stack.extend(set[&x].children());
        }
        if reached != objects.len() {
            return Err(Error::Malformed(*root));
        }
        for (id, object) in objects {
            let mut object = object.clone();
            if let Object::Paragraph(p) = &mut object {
                props::normalize(&mut p.runs);
                self.paragraphs += 1;
            }
            self.objects.insert(*id, Arc::new(object));
            self.next_id = self.next_id.max(id.0 + 1);
        }
        self.children_mut(parent).insert(at, *root);
        Ok(vec![Op::DeleteBlock { block: *root }])
    }

    fn delete_block(&mut self, block: ObjectId) -> Result<Vec<Op>, Error> {
        let object = self
            .object(block)
            .filter(|o| o.in_story())
            .ok_or(Error::NoSuchBlock(block))?;
        let parent = object.parent().ok_or(Error::Root(block))?;
        let ids = self.subtree(block);
        let paragraphs = ids
            .iter()
            .filter(|&&id| self.paragraph(id).is_some())
            .count();
        if paragraphs == self.paragraphs {
            return Err(Error::LastParagraph(block));
        }
        let after = self.previous(block);
        let objects: Vec<(ObjectId, Object)> = ids
            .iter()
            .map(|&id| (id, Object::clone(self.object(id).expect("in the tree"))))
            .collect();
        let siblings = self.children_mut(parent);
        siblings.retain(|&c| c != block);
        for id in ids {
            self.objects.remove(id);
        }
        self.paragraphs -= paragraphs;
        Ok(vec![Op::InsertBlock {
            parent,
            after,
            objects,
        }])
    }

    fn move_block(
        &mut self,
        block: ObjectId,
        parent: ObjectId,
        after: Option<ObjectId>,
    ) -> Result<Vec<Op>, Error> {
        let object = self
            .object(block)
            .filter(|o| o.in_story())
            .ok_or(Error::NoSuchBlock(block))?;
        let old_parent = object.parent().ok_or(Error::Root(block))?;
        let target = self
            .object(parent)
            .filter(|o| o.is_container() && o.in_story())
            .ok_or(Error::NoSuchBlock(parent))?;
        if !target.holds(object) {
            return Err(Error::WrongKind {
                parent,
                child: block,
            });
        }
        if self.contains(block, parent) {
            return Err(Error::Cycle { block, parent });
        }
        if after == Some(block) {
            return Err(Error::NotAChild {
                parent,
                child: block,
            });
        }
        self.place(parent, after)?;
        let old_after = self.previous(block);
        self.children_mut(old_parent).retain(|&c| c != block);
        let at = self.place(parent, after).expect("still there");
        self.children_mut(parent).insert(at, block);
        if old_parent != parent {
            Arc::make_mut(self.objects.get_mut(block).expect("moved")).set_parent(parent);
        }
        Ok(vec![Op::MoveBlock {
            block,
            parent: old_parent,
            after: old_after,
        }])
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
