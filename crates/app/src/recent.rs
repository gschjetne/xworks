//! The documents opened lately, newest first, for File, Open Recent: kept
//! in a file the shell names, and shared by every window.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// How many documents are kept.
pub(crate) const KEPT: usize = 10;

/// The documents opened lately.
#[derive(Debug, Default)]
pub struct Recent {
    /// Where the list is kept, if anywhere.
    file: Option<PathBuf>,
    paths: Mutex<Vec<PathBuf>>,
}

impl Recent {
    /// The list kept in `file`: empty if there's none yet, or it can't be
    /// read.
    pub fn load(file: PathBuf) -> Recent {
        let paths = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| ron::from_str::<Vec<PathBuf>>(&text).ok())
            .unwrap_or_default();
        Recent {
            file: Some(file),
            paths: Mutex::new(paths.into_iter().take(KEPT).collect()),
        }
    }

    /// The documents, newest first, those that are gone left out.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.lock()
            .iter()
            .filter(|p| p.is_file())
            .cloned()
            .collect()
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

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
        self.paths.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the list and writes it to its file, if it has one; where
    /// it can't be written, it's kept for this run.
    fn change(&self, change: impl FnOnce(&mut Vec<PathBuf>)) {
        let mut paths = self.lock();
        change(&mut paths);
        if let Some(file) = &self.file
            && let Ok(text) = ron::ser::to_string_pretty(&*paths, ron::ser::PrettyConfig::default())
        {
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(file, text);
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
        // Kept in its file, for the next run.
        assert_eq!(Recent::load(file).paths()[0], to);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
