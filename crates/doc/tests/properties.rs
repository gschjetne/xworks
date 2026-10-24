//! Properties of the document core, from random documents and random edits
//! (docs/ROADMAP.md, M1 stage 4 and M2 stage 2):
//!
//! - every operation's inverse undoes it, blocks with everything in them
//!   included;
//! - rebasing converges: `a` then `b` rebased over it gives the same
//!   document as `b` then `a` rebased over it, for every pair of kinds;
//! - M1's operations never need the fallback for pairs that can't both
//!   have their way: the rules alone converge;
//! - a snapshot is unchanged by later edits;
//! - a transaction applies all or nothing, and undo and redo are exact;
//! - documents and operations survive serialization;
//! - styles are never based on themselves, through any chain, whatever
//!   bases are set, alone or concurrently (M2 stage 9).
//!
//! Random documents are trees: paragraphs in lists, quotes and tables,
//! nested, made by applying random operations to a new document, which
//! now and then has a stylesheet of styles based on each other and named
//! parts.

use doc::{
    Block, Document, History, Kind, Object, ObjectId, Op, Paragraph, Props, Run, Sheet, Snapshot,
    Style, StyleKind, Value, conflict, rebase,
};
use proptest::prelude::*;

/// Characters of one to four bytes, so character offsets and byte offsets
/// differ.
const ALPHABET: &[char] = &['a', 'b', ' ', 'é', 'ж', '中', '😀'];

/// Draws numbers from proptest's choices, cycling if they run out.
struct Choices<'a> {
    values: &'a [u32],
    at: usize,
}

impl Choices<'_> {
    fn next(&mut self) -> u32 {
        let v = self.values[self.at % self.values.len()];
        self.at += 1;
        v
    }

    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// An offset in `0..=n`, often at the start, the end or the middle,
    /// so that concurrent operations meet at the same place.
    fn offset(&mut self, n: u32) -> u32 {
        match self.below(4) {
            0 => 0,
            1 => n,
            2 => n / 2,
            _ => self.below(n + 1),
        }
    }

    fn text(&mut self, max: u32) -> String {
        let n = self.below(max + 1);
        (0..n)
            .map(|_| ALPHABET[self.below(ALPHABET.len() as u32) as usize])
            .collect()
    }

    fn props(&mut self) -> Props {
        let mut props = Props::new();
        if self.below(2) == 0 {
            props.insert("bold".into(), Value::Bool(true));
        }
        if self.below(3) == 0 {
            props.insert("size".into(), Value::Int(self.below(4) as i64 * 2 + 10));
        }
        props
    }

    fn value(&mut self, key: &str) -> Option<Value> {
        match (self.below(4), key) {
            (0, _) => None,
            (_, "bold" | "italic" | "ordered") => Some(Value::Bool(self.below(2) == 0)),
            (_, "size" | "start") => Some(Value::Int(self.below(4) as i64 + 9)),
            (v, _) => Some(Value::Text(
                ["Body", "Heading 1", "Quote"][v as usize % 3].into(),
            )),
        }
    }

    /// One of `items`, often the first, so that concurrent operations
    /// meet.
    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        if items.is_empty() {
            None
        } else if self.below(3) == 0 {
            Some(items[0])
        } else {
            Some(items[self.below(items.len() as u32) as usize])
        }
    }
}

fn len(doc: &Document, p: ObjectId) -> u32 {
    doc.paragraph(p).unwrap().len()
}

/// A paragraph, often the first, so that concurrent operations meet.
fn pick(doc: &Document, c: &mut Choices<'_>) -> ObjectId {
    c.pick(&doc.paragraphs()).expect("a paragraph")
}

fn insert(doc: &Document, c: &mut Choices<'_>) -> Op {
    let paragraph = pick(doc, c);
    let mut text = c.text(4);
    if text.is_empty() {
        text.push('x');
    }
    let n = text.chars().count() as u32;
    let runs = vec![Run::new(n, c.props())];
    Op::InsertText {
        paragraph,
        offset: c.offset(len(doc, paragraph)),
        text,
        runs,
    }
}

