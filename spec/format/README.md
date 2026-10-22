# The `.xw` format

An xWorks document is one file with the extension `.xw`: an SQLite database
(docs/ARCHITECTURE.md, section 6). This is schema 1, M1's.

## Recognizing a file

- **Application ID:** the database header's application ID is `0x78576B73`,
  "xWks" in ASCII. A file without it isn't an xWorks document.
- **Schema:** the header's user version is the schema version, here `1`.
  A reader that finds a version newer than it knows opens the file
  read-only and says so; it never writes to it, and never guesses at what
  it doesn't understand. Version 0 means a file that was never finished.

## Tables

[schema-1.sql](schema-1.sql) creates them; it is the definition, and
`store` runs it as it is.

| Table | Holds |
|---|---|
| `meta` | what the document is: its `kind`, `story` in M1 |
| `log` | every transaction, in commit order: edits, undos and redos |
| `snapshot` | the whole document after a log entry, from time to time |
| `version` | named versions, as log positions |
| `font` | font files the document carries, by hash |

## Encoding

Operations and snapshots are binary (project owner, 2026-10-22, review
dfx97ttt): `doc::Op` lists and `doc::Snapshot` values, as serde writes
them in [postcard](https://postcard.jamesmunns.com/wire-format)'s wire
format 1. Integers are variable-length, strings and lists carry their
length, text is UTF-8, and text offsets count Unicode scalar values.

postcard writes an enum's variant as its position, so the order of
variants in `doc::Op`, `doc::Object` and `doc::Value` is part of the
format: new variants go at the end, and reordering or removing one is a
new schema. A test in `store` pins the bytes of known values.

## Opening

1. Check the application ID and the schema version.
2. Read the latest snapshot.
3. Apply the operations of every log entry after it, in order.
4. Rebuild undo and redo from the whole log: an edit pushes onto Undo
   and clears Redo; an undo moves the last edit to Redo; a redo moves it
   back.

A snapshot or an operation that doesn't hold together is an error; the
reader never opens a document it can't vouch for.

## Writing

- A new file is written whole, beside its final name, and renamed into
  place, so a file under that name is always a finished one.
- Every transaction is appended to `log` in an SQLite transaction of its
  own as it's committed: nothing is ever rewritten.
- Every 100 log entries, a snapshot is written with the entry, so opening
  never replays more than 100.
- The database runs in write-ahead-log mode with `synchronous = NORMAL`.
  A crash of the program loses nothing committed; a power cut can lose
  the last few transactions, never the file.
