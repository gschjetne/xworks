//! The app: documents, windows, commands and autosave, without platform code;
//! the shells drive it (docs/ARCHITECTURE.md, section 8). It follows the Elm
//! architecture: a window takes events and returns effects for its shell to
//! carry out, and its view is a display list.
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
//! - [`OwnSheets`]: the person's own stylesheets, kept in the system's
//!   configuration folder.
//! - [`picture`]: PNG, JPEG, GIF and WebP files decoded, in sRGB, to
//!   place in documents and draw.
//! - [`recorder`]: the last minutes of what a person did, kept as a
//!   session for bug reports.
//!
//! app is an impure crate: it reads and writes documents, settings and
//! sessions, and lays paragraphs out on rayon's worker pool. It opens no
//! window, draws nothing to a screen and touches no clipboard: the shells
//! do, as its effects ask.
//!
//! # What a caller may rely on
//!
//! - **No saving:** every edit is a transaction, written to the document's
//!   file as it's committed, and a run of typing undoes as one; a document
//!   reopened is as it was left, its undo history with it
//!   (`tests/sessions.rs`, `sessions_end_as_they_expect`, the `reopen`
//!   session, and `the_exit_document_reopens_and_exports_the_same`;
//!   `tests/placing.rs`, `a_placed_picture_survives_reopening_undo_and_redo`).
//! - **What's drawn is the document as it stands,** laid out afresh only
//!   where it changed: whatever a session does, its pages draw as its
//!   document laid out from scratch (`tests/sessions.rs`,
//!   `layout_kept_through_edits_is_layout_made_afresh`).
//! - **The same events give the same document and the same frames,** so
//!   scripted sessions play headless and compare, and a recorded session
//!   plays back to the document it recorded (`tests/sessions.rs`,
//!   `sessions_end_as_they_expect`; `tests/window.rs`,
//!   `the_same_input_gives_the_same_frames_and_tree`; `tests/recorder.rs`,
//!   `a_recorded_session_plays_back_to_the_same_document`;
//!   `tools/shot/tests/golden.rs`, the `editor` and `exit-pages` scenes).
//! - **The window is covered everywhere,** with nothing under the
//!   document's area and the chrome, light and dark, on pages and as a
//!   draft, the inspector and the ruler shown and hidden
//!   (`tests/window.rs`, `the_window_is_covered_everywhere`).
//! - **A Markdown file stays Markdown:** it opens as itself, in the draft
//!   view, and each edit makes it whole again, as the `markdown` crate
//!   writes it, for a worker of its own to write a moment later, through a
//!   rename, flushed to disk; closing writes it at once. An edit Markdown
//!   can't hold asks first, and makes the document an `.xw` file beside it,
//!   with its history and pictures, only if the person says so. A change
//!   another program makes is noticed before xWorks writes and when its
//!   window comes forward (`tests/markdown.rs`; the `markdown-*` sessions).
//! - **A document is open once:** the window that has it comes forward,
//!   and another program's copy opens read-only and says why
//!   (`tests/versions.rs`,
//!   `a_document_open_to_write_opens_read_only_again_and_says_why`;
//!   `tests/window.rs`, `a_document_renamed_keeps_its_lock`).
//! - **Pasting keeps the document whole:** any selection pasted anywhere
//!   loses and doubles no text, and Undo takes it back
//!   (`tests/pasting.rs`, `every_selection_pastes_anywhere_and_undoes`).
//! - **Outside input gives errors, never panics:** pictures are refused
//!   from their headers past `spec/pictures.ron`'s limit, damaged ones are
//!   errors, and colour is converted to sRGB from their profiles
//!   (`tests/pictures.rs`; the `picture_*` fuzz targets); a session file
//!   reads as a session or an error (the `session_file` fuzz target); a
//!   person's own stylesheet is checked as `story` checks any; and settings
//!   or a list of recent documents that don't read give the defaults
//!   (`tests/window.rs`,
//!   `damaged_settings_and_recent_documents_give_the_defaults`).
//! - **Screen readers** get the text as lines, words and the selection
//!   (`tests/window.rs`, `screen_readers_get_lines_words_and_the_selection`).
//! - **Typing an address makes a link,** where `spec/editing.ron` says,
//!   as GitHub reads one (`tests/markdown.rs`,
//!   `links_as_typed_in_xworks_documents_follow_the_switch`; the
//!   `markdown-links-*` sessions).
//!
//! # What it doesn't promise
//!
//! - That a damaged JPEG can't panic: zune-jpeg's AVX2 code can, and
//!   xWorks takes the release that fixes it (project owner, 2026-10-28,
//!   review 7fnfdhb5).
//! - Threads: a [`Window`] and its [`Editor`] are the shell's UI thread's;
//!   the app's own workers take snapshots.
//! - The budgets: `tools/budgets` measures them, not the tests.
//! - What other programs get on the clipboard: plain text in M2.

mod blink;
mod blocks;
mod editor;
pub mod picture;
mod recent;
pub mod recorder;
mod resources;
pub mod session;
mod settings;
mod stylesheets;
mod view;
mod window;

pub use blink::{Blink, BlinkRate, Blinking};
pub use editor::{
    Choice, Edge, Editor, EditorError, Effect, Event, PointerShape, Position, View, WindowAction,
};
pub use recent::Recent;
pub use resources::{ByKind, Editing, ResourceError, Resources};
pub use settings::{RULER_FROM, RulerFrom, Settings, UNITS, units_of_locale};
pub use stylesheets::{OwnSheet, OwnSheets};
pub use window::Window;
