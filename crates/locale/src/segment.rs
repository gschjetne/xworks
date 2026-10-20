use icu_segmenter::options::LineBreakOptions;
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter};

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
/// Thai, Lao, Khmer and Burmese, which don't mark word boundaries, are
/// broken by ICU4X's models of those languages.
pub fn line_breaks(text: &str) -> Vec<Break> {
    let segmenter = LineSegmenter::new_auto(LineBreakOptions::default());
    let mut breaks: Vec<Break> = segmenter
        .segment_str(text)
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

/// The boundaries of `text`'s grapheme clusters, the characters a reader
/// sees (UAX #29), from 0 to the text's length.
pub fn graphemes(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = GraphemeClusterSegmenter::new().segment_str(text).collect();
    if boundaries.is_empty() {
        boundaries.push(0);
    }
    boundaries
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
        line_breaks(text).iter().map(|b| b.offset).collect()
    }

    #[test]
    fn lines_break_after_spaces() {
        assert_eq!(offsets("Hello world, again."), [6, 13, 19]);
        assert_eq!(offsets("Γειά σου κόσμε"), [9, 16, 26]);
        assert_eq!(offsets(""), [0]);
    }

    #[test]
    fn hard_breaks_are_mandatory() {
        let breaks = line_breaks("one\u{2028}two three");
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
}
