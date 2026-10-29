//! What a person chooses that's kept between runs: the unit the ruler and
//! the inspector measure in (project owner, 2026-10-24, review ydqhfds0),
//! and where the ruler counts from (review 77nh3z47). Kept in a file the
//! shell names, beside the documents opened lately, and shared by every
//! window.
//!
//! The file is read the first time a setting is asked for, as the first
//! frame draws the ruler, and written as a setting changes. Where it can't
//! be read, the defaults hold, and where it can't be written, the choice
//! holds for this run, and that's logged.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrome::Unit;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// The units a person can choose for the ruler and the inspector, in the
/// order View, Units lists them, each with its command.
pub const UNITS: [(Unit, &str); 3] = [
    (Unit::Centimetre, "units-centimetres"),
    (Unit::Inch, "units-inches"),
    (Unit::Pica, "units-picas"),
];

/// Where the ruler is numbered from, the person's choice beside the units
/// in View, Units (project owner, 2026-10-24, review 77nh3z47).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum RulerFrom {
    /// The text column's start, the left margin, as word processors
    /// number it.
    #[default]
    Margin,
    /// The paper's left edge, as page layout programs number it.
    Edge,
}

/// Where the ruler can be numbered from, in the order View, Units lists
/// them, each with its command.
pub const RULER_FROM: [(RulerFrom, &str); 2] = [
    (RulerFrom::Margin, "ruler-from-margin"),
    (RulerFrom::Edge, "ruler-from-edge"),
];

/// The settings, shared by every window.
#[derive(Debug)]
pub struct Settings {
    /// Where they're kept, if anywhere.
    file: Option<PathBuf>,
    /// The unit until the person chooses one: the system's, as the shell
    /// reads its measurement system.
    default_units: Unit,
    kept: Mutex<Kept>,
}

/// What the file holds, and whether it's been read.
#[derive(Debug, Default, Deserialize, Serialize)]
struct Kept {
    /// The unit chosen, if one was.
    #[serde(default)]
    units: Option<Unit>,
    /// Where the ruler is numbered from.
    #[serde(default)]
    ruler_from: RulerFrom,
    /// How many times a setting has changed in this run.
    #[serde(skip)]
    changes: u64,
    #[serde(skip)]
    read: bool,
}

impl Default for Settings {
    /// Centimetres, kept nowhere.
    fn default() -> Settings {
        Settings {
            file: None,
            default_units: Unit::Centimetre,
            kept: Mutex::default(),
        }
    }
}

impl Settings {
    /// The settings kept in `file`, read when first asked for, with
    /// `units` until the person chooses others.
    pub fn load(file: PathBuf, units: Unit) -> Settings {
        Settings {
            file: Some(file),
            default_units: units,
            kept: Mutex::default(),
        }
    }

    /// What's kept, its file read into it if it hasn't been.
    fn lock(&self) -> MutexGuard<'_, Kept> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if !kept.read {
            let read: Kept = self
                .file
                .as_ref()
                .and_then(|file| std::fs::read_to_string(file).ok())
                .and_then(|text| ron::from_str(&text).ok())
                .unwrap_or_default();
            kept.units = read.units.filter(|u| UNITS.iter().any(|(v, _)| v == u));
            kept.ruler_from = read.ruler_from;
            kept.read = true;
        }
        kept
    }

    /// The unit the ruler and the inspector measure in.
    pub fn units(&self) -> Unit {
        self.lock().units.unwrap_or(self.default_units)
    }

    /// Chooses `unit`, and keeps it in the file. Only the units of
    /// [`UNITS`] can be chosen.
    pub fn set_units(&self, unit: Unit) {
        if !UNITS.iter().any(|(u, _)| *u == unit) {
            return;
        }
        let mut kept = self.lock();
        if kept.units == Some(unit) {
            return;
        }
        kept.units = Some(unit);
        self.keep(&mut kept);
    }

    /// Where the ruler is numbered from.
    pub fn ruler_from(&self) -> RulerFrom {
        self.lock().ruler_from
    }

    /// Numbers the ruler from `from`, and keeps it in the file.
    pub fn set_ruler_from(&self, from: RulerFrom) {
        let mut kept = self.lock();
        if kept.ruler_from == from {
            return;
        }
        kept.ruler_from = from;
        self.keep(&mut kept);
    }

    /// Counts a change to `kept`, and writes it to the file.
    fn keep(&self, kept: &mut Kept) {
        kept.changes += 1;
        if let Some(file) = &self.file
            && let Ok(text) = ron::ser::to_string_pretty(&*kept, ron::ser::PrettyConfig::default())
        {
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            if let Err(e) = std::fs::write(file, text) {
                warn!("writing {}: {e}", file.display());
            }
        }
    }

    /// How many times a setting has changed in this run, so a window
    /// that didn't change it knows to draw again.
    pub fn changes(&self) -> u64 {
        self.lock().changes
    }
}

