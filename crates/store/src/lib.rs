//! The `.xw` file format: reading, writing, history, and the canonical text
//! dump (docs/ARCHITECTURE.md, section 6; spec/format/README.md).
//!
//! - [`Store::create`] writes a new document; [`Store::open`] reads one, at
//!   its last committed transaction, with its undo history, to write to
//!   unless [`Store::read_only`] says why not; [`Store::open_to_read`]
//!   reads one only.
//! - [`Store::append`] records each transaction as it's committed.
//! - [`Store::name_version`] and [`Store::document_at`]: named versions,
//!   and [`Store::copy_out`], a document made a file of its own.
//! - [`Store::embed_font`] and [`Store::add_image`]: fonts and pictures the
//!   document carries, each kept once by its hash.
//! - [`lock_path`]: the file whose lock says a program has the document
//!   open to write; [`acquire`] takes it. A Markdown file's lives in
//!   xWorks' own folder instead ([`lock_in`], [`acquire_at`]).
//! - [`write_whole`] and [`stamp`]: a file of another format, as a
//!   Markdown file is, written whole and durably, and what it is on disk,
//!   to see another program's changes.
//! - [`dump`]: a document file in canonical text form.
//!
//! store is an impure crate: it reads and writes files.
//!
//! Invariants:
//!
//! - A file under a document's name is always a finished one, and always
//!   opens at a committed transaction, whenever the writer stops.
//! - One program at a time writes to a document: the one holding its
//!   lock. Any other opens it read-only and can say who holds it, where
//!   the system lets it read that. The system lets a lock go when its
//!   program ends, however it ends.
//! - A file from a newer schema is never written to.
//! - A file from an older schema is upgraded when it's opened, all at once
//!   or not at all, and then opens as it did before.
//! - Nothing read from a file is trusted: what doesn't hold together is an
//!   error, never a panic or a broken document.

mod file;
mod lock;
mod v1;
mod v2;

use std::path::{Path, PathBuf};

pub use file::{Stamp, stamp, write_whole};
pub use lock::{Acquired, Lock, acquire, acquire_at, lock_in, lock_path};

use doc::{Document, History, Op, Snapshot, Transaction};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

/// The schema this crate reads and writes.
pub const SCHEMA: i64 = 3;

/// The SQLite application ID of xWorks documents: "xWks".
pub const APPLICATION_ID: i64 = 0x7857_6B73;

/// A snapshot goes with every this many log entries.
pub const SNAPSHOT_EVERY: i64 = 100;

const SCHEMA_SQL: &str = include_str!("../../../spec/format/schema-3.sql");

/// The ID a picture of `data` is kept under: the SHA-256 of its bytes, in
/// lowercase hexadecimal.
pub fn image_id(data: &[u8]) -> String {
    sha256(data)
}

/// Turns schema 1's tables into schema 2's.
const UPGRADE_1_SQL: &str = include_str!("../../../spec/format/upgrade-1.sql");

/// The formats of the pictures a file keeps, as its `image` table names
/// them (spec/format/schema-2.sql).
pub const IMAGE_FORMATS: [&str; 4] = ["png", "jpeg", "gif", "webp"];

