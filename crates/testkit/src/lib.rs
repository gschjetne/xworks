//! Test support for every crate: images, PNG output and golden records.
//!
//! - [`Image`]: straight-alpha RGBA pixels, 8 bits a channel, row by row,
//!   the form every render is compared in.
//! - [`golden`]: approved renders, recorded as SHA-256 hashes of their
//!   pixels under `spec/golden/` (spec/golden/README.md).
//! - [`fonts`]: the bundled fonts, as `spec/text/fonts.ron` lists them.
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
