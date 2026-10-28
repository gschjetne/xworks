//! Where a paragraph's lines break: the places they may (UAX #14, soft
//! hyphens and hyphenation), and two ways of choosing among them. Ragged
//! text breaks first fit, so typing never moves a line above the caret;
//! justified text breaks by Knuth and Plass's optimal algorithm, which
//! weighs every line of the paragraph together.
//!
//! Both measure a line by the same function, and the optimal algorithm
//! scores lines with TeX's integer badness and demerits, so lines break
//! the same on every platform.

use std::ops::Range;

use locale::{Language, Script};

use crate::hyphen::Hyphenation;
use crate::layout::between;

/// A place a line may end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    /// The offset the next line would start at.
    pub offset: usize,
    pub kind: Kind,
}

/// What kind of place a line ends at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Where UAX #14 allows, such as after a space.
    Normal,
    /// After a hard break; the line must end.
    Mandatory,
    /// After a hyphen or dash in the text.
    Explicit,
    /// After a soft hyphen, which then shows as a hyphen.
    Soft,
    /// Inside a word, where its language's patterns allow; a hyphen is
    /// added.
    Hyphen,
    /// Between graphemes of a word too wide for a line on its own.
    Emergency,
}

impl Kind {
    /// Whether a line ending here shows a hyphen that isn't drawn
    /// otherwise.
    pub(crate) fn shows_hyphen(self) -> bool {
        matches!(self, Kind::Soft | Kind::Hyphen)
    }

    /// Whether a line ending here ends in a hyphen, for the demerits of
    /// hyphens on consecutive lines.
    fn flagged(self) -> bool {
        matches!(self, Kind::Soft | Kind::Hyphen | Kind::Explicit)
    }

    /// TeX's penalty for breaking here: \hyphenpenalty and
    /// \exhyphenpenalty for hyphens.
    fn penalty(self) -> i64 {
        match self {
            Kind::Normal | Kind::Mandatory => 0,
            Kind::Explicit | Kind::Soft | Kind::Hyphen => 50,
            Kind::Emergency => 0,
        }
    }
}

/// Where `text`'s lines may break, in order: UAX #14's breaks by the
/// text's `languages`, and, with `hyphenation`, the places its words may
/// be hyphenated. Languages with words to hyphenate that `hyphenation`
/// lacks are added to `wanted`.
pub(crate) fn candidates(
    text: &str,
    graphemes: &[usize],
    languages: &[(Range<usize>, Language)],
    hyphenation: Option<&Hyphenation>,
    wanted: &mut Vec<Language>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut previous = 0;
    for b in locale::line_breaks(text, languages) {
        // UAX #14 lets a line break after a space with a combining mark
        // after it, inside what a reader sees as one character: a line
        // never ends there.
        if !b.mandatory && graphemes.binary_search(&b.offset).is_err() {
            continue;
        }
        if let Some(hyphenation) = hyphenation {
            for offset in hyphen_points(
                text,
                previous..b.offset,
                graphemes,
                languages,
                hyphenation,
                wanted,
            ) {
                out.push(Candidate {
                    offset,
                    kind: Kind::Hyphen,
                });
            }
        }
        let before = text[..b.offset].chars().next_back();
        let kind = if b.mandatory {
            Kind::Mandatory
        } else if before == Some('\u{ad}') {
            Kind::Soft
        } else if matches!(before, Some('-' | '\u{2010}' | '\u{2013}' | '\u{2014}')) {
            Kind::Explicit
        } else {
            Kind::Normal
        };
        out.push(Candidate {
            offset: b.offset,
            kind,
        });
        previous = b.offset;
    }
    out
}

/// Where the word in `chunk` of `text`, a stretch between two places a
/// line may break, may be hyphenated: within its letters, less any
/// punctuation around them, at grapheme boundaries.
fn hyphen_points(
    text: &str,
    chunk: Range<usize>,
    graphemes: &[usize],
    languages: &[(Range<usize>, Language)],
    hyphenation: &Hyphenation,
    wanted: &mut Vec<Language>,
) -> Vec<usize> {
    let s = &text[chunk.clone()];
    let core = s.trim_end_matches(|c: char| !is_letter(c));
    let lead = core.len() - core.trim_start_matches(|c: char| !is_letter(c)).len();
    let core = &core[lead..];
    let start = chunk.start + lead;
    if core.chars().count() < 4 || !core.chars().all(|c| is_letter(c) || is_apostrophe(c)) {
        return Vec::new();
    }
    let Some(language) = languages
        .iter()
        .find(|(range, _)| range.contains(&start))
        .map(|(_, language)| language)
    else {
        return Vec::new();
    };
    if *language == Language::und() {
        return Vec::new();
    }
    let Some(hyphenator) = hyphenation.get(language) else {
        if !wanted.contains(language) {
            wanted.push(language.clone());
        }
        return Vec::new();
    };
    hyphenator
        .hyphenate(core)
        .into_iter()
        .map(|b| start + b)
        .filter(|b| graphemes.binary_search(b).is_ok())
        .collect()
}

