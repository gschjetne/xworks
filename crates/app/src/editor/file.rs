//! The document's file: its name, which a new document takes from its
//! first heading or line until it's named by hand (project owner,
//! 2026-10-23, review a23vvez8), and moving it, which renames it too.
//!
//! [`Editor::relocate`] is the one place a document's file changes its
//! name or folder: it closes the database, so SQLite folds its
//! write-ahead log into the file and removes it, moves the file, and opens
//! it again. What keeps a document from being open twice (M2's stage 7)
//! moves with it there.

use std::path::{Path, PathBuf};

use store::Store;

use super::{Editor, EditorError};

/// The longest name a document takes from its text, in characters.
const LONGEST: usize = 60;

/// `text` as a file's name: on one line, without the characters file
/// systems refuse, its spaces collapsed, and cut at a word near
/// [`LONGEST`] characters. Empty if nothing's left.
pub(crate) fn file_name(text: &str) -> String {
    let line = text.split(['\u{2028}', '\n', '\r']).next().unwrap_or("");
    let clean: String = line
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = clean.split_whitespace().collect();
    let mut name = String::new();
    for word in words {
        let more = name.chars().count() + word.chars().count() + usize::from(!name.is_empty());
        if more > LONGEST {
            if name.is_empty() {
                name = word.chars().take(LONGEST).collect();
            }
            break;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }
    // Names that start with a dot are hidden, and Windows refuses those
    // that end in one.
    name.trim_matches('.').trim().to_string()
}

/// Whether `stem` is `name`, or `name` and a number after a space, as a
/// name is made unique in its folder.
fn is_numbered(stem: &str, name: &str) -> bool {
    stem == name
        || stem
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(' '))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// A path for a document called `name` in `folder` that nothing else has:
/// "NAME.xw", or "NAME 2.xw" and so on, with `extension`; `own` counts as
/// free.
pub(crate) fn free_path(folder: &Path, name: &str, own: &Path, extension: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let file = if n == 1 {
            format!("{name}.{extension}")
        } else {
            format!("{name} {n}.{extension}")
        };
        let path = folder.join(file);
        if path == own || !path.exists() {
            return path;
        }
        n += 1;
    }
}

impl Editor {
    /// The name the document's text gives it: its first heading's, or
    /// with none, its first line's; `None` while it's empty.
    pub fn derived_name(&self) -> Option<String> {
        let sheet = &self.sheet;
        let ids = self.ids();
        let heading = ids
            .iter()
            .map(|&id| self.paragraph(id))
            .find(|p| sheet.style_of(p).heading.is_some() && !file_name(&p.text).is_empty());
        let first = heading.or_else(|| {
            ids.iter()
                .map(|&id| self.paragraph(id))
                .find(|p| !file_name(&p.text).is_empty())
        })?;
        Some(file_name(&first.text))
    }

    /// The extension the document's file has: `xw`, or a Markdown file's
    /// own, `md` or `markdown`.
    pub(crate) fn extension(&self) -> String {
        if !self.is_markdown() {
            return "xw".to_string();
        }
        self.path
            .extension()
            .map_or_else(|| "md".to_string(), |e| e.to_string_lossy().into_owned())
    }

    /// The name of the document's file, without its folder or extension.
    pub fn stem(&self) -> String {
        self.path.file_stem().map_or_else(
            || "Untitled".to_string(),
            |s| s.to_string_lossy().into_owned(),
        )
    }

    /// Whether the file is named as a new document is: "Untitled", or
    /// after its first heading or line, with a number to make it unique.
    pub(super) fn auto_named(&self) -> bool {
        let stem = self.stem();
        is_numbered(&stem, "Untitled")
            || self
                .derived_name()
                .is_some_and(|name| is_numbered(&stem, &name))
    }

    /// The document's name as the title bar shows it: what its text
    /// gives it until it's named by hand, and its file's name after, with
    /// the version's if it shows one.
    pub fn name(&self) -> String {
        if !self.named
            && self.version.is_none()
            && let Some(name) = self.derived_name()
        {
            return name;
        }
        self.title()
    }

    /// Whether the person named the document.
    pub fn named_by_hand(&self) -> bool {
        self.named
    }

