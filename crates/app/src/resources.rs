//! What every window shares: fonts, styles, commands, the chrome's look, and
//! where documents live.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use canvas::{PageStyle, PageStyleSpec};
use chrome::Commands;
use serde::Deserialize;
use story::StyleSheet;
use text::{FontSpec, Fonts};
use toolkit::Theme;

/// Why resources couldn't be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    /// A file couldn't be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// A spec or font isn't what it should be.
    #[error("{path}: {message}")]
    Spec {
        /// The file.
        path: PathBuf,
        /// What's wrong.
        message: String,
    },
}

use ResourceError as Error;

/// The fonts, styles and commands every window uses, loaded once. Cloning
/// shares the fonts.
#[derive(Clone, Debug)]
pub struct Resources {
    /// The bundled fonts, with their fallbacks.
    pub fonts: Arc<Fonts>,
    /// The paragraph styles.
    pub sheet: StyleSheet,
    /// The page style of automatic pages.
    pub page: PageStyle,
    /// The commands and menus.
    pub commands: Commands,
    /// The chrome's look.
    pub theme: Theme,
    /// Where new documents are made and Open looks.
    pub documents: PathBuf,
    /// Whether shortcuts read as on a Mac.
    pub mac: bool,
}

#[derive(Deserialize)]
struct FontFiles {
    files: Vec<FontFile>,
}

#[derive(Deserialize)]
struct FontFile {
    file: String,
}

#[derive(Deserialize)]
struct PageSheet {
    page: PageStyleSpec,
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn spec<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Error> {
    ron::from_str(&read(path)?).map_err(|e| Error::Spec {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

impl Resources {
    /// Loads the specs from `root/spec` and the bundled fonts from
    /// `root/fonts`, for documents in `documents`.
    pub fn load(root: &Path, documents: PathBuf, mac: bool) -> Result<Resources, Error> {
        let fonts_spec = root.join("spec/text/fonts.ron");
        let files: FontFiles = spec(&fonts_spec)?;
        let mut fonts = Fonts::new();
        for f in files.files {
            let path = root.join("fonts").join(&f.file);
            let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            fonts.add(bytes).map_err(|e| Error::Spec {
                path,
                message: e.to_string(),
            })?;
        }
        let fallbacks: FontSpec = spec(&fonts_spec)?;
        fonts.set_fallbacks(&fallbacks);
        let styles = root.join("spec/styles/m1.ron");
        let sheet: StyleSheet = spec(&styles)?;
        let page: PageSheet = spec(&styles)?;
        let commands_path = root.join("spec/commands/m1.ron");
        let commands_spec: chrome::Spec = spec(&commands_path)?;
        let commands = Commands::from_spec(&commands_spec).map_err(|e| Error::Spec {
            path: commands_path,
            message: e.to_string(),
        })?;
        Ok(Resources {
            fonts: Arc::new(fonts),
            sheet,
            page: PageStyle::from_spec(&page.page),
            commands,
            theme: Theme::default(),
            documents,
            mac,
        })
    }
}
