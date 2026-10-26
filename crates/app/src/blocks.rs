//! Editing the story's tree: what Enter, Backspace, Delete and Tab do at
//! the edges of lists, quotes, code and tables, deleting a selection that
//! runs across blocks, the commands that make lists, quotes and tables,
//! and pasting the blocks Copy kept ([`paste`]). Each works out the
//! operations on the document as it stands; the editor commits them.
//!
//! The keys, as M2 has them:
//!
//! - **Enter** in a list item starts a new item with the text after the
//!   caret, a task with its box unticked after a task; in an empty item,
//!   it leaves the list, as Shift+Tab does. In a
//!   quote's last paragraph, empty, it leaves the quote. In code it breaks
//!   the line, and on an empty last line it leaves the code. In a table's
//!   cell it goes to the cell below, making a row at the end.
//! - **Backspace** at the start of a list item's first paragraph takes it
//!   out a level, as Shift+Tab does, and at the start of a quote's first
//!   paragraph takes it out of the quote. Elsewhere it joins the paragraph
//!   to the one before, wherever that is, but never into or out of a
//!   table's cell. **Delete** at a paragraph's end joins the next one in.
//! - **Tab** in a list item nests it in the item before; **Shift+Tab**
//!   takes it out a level, out of the list at the top. In a table they go
//!   to the next and the previous cell, Tab making a row after the last.

use std::collections::{BTreeSet, HashMap};

use doc::{Block, Document, Kind, Object, ObjectId, Op, Paragraph, Props, Value};
use story::{StyleSheet, keys};

use crate::Position;

/// What a key or command does to the document.
pub(crate) enum Outcome {
    /// Operations to commit as one transaction, and where the caret goes
    /// after.
    Edit {
        /// The transaction's name, for Undo.
        name: &'static str,
        /// The operations.
        ops: Vec<Op>,
        /// Where the caret goes.
        caret: Position,
    },
    /// Only the caret moves.
    Go(Position),
    /// Nothing happens.
    Nothing,
}

/// Operations worked out one after another on a copy of the document, so
/// each sees what those before it did.
struct Ops {
    doc: Document,
    ops: Vec<Op>,
}

impl Ops {
    fn new(doc: &Document) -> Ops {
        Ops {
            doc: doc.clone(),
            ops: Vec::new(),
        }
    }

    /// Adds `op`, if it applies.
    fn push(&mut self, op: Op) -> Option<()> {
        self.doc.apply(&op).ok()?;
        self.ops.push(op);
        Some(())
    }

    fn fresh(&mut self) -> ObjectId {
        self.doc.new_id()
    }

    fn edit(self, name: &'static str, caret: Position) -> Outcome {
        Outcome::Edit {
            name,
            ops: self.ops,
            caret,
        }
    }

    /// Deletes what's left empty above `from`, a container something left:
    /// the highest container that holds nothing but empty containers,
    /// short of the story.
    fn tidy(&mut self, from: ObjectId) -> Option<()> {
        let root = self.doc.root();
        let mut empty = None;
        let mut at = Some(from);
        while let Some(x) = at
            && x != root
            && self.doc.block(x).is_some()
            && self.doc.children(x).iter().all(|&c| Some(c) == empty)
        {
            empty = Some(x);
            at = self.doc.parent(x);
        }
        match empty {
            Some(block) => self.push(Op::DeleteBlock { block }),
            None => Some(()),
        }
    }
}

fn start(paragraph: ObjectId) -> Position {
    Position {
        paragraph,
        offset: 0,
        upstream: false,
    }
}

fn end(doc: &Document, paragraph: ObjectId) -> Position {
    Position {
        paragraph,
        offset: doc.paragraph(paragraph).map_or(0, Paragraph::len),
        upstream: true,
    }
}

/// The kind of container `id` is, if it's one.
fn kind(doc: &Document, id: ObjectId) -> Option<Kind> {
    doc.block(id).map(|b| b.kind)
}

/// The kind of container `id` is in.
fn parent_kind(doc: &Document, id: ObjectId) -> Option<Kind> {
    doc.parent(id).and_then(|p| kind(doc, p))
}

fn is_cell(doc: &Document, paragraph: ObjectId) -> bool {
    parent_kind(doc, paragraph) == Some(Kind::Row)
}

fn container(kind: Kind, parent: ObjectId, props: Props) -> Object {
    Object::Block(Block {
        kind,
        parent,
        children: Vec::new(),
        props,
    })
}

fn empty_paragraph(parent: ObjectId, props: Props) -> Object {
    Object::Paragraph(Paragraph {
        parent,
        text: String::new(),
        runs: Vec::new(),
        props,
    })
}

/// The default paragraph style, as a paragraph's properties.
fn body(sheet: &StyleSheet) -> Props {
    sheet
        .default_style()
        .id
        .map(|id| Props::from([(keys::STYLE.to_string(), Value::Ref(id))]))
        .unwrap_or_default()
}

fn is_code(doc: &Document, sheet: &StyleSheet, paragraph: ObjectId) -> bool {
    doc.paragraph(paragraph)
        .is_some_and(|p| sheet.style_of(p).code)
}

/// Moves each of `blocks`, in order, into `parent`, the first after
/// `after`; returns the last.
fn move_all(
    b: &mut Ops,
    blocks: &[ObjectId],
    parent: ObjectId,
    mut after: Option<ObjectId>,
) -> Option<Option<ObjectId>> {
    for &block in blocks {
        b.push(Op::MoveBlock {
            block,
            parent,
            after,
        })?;
        after = Some(block);
    }
    Some(after)
}

