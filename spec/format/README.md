# The `.xw` format

An xWorks document is one file with the extension `.xw`: an SQLite database
(docs/ARCHITECTURE.md, section 6). This is schema 3, M2's since its stage 9,
whose documents carry their own stylesheets; schema 2 was M2's before it,
and schema 1 M1's. A file of an older schema is upgraded when it's opened
("Upgrading").

## Recognizing a file

- **Application ID:** the database header's application ID is `0x78576B73`,
  "xWks" in ASCII. A file without it isn't an xWorks document.
- **Schema:** the header's user version is the schema version, here `3`.
  A reader that finds a version newer than it knows opens the file
  read-only and says so; it never writes to it, and never guesses at what
  it doesn't understand. Version 0 means a file that was never finished.

## Tables

[schema-3.sql](schema-3.sql) creates them; it is the definition, and
`store` runs it as it is. They're schema 2's tables, and
[schema-2.sql](schema-2.sql) and [schema-1.sql](schema-1.sql) are kept as
the records of the schemas before.

| Table | Holds |
|---|---|
| `meta` | what the document is: its `kind`, `story` |
| `log` | every transaction, in commit order: edits, continuing edits, undos and redos |
| `snapshot` | the whole document after a log entry, from time to time |
| `version` | named versions, as log positions |
| `font` | font files the document carries, by hash |
| `image` | images the document shows, by hash, with their format and the bytes they were placed from (new in schema 2) |

## The document

A snapshot holds every object of the document by ID (`doc::Object`). The
root is a story, a tree of blocks:

- a **paragraph** holds text, runs of character properties along it, and
  properties of its own, such as its style. Headings and code blocks are
  paragraphs, set apart by their style.
- a **container** (`doc::Block`) holds blocks, by kind: a **list** holds
  items, and its properties say whether it's numbered and from what; an
  **item** and a **quote** hold blocks as the story does; a **table**
  holds rows, the first its header; a **row** holds its cells, each a
  paragraph. Containers may be empty.

Every object but the root names the container it's in, and is listed by
it exactly once; there is always a paragraph. M1's stories were flat:
schema 1's story listed paragraphs only.

