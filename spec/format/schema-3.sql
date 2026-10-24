-- The xWorks document format, schema 3 (spec/format/README.md), M2's
-- since its stage 9: schema 2's tables, whose documents now hold their own
-- stylesheets. store creates every new file with these statements, and
-- nothing else; upgrade-1.sql makes a schema 1 file's tables these, and a
-- schema 2 file's are these already.

-- What the document is: `kind` ("story"). Other keys may follow in later
-- schemas; readers ignore keys they don't know.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

-- Every transaction, as it was committed, in order: an edit ("commit"),
-- an edit that undoes together with the one before it ("continue", as
-- each keystroke of a run of typing is), or undoing or redoing one. `ops`
-- are the operations that were applied and `inverse` those that undo
-- them, each a list of doc::Op in postcard (README.md, "Encoding").
CREATE TABLE log (
    seq     INTEGER PRIMARY KEY NOT NULL CHECK (seq > 0),
    kind    TEXT NOT NULL CHECK (kind IN ('commit', 'continue', 'undo', 'redo')),
    name    TEXT NOT NULL,
    ops     BLOB NOT NULL,
    inverse BLOB NOT NULL
) STRICT;

-- The whole document after the log entry `seq` (0: as created), a
-- doc::Snapshot in postcard. Opening starts from the latest and replays
-- the log after it.
CREATE TABLE snapshot (
    seq      INTEGER PRIMARY KEY NOT NULL CHECK (seq >= 0),
    document BLOB NOT NULL
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

-- Images the document shows, each stored once with the bytes it was
-- placed from: `id` is the SHA-256 of `data` in lowercase hexadecimal,
-- by which the document refers to the image, and `format` the format the
-- bytes are in (docs/ARCHITECTURE.md, section 10).
CREATE TABLE image (
    id     TEXT PRIMARY KEY NOT NULL,
    format TEXT NOT NULL CHECK (format IN ('png', 'jpeg', 'gif', 'webp')),
    data   BLOB NOT NULL
) STRICT;
