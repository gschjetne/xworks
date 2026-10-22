//! Rebasing: moving an operation over a concurrent one that was applied
//! first, so it means the same change on the document as it now is.
//!
//! When the two touch the same place, `first` breaks the tie: it says
//! whether `op` was ordered before `over`. Applying `a` and then `b`
//! rebased over it (`first: false`) gives the same document as applying
//! `b` and then `a` rebased over it (`first: true`); property tests check
//! this for every pair of kinds.
//!
//! The rules where they meet:
//!
//! - **Inserts at the same offset:** the first one's text comes first.
//! - **An insert at a split point** stays at the end of the paragraph
//!   before the split.
//! - **A deletion** doesn't take text inserted inside its range, and
//!   deletes what's left of its own text after a concurrent deletion.
//! - **Splits at the same offset:** the first split's new paragraph comes
//!   first, empty; the other gets the text after the split.
//! - **The same property set twice:** the later one, the one not
//!   `first`, wins where they overlap.
//! - **A paragraph removed by a join:** operations on it move into the
//!   paragraph it joined; setting its own properties is dropped.
//! - **Blocks put in the same place:** the first one's block comes first.
//! - **A place after a block that's moved or deleted** becomes the place
//!   after the block before it; after a paragraph that's split, the place
//!   after its new second half; after one joined away, the place after
//!   the paragraph it joined.
//! - **A deleted block** takes every change inside it with it, and a
//!   block moved into it stays where it was. A block moved out of one
//!   being deleted goes with it.
//! - **A paragraph split or joined while it's deleted or moved:** both
//!   halves go, or move, together; deleting the paragraph joined away
//!   deletes its text from the one it joined.
//!
//! Some pairs can't both have their way, whatever the rules: two moves
//! that together would put a block inside itself, deletions that together
//! would leave the story no paragraph, a join of paragraphs one of which
//! is moved away. Rebase consults the document both apply to (project
//! owner, 2026-10-22, review pysc65hp) and finds these by applying each
//! order: where they wouldn't meet, the first one wins, and the other is
//! dropped. So the two orders always give the same document; property
//! tests check that the rules alone suffice for M1's operations, and how
//! seldom they don't for blocks.

use crate::Op::{
    DeleteBlock, DeleteText, InsertBlock, InsertText, JoinParagraphs, MoveBlock, SetBlockProp,
    SetCharProp, SetParaProp, SplitParagraph,
};
use crate::{Document, ObjectId, Op};

/// `op` rebased over `over`, which was applied first, both made on
/// `document`: the operations that make `op`'s change now, in order.
/// Empty when `over` already made it moot, or the two conflict
/// ([`conflict`]) and `over` was first; when they conflict and `op` was
/// first, the operations undo `over` and then make `op`. If either doesn't
/// apply to `document`, the rules' answer is given unchecked.
pub fn rebase(document: &Document, op: &Op, over: &Op, first: bool) -> Vec<Op> {
    let moved = transform(document, op, over, first);
    let (a, b) = if first { (op, over) } else { (over, op) };
    if converges(document, a, b) != Some(false) {
        return moved;
    }
    if !first {
        return Vec::new();
    }
    let mut undo = document.clone();
    let mut out = undo.apply(over).unwrap_or_default();
    out.push(op.clone());
    out
}

/// Whether `first` and `second`, both made on `document` and ordered so,
/// can't both have their way: rebased over each other by the rules, the
/// two orders wouldn't give the same document. Then [`rebase`] keeps only
/// `first`. False if either doesn't apply to `document`.
pub fn conflict(document: &Document, first: &Op, second: &Op) -> bool {
    converges(document, first, second) == Some(false)
}

/// Whether `a` then `b` rebased over it gives what `b` then `a` rebased
/// over it does, by the rules alone; `None` if `a` or `b` doesn't apply to
/// `document`.
fn converges(document: &Document, a: &Op, b: &Op) -> Option<bool> {
    let mut left = document.clone();
    left.apply(a).ok()?;
    let mut right = document.clone();
    right.apply(b).ok()?;
    let ok = left.apply_all(&transform(document, b, a, false)).is_ok()
        && right.apply_all(&transform(document, a, b, true)).is_ok();
    Some(ok && left == right)
}

