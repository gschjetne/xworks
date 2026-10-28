//! The menus, from the pointer and the keyboard: a title clicked opens its
//! menu, and the pointer moves between them; on Windows and Linux, Alt or
//! F10 goes to the menus and their access keys show, Alt and a letter
//! opens a menu, a letter picks an item, and the arrows, Enter and Escape
//! move about them as each platform's menus do.

use chrome::{Key, MenuBar, MenuItem, Modifiers};
use scene::Item;
use text::units::PIXEL;
use toolkit::{Kit, Popup, PopupRow, TitleBar};

use super::Window;
use crate::Effect;

/// A menu with the keyboard or open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MenuState {
    /// Which menu.
    pub index: usize,
    /// Whether its list is open, or only its title lit, as Alt leaves it.
    pub open: bool,
    /// Its item that's lit.
    pub lit: Option<usize>,
    /// Its submenu that's open: the item it hangs from, and its own item
    /// that's lit.
    pub sub: Option<(usize, Option<usize>)>,
    /// Whether the keyboard opened it, so its access keys show.
    pub keyboard: bool,
    /// How far its list, and its submenu's, are scrolled, where they're
    /// taller than the window.
    pub scroll: (i64, i64),
}

/// The first item of `items` that can be chosen, from `from` on in the
/// way `step` goes, round the end.
fn next_enabled(items: &[MenuItem], from: Option<usize>, forward: bool) -> Option<usize> {
    let n = items.len();
    if n == 0 {
        return None;
    }
    let mut i = from.unwrap_or(if forward { n - 1 } else { 0 });
    for _ in 0..n {
        i = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        if items[i].enabled() {
            return Some(i);
        }
    }
    None
}

impl Window {
    /// Gives the menus the keyboard, with menu `index` lit, and open if
    /// `open`; their access keys show.
    pub(super) fn focus_menus(&mut self, index: usize, open: bool) {
        let bar = self.menu_bar();
        let lit = if open {
            bar.menus
                .get(index)
                .and_then(|m| next_enabled(&m.items, None, true))
        } else {
            None
        };
        self.menu = Some(MenuState {
            index,
            open,
            lit,
            sub: None,
            keyboard: true,
            scroll: (0, 0),
        });
        self.access = true;
    }

    /// Opens menu `index` from the pointer, or closes it if it's open.
    pub(super) fn open_menu(&mut self, index: usize, keyboard: bool) {
        if self.menu.is_some_and(|m| m.index == index && m.open) {
            self.menu = None;
            self.access = false;
            return;
        }
        self.menu = Some(MenuState {
            index,
            open: true,
            lit: None,
            sub: None,
            keyboard,
            scroll: (0, 0),
        });
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.access = false;
    }

    /// The open menu's list, and its submenu's, laid out and scrolled.
    pub(super) fn popups(
        &self,
        kit: &Kit<'_>,
        bar: &MenuBar,
        title: &TitleBar,
    ) -> Option<(Popup, Option<Popup>)> {
        let state = self.menu?;
        if !state.open {
            return None;
        }
        let menu = bar.menus.get(state.index)?;
        let at = title.menu(state.index)?;
        let window = (self.width, self.height);
        let rows = PopupRow::of_menu(&menu.items);
        let main =
            Popup::new(kit, &rows, (at.x, title.height()), 0, window).scrolled(state.scroll.0);
        let sub = state.sub.and_then(|(item, _)| {
            let MenuItem::Submenu { items, .. } = menu.items.get(item)? else {
                return None;
            };
            let row = main.row(item)?;
            let rows = PopupRow::of_menu(items);
            Some(
                Popup::new(
                    kit,
                    &rows,
                    (main.rect().right() - 4 * PIXEL, row.y - 5 * PIXEL),
                    0,
                    window,
                )
                .scrolled(state.scroll.1),
            )
        });
        Some((self.lit_thumb(main), sub.map(|s| self.lit_thumb(s))))
    }

