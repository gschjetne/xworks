//! The title bar xWorks draws in the generic shell (project owner,
//! 2026-10-22, review 3ce247c2): the menus, the document's name, the
//! contextual buttons and the window's own buttons. What's left of it
//! moves the window.

use chrome::{Control, ControlKind, MenuBar};
use scene::{Color, Item};
use text::units::PIXEL;

use crate::kit::{field_box, fill, px, rounded};
use crate::{Field, Kit, Rect};

/// What the title bar shows.
#[derive(Clone, Copy, Debug)]
pub struct TitleModel<'a> {
    /// The menus.
    pub menus: &'a MenuBar,
    /// The menu that's open, its title lit.
    pub open: Option<usize>,
    /// Whether the menus' access keys are shown, as they are while Alt is
    /// held or the menus have the keyboard.
    pub access: bool,
    /// The document's name.
    pub name: &'a str,
    /// The name as it's being typed, while the document is renamed.
    pub renaming: Option<&'a Field>,
    /// The contextual buttons, right to left from the window's own: the
    /// inspector's, then Draft and Pages.
    pub buttons: &'a [Control],
    /// Whether the window fills the screen, so maximising restores it.
    pub maximized: bool,
}

/// What a point in the title bar hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleHit {
    /// A menu's title, by index.
    Menu(usize),
    /// The document's name.
    Name,
    /// A contextual button, by index, and for one of segments, which.
    Button(usize, usize),
    /// The window's minimise button.
    Minimize,
    /// The window's maximise or restore button.
    Maximize,
    /// The window's close button.
    Close,
    /// Anywhere else, which moves the window.
    Drag,
}

/// The title bar laid out.
#[derive(Clone, Debug)]
pub struct TitleBar {
    width: i64,
    height: i64,
    /// Each menu title's hit area, and where its text starts.
    menus: Vec<(Rect, i64)>,
    name: Rect,
    /// Each contextual button's area, and its segments' if it has them.
    buttons: Vec<(Rect, Vec<Rect>)>,
    /// Minimise, maximise and close.
    window: [Rect; 3],
}

impl TitleBar {
    /// `model` laid out across a window `width` wide.
    pub fn new(kit: &Kit<'_>, model: &TitleModel<'_>, width: i64) -> TitleBar {
        let s = &kit.theme.sizes;
        let height = s.title_bar.units();
        let text = kit.ty(s.text, 400);
        let gap = s.menu_gap.units();
        let mut x = s.menu_left.units();
        let mut menus = Vec::new();
        for menu in &model.menus.menus {
            let w = kit.width(&menu.title, text);
            menus.push((Rect::new(x - gap / 2, 0, w + gap, height), x));
            x += w + gap;
        }
        let menus_end = x - gap / 2;
        let wb = s.window_button.units();
        let window = [
            Rect::new(width - 3 * wb, 0, wb, height),
            Rect::new(width - 2 * wb, 0, wb, height),
            Rect::new(width - wb, 0, wb, height),
        ];
        // The contextual buttons, from the right.
        let mut right = width - 3 * wb - s.contextual_gap.units();
        let mut buttons = vec![(Rect::default(), Vec::new()); model.buttons.len()];
        for (i, control) in model.buttons.iter().enumerate() {
            let (rect, segments) = match &control.kind {
                ControlKind::Segments { segments, .. } => {
                    let (w, h) = (s.segmented.0.units(), s.segmented.1.units());
                    let rect = Rect::new(right - w, (height - h) / 2, w, h);
                    let n = segments.len().max(1) as i64;
                    let parts = (0..n)
                        .map(|k| Rect::new(rect.x + rect.width * k / n, rect.y, rect.width / n, h))
                        .collect();
                    (rect, parts)
                }
                _ => {
                    let b = s.button.units();
                    (Rect::new(right - b, (height - b) / 2, b, b), Vec::new())
                }
            };
            right = rect.x - px(10.0);
            buttons[i] = (rect, segments);
        }
        let contextual = right + px(10.0) - s.contextual_gap.units();
        // The name, centred, if it fits between the menus and the buttons.
        let name_ty = kit.ty(s.text, 500);
        let nw = kit.width(model.name, name_ty).max(px(80.0));
        let mut nx = (width - nw) / 2;
        if nx < menus_end + gap {
            nx = menus_end + gap;
        }
        let name = if nx + nw <= contextual {
            Rect::new(nx, 0, nw, height)
        } else {
            Rect::default()
        };
        TitleBar {
            width,
            height,
            menus,
            name,
            buttons,
            window,
        }
    }

