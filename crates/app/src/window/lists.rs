//! The lists that pop up over the window, scrolled where they're taller
//! than the room they have (M2's exit, review 7fnfdhb5): an inspector's
//! choices, open under or over their field; the editor's panel; and the
//! menus. Each scrolls with the wheel over it and with its scroll bar, as
//! the inspector does; the keys that move about it scroll the row they
//! light into view, and a field's list opens on the row chosen. What a
//! press hits, and what screen readers see, follow the scroll.

use chrome::{ControlKind, Key};
use toolkit::{BarHit, ControlRef, Popup, PopupRow, Rect};

use super::{Drag, Window};
use crate::Effect;

/// An inspector's choice open as a list: which control, the choice lit,
/// and how far the list is scrolled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Choosing {
    pub at: ControlRef,
    pub lit: Option<usize>,
    pub scroll: i64,
}

/// A list that pops up, by which it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum List {
    /// The editor's panel.
    Panel,
    /// An inspector's choices.
    Choice,
    /// The menu open from the bar.
    Menu,
    /// Its submenu.
    Submenu,
}

impl Window {
    /// The list of an inspector's choice, open under its field, or over
    /// it where there's room only there, scrolled as it is.
    pub(super) fn choice_popup(&self, model: &chrome::Inspector) -> Option<(Popup, Vec<PopupRow>)> {
        let choosing = self.choosing?;
        let control = Window::control(model, self.tab, choosing.at)?;
        let ControlKind::Choice {
            choices, selected, ..
        } = &control.kind
        else {
            return None;
        };
        let rows: Vec<PopupRow> = choices
            .iter()
            .enumerate()
            .map(|(i, c)| PopupRow::of_choice(c, *selected == Some(i)))
            .collect();
        let field = self.inspector_view(model)?.control(choosing.at)?;
        let popup = Popup::by_field(&self.kit(), &rows, field, (self.width, self.height))
            .scrolled(choosing.scroll);
        Some((self.lit_thumb(popup), rows))
    }

    /// The editor's panel, open, as a list under the title bar: its
    /// title, a line, and its rows, scrolled as it is.
    pub(super) fn panel_popup(&self) -> Option<(Popup, Vec<PopupRow>)> {
        let (title, names, _) = self.editor.panel()?;
        let mut rows = vec![
            PopupRow {
                enabled: false,
                ..PopupRow::choice(title, false)
            },
            PopupRow::choice("", false),
        ];
        rows.extend(names.iter().map(|n| PopupRow::choice(n, false)));
        let width = self.res.sizes.palette.units().min(self.width);
        let kit = self.kit();
        let popup = Popup::new(
            &kit,
            &rows,
            (
                (self.width - width) / 2,
                self.title_height() + self.res.sizes.palette_top.units(),
            ),
            width,
            (self.width, self.height),
        )
        .scrolled(self.panel_scroll);
        Some((self.lit_thumb(popup), rows))
    }

    /// `popup` with its thumb lit if the pointer is over it.
    pub(super) fn lit_thumb(&self, popup: Popup) -> Popup {
        let (x, y) = self.pointer;
        let lit = popup.bar().is_some_and(|(_, thumb)| thumb.contains(x, y));
        popup.thumb_lit(lit)
    }

    /// List `list`, laid out as it's open and scrolled, if it's open.
    pub(super) fn list_popup(&self, list: List) -> Option<Popup> {
        match list {
            List::Panel => self.panel_popup().map(|(p, _)| p),
            List::Choice => self.choice_popup(&self.inspector_model()).map(|(p, _)| p),
            List::Menu | List::Submenu => {
                let bar = self.menu_bar();
                let buttons = self.buttons();
                let name = self.editor.name();
                let title = self.title_bar(&bar, &buttons, &name);
                let (main, sub) = self.popups(&self.kit(), &bar, &title)?;
                if list == List::Menu { Some(main) } else { sub }
            }
        }
    }

    /// The list open over (`x`, `y`), the uppermost, and it laid out.
    pub(super) fn list_at(&self, x: i64, y: i64) -> Option<(List, Popup)> {
        [List::Panel, List::Choice, List::Submenu, List::Menu]
            .into_iter()
            .filter_map(|list| Some((list, self.list_popup(list)?)))
            .find(|(_, popup)| popup.rect().contains(x, y))
    }

    /// Scrolls list `list` to `to`, as far as it goes.
    pub(super) fn scroll_list(&mut self, list: List, to: i64) {
        let Some(popup) = self.list_popup(list) else {
            return;
        };
        let to = popup.scrolled(to).scroll();
        match list {
            List::Panel => self.panel_scroll = to,
            List::Choice => {
                if let Some(choosing) = &mut self.choosing {
                    choosing.scroll = to;
                }
            }
            List::Menu => {
                if let Some(menu) = &mut self.menu {
                    menu.scroll.0 = to;
                }
            }
            List::Submenu => {
                if let Some(menu) = &mut self.menu {
                    menu.scroll.1 = to;
                }
            }
        }
    }

    /// Scrolls list `list` so its row `row` shows whole.
    pub(super) fn reveal_row(&mut self, list: List, row: Option<usize>) {
        if let (Some(row), Some(popup)) = (row, self.list_popup(list)) {
            self.scroll_list(list, popup.reveal(row));
        }
    }

    /// The wheel over a list that's open: it scrolls by `dy`. Whether one
    /// was there to scroll.
    pub(super) fn wheel_list(&mut self, dy: i64) -> bool {
        let (x, y) = self.pointer;
        let Some((list, popup)) = self.list_at(x, y) else {
            return false;
        };
        self.scroll_list(list, popup.scroll() + dy);
        true
    }

