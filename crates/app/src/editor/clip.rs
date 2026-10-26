//! Copying and pasting with structure and styles (docs/ARCHITECTURE.md,
//! section 6): Copy puts the selection's text on the system's clipboard,
//! as ever, and keeps beside it, in the program, a fragment of what was
//! copied: each paragraph's stretch with its runs and its own properties,
//! and the document it came from, for its stylesheet. Paste uses the
//! fragment while the clipboard still holds the text it was kept with, in
//! any window of this xWorks; text from anywhere else pastes as text.
//!
//! The destination's styles win (project owner, 2026-10-24, review
//! 30dmn1js): a pasted paragraph's style, and its text's character styles
//! and parts, are matched with the destination's by role and then by name,
//! and those it lacks come along with their chains, as `story::merge`
//! brings them. Nothing is renamed "Body 2". Pictures don't come along, as
//! their files are the other document's. A table's cells paste as
//! paragraphs, and those of its header row take the default style, as
//! Table Header is a header row's (review sjj5ak5a).

use doc::{Document, ObjectId, Op, Props, Run, Value};
use std::collections::HashMap;

use super::{Editor, Position, byte, chars};

/// What Copy kept beside the text it put on the clipboard.
#[derive(Clone, Debug)]
pub(crate) struct Fragment {
    /// The text it put on the system's clipboard, by which a paste is
    /// known to be of it.
    text: String,
    /// Each paragraph copied: its own properties, and the stretch of its
    /// text copied, with its runs.
    paragraphs: Vec<(Props, String, Vec<Run>)>,
    /// The document it came from, for its stylesheet.
    source: Document,
}

/// `text` with its line endings as a paste reads them.
fn lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `props` with references to the fragment's styles and parts turned into
/// the destination's, by `map`; a reference to anything else is left out.
fn mapped(props: &Props, map: &HashMap<ObjectId, ObjectId>) -> Props {
    props
        .iter()
        .filter_map(|(k, v)| match v {
            Value::Ref(id) => map.get(id).map(|to| (k.clone(), Value::Ref(*to))),
            other => Some((k.clone(), other.clone())),
        })
        .collect()
}

impl Editor {
    /// Keeps the selection as the program's fragment, beside `text`, what
    /// goes on the system's clipboard.
    pub(super) fn keep_fragment(&self, text: &str) {
        let (start, end) = self.ordered();
        let ids = self.selected_paragraphs();
        let last = ids.len().saturating_sub(1);
        let mut paragraphs = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let p = self.paragraph(id);
            let from = if i == 0 { start.offset } else { 0 };
            let to = if i == last { end.offset } else { p.len() };
            let mut runs = Vec::new();
            let mut pos = 0;
            for run in &p.runs {
                let (a, b) = (pos.max(from), (pos + run.len).min(to));
                if a < b {
                    runs.push(Run {
                        len: b - a,
                        props: run.props.clone(),
                    });
                }
                pos += run.len;
            }
            let stretch = p.text[byte(&p.text, from)..byte(&p.text, to.max(from))].to_string();
            paragraphs.push((p.props.clone(), stretch, runs));
        }
        let mut clip = self
            .res
            .clip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *clip = Some(Fragment {
            text: lines(text),
            paragraphs,
            source: self.document.clone(),
        });
    }

    /// Pastes the program's fragment in place of the selection, if the
    /// clipboard holds the text it was kept with, `text`. False if it
    /// doesn't, or the selection is in code or a table's cell, which take
    /// text as text.
    pub(super) fn paste_fragment(&mut self, text: &str) -> bool {
        let fragment = {
            let clip = self
                .res
                .clip
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match clip.as_ref() {
                Some(f) if f.text == lines(text) => f.clone(),
                _ => return false,
            }
        };
        let at = self.ordered().0.paragraph;
        let in_cell = self
            .document
            .parent(at)
            .and_then(|p| self.document.block(p))
            .is_some_and(|b| b.kind == doc::Kind::Row);
        if self.is_code(at) || in_cell {
            return false;
        }
        let (mut ops, at) = self.deletion();
        let mut scratch = self.document.clone();
        if scratch.apply_all(&ops).is_err() {
            return true;
        }
        let (styles, map) = story::merge(&mut scratch, &fragment.source, false);
        ops.extend(styles);
        let body = self.sheet.default_style().id;
        // A paragraph's own properties as pasted: its style the
        // destination's, but for a header row's, which isn't one here.
        let pasted_props = |props: &Props| -> Props {
            let mut out = mapped(props, &map);
            let header = match props.get(story::keys::STYLE) {
                Some(Value::Ref(id)) => fragment.source.style(*id).is_some_and(|s| {
                    s.props.get(story::keys::ROLE)
                        == Some(&Value::Text(story::TABLE_HEADER.to_string()))
                }),
                _ => false,
            };
            if header {
                match body {
                    Some(body) => {
                        out.insert(story::keys::STYLE.to_string(), Value::Ref(body));
                    }
                    None => {
                        out.remove(story::keys::STYLE);
                    }
                }
            }
            out
        };
        let mut caret: Position = at;
        for (i, (props, text, runs)) in fragment.paragraphs.iter().enumerate() {
            if i > 0 {
                let new = scratch.new_id();
                let op = Op::SplitParagraph {
                    paragraph: caret.paragraph,
                    offset: caret.offset,
                    new,
                    new_props: pasted_props(props),
                };
                if scratch.apply(&op).is_err() {
                    return true;
                }
                ops.push(op);
                caret = Position {
                    paragraph: new,
                    offset: 0,
                    upstream: false,
                };
            }
            // Its text, but for pictures' characters, each run's
            // properties brought over; pictures stay behind.
            let mut kept = String::new();
            let mut kept_runs: Vec<Run> = Vec::new();
            let mut letters = text.chars();
            for run in runs {
                let mut props = mapped(&run.props, &map);
                for key in story::keys::PICTURES {
                    props.remove(key);
                }
                let piece: String = letters
                    .by_ref()
                    .take(run.len as usize)
                    .filter(|&c| {
                        c != story::OBJECT && (!c.is_control() || c == '\t' || c == '\u{2028}')
                    })
                    .collect();
                let n = chars(&piece);
                if n > 0 {
                    kept.push_str(&piece);
                    kept_runs.push(Run::new(n, props));
                }
            }
            if kept.is_empty() {
                continue;
            }
            let len = chars(&kept);
            let op = Op::InsertText {
                paragraph: caret.paragraph,
                offset: caret.offset,
                text: kept,
                runs: kept_runs,
            };
            if scratch.apply(&op).is_err() {
                return true;
            }
            ops.push(op);
            caret.offset += len;
            caret.upstream = true;
        }
        if self.commit("Paste", ops, false) {
            self.go(caret, false);
        }
        true
    }
}