/// What can go wrong with a document file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file isn't an xWorks document.
    #[error("{0} isn't an xWorks document")]
    NotADocument(PathBuf),
    /// The file is from a newer xWorks, and was opened read-only.
    #[error("the document is from a newer xWorks (schema {schema}), so it can't be changed here")]
    ReadOnly {
        /// The file's schema.
        schema: i64,
    },
    /// Another program has the document open, so it was opened read-only.
    #[error("the document is open in {}, so it can't be changed here", .holder.as_deref().unwrap_or("another program"))]
    InUse {
        /// The program, as it describes itself, where that could be read.
        holder: Option<String>,
    },
    /// The document was opened only to read, as a named version is.
    #[error("the document was opened only to read")]
    OpenToRead,
    /// Picture bytes in a format the file doesn't keep.
    #[error("{0:?} isn't a picture format a document keeps")]
    NotAPicture(String),
    /// Something in the file doesn't hold together.
    #[error("the document is damaged: {0}")]
    Damaged(String),
    /// A file is already where a new document would go.
    #[error("{0} already exists")]
    Exists(PathBuf),
    /// SQLite refused.
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file system refused.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// A transaction as the log records it.
#[derive(Clone, Copy, Debug)]
pub enum Entry<'a> {
    /// An edit.
    Commit(&'a Transaction),
    /// An edit that undoes together with the one before it, as each
    /// keystroke of a run of typing does.
    Continue(&'a Transaction),
    /// Undoing an edit: `transaction` is the edit undone.
    Undo(&'a Transaction),
    /// Redoing an edit undone.
    Redo(&'a Transaction),
}

/// Why a document file is open read-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    /// It's from a newer xWorks, of this schema, which this one can't
    /// write.
    Newer(i64),
    /// Another program has it open to write: the program, as it describes
    /// itself, where that could be read.
    InUse(Option<String>),
    /// It was opened only to read ([`Store::open_to_read`]).
    Asked,
}

impl std::fmt::Display for ReadOnly {
    /// Why, as a message ends: "made by a newer xWorks (schema 3)".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadOnly::Newer(schema) => write!(f, "made by a newer xWorks (schema {schema})"),
            ReadOnly::InUse(Some(holder)) => write!(f, "open in {holder}"),
            ReadOnly::InUse(None) => write!(f, "open in another program"),
            ReadOnly::Asked => write!(f, "opened to read"),
        }
    }
}

/// A picture the document shows, as the file keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Its ID: the SHA-256 of its bytes, in lowercase hexadecimal.
    pub id: String,
    /// Its format: one of [`IMAGE_FORMATS`].
    pub format: String,
    /// The bytes it was placed from.
    pub data: Vec<u8>,
}

/// A named version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// Its name.
    pub name: String,
    /// The log entry it follows.
    pub seq: i64,
}

/// An open document file.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
    path: PathBuf,
    /// Why it's open read-only, if it is.
    read_only: Option<ReadOnly>,
    /// The last log entry's sequence number.
    seq: i64,
    kind: String,
    /// The document's lock, while it's open to write, where locks can be
    /// had. After the connection, so it's let go after the connection
    /// closes.
    lock: Option<lock::Lock>,
}

/// How a file is opened: to write, holding its lock if there's one to
/// hold, or read-only, and why.
enum Mode {
    Write(Option<lock::Lock>),
    Read(ReadOnly),
}

/// A document file as opened: the file, and the document and its history
/// at the last committed transaction.
#[derive(Debug)]
pub struct Opened {
    /// The file.
    pub store: Store,
    /// The document.
    pub document: Document,
    /// Its undo and redo history.
    pub history: History,
}

impl Store {
    /// Writes `document`, of `kind` ("story"), as a new file at `path`,
    /// and opens it to write, holding its lock. It is written beside its
    /// name and renamed into place, so a crash never leaves a half-made
    /// file there.
    pub fn create(path: &Path, document: &Document, kind: &str) -> Result<Store, Error> {
        if path.exists() {
            return Err(Error::Exists(path.to_path_buf()));
        }
        // Locked first, so no other program opens it to write as it
        // appears; one already making it has it.
        let lock = match lock::acquire(path) {
            lock::Acquired::Held(lock) => Some(lock),
            lock::Acquired::Unavailable => None,
            lock::Acquired::InUse(_) => return Err(Error::Exists(path.to_path_buf())),
        };
        write_new(path, document, kind, |_| Ok(()))?;
        let mut store = Store::connect(path, Mode::Write(lock))?;
        store.seq = 0;
        Ok(store)
    }