/// A letter of a word to hyphenate: alphabetic, or a combining mark.
fn is_letter(c: char) -> bool {
    c.is_alphabetic()
        || matches!(c, '\u{300}'..='\u{36f}' | '\u{1dc0}'..='\u{1dff}' | '\u{20d0}'..='\u{20ff}' | '\u{fe20}'..='\u{fe2f}')
}

/// An apostrophe inside a word, as in French "l'ouverture", which
/// patterns may hold.
fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}')
}

/// The stretchable gaps of a paragraph, where justification adds space:
/// its spaces, and between the characters of Chinese and Japanese.
#[derive(Clone, Debug, Default)]
pub(crate) struct Glue {
    /// Sorted by `need`.
    gaps: Vec<Gap>,
    /// Running sums of their stretch: `prefix[k]` is the first k's.
    prefix: Vec<i64>,
}

/// A stretchable gap.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gap {
    /// The cluster whose advance grows: a space, or the character before a
    /// gap between Chinese or Japanese characters.
    pub grow: usize,
    /// Where text must go on after it on the line for it to stretch: the
    /// space itself, which mustn't hang at the line's end, or the
    /// character after the gap.
    pub need: usize,
    /// How much it stretches, relative to the others: half a space's
    /// width, as in TeX, and a quarter of a character's between Chinese
    /// and Japanese characters.
    pub stretch: i64,
}

impl Glue {
    /// The gaps of `text`, whose clusters at each offset are `advance`
    /// wide.
    pub(crate) fn new(
        text: &str,
        graphemes: &[usize],
        advance: impl Fn(usize, usize) -> i64,
    ) -> Glue {
        let mut gaps = Vec::new();
        for pair in graphemes.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let c = text[start..end].chars().next().unwrap_or(' ');
            if is_word_space(c) {
                gaps.push(Gap {
                    grow: start,
                    need: start,
                    stretch: advance(start, end) / 2,
                });
                continue;
            }
            let next = text[end..].chars().next();
            if let Some(n) = next
                && !is_word_space(n)
                && !crate::shape::is_hard_break(n)
                && n != '\t'
                && c != '\t'
                && (is_cjk(c) || is_cjk(n))
            {
                gaps.push(Gap {
                    grow: start,
                    need: end,
                    stretch: advance(start, end) / 4,
                });
            }
        }
        gaps.sort_by_key(|g| g.need);
        let mut prefix = Vec::with_capacity(gaps.len() + 1);
        prefix.push(0);
        for g in &gaps {
            prefix.push(prefix.last().copied().unwrap_or(0) + g.stretch.max(0));
        }
        Glue { gaps, prefix }
    }

    /// The gaps that stretch on a line from `start` whose text, less what
    /// hangs at its end, ends at `end`.
    pub(crate) fn on_line(&self, start: usize, end: usize) -> &[Gap] {
        let lo = self.gaps.partition_point(|g| g.need < start);
        let hi = self.gaps.partition_point(|g| g.need < end);
        let lo = lo
            + self.gaps[lo..hi]
                .iter()
                .take_while(|g| g.grow < start)
                .count();
        &self.gaps[lo..hi.max(lo)]
    }

    /// How much a line from `start` to `end`, as for [`Glue::on_line`],
    /// stretches.
    pub(crate) fn stretch(&self, start: usize, end: usize) -> i64 {
        let lo = self.gaps.partition_point(|g| g.need < start);
        let hi = self.gaps.partition_point(|g| g.need < end);
        let skipped: i64 = self.gaps[lo..hi]
            .iter()
            .take_while(|g| g.grow < start)
            .map(|g| g.stretch.max(0))
            .sum();
        self.prefix[hi.max(lo)] - self.prefix[lo] - skipped
    }
}