/// A random operation of M1's kinds that's valid on `doc`. New IDs come
/// from `ids`.
fn text_op(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    let paragraph = pick(doc, c);
    let n = len(doc, paragraph);
    match c.below(6) {
        0 => insert(doc, c),
        1 if n > 0 => {
            let offset = c.offset(n - 1);
            Op::DeleteText {
                paragraph,
                offset,
                len: 1 + c.below(n - offset),
            }
        }
        2 => {
            *ids += 1;
            Op::SplitParagraph {
                paragraph,
                offset: c.offset(n),
                new: ObjectId(*ids),
                new_props: c.props(),
            }
        }
        3 => {
            // Paragraphs followed by a paragraph in the same container.
            let joinable: Vec<(ObjectId, ObjectId)> = doc
                .paragraphs()
                .into_iter()
                .filter_map(|p| {
                    doc.next(p)
                        .filter(|n| doc.paragraph(*n).is_some())
                        .map(|n| (p, n))
                })
                .collect();
            match c.pick(&joinable) {
                Some((p, next)) => Op::JoinParagraphs {
                    paragraph: p,
                    offset: len(doc, p),
                    next,
                },
                None => insert(doc, c),
            }
        }
        4 => {
            let offset = c.offset(n);
            let key = ["bold", "italic", "size"][c.below(3) as usize];
            Op::SetCharProp {
                paragraph,
                offset,
                len: c.below(n - offset + 1),
                key: key.into(),
                value: c.value(key),
            }
        }
        5 => {
            let key = ["style", "align"][c.below(2) as usize];
            Op::SetParaProp {
                paragraph,
                key: key.into(),
                value: c.value(key),
            }
        }
        _ => insert(doc, c),
    }
}

fn fresh(ids: &mut u64) -> ObjectId {
    *ids += 1;
    ObjectId(*ids)
}

fn paragraph(c: &mut Choices<'_>, parent: ObjectId) -> Object {
    let text = c.text(3);
    let n = text.chars().count() as u32;
    Object::Paragraph(Paragraph {
        parent,
        text,
        runs: if n == 0 {
            Vec::new()
        } else {
            vec![Run::new(n, c.props())]
        },
        props: Props::new(),
    })
}

/// A random block for `parent`, a container that holds `holds`: its ID
/// and its objects, it first. `depth` bounds the nesting.
fn block(
    c: &mut Choices<'_>,
    ids: &mut u64,
    parent: ObjectId,
    holds: Holds,
    depth: u32,
) -> (ObjectId, Vec<(ObjectId, Object)>) {
    let id = fresh(ids);
    let kind = match holds {
        Holds::Paragraphs => None,
        Holds::Items => Some(Kind::Item),
        Holds::Rows => Some(Kind::Row),
        Holds::Flow if depth == 0 => None,
        Holds::Flow => {
            [None, Some(Kind::List), Some(Kind::Quote), Some(Kind::Table)][c.below(4) as usize]
        }
    };
    let Some(kind) = kind else {
        return (id, vec![(id, paragraph(c, parent))]);
    };
    let inner = match kind {
        Kind::List => Holds::Items,
        Kind::Table => Holds::Rows,
        Kind::Row => Holds::Paragraphs,
        Kind::Item | Kind::Quote => Holds::Flow,
    };
    // Now and then an empty container, as Markdown's "-" is an empty item.
    let count = if c.below(8) == 0 { 0 } else { 1 + c.below(2) };
    let mut children = Vec::new();
    let mut rest = Vec::new();
    for _ in 0..count {
        let (child, objects) = block(c, ids, id, inner, depth.saturating_sub(1));
        children.push(child);
        rest.extend(objects);
    }
    let mut props = Props::new();
    if kind == Kind::List && c.below(2) == 0 {
        props.insert("ordered".into(), Value::Bool(true));
    }
    let mut out = vec![(
        id,
        Object::Block(Block {
            kind,
            parent,
            children,
            props,
        }),
    )];
    out.extend(rest);
    (id, out)
}

#[derive(Clone, Copy)]
enum Holds {
    Flow,
    Items,
    Rows,
    Paragraphs,
}

fn holds(object: &Object) -> Holds {
    match object {
        Object::Block(b) => match b.kind {
            Kind::List => Holds::Items,
            Kind::Table => Holds::Rows,
            Kind::Row => Holds::Paragraphs,
            Kind::Item | Kind::Quote => Holds::Flow,
        },
        _ => Holds::Flow,
    }
}

/// Every container, the story first.
fn containers(doc: &Document) -> Vec<ObjectId> {
    doc.subtree(doc.root())
        .into_iter()
        .filter(|&id| doc.object(id).is_some_and(Object::is_container))
        .collect()
}

