//! A document's styles resolved: each value a paragraph, a run of text, a
//! list or the page takes, and where it comes from (docs/ARCHITECTURE.md,
//! section 6).
//!
//! A value is the text's own, where it sets it; or else its style's,
//! walking up the chain of styles it's based on to the first that sets
//! it; or else the stylesheet's defaults; or else, for a document whose
//! sheet lacks it, the program's own sheet's. A style may set a value by
//! naming a part (a face, a colour, a layout of tab stops), whose value it
//! takes. A part, or a style, the document doesn't have is as if unset.
//!
//! Character styles, those of marks among them, set only what they say:
//! their chains end in nothing, so what they leave is the paragraph's.
//! [`StyleSheet`] resolves every style of a document once, for laying out
//! its paragraphs; [`find`] says where one value comes from, for the
//! inspector.

use std::collections::HashMap;

use doc::{Document, ObjectId, Paragraph, Props, StyleKind, Value};
use text::units::{INCH, MILLIMETRE, POINT};
use text::{Align, Ligatures, TabStop};

use crate::sheet::{SheetSpec, align_of, parse_colour, parse_ligatures};
use crate::{Indents, keys, parse_tabs};

/// A colour, sRGB.
pub type Rgb = (u8, u8, u8);

/// Where a value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// The text itself: its run, or its paragraph.
    Direct,
    /// A style, which sets it itself: the paragraph's or the run's own,
    /// or one up its chain.
    Style(ObjectId),
    /// The stylesheet's defaults.
    Defaults,
    /// The program's own stylesheet, where the document's doesn't say.
    Program,
}

/// Where a value comes from, and the part it names, if it names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    /// Where it's set.
    pub level: Level,
    /// The part whose value it takes: a face, a colour or a layout of tab
    /// stops.
    pub part: Option<ObjectId>,
}

/// The kind of part a key's value may name, and the key the part holds
/// the value under.
fn part_for(key: &str) -> Option<(StyleKind, &'static str)> {
    match key {
        keys::FONT => Some((StyleKind::Face, keys::FAMILY)),
        keys::COLOR
        | keys::QUOTE_COLOR
        | keys::CODE_TINT
        | keys::RULE_COLOR
        | keys::HAIRLINE_COLOR
        | keys::MISSING_PICTURE => Some((StyleKind::Colour, keys::COLOR)),
        keys::TABS | keys::DOCUMENT_TABS => Some((StyleKind::Tabs, keys::TABS)),
        _ => None,
    }
}

/// `value`, of `key`, with a part it names taken for its own value: the
/// value, and the part. `None` where it names what isn't a part of the
/// key's kind, as if unset.
fn deref(document: &Document, key: &str, value: &Value) -> Option<(Value, Option<ObjectId>)> {
    match value {
        Value::Ref(id) => {
            let (kind, inner) = part_for(key)?;
            let part = document.style(*id).filter(|s| s.kind == kind)?;
            let value = part.props.get(inner)?;
            (!matches!(value, Value::Ref(_))).then(|| (value.clone(), Some(*id)))
        }
        other => Some((other.clone(), None)),
    }
}

/// The value of `key` for `style` and the styles it's based on, or with
/// `defaults` then the stylesheet's defaults, where `valid` takes it, and
/// where it comes from. Parts are taken for their values.
pub fn find(
    document: &Document,
    style: Option<ObjectId>,
    key: &str,
    defaults: bool,
    valid: &dyn Fn(&Value) -> bool,
) -> Option<(Value, Source)> {
    for id in style.map(|s| document.chain(s)).unwrap_or_default() {
        let Some(found) = document.style(id).and_then(|s| s.props.get(key)) else {
            continue;
        };
        if let Some((value, part)) = deref(document, key, found)
            && valid(&value)
        {
            return Some((
                value,
                Source {
                    level: Level::Style(id),
                    part,
                },
            ));
        }
    }
    if !defaults {
        return None;
    }
    let found = document.stylesheet()?.props.get(key)?;
    let (value, part) = deref(document, key, found)?;
    valid(&value).then_some((
        value,
        Source {
            level: Level::Defaults,
            part,
        },
    ))
}

/// The value of `key` a paragraph or run with `props` sets itself, where
/// `valid` takes it, parts taken for their values.
pub fn own(
    document: &Document,
    props: &Props,
    key: &str,
    valid: &dyn Fn(&Value) -> bool,
) -> Option<(Value, Source)> {
    let (value, part) = deref(document, key, props.get(key)?)?;
    valid(&value).then_some((
        value,
        Source {
            level: Level::Direct,
            part,
        },
    ))
}