/// A space between words, which justification stretches.
fn is_word_space(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}')
}

/// A Chinese or Japanese character, between which justification adds
/// space: ideographs, kana, and their punctuation.
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{303f}' | '\u{30fc}' | '\u{ff00}'..='\u{ffef}')
        || Script::of(c).is_some_and(|s| matches!(s.as_str(), "Hani" | "Hira" | "Kana" | "Bopo"))
}

/// Of the line it fills, how much a ragged line must leave empty before
/// a word is hyphenated: a tenth.
const HYPHENATION_ZONE: i64 = 10;

/// How wide a paragraph's lines are: its first, which an indent may make
/// narrower or wider, and the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Widths {
    /// The first line's.
    pub first: i64,
    /// Every other line's.
    pub rest: i64,
}

impl Widths {
    /// The width of the line starting at `start`.
    fn at(self, start: usize) -> i64 {
        if start == 0 { self.first } else { self.rest }
    }

    /// The same width for every line.
    #[cfg(test)]
    fn all(width: i64) -> Widths {
        Widths {
            first: width,
            rest: width,
        }
    }
}

/// Lines first fit: each ends at the last place it may that fits
/// `widths`, by `measure` (of a line, and whether it ends in a hyphen),
/// and a word is hyphenated only where the line would otherwise leave
/// more than a tenth of it empty. A word too wide for a line on its own
/// is broken between graphemes.
pub(crate) fn first_fit(
    text: &str,
    graphemes: &[usize],
    candidates: &[Candidate],
    widths: Widths,
    measure: impl Fn(Range<usize>, bool) -> i64,
) -> Vec<(Range<usize>, Kind)> {
    let fits = |range: Range<usize>, kind: Kind| {
        let width = widths.at(range.start);
        measure(range, kind.shows_hyphen()) <= width
    };
    let mut lines = Vec::new();
    let (mut start, mut next) = (0, 0);
    loop {
        let width = widths.at(start);
        // The last place that fits, and the last that isn't a hyphenation
        // point.
        let mut plain: Option<usize> = None;
        let mut hyphen: Option<usize> = None;
        let mut overflow = text.len();
        let mut i = next;
        while let Some(c) = candidates.get(i) {
            if c.offset <= start {
                i += 1;
            } else if fits(start..c.offset, c.kind) {
                if c.kind == Kind::Hyphen {
                    hyphen = Some(i);
                } else {
                    plain = Some(i);
                }
                i += 1;
                if c.kind == Kind::Mandatory {
                    break;
                }
            } else {
                overflow = c.offset;
                break;
            }
        }
        let chosen = match (plain, hyphen) {
            (Some(p), Some(h)) if h > p => {
                let c = candidates[p];
                let empty = width - measure(start..c.offset, c.kind.shows_hyphen());
                Some(if empty * HYPHENATION_ZONE > width {
                    h
                } else {
                    p
                })
            }
            (Some(p), _) => Some(p),
            (None, h) => h,
        };
        let (end, kind) = match chosen {
            Some(k) => {
                next = k + 1;
                (candidates[k].offset, candidates[k].kind)
            }
            None => {
                // Graphemes, as many as fit, and at least one.
                let first = graphemes
                    .get(graphemes.partition_point(|&g| g <= start))
                    .copied()
                    .unwrap_or(text.len());
                let mut end = first;
                for &g in between(graphemes, first, overflow) {
                    if !fits(start..g, Kind::Emergency) {
                        break;
                    }
                    end = g;
                }
                (end, Kind::Emergency)
            }
        };
        lines.push((start..end, kind));
        start = end;
        if start >= text.len() {
            // A hard break at the very end leaves an empty last line.
            if kind == Kind::Mandatory {
                lines.push((start..start, Kind::Normal));
            }
            break;
        }
    }
    lines
}

/// TeX's \linepenalty: what each line costs, so fewer lines are better.
const LINE_PENALTY: i64 = 10;
/// TeX's \doublehyphendemerits: two hyphenated lines in a row.
const DOUBLE_HYPHEN_DEMERITS: i64 = 10_000;
/// TeX's \finalhyphendemerits: a hyphen ending the last line but one.
const FINAL_HYPHEN_DEMERITS: i64 = 5_000;
/// TeX's \adjdemerits: neighbouring lines of very different tightness.
const ADJ_DEMERITS: i64 = 10_000;
/// TeX's \tolerance: the worst badness a line may have, unless no
/// paragraph is possible without worse.
const TOLERANCE: i64 = 200;
/// Infinitely bad, to TeX.
const INF_BAD: i64 = 10_000;
/// The worst badness the last pass scores a line, past TeX's cap.
const WORST: i64 = 1_000_000;
/// Of a line's width, the emergency stretch the last pass scores lines
/// with: a sixteenth, about TeX's usual two or three ems in a column of
/// text. More would make loose lines so cheap that hyphens never pay.
const EMERGENCY: i64 = 16;

