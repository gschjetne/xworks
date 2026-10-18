# Golden images

Approved renders of xWorks' own output (CLAUDE.md, rule 11). They are
recorded as hashes, not images. Each `NAME.ron` names one of `shot`'s scenes
and holds the size and the SHA-256 of the pixels `shot` drew for it (width
and height as big-endian 32-bit numbers, then every pixel's red, green, blue
and alpha bytes, row by row), and who approved it. Renders are exact, so the
approved image is what `shot` draws at the commit that last changed the
record:

```
git log -1 --format=%h -- spec/golden/NAME.ron
```

To approve renders, look at them first (`cargo run -p shot -- NAME` writes
`target/shot/NAME.png`), then record them from the repository root, all
scenes or the ones named:

```
cargo run -p shot -- --approve "Your Name" [NAME...]
```

and commit the records. Only a person may approve: an agent records an
approval only after the person gave it, in a review session, and names that
session in the commit.

- CI (`tools/shot/tests/golden.rs`) renders every recorded scene again and
  checks its hash. On a mismatch it writes the new render to
  `target/golden-reports/`.
- Nothing but these records and this README belongs here; a test checks
  that too.
