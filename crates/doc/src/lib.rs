//! The document core: a document as objects with stable IDs, changed only
//! by operations that invert and rebase, grouped into named, undoable
//! transactions, and kept as snapshots (docs/ARCHITECTURE.md, section 5).
//! Every substrate's model is made of its objects, and nothing changes a
//! document but [`Document::apply`].
//!
//! - [`Document`]: objects by [`ObjectId`] in a persistent map, so a clone
//!   is an O(1) snapshot. Its root is a [`Story`], a tree of blocks:
//!   [`Paragraph`]s, text with runs of character properties ([`Run`]) and
//!   properties of its own ([`Props`]), and containers ([`Block`]) of each
//!   [`Kind`]: lists of items, quotes, and tables of rows of cells; and
//!   rules, which hold nothing. Headings and code blocks are paragraphs,
//!   set apart by their style. Beside the story there may be a [`Sheet`],
//!   the document's own stylesheet: [`Style`]s of each [`StyleKind`], each
//!   based on another of its kind in a chain of any depth, and named parts
//!   styles refer to (faces, colours, tab layouts and list definitions),
//!   with the defaults every chain ends in. Paragraphs name their styles by
//!   ID ([`Value::Ref`]).
//! - [`Op`]: the changes. [`Document::apply`] applies one and returns the
//!   operations that undo it; [`rebase`] moves one over a concurrent one,
//!   consulting the document both apply to, and [`conflict`] says when
//!   two can't both have their way.
//! - [`Transaction`] and [`History`]: named, undoable steps.
//! - [`Snapshot`]: a document as plain data, for storing, checked when
//!   it's read back ([`Document::from_snapshot`]).
//!
//! # What a caller may rely on
//!
//! - **Pure.** No I/O, threads, clocks or environment (Clippy's
//!   `crates/clippy.toml`), and it builds for WebAssembly (CI). The same
//!   operations on the same document give the same document, on every
//!   platform. Documents, snapshots, operations and history are `Send` and
//!   `Sync`, for workers to take (`tests/properties.rs`,
//!   `documents_go_between_threads`).
//! - **Valid means:** the root is a story, and the story is a tree: every
//!   object but the root, the stylesheet and its styles is held by exactly
//!   one container, of a kind that can hold it ([`Object::holds`]); there's
//!   always a paragraph somewhere in it, though containers may be empty;
//!   each paragraph's runs cover its text exactly, normalized; there's at
//!   most one stylesheet; and every style is based on a style of its own
//!   kind, never on itself through any chain, and parts on nothing. A
//!   document is always valid: operations keep it so, and
//!   [`Document::from_snapshot`] refuses a snapshot that isn't, saying why
//!   ([`SnapshotError`]) (`tests/properties.rs`,
//!   `documents_survive_storing_as_snapshots`, whose random edits always
//!   leave a valid snapshot, and `broken_snapshots_are_refused`;
//!   `tests/blocks.rs`, `snapshots_of_trees_are_checked`;
//!   `tests/styles.rs`, `broken_stylesheets_are_refused`).
//! - **Addresses.** Objects are addressed by ID, never by index, and
//!   [`Document::new_id`] never gives out an ID the document ever held,
//!   deleted ones and those in a snapshot it was read from included
//!   (`tests/properties.rs`, `new_ids_are_never_ones_the_document_held`).
//!   A text position is a paragraph and an offset in Unicode scalar
//!   values; a place among a container's children is the child before it.
//! - **Operations fail whole.** One that doesn't apply returns an
//!   [`Error`] and changes nothing, and none panics; a transaction applies
//!   all its operations or none (`tests/properties.rs`,
//!   `invalid_operations_are_refused_and_change_nothing` and
//!   `transactions_are_all_or_nothing_and_undo_exactly`; `tests/blocks.rs`,
//!   `invalid_block_operations_are_refused_and_change_nothing`;
//!   `tests/styles.rs`, `bases_that_would_loop_and_other_bad_edits_are_refused`).
//! - **Inverses are exact.** Applying an operation's inverse restores the
//!   document exactly; a deleted block's or style's inverse carries
//!   everything that was in it. [`History`]'s undo and redo walk back and
//!   forth through exactly the states the transactions made
//!   (`tests/properties.rs`, `inverses_restore_the_document` and
//!   `transactions_are_all_or_nothing_and_undo_exactly`; `tests/blocks.rs`,
//!   `a_deletion_undoes_with_everything_in_it`; `tests/styles.rs`,
//!   `styles_are_made_based_and_deleted_and_undo_exactly`).
//! - **Equal is equal.** Two documents are equal when their objects are,
//!   whatever edits made them: runs are kept normalized, with no empty
//!   runs and no equal neighbours (`props`' unit test
//!   `normalize_merges_and_drops`; `tests/properties.rs`,
//!   `runs_read_back_normalized`).
//! - **Snapshots.** A clone never changes, whatever is done to the
//!   document after it (`tests/properties.rs`, `snapshots_never_change`),
//!   and [`Document::changed_since`] names every object that differs from
//!   an earlier clone (`tests/properties.rs`,
//!   `changes_since_cover_every_difference`). A document stored as a
//!   [`Snapshot`] reads back equal (`documents_survive_storing_as_snapshots`).
//! - **Rebasing converges:** either order of two concurrent operations,
//!   each rebased over the other, gives the same document, by the rules
//!   [`rebase`] lists. M1's operations always meet by those rules; only
//!   deletions and moves of blocks, and bases of styles, ever conflict,
//!   and then the first wins (`tests/properties.rs`, `rebasing_converges`,
//!   `m1s_operations_never_conflict`, `only_deletions_and_moves_conflict`,
//!   and the `ties` modules there and in `tests/blocks.rs`).
//! - **Styles never loop:** a base that would make a style based on
//!   itself is refused, and of two concurrent ones that would together,
//!   the one rebased over the other is dropped (`tests/properties.rs`,
//!   `bases_never_loop` and `concurrent_bases_never_loop`;
//!   `tests/styles.rs`,
//!   `of_two_bases_that_together_loop_the_one_rebased_later_is_dropped`).
//! - **The story and the stylesheet stay apart:** operations on blocks
//!   never touch the stylesheet, nor those on styles the story
//!   (`tests/styles.rs`, `bases_that_would_loop_and_other_bad_edits_are_refused`).
//! - **Any depth.** Nothing walks the tree by recursion, so no snapshot
//!   can nest blocks deep enough to exhaust the stack (`tests/blocks.rs`,
//!   `a_story_nested_deep_is_walked_without_recursion`).
//! - **Serialized by position.** Operations, transactions and snapshots
//!   are serde data that read back as they were (`tests/properties.rs`,
//!   `operations_survive_serialization`); files store enums by their
//!   variants' positions, so new variants only ever go at the end
//!   (`store`'s `tests/store.rs`, `the_encoding_is_pinned` and its
//!   siblings for blocks, snapshots and stylesheets).
//!
//! # What it doesn't promise
//!
//! - What properties mean. Keys are names and values one of [`Value`]'s
//!   kinds; doc checks neither which keys are set nor what kind their
//!   values are, and a [`Value::Ref`] may name nothing. `story::keys`
//!   gives them meaning, and its readers take a value of the wrong kind,
//!   or a reference to nothing, as unset.
//! - Merging conflicts. Rebasing is property-tested but unused until
//!   collaboration; where two operations conflict, one is dropped, not
//!   merged.
//! - That a [`Transaction`] given to [`History`] was applied: the caller
//!   records only what it applied.
//! - Speed beyond what's said: whole-story queries, such as
//!   [`Document::paragraphs`], take time in the story's size, and there
//!   are no chunked tables yet (M3).
//! - [`Document::dump`]'s form, but that the fixtures of older files
//!   (`store`'s `tests/fixtures.rs`) hold it.

mod document;
mod history;
mod id;
mod idmap;
mod op;
mod props;
mod rebase;

pub use document::{
    Block, Document, Kind, Object, Paragraph, Sheet, Snapshot, SnapshotError, Story, Style,
    StyleKind,
};
pub use history::{History, Transaction};
pub use id::ObjectId;
pub use op::{Error, Op};
pub use props::{Props, Run, Value};
pub use rebase::{conflict, rebase};
