//! Tab stops: where the text after a tab character goes.

/// How the text after a tab sits at its stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum TabAlign {
    /// It starts at the stop: a left tab in left-to-right text.
    Start,
    /// It ends at the stop: a right tab in left-to-right text.
    End,
    /// It's centred on the stop.
    Center,
    /// Its decimal separator, this character, is at the stop: `.` in
    /// English, `,` in German. Text without one ends at the stop.
    Decimal(char),
}

/// A tab stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
pub struct TabStop {
    /// Its distance from the paragraph's start edge: the left in
    /// left-to-right text, the right in right-to-left text.
    pub position: i64,
    /// How the text after a tab sits at it.
    pub align: TabAlign,
}

/// A paragraph's tab stops: those set, and default ones at a regular
/// interval after the last of them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tabs {
    /// The stops set, in order of position.
    pub stops: Vec<TabStop>,
    /// The distance between default stops, which start stops at its
    /// multiples beyond the last stop set; 0 for none. A tab with no stop
    /// left to go to is as wide as a space.
    pub interval: i64,
}

impl Tabs {
    /// The stop a tab whose text before it ends at `pen` goes to: the
    /// first past `pen`.
    pub fn next(&self, pen: i64) -> Option<TabStop> {
        if let Some(stop) = self.stops.iter().find(|s| s.position > pen) {
            return Some(*stop);
        }
        if self.interval <= 0 {
            return None;
        }
        let after = self
            .stops
            .iter()
            .map(|s| s.position)
            .max()
            .unwrap_or(0)
            .max(pen);
        let position = (after.div_euclid(self.interval) + 1).saturating_mul(self.interval);
        Some(TabStop {
            position,
            align: TabAlign::Start,
        })
    }
}

/// What a tab's advance depends on: where the text before it ends, and
/// the text after it up to the next tab or the line's end.
pub(crate) struct After {
    /// The width of the text after the tab.
    pub width: i64,
    /// The width of that text before its first `decimal`, if it has one;
    /// for each decimal character the stops ask about.
    pub before_decimal: Option<i64>,
}

/// How far a tab moves the pen, from `pen`, with `natural` its width as a
/// space: to the stop `tabs` gives, placing the text `after` it as the
/// stop asks, but never back.
pub(crate) fn advance(
    tabs: &Tabs,
    pen: i64,
    natural: i64,
    after: impl Fn(Option<char>) -> After,
) -> i64 {
    let Some(stop) = tabs.next(pen) else {
        return natural;
    };
    let to = stop.position - pen;
    let advance = match stop.align {
        TabAlign::Start => to,
        TabAlign::End => to - after(None).width,
        TabAlign::Center => to - after(None).width / 2,
        TabAlign::Decimal(c) => {
            let after = after(Some(c));
            to - after.before_decimal.unwrap_or(after.width)
        }
    };
    advance.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(position: i64, align: TabAlign) -> TabStop {
        TabStop { position, align }
    }

    #[test]
    fn stops_set_come_first_then_defaults() {
        let tabs = Tabs {
            stops: vec![stop(100, TabAlign::End), stop(250, TabAlign::Start)],
            interval: 80,
        };
        assert_eq!(tabs.next(0), Some(stop(100, TabAlign::End)));
        assert_eq!(tabs.next(100), Some(stop(250, TabAlign::Start)));
        assert_eq!(tabs.next(250), Some(stop(320, TabAlign::Start)));
        assert_eq!(tabs.next(330), Some(stop(400, TabAlign::Start)));
        assert_eq!(Tabs::default().next(0), None);
    }

    #[test]
    fn text_sits_at_its_stop() {
        let after = |width, before| {
            move |_: Option<char>| After {
                width,
                before_decimal: before,
            }
        };
        let at = |align| Tabs {
            stops: vec![stop(100, align)],
            interval: 0,
        };
        assert_eq!(advance(&at(TabAlign::Start), 30, 5, after(40, None)), 70);
        assert_eq!(advance(&at(TabAlign::End), 30, 5, after(40, None)), 30);
        assert_eq!(advance(&at(TabAlign::Center), 30, 5, after(40, None)), 50);
        assert_eq!(
            advance(&at(TabAlign::Decimal('.')), 30, 5, after(40, Some(25))),
            45
        );
        assert_eq!(
            advance(&at(TabAlign::Decimal('.')), 30, 5, after(40, None)),
            30
        );
        // Text too wide to end at the stop starts where the pen is.
        assert_eq!(advance(&at(TabAlign::End), 30, 5, after(90, None)), 0);
        // Past every stop, a tab is a space.
        assert_eq!(advance(&at(TabAlign::Start), 120, 5, after(40, None)), 5);
    }
}
