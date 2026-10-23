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
//! - [`Recent`]: the documents opened lately.
//! - [`picture`]: PNG, JPEG, GIF and WebP files decoded, in sRGB, to
//!   place in documents and draw.
//!
//! Every edit is a transaction, written to the document's file as it's
//! committed; there's no saving and no "Save changes?". A run of typing
//! undoes as one.
//!
//! app is an impure crate: it reads and writes documents.
//!
//! Invariants:
//!
//! - What's drawn is the document as it stands, laid out afresh only where
//!   it changed.
//! - The same events give the same document and the same frames, so scripted
//!   sessions can be played headless and compared.

mod blocks;
mod editor;
pub mod picture;
mod recent;
mod resources;
pub mod session;
mod view;
mod window;

pub use editor::{
    Choice, Edge, Editor, EditorError, Effect, Event, PointerShape, Position, View, WindowAction,
};
pub use recent::Recent;
pub use resources::{ResourceError, Resources};
pub use window::Window;
