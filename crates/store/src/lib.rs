//! The `.xw` file format: reading, writing, history, and the canonical text
//! dump (docs/ARCHITECTURE.md, section 6; spec/format/README.md).
//!
//! - [`Store::create`] writes a new document; [`Store::open`] reads one, at
//!   its last committed transaction, with its undo history.
//! - [`Store::append`] records each transaction as it's committed.
//! - [`Store::name_version`] and [`Store::document_at`]: named versions.
//! - [`Store::embed_font`]: fonts the document carries.
//! - [`dump`]: a document file in canonical text form.
//!
//! store is an impure crate: it reads and writes files.
//!
//! Invariants:
//!
//! - A file under a document's name is always a finished one, and always
//!   opens at a committed transaction, whenever the writer stops.
//! - A file from a newer schema is never written to.
//! - A file from an older schema is upgraded when it's opened, all at once
//!   or not at all, and then opens as it did before.
//! - Nothing read from a file is trusted: what doesn't hold together is an
//!   error, never a panic or a broken document.

mod v1;

use std::path::{Path, PathBuf};

use doc::{Document, History, Op, Snapshot, Transaction};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

/// The schema this crate reads and writes.
pub const SCHEMA: i64 = 2;

/// The SQLite application ID of xWorks documents: "xWks".
pub const APPLICATION_ID: i64 = 0x7857_6B73;

/// A snapshot goes with every this many log entries.
pub const SNAPSHOT_EVERY: i64 = 100;

const SCHEMA_SQL: &str = include_str!("../../../spec/format/schema-2.sql");

/// Turns schema 1's tables into schema 2's.
const UPGRADE_1_SQL: &str = include_str!("../../../spec/format/upgrade-1.sql");

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
    /// The file's schema, when it's newer than ours.
    newer: Option<i64>,
    /// The last log entry's sequence number.
    seq: i64,
    kind: String,
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
    /// Writes `document`, of `kind` ("story"), as a new file at `path`. It
    /// is written beside its name and renamed into place, so a crash never
    /// leaves a half-made file there.
    pub fn create(path: &Path, document: &Document, kind: &str) -> Result<Store, Error> {
        if path.exists() {
            return Err(Error::Exists(path.to_path_buf()));
        }
        let partial = path.with_extension("xw-partial");
        let io = |source| Error::Io {
            path: partial.clone(),
            source,
        };
        match std::fs::remove_file(&partial) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
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
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", SCHEMA)?;
            tx.commit()?;
            connection.close().map_err(|(_, e)| e)?;
        }
        std::fs::rename(&partial, path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut store = Store::connect(path)?;
        store.seq = 0;
        Ok(store)
    }

    /// Opens a connection to an existing document file, checks what it is,
    /// and sets it up for writing unless it's from a newer schema; one
    /// from an older schema is upgraded first.
    fn connect(path: &Path) -> Result<Store, Error> {
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
        let newer = (schema > SCHEMA).then_some(schema);
        if newer.is_some() {
            connection.pragma_update(None, "query_only", true)?;
        } else {
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            if schema < SCHEMA {
                upgrade(&mut connection, schema)?;
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
            newer,
            seq,
            kind,
        })
    }

    /// Opens the document at `path` at its last committed transaction, with
    /// its undo history. A file from a newer schema opens read-only:
    /// [`Store::read_only`] says so, and writing fails.
    pub fn open(path: &Path) -> Result<Opened, Error> {
        let store = Store::connect(path)?;
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

    /// The file's schema, if it's newer than this xWorks reads and writes,
    /// so the file is open read-only.
    pub fn read_only(&self) -> Option<i64> {
        self.newer
    }

    /// The last log entry's sequence number: 0 for a file with no
    /// transactions yet.
    pub fn seq(&self) -> i64 {
        self.seq
    }

    fn writable(&self) -> Result<(), Error> {
        match self.newer {
            Some(schema) => Err(Error::ReadOnly { schema }),
            None => Ok(()),
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
        let hash: String = Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.connection.execute(
            "INSERT OR IGNORE INTO font (sha256, data) VALUES (?1, ?2)",
            params![hash, data],
        )?;
        Ok(hash)
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
    let schema = store.read_only().unwrap_or(SCHEMA);
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
/// snapshot before it. If anything in it doesn't decode, the file is left
/// as it was.
fn upgrade(connection: &mut Connection, from: i64) -> Result<(), Error> {
    let tx = connection.transaction()?;
    if from == 1 {
        tx.execute_batch(UPGRADE_1_SQL)?;
        let snapshots: Vec<(i64, Vec<u8>)> = tx
            .prepare("SELECT seq, document FROM snapshot ORDER BY seq")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (seq, bytes) in snapshots {
            let snapshot: doc::Snapshot = decode::<v1::Snapshot>(&bytes)
                .map_err(|e| Error::Damaged(format!("the snapshot after entry {seq}: {e}")))?
                .into();
            tx.execute(
                "UPDATE snapshot SET document = ?1 WHERE seq = ?2",
                params![encode(&snapshot)?, seq],
            )?;
        }
        let entries: Vec<(i64, Vec<u8>, Vec<u8>)> = tx
            .prepare("SELECT seq, ops, inverse FROM log ORDER BY seq")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let ops = |seq: i64, bytes: &[u8]| -> Result<Vec<u8>, Error> {
            let ops: Vec<Op> = decode::<Vec<v1::Op>>(bytes)
                .map_err(|e| Error::Damaged(format!("log entry {seq}: {e}")))?
                .into_iter()
                .map(Op::from)
                .collect();
            encode(&ops)
        };
        for (seq, applied, inverse) in entries {
            tx.execute(
                "UPDATE log SET ops = ?1, inverse = ?2 WHERE seq = ?3",
                params![ops(seq, &applied)?, ops(seq, &inverse)?, seq],
            )?;
        }
    }
    tx.pragma_update(None, "user_version", SCHEMA)?;
    tx.commit()?;
    Ok(())
}

/// `value` in the file's encoding, postcard (spec/format/README.md).
pub fn encode<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    postcard::to_stdvec(value).map_err(|e| Error::Damaged(format!("encoding: {e}")))
}

/// A value read back from the file's encoding.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    postcard::from_bytes(bytes).map_err(|e| Error::Damaged(format!("reading: {e}")))
}
