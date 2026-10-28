//! The inspector: a side panel of the selection's properties, as the app
//! describes them ([`chrome::Inspector`]), in tabs of sections of rows of
//! typed controls. A tab taller than the panel scrolls under its tabs,
//! which stay put, with a scroll bar at the panel's edge; what's drawn,
//! and what a point hits, follow the scroll.

use chrome::{ControlKind, Inspector};
use scene::{Color, Group, Item};
use text::units::PIXEL;

use crate::kit::{field_box, fill, px, rounded, swatch};
use crate::title::segmented;
use crate::{Field, Kit, Rect};

/// A control of the inspector, by where it is: its section, row, and
/// place in the row, in the tab shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ControlRef {
    /// The section.
    pub section: usize,
    /// The row in it.
    pub row: usize,
    /// The control in the row.
    pub control: usize,
}

/// What a point in the inspector hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorHit {
    /// A tab's title, by index.
    Tab(usize),
    /// A control, and for one of segments, which segment.
    Control(ControlRef, usize),
    /// The scroll bar, where the tab shown is taller than the panel.
    Bar(BarHit),
    /// Somewhere else in the panel.
    Panel,
}

/// What a point on the scroll bar hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarHit {
    /// The thumb, which a drag moves.
    Thumb,
    /// The track above the thumb: a page up.
    Before,
    /// The track below it: a page down.
    After,
}

/// The inspector laid out.
#[derive(Clone, Debug)]
pub struct InspectorView {
    rect: Rect,
    tabs: Rect,
    tab_parts: Vec<Rect>,
    /// Where the tab's sections show, under its tabs: what's scrolled
    /// past its edges is cut off.
    body: Rect,
    /// How far the sections are scrolled, and how far they can be.
    scroll: i64,
    most: i64,
    /// Section headings: the title and where it goes.
    headings: Vec<(String, Rect)>,
    /// Each row: its section and place in it, and where it is.
    rows: Vec<((usize, usize), Rect)>,
    /// Row labels and where they go, by section and row.
    labels: Vec<(String, Rect, (usize, usize))>,
    /// Each control: where it is, its own area, and its segments' areas.
    controls: Vec<(ControlRef, Rect, Vec<Rect>)>,
    /// Boxes drawn behind runs of toggles, which share one, by section and
    /// row.
    groups: Vec<(Rect, (usize, usize))>,
    /// The scroll bar's track and thumb, where it scrolls.
    bar: Option<(Rect, Rect)>,
}

/// How wide the strip at the panel's edge is where the scroll bar takes
/// the pointer, and the bar drawn in it.
pub(crate) const BAR_REACH: f64 = 14.0;
pub(crate) const BAR_WIDTH: f64 = 6.0;