/// A place in `parent`: after one of its children, or first.
fn place(doc: &Document, parent: ObjectId, c: &mut Choices<'_>) -> Option<ObjectId> {
    let children = doc.children(parent);
    match c.below(children.len() as u32 + 1) {
        0 => None,
        i => Some(children[i as usize - 1]),
    }
}

/// A random operation on blocks, valid or not on `doc`.
fn block_op_candidate(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    let all = doc.subtree(doc.root());
    match c.below(4) {
        0 => {
            let parent = c.pick(&containers(doc)).expect("the story");
            let (_, objects) = block(c, ids, parent, holds(doc.object(parent).unwrap()), 2);
            Op::InsertBlock {
                parent,
                after: place(doc, parent, c),
                objects,
            }
        }
        1 => Op::DeleteBlock {
            block: c.pick(&all[1..]).unwrap_or(all[0]),
        },
        2 => {
            let block = c.pick(&all[1..]).unwrap_or(all[0]);
            let parent = c.pick(&containers(doc)).expect("the story");
            Op::MoveBlock {
                block,
                parent,
                after: place(doc, parent, c),
            }
        }
        _ => {
            let blocks: Vec<ObjectId> = all
                .iter()
                .copied()
                .filter(|&id| doc.block(id).is_some())
                .collect();
            let key = ["ordered", "start"][c.below(2) as usize];
            Op::SetBlockProp {
                block: c.pick(&blocks).unwrap_or(all[0]),
                key: key.into(),
                value: c.value(key),
            }
        }
    }
}

/// A random operation on blocks that's valid on `doc`, or text typed if
/// none comes up.
fn block_op(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    for _ in 0..8 {
        let op = block_op_candidate(doc, ids, c);
        if doc.clone().apply(&op).is_ok() {
            return op;
        }
    }
    insert(doc, c)
}

/// A random operation of any kind that's valid on `doc`.
fn op(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    if doc.sheet().is_some() && c.below(3) == 0 {
        return style_op(doc, ids, c);
    }
    if c.below(5) < 2 {
        block_op(doc, ids, c)
    } else {
        text_op(doc, ids, c)
    }
}

const KINDS: [StyleKind; 7] = [
    StyleKind::Paragraph,
    StyleKind::Character,
    StyleKind::Paragraph,
    StyleKind::Face,
    StyleKind::Colour,
    StyleKind::Page,
    StyleKind::Tabs,
];

/// A style's properties: its name, and a value or two, a face by ID among
/// them, where `faces` has one.
fn style_props(c: &mut Choices<'_>, n: u64, faces: &[ObjectId]) -> Props {
    let mut props = Props::new();
    props.insert("name".into(), Value::Text(format!("Style {n}")));
    if c.below(2) == 0 {
        props.insert("size".into(), Value::Int(c.below(4) as i64 + 9));
    }
    if let Some(face) = c.pick(faces) {
        props.insert("font".into(), Value::Ref(face));
    }
    props
}

/// The styles of `kind` among `styles`.
fn of_kind(styles: &[(ObjectId, Style)], kind: StyleKind) -> Vec<ObjectId> {
    styles
        .iter()
        .filter(|(_, s)| s.kind == kind)
        .map(|(id, _)| *id)
        .collect()
}

/// A new document with a stylesheet of a few styles and parts, each style
/// based on one before it of its kind, or none.
fn styled(c: &mut Choices<'_>) -> Document {
    let (root, first, sheet) = (ObjectId(0), ObjectId(1), ObjectId(2));
    let mut objects = vec![
        (
            root,
            Object::Story(doc::Story {
                blocks: vec![first],
            }),
        ),
        (
            first,
            Object::Paragraph(Paragraph {
                parent: root,
                text: String::new(),
                runs: Vec::new(),
                props: Props::new(),
            }),
        ),
    ];
    let count = 1 + c.below(7) as u64;
    let mut styles: Vec<(ObjectId, Style)> = Vec::new();
    for n in 0..count {
        let id = ObjectId(3 + n);
        let kind = KINDS[c.below(KINDS.len() as u32) as usize];
        let base = if kind.is_part() || c.below(3) == 0 {
            None
        } else {
            c.pick(&of_kind(&styles, kind))
        };
        let props = style_props(c, n, &of_kind(&styles, StyleKind::Face));
        styles.push((
            id,
            Style {
                sheet,
                kind,
                base,
                props,
            },
        ));
    }
    let mut defaults = Props::new();
    defaults.insert("size".into(), Value::Int(11));
    objects.push((
        sheet,
        Object::Sheet(Sheet {
            styles: styles.iter().map(|(id, _)| *id).collect(),
            props: defaults,
        }),
    ));
    objects.extend(styles.into_iter().map(|(id, s)| (id, Object::Style(s))));
    Document::from_snapshot(Snapshot {
        root,
        next_id: 3 + count,
        objects,
    })
    .expect("a styled document holds together")
}