    /// Its height.
    pub fn height(&self) -> i64 {
        self.height
    }

    /// Where menu `i`'s title is: its hit area.
    pub fn menu(&self, i: usize) -> Option<Rect> {
        self.menus.get(i).map(|m| m.0)
    }

    /// Where the document's name is.
    pub fn name(&self) -> Rect {
        self.name
    }

    /// What (`x`, `y`) hits, if it's in the bar.
    pub fn hit(&self, x: i64, y: i64) -> Option<TitleHit> {
        if !(0..self.height).contains(&y) || !(0..self.width).contains(&x) {
            return None;
        }
        if let Some(i) = self.menus.iter().position(|(r, _)| r.contains(x, y)) {
            return Some(TitleHit::Menu(i));
        }
        let hits = [TitleHit::Minimize, TitleHit::Maximize, TitleHit::Close];
        for (r, hit) in self.window.iter().zip(hits) {
            if r.contains(x, y) {
                return Some(hit);
            }
        }
        for (i, (rect, segments)) in self.buttons.iter().enumerate() {
            if rect.contains(x, y) {
                let k = segments.iter().position(|s| s.contains(x, y)).unwrap_or(0);
                return Some(TitleHit::Button(i, k));
            }
        }
        if self.name.contains(x, y) {
            return Some(TitleHit::Name);
        }
        Some(TitleHit::Drag)
    }