/// What a value of each key may be: a check that it's of the right kind
/// and within its limits.
pub fn valid(key: &str) -> fn(&Value) -> bool {
    fn text(v: &Value) -> bool {
        matches!(v, Value::Text(t) if !t.is_empty() && t.chars().count() <= 100)
    }
    fn flag(v: &Value) -> bool {
        matches!(v, Value::Bool(_))
    }
    fn size(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (POINT..=1000 * POINT).contains(n))
    }
    fn space(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (0..=10 * INCH).contains(n))
    }
    fn indent(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (-10 * INCH..=10 * INCH).contains(n))
    }
    fn line_height(v: &Value) -> bool {
        matches!(v, Value::Int(n) if *n == 0 || (250..=10_000).contains(n))
    }
    fn colour(v: &Value) -> bool {
        matches!(v, Value::Text(t) if parse_colour(t).is_some())
    }
    fn align(v: &Value) -> bool {
        matches!(v, Value::Text(t) if align_of(t).is_some())
    }
    fn tabs(v: &Value) -> bool {
        matches!(v, Value::Text(t) if parse_tabs(t).is_some())
    }
    fn heading(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (0..=6).contains(n))
    }
    fn ligatures(v: &Value) -> bool {
        matches!(v, Value::Text(_))
    }
    fn page(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (10 * MILLIMETRE..=2000 * MILLIMETRE).contains(n))
    }
    fn margin(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (0..=1000 * MILLIMETRE).contains(n))
    }
    fn small(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (0..=INCH).contains(n))
    }
    fn interval(v: &Value) -> bool {
        matches!(v, Value::Int(n) if (MILLIMETRE..=100 * INCH).contains(n))
    }
    fn any(_: &Value) -> bool {
        true
    }
    match key {
        keys::FONT => text,
        keys::SIZE => size,
        keys::BOLD
        | keys::ITALIC
        | keys::UNDERLINE
        | keys::STRIKETHROUGH
        | keys::KEEP_WITH_NEXT
        | keys::HYPHENATE
        | keys::CODE
        | keys::NUMBERED => flag,
        keys::COLOR
        | keys::QUOTE_COLOR
        | keys::CODE_TINT
        | keys::RULE_COLOR
        | keys::HAIRLINE_COLOR
        | keys::MISSING_PICTURE => colour,
        keys::LIGATURES => ligatures,
        keys::ALIGN => align,
        keys::SPACE_BEFORE | keys::SPACE_AFTER => space,
        keys::INDENT_START | keys::INDENT_END | keys::INDENT_FIRST => indent,
        keys::LINE_HEIGHT => line_height,
        keys::TABS | keys::DOCUMENT_TABS => tabs,
        keys::HEADING => heading,
        keys::PAGE_WIDTH | keys::PAGE_HEIGHT => page,
        keys::MARGIN_TOP | keys::MARGIN_BOTTOM | keys::MARGIN_LEFT | keys::MARGIN_RIGHT => margin,
        keys::QUOTE_INDENT
        | keys::QUOTE_BAR
        | keys::QUOTE_SPACE
        | keys::CODE_PADDING
        | keys::TABLE_SPACE
        | keys::CELL_PADDING
        | keys::RULE
        | keys::HAIRLINE
        | keys::RULE_SPACE
        | keys::LIST_INDENT
        | keys::MARKER_GAP => small,
        keys::TAB_INTERVAL => interval,
        _ => any,
    }
}

/// The keys of a paragraph's direct formatting: properties of its own
/// over its style's, which applying a paragraph style replaces and Clear
/// Formatting takes away.
pub const PARAGRAPH_FORMATTING: [&str; 8] = [
    keys::ALIGN,
    keys::SPACE_BEFORE,
    keys::SPACE_AFTER,
    keys::LINE_HEIGHT,
    keys::INDENT_START,
    keys::INDENT_END,
    keys::INDENT_FIRST,
    keys::TABS,
];

/// The keys of text's direct formatting: properties of its own over its
/// styles'. Marks aren't among them; they're character styles.
pub const CHARACTER_FORMATTING: [&str; 4] = [keys::FONT, keys::SIZE, keys::COLOR, keys::UNDERLINE];

/// The keys Clear Formatting takes from text, leaving it plain in its
/// paragraph's style (review sjj5ak5a): its own formatting
/// ([`CHARACTER_FORMATTING`]), its character style, and its marks
/// ([`MARKS`]), a link's title with its link. Not its language, which
/// isn't formatting, its pictures, or Markdown kept as it was written,
/// which is text.
pub const CLEARED_FROM_TEXT: [&str; 11] = [
    keys::FONT,
    keys::SIZE,
    keys::COLOR,
    keys::UNDERLINE,
    keys::CHAR_STYLE,
    keys::CODE,
    keys::ITALIC,
    keys::BOLD,
    keys::STRIKETHROUGH,
    keys::LINK,
    keys::LINK_TITLE,
];

/// The marks: each a run's key, and the role of the character style it
/// applies, in the order they apply.
pub const MARKS: [(&str, &str); 5] = [
    (keys::CODE, "code"),
    (keys::ITALIC, "emphasis"),
    (keys::BOLD, "strong"),
    (keys::STRIKETHROUGH, "strikethrough"),
    (keys::LINK, "link"),
];

/// Whether a run with `props` carries mark `key`: set on, or for a link,
/// a target; Markdown kept as it was written is shown as code is.
pub fn has_mark(props: &Props, key: &str) -> bool {
    match key {
        keys::LINK => matches!(props.get(keys::LINK), Some(Value::Text(_))),
        keys::CODE => {
            matches!(props.get(keys::CODE), Some(Value::Bool(true)))
                || matches!(props.get(keys::SOURCE), Some(Value::Text(_)))
        }
        _ => matches!(props.get(key), Some(Value::Bool(true))),
    }
}

/// What a character style sets: each value, or nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharValues {
    /// The font family.
    pub family: Option<String>,
    /// The size.
    pub size: Option<i64>,
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// The colour.
    pub color: Option<Rgb>,
    /// Underlined.
    pub underline: Option<bool>,
    /// Struck through.
    pub strikethrough: Option<bool>,
    /// Its ligatures.
    pub ligatures: Option<Ligatures>,
}

