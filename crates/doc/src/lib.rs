//! The document core: objects with stable IDs, operations that invert and
//! rebase, transactions, undo, and snapshots (docs/ARCHITECTURE.md,
//! section 5).
//!
//! - [`Document`]: objects by [`ObjectId`] in a persistent map, so a clone
//!   is an O(1) snapshot. M1 has two kinds: a [`Story`], an ordered list of
//!   paragraphs, and a [`Paragraph`], text with runs of character
//!   properties ([`Run`]) and properties of its own ([`Props`]).
//! - [`Op`]: the changes. [`Document::apply`] applies one and returns the
//!   operations that undo it; [`rebase`] moves one over a concurrent one.
//! - [`Transaction`] and [`History`]: named, undoable steps.
//! - [`Snapshot`]: a document as plain data, for storing, and checked when
//!   it's read back ([`Document::from_snapshot`]).
//!
//! Invariants:
//!
//! - IDs are never reused, and operations address objects by ID, never by
//!   index. Text positions are a paragraph and an offset in Unicode scalar
//!   values.
//! - A failed operation changes nothing, and none panics.
//! - Applying an operation's inverse restores the document exactly.
//! - Equal documents are equal whatever edits made them: runs are kept
//!   normalized.
//! - A snapshot never changes, whatever is done to the document after it.

mod document;
mod history;
mod id;
mod idmap;
mod op;
mod props;
mod rebase;

pub use document::{Document, Object, Paragraph, Snapshot, SnapshotError, Story};
pub use history::{History, Transaction};
pub use id::ObjectId;
pub use op::{Error, Op};
pub use props::{Props, Run, Value};
pub use rebase::rebase;
