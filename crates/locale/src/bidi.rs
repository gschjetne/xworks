use std::ops::Range;

use icu_properties::CodePointMapData;
use icu_properties::props::BidiClass;
use unicode_bidi::{Level, ParagraphBidiInfo};

/// A paragraph's direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Left to right, as in Latin, Greek, Cyrillic and CJK.
    Ltr,
    /// Right to left, as in Arabic and Hebrew.
    Rtl,
}

/// A paragraph's embedding levels, by the Unicode Bidirectional Algorithm
/// (UAX #9) on ICU4X's data. A level is a small number: even levels run
/// left to right, odd ones right to left.
pub struct Bidi<'a>(ParagraphBidiInfo<'a>);

impl<'a> Bidi<'a> {
    /// The levels of `text`, one paragraph, in `direction`; with `None`,
    /// the direction of its first strong character, or left to right if it
    /// has none.
    pub fn new(text: &'a str, direction: Option<Direction>) -> Bidi<'a> {
        let level = direction.map(|d| match d {
            Direction::Ltr => Level::ltr(),
            Direction::Rtl => Level::rtl(),
        });
        let data = CodePointMapData::<BidiClass>::new();
        Bidi(ParagraphBidiInfo::new_with_data_source(&data, text, level))
    }

    /// The paragraph's direction.
    pub fn direction(&self) -> Direction {
        if self.0.paragraph_level.is_rtl() {
            Direction::Rtl
        } else {
            Direction::Ltr
        }
    }

    /// The paragraph's own level: 0 or 1.
    pub fn paragraph_level(&self) -> u8 {
        self.0.paragraph_level.number()
    }

    /// The level of the character at `offset`.
    pub fn level(&self, offset: usize) -> u8 {
        self.0
            .levels
            .get(offset)
            .map_or(self.paragraph_level(), Level::number)
    }

    /// The levels of the line `range`, a byte each, as displayed: with
    /// whitespace at the line's end, and before tabs and separators, at
    /// the paragraph's level (rule L1).
    pub fn line_levels(&self, range: Range<usize>) -> Vec<u8> {
        if range.is_empty() {
            return Vec::new();
        }
        let levels = self.0.reordered_levels(range.clone());
        levels[range].iter().map(Level::number).collect()
    }
}

/// The visual order of items with these levels, left to right, as indices
/// into `levels` (rule L2).
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let levels: Vec<Level> = levels
        .iter()
        .map(|&n| Level::new(n).unwrap_or(Level::ltr()))
        .collect();
    ParagraphBidiInfo::reorder_visual(&levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_strong_character_sets_the_direction() {
        assert_eq!(Bidi::new("abc אבג", None).direction(), Direction::Ltr);
        assert_eq!(Bidi::new("123 אבג abc", None).direction(), Direction::Rtl);
        assert_eq!(Bidi::new("123", None).direction(), Direction::Ltr);
        assert_eq!(Bidi::new("", None).direction(), Direction::Ltr);
        assert_eq!(
            Bidi::new("abc", Some(Direction::Rtl)).direction(),
            Direction::Rtl
        );
    }

    #[test]
    fn levels_follow_the_algorithm() {
        // In "ab אב 12", the Hebrew and the number after it run right to
        // left in a left-to-right paragraph; the number rises to level 2.
        let text = "ab אב 12";
        let bidi = Bidi::new(text, None);
        let levels: Vec<u8> = text.char_indices().map(|(i, _)| bidi.level(i)).collect();
        assert_eq!(levels, [0, 0, 0, 1, 1, 1, 2, 2]);
    }

    #[test]
    fn trailing_whitespace_takes_the_paragraph_level() {
        // In the right-to-left paragraph "אב cd ef", the space between the
        // Latin words is at their level, 2. When a line ends after it, it
        // goes back to the paragraph's level (rule L1).
        let text = "אב cd ef";
        let bidi = Bidi::new(text, None);
        let space = text.len() - 3;
        assert_eq!(bidi.level(space), 2);
        let line = bidi.line_levels(0..space + 1);
        assert_eq!(line[space], 1);
        assert_eq!(line[space - 1], 2);
        assert_eq!(bidi.line_levels(4..4), Vec::<u8>::new());
    }

    #[test]
    fn visual_order_reverses_right_to_left_runs() {
        assert_eq!(visual_order(&[0, 0, 1, 1, 0]), [0, 1, 3, 2, 4]);
        assert_eq!(visual_order(&[1, 1, 2, 2, 1]), [4, 2, 3, 1, 0]);
        assert_eq!(visual_order(&[]), Vec::<usize>::new());
    }
}
