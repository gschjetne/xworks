//! Lists that pop up over the window: a menu open from the bar, its
//! submenus, and the choices of an inspector's field. One taller than the
//! room it has scrolls, with a scroll bar as the inspector's; what's
//! drawn, and what a point hits, follow the scroll.

use chrome::MenuItem;
use scene::{Color, Group, Item};
use text::units::PIXEL;

use crate::inspector::{BAR_REACH, BAR_WIDTH};
use crate::kit::{fill, px, rounded, shadow, swatch};
use crate::{BarHit, Kit, Rect};

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

/// A popup laid out: its rectangle, and each row's, as scrolled. A popup
/// taller than the room it has, the window's or that above or below its
/// field, takes that room and scrolls: its rows move under a fixed frame,
/// with a scroll bar along its right edge, as the inspector's.
#[derive(Clone, Debug)]
pub struct Popup {
    rect: Rect,
    /// Where its rows show: inside its frame's padding.
    body: Rect,
    /// Each row's rectangle, as scrolled: perhaps out of the body.
    rows: Vec<Rect>,
    /// How far its rows are scrolled down, and how far they can be.
    scroll: i64,
    most: i64,
    /// The scroll bar's track and thumb, where it scrolls.
    bar: Option<(Rect, Rect)>,
    /// Whether the thumb is drawn lit, as under the pointer.
    thumb_lit: bool,
}

/// The padding inside a popup's frame, above and below its rows.
const INNER: f64 = 5.0;

/// The narrowest a thumb is drawn, however long the list.
const SHORTEST_THUMB: f64 = 28.0;

/// What a popup's rows take: how wide they are, at least `min_width`, and
/// each one's height.
fn measure(kit: &Kit<'_>, rows: &[PopupRow], min_width: i64) -> (i64, Vec<i64>) {
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
    let heights = rows
        .iter()
        .map(|row| {
            if row.separator() {
                s.menu_separator.units()
            } else {
                s.menu_item.units()
            }
        })
        .collect();
    ((widest + check + 2 * pad).max(min_width), heights)
}

impl Popup {
    /// `rows` laid out from (`x`, `y`) down, at least `min_width` wide,
    /// moved back inside a window `width` by `height` where they'd run out
    /// of it; taller than the window, as tall as it, scrolling.
    pub fn new(
        kit: &Kit<'_>,
        rows: &[PopupRow],
        (x, y): (i64, i64),
        min_width: i64,
        (width, height): (i64, i64),
    ) -> Popup {
        let (w, heights) = measure(kit, rows, min_width);
        let whole = Popup::whole(&heights);
        let h = whole.min(height.max(0));
        let y = if y + h > height { height - h } else { y };
        Popup::placed(&heights, x, y.max(0), w, h, width)
    }

    /// `rows`, the list of a field at `field`, at least as wide as it: under
    /// it where they fit below it in a window `width` by `height`, over it
    /// where they fit above it, and otherwise in the larger of those
    /// rooms, as tall as it, scrolling.
    pub fn by_field(
        kit: &Kit<'_>,
        rows: &[PopupRow],
        field: Rect,
        (width, height): (i64, i64),
    ) -> Popup {
        let (w, heights) = measure(kit, rows, field.width);
        let whole = Popup::whole(&heights);
        let gap = 2 * PIXEL;
        let below = (height - field.bottom() - gap).max(0);
        let above = (field.y - gap).max(0);
        let (y, h) = if whole <= below || below >= above {
            (field.bottom() + gap, whole.min(below))
        } else {
            let h = whole.min(above);
            (field.y - gap - h, h)
        };
        Popup::placed(&heights, field.x, y, w, h, width)
    }

    /// How tall rows of `heights` are, with the frame's padding.
    fn whole(heights: &[i64]) -> i64 {
        heights.iter().sum::<i64>() + 2 * px(INNER)
    }

