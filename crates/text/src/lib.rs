//! Fonts, shaping, paragraph layout in fixed-point units, and editing:
//! carets, selections and hit-testing (docs/ARCHITECTURE.md, section 9).
//!
//! - [`Fonts`]: font files loaded with skrifa, by family, weight and
//!   slant, and the families characters fall back to (`spec/text/fonts.ron`).
//! - [`layout`]: a [`Paragraph`] of styled text laid out to a width. Its
//!   text is divided into items of one style, face, bidi level and script,
//!   shaped with HarfRust; lines are broken first fit where ICU4X allows,
//!   and each is ordered for display by the bidi algorithm.
//! - [`Layout::paint`]: the paragraph as display-list glyph runs (`scene`),
//!   each glyph with the text it shows.
//! - [`Layout`]'s editing queries ([`Cursor`], [`Caret`]): where the
//!   caret stands, how it moves on screen and through the text, what a
//!   click hits, and how a selection is highlighted.
//! - [`units`]: layout units, 914 400 to the inch.
//!
//! Invariants:
//!
//! - Positions are integers in layout units, so the same paragraph lays out
//!   identically on every platform and in WebAssembly.
//! - Lines cover the paragraph's text in order, and each fits its width,
//!   but for spaces hanging at its end, unless one grapheme alone is
//!   wider.
//! - Every place the caret can stand is between graphemes, and
//!   hit-testing where the caret is drawn finds that place again.
//! - No I/O: fonts are bytes the caller read.

mod caret;
mod font;
mod layout;
mod paint;
mod shape;
pub mod units;

pub use caret::{Affinity, Caret, Cursor, Rect};
pub use font::{Face, FaceId, FamilySpec, FontError, FontSpec, Fonts};
pub use layout::{Align, Glyph, GlyphRun, Layout, Line, Paragraph, Span, Style, layout};
pub use locale::{Direction, Language};