impl CharValues {
    /// What `get` finds for each character value.
    fn of(get: impl Fn(&str) -> Option<Value>) -> CharValues {
        let flag = |key: &str| match get(key) {
            Some(Value::Bool(b)) => Some(b),
            _ => None,
        };
        CharValues {
            family: match get(keys::FONT) {
                Some(Value::Text(t)) => Some(t),
                _ => None,
            },
            size: match get(keys::SIZE) {
                Some(Value::Int(n)) => Some(n),
                _ => None,
            },
            bold: flag(keys::BOLD),
            italic: flag(keys::ITALIC),
            color: match get(keys::COLOR) {
                Some(Value::Text(t)) => parse_colour(&t),
                _ => None,
            },
            underline: flag(keys::UNDERLINE),
            strikethrough: flag(keys::STRIKETHROUGH),
            ligatures: match get(keys::LIGATURES) {
                Some(Value::Text(t)) => Some(parse_ligatures(&t)),
                _ => None,
            },
        }
    }

    /// What style `id` and the styles it's based on set, not the defaults.
    pub fn of_style(document: &Document, id: ObjectId) -> CharValues {
        CharValues::of(|key| find(document, Some(id), key, false, &valid(key)).map(|(v, _)| v))
    }

    /// What a run with `props` sets itself: its direct formatting.
    pub fn of_run(document: &Document, props: &Props) -> CharValues {
        CharValues::of(|key| {
            if CHARACTER_FORMATTING.contains(&key) {
                own(document, props, key, &valid(key)).map(|(v, _)| v)
            } else {
                None
            }
        })
    }
}

/// A paragraph style resolved: every value it gives, from it, the styles
/// it's based on, and the defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParagraphStyle {
    /// Its ID; none for the defaults themselves.
    pub id: Option<ObjectId>,
    /// Its name.
    pub name: String,
    /// What it's for, such as "body" or "heading-1".
    pub role: Option<String>,
    /// The style it's based on.
    pub base: Option<ObjectId>,
    /// Its text's font family.
    pub family: String,
    /// Its text's size, in layout units.
    pub size: i64,
    /// Whether its text is bold.
    pub bold: bool,
    /// Whether its text is italic.
    pub italic: bool,
    /// Its text's colour.
    pub color: Rgb,
    /// Whether its text is underlined.
    pub underline: bool,
    /// Whether its text is struck through.
    pub strikethrough: bool,
    /// Which classes of ligature its text is set with.
    pub ligatures: Ligatures,
    /// How its lines are aligned.
    pub align: Align,
    /// Space above it, in layout units.
    pub space_before: i64,
    /// Space below it, in layout units.
    pub space_after: i64,
    /// How tall its lines are, in thousandths of their largest font, if it
    /// says; otherwise as tall as their fonts.
    pub line_height: Option<i64>,
    /// Its indents.
    pub indents: Indents,
    /// Its tab stops, besides the document's.
    pub tabs: Vec<TabStop>,
    /// Whether it stays on the page where the next paragraph starts.
    pub keep_with_next: bool,
    /// Its level as a heading, 1 to 6, if it's a heading's style.
    pub heading: Option<u8>,
    /// Whether its words are hyphenated; if it doesn't say, they are where
    /// its lines are justified.
    pub hyphenate: Option<bool>,
    /// Whether it's a code block's style.
    pub code: bool,
    /// Whether it's numbered, as headings are in a technical document.
    pub numbered: bool,
}

/// A character style resolved: what it and the styles it's based on set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterStyle {
    /// Its ID.
    pub id: ObjectId,
    /// Its name.
    pub name: String,
    /// What it's for, such as "emphasis" or "link".
    pub role: Option<String>,
    /// The style it's based on.
    pub base: Option<ObjectId>,
    /// What it sets.
    pub values: CharValues,
}

/// How a list is marked and indented, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListStyle {
    /// Its ID; none for the program's own.
    pub id: Option<ObjectId>,
    /// Its name.
    pub name: String,
    /// What it's for: "bullets" or "numbers".
    pub role: Option<String>,
    /// The bullets, by depth.
    pub bullets: Vec<String>,
    /// How a number reads, `1` standing for it; empty for bullets.
    pub numbers: String,
    /// How far each level indents its items.
    pub indent: i64,
    /// The room between a marker and its item's text.
    pub gap: i64,
    /// What each of its levels sets of its own, from the outermost, as
    /// many as [`keys::LEVELS`]; what a level doesn't set it takes from
    /// the values above. Lists nested deeper go round them again.
    pub own: Vec<LevelOwn>,
}

/// What a level of a list definition sets of its own: each value, or
/// nothing where it takes the definition's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelOwn {
    /// How its items are numbered, or bulleted.
    pub numbering: Option<Numbering>,
    /// Its bullet.
    pub bullet: Option<String>,
    /// How its number reads, `1` standing for it.
    pub reads: Option<String>,
    /// The number its items start at.
    pub start: Option<i64>,
    /// How far it indents its items.
    pub indent: Option<i64>,
    /// The room between its markers and its items' text.
    pub gap: Option<i64>,
}

impl LevelOwn {
    /// Whether it sets nothing of its own.
    pub fn is_empty(&self) -> bool {
        *self == LevelOwn::default()
    }
}

/// How the items of a level of lists are numbered, or bulleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Numbering {
    /// A bullet, not a number.
    Bullet,
    /// 1, 2, 3.
    Decimal,
    /// a, b, c, … z, aa, ab.
    LowerAlpha,
    /// A, B, C.
    UpperAlpha,
    /// i, ii, iii.
    LowerRoman,
    /// I, II, III.
    UpperRoman,
}

impl Numbering {
    /// Every numbering, the bullet first, as the inspector offers them.
    pub const ALL: [Numbering; 6] = [
        Numbering::Bullet,
        Numbering::Decimal,
        Numbering::LowerAlpha,
        Numbering::UpperAlpha,
        Numbering::LowerRoman,
        Numbering::UpperRoman,
    ];