    /// Rows of `heights` in a popup at (`x`, `y`), `w` wide for its rows
    /// and `h` tall, kept inside a window `width` wide; wider by a scroll
    /// bar where they don't fit its height.
    fn placed(heights: &[i64], x: i64, y: i64, w: i64, h: i64, width: i64) -> Popup {
        let inner = px(INNER);
        let content: i64 = heights.iter().sum();
        let scrolls = content + 2 * inner > h;
        let full = if scrolls { w + px(BAR_REACH) } else { w };
        let x = x.min(width - full).max(0);
        let rect = Rect::new(x, y, full, h);
        let body = Rect::new(x, y + inner, full, (h - 2 * inner).max(0));
        let mut at = body.y;
        let rows = heights
            .iter()
            .map(|&rh| {
                let r = Rect::new(x, at, w, rh);
                at += rh;
                r
            })
            .collect();
        let most = (content - body.height).max(0);
        let mut popup = Popup {
            rect,
            body,
            rows,
            scroll: 0,
            most,
            bar: None,
            thumb_lit: false,
        };
        popup.place_bar();
        popup
    }

    /// The bar for the scroll as it is: a thumb as much of the track as the
    /// body shows of the rows, as far down it as they're scrolled.
    fn place_bar(&mut self) {
        let (body, most) = (self.body, self.most);
        self.bar = (most > 0).then(|| {
            let track = Rect::new(
                self.rect.right() - px(BAR_REACH),
                body.y,
                px(BAR_REACH),
                body.height,
            );
            let length = (track.height * body.height / (body.height + most))
                .max(px(SHORTEST_THUMB))
                .min(track.height);
            let thumb = Rect::new(
                track.x,
                track.y + (track.height - length) * self.scroll / most,
                track.width,
                length,
            );
            (track, thumb)
        });
    }

    /// It with its rows scrolled `scroll` down, as far as they go.
    pub fn scrolled(mut self, scroll: i64) -> Popup {
        let scroll = scroll.clamp(0, self.most);
        let by = scroll - self.scroll;
        for r in &mut self.rows {
            r.y -= by;
        }
        self.scroll = scroll;
        self.place_bar();
        self
    }

