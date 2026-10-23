//! The editor behind the canvas contract (`part`): what a window, or later
//! another part, gives it and gets from it.

use part::{Access, Input, Look, Part, Pointer, Request, Surface};
use text::units::PIXEL;

use super::{Editor, Effect, Event, View};
use crate::Resources;

/// The colours a story is drawn in, light or dark: the chrome's look's.
pub(crate) fn view_look(res: &Resources, dark: bool) -> Look {
    let c = if dark { &res.dark } else { &res.light };
    Look {
        desk: c.desk,
        page: c.page,
        page_ink: c.page_ink,
        ink: c.ink,
        selection: c.selection,
        shadow: c.shadow,
        line: c.line,
        tint: c.hover,
        accent: c.accent,
    }
}

/// Layout units as the logical pixels events carry.
fn pixels(units: i64) -> f64 {
    units as f64 / PIXEL as f64
}

impl Part for Editor {
    fn resize(&mut self, surface: Surface) {
        self.update(Event::Resize {
            width: pixels(surface.width),
            height: pixels(surface.height),
        });
    }

    fn input(&mut self, input: Input) -> Vec<Request> {
        let event = match input {
            Input::Key { key, modifiers } => Event::Key { key, modifiers },
            Input::Text(text) => Event::Text(text),
            Input::Preedit(text) => Event::Preedit(text),
            Input::Press { x, y, shift, count } => Event::Press {
                x: pixels(x),
                y: pixels(y),
                shift,
                count,
            },
            Input::Move { x, y } => Event::Move {
                x: pixels(x),
                y: pixels(y),
            },
            Input::Release => Event::Release,
            Input::Scroll(dy) => Event::Scroll(pixels(dy)),
            Input::Command(id) => Event::Command(id),
            Input::CommandWith(id, args) => Event::CommandWith(id, args),
            Input::Pasted(text) => Event::Pasted(text),
            Input::Chosen(choice, path) => Event::Chosen(choice, path),
        };
        self.update(event)
            .into_iter()
            .filter_map(|effect| {
                Some(match effect {
                    Effect::Copy(text) => Request::Copy(text),
                    Effect::Paste => Request::Paste,
                    Effect::Choose(choice) => Request::Choose(choice),
                    Effect::Open(path) => Request::Open(path),
                    Effect::OpenVersion(path, seq) => Request::OpenVersion(path, seq),
                    Effect::OpenExternally(path) => Request::OpenExternally(path),
                    Effect::Reveal(path) => Request::Reveal(path),
                    Effect::Quit => Request::Quit,
                    Effect::Window(_) | Effect::Pointer(_) => return None,
                })
            })
            .collect()
    }

    fn frame(&self, look: &Look) -> Vec<scene::Item> {
        crate::view::draw(self, look).items
    }

    fn state(&self, id: &str) -> chrome::CommandState {
        Editor::state(self, id)
    }

    fn inspector(&self) -> chrome::Inspector {
        self.inspector_model()
    }

    fn ruler(&self) -> Option<chrome::Ruler> {
        Some(self.ruler_model())
    }

    fn caret(&self) -> Option<(i64, i64, i64)> {
        Some(self.caret_rect())
    }

    fn pointer(&self, x: i64, y: i64) -> Pointer {
        // The text beam over the text's column, on a page or the desk.
        let style = self.page_style();
        let doc_y = y + self.scroll;
        let over = (0..self.laid.pages.len()).any(|n| {
            let (px, py) = self.page_origin(n);
            let height = match self.view {
                View::Pages => style.height,
                View::Draft => self.draft_height().max(style.top),
            };
            (px..px + style.width).contains(&x) && (py..py + height).contains(&doc_y)
        });
        if over { Pointer::Text } else { Pointer::Arrow }
    }

    fn access(&self, first: u64) -> Access {
        self.access_tree(first)
    }

    fn status(&self) -> String {
        Editor::status(self)
    }
}
