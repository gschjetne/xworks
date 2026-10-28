//! The ruler: a page and its text column, measured, with its paragraph's
//! indents and tab stops, which a person sets and drags.

use crate::Unit;

/// A ruler over a page's text column (docs/ARCHITECTURE.md, section 8):
/// marked from the page's one edge to the other, and numbered from its
/// zero, the column's start, as word processors number it, out both
/// ways, or the page's edge, as page layout programs do, the person's
/// choice (review 77nh3z47). Its paragraph's indents and tab stops are
/// measured from the start of the paragraph's own place in the column,
/// which a list or a quote moves in. Positions are in layout units across
/// the ruler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruler {
    /// Where the page starts, from the ruler's left; in a view without
    /// pages, where the column does.
    pub page_start: i64,
    /// How wide the page is; in a view without pages, the column.
    pub page_width: i64,
    /// Where the column starts, from the ruler's left.
    pub start: i64,
    /// How wide the column is.
    pub width: i64,
    /// Where it's numbered from, from the ruler's left: the column's
    /// start, or the page's.
    pub zero: i64,
    /// Where the paragraph's place starts, from the ruler's left.
    pub origin: i64,
    /// How wide the paragraph's place is.
    pub extent: i64,
    /// The unit it's measured in.
    pub unit: Unit,
    /// The paragraph's indents, if there's a paragraph to set them on.
    pub indents: Option<Indents>,
    /// The paragraph's tab stops, in order.
    pub stops: Vec<Stop>,
    /// Whether they can be changed.
    pub enabled: bool,
}

/// How a ruler in a unit is marked: numbers some way apart, and ticks
/// between them, longer the coarser they are. Markers dragged along it
/// snap to its finest ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marks {
    /// How far apart its numbers are, in layout units.
    pub numbered: i64,
    /// What each number counts on from the one before: 1, or 6 for picas,
    /// numbered every inch.
    pub step: i64,
    /// Its ticks, coarsest first: how far apart, in layout units, and how
    /// long, from 1 for the shortest.
    pub ticks: &'static [(i64, u8)],
}

impl Unit {
    /// How a ruler in it is marked: centimetres numbered with ticks at
    /// their halves and quarters; inches at their halves, quarters and
    /// eighths; picas numbered every six, an inch, with ticks at every
    /// three, every one and every six points; millimetres as centimetres,
    /// numbered in tens; points as picas, numbered in seventy-twos.
    pub fn marks(self) -> Marks {
        const CM: i64 = Unit::Centimetre.units();
        const INCH: i64 = Unit::Inch.units();
        const PICA: i64 = Unit::Pica.units();
        match self {
            Unit::Centimetre | Unit::Millimetre => Marks {
                numbered: CM,
                step: if self == Unit::Centimetre { 1 } else { 10 },
                ticks: &[(CM / 2, 2), (CM / 4, 1)],
            },
            Unit::Inch => Marks {
                numbered: INCH,
                step: 1,
                ticks: &[(INCH / 2, 3), (INCH / 4, 2), (INCH / 8, 1)],
            },
            Unit::Pica | Unit::Point => Marks {
                numbered: INCH,
                step: if self == Unit::Pica { 6 } else { 72 },
                ticks: &[(3 * PICA, 3), (PICA, 2), (PICA / 2, 1)],
            },
        }
    }
}

impl Marks {
    /// How far markers snap to: its finest ticks.
    pub fn snap(&self) -> i64 {
        self.ticks
            .last()
            .map_or(self.numbered, |&(at, _)| at)
            .max(1)
    }

    /// `position` snapped to the nearest of its finest ticks.
    pub fn snapped(&self, position: i64) -> i64 {
        let step = self.snap();
        (position + step / 2).div_euclid(step) * step
    }

    /// The marks from `from` to `to`, both measured from where the ruler
    /// is numbered from and inclusive, in order: where each is, and its
    /// number, or how long its tick is. Nothing marks the zero itself.
    pub fn between(&self, from: i64, to: i64) -> Vec<(i64, Mark)> {
        let step = self.snap();
        let first = from.div_euclid(step) + i64::from(from.rem_euclid(step) != 0);
        let last = to.div_euclid(step);
        (first..=last)
            .map(|k| k * step)
            .filter(|&at| at != 0)
            .map(|at| {
                let mark = if at % self.numbered == 0 {
                    Mark::Number(at.abs() / self.numbered * self.step)
                } else {
                    let length = self
                        .ticks
                        .iter()
                        .find(|&&(every, _)| at % every == 0)
                        .map_or(1, |&(_, length)| length);
                    Mark::Tick(length)
                };
                (at, mark)
            })
            .collect()
    }
}

/// A mark on the ruler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A number: how far from where it's numbered from, either way.
    Number(i64),
    /// A tick, this long, from 1 for the shortest.
    Tick(u8),
}

/// A paragraph's indents on the ruler.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Indents {
    /// From the column's start edge.
    pub start: i64,
    /// From its end edge.
    pub end: i64,
    /// The first line's, further in than `start`.
    pub first: i64,
}

/// A tab stop on the ruler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stop {
    /// From the column's start edge.
    pub position: i64,
    /// How text sits at it.
    pub kind: StopKind,
    /// The character that fills the room a tab opens up to it, if any:
    /// its leader, which the ruler shows before its mark.
    pub leader: Option<char>,
}

