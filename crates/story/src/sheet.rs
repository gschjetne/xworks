//! Stylesheets as files: those that come with xWorks (`spec/styles/`) and
//! people's own, kept in their configuration folder (docs/ARCHITECTURE.md,
//! section 6).
//!
//! A file names its faces, colours, tab layouts and list definitions, the
//! named parts its styles refer to; its page style; the defaults every
//! chain of styles ends in; its paragraph and character styles, each based
//! on another of its kind by name, or on none; and what the whole document
//! shares: its default tab stops, and how quotes, code and tables look.
//! [`SheetSpec::objects`] makes it the document's own, styles and parts
//! named by ID; [`SheetSpec::of`] writes a document's back out as a file.
//!
//! A file from outside is never trusted: [`SheetSpec::parse`] checks every
//! limit and every name a style refers to, and never panics.

use std::collections::{BTreeMap, BTreeSet};

use doc::{Document, Object, ObjectId, Props, Sheet, Style, StyleKind, Value};
use serde::{Deserialize, Serialize};
use text::units::{INCH, Length, MILLIMETRE, POINT};
use text::{Align, Ligatures, TabAlign, TabStop};

use crate::{TabStopSpec, format_tabs, keys, parse_tabs};

/// A stylesheet, as a file gives it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SheetSpec {
    /// Its name, as File, New and the inspector show it: "Neutral".
    pub name: String,
    /// Its version: a sheet that comes with xWorks counts up each time it
    /// changes, so a document that copied it can be offered the change.
    pub version: u32,
    /// The faces its styles are set in: font families, by name.
    #[serde(default)]
    pub faces: Vec<FaceSpec>,
    /// Its colours, by name.
    #[serde(default)]
    pub colours: Vec<ColourSpec>,
    /// Its layouts of tab stops, by name.
    #[serde(default)]
    pub tabs: Vec<TabsSpec>,
    /// How its lists are marked and indented, by name.
    #[serde(default)]
    pub lists: Vec<ListSpec>,
    /// Its page style.
    pub page: PageSpec,
    /// The values every chain of styles ends in. A face or a family, a
    /// size and a colour must be among them; what else isn't is off, none
    /// or nothing.
    pub defaults: StyleSpec,
    /// Its paragraph styles, the one with the role "body" the default.
    #[serde(default)]
    pub paragraph: Vec<StyleSpec>,
    /// Its character styles, among them those of the marks: "emphasis",
    /// "strong", "code", "link" and "strikethrough".
    #[serde(default)]
    pub character: Vec<StyleSpec>,
    /// What the whole document shares.
    pub document: DocumentSpec,
}

/// A face: a font family, by a name styles refer to it by.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FaceSpec {
    /// Its name: "Text", "Heading".
    pub name: String,
    /// What it's for, by which another sheet's face matches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The font family.
    pub family: String,
}

/// A colour, by a name styles refer to it by.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColourSpec {
    /// Its name: "Ink", "Link".
    pub name: String,
    /// What it's for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The colour, sRGB, as `#rrggbb`.
    pub colour: String,
}

/// A layout of tab stops, by a name styles refer to it by.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TabsSpec {
    /// Its name.
    pub name: String,
    /// What it's for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Its stops.
    pub stops: Vec<TabStopSpec>,
}

/// How a list is marked and indented, by a name.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListSpec {
    /// Its name: "Bullets", "Numbers".
    pub name: String,
    /// What it's for: "bullets" for lists that aren't numbered, "numbers"
    /// for those that are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The bullets of items, by depth from the outermost, over again
    /// deeper; for a numbered list, those of lists nested in it that
    /// aren't.
    #[serde(default)]
    pub bullets: Vec<String>,
    /// How a number reads, `1` standing for it: "1." or "(1)". Empty for
    /// bullets.
    #[serde(default)]
    pub numbers: String,
    /// How far each level indents its items.
    pub indent: Length,
    /// The room between a marker and its item's text.
    pub gap: Length,
    /// Levels set apart from the rest, each with what it sets of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<LevelSpec>,
}

/// A level of a list definition, set apart: what it sets of its own, over
/// what the definition gives every level.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LevelSpec {
    /// Which level, from 1 for the outermost lists.
    pub level: usize,
    /// "bullet", or how its items are numbered: "1", "a", "A", "i" or
    /// "I".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// Its bullet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bullet: Option<String>,
    /// How its number reads, `1` standing for it: "(1)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reads: Option<String>,
    /// The number its items start at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// How far it indents its items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<Length>,
    /// The room between its markers and its items' text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Length>,
}

/// A page style: a page's size and margins.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageSpec {
    /// Its name.
    #[serde(default = "page_name")]
    pub name: String,
    /// Its width.
    pub width: Length,
    /// Its height.
    pub height: Length,
    /// The margin at the top.
    pub top: Length,
    /// The margin at the bottom.
    pub bottom: Length,
    /// The margin at the left.
    pub left: Length,
    /// The margin at the right.
    pub right: Length,
}

fn page_name() -> String {
    "Page".to_string()
}

