//! Paragraph layout: lines broken first fit, or optimally where they're
//! justified, each ordered for display by the bidi algorithm, aligned,
//! and stacked.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::ops::Range;

use locale::{Bidi, Direction, Language};

use crate::breaking::{self, Candidate, Glue, Kind};
use crate::font::{FaceId, Fonts};
use crate::hyphen::Hyphenation;
use crate::shape::{self, Item, Shaped};
use crate::tabs::{self, After, Tabs};
use crate::units::scale;

/// How a stretch of text is set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    /// The font family, such as "Noto Serif". Characters it lacks come from
    /// its fallbacks ([`Fonts`]).
    pub family: String,
    /// The size of its em, in layout units: 12 points is 152 400.
    pub size: i64,
    /// Its weight, from 1 to 1000: 400 is regular, 700 bold.
    pub weight: u16,
    /// Whether it's italic.
    pub italic: bool,
    /// Its language, which picks the font's forms for it, where its lines
    /// may break, how its words are hyphenated, and what its words are.
    pub language: Language,
    /// Which classes of ligature it's set with.
    pub ligatures: Ligatures,
}

/// Which of OpenType's classes of ligature a stretch of text is set with
/// (ARCHITECTURE.md section 9). A style says, class by class; code sets
/// none, so it shows exactly what was typed. As specs write it, a class
/// left out keeps its default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(default)]
pub struct Ligatures {
    /// Standard ligatures, such as "fi" and "ffl" (`liga`, `clig`): on by
    /// default.
    pub standard: bool,
    /// Contextual alternates (`calt`), with which code fonts draw "->" as
    /// an arrow: on by default.
    pub contextual: bool,
    /// Discretionary ligatures (`dlig`): off by default.
    pub discretionary: bool,
    /// Historical ligatures (`hlig`): off by default.
    pub historical: bool,
}

impl Ligatures {
    /// None at all.
    pub const NONE: Ligatures = Ligatures {
        standard: false,
        contextual: false,
        discretionary: false,
        historical: false,
    };

    /// The OpenType features that set these, where they differ from the
    /// features' own defaults, so the defaults shape as with none asked
    /// for.
    pub(crate) fn features(self) -> Vec<harfrust::Feature> {
        let default = Ligatures::default();
        let mut out = Vec::new();
        let mut set = |tags: &[&[u8; 4]], on: bool, was: bool| {
            if on != was {
                for tag in tags {
                    out.push(harfrust::Feature::new(
                        harfrust::Tag::new(tag),
                        u32::from(on),
                        ..,
                    ));
                }
            }
        };
        set(&[b"liga", b"clig"], self.standard, default.standard);
        set(&[b"calt"], self.contextual, default.contextual);
        set(&[b"dlig"], self.discretionary, default.discretionary);
        set(&[b"hlig"], self.historical, default.historical);
        out
    }
}

impl Default for Ligatures {
    /// OpenType's defaults: standard and contextual on, the others off.
    fn default() -> Ligatures {
        Ligatures {
            standard: true,
            contextual: true,
            discretionary: false,
            historical: false,
        }
    }
}

/// A stretch of a paragraph's text in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Its length in bytes.
    pub len: usize,
    /// Its style.
    pub style: Style,
}

/// Where lines sit between the paragraph's edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum Align {
    /// Against the edge the paragraph's direction starts from: the left
    /// in a left-to-right paragraph, the right in a right-to-left one.
    #[default]
    Start,
    /// Centred.
    Center,
    /// Against the other edge.
    End,
    /// Against both edges: lines are broken by Knuth and Plass's optimal
    /// algorithm and stretched to the width at their spaces, and between
    /// the characters of Chinese and Japanese. The last line, and lines
    /// ending at a hard break, sit at the start.
    Justify,
}

/// A paragraph to lay out.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    /// Its text. Line separators (U+2028) and other hard breaks end lines.
    pub text: &'a str,
    /// Its styles, in order, covering the text exactly. There is always at
    /// least one, so an empty paragraph has a style too.
    pub spans: &'a [Span],
    /// Its direction; `None` takes it from the text's first strong
    /// character.
    pub direction: Option<Direction>,
    /// How its lines are aligned.
    pub align: Align,
    /// How wide it is, in layout units.
    pub width: i64,
}

/// How a paragraph is set beyond its text and styles: whether its words
/// are hyphenated, where its tabs go, how tall its lines are, how far
/// its first line is indented, and what objects stand in its text. The
/// default hyphenates nothing but at soft hyphens, makes tabs as wide as
/// spaces, lines as tall as their fonts, indents nothing, and has no
/// objects.
#[derive(Clone, Copy, Debug, Default)]
pub struct Setting<'a> {
    /// Whether words may be hyphenated at lines' ends, by their language's
    /// patterns in `hyphenation`. Soft hyphens (U+00AD) always may.
    pub hyphenate: bool,
    /// The patterns, by language.
    pub hyphenation: Option<&'a Hyphenation>,
    /// The tab stops.
    pub tabs: Option<&'a Tabs>,
    /// How tall each line is, in thousandths of the largest font size on
    /// it: 1 500 is one and a half times the font. The space it adds to
    /// the fonts' own ascent and descent, or takes from them, is split
    /// evenly above and below the text. `None` makes lines as tall as
    /// their fonts.
    pub line_height: Option<i64>,
    /// How far the first line starts in from the paragraph's start edge,
    /// in layout units, and so how much narrower it is; negative hangs it
    /// out past the edge.
    pub indent: i64,
    /// Objects set in the text as characters are, such as pictures, by the
    /// characters they stand for.
    pub objects: &'a [Inline],
}

