//! The ruler over a page's text column: across the window, marked from
//! the page's one edge to the other in its unit and numbered from its
//! zero, the column's start or the page's (`chrome::Marks`), with the
//! paragraph's indents above
//! and below and its tab stops along its foot, which a person sets and
//! drags. A stop with a leader shows a short run of it before its mark.

use chrome::{LeaderLook, Mark, Ruler, StopKind};
use scene::Item;
use text::units::PIXEL;

use crate::kit::{fill, px, triangle};
use crate::{Kit, Rect};

/// What a point on the ruler hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerHit {
    /// A tab stop, by index.
    Stop(usize),
    /// The first line's indent, above.
    First,
    /// The start indent, below on the left.
    Start,
    /// The end indent, below on the right.
    End,
    /// The foot of the column, where a click sets a stop: how far from
    /// the paragraph's place's start.
    Foot(i64),
    /// Anywhere else on the ruler.
    Bar,
}

/// The ruler laid out.
#[derive(Clone, Debug)]
pub struct RulerView {
    rect: Rect,
    model: Ruler,
}

/// How far from a marker a click still takes it.
const REACH: f64 = 5.0;

/// A stop's stem's width, in pixels.
const STEM: f64 = 1.5;

/// How long a leader's run is, in pixels, and how far before its stop's
/// mark it ends.
const LEADER_RUN: f64 = 12.0;
const LEADER_GAP: f64 = 2.0;

/// Rounds `x` to a whole pixel, so lines are sharp.
fn sharp(x: i64) -> i64 {
    (x + PIXEL / 2).div_euclid(PIXEL) * PIXEL
}

/// The marks of a leader that looks as `look`, in `run`, all on whole
/// pixels so they're sharp at the ruler's size, each ending at its right
/// and sitting on its foot: dots two pixels square, four apart; dashes
/// four pixels long and one high, six apart; or a line a pixel high.
fn leader_marks(look: LeaderLook, run: Rect) -> Vec<Rect> {
    let (end, foot) = (run.right(), run.bottom());
    let marks = |long: f64, high: f64, every: f64| -> Vec<Rect> {
        (0..)
            .map(|k| end - px(long) - k * px(every))
            .take_while(|&x| x >= run.x)
            .map(|x| Rect::new(x, foot - px(high), px(long), px(high)))
            .collect()
    };
    match look {
        LeaderLook::Dots => marks(2.0, 2.0, 4.0),
        LeaderLook::Dashes => marks(4.0, 1.0, 6.0),
        LeaderLook::Line => vec![Rect::new(run.x, foot - PIXEL, run.width, PIXEL)],
    }
}

impl RulerView {
    /// `model` laid out from `top`, across `width`.
    pub fn new(kit: &Kit<'_>, model: &Ruler, top: i64, width: i64) -> RulerView {
        RulerView {
            rect: Rect::new(0, top, width, kit.theme.sizes.ruler.units()),
            model: model.clone(),
        }
    }

    /// Its rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Where the mark of a stop of `kind` at `x` starts on the left: its
    /// stem's edge for a start stop, the foot that reaches left for the
    /// others.
    fn mark_left(x: i64, kind: StopKind) -> i64 {
        match kind {
            StopKind::Start => x - px(STEM) / 2,
            StopKind::End => x - px(5.25),
            StopKind::Center | StopKind::Decimal(_) => x - px(4.0),
        }
    }

    /// Where the leader of stop `i` is drawn, if it has one: the run of its
    /// dots, dashes or line just before its mark, two pixels high, its foot
    /// on the line of the mark's.
    pub fn leader(&self, i: usize) -> Option<Rect> {
        let stop = self.model.stops.get(i)?;
        stop.leader?;
        let x = sharp(self.model.origin + stop.position);
        let end = sharp(Self::mark_left(x, stop.kind) - px(LEADER_GAP));
        let foot = self.rect.y + self.rect.height - px(5.0);
        Some(Rect::new(
            end - px(LEADER_RUN),
            foot - px(2.0),
            px(LEADER_RUN),
            px(2.0),
        ))
    }

    /// How far `x` is from the paragraph's place's start, where indents
    /// and stops are measured from.
    pub fn position(&self, x: i64) -> i64 {
        x - self.model.origin
    }

    /// [`RulerView::position`], snapped to the finest ticks of its unit:
    /// where a marker dragged to `x` goes.
    pub fn snapped(&self, x: i64) -> i64 {
        self.model.unit.marks().snapped(self.position(x))
    }