/// The unit command `id` chooses, if it's one of View, Units.
pub(crate) fn unit_of(id: &str) -> Option<Unit> {
    UNITS.iter().find(|(_, c)| *c == id).map(|(u, _)| *u)
}

/// Where command `id` numbers the ruler from, if it's one of View, Units.
pub(crate) fn ruler_from_of(id: &str) -> Option<RulerFrom> {
    RULER_FROM.iter().find(|(_, c)| *c == id).map(|(f, _)| *f)
}

/// The unit a person measures in until they choose one, by the region of
/// `locale`, a POSIX locale ("en_US.UTF-8") or a BCP 47 tag ("en-US"):
/// inches where measurements are US customary, in the United States,
/// Liberia and Myanmar, and centimetres elsewhere, or with no region.
pub fn units_of_locale(locale: &str) -> Unit {
    let name = locale.split(['.', '@']).next().unwrap_or("");
    let region = name.split(['_', '-']).skip(1).find(|part| {
        (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
    });
    match region.map(str::to_ascii_uppercase).as_deref() {
        Some("US" | "LR" | "MM") => Unit::Inch,
        _ => Unit::Centimetre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_is_kept_between_runs() {
        let dir = std::env::temp_dir().join(format!("xworks-settings-{}", std::process::id()));
        let file = dir.join("settings.ron");
        let _ = std::fs::remove_dir_all(&dir);
        // Until one's chosen, the system's.
        let settings = Settings::load(file.clone(), Unit::Inch);
        assert_eq!(settings.units(), Unit::Inch);
        settings.set_units(Unit::Pica);
        assert_eq!(settings.units(), Unit::Pica);
        assert_eq!(settings.changes(), 1);
        // Only the ruler's units can be chosen.
        settings.set_units(Unit::Millimetre);
        assert_eq!(settings.units(), Unit::Pica);
        // The next run reads it.
        let again = Settings::load(file.clone(), Unit::Inch);
        assert_eq!(again.units(), Unit::Pica);
        assert_eq!(again.changes(), 0);
        // A file that can't be read leaves the defaults.
        std::fs::write(&file, "not RON").unwrap();
        assert_eq!(
            Settings::load(file.clone(), Unit::Centimetre).units(),
            Unit::Centimetre
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(unit_of("units-inches"), Some(Unit::Inch));
        assert_eq!(unit_of("toggle-ruler"), None);
    }

    #[test]
    fn where_the_ruler_counts_from_is_kept_between_runs() {
        let dir = std::env::temp_dir().join(format!("xworks-ruler-from-{}", std::process::id()));
        let file = dir.join("settings.ron");
        let _ = std::fs::remove_dir_all(&dir);
        // Until it's chosen, the margin, as word processors have it.
        let settings = Settings::load(file.clone(), Unit::Inch);
        assert_eq!(settings.ruler_from(), RulerFrom::Margin);
        settings.set_ruler_from(RulerFrom::Edge);
        assert_eq!(settings.ruler_from(), RulerFrom::Edge);
        settings.set_units(Unit::Pica);
        assert_eq!(settings.changes(), 2);
        // The next run reads both.
        let again = Settings::load(file.clone(), Unit::Inch);
        assert_eq!(again.ruler_from(), RulerFrom::Edge);
        assert_eq!(again.units(), Unit::Pica);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(ruler_from_of("ruler-from-edge"), Some(RulerFrom::Edge));
        assert_eq!(ruler_from_of("units-inches"), None);
    }

    #[test]
    fn the_system_measures_by_its_region() {
        for (locale, unit) in [
            ("en_US.UTF-8", Unit::Inch),
            ("en-US", Unit::Inch),
            ("my_MM", Unit::Inch),
            ("en_GB.UTF-8", Unit::Centimetre),
            ("nb-NO", Unit::Centimetre),
            ("zh-Hans-CN", Unit::Centimetre),
            ("es-419", Unit::Centimetre),
            ("de_DE@euro", Unit::Centimetre),
            ("C", Unit::Centimetre),
            ("", Unit::Centimetre),
            ("en", Unit::Centimetre),
        ] {
            assert_eq!(units_of_locale(locale), unit, "{locale}");
        }
    }
}