/// Takes list item `item` out a level: into the list around its own,
/// after the item that holds it, or, at the top, out of the list, its
/// blocks where it was. The items after it stay together: nested in it,
/// or as a list of their own after it, numbered on.
fn lift_item(b: &mut Ops, item: ObjectId) -> Option<()> {
    let list = b.doc.parent(item)?;
    let outer = b.doc.parent(list)?;
    let siblings = b.doc.children(list).to_vec();
    let at = siblings.iter().position(|&c| c == item)?;
    let rest = siblings[at + 1..].to_vec();
    let mut props = b.doc.block(list)?.props.clone();
    if kind(&b.doc, outer) == Some(Kind::Item) {
        let outer_list = b.doc.parent(outer)?;
        b.push(Op::MoveBlock {
            block: item,
            parent: outer_list,
            after: Some(outer),
        })?;
        if !rest.is_empty() {
            props.remove(keys::START);
            let nested = b.fresh();
            b.push(Op::InsertBlock {
                parent: item,
                after: b.doc.children(item).last().copied(),
                objects: vec![(nested, container(Kind::List, item, props))],
            })?;
            move_all(b, &rest, nested, None)?;
        }
    } else {
        let blocks = b.doc.children(item).to_vec();
        if at == 0 {
            // The first item: its blocks go before the list, which keeps
            // the rest.
            move_all(b, &blocks, outer, b.doc.previous(list))?;
        } else {
            let last = move_all(b, &blocks, outer, Some(list))?;
            if !rest.is_empty() {
                if let Some(Value::Bool(true)) = props.get(keys::ORDERED) {
                    let first = match props.get(keys::START) {
                        Some(Value::Int(n)) => *n,
                        _ => 1,
                    };
                    props.insert(keys::START.to_string(), Value::Int(first + at as i64));
                }
                let next = b.fresh();
                b.push(Op::InsertBlock {
                    parent: outer,
                    after: last,
                    objects: vec![(next, container(Kind::List, outer, props))],
                })?;
                move_all(b, &rest, next, None)?;
            }
        }
        b.push(Op::DeleteBlock { block: item })?;
    }
    if b.doc.children(list).is_empty() {
        b.push(Op::DeleteBlock { block: list })?;
    }
    Some(())
}

/// Nests list item `item` in the item before it, at the end of its list
/// of the same kind, made if there's none.
fn indent_item(b: &mut Ops, item: ObjectId) -> Option<()> {
    let list = b.doc.parent(item)?;
    let previous = b.doc.previous(item)?;
    let props = b.doc.block(list)?.props.clone();
    let ordered = props.get(keys::ORDERED).cloned();
    let last = b.doc.children(previous).last().copied();
    let target = match last {
        Some(l)
            if b.doc.block(l).is_some_and(|block| {
                block.kind == Kind::List && block.props.get(keys::ORDERED).cloned() == ordered
            }) =>
        {
            l
        }
        _ => {
            let mut props = props;
            props.remove(keys::START);
            let nested = b.fresh();
            b.push(Op::InsertBlock {
                parent: previous,
                after: last,
                objects: vec![(nested, container(Kind::List, previous, props))],
            })?;
            nested
        }
    };
    b.push(Op::MoveBlock {
        block: item,
        parent: target,
        after: b.doc.children(target).last().copied(),
    })
}

/// Joins `next`, wherever it is, onto the end of `paragraph`: moved after
/// it first if it's elsewhere, and what that leaves empty deleted.
fn join(b: &mut Ops, paragraph: ObjectId, next: ObjectId) -> Option<()> {
    let home = b.doc.parent(next)?;
    if b.doc.next(paragraph) != Some(next) {
        b.push(Op::MoveBlock {
            block: next,
            parent: b.doc.parent(paragraph)?,
            after: Some(paragraph),
        })?;
    }
    let offset = b.doc.paragraph(paragraph)?.len();
    b.push(Op::JoinParagraphs {
        paragraph,
        offset,
        next,
    })?;
    b.tidy(home)
}

/// Where the caret goes in the table `cell` is in, a row down (or up), in
/// the same column; with `make`, going down past the last row makes one.
fn cell_below(doc: &Document, cell: ObjectId, down: bool, make: bool) -> Option<Outcome> {
    let row = doc.parent(cell)?;
    let table = doc.parent(row)?;
    let column = doc.children(row).iter().position(|&c| c == cell)?;
    let rows = doc.children(table);
    let at = rows.iter().position(|&r| r == row)?;
    let target = if down {
        rows.get(at + 1)
    } else {
        at.checked_sub(1).and_then(|i| rows.get(i))
    };
    match target {
        Some(&r) => {
            let cells = doc.children(r);
            let c = cells.get(column).or(cells.last())?;
            Some(Outcome::Go(end(doc, *c)))
        }
        None if down && make => {
            let mut b = Ops::new(doc);
            let new = new_row(&mut b, row)?;
            let c = b.doc.children(new).get(column).copied()?;
            Some(b.edit("New Row", start(c)))
        }
        None => Some(Outcome::Nothing),
    }
}

/// A row after `row`, as many cells as it, each aligned as the cell above.
fn new_row(b: &mut Ops, row: ObjectId) -> Option<ObjectId> {
    let table = b.doc.parent(row)?;
    let new = b.fresh();
    let mut objects = vec![(new, container(Kind::Row, table, Props::new()))];
    let mut cells = Vec::new();
    for &above in b.doc.children(row).to_vec().iter() {
        let mut props = Props::new();
        if let Some(align) = b.doc.paragraph(above)?.props.get(keys::ALIGN) {
            props.insert(keys::ALIGN.to_string(), align.clone());
        }
        let cell = b.fresh();
        cells.push(cell);
        objects.push((cell, empty_paragraph(new, props)));
    }
    if let Object::Block(r) = &mut objects[0].1 {
        r.children = cells;
    }
    b.push(Op::InsertBlock {
        parent: table,
        after: Some(row),
        objects,
    })?;
    Some(new)
}

