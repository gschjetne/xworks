//! The inspector's Style tab: a screen for every kind of style and part a
//! document's stylesheet holds (docs/ARCHITECTURE.md, section 6; project
//! owner, 2026-10-26, review sjj5ak5a).

use doc::ObjectId;

use super::Editor;

/// What the Style tab shows: the caret's paragraph style, until the
/// person chooses a style or a part, or a kind of them; and the screens
/// followed to it from values that name parts, to go back to.
#[derive(Clone, Debug, Default)]
pub(super) struct Showing {
    /// The style or part chosen, if one is.
    chosen: Option<ObjectId>,
}

impl Editor {
    /// The Style tab shows the caret's paragraph style again, as it does
    /// when it's opened.
    pub fn show_caret_style(&mut self) {
        self.showing = Showing::default();
    }

    /// The style or part the Style tab shows, if one was chosen rather
    /// than the caret's paragraph style.
    pub fn style_chosen(&self) -> Option<ObjectId> {
        self.showing.chosen
    }

    /// The Style tab shows style or part `id`.
    pub fn choose_style(&mut self, id: ObjectId) {
        self.showing = Showing { chosen: Some(id) };
    }
}