/// A random operation on styles, valid or not on `doc`, which has a
/// stylesheet.
fn style_op_candidate(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    let sheet = doc.sheet().expect("a stylesheet");
    let styles = doc.styles().to_vec();
    let faces: Vec<ObjectId> = styles
        .iter()
        .copied()
        .filter(|&s| doc.style(s).is_some_and(|s| s.kind == StyleKind::Face))
        .collect();
    match c.below(6) {
        0 => {
            let id = fresh(ids);
            let kind = KINDS[c.below(KINDS.len() as u32) as usize];
            let after = match c.below(styles.len() as u32 + 1) {
                0 => None,
                i => Some(styles[i as usize - 1]),
            };
            let base = if c.below(2) == 0 {
                c.pick(&styles)
            } else {
                None
            };
            Op::InsertStyle {
                sheet,
                after,
                id,
                style: Style {
                    sheet,
                    kind,
                    base,
                    props: style_props(c, id.0, &faces),
                },
            }
        }
        1 => Op::DeleteStyle {
            style: c.pick(&styles).unwrap_or(sheet),
        },
        2 | 3 => {
            let style = c.pick(&styles).unwrap_or(sheet);
            let base = if c.below(4) > 0 {
                c.pick(&styles)
            } else {
                None
            };
            Op::SetBase { style, base }
        }
        4 => {
            let target = if c.below(4) == 0 {
                sheet
            } else {
                c.pick(&styles).unwrap_or(sheet)
            };
            let key = ["size", "name", "font"][c.below(3) as usize];
            let value = match key {
                "font" => c.pick(&faces).map(Value::Ref),
                _ => c.value(key),
            };
            Op::SetStyleProp {
                style: target,
                key: key.into(),
                value,
            }
        }
        _ => Op::SetParaProp {
            paragraph: pick(doc, c),
            key: "style".into(),
            value: c.pick(&styles).map(Value::Ref),
        },
    }
}

/// A random operation on styles that's valid on `doc`, or text typed if
/// none comes up.
fn style_op(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    for _ in 0..8 {
        let op = style_op_candidate(doc, ids, c);
        if doc.clone().apply(&op).is_ok() {
            return op;
        }
    }
    insert(doc, c)
}

/// Whether every style's chain of bases ends.
fn chains_end(doc: &Document) -> bool {
    doc.styles()
        .iter()
        .all(|&s| doc.chain(s).len() <= doc.styles().len())
}

/// A random document: paragraphs of formatted text in many scripts, in
/// lists, quotes and tables, made by applying random operations to a new
/// one, which now and then has a stylesheet.
fn document(c: &mut Choices<'_>, ids: &mut u64) -> Document {
    let mut doc = if c.below(3) == 0 {
        styled(c)
    } else {
        Document::new()
    };
    let steps = 4 + c.below(16);
    for _ in 0..steps {
        let op = if doc.paragraphs().len() >= 8 && c.below(2) == 0 {
            insert(&doc, c)
        } else {
            op(&doc, ids, c)
        };
        doc.apply(&op).expect("a generated operation applies");
    }
    doc
}