    /// The numbering a level's marker names: "bullet", "1", "a", "A", "i"
    /// or "I".
    pub fn of(code: &str) -> Option<Numbering> {
        Numbering::ALL.into_iter().find(|n| n.code() == code)
    }

    /// Its name as a level's marker holds it.
    pub fn code(self) -> &'static str {
        match self {
            Numbering::Bullet => "bullet",
            Numbering::Decimal => "1",
            Numbering::LowerAlpha => "a",
            Numbering::UpperAlpha => "A",
            Numbering::LowerRoman => "i",
            Numbering::UpperRoman => "I",
        }
    }

    /// `n` written in it: letters go on past z as "aa", "ab"; Roman
    /// numerals stop at 3999. A number it can't write, nought or less, or
    /// past those, is written in figures, as for a bullet.
    pub fn number(self, n: i64) -> String {
        let letters = |upper: bool| {
            let mut out = Vec::new();
            let mut n = n;
            while n > 0 {
                let d = ((n - 1) % 26) as u8;
                out.push(if upper { b'A' + d } else { b'a' + d } as char);
                n = (n - 1) / 26;
            }
            out.iter().rev().collect::<String>()
        };
        let roman = |upper: bool| {
            const NUMERALS: [(i64, &str); 13] = [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ];
            let mut out = String::new();
            let mut n = n;
            for (value, numeral) in NUMERALS {
                while n >= value {
                    out.push_str(numeral);
                    n -= value;
                }
            }
            if upper { out.to_uppercase() } else { out }
        };
        match self {
            Numbering::LowerAlpha | Numbering::UpperAlpha if n > 0 => {
                letters(self == Numbering::UpperAlpha)
            }
            Numbering::LowerRoman | Numbering::UpperRoman if (1..4000).contains(&n) => {
                roman(self == Numbering::UpperRoman)
            }
            _ => n.to_string(),
        }
    }
}

/// A level of a list definition, resolved: how its items are marked, and
/// where they sit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevel {
    /// How its items are numbered, or bulleted, if the level says; if not,
    /// numbered lists are numbered and others bulleted.
    pub numbering: Option<Numbering>,
    /// Its bullet.
    pub bullet: String,
    /// How its number reads, `1` standing for it; empty for "1.".
    pub reads: String,
    /// The number its items start at, unless a list says its own.
    pub start: i64,
    /// How far it indents its items from the level outside it.
    pub indent: i64,
    /// The room between its markers and its items' text.
    pub gap: i64,
}

impl ListLevel {
    /// How it numbers a list's items: as it says, or, where it doesn't,
    /// in figures if the list is numbered and with bullets if not.
    pub fn numbering(&self, ordered: bool) -> Numbering {
        self.numbering.unwrap_or(if ordered {
            Numbering::Decimal
        } else {
            Numbering::Bullet
        })
    }

    /// `number`, already written in its numbering, as the level reads it:
    /// "1.2" as "1.2." where it reads "1.".
    pub fn reads(&self, number: &str) -> String {
        if self.reads.is_empty() {
            format!("{number}.")
        } else {
            self.reads.replacen('1', number, 1)
        }
    }

    /// The marker of item number `n` of a list at this level, numbered or
    /// not: its number as the level writes and reads it, or its bullet.
    pub fn marker(&self, ordered: bool, n: i64) -> String {
        match self.numbering(ordered) {
            Numbering::Bullet => self.bullet.clone(),
            numbering => self.reads(&numbering.number(n)),
        }
    }
}

impl ListStyle {
    /// The level lists nested `depth` lists deep take, from 0: what that
    /// level sets of its own, round again past the last level, over the
    /// definition's own values, its bullets by depth, round again past
    /// the last.
    pub fn level(&self, depth: usize) -> ListLevel {
        let own = self
            .own
            .get(depth % keys::LEVELS)
            .cloned()
            .unwrap_or_default();
        ListLevel {
            numbering: own.numbering,
            bullet: own
                .bullet
                .or_else(|| {
                    (!self.bullets.is_empty())
                        .then(|| self.bullets[depth % self.bullets.len()].clone())
                })
                .unwrap_or_default(),
            reads: own.reads.unwrap_or_else(|| self.numbers.clone()),
            start: own.start.unwrap_or(1),
            indent: own.indent.unwrap_or(self.indent),
            gap: own.gap.unwrap_or(self.gap),
        }
    }

    /// How many levels it sets apart: as many as its bullets, or to the
    /// deepest that sets something of its own, and at least one.
    pub fn levels(&self) -> usize {
        let set = self
            .own
            .iter()
            .rposition(|o| !o.is_empty())
            .map_or(0, |k| k + 1);
        self.bullets.len().max(set).clamp(1, keys::LEVELS)
    }

    /// The marker of item number `n` of a list in this style: its number
    /// as the style writes numbers, or "1." if it writes none.
    pub fn number(&self, n: i64) -> String {
        self.format(&n.to_string())
    }

    /// `number` as the style writes numbers: "1.2" as "1.2." where it
    /// writes "1.".
    pub fn format(&self, number: &str) -> String {
        if self.numbers.is_empty() {
            format!("{number}.")
        } else {
            self.numbers.replacen('1', number, 1)
        }
    }
}

/// A named part, for showing and choosing: a face, a colour or a layout
/// of tab stops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Its ID.
    pub id: ObjectId,
    /// What it is.
    pub kind: StyleKind,
    /// Its name.
    pub name: String,
    /// What it's for.
    pub role: Option<String>,
    /// Its value as text: the family, `#rrggbb`, or the stops.
    pub value: String,
}