    /// The bar drawn, lighting what `hover` hits.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        model: &TitleModel<'_>,
        hover: Option<TitleHit>,
    ) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let mut out = Vec::new();
        fill(&mut out, Rect::new(0, 0, self.width, self.height), c.chrome);
        let text = kit.ty(s.text, 400);
        let r = s.radius.units();
        for (i, (menu, (rect, x))) in model.menus.menus.iter().zip(&self.menus).enumerate() {
            if model.open == Some(i) || (model.open.is_some() && hover == Some(TitleHit::Menu(i))) {
                let h = px(26.0);
                let lit = Rect::new(
                    rect.x + px(3.0),
                    (self.height - h) / 2,
                    rect.width - px(6.0),
                    h,
                );
                rounded(&mut out, lit, r, c.hover);
            }
            kit.text(&mut out, &menu.title, *x, 0, self.height, text, c.ink);
            if model.access
                && let Some(key) = menu.access
                && let Some(at) = menu
                    .title
                    .char_indices()
                    .find(|(_, ch)| ch.to_lowercase().next() == Some(key))
                    .map(|(i, _)| i)
            {
                kit.underline(&mut out, &menu.title, at, *x, 0, self.height, text, c.ink);
            }
        }
        // The name, or the field it's typed in.
        let name_ty = kit.ty(s.text, 500);
        if self.name.width > 0 {
            match model.renaming {
                Some(field) => {
                    let h = s.field.units() - px(2.0);
                    let layout = kit.layout(field.text(), name_ty);
                    let w = (layout.lines.first().map_or(0, |l| l.width) + px(24.0)).max(px(160.0));
                    let rect = Rect::new(
                        self.name.x + self.name.width / 2 - w / 2,
                        (self.height - h) / 2,
                        w,
                        h,
                    );
                    field_box(&mut out, c, rect, r - PIXEL);
                    let x = rect.x + px(12.0);
                    let y = (self.height - layout.height) / 2;
                    field.draw(
                        &mut out,
                        &layout,
                        (x, y),
                        kit.caret.then_some(c.ink),
                        c.selection,
                    );
                    kit.text(&mut out, field.text(), x, 0, self.height, name_ty, c.ink);
                }
                None => {
                    let w = kit.width(model.name, name_ty);
                    let x = self.name.x + (self.name.width - w) / 2;
                    if hover == Some(TitleHit::Name) {
                        let h = px(26.0);
                        let lit = Rect::new(x - px(8.0), (self.height - h) / 2, w + px(16.0), h);
                        rounded(&mut out, lit, r, c.hover);
                    }
                    kit.text(
                        &mut out,
                        model.name,
                        x,
                        0,
                        self.height,
                        name_ty,
                        c.secondary,
                    );
                }
            }
        }
        // The contextual buttons.
        for (i, (control, (rect, segments))) in model.buttons.iter().zip(&self.buttons).enumerate()
        {
            match &control.kind {
                ControlKind::Segments {
                    segments: names,
                    selected,
                } => {
                    segmented(kit, &mut out, *rect, segments, names, *selected);
                }
                ControlKind::Toggle { icon, on, .. } => {
                    let lit = *on || hover == Some(TitleHit::Button(i, 0));
                    if lit {
                        rounded(&mut out, *rect, r, c.hover);
                    }
                    let (cx, cy) = rect.centre();
                    kit.icon(&mut out, icon, cx, cy, px(16.0), c.ink);
                }
                ControlKind::Button { label, icon } => {
                    if hover == Some(TitleHit::Button(i, 0)) {
                        rounded(&mut out, *rect, r, c.hover);
                    }
                    let (cx, cy) = rect.centre();
                    match icon {
                        Some(icon) => kit.icon(&mut out, icon, cx, cy, px(16.0), c.ink),
                        None => {
                            let ty = kit.ty(s.label, 500);
                            let w = kit.width(label, ty);
                            kit.text(&mut out, label, cx - w / 2, rect.y, rect.height, ty, c.ink);
                        }
                    }
                }
                _ => {}
            }
        }
        // The window's own buttons: minimise, maximise or restore, close.
        let names = [
            "minus",
            if model.maximized { "copy" } else { "square" },
            "x",
        ];
        let hits = [TitleHit::Minimize, TitleHit::Maximize, TitleHit::Close];
        for ((rect, name), hit) in self.window.iter().zip(names).zip(hits) {
            let mut ink = c.ink;
            if hover == Some(hit) {
                let lit = if hit == TitleHit::Close {
                    c.danger
                } else {
                    c.hover
                };
                fill(&mut out, *rect, lit);
                if hit == TitleHit::Close {
                    ink = Color::WHITE;
                }
            }
            let size = if name == "x" || name == "minus" {
                px(16.0)
            } else {
                px(13.0)
            };
            let (cx, cy) = rect.centre();
            kit.icon(&mut out, name, cx, cy, size, ink);
        }
        fill(
            &mut out,
            Rect::new(0, self.height - PIXEL, self.width, PIXEL),
            c.line,
        );
        out
    }
}

/// Segments side by side as one control, the one `selected` raised in a
/// field's box: Draft and Pages, Text and Page.
pub(crate) fn segmented(
    kit: &Kit<'_>,
    out: &mut Vec<Item>,
    rect: Rect,
    parts: &[Rect],
    segments: &[chrome::Segment],
    selected: Option<usize>,
) {
    let c = &kit.theme.colors;
    let r = kit.theme.sizes.radius.units();
    rounded(out, rect, r, c.hover);
    if let Some(on) = selected.and_then(|k| parts.get(k)) {
        let raised = Rect::new(
            on.x + px(2.0),
            on.y + px(2.0),
            on.width - px(4.0),
            on.height - px(4.0),
        );
        field_box(out, c, raised, r - PIXEL);
    }
    for (k, (part, segment)) in parts.iter().zip(segments).enumerate() {
        let color = if selected == Some(k) {
            c.ink
        } else {
            c.secondary
        };
        let (cx, cy) = part.centre();
        match &segment.icon {
            Some(icon) => kit.icon(out, icon, cx, cy, px(15.0), color),
            None => {
                let ty = kit.ty(kit.theme.sizes.label, 500);
                let w = kit.width(&segment.label, ty);
                kit.text(
                    out,
                    &segment.label,
                    cx - w / 2,
                    part.y,
                    part.height,
                    ty,
                    color,
                );
            }
        }
    }
}
