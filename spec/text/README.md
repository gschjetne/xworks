# Text specs

Specs for `crates/text`, read by its tests (`crates/text/tests/`). The
fonts they use are the bundled ones in [fonts.ron](fonts.ron); fetch them
with `cargo run -p fetch-fonts`.

Offsets here count characters (Unicode scalar values), as the document
does, though `text` itself works in UTF-8 byte offsets. Widths are in
points; expected positions are in layout units, 914 400 to the inch and
12 700 to the point.

## Layout cases: `layout/*.ron`

One paragraph each, laid out and checked against what's written:

```ron
(
    text: "The quick brown fox jumps over the lazy dog.",
    // The paragraph's style. Defaults: size 12, regular, upright, "und".
    style: (family: "Noto Serif", size: 12, language: "en"),
    // Optional: stretches styled differently, from the start, each
    // `chars` long (the last may leave it out to take the rest), with
    // what differs from `style`.
    spans: [(chars: 4, bold: true), ()],
    width: 101,          // points
    direction: Auto,     // Auto (the default), Ltr or Rtl
    align: Start,        // Start (the default), Center or End
    paragraph: Ltr,      // the direction it should resolve to (default Ltr)
    // Optional: which family each stretch of characters comes from, in
    // logical order.
    fonts: [("The quick brown fox jumps over the lazy dog.", "Noto Serif")],
    lines: [
        (
            text: "The quick brown ",       // the line, in logical order
            // Optional: the line from left to right, in stretches of one
            // direction, each in logical order.
            visual: [Ltr("The quick brown ")],
            width: 1237184,     // optional: its width, less hanging spaces
            baseline: 162916,   // optional: from the paragraph's top
            height: 207569,     // optional
        ),
    ],
    // Optional: walks of the caret. From `from`, `by` repeated until it
    // reaches the paragraph's edge, visiting `visits`. A cursor is an
    // offset with `<` if it belongs with the character before it, `>` if
    // with the one after; the two differ where directions meet and at
    // line ends.
    carets: [(from: "0>", by: Right, visits: ["1<", "2<"])],
    // Optional: selecting `from` to `to`, in the text's order, highlights
    // `rects` separate stretches on screen.
    selections: [(from: 4, to: 11, rects: 2)],
    // Optional: what a double click (Word) or triple click (Sentence) at a
    // cursor selects, from and to, in characters.
    units: [("5>", Word, 4, 9)],
    // Optional: Justify too; whether words are hyphenated, by their
    // language's patterns (hyphenation.ron); and tab stops, in points.
    align: Justify,
    hyphenate: true,
    tabs: (stops: [(60, Start), (150, End), (300, Decimal('.'))], interval: 36),
    // Optional: where the caret is drawn at a cursor, in layout units, and
    // halfway between the carets at two cursors.
    positions: [("2>", 762000)],
    middles: [("19>", "25<", 2794000)],
)
```

A line may also say whether it ends in a hyphen that isn't in the text,
`hyphen: true`.

`by` is `Right` or `Left` (on screen), `Next` or `Previous` (in the
text's order, a grapheme at a time), or `NextWord`, `PreviousWord`,
`NextSentence` or `PreviousSentence`.

Besides what a case states, every case is checked for what holds of all
layouts: lines cover the text in order, fit the width unless a single
character doesn't, align as asked, and put their glyphs side by side; and
for every place the caret can stand, hit-testing there finds it again.

Where justified lines' widths are given, they're the paragraph's width:
that's what justifying means, whatever the fonts. Where tab stops place
text, the expected positions are the stops'. The hyphenation cases break
words where an implementation of Liang's algorithm independent of
`text`'s, in Python, puts hyphens with the same pattern files.

Where the expected widths come from: HarfBuzz 14.6.0 (C), through
uharfbuzz, shaping each stretch of one font and one direction on its own,
with advances scaled to layout units and rounded per glyph, half away from
zero. That's an implementation independent of HarfRust, which `text`
shapes with. Line heights and baselines come from the fonts' ascent and
descent (`OS/2` typographic metrics, or `hhea` where a font doesn't ask for
those), scaled the same way.

## Languages: `languages.ron`

Text whose glyphs depend on its language: the tag must reach the shaper,
which picks the font's localized forms (`locl`). Each entry lays out the
text in one family and checks the glyphs, by ID in the bundled font, for
each language. The IDs are HarfBuzz's.

## Hyphenation: `hyphenation.ron` and `hyphenated.ron`

`hyphenation.ron` lists TeX's pattern files that documents hyphenate with,
by Liang's algorithm, fetched by hash into `hyphenation/` with the fonts,
and each language's tags, licence and fewest letters a break leaves on
each side. `hyphenated.ron` holds words in each language with their
hyphens written in, from an implementation of the algorithm independent
of `text`'s, in Python, on the same files.