    /// What (`x`, `y`) hits, if it's on the ruler.
    pub fn hit(&self, x: i64, y: i64) -> Option<RulerHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let m = &self.model;
        let near = |at: i64| (x - (m.origin + at)).abs() <= px(REACH);
        let lower = y >= self.rect.y + self.rect.height / 2;
        if lower && let Some(i) = m.stops.iter().position(|s| near(s.position)) {
            return Some(RulerHit::Stop(i));
        }
        if let Some(ind) = m.indents {
            if !lower && near(ind.start + ind.first) {
                return Some(RulerHit::First);
            }
            if lower && near(ind.start) {
                return Some(RulerHit::Start);
            }
            if lower && near(m.extent - ind.end) {
                return Some(RulerHit::End);
            }
        }
        if lower && (m.start..m.start + m.width).contains(&x) {
            return Some(RulerHit::Foot(x - m.origin));
        }
        Some(RulerHit::Bar)
    }

    /// The ruler drawn.
    pub fn draw(&self, kit: &Kit<'_>) -> Vec<Item> {
        let (s, c) = (&kit.theme.sizes, &kit.theme.colors);
        let m = &self.model;
        let Rect {
            y: top,
            width,
            height: h,
            ..
        } = self.rect;
        let mut out = Vec::new();
        fill(&mut out, self.rect, c.chrome);
        let three = px(3.0);
        fill(
            &mut out,
            Rect::new(m.start, top + three, m.width, h - 2 * three),
            c.field,
        );
        // The unit's marks from the page's one edge to the other: numbers
        // out from its zero, both ways from the column's start, and ticks
        // up from the foot between them.
        let number = kit.ty(s.tiny, 400);
        let from = m.page_start - m.zero;
        for (at, mark) in m.unit.marks().between(from, from + m.page_width) {
            let x = sharp(m.zero + at);
            match mark {
                Mark::Number(n) => {
                    let label = n.to_string();
                    let w = kit.width(&label, number);
                    kit.text(
                        &mut out,
                        &label,
                        x - w / 2 + PIXEL / 2,
                        top + three,
                        px(13.0),
                        number,
                        c.secondary,
                    );
                }
                Mark::Tick(length) => {
                    let tall = px(2.0) * i64::from(length);
                    fill(
                        &mut out,
                        Rect::new(x, top + h - px(4.0) - tall, PIXEL, tall),
                        c.secondary,
                    );
                }
            }
        }
        // Leaders: a short run of each one's dots, dashes or line just
        // before its stop's mark, on the line of the mark's foot, in the
        // ruler's ink, the ticks under it cleared so it reads at the
        // ruler's size. Drawn before the indents and the marks, which
        // stay whole over it.
        let leader_ink = if m.enabled { c.ink } else { c.disabled };
        for (i, stop) in m.stops.iter().enumerate() {
            if let (Some(leader), Some(run)) = (stop.leader, self.leader(i)) {
                fill(
                    &mut out,
                    Rect::new(
                        run.x - PIXEL,
                        top + h - px(10.0),
                        run.width + PIXEL,
                        px(6.0),
                    ),
                    c.field,
                );
                for r in leader_marks(LeaderLook::of(leader), run) {
                    fill(&mut out, r, leader_ink);
                }
            }
        }
        let foot = top + h - px(4.5);
        let bar = px(1.5);
        // Indents: the first line's above, the start and end below.
        let size = px(4.5);
        if let Some(ind) = m.indents {
            let ink = if m.enabled { c.ink } else { c.disabled };
            let at = m.origin;
            triangle(
                &mut out,
                at + ind.start + ind.first,
                top + three + size,
                size,
                true,
                ink,
            );
            triangle(
                &mut out,
                at + ind.start,
                top + h - three - size,
                size,
                false,
                ink,
            );
            triangle(
                &mut out,
                at + m.extent - ind.end,
                top + h - three - size,
                size,
                false,
                ink,
            );
        }
        // Tab stops: start, centred, end, and on the decimal separator.
        let ink = if m.enabled { c.accent } else { c.disabled };
        let stem = px(STEM);
        for stop in &m.stops {
            let x = sharp(m.origin + stop.position);
            fill(
                &mut out,
                Rect::new(x - stem / 2, foot - px(8.0), stem, px(8.0)),
                ink,
            );
            let left = Self::mark_left(x, stop.kind);
            let foot_bar = match stop.kind {
                StopKind::Start | StopKind::End => Rect::new(left, foot - bar, px(6.0), bar),
                StopKind::Center | StopKind::Decimal(_) => {
                    Rect::new(left, foot - bar, px(8.0), bar)
                }
            };
            fill(&mut out, foot_bar, ink);
            if let StopKind::Decimal(_) = stop.kind {
                fill(
                    &mut out,
                    Rect::new(x + px(2.0), foot - px(5.0), px(2.0), px(2.0)),
                    ink,
                );
            }
        }
        fill(
            &mut out,
            Rect::new(0, top + h - PIXEL, width, PIXEL),
            c.line,
        );
        out
    }
}
