use serde::{Deserialize, Serialize};

use crate::ObjectId;
use crate::props::{Props, Run, Value};

/// A change to a document. Offsets and lengths count Unicode scalar values
/// in a paragraph's text.
///
/// Operations name objects by ID, never by index, so they can be rebased
/// over concurrent changes ([`crate::rebase`]). Each is small; edits people
/// make are transactions of several ([`crate::Transaction`]).
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
}
