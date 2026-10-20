//! The fonts M1 bundles, listed in `spec/text/fonts.ron` and kept in
//! `fonts/`, where `cargo run -p fetch-fonts` puts them.

use std::path::PathBuf;

use serde::Deserialize;

use crate::Error;

/// The font files `spec/text/fonts.ron` lists. The file says more, for
/// `text`; this is what fetching and loading need.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The files, in the order they're loaded.
    pub files: Vec<FontFile>,
}

/// A font file and where it comes from.
#[derive(Debug, Clone, Deserialize)]
pub struct FontFile {
    /// Its name in `fonts/`.
    pub file: String,
    /// Where it's downloaded from: a pinned commit, so it never changes.
    pub url: String,
    /// The SHA-256 of its bytes, in lowercase hexadecimal.
    pub sha256: String,
}

/// The path of `spec/text/fonts.ron`.
pub fn spec_path() -> PathBuf {
    crate::workspace_root().join("spec/text/fonts.ron")
}

/// The directory the fonts are kept in.
pub fn dir() -> PathBuf {
    crate::workspace_root().join("fonts")
}

/// `spec/text/fonts.ron`, as text.
pub fn spec() -> Result<String, Error> {
    let path = spec_path();
    std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })
}

/// The files `spec/text/fonts.ron` lists.
pub fn manifest() -> Result<Manifest, Error> {
    ron::from_str(&spec()?).map_err(|e| Error::Record {
        path: spec_path(),
        source: Box::new(e),
    })
}

/// Every listed font's name and bytes, in the manifest's order. A missing
/// font is an error that says how to fetch it.
pub fn load() -> Result<Vec<(String, Vec<u8>)>, Error> {
    manifest()?
        .files
        .into_iter()
        .map(|f| {
            let path = dir().join(&f.file);
            match std::fs::read(&path) {
                Ok(bytes) => Ok((f.file, bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Err(Error::MissingFont { path })
                }
                Err(source) => Err(Error::Io { path, source }),
            }
        })
        .collect()
}
