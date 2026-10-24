//! Moving and selecting by words and sentences, as the arrows with Ctrl,
//! or Option on a Mac, and double and triple clicks do; and setting text's
//! language and paragraphs' alignment.
//!
//! Words go as each platform's do (project owner, 2026-10-23, review
//! dcn5ejrd): forward on Windows and Linux to the next word's start, and
//! on into the next paragraph at its start; on a Mac to a word's end, and
//! into the next paragraph to its first word's end. Back is to a word's
//! start everywhere.

use doc::{Op, Value};
use story::keys;
use text::{Cursor, Direction, Language};

use super::{Editor, Position};

/// What a double click (a word) or a triple click (a sentence) selects,
/// and a drag after it extends the selection by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
    Word,
    Sentence,
}

impl Editor {
    /// Runs command `id` if it's one of this module's: moving or selecting
    /// by words or sentences, a language, or an alignment. Whether it was.
    pub(super) fn unit_command(&mut self, id: &str) -> bool {
        let (unit, right, extend) = match id {
            "word-left" => (Unit::Word, false, false),
            "word-right" => (Unit::Word, true, false),
            "select-word-left" => (Unit::Word, false, true),
            "select-word-right" => (Unit::Word, true, true),
            "sentence-left" => (Unit::Sentence, false, false),
            "sentence-right" => (Unit::Sentence, true, false),
            "select-sentence-left" => (Unit::Sentence, false, true),
            "select-sentence-right" => (Unit::Sentence, true, true),
            _ => {
                if let Some(tag) = id.strip_prefix("language-") {
                    self.set_language(tag);
                } else if let Some(align) = id.strip_prefix("align-") {
                    self.set_align(align);
                } else {
                    return false;
                }
                return true;
            }
        };
        let target = self.unit_step(self.focus, unit, right);
        self.go(target, extend);
        self.goal = None;
        true
    }

    /// Whether command `id`, a language or an alignment, is the one at
    /// the caret.
    pub(super) fn unit_checked(&self, id: &str) -> bool {
        if let Some(tag) = id.strip_prefix("language-") {
            return Language::parse(tag).is_ok_and(|l| Some(l) == self.language_at(self.focus));
        }
        if let Some(align) = id.strip_prefix("align-") {
            let paragraph = self.paragraph(self.focus.paragraph);
            return Some(story::align(paragraph, &self.sheet)) == align_of(align);
        }
        false
    }

    /// The language of text typed at `p`: the character's before it, or
    /// its paragraph's.
    pub(super) fn language_at(&self, p: Position) -> Option<Language> {
        let tag = match self.typing_props(p).get(keys::LANGUAGE) {
            Some(Value::Text(tag)) => Some(tag.clone()),
            _ => match self.paragraph(p.paragraph).props.get(keys::LANGUAGE) {
                Some(Value::Text(tag)) => Some(tag.clone()),
                _ => None,
            },
        };
        tag.and_then(|t| Language::parse(&t).ok())
    }

    /// Where a step by `unit` to the right or left on screen goes from
    /// `p`: in the paragraph's reading direction, forward to the end of a
    /// unit, or on Windows and Linux to the next word's start, or back to
    /// its start; past the paragraph's edge, to the first unit's end in
    /// the next paragraph (its start, by words on Windows and Linux), or
    /// the last's start in the one before.
    fn unit_step(&self, p: Position, unit: Unit, right: bool) -> Position {
        let (k, cursor) = self.cursor(p);
        let forward = right == (self.laid.paragraphs[k].layout.direction() == Direction::Ltr);
        if let Some(c) = self.unit_move(k, cursor, unit, forward) {
            return self.position(k, c);
        }
        let ids = self.ids();
        let next = if forward {
            k.checked_add(1).filter(|&n| n < ids.len())
        } else {
            k.checked_sub(1)
        };
        let Some(n) = next else {
            return p;
        };
        if forward && unit == Unit::Word && !self.res.mac {
            return self.position(n, Cursor::downstream(0));
        }
        let edge = if forward {
            Cursor::downstream(0)
        } else {
            Cursor::upstream(
                self.laid.paragraphs[n]
                    .layout
                    .lines
                    .last()
                    .map_or(0, |l| l.range.end),
            )
        };
        let landing = self.unit_move(n, edge, unit, forward).unwrap_or(edge);
        self.position(n, landing)
    }