/// A style, as a file gives it, or the defaults: each value it sets, the
/// rest left to the style it's based on. Lengths are lengths; a line
/// height is in thousandths of the largest font on the line.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleSpec {
    /// Its name, which paragraphs show and another sheet's matches.
    pub name: String,
    /// What it's for, by which another sheet's style matches it: "body",
    /// "heading-1", "code".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The style of its kind it's based on, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    /// A face its text is set in, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// A font family its text is set in, where it names no face.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Its text's size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Length>,
    /// Whether its text is bold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Whether its text is italic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Its text's colour: a colour by name, or `#rrggbb`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// Whether its text is underlined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Whether its text is struck through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// Which classes of ligature its text is set with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ligatures: Option<Ligatures>,
    /// How its lines are aligned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// The space above it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<Length>,
    /// The space below it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<Length>,
    /// How tall its lines are, in thousandths of their largest font; 0 as
    /// tall as their fonts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<u32>,
    /// Its indent from the start edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_start: Option<Length>,
    /// Its indent from the end edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_end: Option<Length>,
    /// How much further its first line is indented; less than none hangs
    /// it out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_first: Option<Length>,
    /// Its tab stops: a layout of them, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs: Option<String>,
    /// Its tab stops themselves, where it names no layout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stops: Option<Vec<TabStopSpec>>,
    /// Whether it stays on the page where the next paragraph starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    /// Its level as a heading, 1 to 6; 0 for none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<u8>,
    /// Whether its words are hyphenated; where nothing says, they are
    /// where its lines are justified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyphenate: Option<bool>,
    /// Whether it's a code block's: Enter breaks its lines, and it's drawn
    /// on a tint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<bool>,
    /// Whether it's numbered, as headings are in a technical document:
    /// "1.2" for the second heading of its level after the first above it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numbered: Option<bool>,
}

/// What the whole document shares, as a file gives it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSpec {
    /// The distance between default tab stops.
    pub tab_interval: Length,
    /// The tab stops every paragraph has besides its style's, a layout by
    /// name; none if it's left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<String>,
    /// How far a quote indents its blocks, its bar included.
    pub quote_indent: Length,
    /// How wide a quote's bar is.
    pub quote_bar: Length,
    /// The colour of a quote's bar: a colour by name, or `#rrggbb`.
    pub quote_colour: String,
    /// The space above and below a quote, as a table has its own: added
    /// to the space its first block has above it and its last below. None
    /// where it's left out, as in sheets from before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_space: Option<Length>,
    /// The tint behind code.
    pub code_tint: String,
    /// How far a code block's tint reaches around its text.
    pub code_padding: Length,
    /// The space above and below a table.
    pub table_space: Length,
    /// The room around a cell's text.
    pub cell_padding: Length,
    /// The width of a table's rules above and below it, and under its
    /// header.
    pub rule: Length,
    /// The width of the rules between its rows.
    pub hairline: Length,
    /// The colour of a table's rules.
    pub rule_colour: String,
    /// The colour of the rules between its rows.
    pub hairline_colour: String,
    /// The room above and below a rule across the text, Markdown's
    /// thematic break, drawn a hairline wide in the rules' colour; the
    /// program's where it's left out, as in sheets from before rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_space: Option<Length>,
    /// The colour drawn where a picture can't be.
    pub missing_picture: String,
}

/// Why a stylesheet file can't be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SheetError {
    /// It isn't a stylesheet's RON.
    #[error("it isn't a stylesheet: {0}")]
    Syntax(String),
    /// It is, but something in it doesn't hold together or is past a
    /// limit.
    #[error("{0}")]
    Invalid(String),
}

/// The most styles of each kind, and parts of each kind, a sheet may have.
pub const MOST_STYLES: usize = 512;
/// The most characters a name, a role or a family may have.
const MOST_NAME: usize = 100;
/// The longest a stylesheet file may be, in bytes.
pub const MOST_BYTES: usize = 1 << 20;

/// The most a paragraph's space above or below, or an indent, may be.
const MOST_SPACE: i64 = 10 * INCH;
/// The most a tab stop's position may be.
const MOST_TAB: i64 = 100 * INCH;

fn invalid<T>(why: impl Into<String>) -> Result<T, SheetError> {
    Err(SheetError::Invalid(why.into()))
}

