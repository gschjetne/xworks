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

use crate::Op::{DeleteText, InsertText, JoinParagraphs, SetCharProp, SetParaProp, SplitParagraph};
use crate::{ObjectId, Op};

/// `op` rebased over `over`, which was applied first: the operations that
/// make `op`'s change now, in order. Empty when `over` already made it
/// moot.
pub fn rebase(op: &Op, over: &Op, first: bool) -> Vec<Op> {
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
