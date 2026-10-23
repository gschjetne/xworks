//! The documents opened lately, newest first, for File, Open Recent: kept
//! in a file the shell names, and shared by every window.
//!
//! Nothing touches the file while xWorks starts: it's read the first time
//! the list is asked for, as when File, Open Recent opens, and written
//! when xWorks ends or fails ([`Recent::flush`]). What's added before it's
//! read comes first, and what it held follows.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

/// How many documents are kept.
pub(crate) const KEPT: usize = 10;

/// The documents opened lately.
#[derive(Debug, Default)]
pub struct Recent {
    /// Where the list is kept, if anywhere.
    file: Option<PathBuf>,
    list: Mutex<List>,
}

#[derive(Debug, Default)]
struct List {
    paths: Vec<PathBuf>,
    /// Whether the file has been read into the list.
    read: bool,
    /// Whether the list has changed since the file was written.
    changed: bool,
}

impl Recent {
    /// The list kept in `file`, read when it's first asked for: empty if
    /// there's none yet or it can't be read.
    pub fn load(file: PathBuf) -> Recent {
        Recent {
            file: Some(file),
            list: Mutex::default(),
        }
    }

    /// The list, its file read into it if it hasn't been.
    fn lock(&self) -> MutexGuard<'_, List> {
        self.read(self.list.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// `list` with its file read into it if it hasn't been: documents that
    /// are gone are left out of what the file held.
    fn read<'a>(&self, mut list: MutexGuard<'a, List>) -> MutexGuard<'a, List> {
        if !list.read {
            list.read = true;
            let kept: Vec<PathBuf> = self
                .file
                .as_ref()
                .and_then(|file| std::fs::read_to_string(file).ok())
                .and_then(|text| ron::from_str::<Vec<PathBuf>>(&text).ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.is_file())
                .collect();
            for path in kept {
                if !list.paths.contains(&path) {
                    list.paths.push(path);
                }
            }
            list.paths.truncate(KEPT);
        }
        list
    }

    /// The documents, newest first.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.lock().paths.clone()
    }

    /// Puts `path` first, as it's opened.
    pub fn add(&self, path: &Path) {
        self.change(|paths| {
            paths.retain(|p| p != path);
            paths.insert(0, path.to_path_buf());
            paths.truncate(KEPT);
        });
    }

    /// Follows a document moved from `from` to `to`.
    pub fn moved(&self, from: &Path, to: &Path) {
        self.change(|paths| {
            for p in paths.iter_mut().filter(|p| *p == from) {
                *p = to.to_path_buf();
            }
        });
    }

    /// Changes the list, without reading its file, which comes later.
    fn change(&self, change: impl FnOnce(&mut Vec<PathBuf>)) {
        let mut list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut list.paths);
        list.changed = true;
    }

    /// Writes the list to its file, if it has one and it changed, what the
    /// file held kept after what's new: as xWorks ends, or fails. Where it
    /// can't be written, it's kept for this run. It never waits for the
    /// list: as xWorks fails, the code that failed may be holding it.
    pub fn flush(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let list = match self.list.try_lock() {
            Ok(list) => list,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        let mut list = self.read(list);
        if !list.changed {
            return;
        }
        if let Ok(text) = ron::ser::to_string_pretty(&list.paths, ron::ser::PrettyConfig::default())
        {
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            if std::fs::write(file, text).is_ok() {
                list.changed = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_comes_first_and_moves_are_followed() {
        let dir = std::env::temp_dir().join(format!("xworks-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("recent.ron");
        let _ = std::fs::remove_file(&file);
        let docs: Vec<PathBuf> = (0..12).map(|n| dir.join(format!("{n}.xw"))).collect();
        for d in &docs {
            std::fs::write(d, b"").unwrap();
        }
        let recent = Recent::load(file.clone());
        for d in &docs {
            recent.add(d);
        }
        recent.add(&docs[5]);
        let paths = recent.paths();
        assert_eq!(paths.len(), KEPT);
        assert_eq!(paths[0], docs[5]);
        assert_eq!(paths[1], docs[11]);
        let to = dir.join("moved.xw");
        std::fs::rename(&docs[5], &to).unwrap();
        recent.moved(&docs[5], &to);
        recent.flush();
        // Kept in its file, for the next run; what's added before it's
        // read comes first.
        let again = Recent::load(file);
        again.add(&docs[0]);
        let paths = again.paths();
        assert_eq!(paths[0], docs[0]);
        assert_eq!(paths[1], to);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
