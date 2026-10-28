//! locale's answers on text drawn from many scripts and languages, the
//! same text every run: every division covers the text, in order, at
//! byte offsets on character boundaries; a paragraph's last line break is
//! at its end; and a line's visual order is an order of all its
//! characters.

use std::ops::Range;

use locale::{
    Bidi, Direction, Language, graphemes, line_breaks, script_runs, sentences, visual_order,
    word_count, words,
};

/// Pieces of text: letters of many scripts, combining marks, emoji
/// sequences, spaces, hard breaks, punctuation, digits, and controls of
/// direction and of breaking.
const PIECES: &[&str] = &[
    "a",
    "Z",
    "é",
    "e\u{301}",
    " ",
    "  ",
    "\t",
    "\n",
    "\u{2028}",
    "\r\n",
    ".",
    "?",
    ";",
    "-",
    "\u{2010}",
    "1",
    "42",
    "(",
    ")",
    "\"",
    "中",
    "文",
    "日本",
    "語",
    "ゃ",
    "ー",
    "。",
    "「",
    "」",
    "한",
    "ก",
    "ไทย",
    "ا",
    "ل",
    "عربي",
    "ב",
    "שלום",
    "\u{5b0}",
    "😀",
    "👩\u{200d}💻",
    "🇳🇴",
    "\u{200b}",
    "\u{ad}",
    "\u{a0}",
    "Ж",
    "ω",
    "\u{202e}",
    "\u{2067}",
    "\u{2069}",
    "\u{fffc}",
];

const LANGUAGES: &[&str] = &["en", "ja", "zh", "th", "ar", "he", "el", "de", "und"];

/// Text drawn by xorshift64*, from a fixed seed.
struct Draw(u64);

impl Draw {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) % n as u64) as usize
    }

    fn text(&mut self) -> String {
        (0..self.below(40))
            .map(|_| PIECES[self.below(PIECES.len())])
            .collect()
    }

    /// Languages for stretches of `text`, in order, each starting and
    /// ending on a character boundary, with gaps of no language between
    /// some.
    fn languages(&mut self, text: &str) -> Vec<(Range<usize>, Language)> {
        let mut cuts: Vec<usize> = (0..self.below(5))
            .map(|_| self.below(text.len() + 1))
            .filter(|&o| text.is_char_boundary(o))
            .chain([0, text.len()])
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let mut out = Vec::new();
        for w in cuts.windows(2) {
            if self.below(4) != 0 {
                let tag = LANGUAGES[self.below(LANGUAGES.len())];
                out.push((w[0]..w[1], Language::parse(tag).expect("a language")));
            }
        }
        out
    }
}

/// Whether `ranges` cover `text` from its start to its end, each after the
/// one before, none empty, all on character boundaries.
fn covers(text: &str, ranges: &[Range<usize>]) -> bool {
    let mut at = 0;
    for range in ranges {
        if range.start != at || range.end <= range.start || !text.is_char_boundary(range.end) {
            return false;
        }
        at = range.end;
    }
    at == text.len()
}

#[test]
fn every_division_covers_the_text() {
    let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let text = draw.text();
        let languages = draw.languages(&text);
        let why = format!("{text:?} in {languages:?}");

        let g = graphemes(&text);
        let spans: Vec<Range<usize>> = g.windows(2).map(|w| w[0]..w[1]).collect();
        assert_eq!(g[0], 0, "{why}");
        assert!(covers(&text, &spans), "graphemes {g:?} of {why}");

        let breaks = line_breaks(&text, &languages);
        let offsets: Vec<usize> = breaks.iter().map(|b| b.offset).collect();
        assert_eq!(offsets.last(), Some(&text.len()), "{why}");
        assert!(
            offsets.windows(2).all(|w| w[0] < w[1]) && offsets[0] > 0 || text.is_empty(),
            "line breaks {offsets:?} of {why}"
        );
        assert!(
            offsets.iter().all(|&o| text.is_char_boundary(o)),
            "line breaks {offsets:?} of {why}"
        );

        let w = words(&text, &languages);
        let spans: Vec<Range<usize>> = w.iter().map(|w| w.range.clone()).collect();
        assert!(covers(&text, &spans), "words {spans:?} of {why}");
        assert!(word_count(&text, &languages) <= w.iter().filter(|w| w.word_like).count());

        let s = sentences(&text, &languages);
        assert!(covers(&text, &s), "sentences {s:?} of {why}");

        let runs: Vec<Range<usize>> = script_runs(&text).into_iter().map(|(r, _)| r).collect();
        assert!(covers(&text, &runs), "script runs {runs:?} of {why}");

        for direction in [None, Some(Direction::Ltr), Some(Direction::Rtl)] {
            let bidi = Bidi::new(&text, direction);
            let levels = bidi.line_levels(0..text.len());
            assert_eq!(levels.len(), text.len(), "{why}");
            let mut order = visual_order(&levels);
            order.sort_unstable();
            assert!(order.into_iter().eq(0..levels.len()), "{why}");
        }
    }
}

/// Levels past the algorithm's deepest are read as left to right, and the
/// order is still of every item.
#[test]
fn any_levels_have_a_visual_order() {
    let mut draw = Draw(0x2545_f491_4f6c_dd1d);
    for _ in 0..1000 {
        let levels: Vec<u8> = (0..draw.below(30)).map(|_| draw.below(256) as u8).collect();
        let mut order = visual_order(&levels);
        order.sort_unstable();
        assert!(order.into_iter().eq(0..levels.len()), "{levels:?}");
    }
}