fn choices() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(any::<u32>(), 64..256)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn inverses_restore_the_document(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let original = document(c, &mut ids);
        let mut doc = original.clone();
        let mut inverses = Vec::new();
        for _ in 0..1 + c.below(8) {
            let op = op(&doc, &mut ids, c);
            let before = doc.clone();
            let inverse = doc.apply(&op).expect("a generated operation applies");
            let mut undone = doc.clone();
            undone.apply_all(&inverse).expect("an inverse applies");
            prop_assert_eq!(&undone, &before, "{:?} undone by {:?}", op, inverse);
            inverses.push(inverse);
        }
        for inverse in inverses.iter().rev() {
            doc.apply_all(inverse).expect("an inverse applies");
        }
        prop_assert_eq!(doc, original);
    }

    #[test]
    fn rebasing_converges(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let a = op(&doc, &mut ids, c);
        let b = op(&doc, &mut ids, c);

        let mut left = doc.clone();
        left.apply(&a).expect("a applies");
        let b_over_a = rebase(&doc, &b, &a, false);
        for x in &b_over_a {
            left.apply(x).map_err(|e| TestCaseError::fail(format!("b over a: {x:?}: {e}\na {a:?}\nb {b:?}\n{doc:?}")))?;
        }

        let mut right = doc.clone();
        right.apply(&b).expect("b applies");
        let a_over_b = rebase(&doc, &a, &b, true);
        for x in &a_over_b {
            right.apply(x).map_err(|e| TestCaseError::fail(format!("a over b: {x:?}: {e}\na {a:?}\nb {b:?}\n{doc:?}")))?;
        }

        prop_assert_eq!(&left, &right, "a {:?}\nb {:?}\nb over a {:?}\na over b {:?}\nfrom\n{:?}", a, b, b_over_a, a_over_b, doc);
    }

    /// M1's operations, on any document, meet by the rules alone: neither
    /// is ever dropped for the other.
    #[test]
    fn m1s_operations_never_conflict(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let a = text_op(&doc, &mut ids, c);
        let b = text_op(&doc, &mut ids, c);
        prop_assert!(!conflict(&doc, &a, &b), "a {:?}\nb {:?}\nfrom\n{:?}", a, b, doc);
    }

    /// Only deletions and moves of blocks ever conflict: two deletions
    /// that together would leave the story no paragraph, and moves that
    /// together would put a block inside itself, or swap two blocks.
    /// Everything else meets by the rules.
    #[test]
    fn only_deletions_and_moves_conflict(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let a = op(&doc, &mut ids, c);
        let b = op(&doc, &mut ids, c);
        if !conflict(&doc, &a, &b) {
            return Ok(());
        }
        let why = format!("a {a:?}\nb {b:?}\nfrom\n{doc:?}");
        match (&a, &b) {
            (Op::DeleteBlock { block: x }, Op::DeleteBlock { block: y }) => {
                let gone: std::collections::BTreeSet<ObjectId> =
                    doc.subtree(*x).into_iter().chain(doc.subtree(*y)).collect();
                prop_assert!(doc.paragraphs().iter().all(|p| gone.contains(p)), "{}", why);
            }
            (Op::MoveBlock { .. }, Op::MoveBlock { .. }) => {}
            // Bases that together would loop, and a base on, or a style
            // based on, one deleted meanwhile.
            (Op::SetBase { .. }, Op::SetBase { .. })
            | (Op::SetBase { .. } | Op::InsertStyle { .. }, Op::DeleteStyle { .. })
            | (Op::DeleteStyle { .. }, Op::SetBase { .. } | Op::InsertStyle { .. }) => {}
            _ => prop_assert!(false, "{}", why),
        }
    }

    /// Whatever bases are set, valid or not, one after another, no style
    /// is ever based on itself, a refused one changes nothing, and the
    /// document always reads back from its snapshot.
    #[test]
    fn bases_never_loop(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let mut doc = styled(c);
        for _ in 0..24 {
            let op = style_op_candidate(&doc, &mut ids, c);
            let before = doc.clone();
            if doc.apply(&op).is_err() {
                prop_assert_eq!(&doc, &before, "a refused {:?} changed nothing", op);
            }
            prop_assert!(chains_end(&doc), "after {:?}\n{:?}", op, doc);
        }
        prop_assert!(Document::from_snapshot(doc.to_snapshot()).is_ok());
    }

    /// Two concurrent operations on styles, each valid alone, never make
    /// a style based on itself together, in either order.
    #[test]
    fn concurrent_bases_never_loop(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = styled(c);
        let a = style_op(&doc, &mut ids, c);
        let b = style_op(&doc, &mut ids, c);
        for (first, second) in [(&a, &b), (&b, &a)] {
            let mut d = doc.clone();
            d.apply(first).expect("the first applies");
            d.apply_all(&rebase(&doc, second, first, false)).expect("the second over it applies");
            prop_assert!(chains_end(&d), "a {:?}\nb {:?}\n{:?}", a, b, d);
            prop_assert!(Document::from_snapshot(d.to_snapshot()).is_ok());
        }
    }

    #[test]
    fn snapshots_never_change(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let mut doc = document(c, &mut ids);
        let snapshot = doc.clone();
        let dump = snapshot.dump();
        for _ in 0..1 + c.below(8) {
            let op = op(&doc, &mut ids, c);
            doc.apply(&op).expect("a generated operation applies");
        }
        prop_assert_eq!(snapshot.dump(), dump);
    }

    #[test]
    fn documents_survive_storing_as_snapshots(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let mut doc = document(c, &mut ids);
        for _ in 0..c.below(6) {
            let op = op(&doc, &mut ids, c);
            doc.apply(&op).expect("a generated operation applies");
        }
        let text = ron::to_string(&doc.to_snapshot()).expect("a snapshot serializes");
        let back: Snapshot = ron::from_str(&text).expect("and reads back");
        let restored = Document::from_snapshot(back).expect("a snapshot of a document is valid");
        prop_assert_eq!(restored.dump(), doc.dump());
        prop_assert_eq!(&restored, &doc);
        prop_assert_eq!(restored.paragraphs(), doc.paragraphs());
        // IDs given out after it don't collide with the stored ones.
        let mut again = restored.clone();
        let mut original = doc.clone();
        prop_assert_eq!(again.new_id(), original.new_id());
    }

    #[test]
    fn transactions_are_all_or_nothing_and_undo_exactly(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let original = document(c, &mut ids);
        let mut doc = original.clone();
        let mut history = History::new();
        let mut states = vec![doc.clone()];
        for _ in 0..1 + c.below(5) {
            let mut scratch = doc.clone();
            let mut ops = Vec::new();
            for _ in 0..1 + c.below(4) {
                let op = op(&scratch, &mut ids, c);
                scratch.apply(&op).expect("a generated operation applies");
                ops.push(op);
            }
            // A transaction ending in an impossible operation changes nothing.
            let mut bad = ops.clone();
            bad.push(Op::DeleteText { paragraph: ObjectId(u64::MAX), offset: 0, len: 1 });
            let before = doc.clone();
            prop_assert!(doc.transact("Bad", bad).is_err());
            prop_assert_eq!(&doc, &before);

            history.push(doc.transact("Edit", ops).expect("generated operations apply"));
            prop_assert_eq!(&doc, &scratch);
            states.push(doc.clone());
        }
        for state in states.iter().rev().skip(1) {
            prop_assert_eq!(history.undo(&mut doc).unwrap().map(|t| t.name.as_str()), Some("Edit"));
            prop_assert_eq!(&doc, state);
        }
        prop_assert!(history.undo(&mut doc).unwrap().is_none());
        for state in states.iter().skip(1) {
            history.redo(&mut doc).unwrap();
            prop_assert_eq!(&doc, state);
        }
        prop_assert!(history.redo(&mut doc).unwrap().is_none());
    }

    #[test]
    fn operations_survive_serialization(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let op = op(&doc, &mut ids, c);
        let text = ron::to_string(&op).unwrap();
        prop_assert_eq!(ron::from_str::<Op>(&text).unwrap(), op);
    }
}