/// How a leader is drawn small, as the ruler shows it: a run of dots,
/// dashes or a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LeaderLook {
    /// Dots, for a full stop and any character but a dash or a line.
    Dots,
    /// Dashes, for a hyphen or a dash.
    Dashes,
    /// A line, for an underscore or a rule.
    Line,
}

impl LeaderLook {
    /// How leader `c` looks on the ruler.
    pub fn of(c: char) -> LeaderLook {
        match c {
            '-' | '‐' | '‑' | '‒' | '–' | '—' | '―' | '−' => LeaderLook::Dashes,
            '_' | '‾' | '─' | '━' | '⎯' => LeaderLook::Line,
            _ => LeaderLook::Dots,
        }
    }
}

/// How text sits at a tab stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StopKind {
    /// It starts there.
    Start,
    /// It's centred on it.
    Center,
    /// It ends there.
    End,
    /// Its decimal separator, this character, is there.
    Decimal(char),
}

impl StopKind {
    /// Its name, as the inspector shows it.
    pub fn name(self) -> &'static str {
        match self {
            StopKind::Start => "Left",
            StopKind::Center => "Centred",
            StopKind::End => "Right",
            StopKind::Decimal(_) => "Decimal",
        }
    }

    /// Its name as commands take it: "start", "center", "end", or
    /// "decimal:" and its character.
    pub fn id(self) -> String {
        match self {
            StopKind::Start => "start".to_string(),
            StopKind::Center => "center".to_string(),
            StopKind::End => "end".to_string(),
            StopKind::Decimal(c) => format!("decimal:{c}"),
        }
    }

    /// The kind [`StopKind::id`] names.
    pub fn from_id(id: &str) -> Option<StopKind> {
        Some(match id {
            "start" => StopKind::Start,
            "center" => StopKind::Center,
            "end" => StopKind::End,
            _ => {
                let mut c = id.strip_prefix("decimal:")?.chars();
                match (c.next(), c.next()) {
                    (Some(c), None) => StopKind::Decimal(c),
                    _ => return None,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_name_themselves() {
        for kind in [
            StopKind::Start,
            StopKind::Center,
            StopKind::End,
            StopKind::Decimal(','),
        ] {
            assert_eq!(StopKind::from_id(&kind.id()), Some(kind));
        }
        assert_eq!(StopKind::from_id("decimal:"), None);
        assert_eq!(StopKind::Center.name(), "Centred");
    }

    #[test]
    fn leaders_look_as_their_characters_do() {
        assert_eq!(LeaderLook::of('.'), LeaderLook::Dots);
        assert_eq!(LeaderLook::of('·'), LeaderLook::Dots);
        assert_eq!(LeaderLook::of('*'), LeaderLook::Dots);
        assert_eq!(LeaderLook::of('-'), LeaderLook::Dashes);
        assert_eq!(LeaderLook::of('–'), LeaderLook::Dashes);
        assert_eq!(LeaderLook::of('_'), LeaderLook::Line);
    }

    #[test]
    fn each_unit_marks_its_own_ruler() {
        let cm = Unit::Centimetre.units();
        let marks = Unit::Centimetre.marks();
        // Quarters, halves and whole centimetres, numbered out from the
        // column's start both ways; nothing at the start itself.
        assert_eq!(
            marks.between(-cm / 2, cm),
            [
                (-cm / 2, Mark::Tick(2)),
                (-cm / 4, Mark::Tick(1)),
                (cm / 4, Mark::Tick(1)),
                (cm / 2, Mark::Tick(2)),
                (3 * cm / 4, Mark::Tick(1)),
                (cm, Mark::Number(1)),
            ]
        );
        assert_eq!(marks.between(-cm - 1, -cm + 1), [(-cm, Mark::Number(1))]);
        assert_eq!(marks.snap(), 90_000);
        assert_eq!(marks.snapped(130_000), 90_000);
        assert_eq!(marks.snapped(140_000), 180_000);
        assert_eq!(marks.snapped(-50_000), -90_000);

        // Eighths of an inch, its quarters and halves longer.
        let inch = Unit::Inch.units();
        let marks = Unit::Inch.marks();
        let lengths: Vec<Mark> = marks.between(1, inch).into_iter().map(|(_, m)| m).collect();
        assert_eq!(
            lengths,
            [1, 2, 1, 3, 1, 2, 1]
                .map(Mark::Tick)
                .into_iter()
                .chain([Mark::Number(1)])
                .collect::<Vec<_>>()
        );
        assert_eq!(marks.snap(), inch / 8);

        // Picas numbered every six, an inch, with every pica ticked and
        // every six points, half a pica.
        let pica = Unit::Pica.units();
        let marks = Unit::Pica.marks();
        let ticks = marks.between(1, 12 * pica);
        assert_eq!(ticks.len(), 24);
        assert_eq!(ticks[0], (pica / 2, Mark::Tick(1)));
        assert_eq!(ticks[1], (pica, Mark::Tick(2)));
        assert_eq!(ticks[5], (3 * pica, Mark::Tick(3)));
        assert_eq!(ticks[11], (6 * pica, Mark::Number(6)));
        assert_eq!(ticks[23], (12 * pica, Mark::Number(12)));
        assert_eq!(marks.snap(), 6 * Unit::Point.units());
    }
}