    /// Gives the file the name the document's text gives it, if it isn't
    /// named by hand and that's changed: when the caret leaves its first
    /// heading or line, and when it closes.
    pub fn settle_name(&mut self) {
        if self.named || !self.writable() {
            return;
        }
        let Some(name) = self.derived_name() else {
            return;
        };
        if is_numbered(&self.stem(), &name) {
            return;
        }
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let to = free_path(&folder, &name, &self.path, &self.extension());
        if let Err(e) = self.relocate(&to) {
            self.message = format!("Couldn't rename it: {e}");
        }
    }

    /// Names the document `name` by hand, in its folder.
    pub fn rename(&mut self, name: &str) -> Result<(), EditorError> {
        let name = file_name(name);
        if name.is_empty() {
            return Ok(());
        }
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let to = free_path(&folder, &name, &self.path, &self.extension());
        self.relocate(&to)?;
        self.named = true;
        Ok(())
    }

    /// Moves the document to `path`, a file the system's dialog chose,
    /// which names it by hand.
    pub fn move_to(&mut self, path: &Path) -> Result<(), EditorError> {
        let mut to = path.to_path_buf();
        let keeps = if self.is_markdown() {
            super::is_markdown(&to)
        } else {
            to.extension().is_some_and(|e| e == "xw")
        };
        if !keeps {
            let extension = self.extension();
            to.set_extension(extension);
        }
        self.relocate(&to)?;
        self.named = true;
        Ok(())
    }

    /// [`Editor::move_to`], saying in the status bar how it went.
    pub(super) fn move_to_chosen(&mut self, path: &Path) {
        self.message = match self.move_to(path) {
            Ok(()) => format!("Moved to {}", self.path.display()),
            Err(e) => format!("Couldn't move it: {e}"),
        };
    }

    /// Moves the document's file to `to`, where there must be none: the
    /// one place its file changes name or folder. The database is closed
    /// first, so its write-ahead log is folded into the file and nothing
    /// is left beside it, then the file moves, and it's opened again
    /// where it is. If the move fails, it's opened again where it was.
    pub fn relocate(&mut self, to: &Path) -> Result<(), EditorError> {
        if to == self.path {
            return Ok(());
        }
        if to.exists() {
            return Err(EditorError::Store(store::Error::Exists(to.to_path_buf())));
        }
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(|source| EditorError::Io {
                path: folder.to_path_buf(),
                source,
            })?;
        }
        if self.is_markdown() {
            // A Markdown file moves as it is, its lock and versions along.
            move_file(&self.path, to).map_err(|source| EditorError::Io {
                path: to.to_path_buf(),
                source,
            })?;
            if let Some(lock) = self.markdown_lock() {
                let _ = lock.moved(to);
            }
            let from = std::mem::replace(&mut self.path, to.to_path_buf());
            self.markdown_moved(&from);
            return Ok(());
        }
        // Closed: SQLite checkpoints and removes the log and shared memory
        // as the last connection closes.
        self.store = None;
        let moved = move_file(&self.path, to);
        let at = if moved.is_ok() {
            to.to_path_buf()
        } else {
            self.path.clone()
        };
        match Store::open(&at) {
            Ok(opened) => {
                self.store = Some(opened.store);
                self.path = at;
            }
            Err(e) => {
                self.message = format!("Read-only: the document's file can't be reached: {e}");
                return Err(e.into());
            }
        }
        moved.map_err(|source| EditorError::Io {
            path: to.to_path_buf(),
            source,
        })
    }
}

/// Moves the file `from` to `to`: renamed where they're on one file
/// system, and otherwise copied and the original removed.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if from.exists() && !to.exists() => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_makes_file_names() {
        assert_eq!(file_name("A walking skeleton"), "A walking skeleton");
        assert_eq!(file_name("  What: a/b?  test  "), "What a b test");
        assert_eq!(file_name("First line\u{2028}second"), "First line");
        assert_eq!(file_name("...hidden."), "hidden");
        assert_eq!(file_name(""), "");
        let long = "word ".repeat(30);
        let name = file_name(&long);
        assert!(name.chars().count() <= LONGEST && name.ends_with("word"));
        assert_eq!(file_name(&"x".repeat(100)).chars().count(), LONGEST);
    }

    #[test]
    fn numbered_names_are_recognized() {
        assert!(is_numbered("Untitled", "Untitled"));
        assert!(is_numbered("Untitled 12", "Untitled"));
        assert!(!is_numbered("Untitled draft", "Untitled"));
        assert!(!is_numbered("Untitled ", "Untitled"));
    }
}