#[test]
fn a_new_document_is_one_empty_paragraph() {
    let doc = Document::new();
    assert_eq!(doc.paragraphs().len(), 1);
    assert!(doc.paragraph(doc.paragraphs()[0]).unwrap().is_empty());
}

#[test]
fn invalid_operations_are_refused_and_change_nothing() {
    let mut doc = Document::new();
    let p = doc.paragraphs()[0];
    let before = doc.clone();
    for op in [
        Op::DeleteText {
            paragraph: p,
            offset: 0,
            len: 1,
        },
        Op::InsertText {
            paragraph: p,
            offset: 1,
            text: "x".into(),
            runs: vec![Run::new(1, Props::new())],
        },
        Op::InsertText {
            paragraph: p,
            offset: 0,
            text: "xy".into(),
            runs: vec![Run::new(1, Props::new())],
        },
        Op::JoinParagraphs {
            paragraph: p,
            offset: 0,
            next: ObjectId(99),
        },
        Op::SplitParagraph {
            paragraph: p,
            offset: 0,
            new: p,
            new_props: Props::new(),
        },
        Op::SetParaProp {
            paragraph: ObjectId(99),
            key: "style".into(),
            value: None,
        },
    ] {
        assert!(doc.apply(&op).is_err(), "{op:?}");
        assert_eq!(doc, before);
    }
}