/// A page's size and margins, in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageStyle {
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
    /// The margin at the top.
    pub top: i64,
    /// The margin at the bottom.
    pub bottom: i64,
    /// The margin at the left.
    pub left: i64,
    /// The margin at the right.
    pub right: i64,
}

impl PageStyle {
    /// The width of the text between the margins.
    pub fn body_width(&self) -> i64 {
        self.width - self.left - self.right
    }

    /// The height of the text between the margins.
    pub fn body_height(&self) -> i64 {
        self.height - self.top - self.bottom
    }
}

/// How blocks other than paragraphs look, resolved: the stylesheet's, in
/// layout units and sRGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocks {
    /// How far a quote indents its blocks, its bar included.
    pub quote_indent: i64,
    /// How wide a quote's bar is.
    pub quote_bar: i64,
    /// The colour of a quote's bar.
    pub quote_color: Rgb,
    /// The space above and below a quote, besides its first and last
    /// blocks' own.
    pub quote_space: i64,
    /// The tint behind code.
    pub code_tint: Rgb,
    /// How far a code block's tint reaches around its text.
    pub code_padding: i64,
    /// The space above and below a table.
    pub table_space: i64,
    /// The room around a cell's text.
    pub cell_padding: i64,
    /// The width of a table's rules above and below it, and under its
    /// header.
    pub rule: i64,
    /// The width of the rules between its rows.
    pub hairline: i64,
    /// The colour of a table's rules.
    pub rule_color: Rgb,
    /// The colour of the rules between its rows.
    pub hairline_color: Rgb,
    /// The room above and below a rule across the text, Markdown's
    /// thematic break, which is drawn a hairline wide in the rules'
    /// colour.
    pub rule_space: i64,
    /// The colour drawn where a picture can't be.
    pub missing_picture: Rgb,
}

/// The tab stops every paragraph of a document has: stops set, and the
/// interval of the default stops after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTabs {
    /// The stops set.
    pub stops: Vec<TabStop>,
    /// The distance between default stops.
    pub interval: i64,
}

/// A document's stylesheet resolved, for laying out its paragraphs: each
/// style's values, the defaults, the page, its lists and how its blocks
/// look. A document without a sheet, or whose sheet lacks a value, takes
/// the program's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSheet {
    /// The paragraph styles, in the sheet's order.
    pub paragraph: Vec<ParagraphStyle>,
    /// The character styles, in the sheet's order.
    pub character: Vec<CharacterStyle>,
    /// The list definitions, in the sheet's order.
    pub lists: Vec<ListStyle>,
    /// The faces, colours and tab layouts, in the sheet's order.
    pub parts: Vec<Part>,
    /// The defaults, as a style of their own.
    pub defaults: ParagraphStyle,
    /// The page style.
    pub page: PageStyle,
    /// The document's tab stops.
    pub tabs: DocumentTabs,
    /// How lists, quotes, code and tables look.
    pub blocks: Blocks,
    /// The sheet it was copied from, and that sheet's version, if it says.
    pub origin: Option<(String, u32)>,
    by_id: HashMap<ObjectId, usize>,
    character_by_id: HashMap<ObjectId, usize>,
    /// The paragraph style paragraphs that name none take, if the sheet
    /// has paragraph styles: the one with the role "body", or the first.
    default: Option<usize>,
    /// What each mark sets, in [`MARKS`]' order: the document's style of
    /// its role, or the program's.
    marks: Vec<CharValues>,
    /// The lists that aren't numbered, and those that are, which name no
    /// definition of their own.
    bullets: ListStyle,
    numbers: ListStyle,
}

fn name_and_role(props: &Props) -> (String, Option<String>) {
    let text = |key: &str| match props.get(key) {
        Some(Value::Text(t)) => Some(t.clone()),
        _ => None,
    };
    (text(keys::NAME).unwrap_or_default(), text(keys::ROLE))
}

impl StyleSheet {
    /// The stylesheet a file gives, resolved on its own: the program's,
    /// which other documents fall back on.
    pub fn from_spec(spec: &SheetSpec) -> StyleSheet {
        let document = crate::new_document(spec);
        StyleSheet::resolve(&document, None)
    }

    /// `document`'s stylesheet resolved, with what it lacks from
    /// `fallback`, the program's.
    pub fn of(document: &Document, fallback: &StyleSheet) -> StyleSheet {
        StyleSheet::resolve(document, Some(fallback))
    }

