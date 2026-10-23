//! A small field of text, edited on one line: a document's name in the
//! title bar, a length in the inspector, what's typed into the palette. It
//! moves, selects and deletes as documents' text does, through `text`'s
//! editing: graphemes, words as each platform moves by them, and the caret
//! where the text is laid out.

use std::ops::Range;

use chrome::{Key, Modifiers};
use scene::{Color, Item, Path};
use text::units::PIXEL;
use text::{Cursor, Layout};

/// A field's text and its selection, as byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    text: String,
    anchor: usize,
    focus: usize,
}

/// What a key did to a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKey {
    /// The text changed.
    Changed,
    /// The caret or the selection moved.
    Moved,
    /// Enter: what's typed is done.
    Enter,
    /// Escape: what's typed is let go.
    Escape,
    /// Tab, back with Shift: on to the next field.
    Tab {
        /// Whether Shift is held.
        back: bool,
    },
    /// Nothing a field does.
    Ignored,
}

impl Field {
    /// A field holding `text`, all of it selected, so typing replaces it.
    pub fn new(text: &str) -> Field {
        Field {
            text: text.to_string(),
            anchor: 0,
            focus: text.len(),
        }
    }

    /// Its text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The selection, in the text's order.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }

    /// Where the caret is.
    pub fn caret(&self) -> usize {
        self.focus
    }

    /// The selected text.
    pub fn selected(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Selects all of it.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.focus = self.text.len();
    }

    /// Puts `typed` in place of the selection, without line breaks or
    /// other control characters.
    pub fn insert(&mut self, typed: &str) {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        let range = self.selection();
        self.text.replace_range(range.clone(), &typed);
        self.focus = range.start + typed.len();
        self.anchor = self.focus;
    }

    /// Handles `key` with `modifiers`, the text laid out as `layout`; on a
    /// Mac if `mac`, where words are Option's and go to their ends.
    pub fn key(&mut self, layout: &Layout, key: Key, modifiers: Modifiers, mac: bool) -> FieldKey {
        let word = if mac {
            modifiers.alt
        } else {
            modifiers.primary
        };
        let extend = modifiers.shift;
        let at = Cursor::downstream(self.focus);
        let moved = |field: &mut Field, to: Option<Cursor>| {
            if let Some(to) = to {
                field.focus = to.offset;
            }
            if !extend {
                field.anchor = field.focus;
            }
            FieldKey::Moved
        };
        match key {
            Key::Enter => FieldKey::Enter,
            Key::Escape => FieldKey::Escape,
            Key::Tab => FieldKey::Tab { back: extend },
            Key::Char('a') if modifiers.primary => {
                self.select_all();
                FieldKey::Moved
            }
            Key::Left | Key::Right if !extend && !word && self.anchor != self.focus => {
                let range = self.selection();
                self.focus = if key == Key::Right {
                    range.end
                } else {
                    range.start
                };
                self.anchor = self.focus;
                FieldKey::Moved
            }
            Key::Right if word => {
                let to = if mac {
                    layout.next_word(at)
                } else {
                    layout.next_word_start(at)
                };
                moved(self, to)
            }
            Key::Left if word => moved(self, layout.previous_word(at)),
            Key::Right => moved(self, layout.right(at)),
            Key::Left => moved(self, layout.left(at)),
            Key::Home | Key::Up => moved(self, Some(Cursor::downstream(0))),
            Key::End | Key::Down => moved(self, Some(Cursor::upstream(self.text.len()))),
            Key::Backspace | Key::Delete => {
                if self.anchor == self.focus {
                    let to = match (key == Key::Delete, word) {
                        (true, true) => layout.next_word_start(at),
                        (true, false) => layout.next(at),
                        (false, true) => layout.previous_word(at),
                        (false, false) => layout.previous(at),
                    };
                    match to {
                        Some(to) => self.anchor = to.offset,
                        None => return FieldKey::Ignored,
                    }
                }
                self.insert("");
                FieldKey::Changed
            }
            _ => FieldKey::Ignored,
        }
    }

    /// Puts the caret where `x` is across the text laid out as `layout`,
    /// extending the selection if `extend`.
    pub fn press(&mut self, layout: &Layout, x: i64, extend: bool) {
        let at = layout.hit_line(0, x);
        self.focus = at.offset.min(self.text.len());
        if !extend {
            self.anchor = self.focus;
        }
    }

    /// Draws the selection, or the caret, for the text laid out as
    /// `layout` from (`x`, `y`).
    pub fn draw(
        &self,
        out: &mut Vec<Item>,
        layout: &Layout,
        (x, y): (i64, i64),
        caret: Color,
        selection: Color,
    ) {
        if self.anchor == self.focus {
            let c = layout.caret(Cursor::downstream(self.focus));
            out.push(Item::Fill {
                path: Path::rect(
                    x + c.x - PIXEL / 2,
                    y + c.top,
                    PIXEL * 3 / 2,
                    c.bottom - c.top,
                ),
                color: caret,
            });
            return;
        }
        for r in layout.selection(self.selection()) {
            out.push(Item::Fill {
                path: Path::rect(x + r.x, y + r.y, r.width, r.height),
                color: selection,
            });
        }
    }
}
