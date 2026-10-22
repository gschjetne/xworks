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

`m1.xw` was written at commit 74aaffb, M1's code, by playing `m1.ron`
with `app::session::play` and copying the file it made. Its log has 133
entries, so it holds snapshots after entries 0 and 100; it has two named
versions, one replayed from each snapshot, and undo and redo history with
two transactions on Redo. Its paragraphs have headings, bold, italic, a
size, a line separator, and text in Greek, Cyrillic, Hebrew, Arabic and
Chinese.

The fuzz target `store_file` takes this folder as a seed corpus:

```
cd fuzz && cargo +nightly fuzz run store_file corpus/store_file ../spec/format/fixtures
```