    fn resolve(document: &Document, fallback: Option<&StyleSheet>) -> StyleSheet {
        let defaults = paragraph_style(document, None, fallback.map(|f| &f.defaults));
        let mut paragraph = Vec::new();
        let mut character = Vec::new();
        let mut lists = Vec::new();
        let mut parts = Vec::new();
        let mut page = None;
        for &id in document.styles() {
            let Some(style) = document.style(id) else {
                continue;
            };
            let (name, role) = name_and_role(&style.props);
            match style.kind {
                StyleKind::Paragraph => {
                    paragraph.push(paragraph_style(document, Some(id), Some(&defaults)));
                }
                StyleKind::Character => character.push(CharacterStyle {
                    id,
                    name,
                    role,
                    base: style.base,
                    values: CharValues::of_style(document, id),
                }),
                StyleKind::List => lists.push(list_style(id, name, role, &style.props)),
                StyleKind::Page => {
                    if page.is_none() {
                        page = page_style(document, id);
                    }
                }
                StyleKind::Face | StyleKind::Colour | StyleKind::Tabs => {
                    let inner = match style.kind {
                        StyleKind::Face => keys::FAMILY,
                        StyleKind::Colour => keys::COLOR,
                        _ => keys::TABS,
                    };
                    let value = match style.props.get(inner) {
                        Some(Value::Text(t)) => t.clone(),
                        _ => String::new(),
                    };
                    parts.push(Part {
                        id,
                        kind: style.kind,
                        name,
                        role,
                        value,
                    });
                }
            }
        }
        let by_id = paragraph
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.id.map(|id| (id, i)))
            .collect();
        let character_by_id = character
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id, i))
            .collect();
        let default = paragraph
            .iter()
            .position(|s| s.role.as_deref() == Some("body"))
            .or((!paragraph.is_empty()).then_some(0));
        let marks = MARKS
            .iter()
            .enumerate()
            .map(|(k, (_, role))| {
                character
                    .iter()
                    .find(|s| s.role.as_deref() == Some(role))
                    .map(|s| s.values.clone())
                    .or_else(|| fallback.map(|f| f.marks[k].clone()))
                    .unwrap_or_default()
            })
            .collect();
        let list_of = |role: &str, ordered: bool| -> ListStyle {
            lists
                .iter()
                .find(|l| l.role.as_deref() == Some(role))
                .or_else(|| lists.iter().find(|l| !l.numbers.is_empty() == ordered))
                .cloned()
                .or_else(|| {
                    fallback.map(|f| {
                        if ordered {
                            f.numbers.clone()
                        } else {
                            f.bullets.clone()
                        }
                    })
                })
                .unwrap_or_else(|| ListStyle {
                    id: None,
                    name: String::new(),
                    role: None,
                    bullets: Vec::new(),
                    numbers: String::new(),
                    indent: 0,
                    gap: 0,
                    own: levels_own(&Props::new()),
                })
        };
        let bullets = list_of("bullets", false);
        let numbers = list_of("numbers", true);
        let page = page.or(fallback.map(|f| f.page)).unwrap_or(PageStyle {
            width: 210 * MILLIMETRE,
            height: 297 * MILLIMETRE,
            top: 0,
            bottom: 0,
            left: 0,
            right: 0,
        });
        let sheet_props = document.stylesheet().map(|s| &s.props);
        let get = |key: &str| find(document, None, key, true, &valid(key)).map(|(v, _)| v);
        let int = |key: &str, or: i64| match get(key) {
            Some(Value::Int(n)) => n,
            _ => or,
        };
        let colour = |key: &str, or: Rgb| match get(key) {
            Some(Value::Text(t)) => parse_colour(&t).unwrap_or(or),
            _ => or,
        };
        let fb = fallback.map(|f| &f.blocks);
        let blocks = Blocks {
            quote_indent: int(keys::QUOTE_INDENT, fb.map_or(0, |b| b.quote_indent)),
            quote_bar: int(keys::QUOTE_BAR, fb.map_or(0, |b| b.quote_bar)),
            quote_color: colour(keys::QUOTE_COLOR, fb.map_or((0, 0, 0), |b| b.quote_color)),
            // None where the sheet doesn't say, not the program's: sheets
            // from before it set quotes off with none.
            quote_space: int(keys::QUOTE_SPACE, 0),
            code_tint: colour(keys::CODE_TINT, fb.map_or((255, 255, 255), |b| b.code_tint)),
            code_padding: int(keys::CODE_PADDING, fb.map_or(0, |b| b.code_padding)),
            table_space: int(keys::TABLE_SPACE, fb.map_or(0, |b| b.table_space)),
            cell_padding: int(keys::CELL_PADDING, fb.map_or(0, |b| b.cell_padding)),
            rule: int(keys::RULE, fb.map_or(0, |b| b.rule)),
            hairline: int(keys::HAIRLINE, fb.map_or(0, |b| b.hairline)),
            rule_color: colour(keys::RULE_COLOR, fb.map_or((0, 0, 0), |b| b.rule_color)),
            hairline_color: colour(
                keys::HAIRLINE_COLOR,
                fb.map_or((0, 0, 0), |b| b.hairline_color),
            ),
            rule_space: int(keys::RULE_SPACE, fb.map_or(0, |b| b.rule_space)),
            missing_picture: colour(
                keys::MISSING_PICTURE,
                fb.map_or((255, 255, 255), |b| b.missing_picture),
            ),
        };
        let tabs = DocumentTabs {
            stops: match get(keys::DOCUMENT_TABS) {
                Some(Value::Text(t)) => parse_tabs(&t).unwrap_or_default(),
                _ => Vec::new(),
            },
            interval: int(
                keys::TAB_INTERVAL,
                fallback.map_or(12 * MILLIMETRE, |f| f.tabs.interval),
            ),
        };
        let origin =
            sheet_props.and_then(
                |p| match (p.get(keys::ORIGIN), p.get(keys::ORIGIN_VERSION)) {
                    (Some(Value::Text(name)), Some(Value::Int(v))) => {
                        Some((name.clone(), u32::try_from(*v).unwrap_or(0)))
                    }
                    _ => None,
                },
            );
        StyleSheet {
            paragraph,
            character,
            lists,
            parts,
            defaults,
            page,
            tabs,
            blocks,
            origin,
            by_id,
            character_by_id,
            default,
            marks,
            bullets,
            numbers,
        }
    }

    /// The paragraph style paragraphs that name none take: the one with
    /// the role "body", or the first, or the defaults.
    pub fn default_style(&self) -> &ParagraphStyle {
        self.default
            .and_then(|i| self.paragraph.get(i))
            .unwrap_or(&self.defaults)
    }

    /// The paragraph style `id`, or the default if there's no such style.
    pub fn paragraph_style(&self, id: Option<ObjectId>) -> &ParagraphStyle {
        id.and_then(|id| self.by_id.get(&id))
            .and_then(|&i| self.paragraph.get(i))
            .unwrap_or_else(|| self.default_style())
    }

    /// The style `paragraph` names, or the default.
    pub fn style_of(&self, paragraph: &Paragraph) -> &ParagraphStyle {
        let id = match paragraph.props.get(keys::STYLE) {
            Some(Value::Ref(id)) => Some(*id),
            _ => None,
        };
        self.paragraph_style(id)
    }

    /// The paragraph style with role `role`, if the sheet has one.
    pub fn by_role(&self, role: &str) -> Option<&ParagraphStyle> {
        self.paragraph
            .iter()
            .find(|s| s.role.as_deref() == Some(role))
    }

    /// The paragraph style of a heading of `level`: the one with the role
    /// "heading-N", or else the first with that level.
    pub fn heading(&self, level: u8) -> Option<&ParagraphStyle> {
        self.by_role(&format!("heading-{level}"))
            .or_else(|| self.paragraph.iter().find(|s| s.heading == Some(level)))
    }

    /// The paragraph style of a table's header row's paragraphs: the one
    /// with the role [`crate::TABLE_HEADER`], if the sheet has one.
    pub fn table_header(&self) -> Option<&ParagraphStyle> {
        self.by_role(crate::TABLE_HEADER)
    }

    /// The paragraph style of code blocks: the one with the role "code",
    /// or else the first that's a code block's.
    pub fn code(&self) -> Option<&ParagraphStyle> {
        self.by_role("code")
            .or_else(|| self.paragraph.iter().find(|s| s.code))
    }

    /// The character style `id`, if the sheet has it.
    pub fn character_style(&self, id: ObjectId) -> Option<&CharacterStyle> {
        self.character_by_id
            .get(&id)
            .and_then(|&i| self.character.get(i))
    }

    /// The face, colour or tab layout `id`, if the sheet has it.
    pub fn part(&self, id: ObjectId) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// What mark `k` of [`MARKS`] sets.
    pub fn mark(&self, k: usize) -> &CharValues {
        &self.marks[k]
    }

    /// How a list is marked and indented: as the definition it names, if
    /// the sheet has it, or as the sheet's lists numbered or not are.
    pub fn list(&self, ordered: bool, own: Option<ObjectId>) -> &ListStyle {
        own.and_then(|id| self.lists.iter().find(|l| l.id == Some(id)))
            .unwrap_or(if ordered {
                &self.numbers
            } else {
                &self.bullets
            })
    }

    /// The tab stops of a paragraph in `style`: the style's and the
    /// document's, by position, the style's where both have one at the
    /// same place, then the document's default ones.
    pub fn tabs(&self, style: &ParagraphStyle) -> text::Tabs {
        let mut stops: Vec<TabStop> = style.tabs.clone();
        for s in &self.tabs.stops {
            if !stops.iter().any(|t| t.position == s.position) {
                stops.push(*s);
            }
        }
        stops.sort_by_key(|s| s.position);
        text::Tabs {
            stops,
            interval: self.tabs.interval,
        }
    }

    /// The families its styles and parts set text in, each once: those a
    /// document in it may need loaded.
    pub fn families(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |family: &str| {
            if !family.is_empty() && !out.iter().any(|f| f == family) {
                out.push(family.to_string());
            }
        };
        add(&self.defaults.family);
        for s in &self.paragraph {
            add(&s.family);
        }
        for s in &self.character {
            if let Some(f) = &s.values.family {
                add(f);
            }
        }
        for m in &self.marks {
            if let Some(f) = &m.family {
                add(f);
            }
        }
        for p in self.parts.iter().filter(|p| p.kind == StyleKind::Face) {
            add(&p.value);
        }
        out
    }
}

