use std::cell::RefCell;
use std::ops::Range;
use std::sync::OnceLock;

use icu_segmenter::options::{
    LineBreakOptions, LineBreakStrictness, SentenceBreakOptions, WordBreakInvariantOptions,
    WordBreakOptions,
};
use icu_segmenter::{
    GraphemeClusterSegmenter, LineSegmenter, LineSegmenterBorrowed, SentenceSegmenter,
    WordSegmenter,
};

use crate::Language;

/// A place a line may end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Break {
    /// The offset the next line would start at.
    pub offset: usize,
    /// Whether the line must end here, after a line separator or other hard
    /// break.
    pub mandatory: bool,
}

/// Where lines in `text`, a paragraph, may break (UAX #14), in order. The
/// last is always at the end of the text; a break at the start never is.
///
/// `languages` are the text's languages, as ranges of it in order; text
/// they leave out has none. Japanese and Chinese break by CSS's `normal`
/// rules for their language, which allow a line to start with a small
/// kana or a prolonged sound mark, as Japanese word processors do; other
/// text by UAX #14's default, CSS's `strict`.
///
/// Thai, Lao, Khmer and Burmese, which don't mark word boundaries, are
/// broken by ICU4X's models of those languages.
pub fn line_breaks(text: &str, languages: &[(Range<usize>, Language)]) -> Vec<Break> {
    let tailored =
        |language: &Language| matches!(language.identifier().language.as_str(), "ja" | "zh");
    let language_before = |offset: usize| {
        languages
            .iter()
            .find(|(range, _)| range.start < offset && offset <= range.end)
            .map(|(_, language)| language)
    };
    let mut offsets: Vec<usize> = default_lines(text)
        .into_iter()
        .filter(|&b| !language_before(b).is_some_and(tailored))
        .collect();
    let mut done: Vec<&Language> = Vec::new();
    for (_, language) in languages {
        if !tailored(language) || done.contains(&language) {
            continue;
        }
        done.push(language);
        let mut options = LineBreakOptions::default();
        options.strictness = Some(LineBreakStrictness::Normal);
        options.content_locale = Some(language.identifier());
        offsets.extend(
            segment_lines(text, options)
                .into_iter()
                .filter(|&b| language_before(b) == Some(language)),
        );
    }
    offsets.sort_unstable();
    offsets.dedup();
    let mut breaks: Vec<Break> = offsets
        .into_iter()
        .filter(|&offset| offset > 0)
        .map(|offset| Break {
            offset,
            mandatory: text[..offset]
                .chars()
                .next_back()
                .is_some_and(is_hard_break),
        })
        .collect();
    if breaks.last().is_none_or(|b| b.offset != text.len()) {
        breaks.push(Break {
            offset: text.len(),
            mandatory: false,
        });
    }
    breaks
}

fn segment_lines(text: &str, options: LineBreakOptions<'_>) -> Vec<usize> {
    LineSegmenter::new_auto(options).segment_str(text).collect()
}

/// The line segmenter for text of no tailored language, made once: making
/// one finds the models for Thai, Lao, Khmer and Burmese, which is most of
/// what breaking a short paragraph's lines takes. It's compiled data, the
/// same whenever it's made.
fn default_lines(text: &str) -> Vec<usize> {
    static LINES: OnceLock<LineSegmenterBorrowed<'static>> = OnceLock::new();
    LINES
        .get_or_init(|| LineSegmenter::new_auto(LineBreakOptions::default()))
        .segment_str(text)
        .collect()
}

/// The boundaries of `text`'s grapheme clusters, the characters a reader
/// sees (UAX #29), from 0 to the text's length.
pub fn graphemes(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = GraphemeClusterSegmenter::new().segment_str(text).collect();
    if boundaries.is_empty() {
        boundaries.push(0);
    }
    boundaries
}

/// A stretch of text between word boundaries (UAX #29).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    /// Where it is, as byte offsets.
    pub range: Range<usize>,
    /// Whether it's a word: letters, a number or ideographs, rather than
    /// spaces or punctuation.
    pub word_like: bool,
}

