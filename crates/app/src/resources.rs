//! What every window shares: fonts, hyphenation patterns, the stylesheets
//! that come with xWorks and people's own, commands, the chrome's look and
//! icons, where documents live, the documents opened lately, the person's
//! settings, and the language documents are made in.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use chrome::Commands;
use serde::Deserialize;
use story::{SheetSpec, StyleSheet};
use text::{FontSpec, Fonts, Hyphenation, Hyphenator, Language};
use toolkit::{Colors, Icons, Labels, Sizes};

use crate::recent::Recent;
use crate::settings::Settings;
use crate::stylesheets::OwnSheets;

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
    /// The stylesheets that come with xWorks, in the order File, New with
    /// Stylesheet lists them; new documents copy the first unless the
    /// person chooses another.
    pub stylesheets: Arc<Vec<SheetSpec>>,
    /// The first of them, resolved on its own: the program's stylesheet,
    /// which a document whose sheet lacks a value takes it from, and which
    /// Markdown files take, as they can't hold one.
    pub program: Arc<StyleSheet>,
    /// The person's own stylesheets, in the system's configuration
    /// folder, as the shell names it.
    pub own: Arc<OwnSheets>,
    /// The commands and menus.
    pub commands: Commands,
    /// The chrome's type and sizes (`spec/themes/chrome.ron`).
    pub sizes: Sizes,
    /// The light look's colours, and the dark look's.
    pub light: Colors,
    /// The dark look's colours.
    pub dark: Colors,
    /// Lucide's icons, for the chrome, built into the program.
    pub icons: Arc<Icons>,
    /// The chrome's labels laid out, shared by every window.
    pub labels: Arc<Labels>,
    /// Where new documents are made.
    pub documents: PathBuf,
    /// Where the program's own files are: `spec/`, the fonts and the
    /// hyphenation patterns.
    pub root: PathBuf,
    /// The documents opened lately, shared by every window.
    pub recent: Arc<Recent>,
    /// What the person chose that's kept between runs, such as the unit
    /// the ruler measures in, shared by every window.
    pub settings: Arc<Settings>,
    /// Whether shortcuts read as on a Mac.
    pub mac: bool,
    /// How large a picture may be.
    pub pictures: crate::picture::Limits,
    /// How much of what a person does the session recorder keeps.
    pub recorder: crate::recorder::Spec,
    /// The language new documents are in: the system's, as the shell
    /// reads it (project owner, 2026-10-22, review pysc65hp); none until
    /// it's set.
    pub language: Language,
    hyphenation: Arc<Patterns>,
    /// The named versions of the Markdown documents open, which last as
    /// long as each is (ARCHITECTURE.md section 6), by file, and what each
    /// file said: shared by every window, so a version opens in one of
    /// its own.
    pub(crate) markdown_versions: Arc<Mutex<MarkdownVersions>>,
    /// What Copy kept beside the text it put on the system's clipboard,
    /// shared by every window, so pasting from one document into another
    /// brings structure and styles along.
    pub(crate) clip: Arc<Mutex<Option<crate::editor::Fragment>>>,
}

/// Markdown documents' named versions: each file's, and what it said.
pub(crate) type MarkdownVersions =
    std::collections::HashMap<PathBuf, (markdown::File, Vec<(store::Version, doc::Document)>)>;

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
    /// The family the file's faces are of, for one loaded on demand once a
    /// document's stylesheet sets text in it; none for one loaded once
    /// text needs a character the others lack.
    #[serde(default)]
    family: Option<String>,
}

/// The stylesheets that come with xWorks, as `spec/styles.ron` lists them.
#[derive(Deserialize)]
struct SheetList {
    stylesheets: Vec<String>,
}

/// The most groups of fonts that can load on demand.
const GROUPS: usize = 16;

