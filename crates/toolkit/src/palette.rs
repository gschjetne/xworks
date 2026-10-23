//! The command palette: every command, found by name from the keyboard
//! (docs/ARCHITECTURE.md, section 8).

use chrome::Found;
use scene::Item;
use text::units::PIXEL;

use crate::kit::{fill, px, rounded, shadow};
use crate::{Field, Kit, Rect};

/// How many commands the palette shows at once.
pub const SHOWN: usize = 10;

/// What a point hits in the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteHit {
    /// A command found, by index among those shown.
    Row(usize),
    /// What's typed.
    Field,
    /// Somewhere else in the palette.
    Panel,
}

/// The palette laid out.
#[derive(Clone, Debug)]
pub struct PaletteView {
    rect: Rect,
    field: Rect,
    rows: Vec<Rect>,
}

impl PaletteView {
    /// A palette showing `found` commands (as many as fit), centred across
    /// a window `width` wide, below a title bar `top` tall.
    pub fn new(kit: &Kit<'_>, found: usize, width: i64, top: i64) -> PaletteView {
        let s = &kit.theme.sizes;
        let w = s.palette.units().min(width - px(24.0)).max(px(120.0));
        let (field_h, row) = (s.palette_field.units(), s.palette_row.units());
        let x = (width - w) / 2;
        let y = top + s.palette_top.units();
        let n = found.min(SHOWN) as i64;
        let h = field_h
            + if n > 0 {
                px(6.0) + n * row + px(4.0)
            } else {
                0
            };
        let rows = (0..n)
            .map(|i| {
                Rect::new(
                    x + px(6.0),
                    y + field_h + px(6.0) + i * row,
                    w - px(12.0),
                    row,
                )
            })
            .collect();
        PaletteView {
            rect: Rect::new(x, y, w, h),
            field: Rect::new(x, y, w, field_h),
            rows,
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// What (`x`, `y`) hits, if it's in the palette.
    pub fn hit(&self, x: i64, y: i64) -> Option<PaletteHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if self.field.contains(x, y) {
            return Some(PaletteHit::Field);
        }
        Some(
            self.rows
                .iter()
                .position(|r| r.contains(x, y))
                .map_or(PaletteHit::Panel, PaletteHit::Row),
        )
    }

    /// The palette drawn: what's typed in `field`, and the commands
    /// `found`, from `first`, with `selected` lit.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        field: &Field,
        found: &[Found],
        first: usize,
        selected: usize,
    ) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let mut out = Vec::new();
        let r = px(12.0);
        shadow(&mut out, c, self.rect, r);
        rounded(&mut out, self.rect, r, c.field_line);
        rounded(&mut out, self.rect.inset(PIXEL), r - PIXEL, c.field);
        let f = self.field;
        kit.icon(
            &mut out,
            "search",
            f.x + px(24.0),
            f.y + f.height / 2,
            px(17.0),
            c.secondary,
        );
        let typed = kit.ty(s.palette_text, 400);
        let layout = kit.layout(field.text(), typed);
        let x = f.x + px(44.0);
        let y = f.y + (f.height - layout.height) / 2;
        field.draw(
            &mut out,
            &layout,
            (x, y),
            kit.caret.then_some(c.ink),
            c.selection,
        );
        kit.text(&mut out, field.text(), x, f.y, f.height, typed, c.ink);
        if !self.rows.is_empty() {
            fill(
                &mut out,
                Rect::new(f.x + PIXEL, f.bottom(), f.width - 2 * PIXEL, PIXEL),
                c.line,
            );
        }
        let strong = kit.ty(s.palette_item, 500);
        let quiet = kit.ty(s.value, 400);
        for (i, (row, found)) in self.rows.iter().zip(found.iter().skip(first)).enumerate() {
            if first + i == selected {
                rounded(&mut out, *row, px(7.0), c.hover);
            }
            let nx = row.x + px(12.0);
            let nw = kit.text(&mut out, &found.title, nx, row.y, row.height, strong, c.ink);
            kit.text(
                &mut out,
                &found.kind,
                nx + nw + px(10.0),
                row.y,
                row.height,
                quiet,
                c.secondary,
            );
            if let Some(keys) = &found.shortcut {
                kit.text_right(
                    &mut out,
                    keys,
                    row.right() - px(12.0),
                    row.y,
                    row.height,
                    quiet,
                    c.secondary,
                );
            }
        }
        out
    }
}