/// The options stylesheet files are read and written with: `Some` may be
/// left out around a value set.
fn options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// A colour as `#rrggbb` reads, if it is one.
pub fn parse_colour(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// A colour written as `#rrggbb`.
pub fn format_colour((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `units` as a length in the plainest unit that holds it exactly: whole
/// inches, tenths of a millimetre, tenths of a point, or else points.
pub fn length_of(units: i64) -> Length {
    if units != 0 && units % INCH == 0 {
        Length::In((units / INCH) as f64)
    } else if units % (MILLIMETRE / 10) == 0 && units % (POINT / 10) != 0 {
        Length::Mm(units as f64 / MILLIMETRE as f64)
    } else {
        Length::Pt(units as f64 / POINT as f64)
    }
}

/// Ligatures as [`keys::LIGATURES`] holds them: the classes that are on,
/// by name, separated by spaces.
pub fn format_ligatures(l: Ligatures) -> String {
    [
        (l.standard, "standard"),
        (l.contextual, "contextual"),
        (l.discretionary, "discretionary"),
        (l.historical, "historical"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| *name)
    .collect::<Vec<_>>()
    .join(" ")
}

/// The ligatures [`format_ligatures`] wrote; classes it doesn't know are
/// left off.
pub fn parse_ligatures(text: &str) -> Ligatures {
    let on = |name: &str| text.split_whitespace().any(|w| w == name);
    Ligatures {
        standard: on("standard"),
        contextual: on("contextual"),
        discretionary: on("discretionary"),
        historical: on("historical"),
    }
}

/// Alignment as [`keys::ALIGN`] holds it.
pub fn align_name(align: Align) -> &'static str {
    match align {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
        Align::Justify => "justify",
    }
}

/// The alignment [`align_name`] names.
pub fn align_of(name: &str) -> Option<Align> {
    match name {
        "start" => Some(Align::Start),
        "center" => Some(Align::Center),
        "end" => Some(Align::End),
        "justify" => Some(Align::Justify),
        _ => None,
    }
}

/// Checks a name, a role or a family: some text, not too long.
fn check_name(what: &str, name: &str) -> Result<(), SheetError> {
    let n = name.chars().count();
    if n == 0 || n > MOST_NAME || name.chars().any(char::is_control) {
        return invalid(format!(
            "{what} {name:?} must be 1 to {MOST_NAME} characters"
        ));
    }
    Ok(())
}

fn check_length(
    what: &str,
    length: Length,
    range: std::ops::RangeInclusive<i64>,
) -> Result<i64, SheetError> {
    let n = match length {
        Length::Pt(n) | Length::Mm(n) | Length::In(n) | Length::Px(n) => n,
    };
    let units = length.units();
    if !n.is_finite() || !range.contains(&units) {
        return invalid(format!("{what} is out of range"));
    }
    Ok(units)
}

/// Checks that names are unique, each a good name, and that there aren't
/// too many; returns them.
fn names<'a>(
    what: &str,
    items: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<BTreeSet<&'a str>, SheetError> {
    let mut seen = BTreeSet::new();
    let mut roles = BTreeSet::new();
    for (name, role) in items {
        check_name(what, name)?;
        if !seen.insert(name) {
            return invalid(format!("two {what}s are called {name:?}"));
        }
        if let Some(role) = role {
            check_name("a role", role)?;
            if !roles.insert(role) {
                return invalid(format!("two {what}s have the role {role:?}"));
            }
        }
        if seen.len() > MOST_STYLES {
            return invalid(format!("more than {MOST_STYLES} {what}s"));
        }
    }
    Ok(seen)
}

impl SheetSpec {
    /// The stylesheet file `text`, if it is one and holds together: every
    /// name a style refers to is there, no style is based on itself, and
    /// every value is within its limits.
    pub fn parse(text: &str) -> Result<SheetSpec, SheetError> {
        if text.len() > MOST_BYTES {
            return invalid("the file is larger than a megabyte");
        }
        let spec: SheetSpec = options()
            .from_str(text)
            .map_err(|e| SheetError::Syntax(e.to_string()))?;
        spec.check()?;
        Ok(spec)
    }

    /// The stylesheet as a file, which [`SheetSpec::parse`] reads back.
    pub fn write(&self) -> String {
        let config = ron::ser::PrettyConfig::new()
            .extensions(ron::extensions::Extensions::IMPLICIT_SOME)
            .struct_names(false);
        options().to_string_pretty(self, config).unwrap_or_default()
    }

    /// Checks everything [`SheetSpec::parse`] promises.
    pub fn check(&self) -> Result<(), SheetError> {
        check_name("the stylesheet's name", &self.name)?;
        if self.version == 0 {
            return invalid("the version must be 1 or more");
        }
        let faces = names(
            "face",
            self.faces.iter().map(|f| (&*f.name, f.role.as_deref())),
        )?;
        for face in &self.faces {
            check_name("a family", &face.family)?;
        }
        let colours = names(
            "colour",
            self.colours.iter().map(|c| (&*c.name, c.role.as_deref())),
        )?;
        for colour in &self.colours {
            if colour.name.starts_with('#') {
                return invalid(format!(
                    "a colour's name can't start with #: {:?}",
                    colour.name
                ));
            }
            if parse_colour(&colour.colour).is_none() {
                return invalid(format!("{:?} isn't a colour", colour.colour));
            }
        }
        let tabs = names(
            "tab layout",
            self.tabs.iter().map(|t| (&*t.name, t.role.as_deref())),
        )?;
        for layout in &self.tabs {
            check_stops(&layout.stops)?;
        }
        names(
            "list",
            self.lists.iter().map(|l| (&*l.name, l.role.as_deref())),
        )?;
        for list in &self.lists {
            if list.bullets.len() > 8
                || list.bullets.iter().any(|b| {
                    b.is_empty() || b.chars().count() > 8 || b.chars().any(char::is_whitespace)
                })
            {
                return invalid(format!(
                    "{:?}'s bullets must be 8 at most, each a few characters",
                    list.name
                ));
            }
            if list.numbers.chars().count() > 16
                || (!list.numbers.is_empty() && !list.numbers.contains('1'))
            {
                return invalid(format!(
                    "{:?}'s numbers must hold a 1 for the number",
                    list.name
                ));
            }
            check_length("a list's indent", list.indent, 0..=INCH * 4)?;
            check_length("a list's gap", list.gap, 0..=INCH * 2)?;
            let mut seen = BTreeSet::new();
            for level in &list.levels {
                if !(1..=keys::LEVELS).contains(&level.level) || !seen.insert(level.level) {
                    return invalid(format!(
                        "{:?}'s levels must be 1 to {}, each once",
                        list.name,
                        keys::LEVELS
                    ));
                }
                if level
                    .marker
                    .as_deref()
                    .is_some_and(|m| crate::Numbering::of(m).is_none())
                {
                    return invalid(format!(
                        "{:?}'s level {} marker must be \"bullet\", \"1\", \"a\", \"A\", \"i\" or \"I\"",
                        list.name, level.level
                    ));
                }
                if level
                    .bullet
                    .as_deref()
                    .is_some_and(|b| !crate::styles::valid_bullet(b))
                    || level
                        .reads
                        .as_deref()
                        .is_some_and(|r| !crate::styles::valid_reads(r))
                    || level
                        .start
                        .is_some_and(|s| !(0..=crate::styles::MOST_START).contains(&s))
                {
                    return invalid(format!(
                        "{:?}'s level {} must have a bullet of a few characters, a number \
                         that reads with a 1 for it, and a start from 0 to {}",
                        list.name,
                        level.level,
                        crate::styles::MOST_START
                    ));
                }
                for length in [level.indent, level.gap].into_iter().flatten() {
                    check_length("a list level's indent or gap", length, 0..=INCH)?;
                }
            }
        }
        check_name("the page's name", &self.page.name)?;
        let page = &self.page;
        let width = check_length(
            "the page's width",
            page.width,
            10 * MILLIMETRE..=2000 * MILLIMETRE,
        )?;
        let height = check_length(
            "the page's height",
            page.height,
            10 * MILLIMETRE..=2000 * MILLIMETRE,
        )?;
        let top = check_length("the top margin", page.top, 0..=height / 2)?;
        let bottom = check_length("the bottom margin", page.bottom, 0..=height / 2)?;
        let left = check_length("the left margin", page.left, 0..=width / 2)?;
        let right = check_length("the right margin", page.right, 0..=width / 2)?;
        if top + bottom >= height || left + right >= width {
            return invalid("the margins leave no room on the page");
        }
        let refs = Names {
            faces: &faces,
            colours: &colours,
            tabs: &tabs,
        };
        refs.check_style(&self.defaults)?;
        if self.defaults.face.is_none() && self.defaults.family.is_none() {
            return invalid("the defaults must name a face or a family");
        }
        if self.defaults.size.is_none() || self.defaults.colour.is_none() {
            return invalid("the defaults must give a size and a colour");
        }
        for (what, styles) in [
            ("paragraph style", &self.paragraph),
            ("character style", &self.character),
        ] {
            let all = names(what, styles.iter().map(|s| (&*s.name, s.role.as_deref())))?;
            for style in styles {
                refs.check_style(style)?;
                if let Some(base) = &style.based_on
                    && !all.contains(base.as_str())
                {
                    return invalid(format!(
                        "{:?} is based on {base:?}, which isn't a {what}",
                        style.name
                    ));
                }
            }
            // No chain comes back on itself.
            for style in styles {
                let mut at = style.based_on.as_deref();
                let mut steps = 0;
                while let Some(base) = at {
                    steps += 1;
                    if steps > styles.len() {
                        return invalid(format!("{:?} is based on itself", style.name));
                    }
                    at = styles
                        .iter()
                        .find(|s| s.name == base)
                        .and_then(|s| s.based_on.as_deref());
                }
            }
        }
        let d = &self.document;
        check_length("the tab interval", d.tab_interval, MILLIMETRE..=MOST_TAB)?;
        if let Some(t) = &d.tabs
            && !tabs.contains(t.as_str())
        {
            return invalid(format!(
                "the document's tab stops name {t:?}, which isn't a layout"
            ));
        }
        for (what, length) in [
            ("a quote's indent", d.quote_indent),
            ("a quote's bar", d.quote_bar),
            ("code's padding", d.code_padding),
            ("a table's space", d.table_space),
            ("a cell's padding", d.cell_padding),
            ("a rule", d.rule),
            ("a hairline", d.hairline),
        ] {
            check_length(what, length, 0..=INCH)?;
        }
        if let Some(space) = d.rule_space {
            check_length("a rule's space", space, 0..=INCH)?;
        }
        if let Some(space) = d.quote_space {
            check_length("a quote's space", space, 0..=INCH)?;
        }
        for colour in [
            &d.quote_colour,
            &d.code_tint,
            &d.rule_colour,
            &d.hairline_colour,
            &d.missing_picture,
        ] {
            refs.check_colour(colour)?;
        }
        Ok(())
    }
}

/// Checks tab stops: in order, within limits.
fn check_stops(stops: &[TabStopSpec]) -> Result<(), SheetError> {
    if stops.len() > 64 {
        return invalid("more than 64 tab stops");
    }
    for stop in stops {
        check_length("a tab stop", stop.at, 0..=MOST_TAB)?;
        if let TabAlign::Decimal(c) = stop.align
            && c.is_control()
        {
            return invalid("a decimal tab stop on a control character");
        }
        if stop.leader.is_some_and(|c| !crate::valid_leader(c)) {
            return invalid("a tab stop's leader must be a character that shows");
        }
    }
    Ok(())
}

/// The names of a sheet's parts, which its styles refer to.
struct Names<'a> {
    faces: &'a BTreeSet<&'a str>,
    colours: &'a BTreeSet<&'a str>,
    tabs: &'a BTreeSet<&'a str>,
}

impl Names<'_> {
    fn check_colour(&self, colour: &str) -> Result<(), SheetError> {
        if parse_colour(colour).is_none() && !self.colours.contains(colour) {
            return invalid(format!("{colour:?} isn't a colour, nor the name of one"));
        }
        Ok(())
    }

    fn check_style(&self, s: &StyleSpec) -> Result<(), SheetError> {
        if let Some(face) = &s.face
            && !self.faces.contains(face.as_str())
        {
            return invalid(format!(
                "{:?} names the face {face:?}, which isn't there",
                s.name
            ));
        }
        if let Some(family) = &s.family {
            check_name("a family", family)?;
        }
        if let Some(colour) = &s.colour {
            self.check_colour(colour)?;
        }
        if let Some(tabs) = &s.tabs
            && !self.tabs.contains(tabs.as_str())
        {
            return invalid(format!(
                "{:?} names the tab layout {tabs:?}, which isn't there",
                s.name
            ));
        }
        if let Some(stops) = &s.stops {
            check_stops(stops)?;
        }
        if let Some(size) = s.size {
            check_length("a size", size, POINT..=1000 * POINT)?;
        }
        for (what, length) in [
            ("space above", s.space_before),
            ("space below", s.space_after),
        ] {
            if let Some(length) = length {
                check_length(what, length, 0..=MOST_SPACE)?;
            }
        }
        for length in [s.indent_start, s.indent_end, s.indent_first]
            .into_iter()
            .flatten()
        {
            check_length("an indent", length, -MOST_SPACE..=MOST_SPACE)?;
        }
        if let Some(h) = s.line_height
            && h != 0
            && !(250..=10_000).contains(&h)
        {
            return invalid("a line height must be from 250 to 10 000 thousandths, or 0");
        }
        if s.heading.is_some_and(|h| h > 6) {
            return invalid("a heading's level must be 1 to 6, or 0");
        }
        Ok(())
    }
}

