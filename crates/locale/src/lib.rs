//! Unicode and locale services on ICU4X (docs/ARCHITECTURE.md, section
//! 9): so far, what text layout and editing need, the segmentation and
//! bidirectional ordering of text by its languages' rules.
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
//! # What a caller may rely on
//!
//! - **Pure and the same everywhere.** No I/O, threads, clocks or
//!   environment (Clippy's `crates/clippy.toml`), and it builds for
//!   WebAssembly (CI). The Unicode data is compiled in, never read from the
//!   system, so the same text gives the same answers on every platform (the
//!   unit tests, run by CI on Linux, Windows and macOS).
//! - **Offsets are bytes** into UTF-8 text, always on character
//!   boundaries. Every division covers the text, in order, with nothing
//!   empty: graphemes, words, sentences and script runs; a paragraph's
//!   line breaks rise from after its start to its end, where the last
//!   always is; and a line's [`visual_order`] is an order of all its
//!   items, whatever their levels (`tests/coverage.rs`,
//!   `every_division_covers_the_text` and `any_levels_have_a_visual_order`).
//! - **Languages decide:** Japanese and Chinese lines break by CSS's
//!   `normal` rules, which let a line start with a small kana, and other
//!   text by UAX #14's default, `strict`; Chinese and Japanese words come
//!   from a dictionary, and Thai, Lao, Khmer and Burmese from models; a
//!   change of language is a word boundary; Greek asks with `;`; and
//!   hyphenated words count as one (`segment`'s unit tests, such as
//!   `japanese_lines_may_start_with_a_small_kana`,
//!   `japanese_words_come_from_a_dictionary` and
//!   `greek_asks_with_a_semicolon`).
//! - **Graphemes stay whole:** combining marks, emoji sequences and Hangul
//!   syllables are one each (`segment`'s unit test
//!   `graphemes_keep_marks_and_sequences_whole`).
//! - **Bidi** follows UAX #9: the first strong character sets a
//!   paragraph's direction unless it's given, and whitespace at a line's
//!   end takes the paragraph's level (`bidi`'s unit tests).
//! - **Language tags** are kept canonical, so `EN-us` is `en-US`, and a tag
//!   that isn't BCP 47 is an [`Error`], never a panic (`language`'s unit
//!   tests `tags_are_canonical` and `nonsense_is_refused`).
//!
//! # What it doesn't promise
//!
//! - That a line break falls between graphemes: UAX #14 may break inside
//!   one, and `text` keeps lines from ending there.
//! - Valid language ranges: those given with text are byte ranges of it,
//!   in order and on character boundaries, which callers keep; others may
//!   panic.
//! - Collation, case mapping, and number and date formats, which come
//!   with the tracks that need them.
//! - The same answers across versions of Unicode and ICU4X: a newer one
//!   may divide some text otherwise.

mod bidi;
mod language;
mod script;
mod segment;

pub use bidi::{Bidi, Direction, visual_order};
pub use language::{Error, Language};
pub use script::{Script, script_runs};
pub use segment::{Break, Word, graphemes, line_breaks, sentences, word_count, words};