/// `op` moved over `over` by the rules, both made on `base`.
fn transform(base: &Document, op: &Op, over: &Op, first: bool) -> Vec<Op> {
    // A deleted block takes every change inside it with it.
    if let DeleteBlock { block } = over
        && base.object(*block).is_some()
        && inside(base, op, *block)
    {
        return Vec::new();
    }
    match (op, over) {
        (
            InsertBlock {
                parent,
                after,
                objects,
            },
            _,
        ) => {
            let mut after = *after;
            anchor(base, *parent, &mut after, over, first);
            vec![InsertBlock {
                parent: *parent,
                after,
                objects: objects.clone(),
            }]
        }
        (MoveBlock { block, .. }, MoveBlock { block: m, .. }) if block == m => {
            // The same block moved twice: the first move wins.
            if first { vec![op.clone()] } else { Vec::new() }
        }
        (MoveBlock { block, .. }, JoinParagraphs { next, .. }) if block == next => {
            // Its text is in the paragraph it joined now, which the join
            // keeps where it is.
            Vec::new()
        }
        (
            MoveBlock {
                block,
                parent,
                after,
            },
            _,
        ) => {
            let mut after = *after;
            anchor(base, *parent, &mut after, over, first);
            if after == Some(*block) {
                // The place before it was after itself: it stays put.
                after = base.previous(*block);
            }
            let mut out = vec![MoveBlock {
                block: *block,
                parent: *parent,
                after,
            }];
            // A paragraph split while it moves: its second half goes too.
            if let SplitParagraph { paragraph, new, .. } = over
                && paragraph == block
            {
                out.push(MoveBlock {
                    block: *new,
                    parent: *parent,
                    after: Some(*block),
                });
            }
            out
        }
        (DeleteBlock { block }, SplitParagraph { paragraph, new, .. }) if block == paragraph => {
            vec![DeleteBlock { block: *new }, op.clone()]
        }
        (
            DeleteBlock { block },
            JoinParagraphs {
                paragraph,
                offset,
                next,
            },
        ) if block == next => match base.paragraph(*next) {
            // Its text is in the paragraph it joined now.
            Some(n) if !n.is_empty() => vec![DeleteText {
                paragraph: *paragraph,
                offset: *offset,
                len: n.len(),
            }],
            _ => Vec::new(),
        },
        (
            DeleteBlock { block },
            JoinParagraphs {
                paragraph,
                offset,
                next,
            },
        ) if block == paragraph => match base.paragraph(*next) {
            // The paragraph that took the other's text: the other comes
            // back as it was, and this one goes.
            Some(n) => vec![
                SplitParagraph {
                    paragraph: *paragraph,
                    offset: *offset,
                    new: *next,
                    new_props: n.props.clone(),
                },
                op.clone(),
            ],
            None => vec![op.clone()],
        },
        (
            DeleteBlock { block },
            MoveBlock {
                block: m,
                parent: q,
                ..
            },
        ) if block != m => {
            let (m_in, q_in) = (base.contains(*block, *m), base.contains(*block, *q));
            if m_in && !q_in {
                // Moved out of the block: it goes with it all the same.
                vec![DeleteBlock { block: *m }, op.clone()]
            } else if q_in && !m_in {
                // Moved into it: it stays where it was.
                match base.parent(*m) {
                    Some(home) => vec![
                        MoveBlock {
                            block: *m,
                            parent: home,
                            after: base.previous(*m),
                        },
                        op.clone(),
                    ],
                    None => vec![op.clone()],
                }
            } else {
                vec![op.clone()]
            }
        }
        (
            JoinParagraphs {
                paragraph, next, ..
            },
            InsertBlock {
                parent: q,
                after: Some(a),
                objects,
            },
        ) if a == paragraph && base.parent(*paragraph) == Some(*q) && !objects.is_empty() => {
            // A block put between the two: it goes after the second, which
            // is where it ends up when the join comes first.
            vec![
                MoveBlock {
                    block: objects[0].0,
                    parent: *q,
                    after: Some(*next),
                },
                op.clone(),
            ]
        }
        (
            JoinParagraphs {
                paragraph, next, ..
            },
            MoveBlock {
                block: m,
                parent: q,
                ..
            },
        ) if (m == paragraph || m == next) && !no_op(base, over) => {
            // One of the two moved away: the second follows the first,
            // wherever it is now, and they join there.
            let home = if m == paragraph {
                *q
            } else {
                base.parent(*paragraph).unwrap_or(*q)
            };
            vec![
                MoveBlock {
                    block: *next,
                    parent: home,
                    after: Some(*paragraph),
                },
                op.clone(),
            ]
        }
        (
            JoinParagraphs {
                paragraph, next, ..
            },
            MoveBlock {
                block: m,
                parent: q,
                after: Some(a),
            },
        ) if a == paragraph && base.parent(*paragraph) == Some(*q) && m != next => vec![
            MoveBlock {
                block: *m,
                parent: *q,
                after: Some(*next),
            },
            op.clone(),
        ],
        (
            SetBlockProp { block, key, .. },
            SetBlockProp {
                block: b,
                key: key2,
                ..
            },
        ) if block == b && key == key2 && first => Vec::new(),
        _ => text(op, over, first),
    }
}

