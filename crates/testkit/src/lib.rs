//! Test support for every crate: images, PNG output and golden records.
//!
//! - [`Image`]: straight-alpha RGBA pixels, 8 bits a channel, row by row,
//!   the form every render is compared in.
//! - [`golden`]: approved renders, recorded as SHA-256 hashes of their
//!   pixels under `spec/golden/` (spec/golden/README.md).
//! - [`fonts`]: the bundled fonts, as `spec/text/fonts.ron` lists them.
//! - [`pdf`]: a PDF's text read back through its fonts' ToUnicode maps
//!   and its `/ActualText`, a second reader beside pdf-extract.
//! - [`hyphenation`]: the hyphenation patterns, as
//!   `spec/text/hyphenation.ron` lists them.
//! - [`specs`]: the RON files in a directory of `spec/`, for tests to read.
//! - [`scratch_dir`]: a fresh, empty directory under `target/` for a test's
//!   files.
//!
//! testkit is an impure crate: it reads and writes files.
//!
//! Invariants:
//!
//! - An image's pixel buffer always holds exactly width × height × 4
//!   bytes.
//! - The same pixels always give the same hash, on every platform.

mod error;
pub mod fonts;
pub mod golden;
pub mod hyphenation;
mod image;
pub mod pdf;

use std::path::{Path, PathBuf};

pub use error::Error;
pub use image::Image;

/// The workspace's root directory, where `spec/` and `target/` are:
/// `XWORKS_ROOT` if it's set, as for a Windows build run from WSL, which
/// sees the workspace by another path than the one it was built in.
pub fn workspace_root() -> PathBuf {
    if let Some(root) = std::env::var_os("XWORKS_ROOT") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("testkit lives two levels below the workspace root")
        .to_path_buf()
}

/// The `.ron` files in `spec/DIR`, as names (without `.ron`) and text,
/// sorted by name.
pub fn specs(dir: &str) -> Result<Vec<(String, String)>, Error> {
    files(&workspace_root().join("spec").join(dir), "ron")
}

/// Lucide's icons in `icons/lucide/`, as names (without `.svg`) and SVG,
/// sorted by name.
pub fn icons() -> Result<Vec<(String, String)>, Error> {
    files(&workspace_root().join("icons/lucide"), "svg")
}

/// The files in `dir` with the extension `extension`, as names (without
/// it) and text, sorted by name.
fn files(dir: &Path, extension: &str) -> Result<Vec<(String, String)>, Error> {
    let dir = dir.to_path_buf();
    let entries = std::fs::read_dir(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let mut specs = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?
            .path();
        if path.extension().is_some_and(|e| e == extension) {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text =
                std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
            specs.push((name, text));
        }
    }
    specs.sort();
    Ok(specs)
}

/// A fresh, empty directory for a test's files: `target/scratch/NAME`,
/// emptied if it was there before.
pub fn scratch_dir(name: &str) -> Result<PathBuf, Error> {
    let dir = workspace_root().join("target/scratch").join(name);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(Error::Io { path: dir, source }),
    }
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    Ok(dir)
}