/// What Enter does at the caret `at`, with nothing selected, where the
/// paragraph's place in the tree says; `None` where it splits the
/// paragraph as M1's Enter does, or breaks a code block's line.
pub(crate) fn enter(doc: &Document, sheet: &StyleSheet, at: Position) -> Option<Outcome> {
    let p = at.paragraph;
    let paragraph = doc.paragraph(p)?;
    let parent = doc.parent(p)?;
    if is_code(doc, sheet, p) {
        let len = paragraph.len();
        if paragraph.is_empty() {
            let mut b = Ops::new(doc);
            b.push(Op::SetParaProp {
                paragraph: p,
                key: keys::STYLE.to_string(),
                value: body(sheet).remove(keys::STYLE),
            })?;
            return Some(b.edit("Body", at));
        }
        if at.offset == len && paragraph.text.ends_with('\u{2028}') {
            // An empty last line: it goes, and a paragraph follows.
            let mut b = Ops::new(doc);
            b.push(Op::DeleteText {
                paragraph: p,
                offset: len - 1,
                len: 1,
            })?;
            let new = b.fresh();
            b.push(Op::SplitParagraph {
                paragraph: p,
                offset: len - 1,
                new,
                new_props: body(sheet),
            })?;
            return Some(b.edit("New Paragraph", start(new)));
        }
        return None;
    }
    match kind(doc, parent)? {
        Kind::Row => cell_below(doc, p, true, true),
        Kind::Item => {
            let mut b = Ops::new(doc);
            if paragraph.is_empty() && doc.children(parent) == [p] {
                lift_item(&mut b, parent)?;
                return Some(b.edit("Outdent", start(p)));
            }
            // A new item after this one, with the text after the caret and
            // the blocks after this paragraph; after a task, a task, its
            // box unticked.
            let list = doc.parent(parent)?;
            let new = b.fresh();
            b.push(Op::SplitParagraph {
                paragraph: p,
                offset: at.offset,
                new,
                new_props: paragraph.props.clone(),
            })?;
            let item = b.fresh();
            let mut props = Props::new();
            if doc.block(parent)?.props.contains_key(keys::TASK) {
                props.insert(keys::TASK.to_string(), Value::Bool(false));
            }
            b.push(Op::InsertBlock {
                parent: list,
                after: Some(parent),
                objects: vec![(item, container(Kind::Item, list, props))],
            })?;
            let children = b.doc.children(parent).to_vec();
            let from = children.iter().position(|&c| c == new)?;
            move_all(&mut b, &children[from..], item, None)?;
            Some(b.edit("New Item", start(new)))
        }
        Kind::Quote if paragraph.is_empty() && doc.children(parent).last() == Some(&p) => {
            let mut b = Ops::new(doc);
            b.push(Op::MoveBlock {
                block: p,
                parent: doc.parent(parent)?,
                after: Some(parent),
            })?;
            b.tidy(parent)?;
            Some(b.edit("New Paragraph", start(p)))
        }
        _ => None,
    }
}

/// Whether `id` is a rule across the text.
fn is_rule(doc: &Document, id: ObjectId) -> bool {
    kind(doc, id) == Some(Kind::Rule)
}

/// The rule right before paragraph `p` in reading order, if there's one:
/// the block before it, or the last block inside that, and so on.
fn rule_before(doc: &Document, p: ObjectId) -> Option<ObjectId> {
    let mut at = doc.previous(p)?;
    loop {
        if is_rule(doc, at) {
            return Some(at);
        }
        at = *doc.children(at).last()?;
    }
}

/// The rule right after paragraph `p` in reading order, if there's one:
/// the block after it, or after its container if it's the last there,
/// or the first block inside that, and so on.
fn rule_after(doc: &Document, p: ObjectId) -> Option<ObjectId> {
    let mut at = p;
    let next = loop {
        match doc.next(at) {
            Some(next) => break next,
            None => at = doc.parent(at).filter(|&c| c != doc.root())?,
        }
    };
    let mut at = next;
    loop {
        if is_rule(doc, at) {
            return Some(at);
        }
        at = *doc.children(at).first()?;
    }
}