/// What a stylesheet's objects are made from: IDs by name, kind by kind.
struct Ids {
    faces: BTreeMap<String, ObjectId>,
    colours: BTreeMap<String, ObjectId>,
    tabs: BTreeMap<String, ObjectId>,
}

impl Ids {
    fn colour(&self, colour: &str) -> Value {
        match self.colours.get(colour) {
            Some(&id) => Value::Ref(id),
            None => Value::Text(colour.to_string()),
        }
    }

    /// A style's values as properties.
    fn values(&self, s: &StyleSpec) -> Props {
        let mut props = Props::new();
        let mut put = |key: &str, value: Value| {
            props.insert(key.to_string(), value);
        };
        if let Some(face) = s.face.as_ref().and_then(|f| self.faces.get(f)) {
            put(keys::FONT, Value::Ref(*face));
        } else if let Some(family) = &s.family {
            put(keys::FONT, Value::Text(family.clone()));
        }
        if let Some(size) = s.size {
            put(keys::SIZE, Value::Int(size.units()));
        }
        for (key, on) in [
            (keys::BOLD, s.bold),
            (keys::ITALIC, s.italic),
            (keys::UNDERLINE, s.underline),
            (keys::STRIKETHROUGH, s.strikethrough),
            (keys::KEEP_WITH_NEXT, s.keep_with_next),
            (keys::HYPHENATE, s.hyphenate),
            (keys::CODE, s.code),
            (keys::NUMBERED, s.numbered),
        ] {
            if let Some(on) = on {
                put(key, Value::Bool(on));
            }
        }
        if let Some(colour) = &s.colour {
            put(keys::COLOR, self.colour(colour));
        }
        if let Some(l) = s.ligatures {
            put(keys::LIGATURES, Value::Text(format_ligatures(l)));
        }
        if let Some(align) = s.align {
            put(keys::ALIGN, Value::Text(align_name(align).to_string()));
        }
        for (key, length) in [
            (keys::SPACE_BEFORE, s.space_before),
            (keys::SPACE_AFTER, s.space_after),
            (keys::INDENT_START, s.indent_start),
            (keys::INDENT_END, s.indent_end),
            (keys::INDENT_FIRST, s.indent_first),
        ] {
            if let Some(length) = length {
                put(key, Value::Int(length.units()));
            }
        }
        if let Some(h) = s.line_height {
            put(keys::LINE_HEIGHT, Value::Int(i64::from(h)));
        }
        if let Some(tabs) = s.tabs.as_ref().and_then(|t| self.tabs.get(t)) {
            put(keys::TABS, Value::Ref(*tabs));
        } else if let Some(stops) = &s.stops {
            put(keys::TABS, Value::Text(format_tabs(&stops_of(stops))));
        }
        if let Some(h) = s.heading {
            put(keys::HEADING, Value::Int(i64::from(h)));
        }
        props
    }
}

