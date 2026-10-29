//! The inspector: the selection's properties as typed fields, which a part
//! describes and the chrome shows (docs/ARCHITECTURE.md, sections 4 and
//! 8). Each field runs a command with what's chosen or typed in it, so a
//! port can build its platform's own panels from the same description.
//! A tab may be taller than the panel; the panel scrolls, which is the
//! chrome's to do, not the model's.

use crate::{Arg, Length};

/// The inspector's contents: tabs of sections of rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspector {
    /// Its tabs, such as Text and Page.
    pub tabs: Vec<InspectorTab>,
}

/// A tab of the inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectorTab {
    /// Its title.
    pub title: String,
    /// Its sections, top to bottom.
    pub sections: Vec<Section>,
}

/// A section of a tab, under a heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Its heading, such as "Paragraph".
    pub title: String,
    /// Its rows, top to bottom.
    pub rows: Vec<Row>,
}

/// A row: a label, and the controls beside it, sharing its width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What it sets, such as "Spacing"; empty for none.
    pub label: String,
    /// Its controls, left to right.
    pub controls: Vec<Control>,
}

/// A control: what it shows, and the command it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// The command it runs.
    pub command: String,
    /// What the command is run with, before the control's own value: such
    /// as which tab stop a length moves.
    pub args: Vec<Arg>,
    /// Whether it can be used.
    pub enabled: bool,
    /// What screen readers call it, where its row's label and its own
    /// words don't say it, as "Remove tab stop 1" for a button that shows
    /// a cross beside a stop's Position; `None` for one they name.
    pub name: Option<String>,
    /// What kind it is, and what it shows.
    pub kind: ControlKind,
}

/// The kinds of control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// One of several, chosen from a list that pops up: the command runs
    /// with the choice's value.
    Choice {
        /// The choices.
        choices: Vec<Choice>,
        /// The one that's set; `None` where the selection mixes several.
        selected: Option<usize>,
        /// Shown as text alone, without a field's box.
        plain: bool,
    },
    /// A length, typed with its unit or in the one shown: the command runs
    /// with it in layout units.
    Length {
        /// It as it stands; `None` where the selection mixes several.
        value: Option<Length>,
        /// What's shown before it, such as an arrow for space above.
        prefix: String,
        /// Shown as text alone, without a field's box.
        plain: bool,
    },
    /// On or off, shown as an icon: the command runs as it is, to turn it
    /// over.
    Toggle {
        /// The icon's name (Lucide's).
        icon: String,
        /// What it's called, for screen readers and ports without icons.
        label: String,
        /// Whether it's on.
        on: bool,
    },
    /// One of several, side by side: each runs a command of its own.
    Segments {
        /// The segments.
        segments: Vec<Segment>,
        /// The one that's on; `None` where the selection mixes several.
        selected: Option<usize>,
    },
    /// A button: the command runs as it is.
    Button {
        /// What it says, or is called, for one with an icon.
        label: String,
        /// Its icon, if it shows one rather than its words.
        icon: Option<String>,
    },
    /// Text, typed: the command runs with it, such as a style's name.
    Text {
        /// It as it stands.
        value: String,
    },
    /// Words that say something of the row, and run nothing: where a
    /// value comes from, such as "Heading" or "defaults".
    Note {
        /// What it says.
        text: String,
    },
    /// A colour, shown as a swatch and typed as `#rrggbb`: the command
    /// runs with what's typed, as text.
    Colour {
        /// It as it stands, sRGB; `None` where there's none to show.
        value: Option<[u8; 3]>,
    },
}

/// A choice of a [`ControlKind::Choice`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What the command runs with.
    pub value: Arg,
    /// What it's called.
    pub label: String,
    /// The font family its name is shown in, where it's a family: a font
    /// picker shows each family in itself.
    pub family: Option<String>,
    /// A colour shown before its name, sRGB, where it's a colour.
    pub swatch: Option<[u8; 3]>,
}

impl Choice {
    /// A choice called `label` that runs its command with `value`.
    pub fn new(value: Arg, label: impl Into<String>) -> Choice {
        Choice {
            value,
            label: label.into(),
            family: None,
            swatch: None,
        }
    }

    /// It, its name shown in `family`.
    pub fn in_family(self, family: impl Into<String>) -> Choice {
        Choice {
            family: Some(family.into()),
            ..self
        }
    }

    /// It, with a swatch of `colour` before its name.
    pub fn with_swatch(self, colour: [u8; 3]) -> Choice {
        Choice {
            swatch: Some(colour),
            ..self
        }
    }
}

/// A segment of a [`ControlKind::Segments`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// The command it runs.
    pub command: String,
    /// What it's called.
    pub label: String,
    /// Its icon, if it shows one rather than its words.
    pub icon: Option<String>,
}

impl Control {
    /// The arguments the control runs its command with, given its value:
    /// those it carries, then the value.
    pub fn with(&self, value: Arg) -> Vec<Arg> {
        let mut args = self.args.clone();
        args.push(value);
        args
    }
}
