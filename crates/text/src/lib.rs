//! Fonts, shaping, paragraph layout in fixed-point units, and editing:
//! carets, selections and hit-testing (docs/ARCHITECTURE.md, section 9).
//! Every track lays out and edits its text with it, so text in a document,
//! a slide, a cell and the chrome behaves alike.
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
//!   how tall its lines are, how far its first line is indented, and
//!   what [`Inline`] objects, such as pictures, stand in its text,
//!   each for a character, as boxes on the baseline that lines break
//!   around and grow to hold; each line says where they went
//!   ([`Placed`]), for the caller to draw.
//! - [`Layout::paint`]: the paragraph as display-list glyph runs (`scene`),
//!   each glyph with the text it shows, and its underlines and lines
//!   through ([`Decoration`]).
//! - [`Layout`]'s editing queries ([`Cursor`], [`Caret`]): where the
//!   caret stands, how it moves on screen and through the text, by
//!   graphemes, words and sentences, what a click hits, and how a
//!   selection is highlighted.
//! - [`units`]: layout units, 914 400 to the inch.
//!
//! # What a caller may rely on
//!
//! - **Pure.** No I/O, threads, clocks or environment (Clippy's
//!   `crates/clippy.toml`), and it builds for WebAssembly (CI): fonts and
//!   hyphenation patterns are bytes the caller read. Fonts, patterns and
//!   layouts go between threads (`tests/threads.rs`,
//!   `fonts_and_layouts_go_between_threads`).
//! - **Units:** positions are integers in layout units, 914 400 to the
//!   inch, in which points, millimetres and 96-dpi pixels are whole
//!   (`units`' unit tests), from the paragraph's top left, y growing down.
//! - **The same everywhere:** layout, and the scoring of lines, is in
//!   integers, so the same paragraph lays out identically on every
//!   platform and in WebAssembly, to the unit (`tests/layout.rs`,
//!   `layout_cases`, whose cases in `spec/text/layout/` give exact widths
//!   and baselines; `breaking`'s unit test `badness_is_texs`;
//!   `tools/shot/tests/golden.rs`, which CI runs on Linux, Windows and
//!   macOS).
//! - **Lines cover the text and fit:** lines cover the paragraph's text
//!   in order, top to bottom, each as tall as its ascent and descent, and
//!   each fits its room, a first line its indent's, but for spaces hanging
//!   at its end, unless one grapheme alone is wider; a word too wide for
//!   a line breaks between graphemes, and no line ends inside one
//!   (`tests/properties.rs`, `lines_cover_the_text_and_fit`,
//!   `indented_lines_fit_their_room_and_stack_at_their_height`,
//!   `a_line_never_ends_inside_a_grapheme` and
//!   `a_word_broken_where_it_kerns_still_fits`). A justified line, but the
//!   last and those ending at a hard break, is as wide as the paragraph
//!   where it has space to stretch (`tests/layout.rs`, `layout_cases`, the
//!   `justify` cases).
//! - **The caret:** every place it can stand is between graphemes, and
//!   hit-testing where it's drawn finds that place again; moving right
//!   then left on a line comes back, and walking right crosses every line
//!   in its order on screen (`tests/properties.rs`,
//!   `carets_are_found_where_they_are_drawn`,
//!   `moving_right_then_left_on_a_line_returns` and
//!   `walking_right_crosses_every_line`; moves by words and sentences in
//!   `tests/layout.rs`, `layout_cases`).
//! - **A hyphen layout adds** is drawn but isn't in the text: the caret
//!   never stands inside it, and its run reads it as a soft hyphen; a
//!   soft hyphen where a line breaks shows as one (`tests/paint.rs`,
//!   `an_added_hyphen_reads_as_a_soft_hyphen`; `tests/layout.rs`,
//!   `layout_cases`, the `soft-hyphen` and `hyphenate-*` cases).
//! - **Inline objects** take their room in the line and lift it, lines
//!   break around them, the caret stands either side of each, and one
//!   that stands for no character is ignored (`tests/objects.rs`).
//! - **Glyphs carry their text,** in the order it reads, so PDF and screen
//!   readers get it back (`tests/paint.rs`, `glyphs_carry_the_text_they_show`).
//! - **Outside input gives errors, never panics:** bytes that aren't a
//!   font, or a font cut short, are a [`FontError`] and load nothing
//!   (`tests/fonts.rs`, `what_isnt_a_font_is_refused`; the `font_file`
//!   fuzz target); any text reads as hyphenation patterns
//!   (`tests/hyphenation.rs`, `any_patterns_read_and_hyphenate`; the
//!   `hyphenation_patterns` fuzz target); and any UTF-8, typed or pasted,
//!   lays out with every caret place reachable (the `paragraph` fuzz
//!   target).
//! - **Fonts:** the nearest face of a family is chosen, italic in a family
//!   without one is the upright face slanted, characters no face has are
//!   reported, and a language picks its local forms (`tests/fonts.rs` and
//!   `tests/languages.rs`).
//!
//! # What it doesn't promise
//!
//! - A layout for bad spans: [`layout`] and [`layout_with`] panic if the
//!   spans don't cover the text exactly, or end inside a character, or
//!   there are no fonts. Callers make the spans.
//! - The same lines across versions of the fonts, HarfRust or ICU4X, or
//!   of TeX's patterns.
//! - Optimal ragged lines: they're first fit, so typing never moves a line
//!   above the caret. Lines stretch, never shrink.
//! - Vertical text, input-method composition (the app's), or embedding
//!   fonts in documents (`store`'s, once a document uses a font that isn't
//!   bundled).

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
    Align, Glyph, GlyphRun, Inline, Layout, Ligatures, Line, Paragraph, Placed, Setting, Span,
    Style, layout, layout_with,
};
pub use locale::{Direction, Language};
pub use paint::Decoration;
pub use tabs::{TabAlign, TabStop, Tabs};