    /// It with its scroll bar's thumb drawn lit, or not, as under the
    /// pointer.
    pub fn thumb_lit(self, lit: bool) -> Popup {
        Popup {
            thumb_lit: lit,
            ..self
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Where its rows show.
    pub fn body(&self) -> Rect {
        self.body
    }

    /// How far its rows are scrolled down.
    pub fn scroll(&self) -> i64 {
        self.scroll
    }

    /// How far its rows can be scrolled down: 0 where they fit.
    pub fn most(&self) -> i64 {
        self.most
    }

    /// The scroll bar's track and thumb, where its rows don't fit.
    pub fn bar(&self) -> Option<(Rect, Rect)> {
        self.bar
    }

    /// Row `i`'s rectangle, as scrolled: perhaps out of the body.
    pub fn row(&self, i: usize) -> Option<Rect> {
        self.rows.get(i).copied()
    }

    /// Whether row `i` shows, wholly or in part.
    pub fn shows(&self, i: usize) -> bool {
        self.rows
            .get(i)
            .is_some_and(|r| r.bottom() > self.body.y && r.y < self.body.bottom())
    }

    /// How many rows a page of it is: as many as show whole, less one, and
    /// at least one.
    pub fn page(&self) -> usize {
        let row = self.rows.iter().map(|r| r.height).max().unwrap_or(1).max(1);
        (self.body.height / row - 1).max(1) as usize
    }

    /// How far to scroll so that row `i` shows whole: as it is, if it does.
    pub fn reveal(&self, i: usize) -> i64 {
        let Some(r) = self.row(i) else {
            return self.scroll;
        };
        let by = if r.y < self.body.y {
            r.y - self.body.y
        } else if r.bottom() > self.body.bottom() {
            r.bottom() - self.body.bottom()
        } else {
            0
        };
        (self.scroll + by).clamp(0, self.most)
    }

    /// How far to scroll so that row `i` shows in the body's middle, as far
    /// as the rows go: where a list opens on the row chosen.
    pub fn centred(&self, i: usize) -> i64 {
        let Some(r) = self.row(i) else {
            return self.scroll;
        };
        let (_, middle) = self.body.centre();
        let (_, at) = r.centre();
        (self.scroll + at - middle).clamp(0, self.most)
    }

    /// The scroll the thumb gives, dragged `dy` from where it was when the
    /// rows were scrolled `from`.
    pub fn dragged(&self, from: i64, dy: i64) -> i64 {
        let Some((track, thumb)) = self.bar else {
            return 0;
        };
        let free = track.height - thumb.height;
        if free <= 0 {
            return from;
        }
        (from + dy * self.most / free).clamp(0, self.most)
    }

    /// What (`x`, `y`) hits on the scroll bar, if it's there.
    pub fn bar_hit(&self, x: i64, y: i64) -> Option<BarHit> {
        let (track, thumb) = self.bar?;
        if !track.contains(x, y) {
            return None;
        }
        Some(if thumb.contains(x, y) {
            BarHit::Thumb
        } else if y < thumb.y {
            BarHit::Before
        } else {
            BarHit::After
        })
    }

    /// Which row (`x`, `y`) hits: `Some(None)` inside the popup but on no
    /// row, as on its scroll bar or a row scrolled out of sight, `None`
    /// outside it.
    pub fn hit(&self, x: i64, y: i64) -> Option<Option<usize>> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if !self.body.contains(x, y) {
            return Some(None);
        }
        Some(self.rows.iter().position(|r| r.contains(x, y)))
    }

    /// The popup drawn, row `lit` lit, with access keys underlined if
    /// `access`. Rows scrolled out of its body aren't drawn, and those
    /// partly out of it are cut at its edge.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        rows: &[PopupRow],
        lit: Option<usize>,
        access: bool,
    ) -> Vec<Item> {
        let c = &kit.theme.colors;
        let mut out = Vec::new();
        let r = px(8.0);
        shadow(&mut out, c, self.rect, r);
        rounded(&mut out, self.rect, r, c.field_line);
        rounded(&mut out, self.rect.inset(PIXEL), r - PIXEL, c.field);
        let body = self.body;
        let clip = scene::Path::rect(body.x, body.y, body.width, body.height);
        for (i, (rect, row)) in self.rows.iter().zip(rows).enumerate() {
            if !self.shows(i) {
                continue;
            }
            let drawn = Popup::draw_row(kit, *rect, row, lit == Some(i), access);
            if rect.y >= body.y && rect.bottom() <= body.bottom() {
                out.extend(drawn);
            } else {
                out.push(Item::Group(Group {
                    clip: Some(clip.clone()),
                    transform: scene::Transform::IDENTITY,
                    items: drawn,
                }));
            }
        }
        if let Some((_, thumb)) = self.bar {
            let w = px(BAR_WIDTH);
            let drawn = Rect::new(thumb.x + (thumb.width - w) / 2, thumb.y, w, thumb.height);
            let ink = if self.thumb_lit {
                c.secondary
            } else {
                c.disabled
            };
            rounded(&mut out, drawn, w / 2, ink);
        }
        out
    }

    /// `row` drawn at `rect`, lit if `lit`, its access key underlined if
    /// `access`.
    fn draw_row(kit: &Kit<'_>, rect: Rect, row: &PopupRow, lit: bool, access: bool) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let pad = s.menu_padding.units();
        let text = kit.ty(s.text, 400);
        let label = kit.ty(s.label, 400);
        let check = px(22.0);
        let mut out = Vec::new();
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
            return out;
        }
        if lit && row.enabled {
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
        out
    }
}