/// Something set in a paragraph's text as a character is, such as a
/// picture: a box standing on the baseline in place of the character at
/// `offset`, usually U+FFFC OBJECT REPLACEMENT CHARACTER. Lines break
/// around it as the character allows, the caret stands either side of it,
/// and its line is at least as tall above the baseline as it is. No font
/// draws it: its caller draws it where [`Line::objects`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inline {
    /// The byte offset of the character it stands for. One that isn't a
    /// character's start in the text, or that's there twice, is ignored.
    pub offset: usize,
    /// Its width, in layout units; negative counts as none.
    pub width: i64,
    /// Its height above the baseline, in layout units; negative counts
    /// as none.
    pub height: i64,
}

/// An inline object on a line, where layout put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Placed {
    /// The byte offset of the character it stands for.
    pub offset: usize,
    /// Its left edge, from the paragraph's left edge.
    pub x: i64,
    /// Its width.
    pub width: i64,
    /// Its height: it stands on the line's baseline, so its top is the
    /// baseline less this.
    pub height: i64,
}

/// A paragraph laid out: its lines, top to bottom. Positions are in layout
/// units from the paragraph's top left, with y growing down the page.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Its lines, in order. There is always at least one.
    pub lines: Vec<Line>,
    /// The width it was laid out to.
    pub width: i64,
    /// The height of its lines together.
    pub height: i64,
    /// Whether a character had no face to come from, so it shows as a box:
    /// the fonts lack something the text needs.
    pub missing: bool,
    /// The languages it had words to hyphenate in but no patterns for,
    /// so the caller can load them and lay it out again.
    pub unhyphenated: Vec<Language>,
    pub(crate) direction: Direction,
    /// Its text.
    pub(crate) text: String,
    /// Each span's language, as a tag; `None` where it isn't known.
    pub(crate) languages: Vec<Option<String>>,
    /// The text's languages, as ranges of it, for finding its words and
    /// sentences.
    pub(crate) spans: Vec<(Range<usize>, Language)>,
    /// Where its graphemes begin and end, from 0 to the text's length.
    pub(crate) graphemes: Vec<usize>,
    /// The offsets just after hard line breaks.
    pub(crate) hard_breaks: Vec<usize>,
}

impl Layout {
    /// The paragraph's direction.
    pub fn direction(&self) -> Direction {
        self.direction
    }
}

/// A line of a paragraph.
#[derive(Clone, Debug)]
pub struct Line {
    /// Its text, as byte offsets into the paragraph, including the spaces
    /// or hard break that end it.
    pub range: Range<usize>,
    /// Its top.
    pub top: i64,
    /// Its baseline.
    pub baseline: i64,
    /// How far its tallest text reaches above the baseline.
    pub ascent: i64,
    /// How far its deepest text reaches below the baseline.
    pub descent: i64,
    /// Its height: its ascent and descent together.
    pub height: i64,
    /// The left edge of its text, not counting spaces that hang past its
    /// end.
    pub x: i64,
    /// The width of its text, not counting spaces that hang past its end,
    /// as stretched if it's justified.
    pub width: i64,
    /// Whether it ends in a hyphen that isn't in the text: a word broken
    /// by hyphenation, or at a soft hyphen.
    pub hyphen: bool,
    /// Its glyphs, in runs from left to right.
    pub runs: Vec<GlyphRun>,
    /// Its inline objects, from left to right, for the caller to draw.
    pub objects: Vec<Placed>,
    /// Its graphemes from left to right, where the caret can stand.
    pub(crate) cells: Vec<Cell>,
}

/// Glyphs from one face, in one style and one direction.
#[derive(Clone, Debug)]
pub struct GlyphRun {
    /// The face they come from.
    pub face: FaceId,
    /// The size of its em, in layout units.
    pub size: i64,
    /// The span of the paragraph whose style they have.
    pub span: usize,
    /// The text they show, as byte offsets into the paragraph.
    pub range: Range<usize>,
    /// Its bidi level: even runs read left to right, odd ones right to
    /// left.
    pub level: u8,
    /// Whether its glyphs are to be slanted: its style is italic but its
    /// face isn't, because the family has no italic, as Arabic, Hebrew and
    /// CJK fonts often don't (project owner, 2026-10-21).
    pub oblique: bool,
    /// Its left edge.
    pub x: i64,
    /// Its width: its glyphs' advances together.
    pub width: i64,
    /// Its glyphs, left to right.
    pub glyphs: Vec<Glyph>,
}

/// A positioned glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// Its ID in its run's face.
    pub id: u32,
    /// Its origin's distance from the paragraph's left edge.
    pub x: i64,
    /// Its origin's distance below the line's baseline.
    pub y: i64,
    /// How far it moves the pen.
    pub advance: i64,
    /// Where its cluster of the text starts, as a byte offset into the
    /// paragraph.
    pub cluster: usize,
    /// Whether it's a hyphen layout added where it broke a word, which
    /// shows no character of the text; its cluster is the line's end.
    pub hyphen: bool,
}

