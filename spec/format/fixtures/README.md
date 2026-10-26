# Format fixtures

Files earlier xWorks wrote, kept so every later one is tested against
them: each must open with the same text and history it was written with
(`crates/store/tests/fixtures.rs`). Never open a fixture in place: opening
an older file upgrades it, so the tests open copies.

| File | What it is |
|---|---|
| `m1.xw` | a schema 1 document, written by M1 |
| `m1.dump` | M1's own dump of it (`xworks-dump`), which later dumps must match but for the schema line |
| `m1.ron` | the session that wrote it |
| `m2.xw` | a schema 2 document, written by M2 before its stage 9 |
| `m2.dump` | that code's own dump of it, which later dumps must match but for the schema line; its header row's cells name Table Header, as the upgrade has them since stage 9's follow-up (review sjj5ak5a) |
| `m2.ron` | the session that wrote it |

`m1.xw` was written at commit 74aaffb, M1's code, by playing `m1.ron`
with `app::session::play` and copying the file it made. Its log has 133
entries, so it holds snapshots after entries 0 and 100; it has two named
versions, one replayed from each snapshot, and undo and redo history with
two transactions on Redo. Its paragraphs have headings, bold, italic, a
size, a line separator, and text in Greek, Cyrillic, Hebrew, Arabic and
Chinese.

`m2.xw` was written the same way at commit e0db15b, the last that wrote
schema 2, by playing `m2.ron`. Its log has 129 entries, so it too holds
snapshots after entries 0 and 100, and two named versions, one replayed
from each; Redo holds two transactions. Its story has what schema 2
added: a list with a nested list, a numbered list, a task list, a quote,
a code block, a table and a picture, besides headings, emphasis, strong,
code and struck text in runs, and paragraphs with their own alignment,
spacing, indent, line height, tab stop and language.

The fuzz target `store_file` takes this folder as a seed corpus:

```
cd fuzz && cargo +nightly fuzz run store_file corpus/store_file ../spec/format/fixtures
```
