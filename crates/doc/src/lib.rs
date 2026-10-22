//! The document core: objects with stable IDs, operations that invert and
//! rebase, transactions, undo, and snapshots (docs/ARCHITECTURE.md,
//! section 5).
//!
//! - [`Document`]: objects by [`ObjectId`] in a persistent map, so a clone
//!   is an O(1) snapshot. Its root is a [`Story`], a tree of blocks:
//!   [`Paragraph`]s, text with runs of character properties ([`Run`]) and
//!   properties of its own ([`Props`]), and containers ([`Block`]) of each
//!   [`Kind`]: lists of items, quotes, and tables of rows of cells.
//!   Headings and code blocks are paragraphs, set apart by their style.
//! - [`Op`]: the changes. [`Document::apply`] applies one and returns the
//!   operations that undo it; [`rebase`] moves one over a concurrent one,
//!   consulting the document both apply to.
//! - [`Transaction`] and [`History`]: named, undoable steps.
//! - [`Snapshot`]: a document as plain data, for storing, and checked when
//!   it's read back ([`Document::from_snapshot`]).
//!
//! Invariants:
//!
//! - IDs are never reused, and operations address objects by ID, never by
//!   index. Text positions are a paragraph and an offset in Unicode scalar
//!   values; a place among a container's children is the child before it.
//! - The story is a tree: every object but the root is held by exactly one
//!   container, of a kind that can hold it, and there's always a paragraph
//!   somewhere in it. Containers may be empty.
//! - A failed operation changes nothing, and none panics.
//! - Applying an operation's inverse restores the document exactly; a
//!   deleted block's inverse carries everything that was in it.
//! - Equal documents are equal whatever edits made them: runs are kept
//!   normalized.
//! - A snapshot never changes, whatever is done to the document after it.
//! - Rebasing converges: either order of two concurrent operations, each
//!   rebased over the other, gives the same document.

mod document;
mod history;
mod id;
mod idmap;
mod op;
mod props;
mod rebase;

pub use document::{Block, Document, Kind, Object, Paragraph, Snapshot, SnapshotError, Story};
pub use history::{History, Transaction};
pub use id::ObjectId;
pub use op::{Error, Op};
pub use props::{Props, Run, Value};
pub use rebase::{conflict, rebase};