/// What Backspace (`forward` false) or Delete does at the caret `at`, at
/// its paragraph's start or end, with nothing selected; `ids` are the
/// story's paragraphs in reading order, `k` its place among them. `None`
/// where nothing in the tree is in the way: M1's join. A rule right
/// before or after is deleted, as a character would be.
pub(crate) fn edge(doc: &Document, ids: &[ObjectId], k: usize, forward: bool) -> Option<Outcome> {
    let p = *ids.get(k)?;
    let parent = doc.parent(p)?;
    let first = doc.children(parent).first() == Some(&p);
    if forward {
        if !is_cell(doc, p)
            && let Some(rule) = rule_after(doc, p)
        {
            let mut b = Ops::new(doc);
            b.push(Op::DeleteBlock { block: rule })?;
            return Some(b.edit("Delete", end(doc, p)));
        }
        let Some(&next) = ids.get(k + 1) else {
            return Some(Outcome::Nothing);
        };
        if is_cell(doc, p) || is_cell(doc, next) {
            return Some(Outcome::Nothing);
        }
        if doc.next(p) == Some(next) {
            return None;
        }
        let mut b = Ops::new(doc);
        join(&mut b, p, next)?;
        return Some(b.edit("Delete", end(doc, p)));
    }
    match kind(doc, parent) {
        Some(Kind::Row) => {
            let previous = k.checked_sub(1).and_then(|i| ids.get(i));
            return Some(match previous {
                Some(&q) if is_cell(doc, q) && doc.parent(doc.parent(q)?) == doc.parent(parent) => {
                    Outcome::Go(end(doc, q))
                }
                _ => Outcome::Nothing,
            });
        }
        Some(Kind::Item) if first => {
            let mut b = Ops::new(doc);
            lift_item(&mut b, parent)?;
            return Some(b.edit("Outdent", start(p)));
        }
        Some(Kind::Quote) if first => {
            let mut b = Ops::new(doc);
            b.push(Op::MoveBlock {
                block: p,
                parent: doc.parent(parent)?,
                after: doc.previous(parent),
            })?;
            b.tidy(parent)?;
            return Some(b.edit("Delete", start(p)));
        }
        _ => {}
    }
    if let Some(rule) = rule_before(doc, p) {
        let mut b = Ops::new(doc);
        b.push(Op::DeleteBlock { block: rule })?;
        return Some(b.edit("Delete", start(p)));
    }
    let Some(&previous) = k.checked_sub(1).and_then(|i| ids.get(i)) else {
        return Some(Outcome::Nothing);
    };
    if is_cell(doc, previous) {
        // After a table: an empty paragraph goes; otherwise the caret
        // goes into the table.
        let mut b = Ops::new(doc);
        if doc.paragraph(p)?.is_empty() && b.push(Op::DeleteBlock { block: p }).is_some() {
            b.tidy(parent)?;
            return Some(b.edit("Delete", end(doc, previous)));
        }
        return Some(Outcome::Go(end(doc, previous)));
    }
    if doc.previous(p) == Some(previous) {
        return None;
    }
    let mut b = Ops::new(doc);
    join(&mut b, previous, p)?;
    Some(b.edit("Delete", end(doc, previous)))
}

/// The operations that delete from `from` to `to`, two places in the
/// story in reading order in different paragraphs: the ends of those
/// paragraphs, every block between, and the second joined onto the first.
/// Where either is a table's cell, the paragraphs between lose their text
/// and nothing is joined, so tables keep their shape.
pub(crate) fn deletion(
    doc: &Document,
    ids: &[ObjectId],
    from: Position,
    to: Position,
) -> Option<Vec<Op>> {
    let (s, e) = (from.paragraph, to.paragraph);
    let a = ids.iter().position(|&id| id == s)?;
    let z = ids.iter().position(|&id| id == e)?;
    let mut b = Ops::new(doc);
    let s_len = doc.paragraph(s)?.len();
    b.push(Op::DeleteText {
        paragraph: s,
        offset: from.offset,
        len: s_len - from.offset,
    })?;
    if is_cell(doc, s) || is_cell(doc, e) {
        for &id in &ids[a + 1..z] {
            let len = doc.paragraph(id)?.len();
            b.push(Op::DeleteText {
                paragraph: id,
                offset: 0,
                len,
            })?;
        }
        b.push(Op::DeleteText {
            paragraph: e,
            offset: 0,
            len: to.offset,
        })?;
        return Some(b.ops);
    }
    for block in between(doc, s, e) {
        b.push(Op::DeleteBlock { block })?;
    }
    b.push(Op::DeleteText {
        paragraph: e,
        offset: 0,
        len: to.offset,
    })?;
    join(&mut b, s, e)?;
    Some(b.ops)
}

/// The blocks wholly between paragraphs `s` and `e`, `s` before `e` in
/// reading order, as few as hold them all.
pub(crate) fn between(doc: &Document, s: ObjectId, e: ObjectId) -> Vec<ObjectId> {
    let path = |id: ObjectId| {
        let mut out = vec![id];
        let mut at = id;
        while let Some(p) = doc.parent(at) {
            out.push(p);
            at = p;
        }
        out.reverse();
        out
    };
    let (ps, pe) = (path(s), path(e));
    let common = ps.iter().zip(&pe).take_while(|(x, y)| x == y).count();
    let mut out = Vec::new();
    // Below the common container, after each of `s`'s ancestors.
    for &x in ps[common..].iter().skip(1) {
        let siblings = doc.children(doc.parent(x).unwrap_or(x));
        if let Some(i) = siblings.iter().position(|&c| c == x) {
            out.extend(&siblings[i + 1..]);
        }
    }
    // In the common container, between the two branches.
    if let (Some(&cs), Some(&ce), Some(&lca)) = (
        ps.get(common),
        pe.get(common),
        ps.get(common.saturating_sub(1)),
    ) {
        let siblings = doc.children(lca);
        if let (Some(i), Some(j)) = (
            siblings.iter().position(|&c| c == cs),
            siblings.iter().position(|&c| c == ce),
        ) && i < j
        {
            out.extend(&siblings[i + 1..j]);
        }
    }
    // Below the common container, before each of `e`'s ancestors.
    for &x in pe[common..].iter().skip(1) {
        let siblings = doc.children(doc.parent(x).unwrap_or(x));
        if let Some(i) = siblings.iter().position(|&c| c == x) {
            out.extend(&siblings[..i]);
        }
    }
    out
}

