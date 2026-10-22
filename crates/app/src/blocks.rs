//! Editing the story's tree: what Enter, Backspace, Delete and Tab do at
//! the edges of lists, quotes, code and tables, deleting a selection that
//! runs across blocks, and the commands that make lists, quotes and
//! tables. Each works out the operations on the document as it stands;
//! the editor commits them.
//!
//! The keys, as M2 has them:
//!
//! - **Enter** in a list item starts a new item with the text after the
//!   caret; in an empty item, it leaves the list, as Shift+Tab does. In a
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
    Props::from([(
        keys::STYLE.to_string(),
        Value::Text(sheet.paragraph_style(None).name.clone()),
    )])
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
            // the blocks after this paragraph.
            let list = doc.parent(parent)?;
            let new = b.fresh();
            b.push(Op::SplitParagraph {
                paragraph: p,
                offset: at.offset,
                new,
                new_props: paragraph.props.clone(),
            })?;
            let item = b.fresh();
            b.push(Op::InsertBlock {
                parent: list,
                after: Some(parent),
                objects: vec![(item, container(Kind::Item, list, Props::new()))],
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

/// What Backspace (`forward` false) or Delete does at the caret `at`, at
/// its paragraph's start or end, with nothing selected; `ids` are the
/// story's paragraphs in reading order, `k` its place among them. `None`
/// where nothing in the tree is in the way: M1's join.
pub(crate) fn edge(doc: &Document, ids: &[ObjectId], k: usize, forward: bool) -> Option<Outcome> {
    let p = *ids.get(k)?;
    let parent = doc.parent(p)?;
    let first = doc.children(parent).first() == Some(&p);
    if forward {
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
fn between(doc: &Document, s: ObjectId, e: ObjectId) -> Vec<ObjectId> {
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

/// A table of two columns, a header and one row, after the caret's
/// paragraph, or after its table if it's in one; before it if it's empty,
/// so typing goes on after the table. The caret goes to its first cell.
pub(crate) fn insert_table(doc: &Document, at: Position) -> Option<Outcome> {
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
    for _ in 0..2 {
        let row = b.fresh();
        rows.push(row);
        let row_at = objects.len();
        objects.push((row, container(Kind::Row, table, Props::new())));
        let mut cells = Vec::new();
        for _ in 0..2 {
            let cell = b.fresh();
            first.get_or_insert(cell);
            cells.push(cell);
            objects.push((cell, empty_paragraph(row, Props::new())));
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

    fn sheet() -> StyleSheet {
        let source = testkit::specs("styles")
            .expect("spec/styles")
            .into_iter()
            .find(|(name, _)| name == "m1")
            .expect("m1.ron")
            .1;
        ron::from_str(&source).expect("a style sheet")
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
        story::build(&[
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
        ])
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
