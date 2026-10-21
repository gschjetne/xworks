-- The xWorks document format, schema 1 (spec/format/README.md). store
-- creates every new file with these statements, and nothing else.

-- What the document is: `kind` (M1: "story"). Other keys may follow in
-- later schemas; readers ignore keys they don't know.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

-- Every transaction, as it was committed, in order: an edit ("commit"),
-- or undoing or redoing one. `ops` are the operations that were applied
-- and `inverse` those that undo them, each a RON list of doc::Op.
CREATE TABLE log (
    seq     INTEGER PRIMARY KEY NOT NULL CHECK (seq > 0),
    kind    TEXT NOT NULL CHECK (kind IN ('commit', 'undo', 'redo')),
    name    TEXT NOT NULL,
    ops     TEXT NOT NULL,
    inverse TEXT NOT NULL
) STRICT;

-- The whole document after the log entry `seq` (0: as created), as RON
-- of doc::Snapshot. Opening starts from the latest and replays the log
-- after it.
CREATE TABLE snapshot (
    seq      INTEGER PRIMARY KEY NOT NULL CHECK (seq >= 0),
    document TEXT NOT NULL
) STRICT;

-- Named versions: the document as it was after log entry `seq`.
CREATE TABLE version (
    seq  INTEGER NOT NULL CHECK (seq >= 0),
    name TEXT NOT NULL
) STRICT;

-- Fonts the document carries: font files, by the SHA-256 of their bytes
-- in lowercase hexadecimal.
CREATE TABLE font (
    sha256 TEXT PRIMARY KEY NOT NULL,
    data   BLOB NOT NULL
) STRICT;
