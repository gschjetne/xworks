//! Words and sentences of a laid-out paragraph, by its languages' rules:
//! where the caret moves by them, and what a double or a triple click
//! selects.
//!
//! Moving by words goes the same way on every platform: forward to the end
//! of a word, back to its start, so a word selected by moving over it has
//! no space after it. Sentences are the same, but a sentence selected
//! whole takes the spaces after it, so deleting it leaves one space
//! between its neighbours.

use std::ops::Range;

use crate::caret::Cursor;
use crate::layout::Layout;

impl Layout {
    /// The paragraph's words, and the stretches between them.
    fn words(&self) -> Vec<locale::Word> {
        locale::words(&self.text, &self.spans)
    }

    /// The paragraph's sentences, each with the spaces after it.
    fn sentences(&self) -> Vec<Range<usize>> {
        locale::sentences(&self.text, &self.spans)
    }

    /// The cursor at the end of the word `cursor` is in or before, in the
    /// text's order; at the text's end if no word follows, and `None`
    /// there.
    pub fn next_word(&self, cursor: Cursor) -> Option<Cursor> {
        let at = self.normalize(cursor).offset;
        let end = self
            .words()
            .into_iter()
            .filter(|w| w.word_like)
            .map(|w| w.range.end)
            .find(|&end| end > at);
        self.forward_to(at, end)
    }

    /// The cursor at the start of the word `cursor` is in or after, in
    /// the text's order; at the text's start if no word comes before, and
    /// `None` there.
    pub fn previous_word(&self, cursor: Cursor) -> Option<Cursor> {
        let at = self.normalize(cursor).offset;
        let start = self
            .words()
            .into_iter()
            .filter(|w| w.word_like)
            .map(|w| w.range.start)
            .rfind(|&start| start < at);
        self.back_to(at, start)
    }

    /// The cursor at the end of the sentence `cursor` is in or before,
    /// before the spaces after it; `None` at the text's end.
    pub fn next_sentence(&self, cursor: Cursor) -> Option<Cursor> {
        let at = self.normalize(cursor).offset;
        let end = self
            .sentences()
            .into_iter()
            .map(|s| s.start + self.text[s].trim_end().len())
            .find(|&end| end > at);
        self.forward_to(at, end)
    }

    /// The cursor at the start of the sentence `cursor` is in or after;
    /// `None` at the text's start.
    pub fn previous_sentence(&self, cursor: Cursor) -> Option<Cursor> {
        let at = self.normalize(cursor).offset;
        let start = self
            .sentences()
            .into_iter()
            .map(|s| s.start)
            .rfind(|&start| start < at);
        self.back_to(at, start)
    }

    /// The word `cursor` is in, or the spaces or punctuation between words
    /// it's in: what a double click there selects. At a boundary, the
    /// stretch the cursor belongs with.
    pub fn word_at(&self, cursor: Cursor) -> Range<usize> {
        let ranges: Vec<Range<usize>> = self.words().into_iter().map(|w| w.range).collect();
        self.unit_at(cursor, &ranges)
    }

    /// The sentence `cursor` is in, with the spaces after it: what a
    /// triple click there selects.
    pub fn sentence_at(&self, cursor: Cursor) -> Range<usize> {
        self.unit_at(cursor, &self.sentences())
    }

    fn unit_at(&self, cursor: Cursor, ranges: &[Range<usize>]) -> Range<usize> {
        let cursor = self.normalize(cursor);
        let at = cursor.offset;
        let before = cursor.affinity == crate::Affinity::Upstream;
        ranges
            .iter()
            .find(|r| {
                (r.start < at && at < r.end)
                    || (before && r.end == at && r.start < at)
                    || (!before && r.start == at)
            })
            .or_else(|| ranges.iter().find(|r| r.start <= at && at <= r.end))
            .cloned()
            .unwrap_or(at..at)
    }

    /// The cursor moving forward from `at` to `to`, or to the text's end.
    fn forward_to(&self, at: usize, to: Option<usize>) -> Option<Cursor> {
        let end = self.text.len();
        let to = to.unwrap_or(end);
        (to > at).then(|| self.normalize(Cursor::upstream(to)))
    }

    /// The cursor moving back from `at` to `to`, or to the text's start.
    fn back_to(&self, at: usize, to: Option<usize>) -> Option<Cursor> {
        let to = to.unwrap_or(0);
        (to < at).then(|| self.normalize(Cursor::downstream(to)))
    }
}
