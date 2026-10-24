//! Files written whole, and told apart when another program changes
//! them (review jy4a2tx5): for documents kept as files of their own
//! format, as a Markdown file is, which are written whole on each save.
//!
//! - [`write_whole`] writes a file beside its name, flushes it to disk,
//!   and renames it into place, then flushes the folder where the system
//!   allows, so a crash leaves the file as it was or as it's written,
//!   never half written. It writes through a symbolic link to the file it
//!   names, with the file's permissions.
//! - [`stamp`] is what a file is on disk: its size, when it was last
//!   modified, and which file it is (its inode, or its file index on
//!   Windows), so a change another program makes, by writing over it or
//!   by putting another file in its place, is seen.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a file is on disk, to tell whether it's changed: its size, when
/// it was last modified, and which file it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// Its size in bytes.
    pub len: u64,
    /// When it was last modified, where the system says.
    pub modified: Option<SystemTime>,
    /// Which file it is: its device and inode on Unix, its volume and
    /// file index on Windows.
    pub identity: (u64, u64),
}

/// What the file at `path` is on disk, through symbolic links; none if
/// it isn't there or can't be read.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let file = File::open(path).ok()?;
    stamp_of(&file)
}

/// What the open `file` is on disk.
fn stamp_of(file: &File) -> Option<Stamp> {
    let meta = file.metadata().ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
        identity: identity(file, &meta)?,
    })
}

#[cfg(unix)]
fn identity(_: &File, meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    Some((meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn identity(file: &File, _: &std::fs::Metadata) -> Option<(u64, u64)> {
    let info = winapi_util::file::information(file).ok()?;
    Some((info.volume_serial_number(), info.file_index()))
}

#[cfg(not(any(unix, windows)))]
fn identity(_: &File, _: &std::fs::Metadata) -> Option<(u64, u64)> {
    Some((0, 0))
}

/// Writes `bytes` as the file at `path`, whole: beside it, flushed to
/// disk, then renamed into its place, and the folder flushed after where
/// the system allows; with the file's permissions, through a symbolic
/// link to the file it names. Returns what the file is on disk then.
pub fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<Stamp> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let new = beside(&target);
    let written = (|| {
        let mut file = File::create(&new)?;
        file.write_all(bytes)?;
        if let Ok(meta) = std::fs::metadata(&target) {
            let _ = file.set_permissions(meta.permissions());
        }
        file.sync_all()?;
        // The stamp of the file as it's renamed: a rename keeps its
        // size, its time and which file it is.
        let stamp = stamp_of(&file);
        drop(file);
        std::fs::rename(&new, &target)?;
        Ok(stamp)
    })();
    match written {
        Ok(stamp) => {
            sync_folder(&target);
            stamp
                .or_else(|| self::stamp(&target))
                .ok_or_else(|| std::io::Error::other("the file written can't be read"))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            Err(e)
        }
    }
}

/// Where a file at `target` is written before it's renamed into place: a
/// dot, its name and `.xworks-new`, beside it.
fn beside(target: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".xworks-new");
    target.with_file_name(name)
}

/// Flushes the folder `file` is in, so its new name is on disk too, where
/// the system lets a folder be opened and flushed; elsewhere, as on
/// Windows, the rename is durable once it's done.
fn sync_folder(file: &Path) {
    #[cfg(unix)]
    if let Some(folder) = file.parent()
        && let Ok(dir) = File::open(if folder.as_os_str().is_empty() {
            Path::new(".")
        } else {
            folder
        })
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = file;
}