Beside the story is the document's **stylesheet** (`doc::Sheet`), new in
schema 3: a second root, listing its styles and named parts, each by ID,
in order, with properties of its own: the defaults every chain of styles
ends in (`font`, `size`, `color` and the rest), what the document shares
(`tab-interval`, `document-tabs`, and how quotes, code and tables look:
`quote-indent`, `quote-bar`, `quote-color`, `code-tint`, `code-padding`,
`table-space`, `cell-padding`, `rule`, `hairline`, `rule-color`,
`hairline-color`, `missing-picture`), and where it came from (`origin`,
the stylesheet's name, and `origin-version`, its version). A document
may have none, as a new `doc::Document` has none; one xWorks makes always
has one.

- A **style** (`doc::Style`) is of a kind (`doc::StyleKind`): a paragraph
  style, a character style or a page style, or a **named part** styles
  refer to: a face (`family`), a colour (`color`, `#rrggbb`), a layout of
  tab stops (`tabs`) or a list definition (`bullets`, `numbers`,
  `list-indent`, `marker-gap`). A style may be based on another of its
  kind, and sets only what differs from it; parts are based on nothing.
  No chain of bases comes back to where it started. Its `name` and `role`
  are among its properties: its name is what people see and is unique to
  nothing, its role (`body`, `heading-1`, `code`, `emphasis`, `text`,
  `page` …) what another sheet's style matches it by.
- A value naming another object is a **reference** (`doc::Value::Ref`): a
  paragraph's `style`, a run's `char-style`, or a style's `font`, `color`
  or `tabs` naming a part, whose value it then takes. A reference to
  nothing, or to an object of the wrong kind, is as if unset.
- A page style holds `page-width`, `page-height` and `margin-top`,
  `margin-bottom`, `margin-left` and `margin-right`; the document is laid
  out on the stylesheet's first.
- Lengths are in layout units, 914 400 to the inch, as everywhere.

A **picture** is in a paragraph's text: the character U+FFFC, whose run
has the properties `picture`, the picture's ID in the `image` table
(text), `picture-alt`, what it shows in words (text), and
`picture-width` and `picture-height`, the size it's placed at in layout
units (numbers). A paragraph of one picture alone is a figure. Pictures
are kept for good, as undoing may bring back one taken out; one a file
lacks is drawn as a tint.

## Encoding

Operations and snapshots are binary (project owner, 2026-10-22, review
dfx97ttt): `doc::Op` lists and `doc::Snapshot` values, as serde writes
them in [postcard](https://postcard.jamesmunns.com/wire-format)'s wire
format 1. Integers are variable-length, strings and lists carry their
length, text is UTF-8, and text offsets count Unicode scalar values.

postcard writes an enum's variant as its position, so the order of
variants in `doc::Op`, `doc::Object`, `doc::Kind`, `doc::StyleKind` and
`doc::Value` is part of the format: new variants go at the end, and
reordering or removing one is a new schema. Schema 2 added operations on
blocks after M1's six, and containers after stories and paragraphs, so
every schema 1 value means the same in schema 2. Schema 3 added the
stylesheet and styles after containers, references after text, and four
operations after schema 2's ten: `InsertStyle`, `DeleteStyle`,
`SetStyleProp` (on a style, a part or the stylesheet) and `SetBase`; so a
schema 2 value decodes as schema 3's types, and what it says changes only
by the upgrade. Tests in `store` pin the bytes of known values.

## Opening

0. Lock the document's lock file (below), or open it read-only if
   another program holds it.
1. Check the application ID and the schema version; upgrade an older
   schema.
2. Read the latest snapshot.
3. Apply the operations of every log entry after it, in order.
4. Rebuild undo and redo from the whole log: an edit pushes onto Undo
   and clears Redo; a continuing edit joins the last edit on Undo, so they
   undo together, and clears Redo; an undo moves the last edit to Redo; a
   redo moves it back.

A snapshot or an operation that doesn't hold together is an error; the
reader never opens a document it can't vouch for.

## Upgrading

Opening a file of an older schema upgrades it in one SQLite transaction,
so it's upgraded whole or not at all:

- **From schema 1:** [upgrade-1.sql](upgrade-1.sql) adds the `image`
  table, and every snapshot and every log entry is read with schema 1's
  types, kept frozen in `store`; then it's upgraded as a schema 2 file is.
- **From schema 2:** every snapshot gains the stylesheet schema 2's
  documents were laid out in, Neutral as it was at version 1, frozen as
  objects in [upgrade-2.ron](upgrade-2.ron), at IDs above every ID the
  file has ever given out, in a snapshot or an operation; every snapshot's
  next ID goes past them. In every snapshot and every log entry, applied
  or undoing, a paragraph's `style`, which named its style by its name,
  names it by ID; a name the sheet hasn't, which laid out as the default
  style, is unset. Everything is written in schema 3's encoding, and the
  user version becomes 3. Named versions replay from the snapshots before
  them, and undo and redo take back what they did. What doesn't decode
  leaves the file as it was, and opening it is an error.

[fixtures/](fixtures/) holds files older xWorks wrote, which every later
one must open with the same text and history.

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
- A picture is written to `image` before the transaction that places it,
  so a log entry never names a picture the file lacks.
- A named version copied out is a new file of its own: the document as
  the version was, as its snapshot 0, with no log, the fonts and the
  pictures it shows.

## One writer at a time

A program writes to a document only while it holds the lock of the file
beside it named as the document is, hidden, with a dot before and
`-lock` after (`Report.xw` and `.Report.xw-lock`; on Windows the file is
hidden too), taken with the system's own advisory lock on the
whole file (`flock`, or `LockFileEx` on Windows). The holder writes a
line saying who it is ("xWorks, process 4120 on studio"), deletes the
file when it closes the document, and takes it along, still held, if it
renames or moves the document. Another program finding it locked opens
the document read-only. The system lets a lock go when its program ends,
so a lock file left by a crash holds nothing. The lock file is not part
of the document: copying a document leaves it behind.
