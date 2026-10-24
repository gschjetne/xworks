//! People's own stylesheets, kept as files in a folder of the system's
//! configuration folder, beside the settings (docs/ARCHITECTURE.md,
//! section 6): `stylesheets/` there, the shell naming it. A document's
//! stylesheet can be kept there as one's own, and new documents can copy
//! one from there as they copy those that come with xWorks.
//!
//! A file there is outside input: it's read with
//! [`story::SheetSpec::parse`], which checks every limit, and one that
//! doesn't read is left out and said so, never trusted.

use std::path::{Path, PathBuf};

use story::SheetSpec;

/// Where people's own stylesheets are kept, if anywhere.
#[derive(Debug, Default)]
pub struct OwnSheets {
    folder: Option<PathBuf>,
}

/// A stylesheet of a person's own, as its file gave it.
#[derive(Clone, Debug)]
pub struct OwnSheet {
    /// Its file.
    pub path: PathBuf,
    /// The stylesheet.
    pub spec: SheetSpec,
}

impl OwnSheets {
    /// Those kept in `folder`, read each time they're asked for, as the
    /// person may add or change them between.
    pub fn load(folder: PathBuf) -> OwnSheets {
        OwnSheets {
            folder: Some(folder),
        }
    }

    /// The folder, if they're kept anywhere.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Every stylesheet in the folder that reads, by name, and the files
    /// that don't, each with why.
    pub fn list(&self) -> (Vec<OwnSheet>, Vec<(PathBuf, String)>) {
        let (mut sheets, mut bad) = (Vec::new(), Vec::new());
        let Some(folder) = &self.folder else {
            return (sheets, bad);
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return (sheets, bad);
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .collect();
        paths.sort();
        for path in paths {
            let read = std::fs::metadata(&path)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    if m.len() > story::sheet::MOST_BYTES as u64 {
                        Err("it's larger than a megabyte".to_string())
                    } else {
                        std::fs::read_to_string(&path).map_err(|e| e.to_string())
                    }
                })
                .and_then(|text| SheetSpec::parse(&text).map_err(|e| e.to_string()));
            match read {
                Ok(spec) => sheets.push(OwnSheet { path, spec }),
                Err(why) => bad.push((path, why)),
            }
        }
        sheets.sort_by(|a, b| a.spec.name.cmp(&b.spec.name));
        (sheets, bad)
    }

    /// The person's stylesheet named `name`, if there is one that reads.
    pub fn get(&self, name: &str) -> Option<OwnSheet> {
        self.list().0.into_iter().find(|s| s.spec.name == name)
    }

    /// Keeps `spec` as one of the person's own, in a file named after it,
    /// replacing one of that name: a newer copy, one version on, so
    /// documents that copied the older can be offered the change. Returns
    /// its file.
    pub fn save(&self, spec: &SheetSpec) -> Result<PathBuf, String> {
        let folder = self
            .folder
            .as_ref()
            .ok_or_else(|| "there's no folder to keep stylesheets in".to_string())?;
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let mut spec = spec.clone();
        if let Some(old) = self.get(&spec.name) {
            spec.version = old.spec.version.max(spec.version).saturating_add(1);
        }
        spec.check().map_err(|e| e.to_string())?;
        let path = folder.join(format!("{}.ron", file_name(&spec.name)));
        let partial = path.with_extension("ron-partial");
        std::fs::write(&partial, spec.write())
            .map_err(|e| format!("{}: {e}", partial.display()))?;
        std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

/// `name` as a file's name: the characters no system takes in one left
/// out.
fn file_name(name: &str) -> String {
    let out: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .collect();
    let out = out.trim().trim_matches('.').to_string();
    if out.is_empty() {
        "Stylesheet".to_string()
    } else {
        out
    }
}
