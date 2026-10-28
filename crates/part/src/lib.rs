//! Parts and the canvas contract (docs/ARCHITECTURE.md, section 4, "Parts",
//! and section 8): what every kind of document's view offers whatever
//! holds it, a window or, later, another part it's embedded in, so a
//! platform's port replaces the chrome and keeps the documents.
//!
//! A host gives a part a [`Surface`] and passes it [`Input`]: pointer,
//! keys, input-method composition, scrolling, and commands with their
//! arguments. The part returns frames (a display list, `scene`), an
//! accessibility tree ([`Access`], AccessKit's nodes), where its caret is
//! for the input method, what the pointer should look like over it, and
//! [`Request`]s for what only the host can do: the clipboard, files, other
//! windows. It describes its commands' state and the inspector's fields
//! and the ruler for its selection, and reads and writes its objects
//! through `doc`.
//!
//! Embedding is in-process and typed: a part holding another lays it out
//! within constraints (a width, or a page box) and hands it a surface, as a
//! window does. The text editor (`app::Editor`) is the first part; parts
//! edited in place within others come with the canvas track.
//!
//! # What a caller may rely on
//!
//! - **Pure.** No I/O, threads, clocks or environment (Clippy's
//!   `crates/clippy.toml`), and it builds for WebAssembly (CI). It's an
//!   interface: [`Part`] and the data that crosses it.
//! - **Positions are layout units** from the surface's top left: the host
//!   moves input into them and frames out of them, and what falls outside
//!   the surface isn't the part's (`app`'s `tests/window.rs`,
//!   `the_editor_gets_what_falls_in_its_area`).
//! - **A part describes and the host draws:** it never touches the host's
//!   chrome, and asks for what only the host can do as [`Request`]s rather
//!   than doing it.
//! - **The same input gives the same frames,** the same accessibility tree
//!   and the same requests (`app`'s `tests/window.rs`,
//!   `the_same_input_gives_the_same_frames_and_tree`), so a session played
//!   headless draws what it drew live (`tools/shot/tests/golden.rs`, the
//!   `editor` and `editor-lists` scenes).
//!
//! # What it doesn't promise
//!
//! - Embedding: no part holds another yet, and how one hands input to
//!   another it holds is still to be settled with the canvas track.
//! - That the trait is stable: it grows with the parts that implement it.
//! - Pixels: frames are display lists, which renderers draw.

use std::path::PathBuf;

use chrome::{Arg, CommandState, Inspector, Key, Modifiers, Ruler};
use scene::{Color, Item};

pub use accesskit;

/// The area a part is given, in layout units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
}

/// What a file is chosen for, through the system's own dialog
/// (docs/ARCHITECTURE.md, section 8): the host shows the dialog
/// [`Request::Choose`] asks for and sends back [`Input::Chosen`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// A document to open.
    Open,
    /// Where to move the document, which renames it too.
    MoveTo,
    /// A picture to place in the document.
    Picture,
}

/// Something that happened to a part.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A key that doesn't type text, with the modifiers held.
    Key {
        /// The key.
        key: Key,
        /// The modifiers.
        modifiers: Modifiers,
    },
    /// Text typed, or committed by an input method.
    Text(String),
    /// An input method's composition in progress; empty when it ends.
    Preedit(String),
    /// The pointer pressed at (`x`, `y`).
    Press {
        /// Across.
        x: i64,
        /// Down.
        y: i64,
        /// Whether Shift is held, to extend the selection.
        shift: bool,
        /// Which press of a run of quick presses in one place it is: 1 for
        /// a click, 2 for a double click, 3 for a triple.
        count: u32,
    },
    /// The pointer moved, with the button held or not.
    Move {
        /// Across.
        x: i64,
        /// Down.
        y: i64,
    },
    /// The pointer's button let go.
    Release,
    /// The wheel or trackpad scrolled, down positive.
    Scroll(i64),
    /// A command, from a menu, a shortcut, the palette or a script.
    Command(String),
    /// A command with its arguments, from the inspector, the ruler, a
    /// submenu or a script.
    CommandWith(String, Vec<Arg>),
    /// Text from the clipboard, after [`Request::Paste`].
    Pasted(String),
    /// The file a person chose, after [`Request::Choose`], or `None`.
    Chosen(Choice, Option<PathBuf>),
}

/// What a part asks of its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Put this text on the clipboard.
    Copy(String),
    /// Read the clipboard and send it back as [`Input::Pasted`].
    Paste,
    /// Ask the person for a file, in the system's own dialog.
    Choose(Choice),
    /// Open a window: a new document, or the one at this path.
    Open(Option<PathBuf>),
    /// Open a window on a named version of the document at this path, as
    /// it was after this entry of its log, read-only.
    OpenVersion(PathBuf, i64),
    /// Hand this file to the system to open, such as a PDF to print.
    OpenExternally(PathBuf),
    /// Show this file in the system's file manager, such as an export.
    Reveal(PathBuf),
    /// Quit the app.
    Quit,
}

/// What the pointer looks like over a part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pointer {
    /// The arrow.
    #[default]
    Arrow,
    /// The text beam, over text that can be edited.
    Text,
}

/// The colours a host draws its chrome in, which a part draws its
/// surroundings in: the desk behind pages, the selection, and text in a
/// view without pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Behind the pages.
    pub desk: Color,
    /// Pages.
    pub page: Color,
    /// Text on pages, and the caret there.
    pub page_ink: Color,
    /// Text on the desk, without pages, and the caret there.
    pub ink: Color,
    /// The selection.
    pub selection: Color,
    /// Text's own formatting, where it's shown.
    pub formatting: Color,
    /// Shadows on the desk.
    pub shadow: Color,
    /// Hairlines, such as quotes' bars on the desk.
    pub line: Color,
    /// Tints, such as code's on the desk.
    pub tint: Color,
    /// Links on the desk.
    pub accent: Color,
    /// Whether the caret shows: a host blinking it hides it now and then.
    pub caret: bool,
}

/// A part's accessibility tree: its nodes, the one at its root, and the
/// one that has the keyboard.
#[derive(Clone, Debug)]
pub struct Access {
    /// Its nodes.
    pub nodes: Vec<(accesskit::NodeId, accesskit::Node)>,
    /// Its root, which the host makes a child of its own.
    pub root: accesskit::NodeId,
    /// The node that has the keyboard.
    pub focus: accesskit::NodeId,
}

/// A view of a document's substrate object, placed in a window or in
/// another part (docs/ARCHITECTURE.md, section 4).
pub trait Part {
    /// Lays it out in `surface`.
    fn resize(&mut self, surface: Surface);

    /// Handles `input`, returning what it asks of the host.
    fn input(&mut self, input: Input) -> Vec<Request>;

    /// Its frame: what it draws in its surface, in `look`.
    fn frame(&self, look: &Look) -> Vec<Item>;

    /// Whether command `id` can run now, and whether it's on.
    fn state(&self, id: &str) -> CommandState;

    /// The inspector's fields for its selection.
    fn inspector(&self) -> Inspector;

    /// The ruler for its selection, if it has one.
    fn ruler(&self) -> Option<Ruler>;

    /// Where its caret is, as x, top and bottom, for the input method.
    fn caret(&self) -> Option<(i64, i64, i64)>;

    /// What the pointer looks like at (`x`, `y`).
    fn pointer(&self, x: i64, y: i64) -> Pointer;

    /// Its accessibility tree, its nodes numbered from `first`, with
    /// bounds in layout units of its surface.
    fn access(&self, first: u64) -> Access;

    /// What it says in the status bar.
    fn status(&self) -> String;
}