/// What Tab (`back` false) or Shift+Tab does with the caret at `at`; `None`
/// where it does nothing.
pub(crate) fn tab(doc: &Document, ids: &[ObjectId], at: Position, back: bool) -> Option<Outcome> {
    let p = at.paragraph;
    let parent = doc.parent(p)?;
    match kind(doc, parent)? {
        Kind::Row => {
            let k = ids.iter().position(|&id| id == p)?;
            let table = doc.parent(parent)?;
            let in_table = |id: &ObjectId| {
                is_cell(doc, *id) && doc.parent(*id).and_then(|r| doc.parent(r)) == Some(table)
            };
            let target = if back {
                k.checked_sub(1).and_then(|i| ids.get(i))
            } else {
                ids.get(k + 1)
            };
            match target.filter(|id| in_table(id)) {
                Some(&c) => Some(Outcome::Go(end(doc, c))),
                None if !back && doc.children(table).last() == Some(&parent) => {
                    let mut b = Ops::new(doc);
                    let row = new_row(&mut b, parent)?;
                    let first = b.doc.children(row).first().copied()?;
                    Some(b.edit("New Row", start(first)))
                }
                None => Some(Outcome::Nothing),
            }
        }
        Kind::Item if doc.children(parent).first() == Some(&p) => {
            let mut b = Ops::new(doc);
            if back {
                lift_item(&mut b, parent)?;
                Some(b.edit("Outdent", at))
            } else {
                indent_item(&mut b, parent)?;
                Some(b.edit("Indent", at))
            }
        }
        _ => None,
    }
}

/// The paragraphs from `from` to `to` in reading order that are in the
/// same container as `from`'s, not a table's row.
fn siblings_selected(
    doc: &Document,
    ids: &[ObjectId],
    from: ObjectId,
    to: ObjectId,
) -> Vec<ObjectId> {
    let Some(parent) = doc.parent(from) else {
        return Vec::new();
    };
    if kind(doc, parent) == Some(Kind::Row) {
        return Vec::new();
    }
    let a = ids.iter().position(|&id| id == from).unwrap_or(0);
    let z = ids.iter().position(|&id| id == to).unwrap_or(a);
    ids[a..=z.max(a)]
        .iter()
        .copied()
        .filter(|&id| doc.parent(id) == Some(parent))
        .collect()
}

/// Bullets or numbers (`ordered`) for the paragraphs from `from` to `to`:
/// in a list of the other kind, it becomes this kind; in one of this kind,
/// the paragraph's item leaves it; otherwise the paragraphs, those beside
/// the first, become a list, an item each.
pub(crate) fn toggle_list(
    doc: &Document,
    ids: &[ObjectId],
    from: Position,
    to: Position,
    ordered: bool,
) -> Option<Outcome> {
    let p = from.paragraph;
    let parent = doc.parent(p)?;
    let mut b = Ops::new(doc);
    let name = if ordered { "Numbers" } else { "Bullets" };
    if kind(doc, parent) == Some(Kind::Item) {
        let list = doc.parent(parent)?;
        let is = matches!(
            doc.block(list)?.props.get(keys::ORDERED),
            Some(Value::Bool(true))
        );
        if is == ordered {
            // Out of the list, all the way.
            while parent_kind(&b.doc, p) == Some(Kind::Item) {
                let item = b.doc.parent(p)?;
                lift_item(&mut b, item)?;
            }
        } else {
            b.push(Op::SetBlockProp {
                block: list,
                key: keys::ORDERED.to_string(),
                value: ordered.then_some(Value::Bool(true)),
            })?;
        }
        return Some(b.edit(name, from));
    }
    let paragraphs = siblings_selected(doc, ids, p, to.paragraph);
    if paragraphs.is_empty() {
        return Some(Outcome::Nothing);
    }
    let list = b.fresh();
    let mut props = Props::new();
    if ordered {
        props.insert(keys::ORDERED.to_string(), Value::Bool(true));
    }
    let mut objects = vec![(list, container(Kind::List, parent, props))];
    let mut items = Vec::new();
    for _ in &paragraphs {
        let item = b.fresh();
        items.push(item);
        objects.push((item, container(Kind::Item, list, Props::new())));
    }
    if let Object::Block(l) = &mut objects[0].1 {
        l.children = items.clone();
    }
    b.push(Op::InsertBlock {
        parent,
        after: doc.previous(paragraphs[0]),
        objects,
    })?;
    for (&paragraph, &item) in paragraphs.iter().zip(&items) {
        b.push(Op::MoveBlock {
            block: paragraph,
            parent: item,
            after: None,
        })?;
    }
    Some(b.edit(name, from))
}

/// A quote for the paragraphs from `from` to `to`, those beside the
/// first; or, in a quote, out of it, everything in it where it was.
pub(crate) fn toggle_quote(
    doc: &Document,
    ids: &[ObjectId],
    from: Position,
    to: Position,
) -> Option<Outcome> {
    let p = from.paragraph;
    let parent = doc.parent(p)?;
    let mut b = Ops::new(doc);
    if kind(doc, parent) == Some(Kind::Quote) {
        let children = doc.children(parent).to_vec();
        move_all(&mut b, &children, doc.parent(parent)?, Some(parent))?;
        b.push(Op::DeleteBlock { block: parent })?;
        return Some(b.edit("Quote", from));
    }
    let paragraphs = siblings_selected(doc, ids, p, to.paragraph);
    if paragraphs.is_empty() {
        return Some(Outcome::Nothing);
    }
    let quote = b.fresh();
    b.push(Op::InsertBlock {
        parent,
        after: doc.previous(paragraphs[0]),
        objects: vec![(quote, container(Kind::Quote, parent, Props::new()))],
    })?;
    move_all(&mut b, &paragraphs, quote, None)?;
    Some(b.edit("Quote", from))
}