    /// The open menus drawn.
    pub(super) fn draw_menus(&self, kit: &Kit<'_>, bar: &MenuBar, title: &TitleBar) -> Vec<Item> {
        let mut out = Vec::new();
        let (Some(state), Some((main, sub))) = (self.menu, self.popups(kit, bar, title)) else {
            return out;
        };
        let menu = &bar.menus[state.index];
        let access = state.keyboard && !self.res.mac;
        out.extend(main.draw(kit, &PopupRow::of_menu(&menu.items), state.lit, access));
        if let (Some(sub), Some((item, lit))) = (sub, state.sub)
            && let Some(MenuItem::Submenu { items, .. }) = menu.items.get(item)
        {
            out.extend(sub.draw(kit, &PopupRow::of_menu(items), lit, access));
        }
        out
    }

    /// Chooses `item` of the open menu, or of its submenu if `in_sub`: runs
    /// a command, or opens a submenu.
    fn activate(&mut self, item: usize, in_sub: bool) -> Vec<Effect> {
        let bar = self.menu_bar();
        let Some(state) = self.menu else {
            return Vec::new();
        };
        let menu = &bar.menus[state.index];
        let chosen = if in_sub {
            state.sub.and_then(|(s, _)| match menu.items.get(s) {
                Some(MenuItem::Submenu { items, .. }) => items.get(item).cloned(),
                _ => None,
            })
        } else {
            menu.items.get(item).cloned()
        };
        match chosen {
            Some(MenuItem::Command {
                id,
                args,
                enabled: true,
                ..
            }) => {
                self.close_menus();
                self.run(&id, args)
            }
            Some(MenuItem::Submenu { items, .. }) if !in_sub && !items.is_empty() => {
                if let Some(state) = &mut self.menu {
                    state.lit = Some(item);
                    state.sub = Some((item, next_enabled(&items, None, true)));
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A key while the menus have the keyboard; the items it lights are
    /// scrolled into view.
    pub(super) fn menu_key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        let before = self.menu.map(|m| (m.index, m.sub.map(|(s, _)| s)));
        let effects = self.menu_key_moved(key, modifiers);
        if let Some(state) = self.menu {
            // Another menu or submenu opens at its top.
            if before != Some((state.index, state.sub.map(|(s, _)| s))) {
                if before.map(|(i, _)| i) != Some(state.index) {
                    self.scroll_list(super::List::Menu, 0);
                }
                self.scroll_list(super::List::Submenu, 0);
            }
            self.reveal_row(super::List::Menu, state.lit);
            self.reveal_row(super::List::Submenu, state.sub.and_then(|(_, l)| l));
        }
        effects
    }

    fn menu_key_moved(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        let bar = self.menu_bar();
        let Some(mut state) = self.menu else {
            return Vec::new();
        };
        let n = bar.menus.len().max(1);
        let items = &bar.menus[state.index].items;
        let sub_items = state.sub.and_then(|(s, _)| match items.get(s) {
            Some(MenuItem::Submenu { items, .. }) => Some(items.clone()),
            _ => None,
        });
        // A shortcut works with a menu open, and closes it.
        if modifiers.primary
            && let Some(command) = self.res.commands.for_key(key, modifiers, self.res.mac)
        {
            let id = command.id.clone();
            self.close_menus();
            return self.run(&id, Vec::new());
        }
        state.keyboard = true;
        self.access = !self.res.mac;
        match key {
            Key::Escape | Key::Alt | Key::F10 => {
                if state.sub.is_some() {
                    state.sub = None;
                } else if state.open && key == Key::Escape {
                    state.open = false;
                    state.lit = None;
                } else {
                    self.close_menus();
                    return Vec::new();
                }
            }
            Key::Left => {
                if state.sub.is_some() {
                    state.sub = None;
                } else {
                    state.index = (state.index + n - 1) % n;
                    state.lit = if state.open {
                        next_enabled(&bar.menus[state.index].items, None, true)
                    } else {
                        None
                    };
                }
            }
            Key::Right => {
                let on_sub = state
                    .lit
                    .and_then(|l| items.get(l))
                    .is_some_and(|i| matches!(i, MenuItem::Submenu { .. }));
                if state.open && state.sub.is_none() && on_sub {
                    let item = state.lit.unwrap_or(0);
                    self.menu = Some(state);
                    return self.activate(item, false);
                }
                state.sub = None;
                state.index = (state.index + 1) % n;
                state.lit = if state.open {
                    next_enabled(&bar.menus[state.index].items, None, true)
                } else {
                    None
                };
            }
            Key::Down | Key::Up => {
                let forward = key == Key::Down;
                if !state.open {
                    state.open = true;
                    state.lit = next_enabled(items, None, forward);
                } else if let (Some((s, lit)), Some(sub)) = (state.sub, &sub_items) {
                    state.sub = Some((s, next_enabled(sub, lit, forward)));
                } else {
                    state.lit = next_enabled(items, state.lit, forward);
                }
            }
            Key::Enter => {
                if !state.open {
                    state.open = true;
                    state.lit = next_enabled(items, None, true);
                } else if let Some((_, Some(lit))) = state.sub {
                    self.menu = Some(state);
                    return self.activate(lit, true);
                } else if let Some(lit) = state.lit {
                    self.menu = Some(state);
                    return self.activate(lit, false);
                }
            }
            Key::Char(c) => {
                let c = c.to_lowercase().next().unwrap_or(c);
                if !state.open {
                    if let Some(i) = bar.menus.iter().position(|m| m.access == Some(c)) {
                        self.menu = Some(state);
                        self.focus_menus(i, true);
                    }
                    return Vec::new();
                }
                if let Some(sub) = &sub_items
                    && let Some(i) = sub
                        .iter()
                        .position(|i| i.access() == Some(c) && i.enabled())
                {
                    self.menu = Some(state);
                    return self.activate(i, true);
                }
                if let Some(i) = items
                    .iter()
                    .position(|i| i.access() == Some(c) && i.enabled())
                {
                    self.menu = Some(state);
                    return self.activate(i, false);
                }
            }
            _ => {}
        }
        self.menu = Some(state);
        Vec::new()
    }

    /// A press while a menu is open: on its list or its submenu's, it
    /// chooses what it lands on; on a menu's title, the title bar takes
    /// it; elsewhere, the menus close and the press goes no further.
    pub(super) fn menu_press(&mut self, x: i64, y: i64) -> Option<Vec<Effect>> {
        let bar = self.menu_bar();
        let buttons = self.buttons();
        let name = self.editor.name();
        let title = self.title_bar(&bar, &buttons, &name);
        let kit = self.kit();
        if let Some((main, sub)) = self.popups(&kit, &bar, &title) {
            if let Some(hit) = sub.as_ref().and_then(|s| s.hit(x, y)) {
                return Some(hit.map_or_else(Vec::new, |row| self.activate(row, true)));
            }
            if let Some(hit) = main.hit(x, y) {
                return Some(hit.map_or_else(Vec::new, |row| self.activate(row, false)));
            }
        }
        if let Some(toolkit::TitleHit::Menu(_)) = title.hit(x, y) {
            return None;
        }
        self.close_menus();
        Some(Vec::new())
    }

    /// The pointer over the menus: another title opens its menu, and items
    /// light, a submenu's opening.
    pub(super) fn menu_hover(&mut self, x: i64, y: i64) {
        let bar = self.menu_bar();
        let buttons = self.buttons();
        let name = self.editor.name();
        let title = self.title_bar(&bar, &buttons, &name);
        let kit = self.kit();
        let popups = self.popups(&kit, &bar, &title);
        let Some(state) = &mut self.menu else {
            return;
        };
        if let Some(toolkit::TitleHit::Menu(i)) = title.hit(x, y) {
            if state.open && i != state.index {
                *state = MenuState {
                    index: i,
                    open: true,
                    lit: None,
                    sub: None,
                    keyboard: state.keyboard,
                    scroll: (0, 0),
                };
            }
            return;
        }
        let Some((main, sub)) = popups else {
            return;
        };
        if let (Some(Some(row)), Some((s, _))) = (sub.as_ref().and_then(|p| p.hit(x, y)), state.sub)
        {
            state.sub = Some((s, Some(row)));
            return;
        }
        if let Some(Some(row)) = main.hit(x, y) {
            state.lit = Some(row);
            let items = &bar.menus[state.index].items;
            // Another submenu opens at its top.
            if state.sub.is_some_and(|(s, _)| s != row) {
                state.scroll.1 = 0;
            }
            state.sub = match items.get(row) {
                Some(MenuItem::Submenu { items, .. }) if !items.is_empty() => Some((
                    row,
                    state
                        .sub
                        .and_then(|(s, l)| (s == row).then_some(l))
                        .flatten(),
                )),
                _ => None,
            };
        }
    }
}