impl InspectorView {
    /// `model`'s tab `tab` laid out in a panel from `top` to `bottom`
    /// along the right of a window `width` wide, its sections scrolled
    /// `scroll` down, as far as they go.
    pub fn new(
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        width: i64,
        (top, bottom): (i64, i64),
        scroll: i64,
    ) -> InspectorView {
        let s = &kit.theme.sizes;
        let panel = s.inspector.units();
        let x0 = width - panel;
        let rect = Rect::new(x0, top, panel, (bottom - top).max(0));
        let pad = s.inspector_padding.units();
        let label_w = s.inspector_label.units();
        let row_h = s.inspector_row.units();
        let field_h = s.field.units();
        let fx = x0 + pad + label_w;
        let fw = panel - 2 * pad - label_w;
        let gap = px(8.0);
        let note_ty = kit.ty(s.label, 400);
        let tabs = Rect::new(x0 + pad, top + px(12.0), panel - 2 * pad, field_h);
        let n = model.tabs.len().max(1) as i64;
        let tab_parts = (0..n)
            .map(|k| {
                Rect::new(
                    tabs.x + tabs.width * k / n,
                    tabs.y,
                    tabs.width / n,
                    tabs.height,
                )
            })
            .collect();
        let body_top = (top + px(44.0)).min(rect.bottom());
        let body = Rect::new(x0, body_top, panel, rect.bottom() - body_top);
        let mut y = body.y + px(12.0);
        let mut headings = Vec::new();
        let mut rows = Vec::new();
        let mut labels = Vec::new();
        let mut controls = Vec::new();
        let mut groups = Vec::new();
        let sections = model.tabs.get(tab).map_or(&[][..], |t| &t.sections[..]);
        for (si, section) in sections.iter().enumerate() {
            headings.push((
                section.title.to_uppercase(),
                Rect::new(x0 + pad, y, fw + label_w, px(20.0)),
            ));
            y += px(26.0);
            for (ri, row) in section.rows.iter().enumerate() {
                let plain = row.controls.iter().all(|c| {
                    matches!(
                        c.kind,
                        ControlKind::Choice { plain: true, .. }
                            | ControlKind::Length { plain: true, .. }
                            | ControlKind::Button { .. }
                            | ControlKind::Note { .. }
                    )
                });
                let h = if plain { px(24.0) } else { field_h };
                let step = if plain { px(26.0) } else { row_h };
                // Where the controls go: beside the label, or from the
                // panel's edge where there's none.
                let (mut x, mut room) = (fx, fw);
                rows.push(((si, ri), Rect::new(x0, y, panel, h)));
                if !row.label.is_empty() {
                    labels.push((
                        row.label.clone(),
                        Rect::new(x0 + pad, y, label_w, h),
                        (si, ri),
                    ));
                }
                // Buttons with icons take a small square at the right.
                let icons = row
                    .controls
                    .iter()
                    .filter(|c| matches!(c.kind, ControlKind::Button { icon: Some(_), .. }))
                    .count() as i64;
                room -= icons * (h + gap);
                // Runs of toggles share a cell; the rest a cell each.
                let mut cells: Vec<Vec<usize>> = Vec::new();
                for (ci, c) in row.controls.iter().enumerate() {
                    match c.kind {
                        ControlKind::Button { icon: Some(_), .. } => {}
                        ControlKind::Toggle { .. }
                            if cells.last().is_some_and(|cell| {
                                matches!(row.controls[cell[0]].kind, ControlKind::Toggle { .. })
                            }) =>
                        {
                            cells.last_mut().expect("a cell").push(ci);
                        }
                        _ => cells.push(vec![ci]),
                    }
                }
                let mut cell_rects = Vec::new();
                let shared_cells = if row.label.is_empty() && cells.len() > 1 {
                    // The first sits where a label would.
                    cell_rects.push(Rect::new(x0 + pad, y, label_w, h));
                    &cells[1..]
                } else {
                    if row.label.is_empty() {
                        x = x0 + pad;
                        room += label_w;
                    }
                    &cells[..]
                };
                // A note beside other controls, such as where a value
                // comes from, takes the room its words need, up to half
                // the row; the others share the rest alike.
                let notes: Vec<Option<i64>> = shared_cells
                    .iter()
                    .map(|cell| match &row.controls[cell[0]].kind {
                        ControlKind::Note { text } if cell.len() == 1 => {
                            Some(kit.width(text, note_ty).min(room / 2))
                        }
                        _ => None,
                    })
                    .collect();
                let n = shared_cells.len().max(1) as i64;
                let flexible = notes.iter().filter(|w| w.is_none()).count() as i64;
                let fixed: i64 = notes.iter().flatten().sum();
                let shared = if flexible == 0 {
                    None
                } else {
                    Some((room - gap * (n - 1) - fixed) / flexible)
                };
                let mut at = x;
                for note in &notes {
                    let w = match (shared, *note) {
                        (Some(shared), Some(note)) => note.max(0).min(room - shared),
                        (Some(shared), None) => shared,
                        (None, _) => (room - gap * (n - 1)) / n,
                    };
                    cell_rects.push(Rect::new(at, y, w.max(0), h));
                    at += w.max(0) + gap;
                }
                for (cell, rect) in cells.iter().zip(&cell_rects) {
                    if cell.len() > 1
                        || matches!(row.controls[cell[0]].kind, ControlKind::Toggle { .. })
                    {
                        groups.push((*rect, (si, ri)));
                    }
                    let k = cell.len() as i64;
                    for (j, &ci) in cell.iter().enumerate() {
                        let j = j as i64;
                        let own = Rect::new(
                            rect.x + rect.width * j / k,
                            rect.y,
                            rect.width / k,
                            rect.height,
                        );
                        let parts = match &row.controls[ci].kind {
                            ControlKind::Segments { segments, .. } => {
                                let m = segments.len().max(1) as i64;
                                (0..m)
                                    .map(|q| {
                                        Rect::new(
                                            own.x + own.width * q / m,
                                            own.y,
                                            own.width / m,
                                            own.height,
                                        )
                                    })
                                    .collect()
                            }
                            _ => Vec::new(),
                        };
                        let at = ControlRef {
                            section: si,
                            row: ri,
                            control: ci,
                        };
                        controls.push((at, own, parts));
                    }
                }
                // Icon buttons at the right.
                let mut bx = x0 + panel - pad - h;
                for (ci, c) in row.controls.iter().enumerate().rev() {
                    if matches!(c.kind, ControlKind::Button { icon: Some(_), .. }) {
                        let at = ControlRef {
                            section: si,
                            row: ri,
                            control: ci,
                        };
                        controls.push((at, Rect::new(bx, y, h, h), Vec::new()));
                        bx -= h + gap;
                    }
                }
                y += step;
            }
            y += s.section_gap.units();
        }
        // How far the sections reach, with room under the last as above
        // the first; past the body, they scroll.
        let end = if sections.is_empty() {
            body.y
        } else {
            y - s.section_gap.units() + px(12.0)
        };
        let most = (end - body.bottom()).max(0);
        let scroll = scroll.clamp(0, most);
        let up = |r: &mut Rect| r.y -= scroll;
        headings.iter_mut().for_each(|(_, r)| up(r));
        rows.iter_mut().for_each(|(_, r)| up(r));
        labels.iter_mut().for_each(|(_, r, _)| up(r));
        groups.iter_mut().for_each(|(r, _)| up(r));
        for (_, r, parts) in &mut controls {
            up(r);
            parts.iter_mut().for_each(up);
        }
        // The bar: a thumb as much of the track as the body shows of the
        // sections, as far down it as they're scrolled.
        let bar = (most > 0).then(|| {
            let track = Rect::new(
                rect.right() - px(BAR_REACH),
                body.y + px(4.0),
                px(BAR_REACH),
                (body.height - px(8.0)).max(0),
            );
            let length = (track.height * body.height / (body.height + most))
                .max(px(28.0))
                .min(track.height);
            let thumb = Rect::new(
                track.x,
                track.y + (track.height - length) * scroll / most,
                track.width,
                length,
            );
            (track, thumb)
        });
        InspectorView {
            rect,
            tabs,
            tab_parts,
            body,
            scroll,
            most,
            headings,
            rows,
            labels,
            controls,
            groups,
            bar,
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Where its sections show, under the tabs.
    pub fn body(&self) -> Rect {
        self.body
    }

    /// How far its sections are scrolled down.
    pub fn scroll(&self) -> i64 {
        self.scroll
    }

    /// How far its sections can be scrolled down: 0 where they fit.
    pub fn most(&self) -> i64 {
        self.most
    }

    /// The scroll bar's track and thumb, where its sections don't fit.
    pub fn bar(&self) -> Option<(Rect, Rect)> {
        self.bar
    }

    /// Where control `at` is, as scrolled: perhaps out of the body.
    pub fn control(&self, at: ControlRef) -> Option<Rect> {
        self.controls
            .iter()
            .find(|(c, ..)| *c == at)
            .map(|(_, r, _)| *r)
    }

    /// Every control and where it is, as scrolled, in the order they're
    /// laid out.
    pub fn controls(&self) -> impl Iterator<Item = (ControlRef, Rect)> + '_ {
        self.controls.iter().map(|(at, r, _)| (*at, *r))
    }

    /// Where tab `k`'s title is.
    pub fn tab(&self, k: usize) -> Option<Rect> {
        self.tab_parts.get(k).copied()
    }

    /// Where section `k`'s heading is, as scrolled.
    pub fn heading(&self, k: usize) -> Option<Rect> {
        self.headings.get(k).map(|(_, r)| *r)
    }

    /// How far to scroll so that control `at` shows whole in the body,
    /// with a little room round it: as it is, if it does.
    pub fn reveal(&self, at: ControlRef) -> i64 {
        let Some(r) = self.control(at) else {
            return self.scroll;
        };
        let room = px(8.0);
        let (top, bottom) = (self.body.y + room, self.body.bottom() - room);
        let by = if r.y < top {
            r.y - top
        } else if r.bottom() > bottom {
            (r.bottom() - bottom).min(r.y - top)
        } else {
            0
        };
        (self.scroll + by).clamp(0, self.most)
    }

    /// The scroll the thumb gives, dragged `dy` from where it was when the
    /// sections were scrolled `from`.
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

    /// What (`x`, `y`) hits, if it's in the panel.
    pub fn hit(&self, x: i64, y: i64) -> Option<InspectorHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if let Some(k) = self.tab_parts.iter().position(|r| r.contains(x, y)) {
            return Some(InspectorHit::Tab(k));
        }
        if let Some((track, thumb)) = self.bar
            && track.contains(x, y)
        {
            return Some(InspectorHit::Bar(if thumb.contains(x, y) {
                BarHit::Thumb
            } else if y < thumb.y {
                BarHit::Before
            } else {
                BarHit::After
            }));
        }
        if !self.body.contains(x, y) {
            return Some(InspectorHit::Panel);
        }
        for (at, rect, parts) in &self.controls {
            if rect.contains(x, y) {
                let k = parts.iter().position(|p| p.contains(x, y)).unwrap_or(0);
                return Some(InspectorHit::Control(*at, k));
            }
        }
        Some(InspectorHit::Panel)
    }