/// A rule across the text at the caret `at`, Insert, Rule: before its
/// paragraph if the caret is at its start, or it's empty; otherwise the
/// paragraph is split there, the rule between its halves, as Enter
/// splits it. In a table's cell, it goes after the table, a paragraph
/// after it if there's none. The caret goes to the paragraph after it.
pub(crate) fn insert_rule(doc: &Document, sheet: &StyleSheet, at: Position) -> Option<Outcome> {
    let p = at.paragraph;
    let paragraph = doc.paragraph(p)?;
    let mut b = Ops::new(doc);
    let rule = b.fresh();
    let rule_in = |parent: ObjectId| container(Kind::Rule, parent, Props::new());
    if is_cell(doc, p) {
        let table = doc.parent(doc.parent(p)?)?;
        let parent = doc.parent(table)?;
        b.push(Op::InsertBlock {
            parent,
            after: Some(table),
            objects: vec![(rule, rule_in(parent))],
        })?;
        let caret = match doc.next(table).filter(|&n| doc.paragraph(n).is_some()) {
            Some(next) => next,
            None => {
                let new = b.fresh();
                b.push(Op::InsertBlock {
                    parent,
                    after: Some(rule),
                    objects: vec![(new, empty_paragraph(parent, body(sheet)))],
                })?;
                new
            }
        };
        return Some(b.edit("Rule", start(caret)));
    }
    let parent = doc.parent(p)?;
    if at.offset == 0 || paragraph.is_empty() {
        b.push(Op::InsertBlock {
            parent,
            after: doc.previous(p),
            objects: vec![(rule, rule_in(parent))],
        })?;
        return Some(b.edit("Rule", start(p)));
    }
    // After a heading's last character comes the default style, as Enter
    // has it.
    let mut props = paragraph.props.clone();
    if at.offset == paragraph.len() && sheet.style_of(paragraph).heading.is_some() {
        props.extend(body(sheet));
    }
    let new = b.fresh();
    b.push(Op::SplitParagraph {
        paragraph: p,
        offset: at.offset,
        new,
        new_props: props,
    })?;
    b.push(Op::InsertBlock {
        parent,
        after: Some(p),
        objects: vec![(rule, rule_in(parent))],
    })?;
    Some(b.edit("Rule", start(new)))
}

/// What a paste puts in, as Copy kept it (`editor::clip`): the blocks
/// copied, as the story of a document of their own, their styles already
/// the destination's; its first and last paragraphs; and whether each of
/// those was copied in part, so joins the text it lands beside.
pub(crate) struct Slice<'a> {
    /// The blocks, in its story, inside the containers around them.
    pub(crate) blocks: &'a Document,
    /// The first paragraph copied.
    pub(crate) first: ObjectId,
    /// The last paragraph copied.
    pub(crate) last: ObjectId,
    /// Whether the first paragraph's text joins the text before the caret.
    pub(crate) open_start: bool,
    /// Whether the last paragraph's text joins the text after the caret.
    pub(crate) open_end: bool,
}

/// Where blocks go: in a container, after a child of it, or first.
#[derive(Clone, Copy)]
struct Point {
    parent: ObjectId,
    after: Option<ObjectId>,
}

/// Leaves `paragraph` out of what a paste puts in, with the containers
/// around it that it leaves with nothing to put in.
fn leave_out(from: &Document, paragraph: ObjectId, out: &mut BTreeSet<ObjectId>) {
    out.insert(paragraph);
    let mut at = from.parent(paragraph);
    while let Some(x) = at
        && x != from.root()
        && from.children(x).iter().all(|c| out.contains(c))
    {
        out.insert(x);
        at = from.parent(x);
    }
}

/// `block` of `from` and everything in it but what's `left` out, as
/// objects of `b`'s document by IDs fresh there, in reading order, the
/// block first, its parent `parent`: as Insert Block takes them. `last`
/// becomes the last paragraph among them, if there's one.
fn copy_of(
    b: &mut Ops,
    from: &Document,
    block: ObjectId,
    parent: ObjectId,
    left: &BTreeSet<ObjectId>,
    last: &mut Option<ObjectId>,
) -> Vec<(ObjectId, Object)> {
    let mut objects: Vec<(ObjectId, Object)> = Vec::new();
    let mut index: HashMap<ObjectId, usize> = HashMap::new();
    let mut stack = vec![(block, parent)];
    while let Some((id, parent)) = stack.pop() {
        let mut object = match from.object(id) {
            Some(o @ (Object::Paragraph(_) | Object::Block(_))) => o.clone(),
            _ => continue,
        };
        let new = b.fresh();
        match &mut object {
            Object::Paragraph(p) => {
                p.parent = parent;
                *last = Some(new);
            }
            Object::Block(block) => {
                block.parent = parent;
                block.children = Vec::new();
            }
            _ => {}
        }
        if let Some(&at) = index.get(&parent)
            && let Object::Block(holder) = &mut objects[at].1
        {
            holder.children.push(new);
        }
        index.insert(new, objects.len());
        objects.push((new, object));
        stack.extend(
            from.children(id)
                .iter()
                .rev()
                .filter(|c| !left.contains(c))
                .map(|&c| (c, new)),
        );
    }
    objects
}

/// Puts a copy of `block` of `from` in at `point`, which moves on past it.
fn put(
    b: &mut Ops,
    point: &mut Point,
    from: &Document,
    block: ObjectId,
    left: &BTreeSet<ObjectId>,
    last: &mut Option<ObjectId>,
) -> Option<()> {
    let objects = copy_of(b, from, block, point.parent, left, last);
    let id = objects.first()?.0;
    b.push(Op::InsertBlock {
        parent: point.parent,
        after: point.after,
        objects,
    })?;
    point.after = Some(id);
    Some(())
}