/// The values a stylesheet's defaults take where its file leaves them
/// out: off, none or nothing.
fn zeros() -> Vec<(&'static str, Value)> {
    let off = |key| (key, Value::Bool(false));
    let none = |key| (key, Value::Int(0));
    vec![
        off(keys::BOLD),
        off(keys::ITALIC),
        off(keys::UNDERLINE),
        off(keys::STRIKETHROUGH),
        off(keys::KEEP_WITH_NEXT),
        off(keys::CODE),
        off(keys::NUMBERED),
        (
            keys::ALIGN,
            Value::Text(align_name(Align::Start).to_string()),
        ),
        none(keys::SPACE_BEFORE),
        none(keys::SPACE_AFTER),
        none(keys::LINE_HEIGHT),
        none(keys::INDENT_START),
        none(keys::INDENT_END),
        none(keys::INDENT_FIRST),
        none(keys::HEADING),
        (keys::TABS, Value::Text(String::new())),
        (
            keys::LIGATURES,
            Value::Text(format_ligatures(Ligatures::default())),
        ),
    ]
}

fn stops_of(stops: &[TabStopSpec]) -> Vec<TabStop> {
    stops
        .iter()
        .map(|s| TabStop {
            position: s.at.units(),
            align: s.align,
            leader: s.leader,
        })
        .collect()
}

/// A part's or a style's identity: its name, and its role if it has one.
fn identity(name: &str, role: Option<&str>) -> Props {
    let mut props = Props::new();
    props.insert(keys::NAME.to_string(), Value::Text(name.to_string()));
    if let Some(role) = role {
        props.insert(keys::ROLE.to_string(), Value::Text(role.to_string()));
    }
    props
}