    /// The panel drawn: tab `tab` of `model`, with `editing` the control
    /// being typed into and what's typed. What's scrolled out of the body
    /// isn't drawn, and what's partly out of it is cut at its edge; the
    /// rest is drawn as it is, so a change to one control changes only
    /// its own items.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        hover: Option<InspectorHit>,
        editing: Option<(ControlRef, &Field)>,
    ) -> Vec<Item> {
        let c = &kit.theme.colors;
        let mut out = Vec::new();
        fill(&mut out, self.rect, c.chrome);
        fill(
            &mut out,
            Rect::new(self.rect.x, self.rect.y, PIXEL, self.rect.height),
            c.line,
        );
        // Each heading and each row is a group of its own: a scroll moves
        // a few dozen of them, and a change to a control, such as its
        // caret blinking, changes only its row's.
        let body = self.body;
        let clip = scene::Path::rect(body.x, body.y, body.width, body.height);
        for items in self.draw_sections(kit, model, tab, hover, editing) {
            let Some(b) = items
                .iter()
                .filter_map(Item::bounds)
                .reduce(scene::Rect::union)
            else {
                continue;
            };
            if b.y >= body.bottom() || b.bottom() <= body.y {
                continue;
            }
            let inside = b.y >= body.y && b.bottom() <= body.bottom();
            out.push(Item::Group(Group {
                clip: (!inside).then(|| clip.clone()),
                transform: scene::Transform::IDENTITY,
                items,
            }));
        }
        let titles: Vec<chrome::Segment> = model
            .tabs
            .iter()
            .map(|t| chrome::Segment {
                command: String::new(),
                label: t.title.clone(),
                icon: None,
            })
            .collect();
        segmented(
            kit,
            &mut out,
            self.tabs,
            &self.tab_parts,
            &titles,
            Some(tab),
        );
        if let Some((_, thumb)) = self.bar {
            let lit = hover == Some(InspectorHit::Bar(BarHit::Thumb));
            let w = px(BAR_WIDTH);
            let drawn = Rect::new(thumb.x + (thumb.width - w) / 2, thumb.y, w, thumb.height);
            rounded(
                &mut out,
                drawn,
                w / 2,
                if lit { c.secondary } else { c.disabled },
            );
        }
        out
    }

    /// The sections' headings, and their rows' labels and controls, as
    /// scrolled: what each heading and each row in sight draws.
    fn draw_sections(
        &self,
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        hover: Option<InspectorHit>,
        editing: Option<(ControlRef, &Field)>,
    ) -> Vec<Vec<Item>> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let heading = kit.ty(s.heading, 600);
        let name = kit.ty(s.label, 400);
        let visible = |r: &Rect| r.bottom() > self.body.y && r.y < self.body.bottom();
        let mut units = Vec::new();
        for (title, r) in self.headings.iter().filter(|(_, r)| visible(r)) {
            let mut out = Vec::new();
            kit.text(&mut out, title, r.x, r.y, r.height, heading, c.secondary);
            units.push(out);
        }
        let radius = s.radius.units();
        for (row, _) in self.rows.iter().filter(|(_, r)| visible(r)) {
            let mut out = Vec::new();
            for (label, r, _) in self.labels.iter().filter(|l| l.2 == *row) {
                let shown = kit.fit(label, None, r.width - px(4.0), name);
                kit.text(&mut out, &shown, r.x, r.y, r.height, name, c.secondary);
            }
            for (group, _) in self.groups.iter().filter(|g| g.1 == *row) {
                rounded(&mut out, *group, radius, c.hover);
            }
            for (at, rect, parts) in self
                .controls
                .iter()
                .filter(|(at, ..)| (at.section, at.row) == *row)
            {
                self.draw_control(
                    kit,
                    model,
                    tab,
                    (*at, *rect, parts),
                    hover,
                    editing,
                    &mut out,
                );
            }
            units.push(out);
        }
        units
    }

    /// Control `at`, at `rect` with its segments at `parts`, drawn into
    /// `out`, lit if `hover` is over it, with what's typed if `editing`
    /// is it.
    #[allow(
        clippy::too_many_arguments,
        reason = "the control, where it is, and what's over and in it"
    )]
    fn draw_control(
        &self,
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        (at, rect, parts): (ControlRef, Rect, &[Rect]),
        hover: Option<InspectorHit>,
        editing: Option<(ControlRef, &Field)>,
        out: &mut Vec<Item>,
    ) {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let name = kit.ty(s.label, 400);
        let value = kit.ty(s.value, 400);
        let radius = s.radius.units();
        let sections = model.tabs.get(tab).map_or(&[][..], |t| &t.sections[..]);
        let (rect, at) = (&rect, &at);
        let Some(control) = sections
            .get(at.section)
            .and_then(|s| s.rows.get(at.row))
            .and_then(|r| r.controls.get(at.control))
        else {
            return;
        };
        let ink = if control.enabled { c.ink } else { c.disabled };
        let quiet = if control.enabled {
            c.secondary
        } else {
            c.disabled
        };
        let (cx, cy) = rect.centre();
        let lit = hover == Some(InspectorHit::Control(*at, 0)) && control.enabled;
        let typing = editing.filter(|(e, _)| e == at).map(|(_, f)| f);
        // What's typed, with its caret or selection, from `x`.
        let typed = |out: &mut Vec<Item>, field: &Field, x: i64| {
            let layout = kit.layout(field.text(), value);
            let y = rect.y + (rect.height - layout.height) / 2;
            field.draw(
                out,
                &layout,
                (x, y),
                kit.caret.then_some(c.ink),
                c.selection,
            );
            kit.text(out, field.text(), x, rect.y, rect.height, value, ink);
        };
        match &control.kind {
            ControlKind::Choice {
                choices,
                selected,
                plain,
            } => {
                let choice = selected.and_then(|k| choices.get(k));
                let label = choice.map_or("—", |ch| ch.label.as_str());
                let family = choice.and_then(|ch| ch.family.as_deref());
                if *plain {
                    let shown = kit.fit(label, family, rect.width, name);
                    kit.text_in(
                        out,
                        &shown,
                        family,
                        rect.x,
                        rect.y,
                        rect.height,
                        name,
                        quiet,
                    );
                } else {
                    field_box(out, c, *rect, radius - PIXEL);
                    let mut x = rect.x + px(10.0);
                    if let Some([r, g, b]) = choice.and_then(|ch| ch.swatch) {
                        let side = px(14.0);
                        swatch(
                            out,
                            c,
                            Rect::new(x, cy - side / 2, side, side),
                            Color::rgb(r, g, b),
                        );
                        x += side + px(7.0);
                    }
                    // The label stops short of the arrow.
                    let room = rect.right() - px(26.0) - x;
                    let shown = kit.fit(label, family, room, value);
                    kit.text_in(out, &shown, family, x, rect.y, rect.height, value, ink);
                    kit.icon(
                        out,
                        "chevron-down",
                        rect.right() - px(14.0),
                        cy,
                        px(14.0),
                        quiet,
                    );
                }
            }
            ControlKind::Length {
                value: length,
                prefix,
                plain,
            } => {
                let shown = length.map_or_else(|| "—".to_string(), |l| l.to_string());
                let mut x = if *plain {
                    rect.x
                } else {
                    field_box(out, c, *rect, radius - PIXEL);
                    rect.x + px(10.0)
                };
                if !prefix.is_empty() {
                    x += kit.text(out, prefix, x, rect.y, rect.height, value, quiet) + px(8.0);
                }
                match typing {
                    Some(field) => typed(out, field, x),
                    None => {
                        let shown = kit.fit(&shown, None, rect.right() - px(6.0) - x, value);
                        kit.text(out, &shown, x, rect.y, rect.height, value, ink);
                    }
                }
            }
            ControlKind::Toggle { icon, on, .. } => {
                if *on {
                    let raised = Rect::new(
                        rect.x + px(2.0),
                        rect.y + px(2.0),
                        rect.width - px(4.0),
                        rect.height - px(4.0),
                    );
                    field_box(out, c, raised, radius - PIXEL);
                } else if lit {
                    rounded(out, rect.inset(px(2.0)), radius - PIXEL, c.line);
                }
                kit.icon(out, icon, cx, cy, px(15.0), ink);
            }
            ControlKind::Segments { segments, selected } => {
                segmented(kit, out, *rect, parts, segments, *selected);
            }
            ControlKind::Text { value: text } => {
                field_box(out, c, *rect, radius - PIXEL);
                let x = rect.x + px(10.0);
                match typing {
                    Some(field) => typed(out, field, x),
                    None => {
                        let shown = kit.fit(text, None, rect.right() - px(6.0) - x, value);
                        kit.text(out, &shown, x, rect.y, rect.height, value, ink);
                    }
                }
            }
            ControlKind::Colour { value: colour } => {
                field_box(out, c, *rect, radius - PIXEL);
                let side = px(16.0);
                let at = Rect::new(rect.x + px(7.0), cy - side / 2, side, side);
                let hex = match colour {
                    Some([r, g, b]) => {
                        swatch(out, c, at, Color::rgb(*r, *g, *b));
                        format!("#{r:02x}{g:02x}{b:02x}")
                    }
                    None => "—".to_string(),
                };
                let x = at.right() + px(8.0);
                match typing {
                    Some(field) => typed(out, field, x),
                    None => {
                        kit.text(out, &hex, x, rect.y, rect.height, value, ink);
                    }
                }
            }
            ControlKind::Note { text } => {
                let shown = kit.fit(text, None, rect.width, name);
                kit.text(out, &shown, rect.x, rect.y, rect.height, name, c.secondary);
            }
            ControlKind::Button { label, icon } => match icon {
                Some(icon) => {
                    if lit {
                        rounded(out, *rect, radius, c.hover);
                    }
                    kit.icon(out, icon, cx, cy, px(14.0), quiet);
                }
                None => {
                    let link = kit.ty(s.value, 500);
                    let color = if control.enabled {
                        c.accent
                    } else {
                        c.disabled
                    };
                    let shown = kit.fit(label, None, rect.width, link);
                    // A link on a line of its own sits a little low, as
                    // text under a row of fields does; one in a row of
                    // fields, as a label that follows its value, on
                    // their line.
                    let low = if rect.height < s.field.units() {
                        px(2.0)
                    } else {
                        0
                    };
                    kit.text(out, &shown, rect.x, rect.y + low, rect.height, link, color);
                }
            },
        }
    }
}
