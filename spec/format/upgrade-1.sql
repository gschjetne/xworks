-- Turns a schema 1 file's tables into schema 2's (schema-2.sql): store
-- runs these statements when it opens a schema 1 file, in the SQLite
-- transaction that also rewrites its snapshots and log in schema 2's
-- encoding and marks it schema 2 (README.md, "Upgrading").

-- Images the document shows, each stored once with the bytes it was
-- placed from: `id` is the SHA-256 of `data` in lowercase hexadecimal,
-- by which the document refers to the image, and `format` the format the
-- bytes are in (docs/ARCHITECTURE.md, section 10).
CREATE TABLE image (
    id     TEXT PRIMARY KEY NOT NULL,
    format TEXT NOT NULL CHECK (format IN ('png', 'jpeg', 'gif', 'webp')),
    data   BLOB NOT NULL
) STRICT;