impl SheetSpec {
    /// The stylesheet as a document's objects, the sheet itself first at
    /// `first`, then its parts, its page style, and its paragraph and
    /// character styles, at the IDs after it, in order. It records the
    /// sheet's name and version, which it came from. The spec must hold
    /// together ([`SheetSpec::check`]); names it doesn't know are left
    /// out.
    pub fn objects(&self, first: u64) -> Vec<(ObjectId, Object)> {
        let sheet = ObjectId(first);
        let mut next = first + 1;
        let mut id = || {
            let id = ObjectId(next);
            next += 1;
            id
        };
        let mut styles: Vec<(ObjectId, Style)> = Vec::new();
        let mut part = |kind: StyleKind, props: Props, id: ObjectId| {
            styles.push((
                id,
                Style {
                    sheet,
                    kind,
                    base: None,
                    props,
                },
            ));
            id
        };
        let mut ids = Ids {
            faces: BTreeMap::new(),
            colours: BTreeMap::new(),
            tabs: BTreeMap::new(),
        };
        for face in &self.faces {
            let mut props = identity(&face.name, face.role.as_deref());
            props.insert(keys::FAMILY.to_string(), Value::Text(face.family.clone()));
            let made = part(StyleKind::Face, props, id());
            ids.faces.insert(face.name.clone(), made);
        }
        for colour in &self.colours {
            let mut props = identity(&colour.name, colour.role.as_deref());
            let rgb = parse_colour(&colour.colour).unwrap_or_default();
            props.insert(keys::COLOR.to_string(), Value::Text(format_colour(rgb)));
            let made = part(StyleKind::Colour, props, id());
            ids.colours.insert(colour.name.clone(), made);
        }
        for layout in &self.tabs {
            let mut props = identity(&layout.name, layout.role.as_deref());
            props.insert(
                keys::TABS.to_string(),
                Value::Text(format_tabs(&stops_of(&layout.stops))),
            );
            let made = part(StyleKind::Tabs, props, id());
            ids.tabs.insert(layout.name.clone(), made);
        }
        for list in &self.lists {
            let mut props = identity(&list.name, list.role.as_deref());
            props.insert(
                keys::BULLETS.to_string(),
                Value::Text(list.bullets.join(" ")),
            );
            props.insert(keys::NUMBERS.to_string(), Value::Text(list.numbers.clone()));
            props.insert(
                keys::LIST_INDENT.to_string(),
                Value::Int(list.indent.units()),
            );
            props.insert(keys::MARKER_GAP.to_string(), Value::Int(list.gap.units()));
            for level in &list.levels {
                let k = level.level;
                let mut put = |what: &str, value: Option<Value>| {
                    if let Some(value) = value {
                        props.insert(keys::level(k, what), value);
                    }
                };
                put(keys::LEVEL_MARKER, level.marker.clone().map(Value::Text));
                put(keys::LEVEL_BULLET, level.bullet.clone().map(Value::Text));
                put(keys::LEVEL_READS, level.reads.clone().map(Value::Text));
                put(keys::LEVEL_START, level.start.map(Value::Int));
                put(
                    keys::LEVEL_INDENT,
                    level.indent.map(|l| Value::Int(l.units())),
                );
                put(keys::LEVEL_GAP, level.gap.map(|l| Value::Int(l.units())));
            }
            part(StyleKind::List, props, id());
        }
        let page = &self.page;
        let mut props = identity(&page.name, Some("page"));
        for (key, length) in [
            (keys::PAGE_WIDTH, page.width),
            (keys::PAGE_HEIGHT, page.height),
            (keys::MARGIN_TOP, page.top),
            (keys::MARGIN_BOTTOM, page.bottom),
            (keys::MARGIN_LEFT, page.left),
            (keys::MARGIN_RIGHT, page.right),
        ] {
            props.insert(key.to_string(), Value::Int(length.units()));
        }
        part(StyleKind::Page, props, id());
        // Styles: IDs first, so bases can name styles further on.
        let kinds = [
            (StyleKind::Paragraph, &self.paragraph),
            (StyleKind::Character, &self.character),
        ];
        let mut by_name: BTreeMap<(StyleKind, &str), ObjectId> = BTreeMap::new();
        let mut made = Vec::new();
        for (kind, specs) in kinds {
            for s in specs {
                let at = id();
                by_name.insert((kind, s.name.as_str()), at);
                made.push((at, kind, s));
            }
        }
        for (at, kind, s) in made {
            let mut props = identity(&s.name, s.role.as_deref());
            props.extend(ids.values(s));
            let base = s
                .based_on
                .as_deref()
                .and_then(|b| by_name.get(&(kind, b)).copied());
            styles.push((
                at,
                Style {
                    sheet,
                    kind,
                    base,
                    props,
                },
            ));
        }
        // The sheet: its defaults, complete, so every chain ends in a value
        // and says it does; where it came from; and what the document
        // shares. A value the file leaves out of its defaults is off, none
        // or nothing; hyphenation alone is left to the alignment.
        let mut props = ids.values(&self.defaults);
        props.remove(keys::NAME);
        for (key, zero) in zeros() {
            props.entry(key.to_string()).or_insert(zero);
        }
        props.insert(keys::ORIGIN.to_string(), Value::Text(self.name.clone()));
        props.insert(
            keys::ORIGIN_VERSION.to_string(),
            Value::Int(i64::from(self.version)),
        );
        let d = &self.document;
        if let Some(tabs) = d.tabs.as_ref().and_then(|t| ids.tabs.get(t)) {
            props.insert(keys::DOCUMENT_TABS.to_string(), Value::Ref(*tabs));
        }
        for (key, length) in [
            (keys::TAB_INTERVAL, d.tab_interval),
            (keys::QUOTE_INDENT, d.quote_indent),
            (keys::QUOTE_BAR, d.quote_bar),
            (keys::CODE_PADDING, d.code_padding),
            (keys::TABLE_SPACE, d.table_space),
            (keys::CELL_PADDING, d.cell_padding),
            (keys::RULE, d.rule),
            (keys::HAIRLINE, d.hairline),
        ] {
            props.insert(key.to_string(), Value::Int(length.units()));
        }
        if let Some(space) = d.rule_space {
            props.insert(keys::RULE_SPACE.to_string(), Value::Int(space.units()));
        }
        if let Some(space) = d.quote_space {
            props.insert(keys::QUOTE_SPACE.to_string(), Value::Int(space.units()));
        }
        for (key, colour) in [
            (keys::QUOTE_COLOR, &d.quote_colour),
            (keys::CODE_TINT, &d.code_tint),
            (keys::RULE_COLOR, &d.rule_colour),
            (keys::HAIRLINE_COLOR, &d.hairline_colour),
            (keys::MISSING_PICTURE, &d.missing_picture),
        ] {
            props.insert(key.to_string(), ids.colour(colour));
        }
        let mut out = vec![(
            sheet,
            Object::Sheet(Sheet {
                styles: styles.iter().map(|(id, _)| *id).collect(),
                props,
            }),
        )];
        out.extend(styles.into_iter().map(|(id, s)| (id, Object::Style(s))));
        out
    }