/// A grapheme on a line, for caret placement: its text and its extent.
#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub range: Range<usize>,
    pub left: i64,
    pub right: i64,
    pub rtl: bool,
}

/// Lays out `paragraph` in `fonts`, with no hyphenation but at soft
/// hyphens, and tabs as wide as spaces: [`layout_with`] and a default
/// [`Setting`].
///
/// # Panics
///
/// If the spans don't cover the text exactly, a span doesn't end on a
/// character boundary, or there are no fonts.
pub fn layout(fonts: &Fonts, paragraph: &Paragraph<'_>) -> Layout {
    layout_with(fonts, paragraph, &Setting::default())
}

/// Lays out `paragraph` in `fonts`, hyphenated and tabbed as `setting`
/// says.
///
/// # Panics
///
/// If the spans don't cover the text exactly, a span doesn't end on a
/// character boundary, or there are no fonts.
pub fn layout_with(fonts: &Fonts, paragraph: &Paragraph<'_>, setting: &Setting<'_>) -> Layout {
    let text = paragraph.text;
    assert!(!paragraph.spans.is_empty(), "a paragraph needs a style");
    assert_eq!(
        paragraph.spans.iter().map(|s| s.len).sum::<usize>(),
        text.len(),
        "a paragraph's spans cover its text"
    );
    let bidi = Bidi::new(text, paragraph.direction);
    let graphemes = locale::graphemes(text);
    // The objects that stand for characters, in order, once each.
    let mut objects: Vec<Inline> = setting
        .objects
        .iter()
        .filter(|o| o.offset < text.len() && text.is_char_boundary(o.offset))
        .map(|o| Inline {
            width: o.width.max(0),
            height: o.height.max(0),
            ..*o
        })
        .collect();
    objects.sort_by_key(|o| o.offset);
    objects.dedup_by_key(|o| o.offset);
    let items = shape::itemize(fonts, text, paragraph.spans, &bidi, &graphemes, &objects);
    let missing = items.iter().any(|item| item.missing);

    // Each cluster's advance, at the offset where it starts, and their
    // running sums: the width of a stretch of text as shaped whole.
    let mut advance = vec![0; text.len() + 1];
    for glyph in items.iter().flat_map(|i| &i.glyphs) {
        advance[glyph.cluster] += glyph.advance;
    }
    let mut position = Vec::with_capacity(advance.len() + 1);
    position.push(0);
    for a in &advance {
        position.push(position.last().copied().unwrap_or(0) + a);
    }
    let mut spans = Vec::with_capacity(paragraph.spans.len());
    let mut at = 0;
    for span in paragraph.spans {
        spans.push((at..at + span.len, span.style.language.clone()));
        at += span.len;
    }
    let no_tabs = Tabs::default();
    let glue = Glue::new(text, &graphemes, |a, b| position[b] - position[a]);
    let cuts = cuts(text, &items);
    let shaping = Shaping {
        fonts,
        paragraph,
        cut_points: (0..cuts.len()).filter(|&o| cuts[o]).collect(),
        cuts,
        reshaped: RefCell::new(BTreeMap::new()),
        hyphens: items.iter().map(|_| OnceCell::new()).collect(),
        items,
        position,
        tabs: setting.tabs.unwrap_or(&no_tabs),
        tab_offsets: text
            .char_indices()
            .filter(|&(_, c)| c == '\t')
            .map(|(i, _)| i)
            .collect(),
        glue,
        objects,
    };

    let hard_breaks: Vec<usize> = text
        .char_indices()
        .filter(|&(_, c)| shape::is_hard_break(c))
        .map(|(i, c)| i + c.len_utf8())
        .collect();
    let mut unhyphenated = Vec::new();
    let empty = Hyphenation::new();
    let hyphenation = setting
        .hyphenate
        .then(|| setting.hyphenation.unwrap_or(&empty));
    let candidates: Vec<Candidate> =
        breaking::candidates(text, &graphemes, &spans, hyphenation, &mut unhyphenated);
    let measure = |range: Range<usize>, hyphen: bool| shaping.measure(range, hyphen);
    let justify = paragraph.align == Align::Justify;
    let widths = breaking::Widths {
        first: paragraph.width.saturating_sub(setting.indent),
        rest: paragraph.width,
    };
    let lines_at = if justify {
        breaking::knuth_plass(text, &graphemes, &candidates, widths, measure, |range| {
            shaping.stretch(range)
        })
    } else {
        breaking::first_fit(text, &graphemes, &candidates, widths, measure)
    };
    let count = lines_at.len();
    let mut lines = Vec::with_capacity(count);
    let mut top = 0;
    for (k, (range, kind)) in lines_at.into_iter().enumerate() {
        let stretch = justify && k + 1 < count && kind != Kind::Mandatory;
        let indent = if k == 0 { setting.indent } else { 0 };
        let mut line = build_line(
            &shaping, &bidi, &graphemes, range, kind, stretch, top, indent,
        );
        if let Some(per_mille) = setting.line_height {
            set_line_height(&mut line, paragraph, per_mille);
        }
        top += line.height;
        lines.push(line);
    }
    Layout {
        lines,
        width: paragraph.width,
        height: top,
        missing,
        unhyphenated,
        direction: bidi.direction(),
        text: text.to_string(),
        languages: paragraph
            .spans
            .iter()
            .map(|s| (s.style.language != Language::und()).then(|| s.style.language.to_string()))
            .collect(),
        spans,
        graphemes,
        hard_breaks,
    }
}