/// How bad a line is that must stretch by `slack` with glue that
/// stretches by `stretch`: 100 times the cube of their ratio, up to
/// 10 000, as TeX computes it in integers. The ratio is scaled by 297,
/// whose cube is about 100 · 2¹⁸, so the cube stays small.
#[cfg(test)]
fn badness(slack: i64, stretch: i64) -> i64 {
    badness_up_to(slack, stretch, INF_BAD)
}

/// [`badness`], but up to `most` rather than TeX's 10 000, so that lines
/// too loose for TeX are still ranked: one that stretches twenty times
/// its glue is a million.
fn badness_up_to(slack: i64, stretch: i64, most: i64) -> i64 {
    if slack <= 0 {
        return 0;
    }
    if stretch <= 0 {
        return most;
    }
    let ratio = match slack.checked_mul(297) {
        Some(scaled) => scaled / stretch,
        None => slack / (stretch / 297).max(1),
    };
    // 6237 is 297 times 21, whose cube times 100 is near a million; its
    // cube fits easily.
    if ratio > 6237 {
        return most;
    }
    ((ratio * ratio * ratio + 0x20000) / 0x40000).min(most)
}

/// How tight a line is, by its badness, for TeX's demerits of
/// neighbours that differ: very loose, loose or decent. Lines here never
/// shrink, so none is tight.
fn fitness(badness: i64) -> usize {
    if badness > 99 {
        0
    } else if badness > 12 {
        1
    } else {
        2
    }
}

/// The best way to reach a place: its demerits so far, and where the
/// line before it started, as a place and its state.
#[derive(Clone, Copy, Debug)]
struct Best {
    total: i64,
    from: Option<(usize, usize)>,
}

/// States per place: three fitnesses, each hyphenated or not.
const STATES: usize = 6;

/// Lines broken by Knuth and Plass's algorithm: the breaks that give the
/// paragraph the least demerits, each line within `widths` by `measure`,
/// stretching by the `glue` on it. The last line, and lines ending at a
/// hard break, don't stretch.
///
/// A first pass allows lines up to TeX's tolerance. If no paragraph fits
/// that, as in narrow columns, a second allows any line that fits, scoring
/// each as though it had emergency stretch, as TeX's \emergencystretch
/// does, and past TeX's cap on badness: so a line that must stretch far
/// is still worse than one that stretches less, rather than both
/// infinitely bad. Words too wide for a line break between graphemes.
pub(crate) fn knuth_plass(
    text: &str,
    graphemes: &[usize],
    candidates: &[Candidate],
    widths: Widths,
    measure: impl Fn(Range<usize>, bool) -> i64,
    stretch: impl Fn(Range<usize>) -> i64,
) -> Vec<(Range<usize>, Kind)> {
    // Places inside words too wide for any line, so every line can fit.
    let width = widths.first.min(widths.rest);
    let mut all: Vec<Candidate> = Vec::with_capacity(candidates.len());
    let mut previous = 0;
    for &c in candidates {
        if c.offset > previous && measure(previous..c.offset, c.kind.shows_hyphen()) > width {
            for &g in between(graphemes, previous, c.offset) {
                if g < c.offset {
                    all.push(Candidate {
                        offset: g,
                        kind: Kind::Emergency,
                    });
                }
            }
        }
        all.push(c);
        previous = c.offset;
    }
    all.sort_by_key(|c| c.offset);
    all.dedup_by_key(|c| c.offset);

    let path = pass(&all, widths, TOLERANCE, 0, &measure, &stretch)
        .or_else(|| {
            let emergency = widths.rest / EMERGENCY;
            pass(&all, widths, WORST, emergency, &measure, &stretch)
        })
        .unwrap_or_default();
    let mut lines = Vec::with_capacity(path.len() + 1);
    let mut start = 0;
    for k in path {
        let c = all[k];
        lines.push((start..c.offset, c.kind));
        start = c.offset;
    }
    if lines.is_empty() {
        lines.push((0..text.len(), Kind::Normal));
    } else if lines
        .last()
        .is_some_and(|(_, kind)| *kind == Kind::Mandatory)
        && start >= text.len()
    {
        lines.push((start..start, Kind::Normal));
    }
    lines
}