/// The tie rules rebasing documents (`doc::rebase`'s module comment), which
/// convergence alone can't tell from their mirror images.
mod ties {
    use super::*;

    fn plain(text: &str) -> Vec<Run> {
        vec![Run::new(text.chars().count() as u32, Props::new())]
    }

    fn insert(p: ObjectId, offset: u32, text: &str) -> Op {
        Op::InsertText {
            paragraph: p,
            offset,
            text: text.into(),
            runs: plain(text),
        }
    }

    /// "hello" in one paragraph.
    fn hello() -> (Document, ObjectId) {
        let mut doc = Document::new();
        let p = doc.paragraphs()[0];
        doc.apply(&insert(p, 0, "hello")).unwrap();
        (doc, p)
    }

    /// `first` then `second` rebased over it.
    fn both(doc: &Document, first: &Op, second: &Op) -> Document {
        let mut d = doc.clone();
        d.apply(first).unwrap();
        d.apply_all(&rebase(doc, second, first, false)).unwrap();
        d
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.paragraphs()
            .iter()
            .map(|p| doc.paragraph(*p).unwrap().text.clone())
            .collect()
    }

    #[test]
    fn inserts_at_one_offset_put_the_first_text_first() {
        let (doc, p) = hello();
        let d = both(&doc, &insert(p, 2, "AB"), &insert(p, 2, "xy"));
        assert_eq!(texts(&d), ["heABxyllo"]);
    }

    #[test]
    fn an_insert_at_a_split_point_ends_the_first_paragraph() {
        let (doc, p) = hello();
        let split = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(50),
            new_props: Props::new(),
        };
        assert_eq!(
            texts(&both(&doc, &split, &insert(p, 2, "XY"))),
            ["heXY", "llo"]
        );
        assert_eq!(
            texts(&both(&doc, &insert(p, 2, "XY"), &split)),
            ["heXY", "llo"]
        );
    }

    #[test]
    fn a_deletion_leaves_text_inserted_where_it_starts_or_inside_it() {
        let (doc, p) = hello();
        let delete = Op::DeleteText {
            paragraph: p,
            offset: 1,
            len: 3,
        };
        assert_eq!(texts(&both(&doc, &insert(p, 1, "XY"), &delete)), ["hXYo"]);
        assert_eq!(texts(&both(&doc, &insert(p, 2, "XY"), &delete)), ["hXYo"]);
        assert_eq!(texts(&both(&doc, &insert(p, 4, "XY"), &delete)), ["hXYo"]);
    }

    #[test]
    fn splits_at_one_offset_put_the_first_new_paragraph_first_and_empty() {
        let (doc, p) = hello();
        let one = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(50),
            new_props: Props::new(),
        };
        let two = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(51),
            new_props: Props::new(),
        };
        let d = both(&doc, &one, &two);
        assert_eq!(d.paragraphs(), [p, ObjectId(50), ObjectId(51)]);
        assert_eq!(texts(&d), ["he", "", "llo"]);
    }

    #[test]
    fn the_later_of_two_settings_wins_where_they_overlap() {
        let (doc, p) = hello();
        let set = |offset, len, size| Op::SetCharProp {
            paragraph: p,
            offset,
            len,
            key: "size".into(),
            value: Some(Value::Int(size)),
        };
        let d = both(&doc, &set(0, 3, 10), &set(2, 3, 20));
        let sizes: Vec<_> = d
            .paragraph(p)
            .unwrap()
            .runs
            .iter()
            .map(|r| (r.len, r.props.get("size").cloned()))
            .collect();
        assert_eq!(
            sizes,
            [(2, Some(Value::Int(10))), (3, Some(Value::Int(20)))]
        );
        let para = |v: &str| Op::SetParaProp {
            paragraph: p,
            key: "style".into(),
            value: Some(Value::Text(v.into())),
        };
        let d = both(&doc, &para("Body"), &para("Quote"));
        assert_eq!(
            d.paragraph(p).unwrap().props.get("style"),
            Some(&Value::Text("Quote".into()))
        );
    }
}

mod snapshots {
    use std::sync::Arc;

    use doc::{Document, Object, ObjectId, Op, Props, Run, Snapshot, SnapshotError};