/// Where `range` of `text` ends, less the spaces and hard break at its end,
/// which hang.
pub(crate) fn trim(text: &str, range: Range<usize>) -> usize {
    let line = &text[range.clone()];
    range.start + line.trim_end_matches(hangs).len()
}

fn hangs(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

/// A paragraph shaped whole, and what shapes parts of it again.
struct Shaping<'a> {
    fonts: &'a Fonts,
    paragraph: &'a Paragraph<'a>,
    items: Vec<Item>,
    /// Where items can be cut without shaping again, by byte offset.
    cuts: Vec<bool>,
    /// The same, as a sorted list of those offsets.
    cut_points: Vec<usize>,
    /// Parts of items shaped again, by item and part.
    reshaped: RefCell<BTreeMap<(usize, usize, usize), Vec<Shaped>>>,
    /// The running sums of clusters' advances, by byte offset: the width of
    /// a stretch of text as shaped whole.
    position: Vec<i64>,
    /// Each item's hyphen, shaped once it's needed.
    hyphens: Vec<OnceCell<Option<Shaped>>>,
    tabs: &'a Tabs,
    /// Where the text's tabs are.
    tab_offsets: Vec<usize>,
    glue: Glue,
    /// The inline objects, by offset.
    objects: Vec<Inline>,
}

