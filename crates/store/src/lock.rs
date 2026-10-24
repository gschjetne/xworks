//! A document open in one program at a time (project owner, 2026-10-23,
//! review a23vvez8): the program that has it open to write holds a lock
//! on a hidden file beside it, a dot, its name and `-lock`, as SQLite's
//! own files beside it are its name and `-wal` or `-shm`. The dot hides it
//! on Linux and macOS, and on Windows it's made hidden too (project owner,
//! 2026-10-23, review k07a0fw7), so it doesn't clutter the folder.
//!
//! The lock is the standard library's (`File::try_lock`): `flock` on
//! Linux and macOS, `LockFileEx` on Windows. The system lets it go when
//! the program ends, however it ends, so a crash leaves nothing that
//! keeps the document shut. The file says who holds it, for others to say
//! why they opened the document read-only; on Windows the lock keeps
//! others from reading even that, so they say only that another program
//! has it.
//!
//! The lock file goes when the document closes. A program that opened the
//! lock file just before its holder deleted it would lock a file no
//! longer there, so after locking, a lock is checked to be the file under
//! the name, and tried again if it isn't.
//!
//! A Markdown file's lock lives in xWorks' own folder instead (review
//! jy4a2tx5), where git and other tools don't see it: [`lock_in`] names it
//! there by a hash of the file's canonical path, so every xWorks on the
//! machine that uses that folder finds it, and [`acquire_at`] takes it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The lock file of the document at `document`: beside it, hidden, a dot,
/// its name and `-lock`. A document renamed or moved while it's open
/// takes its lock file along: [`Lock::moved`].
pub fn lock_path(document: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(document.file_name().unwrap_or_default());
    name.push("-lock");
    document.with_file_name(name)
}

/// The lock file, in `folder`, of the document at `document`, a file of
/// another format than xWorks' own, such as Markdown, whose folder its
/// lock mustn't clutter: its name, and the first sixteen hexadecimal
/// digits of the SHA-256 of its canonical path, so the same file has the
/// same lock however it's reached. A document that isn't there yet is
/// named by its path as it is.
pub fn lock_in(folder: &Path, document: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(document).unwrap_or_else(|_| document.to_path_buf());
    let hash = crate::sha256(canonical.as_os_str().as_encoded_bytes());
    let mut name = document.file_name().unwrap_or_default().to_os_string();
    name.push(format!(" {}-lock", &hash[..16]));
    folder.join(name)
}

/// A document's lock, held: let go, and its file deleted, when dropped.
#[derive(Debug)]
pub struct Lock {
    file: File,
    path: PathBuf,
}

/// What trying to lock a document came to.
#[derive(Debug)]
pub enum Acquired {
    /// The lock, held.
    Held(Lock),
    /// Another program holds it: who, as it says, if that can be read.
    InUse(Option<String>),
    /// Locks can't be had here, as on some network file systems, or the
    /// lock file can't be made, as in a folder that can't be written to.
    Unavailable,
}

/// Tries to lock the document at `document`, without waiting, by its
/// lock file beside it ([`lock_path`]).
pub fn acquire(document: &Path) -> Acquired {
    acquire_at(&lock_path(document))
}

/// Tries to take the lock file at `path`, without waiting: one beside its
/// document ([`lock_path`]) or in a folder of locks ([`lock_in`]), which is
/// made if it isn't there.
pub fn acquire_at(path: &Path) -> Acquired {
    let path = path.to_path_buf();
    if let Some(folder) = path.parent()
        && !folder.as_os_str().is_empty()
        && std::fs::create_dir_all(folder).is_err()
    {
        return Acquired::Unavailable;
    }
    // An orphan, a file its holder deleted after this program opened it,
    // is tried again; three in a row means something else is at work.
    for _ in 0..3 {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_ATTRIBUTE_HIDDEN, as the dot hides it elsewhere.
            options.attributes(0x2);
        }
        let Ok(file) = options.open(&path) else {
            return Acquired::Unavailable;
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Acquired::InUse(holder(&path)),
            Err(TryLockError::Error(_)) => return Acquired::Unavailable,
        }
        if !under_its_name(&file, &path) {
            continue;
        }
        // Who holds it, for others to say. It's only a courtesy, so a
        // failure to write it changes nothing.
        let _ = file.set_len(0);
        let _ = (&file).write_all(description().as_bytes());
        let _ = (&file).flush();
        return Acquired::Held(Lock { file, path });
    }
    Acquired::InUse(None)
}

impl Lock {
    /// The lock file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Takes the lock file along to the lock path of `document`, where
    /// the document has gone, still holding it: renaming an open file
    /// keeps its lock on Linux, macOS and Windows, where the standard
    /// library opens files so they can be renamed.
    pub fn moved(&mut self, document: &Path) -> std::io::Result<()> {
        self.moved_to(&lock_path(document))
    }

    /// Takes the lock file along to `path`, still holding it, as
    /// [`Lock::moved`] does: for a lock in a folder of locks, named by its
    /// document's path, which [`lock_in`] gives anew.
    pub fn moved_to(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::rename(&self.path, path)?;
        self.path = path.to_path_buf();
        Ok(())
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Deleted while still held, then let go as the file closes.
        let _ = std::fs::remove_file(&self.path);
        let _ = self.file.unlock();
    }
}

/// Whether `file`, locked, is the file named `path`: it hasn't been
/// deleted, and the file under the name is locked, which only `file`'s
/// lock can make it.
fn under_its_name(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if file.metadata().map_or(true, |m| m.nlink() == 0) {
            return false;
        }
    }
    #[cfg(not(unix))]
    let _ = file;
    match File::open(path) {
        Ok(other) => matches!(other.try_lock(), Err(TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// What the holder of a lock says of itself, if it can be read.
fn holder(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim();
    // A line of the holder's own making, not a whole file of anything.
    (!line.is_empty() && line.chars().count() <= 200).then(|| line.to_string())
}

/// This program, as a lock's holder: "xWorks, process 1234 on HOST".
fn description() -> String {
    let process = std::process::id();
    match host() {
        Some(host) => format!("xWorks, process {process} on {host}"),
        None => format!("xWorks, process {process}"),
    }
}

/// The computer's name, where the system says it simply.
fn host() -> Option<String> {
    let from_env = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok());
    #[cfg(unix)]
    let from_env = from_env.or_else(|| std::fs::read_to_string("/etc/hostname").ok());
    from_env
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty() && h.len() <= 100 && !h.contains('\n'))
}
