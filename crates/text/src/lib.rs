//! Fonts, shaping, paragraph layout in fixed-point units, and editing:
//! carets, selections and hit-testing (docs/ARCHITECTURE.md, section 9).
//!
//! - [`Fonts`]: font files loaded with skrifa, by family, weight and
//!   slant, and the families characters fall back to (`spec/text/fonts.ron`).
//! - [`layout`] and [`layout_with`]: a [`Paragraph`] of styled text laid
//!   out to a width. Its text is divided into items of one style, face,
//!   bidi level and script, shaped with HarfRust. Ragged lines are broken
//!   first fit where ICU4X allows, by the text's languages; justified ones
//!   ([`Align::Justify`]) by Knuth and Plass's optimal algorithm, and
//!   stretched. Each line is ordered for display by the bidi algorithm.
//! - [`Hyphenator`] and [`Hyphenation`]: hyphenation by Liang's
//!   algorithm, from TeX's pattern files as they ship; a [`Setting`] says
//!   whether a paragraph's words are hyphenated, where its [`Tabs`] are,
//!   how tall its lines are and how far its first line is indented.
//! - [`Layout::paint`]: the paragraph as display-list glyph runs (`scene`),
//!   each glyph with the text it shows.
//! - [`Layout`]'s editing queries ([`Cursor`], [`Caret`]): where the
//!   caret stands, how it moves on screen and through the text, by
//!   graphemes, words and sentences, what a click hits, and how a
//!   selection is highlighted.
//! - [`units`]: layout units, 914 400 to the inch.
//!
//! Invariants:
//!
//! - Positions are integers in layout units, and line breaking scores
//!   lines in integers too, so the same paragraph lays out identically on
//!   every platform and in WebAssembly.
//! - Lines cover the paragraph's text in order, and each fits its width,
//!   but for spaces hanging at its end, unless one grapheme alone is
//!   wider. A justified line, but the last and those ending at a hard
//!   break, is as wide as the paragraph where it has space to stretch.
//! - A hyphen layout adds is drawn, but isn't in the text: the caret
//!   never stands inside it, and it reads as a soft hyphen.
//! - Every place the caret can stand is between graphemes, and
//!   hit-testing where the caret is drawn finds that place again.
//! - No I/O: fonts and hyphenation patterns are bytes the caller read.

mod breaking;
mod caret;
mod font;
mod hyphen;
mod layout;
mod paint;
mod shape;
mod tabs;
pub mod units;
mod words;

pub use caret::{Affinity, Caret, Cursor, Rect};
pub use font::{Face, FaceId, FamilySpec, FontError, FontSpec, Fonts};
pub use hyphen::{Hyphenation, Hyphenator};
pub use layout::{
    Align, Glyph, GlyphRun, Layout, Ligatures, Line, Paragraph, Setting, Span, Style, layout,
    layout_with,
};
pub use locale::{Direction, Language};
pub use paint::Decoration;
pub use tabs::{TabAlign, TabStop, Tabs};