impl Shaping<'_> {
    /// The parts of items in `range`, in order: each item's index, and the
    /// part of its text in the range.
    fn parts(&self, range: Range<usize>) -> impl Iterator<Item = (usize, Range<usize>)> + '_ {
        let first = self.items.partition_point(|i| i.range.end <= range.start);
        self.items[first..]
            .iter()
            .take_while(move |i| i.range.start < range.end)
            .enumerate()
            .map(move |(k, i)| {
                (
                    first + k,
                    i.range.start.max(range.start)..i.range.end.min(range.end),
                )
            })
    }

    /// Whether a part of an item keeps the glyphs the item was shaped
    /// with: it's cut, if at all, where breaking is safe.
    fn keeps(&self, part: &Range<usize>) -> bool {
        self.cuts[part.start] && self.cuts[part.end]
    }

    /// The glyphs of `part` of item `index` at bidi `level`, left to right:
    /// the item's own where they hold, otherwise shaped again.
    fn glyphs(&self, index: usize, part: Range<usize>, level: u8) -> Vec<Shaped> {
        let item = &self.items[index];
        if level != item.level {
            return self.shape(index, part, level);
        }
        if self.keeps(&part) {
            return self.kept(index, part);
        }
        // Cut where breaking isn't safe: only the text between the cut and
        // the nearest safe place is shaped again, as HarfBuzz's flags
        // promise, and the item's own glyphs serve the rest.
        let (from, to) = self.safe(&part);
        if from >= to {
            return self.reshaped(index, part);
        }
        let start = self.reshaped(index, part.start..from);
        let middle = self.kept(index, from..to);
        let end = self.reshaped(index, to..part.end);
        let mut out = Vec::with_capacity(start.len() + middle.len() + end.len());
        if item.level % 2 == 1 {
            out.extend(end);
            out.extend(middle);
            out.extend(start);
        } else {
            out.extend(start);
            out.extend(middle);
            out.extend(end);
        }
        out
    }

    /// The item's own glyphs of `part`.
    fn kept(&self, index: usize, part: Range<usize>) -> Vec<Shaped> {
        let item = &self.items[index];
        if part == item.range {
            return item.glyphs.clone();
        }
        // HarfBuzz keeps clusters in order: rising along a left-to-right
        // item's glyphs, falling along a right-to-left one's.
        let glyphs = &item.glyphs;
        let (lo, hi) = if item.level % 2 == 1 {
            (
                glyphs.partition_point(|g| g.cluster >= part.end),
                glyphs.partition_point(|g| g.cluster >= part.start),
            )
        } else {
            (
                glyphs.partition_point(|g| g.cluster < part.start),
                glyphs.partition_point(|g| g.cluster < part.end),
            )
        };
        glyphs[lo..hi.max(lo)]
            .iter()
            .filter(|g| part.contains(&g.cluster))
            .copied()
            .collect()
    }

    /// Where `part` of an item is safe to cut nearest its edges: its
    /// start, or the first safe place after it; its end, or the last safe
    /// place before it.
    fn safe(&self, part: &Range<usize>) -> (usize, usize) {
        let cut = |o: usize| self.cuts[o];
        let from = if cut(part.start) {
            part.start
        } else {
            let k = self.cut_points.partition_point(|&o| o <= part.start);
            self.cut_points
                .get(k)
                .copied()
                .unwrap_or(part.end)
                .min(part.end)
        };
        let to = if cut(part.end) {
            part.end
        } else {
            let k = self.cut_points.partition_point(|&o| o < part.end);
            k.checked_sub(1)
                .map_or(part.start, |k| self.cut_points[k])
                .max(part.start)
        };
        (from, to)
    }

    /// `part` of item `index` shaped again on its own, at the item's level,
    /// once.
    fn reshaped(&self, index: usize, part: Range<usize>) -> Vec<Shaped> {
        if part.is_empty() {
            return Vec::new();
        }
        let key = (index, part.start, part.end);
        if let Some(glyphs) = self.reshaped.borrow().get(&key) {
            return glyphs.clone();
        }
        let glyphs = self.shape(index, part, self.items[index].level);
        self.reshaped.borrow_mut().insert(key, glyphs.clone());
        glyphs
    }

    fn shape(&self, index: usize, part: Range<usize>, level: u8) -> Vec<Shaped> {
        let item = &self.items[index];
        shape::shape(
            self.fonts,
            self.paragraph.text,
            part,
            &self.paragraph.spans[item.span],
            item.face,
            level,
            item.script,
            &self.objects,
        )
    }

    /// How wide `range` is as a line, less the spaces hanging at its end,
    /// with a hyphen at its end if `hyphen`: with its edges shaped again
    /// where breaking there changes them, and its tabs at their stops.
    fn measure(&self, range: Range<usize>, hyphen: bool) -> i64 {
        let hyphen = if hyphen {
            self.hyphen_before(range.end).map_or(0, |h| h.advance)
        } else {
            0
        };
        let end = trim(self.paragraph.text, range.clone());
        let tabbed = !self.tabs_in(range.start, end).is_empty();
        if !tabbed && self.cuts[range.start] && self.cuts[range.end] {
            // Every part keeps its glyphs.
            return self.position[end] - self.position[range.start] + hyphen;
        }
        if !tabbed {
            // Parts that keep their glyphs, by the running sums; the
            // others, by the glyphs shaped again at their edges.
            let mut width = 0;
            for (i, part) in self.parts(range.clone()) {
                let upto = part.end.min(end).max(part.start);
                if self.keeps(&part) {
                    width += self.position[upto] - self.position[part.start];
                } else {
                    width += self
                        .glyphs(i, part, self.items[i].level)
                        .iter()
                        .filter(|g| g.cluster < end)
                        .map(|g| g.advance)
                        .sum::<i64>();
                }
            }
            return width + hyphen;
        }
        // Each cluster's advance on the line.
        let mut advances = vec![0; range.len()];
        for (i, part) in self.parts(range.clone()) {
            for g in self.glyphs(i, part, self.items[i].level) {
                advances[g.cluster - range.start] += g.advance;
            }
        }
        let width: i64 = advances[..end - range.start].iter().sum();
        let tabs: i64 = self
            .tab_advances(range.start, end, |o| advances[o - range.start])
            .into_iter()
            .map(|(t, a)| a - advances[t - range.start])
            .sum();
        width + tabs + hyphen
    }

    /// The tabs from `start` up to `end`.
    fn tabs_in(&self, start: usize, end: usize) -> &[usize] {
        let lo = self.tab_offsets.partition_point(|&t| t < start);
        let hi = self.tab_offsets.partition_point(|&t| t < end);
        &self.tab_offsets[lo..hi.max(lo)]
    }

    /// How far each tab on a line from `start`, whose text ends at `end`,
    /// moves the pen, where the cluster at each offset is `advance` wide:
    /// each tab's offset and advance, in order. Tabs are measured from
    /// the line's start edge.
    fn tab_advances(
        &self,
        start: usize,
        end: usize,
        advance: impl Fn(usize) -> i64,
    ) -> Vec<(usize, i64)> {
        let text = self.paragraph.text;
        let tabbed = self.tabs_in(start, end);
        let width = |a: usize, b: usize| (a..b).map(&advance).sum::<i64>();
        let mut out = Vec::with_capacity(tabbed.len());
        let mut pen = 0;
        let mut from = start;
        for (k, &t) in tabbed.iter().enumerate() {
            pen += width(from, t);
            let next = tabbed.get(k + 1).copied().unwrap_or(end);
            let a = tabs::advance(self.tabs, pen, advance(t), |decimal| {
                let segment = &text[t + 1..next];
                After {
                    width: width(t + 1, next),
                    before_decimal: decimal
                        .and_then(|c| segment.find(c))
                        .map(|d| width(t + 1, t + 1 + d)),
                }
            });
            out.push((t, a));
            pen += a;
            from = t + 1;
        }
        out
    }

    /// How much the line `range` stretches, if it's justified: at the
    /// spaces and between Chinese and Japanese characters after its last
    /// tab.
    fn stretch(&self, range: Range<usize>) -> i64 {
        let end = trim(self.paragraph.text, range.clone());
        let start = self
            .tabs_in(range.start, end)
            .last()
            .map_or(range.start, |&t| t + 1);
        self.glue.stretch(start, end)
    }

    /// The hyphen shown when a line breaks at `offset`: in the face and
    /// style of the text before it.
    fn hyphen_before(&self, offset: usize) -> Option<Shaped> {
        let before = offset.checked_sub(1)?;
        let index = self.items.partition_point(|i| i.range.end <= before);
        let item = self.items.get(index)?;
        *self.hyphens[index].get_or_init(|| {
            let face = self.fonts.face(item.face);
            // The hyphen, U+2010, where the face has it; the hyphen-minus
            // otherwise.
            let hyphen = if face.has('\u{2010}') {
                "\u{2010}"
            } else {
                "-"
            };
            let glyphs = shape::shape(
                self.fonts,
                hyphen,
                0..hyphen.len(),
                &self.paragraph.spans[item.span],
                item.face,
                item.level,
                item.script,
                &[],
            );
            glyphs.first().map(|g| Shaped {
                advance: glyphs.iter().map(|g| g.advance).sum(),
                inserted: true,
                ..*g
            })
        })
    }
}

