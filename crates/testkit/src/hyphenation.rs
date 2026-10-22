//! The hyphenation patterns, listed in `spec/text/hyphenation.ron` and
//! kept in `hyphenation/`, beside the fonts, where `cargo run -p
//! fetch-fonts` puts them.

use std::path::PathBuf;

use serde::Deserialize;

use crate::Error;
use crate::fonts::FontFile;

/// What `spec/text/hyphenation.ron` lists: the files, and each language's.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The files, and where they come from.
    pub files: Vec<FontFile>,
    /// The languages, and their files.
    pub languages: Vec<LanguageFiles>,
}

/// A language's hyphenation files.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageFiles {
    /// Its name.
    pub name: String,
    /// The language tags it hyphenates.
    pub tags: Vec<String>,
    /// Its patterns' file.
    pub patterns: String,
    /// Its exceptions' file, if it has one.
    pub exceptions: Option<String>,
    /// Its licence.
    pub licence: String,
    /// The files whose headers hold its copyright and licence.
    pub notices: Vec<String>,
    /// The fewest letters a break leaves before it.
    pub left: usize,
    /// The fewest letters a break leaves after it.
    pub right: usize,
}

/// A language's hyphenation, read.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The language's entry.
    pub language: LanguageFiles,
    /// Its patterns.
    pub patterns: String,
    /// Its exceptions, empty if it has none.
    pub exceptions: String,
}

/// The path of `spec/text/hyphenation.ron`.
pub fn spec_path() -> PathBuf {
    crate::workspace_root().join("spec/text/hyphenation.ron")
}

/// The directory the files are kept in.
pub fn dir() -> PathBuf {
    crate::workspace_root().join("hyphenation")
}

/// What `spec/text/hyphenation.ron` lists.
pub fn manifest() -> Result<Manifest, Error> {
    let path = spec_path();
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    ron::from_str(&text).map_err(|e| Error::Record {
        path,
        source: Box::new(e),
    })
}

/// Every language's patterns and exceptions, read, in the manifest's
/// order. A missing file is an error that says how to fetch it.
pub fn load() -> Result<Vec<Loaded>, Error> {
    let read = |file: &str| {
        let path = dir().join(file);
        std::fs::read_to_string(&path).map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => Error::MissingHyphenation { path },
            _ => Error::Io { path, source },
        })
    };
    manifest()?
        .languages
        .into_iter()
        .map(|language| {
            Ok(Loaded {
                patterns: read(&language.patterns)?,
                exceptions: match &language.exceptions {
                    Some(file) => read(file)?,
                    None => String::new(),
                },
                language,
            })
        })
        .collect()
}