    /// Opens a connection to an existing document file, checks what it is,
    /// and sets it up for writing if `mode` says so and it isn't from a
    /// newer schema; one from an older schema is upgraded first, which a
    /// file opened read-only can't be.
    fn connect(path: &Path, mode: Mode) -> Result<Store, Error> {
        if !path.is_file() {
            return Err(Error::NotADocument(path.to_path_buf()));
        }
        let not = || Error::NotADocument(path.to_path_buf());
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|_| not())?;
        if id != APPLICATION_ID {
            return Err(not());
        }
        let schema: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema < 1 {
            return Err(not());
        }
        let (read_only, lock) = match mode {
            _ if schema > SCHEMA => (Some(ReadOnly::Newer(schema)), None),
            Mode::Read(why) => (Some(why), None),
            Mode::Write(lock) => (None, lock),
        };
        match &read_only {
            Some(why) => {
                if schema < SCHEMA {
                    // It can't be read until it's upgraded, which only its
                    // writer can do.
                    return Err(match why {
                        ReadOnly::InUse(holder) => Error::InUse {
                            holder: holder.clone(),
                        },
                        _ => Error::OpenToRead,
                    });
                }
                connection.pragma_update(None, "query_only", true)?;
            }
            None => {
                connection.pragma_update(None, "journal_mode", "WAL")?;
                connection.pragma_update(None, "synchronous", "NORMAL")?;
                if schema < SCHEMA {
                    upgrade(&mut connection, schema)?;
                }
            }
        }
        let kind: String = connection
            .query_row("SELECT value FROM meta WHERE key = 'kind'", [], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::Damaged("it says nothing of its kind".to_string()))?;
        let seq: i64 =
            connection.query_row("SELECT COALESCE(MAX(seq), 0) FROM log", [], |row| {
                row.get(0)
            })?;
        Ok(Store {
            connection,
            path: path.to_path_buf(),
            read_only,
            seq,
            kind,
            lock,
        })
    }

    /// Opens the document at `path` at its last committed transaction, with
    /// its undo history, to write, holding its lock. A file another
    /// program has open, or from a newer schema, opens read-only:
    /// [`Store::read_only`] says why, and writing fails.
    pub fn open(path: &Path) -> Result<Opened, Error> {
        let mode = match lock::acquire(path) {
            lock::Acquired::Held(lock) => Mode::Write(Some(lock)),
            lock::Acquired::Unavailable => Mode::Write(None),
            lock::Acquired::InUse(holder) => Mode::Read(ReadOnly::InUse(holder)),
        };
        Store::opened(Store::connect(path, mode)?)
    }

    /// Opens the document at `path` only to read, as a named version is
    /// shown, without its lock: whoever has it open to write goes on
    /// writing. A file of an older schema, not yet upgraded, can't be read
    /// so.
    pub fn open_to_read(path: &Path) -> Result<Opened, Error> {
        Store::opened(Store::connect(path, Mode::Read(ReadOnly::Asked))?)
    }

    /// The document `store` holds at its last committed transaction, with
    /// its undo history.
    fn opened(store: Store) -> Result<Opened, Error> {
        let document = store.document_at(store.seq)?;
        let mut history = History::new();
        let mut statement = store
            .connection
            .prepare("SELECT seq, kind, name, ops, inverse FROM log ORDER BY seq")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let seq: i64 = row.get(0)?;
            let kind: String = row.get(1)?;
            let ok = match kind.as_str() {
                "commit" => {
                    history.push(Transaction {
                        name: row.get(2)?,
                        ops: decode(&row.get::<_, Vec<u8>>(3)?)?,
                        inverse: decode(&row.get::<_, Vec<u8>>(4)?)?,
                    });
                    true
                }
                "continue" => history.continue_last(Transaction {
                    name: row.get(2)?,
                    ops: decode(&row.get::<_, Vec<u8>>(3)?)?,
                    inverse: decode(&row.get::<_, Vec<u8>>(4)?)?,
                }),
                "undo" => history.replay_undo(),
                "redo" => history.replay_redo(),
                _ => false,
            };
            if !ok {
                return Err(Error::Damaged(format!(
                    "log entry {seq} doesn't follow from those before it"
                )));
            }
        }
        drop(rows);
        drop(statement);
        Ok(Opened {
            store,
            document,
            history,
        })
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document's kind: "story" in M1.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Why the file is open read-only, if it is: it's from a newer
    /// xWorks, another program has it open, or it was opened to read.
    pub fn read_only(&self) -> Option<&ReadOnly> {
        self.read_only.as_ref()
    }

    /// The lock file this store holds the lock of, while it has the
    /// document open to write, where locks can be had. A rename of the
    /// document while it's open takes the lock file along: see
    /// [`Store::moved`].
    pub fn lock_file(&self) -> Option<&Path> {
        self.lock.as_ref().map(|l| l.path())
    }

    /// Takes the document's lock file along to `path`'s, still holding
    /// it, after the document was renamed or moved there: for whatever
    /// renames an open document's files, which this lets keep the
    /// document from being opened twice throughout.
    pub fn moved(&mut self, path: &Path) -> Result<(), Error> {
        if let Some(lock) = &mut self.lock {
            lock.moved(path).map_err(|source| Error::Io {
                path: lock_path(path),
                source,
            })?;
        }
        Ok(())
    }

    /// The last log entry's sequence number: 0 for a file with no
    /// transactions yet.
    pub fn seq(&self) -> i64 {
        self.seq
    }

    /// Whether the file can be written to: an error saying why not if it's
    /// open read-only.
    pub fn writable(&self) -> Result<(), Error> {
        match &self.read_only {
            None => Ok(()),
            Some(ReadOnly::Newer(schema)) => Err(Error::ReadOnly { schema: *schema }),
            Some(ReadOnly::InUse(holder)) => Err(Error::InUse {
                holder: holder.clone(),
            }),
            Some(ReadOnly::Asked) => Err(Error::OpenToRead),
        }
    }

    /// Records a transaction just committed, which made `after`. Returns its
    /// sequence number. Every [`SNAPSHOT_EVERY`] entries, a snapshot of
    /// `after` goes with it, in the same SQLite transaction.
    pub fn append(&mut self, entry: Entry<'_>, after: &Document) -> Result<i64, Error> {
        self.writable()?;
        let (kind, name, ops, inverse) = match entry {
            Entry::Commit(t) => ("commit", &t.name, &t.ops, &t.inverse),
            Entry::Continue(t) => ("continue", &t.name, &t.ops, &t.inverse),
            Entry::Undo(t) => ("undo", &t.name, &t.inverse, &t.ops),
            Entry::Redo(t) => ("redo", &t.name, &t.ops, &t.inverse),
        };
        let seq = self.seq + 1;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO log (seq, kind, name, ops, inverse) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![seq, kind, name, encode(ops)?, encode(inverse)?],
        )?;
        if seq % SNAPSHOT_EVERY == 0 {
            tx.execute(
                "INSERT INTO snapshot (seq, document) VALUES (?1, ?2)",
                params![seq, encode(&after.to_snapshot())?],
            )?;
        }
        tx.commit()?;
        self.seq = seq;
        Ok(seq)
    }

    /// Names the document as it is now.
    pub fn name_version(&mut self, name: &str) -> Result<(), Error> {
        self.writable()?;
        self.connection.execute(
            "INSERT INTO version (seq, name) VALUES (?1, ?2)",
            params![self.seq, name],
        )?;
        Ok(())
    }

    /// The named versions, oldest first.
    pub fn versions(&self) -> Result<Vec<Version>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT name, seq FROM version ORDER BY seq, rowid")?;
        let versions = statement
            .query_map([], |row| {
                Ok(Version {
                    name: row.get(0)?,
                    seq: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(versions)
    }

    /// The document as it was after log entry `seq`: from the latest
    /// snapshot at or before it, with the entries after that applied.
    pub fn document_at(&self, seq: i64) -> Result<Document, Error> {
        let (from, bytes): (i64, Vec<u8>) = self
            .connection
            .query_row(
                "SELECT seq, document FROM snapshot WHERE seq <= ?1 ORDER BY seq DESC LIMIT 1",
                params![seq],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::Damaged("there's no snapshot to start from".to_string()))?;
        let snapshot: Snapshot = decode(&bytes)?;
        let mut document = Document::from_snapshot(snapshot)
            .map_err(|e| Error::Damaged(format!("the snapshot after entry {from}: {e}")))?;
        let mut statement = self
            .connection
            .prepare("SELECT seq, ops FROM log WHERE seq > ?1 AND seq <= ?2 ORDER BY seq")?;
        let mut rows = statement.query(params![from, seq])?;
        let mut expected = from + 1;
        while let Some(row) = rows.next()? {
            let at: i64 = row.get(0)?;
            if at != expected {
                return Err(Error::Damaged(format!("log entry {expected} is missing")));
            }
            let ops: Vec<Op> = decode(&row.get::<_, Vec<u8>>(1)?)?;
            document
                .apply_all(&ops)
                .map_err(|e| Error::Damaged(format!("log entry {at} doesn't apply: {e}")))?;
            expected += 1;
        }
        if seq > from && expected != seq + 1 {
            return Err(Error::Damaged(format!("log entry {expected} is missing")));
        }
        Ok(document)
    }

    /// Stores a font file the document carries. Returns its hash, by which
    /// it's stored once however often it's embedded.
    pub fn embed_font(&mut self, data: &[u8]) -> Result<String, Error> {
        self.writable()?;
        let hash = sha256(data);
        self.connection.execute(
            "INSERT OR IGNORE INTO font (sha256, data) VALUES (?1, ?2)",
            params![hash, data],
        )?;
        Ok(hash)
    }

    /// Keeps a picture the document shows: `data`, the bytes of a file in
    /// `format`, one of [`IMAGE_FORMATS`]. Returns its ID, the hash by
    /// which it's kept once however often it's placed, and by which the
    /// document refers to it. Pictures are kept for good, as undoing may
    /// bring back one taken out.
    pub fn add_image(&mut self, format: &str, data: &[u8]) -> Result<String, Error> {
        self.writable()?;
        if !IMAGE_FORMATS.contains(&format) {
            return Err(Error::NotAPicture(format.to_string()));
        }
        let id = sha256(data);
        self.connection.execute(
            "INSERT OR IGNORE INTO image (id, format, data) VALUES (?1, ?2, ?3)",
            params![id, format, data],
        )?;
        Ok(id)
    }

    /// The picture `id`, if the file keeps it. One whose bytes aren't
    /// what its ID says, or whose format isn't one of [`IMAGE_FORMATS`],
    /// is damage.
    pub fn image(&self, id: &str) -> Result<Option<Image>, Error> {
        let found: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT format, data FROM image WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((format, data)) = found else {
            return Ok(None);
        };
        if !IMAGE_FORMATS.contains(&format.as_str()) || sha256(&data) != id {
            return Err(Error::Damaged(format!(
                "the picture {id} isn't what it says"
            )));
        }
        Ok(Some(Image {
            id: id.to_string(),
            format,
            data,
        }))
    }

    /// The IDs of the pictures the file keeps, in order.
    pub fn image_ids(&self) -> Result<Vec<String>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM image ORDER BY id")?;
        let ids = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Writes `document`, which may be this file's as it was at a named
    /// version, as a new file at `path`: a document of its own, of this
    /// one's kind, with no history, carrying this one's fonts and the
    /// pictures `images` names. It's written beside its name and renamed
    /// into place, as [`Store::create`] writes, and isn't opened.
    pub fn copy_out(
        &self,
        path: &Path,
        document: &Document,
        images: &[String],
    ) -> Result<(), Error> {
        if path.exists() {
            return Err(Error::Exists(path.to_path_buf()));
        }
        let fonts = self.fonts()?;
        let mut kept = Vec::with_capacity(images.len());
        for id in images {
            if let Some(image) = self.image(id)? {
                kept.push(image);
            }
        }
        write_new(path, document, &self.kind, |tx| {
            for (hash, data) in &fonts {
                tx.execute(
                    "INSERT INTO font (sha256, data) VALUES (?1, ?2)",
                    params![hash, data],
                )?;
            }
            for image in &kept {
                tx.execute(
                    "INSERT OR IGNORE INTO image (id, format, data) VALUES (?1, ?2, ?3)",
                    params![image.id, image.format, image.data],
                )?;
            }
            Ok(())
        })
    }

    /// The fonts the document carries, by hash, in hash order.
    pub fn fonts(&self) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT sha256, data FROM font ORDER BY sha256")?;
        let fonts = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(fonts)
    }

    /// The log, as (sequence number, kind, name), in order.
    pub fn log(&self) -> Result<Vec<(i64, String, String)>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT seq, kind, name FROM log ORDER BY seq")?;
        let log = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(log)
    }
}