    fn two_paragraphs() -> Snapshot {
        let mut doc = Document::new();
        let first = doc.paragraphs()[0];
        doc.apply(&Op::InsertText {
            paragraph: first,
            offset: 0,
            text: "ab".to_string(),
            runs: vec![Run::new(2, Props::new())],
        })
        .unwrap();
        let new = doc.new_id();
        doc.apply(&Op::SplitParagraph {
            paragraph: first,
            offset: 1,
            new,
            new_props: Props::new(),
        })
        .unwrap();
        doc.to_snapshot()
    }

    fn paragraph_mut(s: &mut Snapshot, id: ObjectId) -> &mut doc::Paragraph {
        match &mut s.objects.iter_mut().find(|(i, _)| *i == id).unwrap().1 {
            Object::Paragraph(p) => p,
            _ => panic!("a paragraph"),
        }
    }

    #[test]
    fn broken_snapshots_are_refused() {
        let good = two_paragraphs();
        assert!(Document::from_snapshot(good.clone()).is_ok());
        let (root, p1, p2) = (good.root, good.objects[1].0, good.objects[2].0);

        let mut s = good.clone();
        s.objects.push(s.objects[1].clone());
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::Duplicate(p1))
        );

        let mut s = good.clone();
        s.next_id = p2.0;
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::IdTooHigh(p2))
        );

        let mut s = good.clone();
        s.root = p1;
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::RootNotAStory(p1))
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story { blocks: vec![] });
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::EmptyStory(root))
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story {
            blocks: vec![p1, ObjectId(99)],
        });
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::NotItsParagraph {
                story: root,
                paragraph: ObjectId(99)
            })
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story {
            blocks: vec![p1, p1],
        });
        assert_eq!(Document::from_snapshot(s), Err(SnapshotError::Stray(p1)));

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story { blocks: vec![p1] });
        assert_eq!(Document::from_snapshot(s), Err(SnapshotError::Stray(p2)));

        let mut s = good.clone();
        paragraph_mut(&mut s, p1).text.push('x');
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::RunsMismatch(p1))
        );
    }

    #[test]
    fn runs_read_back_normalized() {
        let mut s = two_paragraphs();
        let p1 = s.objects[1].0;
        let p = paragraph_mut(&mut s, p1);
        p.text = "xyz".to_string();
        p.runs = vec![
            Run::new(1, Props::new()),
            Run::new(0, Props::new()),
            Run::new(2, Props::new()),
        ];
        let doc = Document::from_snapshot(s).unwrap();
        assert_eq!(
            doc.paragraph(p1).unwrap().runs,
            vec![Run {
                len: 3,
                props: Arc::new(Props::new())
            }]
        );
    }

    #[test]
    fn a_continued_transaction_undoes_with_the_one_before() {
        let mut doc = Document::new();
        let p = doc.paragraphs()[0];
        let mut history = doc::History::new();
        let typed = |doc: &mut Document, offset: u32, text: &str| {
            doc.transact(
                "Typing",
                vec![Op::InsertText {
                    paragraph: p,
                    offset,
                    text: text.to_string(),
                    runs: vec![Run::new(text.chars().count() as u32, Props::new())],
                }],
            )
            .unwrap()
        };
        let empty = doc.clone();
        assert!(!history.continue_last(typed(&mut doc.clone(), 0, "x")));
        let t = typed(&mut doc, 0, "a");
        history.push(t);
        let t = typed(&mut doc, 1, "b");
        assert!(history.continue_last(t));
        let t = typed(&mut doc, 2, "c");
        assert!(history.continue_last(t));
        assert_eq!(doc.paragraph(p).unwrap().text, "abc");
        let undone = history.undo(&mut doc).unwrap().unwrap().name.clone();
        assert_eq!(undone, "Typing");
        assert_eq!(doc, empty);
        assert!(history.undo(&mut doc).unwrap().is_none());
        history.redo(&mut doc).unwrap();
        assert_eq!(doc.paragraph(p).unwrap().text, "abc");
    }

    #[test]
    fn history_rebuilds_from_a_record_without_applying() {
        let mut history = doc::History::new();
        let t = |name: &str| doc::Transaction {
            name: name.to_string(),
            ops: vec![],
            inverse: vec![],
        };
        history.push(t("Typing"));
        history.push(t("Bold"));
        assert!(history.replay_undo());
        assert_eq!(
            (history.undo_name(), history.redo_name()),
            (Some("Typing"), Some("Bold"))
        );
        assert!(history.replay_redo());
        assert_eq!(
            (history.undo_name(), history.redo_name()),
            (Some("Bold"), None)
        );
        assert!(!history.replay_redo());
    }
}