/// A piece of an item on a line, at one level.
struct Piece {
    range: Range<usize>,
    level: u8,
    item: usize,
    glyphs: Vec<Shaped>,
}

/// Makes `line` `per_mille` thousandths of its largest font size tall,
/// the difference from its fonts' height split evenly above and below,
/// but never shorter above its baseline than its objects.
fn set_line_height(line: &mut Line, paragraph: &Paragraph<'_>, per_mille: i64) {
    let size = line.runs.iter().map(|r| r.size).max().unwrap_or_else(|| {
        paragraph.spans[span_at(paragraph.spans, line.range.start)]
            .style
            .size
    });
    let per_mille = per_mille.clamp(100, 10_000);
    let height = (size.saturating_mul(per_mille) + 500).div_euclid(1000);
    let extra = height - line.height;
    let above = extra.div_euclid(2);
    line.ascent += above;
    line.descent += extra - above;
    // Still as tall as its objects, which stand on the baseline.
    for object in &line.objects {
        line.ascent = line.ascent.max(object.height);
    }
    line.height = line.ascent + line.descent;
    line.baseline = line.top + line.ascent;
}

/// Line `range`, at `top`, as wide as the paragraph less `indent`, which
/// it starts in from the paragraph's start edge.
#[allow(clippy::too_many_arguments, reason = "a line's place and its text")]
fn build_line(
    shaping: &Shaping<'_>,
    bidi: &Bidi<'_>,
    graphemes: &[usize],
    range: Range<usize>,
    kind: Kind,
    stretch: bool,
    top: i64,
    indent: i64,
) -> Line {
    let (fonts, paragraph) = (shaping.fonts, shaping.paragraph);
    let room = paragraph.width.saturating_sub(indent);
    let text = paragraph.text;
    let levels = bidi.line_levels(range.clone());
    let level_at = |offset: usize| levels[offset - range.start];

    // The items' parts on this line, shaped as `Shaping::measure` measured
    // them, then split where rule L1 changed levels: whitespace that now
    // runs the paragraph's way is shaped again in that direction.
    let mut pieces: Vec<Piece> = Vec::new();
    for (index, part) in shaping.parts(range.clone()) {
        let item = &shaping.items[index];
        let glyphs = shaping.glyphs(index, part.clone(), item.level);
        let mut from = part.start;
        for &g in between(graphemes, part.start, part.end) {
            if g == part.end || level_at(g) != level_at(from) {
                let (sub, level) = (from..g, level_at(from));
                let glyphs = if level != item.level {
                    shape::shape(
                        fonts,
                        text,
                        sub.clone(),
                        &paragraph.spans[item.span],
                        item.face,
                        level,
                        item.script,
                        &shaping.objects,
                    )
                } else if sub == part {
                    glyphs.clone()
                } else {
                    glyphs
                        .iter()
                        .filter(|g| sub.contains(&g.cluster))
                        .copied()
                        .collect()
                };
                pieces.push(Piece {
                    range: sub,
                    level,
                    item: index,
                    glyphs,
                });
                from = g;
            }
        }
    }

    // A hyphen where a word breaks: in place of a soft hyphen's invisible
    // glyph, or after the line's last character.
    let hyphen = kind.shows_hyphen()
        && shaping
            .hyphen_before(range.end)
            .is_some_and(|glyph| add_hyphen(&mut pieces, range.end, kind, glyph));

    let end = trim(text, range.clone());
    // Tabs at their stops.
    if !shaping.tabs_in(range.start, end).is_empty() {
        let mut advances = vec![0; range.len()];
        for g in pieces
            .iter()
            .flat_map(|p| &p.glyphs)
            .filter(|g| !g.inserted)
        {
            advances[g.cluster - range.start] += g.advance;
        }
        for (t, a) in shaping.tab_advances(range.start, end, |o| advances[o - range.start]) {
            set_advance(&mut pieces, t, a);
        }
    }

    // How wide: the text, and the spaces hanging at its end.
    let total: i64 = pieces
        .iter()
        .flat_map(|p| &p.glyphs)
        .map(|g| g.advance)
        .sum();
    let hang: i64 = pieces
        .iter()
        .flat_map(|p| &p.glyphs)
        .filter(|g| !g.inserted && g.cluster >= end)
        .map(|g| g.advance)
        .sum();
    let mut width = total - hang;
    if stretch && width < room {
        width += justify(&mut pieces, shaping, range.start, end, room - width);
    }
    // Left to right, aligned in the room the line has: from its indent
    // to the far edge.
    let order = locale::visual_order(&pieces.iter().map(|p| p.level).collect::<Vec<_>>());
    let rtl = bidi.paragraph_level() % 2 == 1;
    let free = room - width;
    let left = if rtl { 0 } else { indent };
    let text_x = left
        + match (paragraph.align, rtl) {
            (Align::Start | Align::Justify, false) | (Align::End, true) => 0,
            (Align::Start | Align::Justify, true) | (Align::End, false) => free,
            (Align::Center, _) => free.div_euclid(2),
        };
    // Hanging spaces are at the line's end: the right in a left-to-right
    // paragraph, the left in a right-to-left one.
    let mut pen = if rtl { text_x - hang } else { text_x };

    let mut runs = Vec::new();
    let mut cells = Vec::new();
    let mut objects = Vec::new();
    for &i in &order {
        let piece = &pieces[i];
        let item = &shaping.items[piece.item];
        let x = pen;
        let mut glyphs = Vec::with_capacity(piece.glyphs.len());
        for g in &piece.glyphs {
            if g.object {
                // No font draws it: its caller does, here.
                let height = shape::object_at(&shaping.objects, g.cluster).map_or(0, |o| o.height);
                objects.push(Placed {
                    offset: g.cluster,
                    x: pen,
                    width: g.advance,
                    height,
                });
            } else if !g.hidden {
                glyphs.push(Glyph {
                    id: g.id,
                    x: pen + g.x_offset,
                    y: -g.y_offset,
                    advance: g.advance,
                    cluster: g.cluster,
                    hyphen: g.inserted,
                });
            }
            pen += g.advance;
        }
        cells.extend(piece_cells(text, graphemes, piece, x));
        let style = &paragraph.spans[item.span].style;
        runs.push(GlyphRun {
            face: item.face,
            size: style.size,
            span: item.span,
            range: piece.range.clone(),
            level: piece.level,
            oblique: style.italic && !fonts.face(item.face).italic(),
            x,
            width: pen - x,
            glyphs,
        });
    }

    // As tall as its fonts: each face's ascent and descent, with half its
    // line gap above and half below.
    let mut faces: Vec<(FaceId, i64)> = runs.iter().map(|r| (r.face, r.size)).collect();
    if faces.is_empty() {
        let span = span_at(paragraph.spans, range.start);
        let style = &paragraph.spans[span].style;
        let face = fonts.candidates(&style.family, style.weight, style.italic)[0];
        faces.push((face, style.size));
    }
    let (mut ascent, mut descent) = (0, 0);
    for (face, size) in faces {
        let face = fonts.face(face);
        let upem = face.units_per_em;
        let gap = scale(face.line_gap, size, upem);
        ascent = ascent.max(scale(face.ascent, size, upem) + gap / 2);
        descent = descent.max(scale(face.descent, size, upem) + gap - gap / 2);
    }
    // And as tall as its objects, which stand on the baseline.
    for object in &objects {
        ascent = ascent.max(object.height);
    }
    Line {
        range,
        top,
        baseline: top + ascent,
        ascent,
        descent,
        height: ascent + descent,
        x: text_x,
        width,
        hyphen,
        runs,
        objects,
        cells,
    }
}