    /// A press on a list's scroll bar, if it's on one: its thumb drags,
    /// and its track pages, as the inspector's does.
    pub(super) fn press_list_bar(&mut self, x: i64, y: i64) -> bool {
        let Some((list, popup)) = self.list_at(x, y) else {
            return false;
        };
        let Some(hit) = popup.bar_hit(x, y) else {
            return false;
        };
        let page = popup.body().height - self.res.sizes.menu_item.units();
        match hit {
            BarHit::Thumb => {
                self.drag = Some(Drag::List {
                    list,
                    y,
                    from: popup.scroll(),
                });
            }
            BarHit::Before => self.scroll_list(list, popup.scroll() - page),
            BarHit::After => self.scroll_list(list, popup.scroll() + page),
        }
        true
    }

    /// A list's thumb dragged to `y`, from where it was pressed at `from_y`
    /// when the list was scrolled `from`.
    pub(super) fn drag_list(&mut self, list: List, from_y: i64, from: i64, y: i64) {
        if let Some(popup) = self.list_popup(list) {
            self.scroll_list(list, popup.dragged(from, y - from_y));
        }
    }

    /// Opens the list of the inspector's choice `at`, of which `selected`
    /// is chosen, scrolled to show that one in its middle.
    pub(super) fn open_choice(&mut self, at: ControlRef, selected: Option<usize>) {
        self.choosing = Some(Choosing {
            at,
            lit: selected,
            scroll: 0,
        });
        self.reveal_control(at);
        if let (Some(i), Some(popup)) = (selected, self.list_popup(List::Choice)) {
            self.scroll_list(List::Choice, popup.centred(i));
        }
    }

    /// A key while an inspector's choice is open as a list: the arrows,
    /// Page Up and Down, Home and End light another, scrolled into view;
    /// Enter chooses it; Escape closes the list.
    pub(super) fn choice_key(&mut self, key: Key, shift: bool) -> Vec<Effect> {
        let model = part::Part::inspector(&self.editor);
        let Some(Choosing { at, lit, .. }) = self.choosing else {
            return Vec::new();
        };
        let count = match Window::control(&model, self.tab, at).map(|c| &c.kind) {
            Some(ControlKind::Choice { choices, .. }) => choices.len(),
            _ => 0,
        };
        let last = count.saturating_sub(1);
        let page = self.list_popup(List::Choice).map_or(1, |p| p.page());
        let to = match key {
            Key::Down => lit.map_or(0, |l| (l + 1).min(last)),
            Key::Up => lit.map_or(0, |l| l.saturating_sub(1)),
            Key::PageDown => lit.map_or(0, |l| (l + page).min(last)),
            Key::PageUp => lit.map_or(0, |l| l.saturating_sub(page)),
            Key::Home => 0,
            Key::End => last,
            Key::Enter => {
                if let Some(l) = lit {
                    return self.choose(&model, at, l, shift);
                }
                self.choosing = None;
                return Vec::new();
            }
            Key::Escape => {
                self.choosing = None;
                return Vec::new();
            }
            _ => return Vec::new(),
        };
        if count > 0
            && let Some(choosing) = &mut self.choosing
        {
            choosing.lit = Some(to);
            self.reveal_row(List::Choice, Some(to));
        }
        Vec::new()
    }

    /// The list open from the inspector as screen readers see it, if one
    /// is, its nodes added to `nodes`: a list box of its choices, each
    /// where it's drawn as scrolled and hidden where it's scrolled out of
    /// sight, the one chosen selected and the one lit its active one.
    pub(super) fn list_access(
        &self,
        nodes: &mut Vec<(part::accesskit::NodeId, part::accesskit::Node)>,
    ) -> Option<part::accesskit::NodeId> {
        use part::accesskit::{Node, NodeId, Rect as Bounds, Role};
        let model = self.inspector_model();
        let choosing = self.choosing?;
        let (popup, rows) = self.choice_popup(&model)?;
        let px = |v: i64| v as f64 / text::units::PIXEL as f64;
        let bounds = |r: Rect| Bounds {
            x0: px(r.x),
            y0: px(r.y),
            x1: px(r.right()),
            y1: px(r.bottom()),
        };
        // Numbered from here, beside the inspector's, which start at
        // 1 << 16, and below the document's, at 1 << 20.
        let first = 1u64 << 18;
        let list_id = NodeId(first);
        let mut list = Node::new(Role::ListBox);
        let label = Window::control(&model, self.tab, choosing.at)
            .and_then(|_| model.tabs.get(self.tab))
            .and_then(|t| t.sections.get(choosing.at.section))
            .and_then(|s| s.rows.get(choosing.at.row))
            .map(|r| r.label.clone())
            .unwrap_or_default();
        if !label.is_empty() {
            list.set_label(label);
        }
        list.set_bounds(bounds(popup.rect()));
        list.set_clips_children();
        list.set_scroll_y(px(popup.scroll()));
        list.set_scroll_y_min(0.0);
        list.set_scroll_y_max(px(popup.most()));
        let mut children = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let id = NodeId(first + 1 + i as u64);
            let mut node = Node::new(Role::ListBoxOption);
            node.set_label(row.title.clone());
            node.set_selected(row.checked);
            if let Some(r) = popup.row(i) {
                node.set_bounds(bounds(r));
            }
            if !popup.shows(i) {
                node.set_hidden();
            }
            if choosing.lit == Some(i) {
                list.set_active_descendant(id);
            }
            children.push(id);
            nodes.push((id, node));
        }
        list.set_children(children);
        nodes.push((list_id, list));
        Some(list_id)
    }
}