/// Whether `op` changes only what's in `block`, in `base`: so deleting
/// `block` makes it moot.
fn inside(base: &Document, op: &Op, block: ObjectId) -> bool {
    match op {
        InsertText { paragraph, .. }
        | DeleteText { paragraph, .. }
        | SplitParagraph { paragraph, .. }
        | SetCharProp { paragraph, .. }
        | SetParaProp { paragraph, .. } => base.contains(block, *paragraph),
        JoinParagraphs {
            paragraph, next, ..
        } => base.contains(block, *paragraph) || base.contains(block, *next),
        InsertBlock { parent, .. } => base.contains(block, *parent),
        DeleteBlock { block: b } | SetBlockProp { block: b, .. } => base.contains(block, *b),
        MoveBlock {
            block: m, parent, ..
        } => base.contains(block, *m) || base.contains(block, *parent),
    }
}

/// Moves `after`, a place in container `parent`, over `over`.
fn anchor(base: &Document, parent: ObjectId, after: &mut Option<ObjectId>, over: &Op, first: bool) {
    match over {
        SplitParagraph { paragraph, new, .. } if *after == Some(*paragraph) => {
            *after = Some(*new);
        }
        JoinParagraphs {
            paragraph, next, ..
        } if *after == Some(*next) => *after = Some(*paragraph),
        DeleteBlock { block } if *after == Some(*block) => *after = base.previous(*block),
        MoveBlock {
            block,
            parent: q,
            after: a,
        } => {
            // A block moved away; one that stays put keeps its place.
            if *after == Some(*block) && !no_op(base, over) {
                *after = base.previous(*block);
            }
            if parent == *q && after == a && !first {
                *after = Some(*block);
            }
        }
        InsertBlock {
            parent: q,
            after: a,
            objects,
        } if parent == *q && after == a && !first => {
            if let Some((id, _)) = objects.first() {
                *after = Some(*id);
            }
        }
        _ => {}
    }
}

/// Whether `op` is a move that leaves its block where it is in `base`.
fn no_op(base: &Document, op: &Op) -> bool {
    match op {
        MoveBlock {
            block,
            parent,
            after,
        } => base.parent(*block) == Some(*parent) && base.previous(*block) == *after,
        _ => false,
    }
}