/// Puts `hyphen` at the end of the line ending at `end`: in place of the
/// glyphs of the soft hyphen just before `end`, or after the line's last
/// character, at the end of its piece in the text's order. Whether there
/// was a piece to put it in.
fn add_hyphen(pieces: &mut [Piece], end: usize, kind: Kind, hyphen: Shaped) -> bool {
    let soft = kind == Kind::Soft;
    let cluster = if soft { end - '\u{ad}'.len_utf8() } else { end };
    let last = end - 1;
    let Some(piece) = pieces
        .iter_mut()
        .find(|p| p.range.start <= last && last < p.range.end)
    else {
        return false;
    };
    if soft {
        piece.glyphs.retain(|g| g.cluster != cluster);
    }
    let glyph = Shaped {
        cluster,
        inserted: !soft,
        ..hyphen
    };
    if piece.level % 2 == 1 {
        piece.glyphs.insert(0, glyph);
    } else {
        piece.glyphs.push(glyph);
    }
    true
}

/// Makes the cluster at `offset` `advance` wide: its first glyph takes it
/// all.
fn set_advance(pieces: &mut [Piece], offset: usize, advance: i64) {
    let mut first = true;
    for g in pieces
        .iter_mut()
        .flat_map(|p| &mut p.glyphs)
        .filter(|g| g.cluster == offset && !g.inserted)
    {
        g.advance = if first { advance } else { 0 };
        first = false;
    }
}

