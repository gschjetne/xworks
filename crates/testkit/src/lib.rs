//! Test support for every crate: images, PNG output and golden records,
//! and the files tests read from the workspace (docs/ARCHITECTURE.md,
//! section 13). Only tests and tools use it.
//!
//! - [`Image`]: straight-alpha RGBA pixels, 8 bits a channel, row by row,
//!   the form every render is compared in.
//! - [`golden`]: approved renders, recorded as SHA-256 hashes of their
//!   pixels under `spec/golden/` (spec/golden/README.md).
//! - [`fonts`]: the bundled fonts, as `spec/text/fonts.ron` lists them.
//! - [`pdf`]: a PDF's text read back through its fonts' ToUnicode maps
//!   and its `/ActualText`, a second reader beside pdf-extract.
//! - [`commonmark`]: CommonMark's own examples, as
//!   `spec/markdown/commonmark.ron` lists them.
//! - [`hyphenation`]: the hyphenation patterns, as
//!   `spec/text/hyphenation.ron` lists them.
//! - [`specs`] and [`spec_bytes`]: the files in a directory of `spec/`, for
//!   tests to read, and [`icons`], Lucide's.
//! - [`scratch_dir`]: a fresh, empty directory under `target/` for a test's
//!   files.
//!
//! testkit is an impure crate: it reads and writes files.
//!
//! # What a caller may rely on
//!
//! - **An image's pixel buffer** always holds exactly width × height × 4
//!   bytes, and filling a rectangle stays within it (`image`'s unit tests
//!   `from_rgba_checks_the_length`, `fill_rect_is_clipped_to_the_image` and
//!   `fill_rect_outside_the_image_changes_nothing`); PNG written reads back
//!   as the same pixels (`png_round_trips`).
//! - **The same pixels give the same hash,** on every platform, and the
//!   size is in it, so the same pixels in another shape hash otherwise
//!   (`golden`'s unit tests `hash_covers_size_then_pixels` and
//!   `same_pixels_in_another_shape_hash_differently`); a record matches
//!   its image and no other, reads back as it was written, and is refused
//!   where its file's name isn't its scene's (`a_record_matches_its_image_and_nothing_else`,
//!   `records_round_trip_through_ron` and `load_all_rejects_a_misnamed_record`).
//! - **Files are found from the workspace's root,** or from `XWORKS_ROOT`
//!   where it's set, as for a Windows build run from WSL; what's listed is
//!   sorted by name, and what's missing is an [`Error`] saying where, and
//!   for a font or a pattern file how to fetch it (unit test
//!   `specs_are_sorted_by_name_and_a_missing_folder_says_where`).
//! - **A scratch directory** is empty when it's given, whatever an earlier
//!   run left in it (unit test `a_scratch_directory_starts_empty`).
//! - **PDF text** reads as typed in every script, where pdf-extract
//!   misreads Arabic (`render-pdf`'s `tests/pdf.rs`,
//!   `text_extracts_back_as_typed`).
//!
//! # What it doesn't promise
//!
//! - That fetched files are what their specs' hashes say: `fetch-fonts`
//!   checks those (`fetch-fonts --check`).
//! - Separate scratch directories for tests that run at once: each test
//!   names its own.
//! - A stable API: it serves the workspace's tests and tools, and changes
//!   with them.

pub mod commonmark;
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

/// The files in `spec/DIR` with the extension `extension`, as names (with
/// it) and their bytes as they are, sorted by name: for files whose bytes
/// matter, such as Markdown kept byte for byte.
pub fn spec_bytes(dir: &str, extension: &str) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let dir = workspace_root().join("spec").join(dir);
    let entries = std::fs::read_dir(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?
            .path();
        if path.extension().is_some_and(|e| e == extension) {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = std::fs::read(&path).map_err(|source| Error::Io { path, source })?;
            out.push((name, bytes));
        }
    }
    out.sort();
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_directory_starts_empty() {
        let dir = scratch_dir("testkit-scratch").unwrap();
        std::fs::write(dir.join("left.txt"), "left over").unwrap();
        std::fs::create_dir_all(dir.join("deeper/still")).unwrap();
        let again = scratch_dir("testkit-scratch").unwrap();
        assert_eq!(again, dir);
        assert_eq!(std::fs::read_dir(&again).unwrap().count(), 0);
    }

    #[test]
    fn specs_are_sorted_by_name_and_a_missing_folder_says_where() {
        let golden = specs("golden").unwrap();
        assert!(!golden.is_empty());
        assert!(golden.windows(2).all(|w| w[0].0 < w[1].0));
        match specs("no-such-folder") {
            Err(Error::Io { path, .. }) => assert!(path.ends_with("spec/no-such-folder")),
            other => panic!("{other:?}"),
        }
    }
}
