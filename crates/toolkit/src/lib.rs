//! The generic shell's toolkit: draws the chrome model with the documents'
//! renderer and turns pointing into what it hits (docs/ARCHITECTURE.md,
//! section 8), as M2's UX design settled it and its approved mockups show
//! (reviews frhewnk0 and 3ce247c2). A port to a platform's own widgets
//! replaces it and keeps the model.
//!
//! - [`TitleBar`]: xWorks' own title bar, with the menus' titles, the
//!   document's name, the contextual buttons and the window's own.
//! - [`Popup`]: a menu open from the bar, its submenus, and a field's
//!   choices.
//! - [`RulerView`]: the ruler over a text column, with indents and tab
//!   stops.
//! - [`InspectorView`]: the inspector's tabs of typed controls, scrolled
//!   where a tab is taller than the panel.
//! - [`PaletteView`]: the command palette.
//! - [`status_bar`]: the line along a window's foot.
//! - [`Field`]: a small field of text, edited as documents' text is.
//! - [`Icon`] and [`Icons`]: Lucide's icons, read from SVG and drawn as
//!   fills.
//!
//! Widgets draw with a [`Kit`]: the fonts, the [`Theme`] (sizes from
//! `spec/themes/chrome.ron`, colours from `light.ron` or `dark.ron`), the
//! icons, and [`Labels`] laid out once and kept. Each widget is laid out
//! from the model, then hit-tested and drawn from that one layout.
//!
//! # What a caller may rely on
//!
//! - **Pure.** No I/O, threads, clocks or environment (Clippy's
//!   `crates/clippy.toml`), and it builds for WebAssembly (CI): themes are
//!   data the caller read, and Lucide's icons are built in.
//! - **Units:** everything is in layout units, with the window's top left
//!   at the origin, and draws as display lists (`scene`).
//! - **What's drawn is what's hit:** drawing and hit-testing use the same
//!   layout, so what's drawn where a click lands is what the click hits
//!   (`tests/chrome.rs`, `the_title_bar_hits_what_it_draws`,
//!   `popups_hit_their_rows_and_stay_in_the_window`,
//!   `the_ruler_hits_its_stops_and_indents`,
//!   `the_inspector_hits_its_controls` and `the_palette_hits_what_it_found`).
//! - **Widgets read the model; they never change it.** What a hit means is
//!   the app's to decide: a hit names what it hit.
//! - **The look is data,** light and dark, each read from its file
//!   (`tests/chrome.rs`, `both_looks_read_and_differ`), and the chrome draws
//!   as its approved goldens (`tools/shot/tests/golden.rs`, the `chrome-*`,
//!   `ruler-*` and `mockup-*` scenes).
//! - **Popups stay in the window,** and the inspector scrolls what doesn't
//!   fit, a change to one control changing only its row's items
//!   (`tests/chrome.rs`, `popups_hit_their_rows_and_stay_in_the_window`,
//!   `the_inspector_scrolls_what_doesnt_fit` and
//!   `a_change_to_a_control_changes_only_its_row`).
//! - **Labels fit:** notes take the room their words need, and what still
//!   doesn't fit its field ends in an ellipsis (`tests/chrome.rs`,
//!   `notes_take_their_room_and_labels_fit_their_fields`); a label kept
//!   from before the fonts it needed loaded is laid out again (`kit`'s unit
//!   test `a_label_missing_characters_is_laid_out_again`).
//! - **Fields edit as documents do,** by graphemes and words
//!   (`tests/chrome.rs`, `fields_edit_as_documents_do`).
//! - **Icons:** every icon xWorks uses is built in, reads, and draws inside
//!   its square; SVG with no shapes is an [`IconError`] (`tests/icons.rs`;
//!   `icon`'s unit test `an_svg_without_shapes_is_refused`).
//!
//! # What it doesn't promise
//!
//! - A general-purpose GUI library: it draws xWorks' chrome model and
//!   nothing else.
//! - Pixels beyond the goldens': sizes and colours are the theme's, and a
//!   theme changed changes them.
//! - Any SVG: it reads the subset Lucide's icons use.

mod field;
mod icon;
mod inspector;
mod kit;
mod palette;
mod popup;
mod ruler;
mod theme;
mod title;

use scene::Item;
use text::units::PIXEL;

pub use field::{Field, FieldKey};
pub use icon::{Icon, IconError};
pub use inspector::{BarHit, ControlRef, InspectorHit, InspectorView};
pub use kit::{Icons, Kit, Labels, Rect, Type};
pub use palette::{PaletteHit, PaletteView, SHOWN};
pub use popup::{Popup, PopupRow};
pub use ruler::{RulerHit, RulerView};
pub use theme::{ColorSpec, Colors, Sizes, Theme};
pub use title::{TitleBar, TitleHit, TitleModel};

/// The path of `rect` with corners of radius `r`, as the chrome draws its
/// boxes and the views their pages' shadows.
pub fn rounded(rect: Rect, r: i64) -> scene::Path {
    kit::rounded_path(rect, r)
}

/// The path of `rect` with corners of radius `r`, less `hole`: the part
/// of a shadow that shows round the page over it, as only that is drawn.
pub fn ring(rect: Rect, r: i64, hole: Rect) -> scene::Path {
    kit::ring_path(rect, r, hole)
}

/// The status bar along the foot of a window `width` wide with its bottom
/// at `bottom`: `left` at its start, and `right` at its end.
pub fn status_bar(kit: &Kit<'_>, left: &str, right: &str, width: i64, bottom: i64) -> Vec<Item> {
    let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
    let h = s.status_bar.units();
    let top = bottom - h;
    let mut out = Vec::new();
    kit::fill(&mut out, Rect::new(0, top, width, h), c.chrome);
    kit::fill(&mut out, Rect::new(0, top, width, PIXEL), c.line);
    let quiet = kit.ty(s.label, 400);
    let edge = s.menu_left.units();
    kit.text(&mut out, left, edge, top, h, quiet, c.secondary);
    kit.text_right(&mut out, right, width - edge, top, h, quiet, c.secondary);
    out
}