/// The document file at `path` in canonical text form: what it is, the
/// document as it stands, its history, named versions and fonts. Equal
/// documents with equal histories dump identically.
pub fn dump(path: &Path) -> Result<String, Error> {
    use std::fmt::Write as _;
    let opened = Store::open(path)?;
    let store = &opened.store;
    let mut out = String::new();
    let schema = match store.read_only() {
        Some(ReadOnly::Newer(schema)) => *schema,
        _ => SCHEMA,
    };
    let _ = writeln!(out, "xworks schema {schema}");
    let _ = writeln!(out, "kind {}", store.kind());
    let _ = writeln!(out, "document");
    out.push_str(&opened.document.dump());
    let _ = writeln!(out, "log");
    for (seq, kind, name) in store.log()? {
        let _ = writeln!(out, "    {seq} {kind} {name:?}");
    }
    let _ = writeln!(out, "versions");
    for version in store.versions()? {
        let _ = writeln!(out, "    {} {:?}", version.seq, version.name);
    }
    let _ = writeln!(out, "fonts");
    for (hash, data) in store.fonts()? {
        let _ = writeln!(out, "    {hash} {} bytes", data.len());
    }
    // Pictures only where there are some, so the dumps of files that
    // have none, M1's among them, stay as they were.
    let images = store.image_ids()?;
    if !images.is_empty() {
        let _ = writeln!(out, "images");
        for id in images {
            if let Some(image) = store.image(&id)? {
                let _ = writeln!(out, "    {id} {} {} bytes", image.format, image.data.len());
            }
        }
    }
    let _ = writeln!(
        out,
        "undo {:?} redo {:?}",
        opened.history.undo_name(),
        opened.history.redo_name()
    );
    Ok(out)
}