/// Style `id` (or with none, the defaults alone) resolved, each value from
/// its chain and the defaults, or else from `fallback`'s.
fn paragraph_style(
    document: &Document,
    id: Option<ObjectId>,
    fallback: Option<&ParagraphStyle>,
) -> ParagraphStyle {
    let get = |key: &str| find(document, id, key, true, &valid(key)).map(|(v, _)| v);
    let flag = |key: &str, or: bool| match get(key) {
        Some(Value::Bool(b)) => b,
        _ => or,
    };
    let int = |key: &str, or: i64| match get(key) {
        Some(Value::Int(n)) => n,
        _ => or,
    };
    let fb = fallback;
    let (name, role, base) = match id.and_then(|id| document.style(id)) {
        Some(style) => {
            let (name, role) = name_and_role(&style.props);
            (name, role, style.base)
        }
        None => ("Defaults".to_string(), None, None),
    };
    ParagraphStyle {
        id,
        name,
        role,
        base,
        family: match get(keys::FONT) {
            Some(Value::Text(t)) => t,
            _ => fb.map_or_else(String::new, |f| f.family.clone()),
        },
        size: int(keys::SIZE, fb.map_or(12 * POINT, |f| f.size)),
        bold: flag(keys::BOLD, fb.is_some_and(|f| f.bold)),
        italic: flag(keys::ITALIC, fb.is_some_and(|f| f.italic)),
        color: match get(keys::COLOR) {
            Some(Value::Text(t)) => parse_colour(&t).unwrap_or_default(),
            _ => fb.map_or((0, 0, 0), |f| f.color),
        },
        underline: flag(keys::UNDERLINE, fb.is_some_and(|f| f.underline)),
        strikethrough: flag(keys::STRIKETHROUGH, fb.is_some_and(|f| f.strikethrough)),
        ligatures: match get(keys::LIGATURES) {
            Some(Value::Text(t)) => parse_ligatures(&t),
            _ => fb.map_or_else(Ligatures::default, |f| f.ligatures),
        },
        align: match get(keys::ALIGN) {
            Some(Value::Text(t)) => align_of(&t).unwrap_or_default(),
            _ => fb.map_or(Align::Start, |f| f.align),
        },
        space_before: int(keys::SPACE_BEFORE, fb.map_or(0, |f| f.space_before)),
        space_after: int(keys::SPACE_AFTER, fb.map_or(0, |f| f.space_after)),
        line_height: match get(keys::LINE_HEIGHT) {
            Some(Value::Int(0)) => None,
            Some(Value::Int(n)) => Some(n),
            _ => fb.and_then(|f| f.line_height),
        },
        indents: Indents {
            start: int(keys::INDENT_START, fb.map_or(0, |f| f.indents.start)),
            end: int(keys::INDENT_END, fb.map_or(0, |f| f.indents.end)),
            first: int(keys::INDENT_FIRST, fb.map_or(0, |f| f.indents.first)),
        },
        tabs: match get(keys::TABS) {
            Some(Value::Text(t)) => parse_tabs(&t).unwrap_or_default(),
            _ => fb.map_or_else(Vec::new, |f| f.tabs.clone()),
        },
        keep_with_next: flag(keys::KEEP_WITH_NEXT, fb.is_some_and(|f| f.keep_with_next)),
        heading: match get(keys::HEADING) {
            Some(Value::Int(n)) if n > 0 => u8::try_from(n).ok(),
            Some(_) => None,
            None => fb.and_then(|f| f.heading),
        },
        hyphenate: match get(keys::HYPHENATE) {
            Some(Value::Bool(b)) => Some(b),
            _ => fb.and_then(|f| f.hyphenate),
        },
        code: flag(keys::CODE, fb.is_some_and(|f| f.code)),
        numbered: flag(keys::NUMBERED, fb.is_some_and(|f| f.numbered)),
    }
}

