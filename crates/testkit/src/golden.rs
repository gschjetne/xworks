//! Golden images: approved renders of xWorks' own output, recorded as
//! hashes of their pixels, not as images (spec/golden/README.md).
//!
//! A record's hash is the SHA-256 of the image's width and height as
//! big-endian 32-bit numbers, then every pixel's red, green, blue and alpha
//! bytes, row by row. Renders are exact, so the approved image is what the
//! renderer draws at the commit that last changed the record.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Image};

/// An approved render, as `spec/golden/NAME.ron` records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "Golden")]
pub struct Golden {
    /// The rendered scene's name, which is also the record's file name.
    pub name: String,
    /// The image's width and height, in pixels.
    pub size: (u32, u32),
    /// The image's hash, in lowercase hexadecimal.
    pub sha256: String,
    /// The person who approved it.
    pub approved_by: String,
}

impl Golden {
    /// A record of `image`, approved by `approved_by`.
    pub fn approve(name: &str, image: &Image, approved_by: &str) -> Golden {
        Golden {
            name: name.to_string(),
            size: (image.width(), image.height()),
            sha256: hash(image),
            approved_by: approved_by.to_string(),
        }
    }

    /// Whether `image` is the approved one.
    pub fn matches(&self, image: &Image) -> bool {
        self.size == (image.width(), image.height()) && self.sha256 == hash(image)
    }

    /// The record as the RON text of its file.
    pub fn to_ron(&self) -> String {
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        let body = ron::ser::to_string_pretty(self, config)
            .expect("a record of strings and numbers always serializes");
        format!(
            "// An approved golden image (spec/golden/README.md), written by `shot --approve`.\n{body}\n"
        )
    }

    /// A record from the RON text of the file at `path`.
    pub fn from_ron(text: &str, path: &Path) -> Result<Golden, Error> {
        ron::from_str(text).map_err(|source| Error::Record {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }
}

/// The SHA-256 of an image's size and pixels, in lowercase hexadecimal.
pub fn hash(image: &Image) -> String {
    let mut hasher = Sha256::new();
    hasher.update(image.width().to_be_bytes());
    hasher.update(image.height().to_be_bytes());
    hasher.update(image.pixels());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The directory of golden records: `spec/golden/`.
pub fn dir() -> PathBuf {
    crate::workspace_root().join("spec/golden")
}

/// Where a render that doesn't match its record is written, for a person
/// to look at: `target/golden-reports/`.
pub fn report_dir() -> PathBuf {
    crate::workspace_root().join("target/golden-reports")
}

/// The file that records the image `name` in `dir`.
pub fn record_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.ron"))
}

/// Every record in `dir`, sorted by name. A record whose name isn't its
/// file's name is an error.
pub fn load_all(dir: &Path) -> Result<Vec<Golden>, Error> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io { path, source }
    };
    let mut records = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io(dir))? {
        let path = entry.map_err(io(dir))?.path();
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        let record = Golden::from_ron(&text, &path)?;
        if path
            .file_stem()
            .is_none_or(|stem| stem != record.name.as_str())
        {
            return Err(Error::Misnamed {
                path,
                name: record.name,
            });
        }
        records.push(record);
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

/// Writes `record` into `dir`, replacing an earlier record of the same
/// image.
pub fn write(dir: &Path, record: &Golden) -> Result<(), Error> {
    let path = record_path(dir, &record.name);
    std::fs::write(&path, record.to_ron()).map_err(|source| Error::Io { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_covers_size_then_pixels() {
        // Computed independently: SHA-256 of 00000002 00000001 01..08.
        let image = Image::from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(
            hash(&image),
            "6df082ee9e3a3ee94a5b45e56195b613d35ef923f0eae841590515b750f34dcd"
        );
    }

    #[test]
    fn same_pixels_in_another_shape_hash_differently() {
        let wide = Image::from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let tall = Image::from_rgba(1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_ne!(hash(&wide), hash(&tall));
    }

    #[test]
    fn a_record_matches_its_image_and_nothing_else() {
        let mut image = Image::filled(8, 8, [255, 255, 255, 255]);
        let record = Golden::approve("white", &image, "A Person");
        assert!(record.matches(&image));
        image.fill_rect(7, 7, 1, 1, [255, 255, 255, 254]);
        assert!(!record.matches(&image));
    }

    #[test]
    fn records_round_trip_through_ron() {
        let image = Image::filled(3, 2, [1, 2, 3, 4]);
        let record = Golden::approve("tiny", &image, "A Person");
        let text = record.to_ron();
        assert!(text.contains("Golden("), "{text}");
        assert_eq!(
            Golden::from_ron(&text, Path::new("tiny.ron")).unwrap(),
            record
        );
    }

    #[test]
    fn load_all_rejects_a_misnamed_record() {
        let dir = std::env::temp_dir().join(format!("xworks-golden-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let record = Golden::approve("right", &Image::new(1, 1), "A Person");
        std::fs::write(dir.join("wrong.ron"), record.to_ron()).unwrap();
        let result = load_all(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(result, Err(Error::Misnamed { .. })), "{result:?}");
    }
}