    /// The cursor a move by `unit` in paragraph `k` reaches from `cursor`,
    /// forward or back in the text's order.
    fn unit_move(&self, k: usize, cursor: Cursor, unit: Unit, forward: bool) -> Option<Cursor> {
        let layout = &self.laid.paragraphs[k].layout;
        match (unit, forward) {
            (Unit::Word, true) if self.res.mac => layout.next_word(cursor),
            (Unit::Word, true) => layout.next_word_start(cursor),
            (Unit::Word, false) => layout.previous_word(cursor),
            (Unit::Sentence, true) => layout.next_sentence(cursor),
            (Unit::Sentence, false) => layout.previous_sentence(cursor),
        }
    }

    /// The `unit` at `p`, from its start to its end.
    fn unit_at(&self, p: Position, unit: Unit) -> (Position, Position) {
        let (k, cursor) = self.cursor(p);
        let layout = &self.laid.paragraphs[k].layout;
        let range = match unit {
            Unit::Word => layout.word_at(cursor),
            Unit::Sentence => layout.sentence_at(cursor),
        };
        (
            self.position(k, Cursor::downstream(range.start)),
            self.position(k, Cursor::upstream(range.end)),
        )
    }

    /// Selects the `unit` at `p`, as a double or triple click does; a
    /// drag goes on to extend the selection by it.
    pub(super) fn select_unit(&mut self, p: Position, unit: Unit) {
        let (start, end) = self.unit_at(p, unit);
        self.go(start, false);
        self.focus = end;
        self.dragged = Some((unit, start, end));
    }

    /// Extends the selection made by a double or triple click to take in
    /// the unit at `p` too.
    pub(super) fn drag_unit(&mut self, p: Position) {
        let Some((unit, start, end)) = self.dragged else {
            return;
        };
        let (from, to) = self.unit_at(p, unit);
        if self.order(from) < self.order(start) {
            self.anchor = end;
            self.focus = from;
        } else {
            self.anchor = start;
            self.focus = if self.order(to) > self.order(end) {
                to
            } else {
                end
            };
        }
        self.typing = None;
    }

    /// Sets the selected text's language to `tag`, or with no selection
    /// the caret's whole paragraph's.
    pub(super) fn set_language(&mut self, tag: &str) {
        let Ok(language) = Language::parse(tag) else {
            return;
        };
        let value = Some(Value::Text(language.to_string()));
        let (start, end) = self.ordered();
        let collapsed = self.collapsed();
        let ids = self.ids().to_vec();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let mut ops = Vec::new();
        for (i, &id) in ids.iter().enumerate().take(b + 1).skip(a) {
            let len = self.paragraph(id).len();
            let from = if i == a { start.offset } else { 0 };
            let to = if i == b { end.offset } else { len };
            if collapsed || (from == 0 && to == len) {
                // The whole paragraph: its own language, and none of its
                // text's own.
                ops.push(Op::SetParaProp {
                    paragraph: id,
                    key: keys::LANGUAGE.to_string(),
                    value: value.clone(),
                });
                if len > 0 {
                    ops.push(Op::SetCharProp {
                        paragraph: id,
                        offset: 0,
                        len,
                        key: keys::LANGUAGE.to_string(),
                        value: None,
                    });
                }
            } else if to > from {
                ops.push(Op::SetCharProp {
                    paragraph: id,
                    offset: from,
                    len: to - from,
                    key: keys::LANGUAGE.to_string(),
                    value: value.clone(),
                });
            }
        }
        self.commit("Language", ops, false);
    }

    /// Aligns the selected paragraphs `align`: "start", "center", "end" or
    /// "justify".
    fn set_align(&mut self, align: &str) {
        if align_of(align).is_none() {
            return;
        }
        let (start, end) = self.ordered();
        let ids = self.ids().to_vec();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        let ops = ids[a..=b]
            .iter()
            .map(|&id| Op::SetParaProp {
                paragraph: id,
                key: keys::ALIGN.to_string(),
                value: Some(Value::Text(align.to_string())),
            })
            .collect();
        self.commit("Alignment", ops, false);
    }
}

/// The alignment a command names.
fn align_of(name: &str) -> Option<text::Align> {
    match name {
        "start" => Some(text::Align::Start),
        "center" => Some(text::Align::Center),
        "end" => Some(text::Align::End),
        "justify" => Some(text::Align::Justify),
        _ => None,
    }
}