/// Puts the items of `list`, a list of `from`, in the list around the
/// item `point` is in, at its level: after that item, which keeps what's
/// before the point and gives what's after it to an item of its own after
/// them, or before it if nothing is before the point. The point moves to
/// the end of the last item put in.
fn join_items(
    b: &mut Ops,
    point: &mut Point,
    from: &Document,
    list: ObjectId,
    left: &BTreeSet<ObjectId>,
    last: &mut Option<ObjectId>,
) -> Option<()> {
    let item = point.parent;
    let into = b.doc.parent(item)?;
    let children = b.doc.children(item).to_vec();
    let (mut after, rest) = match point.after {
        Some(a) => {
            let at = children.iter().position(|&c| c == a)?;
            (Some(item), children[at + 1..].to_vec())
        }
        // An empty item, as one just put in can be: after it.
        None if children.is_empty() => (Some(item), Vec::new()),
        None => (b.doc.previous(item), Vec::new()),
    };
    let mut put_last = None;
    for &it in from.children(list).iter().filter(|c| !left.contains(c)) {
        let objects = copy_of(b, from, it, into, left, last);
        let id = objects.first()?.0;
        b.push(Op::InsertBlock {
            parent: into,
            after,
            objects,
        })?;
        after = Some(id);
        put_last = Some(id);
    }
    let Some(last_item) = put_last else {
        return Some(());
    };
    if !rest.is_empty() {
        // What was after the point, in an item of its own, as Enter makes
        // one: after a task, a task, its box unticked.
        let new = b.fresh();
        let mut props = Props::new();
        if b.doc.block(item)?.props.contains_key(keys::TASK) {
            props.insert(keys::TASK.to_string(), Value::Bool(false));
        }
        b.push(Op::InsertBlock {
            parent: into,
            after: Some(last_item),
            objects: vec![(new, container(Kind::Item, into, props))],
        })?;
        move_all(b, &rest, new, None)?;
    }
    *point = Point {
        parent: last_item,
        after: b.doc.children(last_item).last().copied(),
    };
    Some(())
}

/// Pastes `slice` at the caret `at`, in a paragraph that isn't code or a
/// table's cell, the selection already gone: the operations, and where
/// the caret goes after, at the end of what was pasted.
///
/// - A paragraph copied in part is text: its runs go in at the caret, and
///   the paragraph keeps its style.
/// - Otherwise the first paragraph, if it was copied in part, joins the
///   text before the caret, and the last the text after it, each taking
///   that paragraph's style; the blocks between come as they were, with
///   their containers: before the caret's paragraph at its start, after
///   it at its end, and between its halves in its middle. An empty
///   paragraph is replaced by them.
/// - Blocks nest where the caret is, but a list in a list item: its items
///   join the item's list at its level. A quote in a quote is one quote.
pub(crate) fn paste(
    doc: &Document,
    at: Position,
    slice: &Slice<'_>,
) -> Option<(Vec<Op>, Position)> {
    let from = slice.blocks;
    let x = at.paragraph;
    let c = at.offset;
    if is_cell(doc, x) {
        return None;
    }
    let mut b = Ops::new(doc);
    let text_of = |id: ObjectId| from.paragraph(id).map(|p| (p.text.clone(), p.runs.clone()));
    let insert =
        |b: &mut Ops, paragraph: ObjectId, offset: u32, (text, runs): (String, Vec<doc::Run>)| {
            let len = text.chars().count() as u32;
            if len > 0 {
                b.push(Op::InsertText {
                    paragraph,
                    offset,
                    text,
                    runs,
                })?;
            }
            Some(len)
        };
    if slice.first == slice.last && (slice.open_start || slice.open_end) {
        let len = insert(&mut b, x, c, text_of(slice.first)?)?;
        let caret = Position {
            paragraph: x,
            offset: c + len,
            upstream: c + len > 0,
        };
        return Some((b.ops, caret));
    }
    let mut left = BTreeSet::new();
    let head = if slice.open_start {
        leave_out(from, slice.first, &mut left);
        Some(text_of(slice.first)?)
    } else {
        None
    };
    let tail = if slice.open_end {
        leave_out(from, slice.last, &mut left);
        Some(text_of(slice.last)?)
    } else {
        None
    };
    let paragraph = doc.paragraph(x)?;
    let has_head = c > 0 || head.is_some();
    let has_tail = c < paragraph.len() || tail.is_some();
    let mut point = Point {
        parent: doc.parent(x)?,
        after: if has_head { Some(x) } else { doc.previous(x) },
    };
    // The caret's paragraph split, its second half after the first.
    let after_caret = if has_head && has_tail {
        let new = b.fresh();
        b.push(Op::SplitParagraph {
            paragraph: x,
            offset: c,
            new,
            new_props: paragraph.props.clone(),
        })?;
        Some(new)
    } else {
        has_tail.then_some(x)
    };
    let mut caret = Position {
        paragraph: x,
        offset: c,
        upstream: c > 0,
    };
    if let Some(head) = head {
        caret.offset += insert(&mut b, x, c, head)?;
        caret.upstream = caret.offset > 0;
    }
    let mut last = None;
    for &block in from.children(from.root()) {
        if left.contains(&block) {
            continue;
        }
        let into = kind(&b.doc, point.parent);
        match (kind(from, block), into) {
            (Some(Kind::List), Some(Kind::Item)) => {
                join_items(&mut b, &mut point, from, block, &left, &mut last)?;
            }
            (Some(Kind::Quote), Some(Kind::Quote)) => {
                for &child in from.children(block) {
                    if !left.contains(&child) {
                        put(&mut b, &mut point, from, child, &left, &mut last)?;
                    }
                }
            }
            _ => put(&mut b, &mut point, from, block, &left, &mut last)?,
        }
    }
    if let Some(p) = last {
        caret = end(&b.doc, p);
    }
    if let (Some(tail), Some(t)) = (tail, after_caret) {
        let len = insert(&mut b, t, 0, tail)?;
        caret = Position {
            paragraph: t,
            offset: len,
            upstream: len > 0,
        };
    }
    if !has_head && !has_tail {
        // An empty paragraph, replaced.
        let home = b.doc.parent(x)?;
        b.push(Op::DeleteBlock { block: x })?;
        b.tidy(home)?;
    }
    Some((b.ops, caret))
}

