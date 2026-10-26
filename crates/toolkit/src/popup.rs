//! Lists that pop up over the window: a menu open from the bar, its
//! submenus, and the choices of an inspector's field.

use chrome::MenuItem;
use scene::{Color, Item};
use text::units::PIXEL;

use crate::kit::{fill, px, rounded, shadow, swatch};
use crate::{Kit, Rect};

/// The room a colour's swatch takes before a row's title.
const SWATCH: i64 = 21 * PIXEL;

/// A row of a popup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopupRow {
    /// What it says; empty for a line between groups.
    pub title: String,
    /// Its access key, lower case.
    pub access: Option<char>,
    /// Its shortcut, as it's shown.
    pub shortcut: Option<String>,
    /// Whether it can be chosen.
    pub enabled: bool,
    /// Whether it's on, with a check beside it.
    pub checked: bool,
    /// Whether it opens a submenu.
    pub submenu: bool,
    /// The font family its title is shown in, for a choice of families;
    /// the interface's for none.
    pub family: Option<String>,
    /// A colour shown before its title, sRGB, for a choice of colours.
    pub swatch: Option<[u8; 3]>,
}

impl PopupRow {
    /// A row for choice `title`, checked if it's the one set.
    pub fn choice(title: &str, checked: bool) -> PopupRow {
        PopupRow {
            title: title.to_string(),
            access: None,
            shortcut: None,
            enabled: true,
            checked,
            submenu: false,
            family: None,
            swatch: None,
        }
    }

    /// A row for an inspector's `choice`, checked if it's the one set:
    /// shown in its family, or with its swatch, if it has one.
    pub fn of_choice(choice: &chrome::Choice, checked: bool) -> PopupRow {
        PopupRow {
            family: choice.family.clone(),
            swatch: choice.swatch,
            ..PopupRow::choice(&choice.label, checked)
        }
    }

    /// The rows of a menu's `items`.
    pub fn of_menu(items: &[MenuItem]) -> Vec<PopupRow> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Command {
                    title,
                    access,
                    shortcut,
                    enabled,
                    checked,
                    ..
                } => PopupRow {
                    access: *access,
                    shortcut: shortcut.clone(),
                    enabled: *enabled,
                    ..PopupRow::choice(title, *checked)
                },
                MenuItem::Submenu {
                    title,
                    access,
                    items,
                } => PopupRow {
                    access: *access,
                    enabled: !items.is_empty(),
                    submenu: true,
                    ..PopupRow::choice(title, false)
                },
                MenuItem::Separator => PopupRow {
                    enabled: false,
                    ..PopupRow::choice("", false)
                },
            })
            .collect()
    }

    fn separator(&self) -> bool {
        self.title.is_empty()
    }
}

/// A popup laid out: its rectangle, and each row's.
#[derive(Clone, Debug)]
pub struct Popup {
    rect: Rect,
    rows: Vec<Rect>,
}

