//! Unicode and locale services on ICU4X: segmentation, bidi, collation, case
//! mapping, and number and date formats (docs/ARCHITECTURE.md, section 9).
//! It has what text layout and editing need:
//!
//! - [`Language`]: a BCP 47 language tag, such as `sr` or `ja`.
//! - [`line_breaks`] and [`graphemes`]: where lines may break, and where
//!   user-perceived characters begin and end (UAX #14 and #29).
//! - [`words`], [`word_count`] and [`sentences`]: text divided into words
//!   and sentences by its languages' rules (UAX #29), for moving and
//!   selecting by them, and counting.
//! - [`script_runs`]: text divided by writing system (UAX #24), for shaping.
//! - [`Bidi`]: embedding levels from the Unicode Bidirectional Algorithm
//!   (UAX #9), run on ICU4X's data, and [`visual_order`] for a line.
//!
//! Invariants:
//!
//! - Offsets are byte offsets into UTF-8 text, always on character
//!   boundaries.
//! - The same text gives the same answers on every platform: the Unicode
//!   data is compiled in, never read from the system.

mod bidi;
mod language;
mod script;
mod segment;

pub use bidi::{Bidi, Direction, visual_order};
pub use language::{Error, Language};
pub use script::{Script, script_runs};
pub use segment::{Break, Word, graphemes, line_breaks, sentences, word_count, words};