/// Stretches the line from `start`, whose text ends at `end`, by `slack`
/// at its gaps after its last tab, each in proportion to how much it
/// stretches: the space goes on a space's glyph, or on the glyph of the
/// character before a gap between Chinese or Japanese characters. What
/// doesn't divide evenly goes a unit at a time to the first gaps. Returns
/// how much it stretched.
fn justify(
    pieces: &mut [Piece],
    shaping: &Shaping<'_>,
    start: usize,
    end: usize,
    slack: i64,
) -> i64 {
    let from = shaping.tabs_in(start, end).last().map_or(start, |&t| t + 1);
    let mut gaps: Vec<(usize, i64)> = shaping
        .glue
        .on_line(from, end)
        .iter()
        .map(|g| (g.grow, g.stretch.max(0)))
        .collect();
    gaps.sort_by_key(|&(grow, _)| grow);
    let total: i64 = gaps.iter().map(|&(_, s)| s).sum();
    if total <= 0 {
        return 0;
    }
    let mut extra: Vec<(usize, i64)> = gaps
        .iter()
        .map(|&(grow, s)| {
            let share = i128::from(slack) * i128::from(s) / i128::from(total);
            (grow, share as i64)
        })
        .collect();
    let mut left = slack - extra.iter().map(|&(_, e)| e).sum::<i64>();
    for (_, e) in &mut extra {
        if left <= 0 {
            break;
        }
        *e += 1;
        left -= 1;
    }
    let mut stretched = 0;
    for (grow, e) in extra {
        let Some(piece) = pieces.iter_mut().find(|p| p.range.contains(&grow)) else {
            continue;
        };
        // The cluster's last glyph in the text's order takes it.
        let rtl = piece.level % 2 == 1;
        let mut found = piece
            .glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| g.cluster == grow && !g.inserted)
            .map(|(k, _)| k);
        let k = if rtl { found.next() } else { found.next_back() };
        if let Some(k) = k {
            piece.glyphs[k].advance += e;
            stretched += e;
        }
    }
    stretched
}

/// Where the paragraph's items can be cut without shaping again, by byte
/// offset: at their edges, and where a cluster starts whose glyphs are all
/// safe to break at.
fn cuts(text: &str, items: &[Item]) -> Vec<bool> {
    let mut seen = vec![false; text.len() + 1];
    let mut safe = vec![true; text.len() + 1];
    for g in items.iter().flat_map(|i| &i.glyphs) {
        seen[g.cluster] = true;
        safe[g.cluster] &= !g.unsafe_to_break;
    }
    let mut cuts: Vec<bool> = seen.iter().zip(&safe).map(|(&s, &f)| s && f).collect();
    cuts[0] = true;
    cuts[text.len()] = true;
    for item in items {
        cuts[item.range.start] = true;
        cuts[item.range.end] = true;
    }
    cuts
}

/// The index of the span holding `offset`; at the end, the last span.
fn span_at(spans: &[Span], offset: usize) -> usize {
    let mut end = 0;
    for (i, span) in spans.iter().enumerate() {
        end += span.len;
        if offset < end {
            return i;
        }
    }
    spans.len() - 1
}

/// The boundaries in `graphemes`, which are sorted, after `start` and up to
/// `end`.
pub(crate) fn between(graphemes: &[usize], start: usize, end: usize) -> &[usize] {
    let from = graphemes.partition_point(|&g| g <= start);
    let to = graphemes.partition_point(|&g| g <= end);
    &graphemes[from..to.max(from)]
}

/// `piece`'s graphemes, left to right, with the piece's left edge at `x`.
/// A cluster of several graphemes, such as a ligature, is shared evenly
/// between them. Hard breaks have no cell, and a hyphen layout added is
/// part of the cell of the grapheme before it, so the caret at the line's
/// end stands after it.
fn piece_cells(text: &str, graphemes: &[usize], piece: &Piece, x: i64) -> Vec<Cell> {
    let rtl = piece.level % 2 == 1;
    // Clusters left to right, with their extents.
    let mut clusters: Vec<(usize, i64, i64)> = Vec::new();
    let mut pen = x;
    for g in &piece.glyphs {
        match clusters.last_mut() {
            // A hyphen ending a left-to-right piece.
            Some((_, _, right)) if g.inserted => *right += g.advance,
            // One starting a right-to-left piece: the first cluster takes it.
            None if g.inserted => {}
            Some((cluster, _, right)) if *cluster == g.cluster => *right += g.advance,
            Some(_) => clusters.push((g.cluster, pen, pen + g.advance)),
            None => clusters.push((g.cluster, x, pen + g.advance)),
        }
        pen += g.advance;
    }
    let mut starts: Vec<usize> = clusters.iter().map(|c| c.0).collect();
    starts.sort_unstable();
    let mut cells = Vec::new();
    for (cluster, left, right) in clusters {
        let end = starts
            .get(starts.partition_point(|&s| s <= cluster))
            .copied()
            .unwrap_or(piece.range.end);
        let mut bounds = vec![cluster];
        bounds.extend(between(graphemes, cluster, end));
        if bounds.last() != Some(&end) {
            bounds.push(end);
        }
        let n = (bounds.len() - 1) as i64;
        let width = right - left;
        let mut parts: Vec<Cell> = bounds
            .windows(2)
            .enumerate()
            .map(|(k, pair)| {
                let k = k as i64;
                let (from, to) = if rtl { (n - 1 - k, n - k) } else { (k, k + 1) };
                Cell {
                    range: pair[0]..pair[1],
                    left: left + width * from / n,
                    right: left + width * to / n,
                    rtl,
                }
            })
            .collect();
        if rtl {
            parts.reverse();
        }
        cells.extend(parts.into_iter().filter(|c| {
            !text[c.range.clone()]
                .chars()
                .next()
                .is_some_and(shape::is_hard_break)
        }));
    }
    cells
}