    /// The stylesheet `document` holds, as a file would give it, if it has
    /// one: to keep as one's own. Values that don't read as a file writes
    /// them are left out, and a part whose name another of its kind has
    /// already is left out too.
    pub fn of(document: &Document) -> Option<SheetSpec> {
        let sheet = document.stylesheet()?;
        let name_of = |id: ObjectId| -> Option<String> {
            match document.style(id)?.props.get(keys::NAME) {
                Some(Value::Text(name)) => Some(name.clone()),
                _ => None,
            }
        };
        let text = |props: &Props, key: &str| match props.get(key) {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        };
        let int = |props: &Props, key: &str| match props.get(key) {
            Some(Value::Int(n)) => Some(*n),
            _ => None,
        };
        let role = |props: &Props| text(props, keys::ROLE);
        // Its page style: the first, which must give the page whole.
        let page = sheet
            .styles
            .iter()
            .filter_map(|&id| Some((id, document.style(id)?)))
            .find(|(_, s)| s.kind == StyleKind::Page)?;
        let get = |key: &str| int(&page.1.props, key).map(length_of);
        let page = PageSpec {
            name: name_of(page.0).unwrap_or_else(page_name),
            width: get(keys::PAGE_WIDTH)?,
            height: get(keys::PAGE_HEIGHT)?,
            top: get(keys::MARGIN_TOP)?,
            bottom: get(keys::MARGIN_BOTTOM)?,
            left: get(keys::MARGIN_LEFT)?,
            right: get(keys::MARGIN_RIGHT)?,
        };
        let colour = |key: &str| -> String {
            match sheet.props.get(key) {
                Some(Value::Ref(id)) => name_of(*id).unwrap_or_default(),
                Some(Value::Text(t)) => t.clone(),
                _ => String::new(),
            }
        };
        let length = |key: &str| int(&sheet.props, key).map(length_of);
        let document_spec = DocumentSpec {
            tab_interval: length(keys::TAB_INTERVAL)?,
            tabs: match sheet.props.get(keys::DOCUMENT_TABS) {
                Some(Value::Ref(id)) => name_of(*id),
                _ => None,
            },
            quote_indent: length(keys::QUOTE_INDENT)?,
            quote_bar: length(keys::QUOTE_BAR)?,
            quote_colour: colour(keys::QUOTE_COLOR),
            quote_space: int(&sheet.props, keys::QUOTE_SPACE).map(length_of),
            code_tint: colour(keys::CODE_TINT),
            code_padding: length(keys::CODE_PADDING)?,
            table_space: length(keys::TABLE_SPACE)?,
            cell_padding: length(keys::CELL_PADDING)?,
            rule: length(keys::RULE)?,
            hairline: length(keys::HAIRLINE)?,
            rule_colour: colour(keys::RULE_COLOR),
            hairline_colour: colour(keys::HAIRLINE_COLOR),
            rule_space: int(&sheet.props, keys::RULE_SPACE).map(length_of),
            missing_picture: colour(keys::MISSING_PICTURE),
        };
        let mut spec = SheetSpec {
            name: text(&sheet.props, keys::ORIGIN).unwrap_or_else(|| "Untitled".to_string()),
            version: int(&sheet.props, keys::ORIGIN_VERSION)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&v| v > 0)
                .unwrap_or(1),
            faces: Vec::new(),
            colours: Vec::new(),
            tabs: Vec::new(),
            lists: Vec::new(),
            page,
            defaults: spec_values(document, &sheet.props),
            paragraph: Vec::new(),
            character: Vec::new(),
            document: document_spec,
        };
        let mut seen: BTreeSet<(StyleKind, String)> = BTreeSet::new();
        for &id in &sheet.styles {
            let Some(style) = document.style(id) else {
                continue;
            };
            let Some(name) = name_of(id) else {
                continue;
            };
            if !seen.insert((style.kind, name.clone())) {
                continue;
            }
            let props = &style.props;
            match style.kind {
                StyleKind::Face => {
                    if let Some(family) = text(props, keys::FAMILY) {
                        spec.faces.push(FaceSpec {
                            name,
                            role: role(props),
                            family,
                        });
                    }
                }
                StyleKind::Colour => {
                    if let Some(colour) =
                        text(props, keys::COLOR).filter(|c| parse_colour(c).is_some())
                    {
                        spec.colours.push(ColourSpec {
                            name,
                            role: role(props),
                            colour,
                        });
                    }
                }
                StyleKind::Tabs => spec.tabs.push(TabsSpec {
                    name,
                    role: role(props),
                    stops: text(props, keys::TABS)
                        .and_then(|t| parse_tabs(&t))
                        .unwrap_or_default()
                        .iter()
                        .map(|s| TabStopSpec {
                            at: length_of(s.position),
                            align: s.align,
                            leader: s.leader,
                        })
                        .collect(),
                }),
                StyleKind::List => spec.lists.push(ListSpec {
                    name,
                    role: role(props),
                    bullets: text(props, keys::BULLETS)
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                    numbers: text(props, keys::NUMBERS).unwrap_or_default(),
                    indent: length_of(int(props, keys::LIST_INDENT).unwrap_or(0)),
                    gap: length_of(int(props, keys::MARKER_GAP).unwrap_or(0)),
                    // What each level sets of its own, where it holds: so
                    // the file reads back.
                    levels: (1..=keys::LEVELS)
                        .map(|k| {
                            let text = |what: &str| text(props, &keys::level(k, what));
                            let int = |what: &str| int(props, &keys::level(k, what));
                            let length = |what: &str| {
                                int(what).filter(|n| (0..=INCH).contains(n)).map(length_of)
                            };
                            LevelSpec {
                                level: k,
                                marker: text(keys::LEVEL_MARKER)
                                    .filter(|m| crate::Numbering::of(m).is_some()),
                                bullet: text(keys::LEVEL_BULLET)
                                    .filter(|b| crate::styles::valid_bullet(b)),
                                reads: text(keys::LEVEL_READS)
                                    .filter(|r| crate::styles::valid_reads(r)),
                                start: int(keys::LEVEL_START)
                                    .filter(|s| (0..=crate::styles::MOST_START).contains(s)),
                                indent: length(keys::LEVEL_INDENT),
                                gap: length(keys::LEVEL_GAP),
                            }
                        })
                        .filter(|l| {
                            *l != LevelSpec {
                                level: l.level,
                                ..LevelSpec::default()
                            }
                        })
                        .collect(),
                }),
                StyleKind::Page => {}
                StyleKind::Paragraph | StyleKind::Character => {
                    let mut s = spec_values(document, props);
                    s.name = name;
                    s.role = role(props);
                    s.based_on = style.base.and_then(name_of);
                    if style.kind == StyleKind::Paragraph {
                        spec.paragraph.push(s);
                    } else {
                        spec.character.push(s);
                    }
                }
            }
        }
        Some(spec)
    }
}