/// The fonts loaded only once text needs them, shared by every clone of
/// the resources: in groups, those for characters the others lack (the
/// CJK faces) and those of each family nothing uses until a document's
/// stylesheet sets text in it.
#[derive(Debug, Default)]
struct OnDemand {
    /// Each group: the family it's of, or none for characters, and its
    /// files.
    groups: Vec<(Option<String>, Vec<PathBuf>)>,
    /// Every font loaded so far, after each load: the latest is the one
    /// in use. Faces are only ever added, so each holds the one before.
    loaded: [OnceLock<Arc<Fonts>>; GROUPS],
    /// Held while loading: the groups tried.
    tried: Mutex<Vec<usize>>,
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
                let groups = &mut on_demand.groups;
                if let Some(k) = groups.iter().position(|(family, _)| *family == f.family) {
                    groups[k].1.push(path);
                } else if groups.len() < GROUPS {
                    groups.push((f.family.clone(), vec![path]));
                } else {
                    add_font(&mut fonts, path)?;
                }
            } else {
                add_font(&mut fonts, path)?;
            }
        }
        let fallbacks: FontSpec = spec(&fonts_spec)?;
        fonts.set_fallbacks(&fallbacks);
        let list: SheetList = spec(&root.join("spec/styles.ron"))?;
        let mut stylesheets = Vec::new();
        for name in &list.stylesheets {
            let path = root.join("spec/styles").join(format!("{name}.ron"));
            let sheet = SheetSpec::parse(&read(&path)?).map_err(|e| Error::Spec {
                path: path.clone(),
                message: e.to_string(),
            })?;
            stylesheets.push(sheet);
        }
        let Some(first) = stylesheets.first() else {
            return Err(Error::Spec {
                path: root.join("spec/styles.ron"),
                message: "it names no stylesheet".to_string(),
            });
        };
        let program = StyleSheet::from_spec(first);
        let commands_path = root.join("spec/commands/m1.ron");
        let commands_spec: chrome::Spec = spec(&commands_path)?;
        let commands = Commands::from_spec(&commands_spec).map_err(|e| Error::Spec {
            path: commands_path,
            message: e.to_string(),
        })?;
        let sizes: Sizes = spec(&root.join("spec/themes/chrome.ron"))?;
        let light: Colors = spec(&root.join("spec/themes/light.ron"))?;
        let dark: Colors = spec(&root.join("spec/themes/dark.ron"))?;
        let (icons, bad) = Icons::bundled();
        if let Some((name, why)) = bad.first() {
            return Err(Error::Spec {
                path: root.join(format!("icons/lucide/{name}.svg")),
                message: why.clone(),
            });
        }
        let pictures = spec(&root.join("spec/pictures.ron"))?;
        let recorder = spec(&root.join("spec/recorder.ron"))?;
        let patterns: PatternSpec = spec(&root.join("spec/text/hyphenation.ron"))?;
        Ok(Resources {
            fonts: Arc::new(fonts),
            on_demand: Arc::new(on_demand),
            program: Arc::new(program),
            stylesheets: Arc::new(stylesheets),
            own: Arc::new(OwnSheets::default()),
            commands,
            sizes,
            light,
            dark,
            icons: Arc::new(icons),
            labels: Arc::new(Labels::new()),
            documents,
            root: root.to_path_buf(),
            recent: Arc::new(Recent::default()),
            settings: Arc::new(Settings::default()),
            mac,
            pictures,
            recorder,
            language: Language::und(),
            hyphenation: Arc::new(Patterns {
                dir: root.join("hyphenation"),
                languages: patterns.languages,
                loaded: Mutex::default(),
            }),
            markdown_versions: Arc::default(),
            clip: Arc::default(),
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

    /// The bundled fonts, with their fallbacks: those loaded on demand
    /// among them once [`Resources::load_on_demand`] or
    /// [`Resources::load_families`] has loaded them. Faces keep their IDs
    /// when the rest are added, so layouts made before still draw.
    pub fn fonts(&self) -> &Arc<Fonts> {
        self.on_demand
            .loaded
            .iter()
            .rev()
            .find_map(OnceLock::get)
            .unwrap_or(&self.fonts)
    }

    /// Loads the fonts kept until text needs a character the others lack,
    /// such as the CJK faces, for every window. True if this call loaded
    /// them; false if they were loaded or tried before, or there are none.
    pub fn load_on_demand(&self) -> Result<bool, Error> {
        self.load_groups(|family| family.is_none())
    }

    /// Loads the fonts of `families` kept until a document's stylesheet
    /// sets text in them, for every window. True if this call loaded any.
    pub fn load_families(&self, families: &[String]) -> Result<bool, Error> {
        self.load_groups(|family| family.as_ref().is_some_and(|f| families.contains(f)))
    }

    /// Loads the groups of fonts kept until they're needed that `wanted`
    /// takes and that weren't tried before.
    fn load_groups(&self, wanted: impl Fn(&Option<String>) -> bool) -> Result<bool, Error> {
        let mut tried = self
            .on_demand
            .tried
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let groups: Vec<usize> = (0..self.on_demand.groups.len())
            .filter(|k| !tried.contains(k) && wanted(&self.on_demand.groups[*k].0))
            .collect();
        if groups.is_empty() {
            return Ok(false);
        }
        tried.extend(&groups);
        let mut all = (**self.fonts()).clone();
        for &k in &groups {
            for path in &self.on_demand.groups[k].1 {
                add_font(&mut all, path.clone())?;
            }
        }
        // Only this call, holding `tried`, sets the next.
        if let Some(slot) = self.on_demand.loaded.iter().find(|s| s.get().is_none()) {
            let _ = slot.set(Arc::new(all));
        }
        Ok(true)
    }
}

impl Resources {
    /// The chrome's look, dark or light.
    pub fn theme(&self, dark: bool) -> toolkit::Theme {
        let colors = if dark { self.dark } else { self.light };
        toolkit::Theme::new(self.sizes.clone(), colors)
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