/// `text` divided at word boundaries (UAX #29), in order, covering it.
///
/// `languages` are the text's languages, as ranges of it in order; each
/// stretch of one language is divided by that language's rules, and a
/// change of language is a boundary. Chinese and Japanese, which don't put
/// spaces between words, are divided by ICU4X's dictionary of their
/// words; Thai, Lao, Khmer and Burmese by its models.
pub fn words(text: &str, languages: &[(Range<usize>, Language)]) -> Vec<Word> {
    let mut out = Vec::new();
    for (range, language) in stretches(text, languages) {
        let boundaries: Vec<(usize, bool)> = with_word_segmenter(&language, |segmenter| {
            let stretch = &text[range.clone()];
            match segmenter {
                Some(s) => s
                    .as_borrowed()
                    .segment_str(stretch)
                    .iter_with_word_type()
                    .map(|(offset, kind)| (offset, kind.is_word_like()))
                    .collect(),
                None => WordSegmenter::new_auto(WordBreakInvariantOptions::default())
                    .segment_str(stretch)
                    .iter_with_word_type()
                    .map(|(offset, kind)| (offset, kind.is_word_like()))
                    .collect(),
            }
        });
        let mut start = range.start;
        for (offset, word_like) in boundaries {
            let end = range.start + offset;
            if end > start {
                out.push(Word {
                    range: start..end,
                    word_like,
                });
                start = end;
            }
        }
    }
    out
}

/// Calls `f` with the word segmenter for `language`, none if compiled data
/// has none for it: one for each language on each thread, made the first
/// time it's needed and kept, as making one finds its language's data and
/// the models for Thai, Lao, Khmer and Burmese, and the dictionary for
/// Chinese and Japanese, which is most of what dividing a short paragraph
/// takes. Kept on the thread, as a segmenter can't be shared between
/// threads; made from compiled data, it divides text the same wherever
/// it's made.
fn with_word_segmenter<T>(
    language: &Option<Language>,
    f: impl FnOnce(Option<&WordSegmenter>) -> T,
) -> T {
    type Kept = Vec<(Option<Language>, Option<WordSegmenter>)>;
    thread_local! {
        static SEGMENTERS: RefCell<Kept> = const { RefCell::new(Vec::new()) };
    }
    SEGMENTERS.with(|kept| {
        let mut kept = kept.borrow_mut();
        let k = match kept.iter().position(|(l, _)| l == language) {
            Some(k) => k,
            None => {
                let mut options = WordBreakOptions::default();
                options.content_locale = language.as_ref().map(Language::identifier);
                // Compiled data has every language, so this is only none if
                // that ever changes.
                kept.push((language.clone(), WordSegmenter::try_new_auto(options).ok()));
                kept.len() - 1
            }
        };
        f(kept[k].1.as_ref())
    })
}

/// How many words `text` has: stretches of letters, numbers or
/// ideographs between word boundaries, by its languages' rules
/// ([`words`]). Words joined by a hyphen, as in "fixed-point", count as
/// one, as word processors count them.
pub fn word_count(text: &str, languages: &[(Range<usize>, Language)]) -> usize {
    let words = words(text, languages);
    let mut count = 0;
    for (k, word) in words.iter().enumerate() {
        let joined = k >= 2
            && matches!(&text[words[k - 1].range.clone()], "-" | "\u{2010}")
            && words[k - 2].word_like;
        if word.word_like && !joined {
            count += 1;
        }
    }
    count
}

/// `text` divided into sentences (UAX #29), in order, covering it. Each
/// takes the spaces after it.
///
/// `languages` are as for [`words`]: Greek, for one, ends a question
/// with `;`.
pub fn sentences(text: &str, languages: &[(Range<usize>, Language)]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for (range, language) in stretches(text, languages) {
        let mut options = SentenceBreakOptions::default();
        options.content_locale = language.as_ref().map(Language::identifier);
        let boundaries: Vec<usize> = match SentenceSegmenter::try_new(options) {
            Ok(s) => s.as_borrowed().segment_str(&text[range.clone()]).collect(),
            Err(_) => SentenceSegmenter::new(Default::default())
                .segment_str(&text[range.clone()])
                .collect(),
        };
        let mut start = range.start;
        for offset in boundaries {
            let end = range.start + offset;
            if end > start {
                out.push(start..end);
                start = end;
            }
        }
    }
    out
}