/// The values a style's or the defaults' properties hold, as a file
/// writes them, parts by name.
fn spec_values(document: &Document, props: &Props) -> StyleSpec {
    let name_of = |id: &ObjectId| match document.style(*id).and_then(|s| s.props.get(keys::NAME)) {
        Some(Value::Text(name)) => Some(name.clone()),
        _ => None,
    };
    let flag = |key: &str| match props.get(key) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    };
    let length = |key: &str| match props.get(key) {
        Some(Value::Int(n)) => Some(length_of(*n)),
        _ => None,
    };
    let mut s = StyleSpec::default();
    match props.get(keys::FONT) {
        Some(Value::Ref(id)) => s.face = name_of(id),
        Some(Value::Text(family)) => s.family = Some(family.clone()),
        _ => {}
    }
    s.size = length(keys::SIZE);
    s.bold = flag(keys::BOLD);
    s.italic = flag(keys::ITALIC);
    s.colour = match props.get(keys::COLOR) {
        Some(Value::Ref(id)) => name_of(id),
        Some(Value::Text(c)) if parse_colour(c).is_some() => Some(c.clone()),
        _ => None,
    };
    s.underline = flag(keys::UNDERLINE);
    s.strikethrough = flag(keys::STRIKETHROUGH);
    s.ligatures = match props.get(keys::LIGATURES) {
        Some(Value::Text(t)) => Some(parse_ligatures(t)),
        _ => None,
    };
    s.align = match props.get(keys::ALIGN) {
        Some(Value::Text(a)) => align_of(a),
        _ => None,
    };
    s.space_before = length(keys::SPACE_BEFORE);
    s.space_after = length(keys::SPACE_AFTER);
    s.line_height = match props.get(keys::LINE_HEIGHT) {
        Some(Value::Int(n)) => u32::try_from(*n).ok(),
        _ => None,
    };
    s.indent_start = length(keys::INDENT_START);
    s.indent_end = length(keys::INDENT_END);
    s.indent_first = length(keys::INDENT_FIRST);
    match props.get(keys::TABS) {
        Some(Value::Ref(id)) => s.tabs = name_of(id),
        Some(Value::Text(t)) => {
            s.stops = parse_tabs(t).map(|stops| {
                stops
                    .iter()
                    .map(|st| TabStopSpec {
                        at: length_of(st.position),
                        align: st.align,
                        leader: st.leader,
                    })
                    .collect()
            });
        }
        _ => {}
    }
    s.keep_with_next = flag(keys::KEEP_WITH_NEXT);
    s.heading = match props.get(keys::HEADING) {
        Some(Value::Int(n)) => u8::try_from(*n).ok(),
        _ => None,
    };
    s.hyphenate = flag(keys::HYPHENATE);
    s.code = flag(keys::CODE);
    s.numbered = flag(keys::NUMBERED);
    s
}
