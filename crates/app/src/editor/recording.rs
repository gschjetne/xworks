//! The editor's side of the session recorder ([`crate::recorder`]): where
//! a recorded session can start, a window taking one up again to play it
//! back, and Save Recent Session.

use std::sync::Arc;

use doc::{Document, History, ObjectId};
use store::Store;
use text::units::PIXEL;

use super::{Editor, EditorError, Position};
use crate::Resources;
use crate::recorder::Checkpoint;
use crate::session::Start;

impl Editor {
    /// The editor's state, if it's at rest, so a recorded session can
    /// start there: no panel, drag or composition is open. (The window
    /// says whether its chrome is at rest: [`crate::Window`].)
    pub(crate) fn checkpoint(&self) -> Option<Checkpoint> {
        if self.panel.is_some() || self.dragging || !self.composition.is_empty() {
            return None;
        }
        let place = |p: Position| (p.paragraph.0, p.offset, p.upstream);
        Some(Checkpoint {
            document: self.document.clone(),
            anchor: place(self.anchor),
            focus: place(self.focus),
            scroll: self.scroll,
            goal: self.goal,
            typing: self.typing == Some(self.focus),
            pending: self.pending.clone(),
            size: (
                self.width as f64 / PIXEL as f64,
                self.height as f64 / PIXEL as f64,
            ),
            chrome: None,
        })
    }

    /// A window taking up a recorded session where it starts, `size`
    /// logical pixels: its document, made a new one in the documents
    /// folder, with its history, its selection, scrolled as it was.
    pub fn resume(
        res: Arc<Resources>,
        start: &Start,
        size: (f64, f64),
    ) -> Result<Editor, EditorError> {
        let document = Document::from_snapshot(start.document.clone())
            .map_err(|e| store::Error::Damaged(e.to_string()))?;
        let path = Editor::untitled(&res)?;
        let store = Store::create(&path, &document, "story")?;
        let history = History::from_stacks(start.undo.clone(), start.redo.clone());
        let mut editor = Editor::with(res, path, Some(store), document, history);
        let px = |v: f64| ((v * PIXEL as f64).round() as i64).max(PIXEL);
        (editor.width, editor.height) = (px(size.0), px(size.1));
        // The view first, as the scroll is the view's.
        if start.chrome.is_some_and(|c| c.draft) {
            editor.view = super::View::Draft;
            editor.relayout(true);
        }
        let at = |(id, offset, upstream): (u64, u32, bool)| {
            let p = editor.document.paragraph(ObjectId(id))?;
            (offset <= p.len()).then_some(Position {
                paragraph: ObjectId(id),
                offset,
                upstream,
            })
        };
        let first = editor.document_edge(false);
        editor.anchor = at(start.anchor).unwrap_or(first);
        editor.focus = at(start.focus).unwrap_or(first);
        editor.scroll = start.scroll;
        editor.clamp_scroll();
        editor.goal = start.goal;
        editor.typing = start.typing.then_some(editor.focus);
        editor.pending = start.pending.clone();
        Ok(editor)
    }
}
