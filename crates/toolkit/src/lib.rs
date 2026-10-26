//! The generic shell's toolkit: draws the chrome model with the documents'
//! renderer and turns pointing into what it hits (docs/ARCHITECTURE.md,
//! section 8), as M2's UX design settled it and its approved mockups show
//! (reviews frhewnk0 and 3ce247c2):
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
//! icons, and [`Labels`] laid out once and kept. Everything is in layout
//! units, with the window's top left at the origin. Each widget is laid
//! out from the model, then hit-tested and drawn from that one layout.
//!
//! Invariants:
//!
//! - Drawing and hit-testing use the same layout: what's drawn where a
//!   click lands is what the click hits.
//! - Widgets read the model; they never change it. What a hit means is the
//!   app's to decide.

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
