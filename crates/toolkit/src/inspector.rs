//! The inspector: a side panel of the selection's properties, as the app
//! describes them ([`chrome::Inspector`]), in tabs of sections of rows of
//! typed controls.

use chrome::{ControlKind, Inspector};
use scene::Item;
use text::units::PIXEL;

use crate::kit::{field_box, fill, px, rounded};
use crate::title::segmented;
use crate::{Field, Kit, Rect};

/// A control of the inspector, by where it is: its section, row, and
/// place in the row, in the tab shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Somewhere else in the panel.
    Panel,
}

/// The inspector laid out.
#[derive(Clone, Debug)]
pub struct InspectorView {
    rect: Rect,
    tabs: Rect,
    tab_parts: Vec<Rect>,
    /// Section headings: the title and where it goes.
    headings: Vec<(String, Rect)>,
    /// Row labels and where they go.
    labels: Vec<(String, Rect)>,
    /// Each control: where it is, its own area, and its segments' areas.
    controls: Vec<(ControlRef, Rect, Vec<Rect>)>,
    /// Boxes drawn behind runs of toggles, which share one.
    groups: Vec<Rect>,
}

impl InspectorView {
    /// `model`'s tab `tab` laid out in a panel from `top` to `bottom`
    /// along the right of a window `width` wide.
    pub fn new(
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        width: i64,
        top: i64,
        bottom: i64,
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
        let mut y = top + px(12.0);
        let tabs = Rect::new(x0 + pad, y, panel - 2 * pad, field_h);
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
        y += px(44.0);
        let mut headings = Vec::new();
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
                if !row.label.is_empty() {
                    labels.push((row.label.clone(), Rect::new(x0 + pad, y, label_w, h)));
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
                if row.label.is_empty() && cells.len() > 1 {
                    // The first sits where a label would.
                    cell_rects.push(Rect::new(x0 + pad, y, label_w, h));
                    let rest = (cells.len() - 1) as i64;
                    let w = (room - gap * (rest - 1)) / rest;
                    for k in 0..rest {
                        cell_rects.push(Rect::new(x + k * (w + gap), y, w, h));
                    }
                } else {
                    if row.label.is_empty() {
                        x = x0 + pad;
                        room += label_w;
                    }
                    let n = cells.len().max(1) as i64;
                    let w = (room - gap * (n - 1)) / n;
                    for k in 0..n {
                        cell_rects.push(Rect::new(x + k * (w + gap), y, w, h));
                    }
                }
                for (cell, rect) in cells.iter().zip(&cell_rects) {
                    if cell.len() > 1
                        || matches!(row.controls[cell[0]].kind, ControlKind::Toggle { .. })
                    {
                        groups.push(*rect);
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
        InspectorView {
            rect,
            tabs,
            tab_parts,
            headings,
            labels,
            controls,
            groups,
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Where control `at` is.
    pub fn control(&self, at: ControlRef) -> Option<Rect> {
        self.controls
            .iter()
            .find(|(c, ..)| *c == at)
            .map(|(_, r, _)| *r)
    }

    /// What (`x`, `y`) hits, if it's in the panel.
    pub fn hit(&self, x: i64, y: i64) -> Option<InspectorHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if let Some(k) = self.tab_parts.iter().position(|r| r.contains(x, y)) {
            return Some(InspectorHit::Tab(k));
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
    /// being typed into and what's typed.
    pub fn draw(
        &self,
        kit: &Kit<'_>,
        model: &Inspector,
        tab: usize,
        hover: Option<InspectorHit>,
        editing: Option<(ControlRef, &Field)>,
    ) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let mut out = Vec::new();
        fill(&mut out, self.rect, c.chrome);
        fill(
            &mut out,
            Rect::new(self.rect.x, self.rect.y, PIXEL, self.rect.height),
            c.line,
        );
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
        let heading = kit.ty(s.heading, 600);
        let name = kit.ty(s.label, 400);
        let value = kit.ty(s.value, 400);
        for (title, r) in &self.headings {
            kit.text(&mut out, title, r.x, r.y, r.height, heading, c.secondary);
        }
        for (label, r) in &self.labels {
            kit.text(&mut out, label, r.x, r.y, r.height, name, c.secondary);
        }
        let radius = s.radius.units();
        for group in &self.groups {
            rounded(&mut out, *group, radius, c.hover);
        }
        let sections = model.tabs.get(tab).map_or(&[][..], |t| &t.sections[..]);
        for (at, rect, parts) in &self.controls {
            let Some(control) = sections
                .get(at.section)
                .and_then(|s| s.rows.get(at.row))
                .and_then(|r| r.controls.get(at.control))
            else {
                continue;
            };
            let ink = if control.enabled { c.ink } else { c.disabled };
            let quiet = if control.enabled {
                c.secondary
            } else {
                c.disabled
            };
            let (cx, cy) = rect.centre();
            let lit = hover == Some(InspectorHit::Control(*at, 0)) && control.enabled;
            match &control.kind {
                ControlKind::Choice {
                    choices,
                    selected,
                    plain,
                } => {
                    let label = selected
                        .and_then(|k| choices.get(k))
                        .map_or("—", |ch| ch.label.as_str());
                    if *plain {
                        kit.text(&mut out, label, rect.x, rect.y, rect.height, name, quiet);
                    } else {
                        field_box(&mut out, c, *rect, radius - PIXEL);
                        kit.text(
                            &mut out,
                            label,
                            rect.x + px(10.0),
                            rect.y,
                            rect.height,
                            value,
                            ink,
                        );
                        kit.icon(
                            &mut out,
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
                    let typing = editing.filter(|(e, _)| e == at).map(|(_, f)| f);
                    let x = if *plain {
                        rect.x
                    } else {
                        field_box(&mut out, c, *rect, radius - PIXEL);
                        rect.x + px(10.0)
                    };
                    let mut x = x;
                    if !prefix.is_empty() {
                        x += kit.text(&mut out, prefix, x, rect.y, rect.height, value, quiet)
                            + px(8.0);
                    }
                    match typing {
                        Some(field) => {
                            let layout = kit.layout(field.text(), value);
                            let y = rect.y + (rect.height - layout.height) / 2;
                            field.draw(
                                &mut out,
                                &layout,
                                (x, y),
                                kit.caret.then_some(c.ink),
                                c.selection,
                            );
                            kit.text(&mut out, field.text(), x, rect.y, rect.height, value, ink);
                        }
                        None => {
                            kit.text(&mut out, &shown, x, rect.y, rect.height, value, ink);
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
                        field_box(&mut out, c, raised, radius - PIXEL);
                    } else if lit {
                        rounded(&mut out, rect.inset(px(2.0)), radius - PIXEL, c.line);
                    }
                    kit.icon(&mut out, icon, cx, cy, px(15.0), ink);
                }
                ControlKind::Segments { segments, selected } => {
                    segmented(kit, &mut out, *rect, parts, segments, *selected);
                }
                ControlKind::Text { value: text } => {
                    field_box(&mut out, c, *rect, radius - PIXEL);
                    let x = rect.x + px(10.0);
                    match editing.filter(|(e, _)| e == at).map(|(_, f)| f) {
                        Some(field) => {
                            let layout = kit.layout(field.text(), value);
                            let y = rect.y + (rect.height - layout.height) / 2;
                            field.draw(
                                &mut out,
                                &layout,
                                (x, y),
                                kit.caret.then_some(c.ink),
                                c.selection,
                            );
                            kit.text(&mut out, field.text(), x, rect.y, rect.height, value, ink);
                        }
                        None => {
                            kit.text(&mut out, text, x, rect.y, rect.height, value, ink);
                        }
                    }
                }
                ControlKind::Note { text } => {
                    kit.text(
                        &mut out,
                        text,
                        rect.x,
                        rect.y,
                        rect.height,
                        name,
                        c.secondary,
                    );
                }
                ControlKind::Button { label, icon } => match icon {
                    Some(icon) => {
                        if lit {
                            rounded(&mut out, *rect, radius, c.hover);
                        }
                        kit.icon(&mut out, icon, cx, cy, px(14.0), quiet);
                    }
                    None => {
                        let link = kit.ty(s.value, 500);
                        let color = if control.enabled {
                            c.accent
                        } else {
                            c.disabled
                        };
                        kit.text(
                            &mut out,
                            label,
                            rect.x,
                            rect.y + px(2.0),
                            rect.height,
                            link,
                            color,
                        );
                    }
                },
            }
        }
        out
    }
}
