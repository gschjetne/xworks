//! The app: documents, windows, commands and autosave, without platform code;
//! the shells drive it (docs/ARCHITECTURE.md, section 8). M1 has its first
//! form, Elm-style:
//!
//! - [`Resources`]: what every window shares, loaded once.
//! - [`session`]: scripted sessions, played headless.
//! - [`Window`]: one document in one window, with the chrome xWorks draws
//!   around it: its title bar and menus, the ruler, the inspector, the
//!   command palette and the status bar, light or dark. Shells feed it
//!   [`Event`]s and carry out the [`Effect`]s it returns; [`Window::view`]
//!   draws it as a display list, which the shell renders.
//! - [`Editor`]: the document's story, edited, on pages or as a draft: the
//!   first part behind the canvas contract (`part`).
//! - [`Blink`]: the caret's blinking, at the system's rate, which the
//!   shell times; headless, the caret always shows.
//! - [`Recent`]: the documents opened lately.
//! - [`Settings`]: what the person chose that's kept between runs, such as
//!   the unit the ruler and the inspector measure in.
//! - [`picture`]: PNG, JPEG, GIF and WebP files decoded, in sRGB, to
//!   place in documents and draw.
//!
//! Every edit is a transaction, written to the document's file as it's
//! committed; there's no saving and no "Save changes?". A run of typing
//! undoes as one. A Markdown file opens as itself, in the draft view, and
//! each edit writes it back whole, through a rename, as the `markdown`
//! crate writes it; an edit Markdown can't hold asks first, and makes the
//! document an `.xw` file beside it only if the person says so.
//!
//! app is an impure crate: it reads and writes documents.
//!
//! Invariants:
//!
//! - What's drawn is the document as it stands, laid out afresh only where
//!   it changed.
//! - The same events give the same document and the same frames, so scripted
//!   sessions can be played headless and compared.

mod blink;
mod blocks;
mod editor;
pub mod picture;
mod recent;
pub mod recorder;
mod resources;
pub mod session;
mod settings;
mod view;
mod window;

pub use blink::{Blink, BlinkRate, Blinking};
pub use editor::{
    Choice, Edge, Editor, EditorError, Effect, Event, PointerShape, Position, View, WindowAction,
};
pub use recent::Recent;
pub use resources::{ResourceError, Resources};
pub use settings::{RULER_FROM, RulerFrom, Settings, UNITS, units_of_locale};
pub use window::Window;