/// Upgrades the file `connection` is open on from schema `from` to
/// today's, in one SQLite transaction: its tables, and its snapshots and
/// log in today's encoding, so every named version still replays from the
/// snapshot before it. A schema 1 file's are read with schema 1's types;
/// a schema 2 file's documents gain the stylesheet they were laid out
/// with, and their paragraphs name its styles by ID, its tables' header
/// rows' its Table Header. If anything in it doesn't decode, the file is
/// left as it was.
fn upgrade(connection: &mut Connection, from: i64) -> Result<(), Error> {
    let tx = connection.transaction()?;
    if from == 1 {
        tx.execute_batch(UPGRADE_1_SQL)?;
    }
    let raw: Vec<(i64, Vec<u8>)> = tx
        .prepare("SELECT seq, document FROM snapshot ORDER BY seq")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut snapshots: Vec<(i64, Snapshot)> = Vec::with_capacity(raw.len());
    for (seq, bytes) in raw {
        let damaged = |e: Error| Error::Damaged(format!("the snapshot after entry {seq}: {e}"));
        let snapshot = if from == 1 {
            decode::<v1::Snapshot>(&bytes).map_err(damaged)?.into()
        } else {
            decode::<Snapshot>(&bytes).map_err(damaged)?
        };
        snapshots.push((seq, snapshot));
    }
    let raw: Vec<(i64, Vec<u8>, Vec<u8>)> = tx
        .prepare("SELECT seq, ops, inverse FROM log ORDER BY seq")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let ops = |seq: i64, bytes: &[u8]| -> Result<Vec<Op>, Error> {
        let damaged = |e: Error| Error::Damaged(format!("log entry {seq}: {e}"));
        if from == 1 {
            Ok(decode::<Vec<v1::Op>>(bytes)
                .map_err(damaged)?
                .into_iter()
                .map(Op::from)
                .collect())
        } else {
            decode::<Vec<Op>>(bytes).map_err(damaged)
        }
    };
    let mut entries: Vec<(i64, Vec<Op>, Vec<Op>)> = Vec::with_capacity(raw.len());
    for (seq, applied, inverse) in raw {
        entries.push((seq, ops(seq, &applied)?, ops(seq, &inverse)?));
    }
    // The stylesheet goes above every ID the file has ever given out.
    let top = snapshots
        .iter()
        .map(|(_, s)| s.next_id)
        .chain(
            entries
                .iter()
                .flat_map(|(_, a, b)| a.iter().chain(b))
                .flat_map(v2::created)
                .map(|id| id.saturating_add(1)),
        )
        .max()
        .unwrap_or(0);
    let headers = v2::header_cells(
        snapshots.iter().map(|(_, s)| s),
        entries.iter().flat_map(|(_, a, b)| a.iter().chain(b)),
    );
    let upgrade = v2::Upgrade::new(top, headers)?;
    for (seq, mut snapshot) in snapshots {
        upgrade.snapshot(&mut snapshot);
        tx.execute(
            "UPDATE snapshot SET document = ?1 WHERE seq = ?2",
            params![encode(&snapshot)?, seq],
        )?;
    }
    for (seq, mut applied, mut inverse) in entries {
        for op in applied.iter_mut().chain(inverse.iter_mut()) {
            upgrade.op(op);
        }
        tx.execute(
            "UPDATE log SET ops = ?1, inverse = ?2 WHERE seq = ?3",
            params![encode(&applied)?, encode(&inverse)?, seq],
        )?;
    }
    tx.pragma_update(None, "user_version", SCHEMA)?;
    tx.commit()?;
    Ok(())
}

/// Writes a new file at `path` holding `document`, of `kind`, with no
/// history, and whatever `fill` adds in the same SQLite transaction: whole,
/// beside its name, and renamed into place.
fn write_new(
    path: &Path,
    document: &Document,
    kind: &str,
    fill: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let partial = path.with_extension("xw-partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                path: partial,
                source,
            });
        }
    }
    {
        let mut connection = Connection::open(&partial)?;
        let tx = connection.transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('kind', ?1)",
            params![kind],
        )?;
        tx.execute(
            "INSERT INTO snapshot (seq, document) VALUES (0, ?1)",
            params![encode(&document.to_snapshot())?],
        )?;
        fill(&tx)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", SCHEMA)?;
        tx.commit()?;
        connection.close().map_err(|(_, e)| e)?;
    }
    std::fs::rename(&partial, path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The SHA-256 of `data`, in lowercase hexadecimal: how fonts and pictures
/// are kept.
fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `value` in the file's encoding, postcard (spec/format/README.md).
pub fn encode<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    postcard::to_stdvec(value).map_err(|e| Error::Damaged(format!("encoding: {e}")))
}

/// A value read back from the file's encoding.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    postcard::from_bytes(bytes).map_err(|e| Error::Damaged(format!("reading: {e}")))
}