/// `op` moved over `over` by M1's rules for text, which leave blocks be.
fn text(op: &Op, over: &Op, first: bool) -> Vec<Op> {
    let original = op;
    let mut op = op.clone();
    match (&mut op, over) {
        // ---- an insertion
        (
            InsertText {
                paragraph, offset, ..
            },
            InsertText {
                paragraph: p,
                offset: k,
                text,
                ..
            },
        ) if paragraph == p => {
            if *k < *offset || (*k == *offset && !first) {
                *offset += chars(text);
            }
        }
        (
            InsertText {
                paragraph, offset, ..
            },
            DeleteText {
                paragraph: p,
                offset: k,
                len,
            },
        ) if paragraph == p => *offset = map_delete(*offset, *k, *len),
        (
            InsertText {
                paragraph, offset, ..
            },
            SplitParagraph {
                paragraph: p,
                offset: at,
                new,
                ..
            },
        ) if paragraph == p && *offset > *at => {
            *paragraph = *new;
            *offset -= at;
        }
        (
            InsertText {
                paragraph, offset, ..
            },
            JoinParagraphs {
                paragraph: p,
                offset: at,
                next,
            },
        ) if paragraph == next => {
            *paragraph = *p;
            *offset += at;
        }

        // ---- a deletion
        (
            DeleteText {
                paragraph,
                offset,
                len,
            },
            InsertText {
                paragraph: p,
                offset: k,
                text,
                ..
            },
        ) if paragraph == p => {
            let n = chars(text);
            if *k <= *offset {
                *offset += n;
            } else if *k < *offset + *len {
                // Keep the inserted text: delete around it, the later part
                // first so the earlier part's offset holds.
                let before = *k - *offset;
                return vec![
                    DeleteText {
                        paragraph: *paragraph,
                        offset: *k + n,
                        len: *len - before,
                    },
                    DeleteText {
                        paragraph: *paragraph,
                        offset: *offset,
                        len: before,
                    },
                ];
            }
        }
        (
            DeleteText {
                paragraph,
                offset,
                len,
            },
            DeleteText {
                paragraph: p,
                offset: k,
                len: n,
            },
        ) if paragraph == p => {
            let start = map_delete(*offset, *k, *n);
            let end = map_delete(*offset + *len, *k, *n);
            if end == start {
                return Vec::new();
            }
            *offset = start;
            *len = end - start;
        }
        (
            DeleteText {
                paragraph,
                offset,
                len,
            },
            SplitParagraph {
                paragraph: p,
                offset: at,
                new,
                ..
            },
        ) if paragraph == p => return split_range(original, *paragraph, *offset, *len, *at, *new),
        (
            DeleteText {
                paragraph, offset, ..
            },
            JoinParagraphs {
                paragraph: p,
                offset: at,
                next,
            },
        ) if paragraph == next => {
            *paragraph = *p;
            *offset += at;
        }

        // ---- a split
        (
            SplitParagraph {
                paragraph, offset, ..
            },
            InsertText {
                paragraph: p,
                offset: k,
                text,
                ..
            },
        ) if paragraph == p && *k <= *offset => *offset += chars(text),
        (
            SplitParagraph {
                paragraph, offset, ..
            },
            DeleteText {
                paragraph: p,
                offset: k,
                len,
            },
        ) if paragraph == p => *offset = map_delete(*offset, *k, *len),
        (
            SplitParagraph {
                paragraph, offset, ..
            },
            SplitParagraph {
                paragraph: p,
                offset: at,
                new,
                ..
            },
        ) if paragraph == p => {
            if *at < *offset || (*at == *offset && !first) {
                *paragraph = *new;
                *offset -= at;
            }
        }
        (
            SplitParagraph {
                paragraph, offset, ..
            },
            JoinParagraphs {
                paragraph: p,
                offset: at,
                next,
            },
        ) if paragraph == next => {
            *paragraph = *p;
            *offset += at;
        }

        // ---- a join
        (
            JoinParagraphs {
                paragraph, offset, ..
            },
            InsertText {
                paragraph: p, text, ..
            },
        ) if paragraph == p => *offset += chars(text),
        (
            JoinParagraphs {
                paragraph, offset, ..
            },
            DeleteText {
                paragraph: p, len, ..
            },
        ) if paragraph == p => *offset -= len,
        (
            JoinParagraphs {
                paragraph, offset, ..
            },
            SplitParagraph {
                paragraph: p,
                offset: at,
                new,
                ..
            },
        ) if paragraph == p => {
            // The end of the paragraph, where the join happens, is now in
            // the new one.
            *paragraph = *new;
            *offset -= at;
        }
        (
            JoinParagraphs {
                paragraph,
                offset,
                next,
            },
            JoinParagraphs {
                paragraph: p,
                offset: at,
                next: q,
            },
        ) => {
            if paragraph == p && next == q {
                return Vec::new();
            }
            if paragraph == q {
                *paragraph = *p;
                *offset += at;
            }
        }

        // ---- a character property
        (
            SetCharProp {
                paragraph,
                offset,
                len,
                ..
            },
            InsertText {
                paragraph: p,
                offset: k,
                text,
                ..
            },
        ) if paragraph == p => {
            let n = chars(text);
            if *k <= *offset {
                *offset += n;
            } else if *k < *offset + *len {
                // The inserted text keeps its own formatting.
                let before = *k - *offset;
                return vec![
                    with_range(original, *paragraph, *offset, before),
                    with_range(original, *paragraph, *k + n, *len - before),
                ];
            }
        }
        (
            SetCharProp {
                paragraph,
                offset,
                len,
                ..
            },
            DeleteText {
                paragraph: p,
                offset: k,
                len: n,
            },
        ) if paragraph == p => {
            let start = map_delete(*offset, *k, *n);
            let end = map_delete(*offset + *len, *k, *n);
            if end == start {
                return Vec::new();
            }
            *offset = start;
            *len = end - start;
        }
        (
            SetCharProp {
                paragraph,
                offset,
                len,
                ..
            },
            SplitParagraph {
                paragraph: p,
                offset: at,
                new,
                ..
            },
        ) if paragraph == p => return split_range(original, *paragraph, *offset, *len, *at, *new),
        (
            SetCharProp {
                paragraph, offset, ..
            },
            JoinParagraphs {
                paragraph: p,
                offset: at,
                next,
            },
        ) if paragraph == next => {
            *paragraph = *p;
            *offset += at;
        }
        (
            SetCharProp {
                paragraph,
                offset,
                len,
                key,
                ..
            },
            SetCharProp {
                paragraph: p,
                offset: k,
                len: n,
                key: key2,
                ..
            },
        ) if paragraph == p && key == key2 && first => {
            // `over` came later and wins where they overlap.
            let (a, b) = (*offset, *offset + *len);
            let (c, d) = (*k, *k + *n);
            if c >= b || d <= a {
                return vec![original.clone()];
            }
            return [(a, c.max(a).min(b)), (d.min(b).max(a), b)]
                .into_iter()
                .filter(|(s, e)| s < e)
                .map(|(s, e)| with_range(original, *paragraph, s, e - s))
                .collect();
        }

        // ---- a paragraph property
        (SetParaProp { paragraph, .. }, JoinParagraphs { next, .. }) if paragraph == next => {
            return Vec::new();
        }
        (
            SetParaProp { paragraph, key, .. },
            SetParaProp {
                paragraph: p,
                key: key2,
                ..
            },
        ) if paragraph == p && key == key2 && first => return Vec::new(),

        _ => {}
    }
    vec![op]
}

