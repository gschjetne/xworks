//! What every window shares: fonts, hyphenation patterns, styles,
//! commands, the chrome's look, where documents live, and the language
//! they're made in.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use canvas::{PageStyle, PageStyleSpec};
use chrome::Commands;
use serde::Deserialize;
use story::StyleSheet;
use text::{FontSpec, Fonts, Hyphenation, Hyphenator, Language};
use toolkit::{Theme, ThemeSpec};

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
    /// The bundled fonts, with their fallbacks, but for those loaded on
    /// demand: see [`Resources::fonts`].
    fonts: Arc<Fonts>,
    on_demand: Arc<OnDemand>,
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
    /// How large a picture may be.
    pub pictures: crate::picture::Limits,
    /// The language new documents are in: the system's, as the shell
    /// reads it (project owner, 2026-10-22, review pysc65hp); none until
    /// it's set.
    pub language: Language,
    hyphenation: Arc<Patterns>,
}

/// The hyphenation patterns, read from `hyphenation/` the first time text
/// in their language is hyphenated, and shared by every clone of the
/// resources.
#[derive(Debug, Default)]
struct Patterns {
    dir: PathBuf,
    languages: Vec<PatternFiles>,
    /// Those loaded so far, and the files tried.
    loaded: Mutex<(Arc<Hyphenation>, Vec<String>)>,
}

#[derive(Debug, Deserialize)]
struct PatternSpec {
    languages: Vec<PatternFiles>,
}

/// A language's files, as `spec/text/hyphenation.ron` lists them.
#[derive(Clone, Debug, Deserialize)]
struct PatternFiles {
    tags: Vec<String>,
    patterns: String,
    exceptions: Option<String>,
    left: usize,
    right: usize,
}

#[derive(Deserialize)]
struct FontFiles {
    files: Vec<FontFile>,
}

#[derive(Deserialize)]
struct FontFile {
    file: String,
    #[serde(default)]
    on_demand: bool,
}

/// The fonts loaded only once text needs them, shared by every clone of
/// the resources.
#[derive(Debug, Default)]
struct OnDemand {
    files: Vec<PathBuf>,
    /// Every bundled font, once these are loaded.
    loaded: OnceLock<Arc<Fonts>>,
    /// Held while loading; whether loading was tried.
    tried: Mutex<bool>,
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
        let mut on_demand = OnDemand::default();
        for f in files.files {
            let path = root.join("fonts").join(&f.file);
            if f.on_demand {
                on_demand.files.push(path);
            } else {
                add_font(&mut fonts, path)?;
            }
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
        let theme: ThemeSpec = spec(&root.join("spec/themes/m1.ron"))?;
        let pictures = spec(&root.join("spec/pictures.ron"))?;
        let patterns: PatternSpec = spec(&root.join("spec/text/hyphenation.ron"))?;
        Ok(Resources {
            fonts: Arc::new(fonts),
            on_demand: Arc::new(on_demand),
            sheet,
            page: PageStyle::from_spec(&page.page),
            commands,
            theme: Theme::from_spec(&theme),
            documents,
            mac,
            pictures,
            language: Language::und(),
            hyphenation: Arc::new(Patterns {
                dir: root.join("hyphenation"),
                languages: patterns.languages,
                loaded: Mutex::default(),
            }),
        })
    }

    /// The hyphenation patterns loaded so far, by language.
    pub fn hyphenation(&self) -> Arc<Hyphenation> {
        let loaded = self
            .hyphenation
            .loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        loaded.0.clone()
    }

    /// Loads the patterns `languages` are hyphenated with, those not
    /// loaded or tried before, for every window. True if it loaded any.
    pub fn load_hyphenation(&self, languages: &[Language]) -> Result<bool, Error> {
        let patterns = &self.hyphenation;
        let mut loaded = patterns
            .loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut all = (*loaded.0).clone();
        let mut any = false;
        for language in languages {
            let Some(files) = patterns.languages.iter().find(|f| serves(f, language)) else {
                continue;
            };
            if loaded.1.contains(&files.patterns) {
                continue;
            }
            loaded.1.push(files.patterns.clone());
            let read = |file: &str| read(&patterns.dir.join(file));
            let exceptions = match &files.exceptions {
                Some(file) => read(file)?,
                None => String::new(),
            };
            let hyphenator = Arc::new(Hyphenator::parse(
                &read(&files.patterns)?,
                &exceptions,
                files.left,
                files.right,
            ));
            for tag in &files.tags {
                if let Ok(tag) = Language::parse(tag) {
                    all.insert(&tag, hyphenator.clone());
                }
            }
            any = true;
        }
        if any {
            loaded.0 = Arc::new(all);
        }
        Ok(any)
    }

    /// The bundled fonts, with their fallbacks: every one once
    /// [`Resources::load_on_demand`] has loaded those kept until text needs
    /// them, and the rest until then. Faces keep their IDs when the rest
    /// are added, so layouts made before still draw.
    pub fn fonts(&self) -> &Arc<Fonts> {
        self.on_demand.loaded.get().unwrap_or(&self.fonts)
    }

    /// Loads the fonts kept until text needs them, such as the CJK faces,
    /// for every window. True if this call loaded them; false if they were
    /// loaded or tried before, or there are none.
    pub fn load_on_demand(&self) -> Result<bool, Error> {
        let mut tried = self
            .on_demand
            .tried
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *tried || self.on_demand.files.is_empty() {
            return Ok(false);
        }
        *tried = true;
        let mut all = (*self.fonts).clone();
        for path in &self.on_demand.files {
            add_font(&mut all, path.clone())?;
        }
        // Only this call, holding `tried`, sets it.
        let _ = self.on_demand.loaded.set(Arc::new(all));
        Ok(true)
    }
}

/// Whether `files` hyphenate `language`: one of their tags is its own, its
/// language and region's, or its language's, as text's `Hyphenation`
/// looks them up.
fn serves(files: &PatternFiles, language: &Language) -> bool {
    let mut one = Hyphenation::new();
    for tag in &files.tags {
        if let Ok(tag) = Language::parse(tag) {
            one.insert(&tag, Arc::new(Hyphenator::default()));
        }
    }
    one.has(language)
}

/// Adds the faces in the font file at `path` to `fonts`.
fn add_font(fonts: &mut Fonts, path: PathBuf) -> Result<(), Error> {
    let bytes = std::fs::read(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    fonts.add(bytes).map_err(|e| Error::Spec {
        path,
        message: e.to_string(),
    })?;
    Ok(())
}
