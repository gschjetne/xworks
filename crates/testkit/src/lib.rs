//! Test support for every crate: images, PNG output and golden records.
//!
//! - [`Image`]: straight-alpha RGBA pixels, 8 bits a channel, row by row,
//!   the form every render is compared in.
//! - [`golden`]: approved renders, recorded as SHA-256 hashes of their
//!   pixels under `spec/golden/` (spec/golden/README.md).
//! - [`fonts`]: the bundled fonts, as `spec/text/fonts.ron` lists them.
//! - [`specs`]: the RON files in a directory of `spec/`, for tests to read.
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
mod image;

use std::path::{Path, PathBuf};

pub use error::Error;
pub use image::Image;

/// The workspace's root directory, where `spec/` and `target/` are.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("testkit lives two levels below the workspace root")
        .to_path_buf()
}

/// The `.ron` files in `spec/DIR`, as names (without `.ron`) and text,
/// sorted by name.
pub fn specs(dir: &str) -> Result<Vec<(String, String)>, Error> {
    let dir = workspace_root().join("spec").join(dir);
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
        if path.extension().is_some_and(|e| e == "ron") {
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