impl Popup {
    /// `rows` laid out from (`x`, `y`) down, at least `min_width` wide,
    /// moved back inside a window `width` by `height` where they'd run out
    /// of it.
    pub fn new(
        kit: &Kit<'_>,
        rows: &[PopupRow],
        (x, y): (i64, i64),
        min_width: i64,
        (width, height): (i64, i64),
    ) -> Popup {
        let s = &kit.theme.sizes;
        let pad = s.menu_padding.units();
        let text = kit.ty(s.text, 400);
        let label = kit.ty(s.label, 400);
        let check = px(22.0);
        let widest = rows
            .iter()
            .map(|row| {
                let shortcut = row
                    .shortcut
                    .as_ref()
                    .map_or(0, |k| kit.width(k, label) + px(28.0));
                let arrow = if row.submenu { px(24.0) } else { 0 };
                let swatch = if row.swatch.is_some() { SWATCH } else { 0 };
                let title = kit
                    .layout_in(&row.title, row.family.as_deref(), text)
                    .lines
                    .first()
                    .map_or(0, |l| l.width);
                title + shortcut + arrow + swatch
            })
            .max()
            .unwrap_or(0);
        let w = (widest + check + 2 * pad).max(min_width);
        let inner = px(5.0);
        let mut at = inner;
        let mut rects = Vec::with_capacity(rows.len());
        for row in rows {
            let h = if row.separator() {
                s.menu_separator.units()
            } else {
                s.menu_item.units()
            };
            rects.push(Rect::new(0, at, w, h));
            at += h;
        }
        let h = at + inner;
        let x = x.min(width - w).max(0);
        let y = if y + h > height {
            (height - h).max(0)
        } else {
            y
        };
        for r in &mut rects {
            r.x += x;
            r.y += y;
        }
        Popup {
            rect: Rect::new(x, y, w, h),
            rows: rects,
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Row `i`'s rectangle.
    pub fn row(&self, i: usize) -> Option<Rect> {
        self.rows.get(i).copied()
    }

    /// Which row (`x`, `y`) hits: `Some(None)` inside the popup but on no
    /// row, `None` outside it.
    pub fn hit(&self, x: i64, y: i64) -> Option<Option<usize>> {
        if !self.rect.contains(x, y) {
            return None;
        }
        Some(self.rows.iter().position(|r| r.contains(x, y)))
    }

    /// The popup drawn, row `lit` lit, with access keys underlined if
    /// `access`.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        rows: &[PopupRow],
        lit: Option<usize>,
        access: bool,
    ) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let mut out = Vec::new();
        let r = px(8.0);
        shadow(&mut out, c, self.rect, r);
        rounded(&mut out, self.rect, r, c.field_line);
        rounded(&mut out, self.rect.inset(PIXEL), r - PIXEL, c.field);
        let pad = s.menu_padding.units();
        let text = kit.ty(s.text, 400);
        let label = kit.ty(s.label, 400);
        let check = px(22.0);
        for (i, (rect, row)) in self.rows.iter().zip(rows).enumerate() {
            if row.separator() {
                fill(
                    &mut out,
                    Rect::new(
                        rect.x + pad,
                        rect.y + rect.height / 2,
                        rect.width - 2 * pad,
                        PIXEL,
                    ),
                    c.line,
                );
                continue;
            }
            if lit == Some(i) && row.enabled {
                let band = Rect::new(rect.x + px(5.0), rect.y, rect.width - px(10.0), rect.height);
                rounded(&mut out, band, px(5.0), c.hover);
            }
            let ink = if row.enabled { c.ink } else { c.disabled };
            let quiet = if row.enabled { c.secondary } else { c.disabled };
            let (_, cy) = rect.centre();
            if row.checked {
                kit.icon(&mut out, "check", rect.x + pad + px(7.0), cy, px(14.0), ink);
            }
            let mut x = rect.x + pad + check;
            if let Some([r, g, b]) = row.swatch {
                let side = px(14.0);
                swatch(
                    &mut out,
                    c,
                    Rect::new(x, cy - side / 2, side, side),
                    Color::rgb(r, g, b),
                );
                x += SWATCH;
            }
            kit.text_in(
                &mut out,
                &row.title,
                row.family.as_deref(),
                x,
                rect.y,
                rect.height,
                text,
                ink,
            );
            if access
                && let Some(key) = row.access
                && let Some(at) = row
                    .title
                    .char_indices()
                    .find(|(_, ch)| ch.to_lowercase().next() == Some(key))
                    .map(|(i, _)| i)
            {
                kit.underline(&mut out, &row.title, at, x, rect.y, rect.height, text, ink);
            }
            if let Some(shortcut) = &row.shortcut {
                kit.text_right(
                    &mut out,
                    shortcut,
                    rect.right() - pad,
                    rect.y,
                    rect.height,
                    label,
                    quiet,
                );
            }
            if row.submenu {
                kit.icon(
                    &mut out,
                    "chevron-right",
                    rect.right() - pad - px(6.0),
                    cy,
                    px(14.0),
                    quiet,
                );
            }
        }
        out
    }
}
