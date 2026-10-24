use serde::{Deserialize, Serialize};

use crate::props::{Props, Run, Value};
use crate::{Object, ObjectId, Style};

/// A change to a document. Offsets and lengths count Unicode scalar values
/// in a paragraph's text.
///
/// Operations name objects by ID, never by index, so they can be rebased
/// over concurrent changes ([`crate::rebase`]). Each is small; edits people
/// make are transactions of several ([`crate::Transaction`]). A place among
/// a container's children is the child it comes after, or `None` for the
/// first place.
///
/// Files store operations by their variants' positions
/// (spec/format/README.md): new variants go at the end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// Inserts text into a paragraph.
    InsertText {
        /// The paragraph.
        paragraph: ObjectId,
        /// Where the text goes.
        offset: u32,
        /// The text.
        text: String,
        /// Its formatting, covering it exactly.
        runs: Vec<Run>,
    },
    /// Deletes text from a paragraph.
    DeleteText {
        /// The paragraph.
        paragraph: ObjectId,
        /// Where the deleted text starts.
        offset: u32,
        /// How many characters go.
        len: u32,
    },
    /// Splits a paragraph in two: the text after `offset` moves into a new
    /// paragraph right after it.
    SplitParagraph {
        /// The paragraph.
        paragraph: ObjectId,
        /// Where it splits.
        offset: u32,
        /// The new paragraph's ID, unused until now.
        new: ObjectId,
        /// The new paragraph's properties.
        new_props: Props,
    },
    /// Joins a paragraph with the one right after it, which goes.
    JoinParagraphs {
        /// The paragraph that stays.
        paragraph: ObjectId,
        /// Its length, where the next one's text starts.
        offset: u32,
        /// The paragraph after it, whose text it takes.
        next: ObjectId,
    },
    /// Sets a character property along a stretch of a paragraph.
    SetCharProp {
        /// The paragraph.
        paragraph: ObjectId,
        /// Where the stretch starts.
        offset: u32,
        /// How many characters it covers.
        len: u32,
        /// The property.
        key: String,
        /// Its value, or `None` to unset it.
        value: Option<Value>,
    },
    /// Sets a paragraph's property.
    SetParaProp {
        /// The paragraph.
        paragraph: ObjectId,
        /// The property.
        key: String,
        /// Its value, or `None` to unset it.
        value: Option<Value>,
    },
    /// Creates a block with everything in it, at a place in a container.
    InsertBlock {
        /// The container.
        parent: ObjectId,
        /// The child it comes after, or `None` to come first.
        after: Option<ObjectId>,
        /// The block and everything in it, each by an ID unused until now:
        /// the block first, then the rest in reading order. Their parents
        /// and children say how they nest; the block's parent is `parent`.
        objects: Vec<(ObjectId, Object)>,
    },
    /// Deletes a block with everything in it.
    DeleteBlock {
        /// The block: a paragraph or a container.
        block: ObjectId,
    },
    /// Moves a block with everything in it to another place, in the same
    /// container or another.
    MoveBlock {
        /// The block: a paragraph or a container.
        block: ObjectId,
        /// The container it moves to.
        parent: ObjectId,
        /// The child it comes after there, or `None` to come first.
        after: Option<ObjectId>,
    },
    /// Sets a container's property, such as whether a list is numbered.
    SetBlockProp {
        /// The container.
        block: ObjectId,
        /// The property.
        key: String,
        /// Its value, or `None` to unset it.
        value: Option<Value>,
    },
    /// Creates a style or a named part, at a place in the stylesheet.
    InsertStyle {
        /// The stylesheet.
        sheet: ObjectId,
        /// The style it comes after, or `None` to come first.
        after: Option<ObjectId>,
        /// Its ID, unused until now.
        id: ObjectId,
        /// The style: in `sheet`, based on a style of its kind if on any.
        style: Style,
    },
    /// Deletes a style or a named part that no style is based on.
    /// Paragraphs that name it, and styles that refer to it, take what they
    /// would without it.
    DeleteStyle {
        /// The style.
        style: ObjectId,
    },
    /// Sets a property of a style or a part, or of the stylesheet: one of
    /// its defaults, or what the whole document shares.
    SetStyleProp {
        /// The style, part or stylesheet.
        style: ObjectId,
        /// The property.
        key: String,
        /// Its value, or `None` to unset it.
        value: Option<Value>,
    },
    /// Bases a style on another of its kind, or on none. A base that would
    /// make the style based on itself, through any chain, is refused.
    SetBase {
        /// The style.
        style: ObjectId,
        /// The style it's based on from now on, if any.
        base: Option<ObjectId>,
    },
}