/// `text` in stretches of one language, covering it: `languages` with
/// neighbours of the same language joined, and what they leave out as
/// stretches of none.
fn stretches(
    text: &str,
    languages: &[(Range<usize>, Language)],
) -> Vec<(Range<usize>, Option<Language>)> {
    let mut out: Vec<(Range<usize>, Option<Language>)> = Vec::new();
    let mut at = 0;
    let mut push = |range: Range<usize>, language: Option<Language>| {
        if range.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, l)) if *l == language && last.end == range.start => last.end = range.end,
            _ => out.push((range, language)),
        }
    };
    for (range, language) in languages {
        let range = range.start.max(at).min(text.len())..range.end.min(text.len());
        if range.start > at {
            push(at..range.start, None);
        }
        if range.end > range.start {
            let language = (*language != Language::und()).then(|| language.clone());
            push(range.clone(), language);
            at = range.end;
        }
    }
    push(at..text.len(), None);
    out
}

/// Whether a line must break after `c` (UAX #14 classes BK, CR, LF and NL).
fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(text: &str) -> Vec<usize> {
        line_breaks(text, &[]).iter().map(|b| b.offset).collect()
    }

    fn tagged(text: &str, tag: &str) -> Vec<(Range<usize>, Language)> {
        vec![(0..text.len(), Language::parse(tag).unwrap())]
    }

    /// The segmenters kept, used again and again, divide text as ones
    /// made for the call do, in each language, and in none.
    #[test]
    fn kept_segmenters_divide_as_ones_made_afresh() {
        let cases = [
            (
                "und",
                "Hello, world. It's 3.14 — fine-tuned!\u{2028}Next line",
            ),
            (
                "en-GB",
                "Hello, world. It's 3.14 — fine-tuned!\u{2028}Next line",
            ),
            ("sv", "Det är 10:30, sa hon. Godmorgon!"),
            ("ja", "日本語の文章です。カタカナも、ひらがなも。"),
            ("zh", "这是一个中文句子，没有空格。"),
            ("th", "ทุกสองสัปดาห์ฉันไปตลาด"),
            ("ar", "مرحبا بالعالم، كيف حالك؟"),
        ];
        for (tag, text) in cases {
            let language = Language::parse(tag).unwrap();
            let mut options = WordBreakOptions::default();
            options.content_locale = (language != Language::und()).then(|| language.identifier());
            let made = WordSegmenter::try_new_auto(options).expect("compiled data");
            let mut start = 0;
            let fresh: Vec<Word> = made
                .as_borrowed()
                .segment_str(text)
                .iter_with_word_type()
                .filter(|&(offset, _)| offset > 0)
                .map(|(offset, kind)| {
                    let word = Word {
                        range: start..offset,
                        word_like: kind.is_word_like(),
                    };
                    start = offset;
                    word
                })
                .collect();
            let lines: Vec<usize> = LineSegmenter::new_auto(LineBreakOptions::default())
                .segment_str(text)
                .collect();
            for _ in 0..3 {
                assert_eq!(words(text, &tagged(text, tag)), fresh, "{tag}");
                assert_eq!(default_lines(text), lines, "{tag}");
            }
        }
    }

    #[test]
    fn lines_break_after_spaces() {
        assert_eq!(offsets("Hello world, again."), [6, 13, 19]);
        assert_eq!(offsets("Γειά σου κόσμε"), [9, 16, 26]);
        assert_eq!(offsets(""), [0]);
    }

    #[test]
    fn hard_breaks_are_mandatory() {
        let breaks = line_breaks("one\u{2028}two three", &[]);
        assert_eq!(
            breaks,
            [
                Break {
                    offset: 6,
                    mandatory: true
                },
                Break {
                    offset: 10,
                    mandatory: false
                },
                Break {
                    offset: 15,
                    mandatory: false
                },
            ]
        );
    }

    #[test]
    fn ideographs_break_between_but_not_around_brackets() {
        // 「日本」の: no break after the opening bracket or before the
        // closing one; one between the ideographs.
        let text = "「日本」の";
        let breaks = offsets(text);
        assert!(!breaks.contains(&3), "{breaks:?}");
        assert!(breaks.contains(&6), "{breaks:?}");
        assert!(!breaks.contains(&9), "{breaks:?}");
        assert_eq!(*breaks.last().unwrap(), text.len());
    }

    #[test]
    fn japanese_lines_may_start_with_a_small_kana() {
        // ちょっと: in Japanese, a line may start at ょ or っ; by UAX #14's
        // default, small kana stay with the kana before them.
        let text = "ちょっと";
        let ja: Vec<usize> = line_breaks(text, &tagged(text, "ja"))
            .iter()
            .map(|b| b.offset)
            .collect();
        assert!(ja.contains(&3) && ja.contains(&6), "{ja:?}");
        let default = offsets(text);
        assert!(
            !default.contains(&3) && !default.contains(&6),
            "{default:?}"
        );
        // Only the Japanese part of mixed text breaks the Japanese way.
        let mixed = "ちょっと ちょっと";
        let languages = vec![(0..13, Language::parse("ja").unwrap())];
        let breaks: Vec<usize> = line_breaks(mixed, &languages)
            .iter()
            .map(|b| b.offset)
            .collect();
        assert!(breaks.contains(&3), "{breaks:?}");
        assert!(!breaks.contains(&16), "{breaks:?}");
    }

    #[test]
    fn thai_breaks_between_words() {
        // ภาษาไทย ง่าย: "Thai language is easy", with no spaces inside words.
        let breaks = offsets("ภาษาไทยง่าย");
        assert!(breaks.len() >= 2, "{breaks:?}");
    }

    #[test]
    fn graphemes_keep_marks_and_sequences_whole() {
        assert_eq!(graphemes("e\u{301}x"), [0, 3, 4]);
        // A family emoji: four people joined by zero-width joiners.
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        assert_eq!(graphemes(family), [0, family.len()]);
        // Hangul syllables built from jamo.
        assert_eq!(graphemes("\u{1100}\u{1161}\u{11a8}"), [0, 9]);
        assert_eq!(graphemes(""), [0]);
    }

    fn word_texts<'a>(
        text: &'a str,
        languages: &[(Range<usize>, Language)],
    ) -> Vec<(&'a str, bool)> {
        words(text, languages)
            .into_iter()
            .map(|w| (&text[w.range], w.word_like))
            .collect()
    }

    #[test]
    fn words_are_letters_and_numbers_between_spaces_and_punctuation() {
        assert_eq!(
            word_texts("Don't stop, 3.5 times!", &[]),
            [
                ("Don't", true),
                (" ", false),
                ("stop", true),
                (",", false),
                (" ", false),
                ("3.5", true),
                (" ", false),
                ("times", true),
                ("!", false),
            ]
        );
        assert_eq!(word_texts("", &[]), []);
        assert_eq!(word_count("Γειά σου, κόσμε!", &[]), 3);
        // Hyphenated words count as one, but words around a dash don't.
        assert_eq!(word_count("a fixed-point number", &[]), 3);
        assert_eq!(word_count("well - known, well\u{2014}known", &[]), 4);
    }

    #[test]
    fn japanese_words_come_from_a_dictionary() {
        let text = "こんにちは世界";
        let words = word_texts(text, &tagged(text, "ja"));
        assert_eq!(words, [("こんにちは", true), ("世界", true)]);
    }

    #[test]
    fn a_change_of_language_is_a_word_boundary() {
        let text = "abcdef";
        let languages = vec![
            (0..3, Language::parse("en").unwrap()),
            (3..6, Language::parse("de").unwrap()),
        ];
        assert_eq!(word_texts(text, &languages), [("abc", true), ("def", true)]);
        // Neighbours of one language join.
        let languages = vec![
            (0..3, Language::parse("en").unwrap()),
            (3..6, Language::parse("en").unwrap()),
        ];
        assert_eq!(word_texts(text, &languages), [("abcdef", true)]);
    }

    #[test]
    fn sentences_take_the_spaces_after_them() {
        let text = "One. Two? Three";
        let got: Vec<&str> = sentences(text, &[]).into_iter().map(|r| &text[r]).collect();
        assert_eq!(got, ["One. ", "Two? ", "Three"]);
        assert_eq!(sentences("", &[]), Vec::<Range<usize>>::new());
    }

    #[test]
    fn greek_asks_with_a_semicolon() {
        let text = "Τι κάνεις; Καλά.";
        let got: Vec<&str> = sentences(text, &tagged(text, "el"))
            .into_iter()
            .map(|r| &text[r])
            .collect();
        assert_eq!(got, ["Τι κάνεις; ", "Καλά."]);
        assert_eq!(sentences(text, &[]).len(), 1);
    }
}