/// Where `x` lands after deleting `len` characters at `at`.
fn map_delete(x: u32, at: u32, len: u32) -> u32 {
    if x <= at {
        x
    } else if x >= at + len {
        x - len
    } else {
        at
    }
}

/// `op`, a deletion or a character property over `offset..offset + len` in
/// `paragraph`, after `paragraph` was split at `at` into `new`.
fn split_range(
    op: &Op,
    paragraph: ObjectId,
    offset: u32,
    len: u32,
    at: u32,
    new: ObjectId,
) -> Vec<Op> {
    let end = offset + len;
    if end <= at {
        vec![op.clone()]
    } else if offset >= at {
        vec![with_range(op, new, offset - at, len)]
    } else {
        vec![
            with_range(op, new, 0, end - at),
            with_range(op, paragraph, offset, at - offset),
        ]
    }
}

/// `op`, a deletion or a character property, over another range.
fn with_range(op: &Op, p: ObjectId, o: u32, l: u32) -> Op {
    let mut part = op.clone();
    match &mut part {
        DeleteText {
            paragraph,
            offset,
            len,
        }
        | SetCharProp {
            paragraph,
            offset,
            len,
            ..
        } => {
            *paragraph = p;
            *offset = o;
            *len = l;
        }
        _ => unreachable!("only ranges move"),
    }
    part
}

fn chars(text: &str) -> u32 {
    text.chars().count() as u32
}