/// Why an operation couldn't be applied. A failed operation changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The operation names a paragraph the document doesn't have.
    #[error("there is no paragraph {0}")]
    NoSuchParagraph(ObjectId),
    /// A position or stretch lies outside its paragraph's text.
    #[error("{paragraph} has {length} characters, so {offset}+{len} is out of range")]
    OutOfRange {
        /// The paragraph.
        paragraph: ObjectId,
        /// Where the stretch starts.
        offset: u32,
        /// How long it is.
        len: u32,
        /// How long the paragraph is.
        length: u32,
    },
    /// Inserted text whose runs don't cover it exactly.
    #[error("the inserted text has {text} characters but its runs cover {runs}")]
    RunsMismatch {
        /// The text's length.
        text: u32,
        /// The runs' total length.
        runs: u32,
    },
    /// A split's new ID is already some object's.
    #[error("{0} is already in use")]
    IdInUse(ObjectId),
    /// A join of paragraphs that aren't neighbours in one story.
    #[error("{next} doesn't follow {paragraph} in the same story")]
    NotAdjacent {
        /// The first paragraph.
        paragraph: ObjectId,
        /// The one that should follow it.
        next: ObjectId,
    },
    /// A join whose offset isn't the paragraph's length.
    #[error("joining at {offset}, but {paragraph} has {length} characters")]
    OffsetMismatch {
        /// The paragraph.
        paragraph: ObjectId,
        /// The join's offset.
        offset: u32,
        /// The paragraph's length.
        length: u32,
    },
    /// The operation names a block the document doesn't have, or a
    /// container that isn't one.
    #[error("there is no such block as {0}")]
    NoSuchBlock(ObjectId),
    /// A place after something that isn't a child of the container.
    #[error("{child} isn't a child of {parent}")]
    NotAChild {
        /// The container.
        parent: ObjectId,
        /// What the place is after.
        child: ObjectId,
    },
    /// A container that can't hold that kind of block, such as a list
    /// holding a paragraph rather than items.
    #[error("{parent} can't hold {child}")]
    WrongKind {
        /// The container.
        parent: ObjectId,
        /// The block.
        child: ObjectId,
    },
    /// Inserted objects that don't make one block: an object nothing in
    /// them holds, one held twice, or one whose parent isn't the one
    /// holding it.
    #[error("the objects inserted with {0} don't make one block")]
    Malformed(ObjectId),
    /// A block moved into itself or into something in it.
    #[error("{block} can't move into {parent}, which is in it")]
    Cycle {
        /// The block.
        block: ObjectId,
        /// Where it would go.
        parent: ObjectId,
    },
    /// The root story can't be deleted or moved.
    #[error("{0} is the root")]
    Root(ObjectId),
    /// Deleting the story's last paragraph, which would leave nowhere to
    /// type.
    #[error("{0} holds the story's last paragraph")]
    LastParagraph(ObjectId),
    /// The operation names a style the document doesn't have, or a
    /// stylesheet that isn't its own.
    #[error("there is no such style as {0}")]
    NoSuchStyle(ObjectId),
    /// A style based on what isn't a style of its kind, or a part based on
    /// anything.
    #[error("{style} can't be based on {base}")]
    WrongBase {
        /// The style.
        style: ObjectId,
        /// What it would be based on.
        base: ObjectId,
    },
    /// A base that would make a style based on itself.
    #[error("basing {style} on {base} would base it on itself")]
    Loop {
        /// The style.
        style: ObjectId,
        /// What it would be based on.
        base: ObjectId,
    },
    /// Deleting a style another is based on.
    #[error("{by} is based on {style}")]
    BasedOn {
        /// The style.
        style: ObjectId,
        /// A style based on it.
        by: ObjectId,
    },
}