/// A table of two columns, a header and one row, after the caret's
/// paragraph, or after its table if it's in one; before it if it's empty,
/// so typing goes on after the table. The header's cells are in the
/// sheet's Table Header, as headings are in theirs (review sjj5ak5a), the
/// row's in no style of their own. The caret goes to its first cell.
pub(crate) fn insert_table(doc: &Document, sheet: &StyleSheet, at: Position) -> Option<Outcome> {
    let mut place = at.paragraph;
    while matches!(parent_kind(doc, place), Some(Kind::Row | Kind::Table)) {
        place = doc.parent(place)?;
    }
    let parent = doc.parent(place)?;
    let after = if doc.paragraph(place).is_some_and(Paragraph::is_empty) {
        doc.previous(place)
    } else {
        Some(place)
    };
    let mut b = Ops::new(doc);
    let table = b.fresh();
    let mut objects = vec![(table, container(Kind::Table, parent, Props::new()))];
    let mut rows = Vec::new();
    let mut first = None;
    let header = sheet
        .table_header()
        .and_then(|s| s.id)
        .map(|id| Props::from([(keys::STYLE.to_string(), Value::Ref(id))]))
        .unwrap_or_default();
    for index in 0..2 {
        let row = b.fresh();
        rows.push(row);
        let row_at = objects.len();
        objects.push((row, container(Kind::Row, table, Props::new())));
        let mut cells = Vec::new();
        for _ in 0..2 {
            let cell = b.fresh();
            first.get_or_insert(cell);
            cells.push(cell);
            let props = if index == 0 {
                header.clone()
            } else {
                Props::new()
            };
            objects.push((cell, empty_paragraph(row, props)));
        }
        if let Object::Block(r) = &mut objects[row_at].1 {
            r.children = cells;
        }
    }
    if let Object::Block(t) = &mut objects[0].1 {
        t.children = rows;
    }
    b.push(Op::InsertBlock {
        parent,
        after,
        objects,
    })?;
    Some(b.edit("Table", start(first?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use story::BlockSpec;

    /// Neutral, the stylesheet documents take unless the person chooses.
    fn neutral() -> story::SheetSpec {
        let source = testkit::specs("styles")
            .expect("spec/styles")
            .into_iter()
            .find(|(name, _)| name == "neutral")
            .expect("neutral.ron")
            .1;
        story::SheetSpec::parse(&source).expect("a stylesheet")
    }

    fn sheet() -> StyleSheet {
        StyleSheet::from_spec(&neutral())
    }

    fn p(text: &str) -> BlockSpec {
        BlockSpec::Paragraph("Body".into(), vec![(text.into(), Props::new())])
    }

    fn at(doc: &Document, text: &str, offset: u32) -> Position {
        let paragraph = doc
            .paragraphs()
            .into_iter()
            .find(|&id| doc.paragraph(id).is_some_and(|q| q.text == text))
            .expect("the paragraph");
        Position {
            paragraph,
            offset,
            upstream: false,
        }
    }

    /// The outline of `doc` after deleting from `from` to `to`.
    fn delete(doc: &Document, from: Position, to: Position) -> Vec<String> {
        let ids = doc.paragraphs();
        let ops = deletion(doc, &ids, from, to).expect("the deletion works out");
        let mut after = doc.clone();
        after.apply_all(&ops).expect("it applies");
        story::outline(&after, &sheet())
    }

    fn nested() -> Document {
        story::build(
            &neutral(),
            &[
                p("alpha"),
                BlockSpec::List(
                    false,
                    vec![
                        vec![p("one"), BlockSpec::List(false, vec![vec![p("deep")]])],
                        vec![p("two")],
                    ],
                ),
                BlockSpec::Quote(vec![p("q1"), p("q2")]),
                p("omega"),
            ],
        )
    }

    #[test]
    fn a_deletion_from_deep_in_a_list_into_a_quote() {
        let doc = nested();
        assert_eq!(
            delete(&doc, at(&doc, "one", 1), at(&doc, "q2", 1)),
            [
                "Body: alpha",
                "list",
                "  item",
                "    Body: o2",
                "Body: omega"
            ]
        );
    }

    #[test]
    fn a_deletion_from_a_paragraph_into_a_nested_item() {
        let doc = nested();
        assert_eq!(
            delete(&doc, at(&doc, "alpha", 2), at(&doc, "deep", 2)),
            [
                "Body: alep",
                "list",
                "  item",
                "    Body: two",
                "quote",
                "  Body: q1",
                "  Body: q2",
                "Body: omega"
            ]
        );
    }

    #[test]
    fn a_deletion_from_a_quote_to_the_end() {
        let doc = nested();
        assert_eq!(
            delete(&doc, at(&doc, "q1", 0), at(&doc, "omega", 5)),
            [
                "Body: alpha",
                "list",
                "  item",
                "    Body: one",
                "    list",
                "      item",
                "        Body: deep",
                "  item",
                "    Body: two",
                "quote",
                "  Body: ",
            ]
        );
    }
}
