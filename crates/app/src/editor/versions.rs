//! Named versions (docs/ARCHITECTURE.md, section 6): Save a Version names
//! the document as it is; Versions lists those named, newest first, and
//! one chosen opens in a window of its own, read-only, as it was. Copy Out
//! makes it a document of its own beside the original, with its pictures,
//! and opens it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use doc::History;
use store::Store;

use super::{Editor, EditorError, Effect, Panel, Pick};
use crate::Resources;

impl Editor {
    /// The named version of the document at `path` that follows its log
    /// entry `seq`, read-only, as it was: opened only to read, so whoever
    /// has the document open goes on writing to it.
    pub fn open_version(res: Arc<Resources>, path: &Path, seq: i64) -> Result<Editor, EditorError> {
        if super::is_markdown(path) {
            return Editor::open_markdown_version(res, path, seq);
        }
        let opened = Store::open_to_read(path)?;
        let version = opened
            .store
            .versions()?
            .into_iter()
            .rev()
            .find(|v| v.seq == seq)
            .ok_or_else(|| {
                store::Error::Damaged(format!("there's no version named after entry {seq}"))
            })?;
        let document = opened.store.document_at(seq)?;
        let mut editor = Editor::with(
            res,
            path.to_path_buf(),
            Some(opened.store),
            document,
            History::new(),
        );
        editor.load_pictures(story::pictures(&editor.document));
        editor.message = format!("{}, read-only: Copy Out to change it", version.name);
        editor.version = Some(version);
        Ok(editor)
    }

    /// The named version this window shows, if it shows one.
    pub fn version(&self) -> Option<&store::Version> {
        self.version.as_ref()
    }

    /// Opens the panel of named versions, newest first; one chosen opens
    /// in a window of its own. With none named, says how to name one.
    pub(super) fn versions_panel(&mut self) {
        let versions = if self.is_markdown() {
            self.markdown_versions()
        } else {
            let Some(store) = &self.store else {
                return;
            };
            match store.versions() {
                Ok(versions) => versions,
                Err(e) => {
                    self.message = format!("Couldn't read the versions: {e}");
                    return;
                }
            }
        };
        if versions.is_empty() {
            self.message = "No versions yet: Save a Version names one".to_string();
            return;
        }
        let rows = versions
            .into_iter()
            .rev()
            .map(|v| {
                (
                    v.name,
                    Pick::Effect(Effect::OpenVersion(self.path.clone(), v.seq)),
                )
            })
            .collect();
        self.panel = Some(Panel {
            title: "Open a version".to_string(),
            rows,
            selected: 0,
        });
    }

    /// Makes the version this window shows a document of its own, beside
    /// the original and named after both, with the pictures it shows, and
    /// asks the shell to open it.
    pub(super) fn copy_out(&mut self) -> Vec<Effect> {
        let Some(version) = &self.version else {
            return Vec::new();
        };
        let path = copy_path(&self.path, &version.name, &self.extension());
        if self.is_markdown() {
            return match self.copy_out_markdown(&path) {
                Ok(()) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.message = format!("Copied out as {name}");
                    vec![Effect::Open(Some(path))]
                }
                Err(e) => {
                    self.message = format!("Couldn't copy it out: {e}");
                    Vec::new()
                }
            };
        }
        let pictures = story::pictures(&self.document);
        let Some(store) = &self.store else {
            return Vec::new();
        };
        match store.copy_out(&path, &self.document, &pictures) {
            Ok(()) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.message = format!("Copied out as {name}");
                vec![Effect::Open(Some(path))]
            }
            Err(e) => {
                self.message = format!("Couldn't copy it out: {e}");
                Vec::new()
            }
        }
    }
}

/// Where version `version` of the document at `path` is copied out to:
/// beside it, "Report (Version 2).xw", or "Report (Version 2) 2.xw" and so
/// on if that's taken, with `extension`.
fn copy_path(path: &Path, version: &str, extension: &str) -> PathBuf {
    let stem = path.file_stem().map_or_else(
        || "Untitled".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    // A version's name may hold what a file's name can't.
    let version: String = version
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem} ({version}).{extension}")
        } else {
            format!("{stem} ({version}) {n}.{extension}")
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}