fn list_style(id: ObjectId, name: String, role: Option<String>, props: &Props) -> ListStyle {
    let int = |key: &str| match props.get(key) {
        Some(Value::Int(n)) if valid(key)(&Value::Int(*n)) => *n,
        _ => 0,
    };
    let text = |key: &str| match props.get(key) {
        Some(Value::Text(t)) if t.chars().count() <= 64 => t.clone(),
        _ => String::new(),
    };
    let bullets: Vec<String> = text(keys::BULLETS)
        .split_whitespace()
        .take(8)
        .map(str::to_string)
        .collect();
    let numbers = text(keys::NUMBERS);
    ListStyle {
        id: Some(id),
        name,
        role,
        bullets,
        numbers,
        indent: int(keys::LIST_INDENT),
        gap: int(keys::MARKER_GAP),
        own: levels_own(props),
    }
}

/// Whether `reads` can be how a level's number reads: a few characters,
/// with a 1 for the number.
pub fn valid_reads(reads: &str) -> bool {
    reads.contains('1') && reads.chars().count() <= 16
}

/// Whether `bullet` can be a level's bullet: a few characters, no space.
pub fn valid_bullet(bullet: &str) -> bool {
    !bullet.is_empty() && bullet.chars().count() <= 8 && !bullet.chars().any(char::is_whitespace)
}

/// The most a level's items can start at.
pub const MOST_START: i64 = 9999;

/// What each level of a list definition with `props` sets of its own,
/// from the outermost, [`keys::LEVELS`] of them. A value of the wrong
/// kind or past its limits is as if unset.
pub(crate) fn levels_own(props: &Props) -> Vec<LevelOwn> {
    (1..=keys::LEVELS)
        .map(|k| {
            let get = |what: &str| props.get(&keys::level(k, what));
            let text = |what: &str| match get(what) {
                Some(Value::Text(t)) => Some(t.clone()),
                _ => None,
            };
            let length = |what: &str| match get(what) {
                Some(Value::Int(n)) if valid(keys::LIST_INDENT)(&Value::Int(*n)) => Some(*n),
                _ => None,
            };
            LevelOwn {
                numbering: text(keys::LEVEL_MARKER).and_then(|m| Numbering::of(&m)),
                bullet: text(keys::LEVEL_BULLET).filter(|b| valid_bullet(b)),
                reads: text(keys::LEVEL_READS).filter(|r| valid_reads(r)),
                start: match get(keys::LEVEL_START) {
                    Some(Value::Int(n)) if (0..=MOST_START).contains(n) => Some(*n),
                    _ => None,
                },
                indent: length(keys::LEVEL_INDENT),
                gap: length(keys::LEVEL_GAP),
            }
        })
        .collect()
}

/// Page style `id`'s size and margins, if it gives them all, and they
/// leave room on the page.
fn page_style(document: &Document, id: ObjectId) -> Option<PageStyle> {
    let get = |key: &str| match find(document, Some(id), key, false, &valid(key)) {
        Some((Value::Int(n), _)) => Some(n),
        _ => None,
    };
    let page = PageStyle {
        width: get(keys::PAGE_WIDTH)?,
        height: get(keys::PAGE_HEIGHT)?,
        top: get(keys::MARGIN_TOP)?,
        bottom: get(keys::MARGIN_BOTTOM)?,
        left: get(keys::MARGIN_LEFT)?,
        right: get(keys::MARGIN_RIGHT)?,
    };
    (page.body_width() >= MILLIMETRE && page.body_height() >= MILLIMETRE).then_some(page)
}