/// One pass of the algorithm, allowing lines up to `tolerance`, scored as
/// though each stretched `emergency` more: the candidates the paragraph's
/// lines end at, by index, or `None` if no paragraph is possible.
fn pass(
    candidates: &[Candidate],
    widths: Widths,
    tolerance: i64,
    emergency: i64,
    measure: &impl Fn(Range<usize>, bool) -> i64,
    stretch: &impl Fn(Range<usize>) -> i64,
) -> Option<Vec<usize>> {
    let n = candidates.len();
    // Place 0 is the paragraph's start; place j > 0 is candidate j - 1.
    let offset = |place: usize| place.checked_sub(1).map_or(0, |k| candidates[k].offset);
    let mut best: Vec<[Option<Best>; STATES]> = vec![[None; STATES]; n + 1];
    best[0][2 * 2] = Some(Best {
        total: 0,
        from: None,
    });
    for j in 1..=n {
        let c = candidates[j - 1];
        let end = c.offset;
        let last = j == n || c.kind == Kind::Mandatory;
        let mut i = j;
        while i > 0 {
            i -= 1;
            // A line never runs past a hard break.
            if i + 1 < j && candidates[i].kind == Kind::Mandatory {
                break;
            }
            let start = offset(i);
            let width = widths.at(start);
            let natural = measure(start..end, c.kind.shows_hyphen());
            let single = i + 1 == j;
            if natural > width && !single {
                // A first line hanging past the edge is wider than the
                // rest: one from the paragraph's start may still fit.
                if i > 0
                    && widths.first > widths.rest
                    && !candidates[..i].iter().any(|c| c.kind == Kind::Mandatory)
                {
                    i = 1;
                    continue;
                }
                break;
            }
            if best[i].iter().all(Option::is_none) {
                continue;
            }
            // TeX's badness, or past its cap in the last pass.
            let most = tolerance.max(INF_BAD);
            let bad = if natural > width {
                // One piece too wide for any line: allowed when nothing
                // else is.
                most
            } else if last {
                0
            } else {
                badness_up_to(width - natural, stretch(start..end) + emergency, most)
            };
            if bad > tolerance || (bad >= INF_BAD && tolerance < INF_BAD) {
                continue;
            }
            let class = if last { 2 } else { fitness(bad) };
            let flagged = c.kind.flagged();
            let penalty = if c.kind == Kind::Mandatory {
                0
            } else {
                c.kind.penalty() * c.kind.penalty()
            };
            let demerits = (LINE_PENALTY + bad).pow(2) + penalty;
            let to = class * 2 + usize::from(flagged);
            let froms = best[i];
            for (state, from) in froms.iter().enumerate() {
                let Some(from) = from else {
                    continue;
                };
                let (from_class, from_flagged) = (state / 2, state % 2 == 1);
                let mut total = from.total + demerits;
                if from_class.abs_diff(class) > 1 {
                    total += ADJ_DEMERITS;
                }
                if from_flagged && flagged {
                    total += DOUBLE_HYPHEN_DEMERITS;
                }
                if j == n && from_flagged {
                    total += FINAL_HYPHEN_DEMERITS;
                }
                if best[j][to].is_none_or(|b| total < b.total) {
                    best[j][to] = Some(Best {
                        total,
                        from: Some((i, state)),
                    });
                }
            }
        }
    }
    // The best way to the end, traced back.
    let (mut state, _) = best[n]
        .iter()
        .enumerate()
        .filter_map(|(s, b)| b.map(|b| (s, b.total)))
        .min_by_key(|&(_, total)| total)?;
    let mut place = n;
    let mut path = Vec::new();
    while place > 0 {
        path.push(place - 1);
        let (from, from_state) = best[place][state]?.from?;
        place = from;
        state = from_state;
    }
    path.reverse();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badness_is_texs() {
        assert_eq!(badness(0, 100), 0);
        assert_eq!(badness(100, 0), INF_BAD);
        // Stretching by the glue's full stretch is badness 100.
        assert_eq!(badness(100, 100), 100);
        assert_eq!(badness(50, 100), 12);
        assert_eq!(badness(1000, 100), INF_BAD);
        assert_eq!(badness(i64::MAX, 1), INF_BAD);
        assert_eq!(badness(i64::MAX, i64::MAX), 100);
        // Past TeX's cap, for the last pass: ten times the stretch is
        // about 100 000, and far more is the worst.
        assert_eq!(badness_up_to(1000, 100, WORST), 99_938);
        assert_eq!(badness_up_to(100_000, 1, WORST), WORST);
        assert_eq!(badness_up_to(100, 100, WORST), 100);
    }

    /// Words of these widths, a space of 10 between them that stretches by
    /// 5, as offsets: each word one byte per unit of width, each space
    /// one byte.
    fn words(widths: &[usize]) -> (String, Vec<Candidate>) {
        let mut text = String::new();
        let mut candidates = Vec::new();
        for (i, &w) in widths.iter().enumerate() {
            text.push_str(&"x".repeat(w));
            if i + 1 < widths.len() {
                text.push(' ');
            }
            candidates.push(Candidate {
                offset: text.len(),
                kind: Kind::Normal,
            });
        }
        (text, candidates)
    }

    /// The demerits of breaking at `ends`, by the rules `pass` scores
    /// with, for lines of words alone.
    fn demerits(text: &str, ends: &[usize], width: i64) -> Option<i64> {
        let mut total = 0;
        let mut start = 0;
        let mut previous = 2;
        for (k, &end) in ends.iter().enumerate() {
            let line = text[start..end].trim_end();
            let natural = line.len() as i64 * 10;
            if natural > width {
                return None;
            }
            let last = k + 1 == ends.len();
            let bad = if last {
                0
            } else {
                badness(width - natural, line.matches(' ').count() as i64 * 5)
            };
            if bad > TOLERANCE {
                return None;
            }
            let class = if last { 2 } else { fitness(bad) };
            total += (LINE_PENALTY + bad).pow(2);
            if class.abs_diff(previous) > 1 {
                total += ADJ_DEMERITS;
            }
            previous = class;
            start = end;
        }
        Some(total)
    }

    #[test]
    fn optimal_breaking_finds_the_least_demerits() {
        // Paragraphs of words of pseudo-random widths, checked against
        // every way of breaking them.
        let mut seed: u64 = 7;
        for _ in 0..40 {
            let widths: Vec<usize> = (0..10)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    1 + (seed >> 60) as usize
                })
                .collect();
            let (text, candidates) = words(&widths);
            let graphemes: Vec<usize> = (0..=text.len()).collect();
            let measure = |r: Range<usize>, _: bool| text[r].trim_end().len() as i64 * 10;
            let spaces = |r: Range<usize>| text[r].trim_end().matches(' ').count() as i64 * 5;
            let width = 200;
            let n = candidates.len();
            let mut least: Option<i64> = None;
            for mask in 0..1u32 << (n - 1) {
                let mut ends: Vec<usize> = (0..n - 1)
                    .filter(|k| mask & (1 << k) != 0)
                    .map(|k| candidates[k].offset)
                    .collect();
                ends.push(text.len());
                if let Some(d) = demerits(&text, &ends, width) {
                    least = Some(least.map_or(d, |l: i64| l.min(d)));
                }
            }
            let lines = knuth_plass(
                &text,
                &graphemes,
                &candidates,
                Widths::all(width),
                measure,
                spaces,
            );
            let ends: Vec<usize> = lines.iter().map(|(r, _)| r.end).collect();
            if let Some(least) = least {
                assert_eq!(demerits(&text, &ends, width), Some(least), "{text:?}");
            }
        }
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_between_graphemes() {
        let (text, candidates) = words(&[5, 50, 5]);
        let graphemes: Vec<usize> = (0..=text.len()).collect();
        let measure = |r: Range<usize>, _: bool| text[r].trim_end().len() as i64 * 10;
        let spaces = |r: Range<usize>| text[r].trim_end().matches(' ').count() as i64 * 5;
        let lines = knuth_plass(
            &text,
            &graphemes,
            &candidates,
            Widths::all(200),
            measure,
            spaces,
        );
        let mut at = 0;
        for (range, _) in &lines {
            assert_eq!(range.start, at);
            assert!(measure(range.clone(), false) <= 200, "{lines:?}");
            at = range.end;
        }
        assert_eq!(at, text.len());
    }
}
