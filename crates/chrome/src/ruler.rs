//! The ruler: a text column, measured, with its paragraph's indents and
//! tab stops, which a person sets and drags.

use crate::Unit;

/// A ruler over a text column, measured from the column's start. Its
/// paragraph's indents and tab stops are measured from the start of the
/// paragraph's own place in it, which a list or a quote moves in.
/// Positions are in layout units across the ruler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruler {
    /// Where the column starts, from the ruler's left.
    pub start: i64,
    /// How wide the column is.
    pub width: i64,
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
}
