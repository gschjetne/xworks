//! The inspector's Style tab: a screen for every kind of style and part a
//! document's stylesheet holds (docs/ARCHITECTURE.md, section 6; project
//! owner, 2026-10-26, review sjj5ak5a): paragraph and character styles,
//! faces, colours, tab layouts and list definitions.
//!
//! - **Choosing what to edit.** At its head, a kind (Paragraph styles,
//!   Character styles, Faces, Colours, Tab layouts, Lists) and one of it;
//!   New and Delete. The tab opens on the caret's paragraph style and
//!   follows the caret until the person chooses another; opening the tab
//!   again goes back to the caret's.
//! - **Following a part.** A value that names a part (a face, a colour, a
//!   tab layout) has an arrow that shows the part's screen; "Back to" and
//!   the style's name goes back.
//! - **Each value** of a style shows where it comes from, this style, a
//!   style up its chain, a part, or the defaults, with a button to let a
//!   value the style sets itself go. A character style sets only what it
//!   says, so "not set" is a value of its own.
//! - **Every edit is an operation** (InsertStyle, DeleteStyle,
//!   SetStyleProp, SetBase), named for Undo; a delete the model refuses,
//!   of a style another is based on, is refused, and the screen says why.

use chrome::{
    Arg, Choice, Control, ControlKind, InspectorTab, Length, Row, Section, StopKind, Unit,
};
use doc::{Document, ObjectId, Op, Props, StyleKind, Value};
use story::sheet::{format_colour, format_ligatures, parse_colour};
use story::styles::{Level, MOST_START, find, valid, valid_bullet, valid_reads};
use story::{Numbering, keys};
use text::units::INCH;
use text::{Ligatures, TabStop};

use super::Editor;
use super::format::{
    MOST, add_stop, line_height_label, move_stop, set_stop_kind, stop_kind, stop_label,
};

/// The kinds the Style tab shows, in the order its list offers them: each
/// what it's called as a kind and as one of it, and its name in commands.
const KINDS: [(StyleKind, &str, &str, &str); 6] = [
    (
        StyleKind::Paragraph,
        "Paragraph styles",
        "Style",
        "paragraph",
    ),
    (
        StyleKind::Character,
        "Character styles",
        "Style",
        "character",
    ),
    (StyleKind::Face, "Faces", "Face", "face"),
    (StyleKind::Colour, "Colours", "Colour", "colour"),
    (StyleKind::Tabs, "Tab layouts", "Layout", "tabs"),
    (StyleKind::List, "Lists", "List", "list"),
];

/// What a new style or part of each kind is called, before it's renamed.
fn new_name(kind: StyleKind) -> &'static str {
    match kind {
        StyleKind::Paragraph => "New Style",
        StyleKind::Character => "New Character Style",
        StyleKind::Face => "New Face",
        StyleKind::Colour => "New Colour",
        StyleKind::Tabs => "New Tab Layout",
        StyleKind::List | StyleKind::Page => "New List",
    }
}

/// The kind a command names: "paragraph", "colour" and so on.
fn kind_of(code: &str) -> Option<StyleKind> {
    KINDS.iter().find(|k| k.3 == code).map(|k| k.0)
}

/// Line heights the Style tab offers, in thousandths of the font; 0 is as
/// tall as the fonts make them.
const LINE_HEIGHTS: &[i64] = &[0, 1000, 1150, 1200, 1250, 1350, 1500, 2000];

/// The leaders a tab layout's stop can have: none, or a character, with
/// what each is called.
const LEADERS: [(&str, &str); 4] = [
    ("", "No leader"),
    (".", "Dots"),
    ("-", "Dashes"),
    ("_", "Line"),
];

/// What the Style tab shows: the caret's paragraph style, until the
/// person chooses a style or a part, or a kind of them; and the screens
/// followed to it from values that name parts, to go back to.
#[derive(Clone, Debug, Default)]
pub(super) struct Showing {
    /// The style or part chosen, if one is.
    chosen: Option<ObjectId>,
    /// The kind chosen: the chosen style's or part's, or one chosen with
    /// none of it to show, as when the sheet has no tab layouts, or its
    /// chosen one was deleted.
    kind: Option<StyleKind>,
    /// The styles and parts followed from, latest last.
    trail: Vec<ObjectId>,
}

fn control(command: &str, args: Vec<Arg>, enabled: bool, kind: ControlKind) -> Control {
    Control {
        command: command.to_string(),
        args,
        enabled,
        kind,
    }
}

fn note(text: &str) -> Control {
    control(
        "",
        Vec::new(),
        true,
        ControlKind::Note {
            text: text.to_string(),
        },
    )
}

/// A button that says `label`, or shows `icon` if it has one.
fn button(
    command: &str,
    args: Vec<Arg>,
    enabled: bool,
    label: &str,
    icon: Option<&str>,
) -> Control {
    control(
        command,
        args,
        enabled,
        ControlKind::Button {
            label: label.to_string(),
            icon: icon.map(str::to_string),
        },
    )
}

fn row(label: &str, controls: Vec<Control>) -> Row {
    Row {
        label: label.to_string(),
        controls,
    }
}

fn choice(choices: Vec<Choice>, selected: Option<usize>) -> ControlKind {
    ControlKind::Choice {
        choices,
        selected,
        plain: false,
    }
}

fn id_arg(id: ObjectId) -> Arg {
    Arg::Int(id.0 as i64)
}

fn length(value: i64, unit: Unit) -> ControlKind {
    ControlKind::Length {
        value: Some(Length { value, unit }),
        prefix: String::new(),
        plain: false,
    }
}

/// A style's or a part's name.
fn name_of(document: &Document, id: ObjectId) -> String {
    match document.style(id).and_then(|s| s.props.get(keys::NAME)) {
        Some(Value::Text(name)) => name.clone(),
        _ => String::new(),
    }
}

/// `(r, g, b)` as a swatch's colour.
fn rgb((r, g, b): (u8, u8, u8)) -> [u8; 3] {
    [r, g, b]
}

impl Editor {
    /// The Style tab shows the caret's paragraph style again, as it does
    /// when it's opened.
    pub fn show_caret_style(&mut self) {
        self.showing = Showing::default();
    }

    /// The style or part the Style tab shows, if one was chosen rather
    /// than the caret's paragraph style.
    pub fn style_chosen(&self) -> Option<ObjectId> {
        self.showing.chosen
    }

    /// The Style tab shows style or part `id`, as if chosen from its list.
    pub fn choose_style(&mut self, id: ObjectId) {
        self.showing = Showing::default();
        self.show(id);
    }

    /// The kind the Style tab shows, if one was chosen with none of it to
    /// show.
    pub fn kind_chosen(&self) -> Option<StyleKind> {
        self.showing.kind.filter(|_| self.showing.chosen.is_none())
    }

    /// The Style tab shows kind `kind`: its first style or part, or, for
    /// paragraph styles, the caret's, as if chosen from its list of kinds.
    pub fn choose_kind(&mut self, kind: StyleKind) {
        if kind == StyleKind::Paragraph {
            self.show_caret_style();
            return;
        }
        self.showing = Showing {
            kind: Some(kind),
            ..Showing::default()
        };
        if let Some(first) = self.first_of(kind) {
            self.show(first);
        }
    }

    /// What the Style tab shows: a kind, and the style or part of it, if
    /// there's one to show.
    pub fn style_screen(&self) -> (StyleKind, Option<ObjectId>) {
        if let Some(id) = self.showing.chosen
            && let Some(style) = self.document.style(id)
        {
            return (style.kind, Some(id));
        }
        match self.showing.kind {
            Some(kind) => (kind, self.first_of(kind)),
            None => (
                StyleKind::Paragraph,
                self.sheet.style_of(self.paragraph(self.focus.paragraph)).id,
            ),
        }
    }

    /// The document's styles or parts of `kind`, in the sheet's order.
    fn all_of(&self, kind: StyleKind) -> Vec<ObjectId> {
        self.document
            .styles()
            .iter()
            .copied()
            .filter(|&id| self.document.style(id).is_some_and(|s| s.kind == kind))
            .collect()
    }

    fn first_of(&self, kind: StyleKind) -> Option<ObjectId> {
        self.all_of(kind).first().copied()
    }

    /// Shows style or part `id`, keeping the trail followed to it.
    fn show(&mut self, id: ObjectId) {
        let Some(kind) = self.document.style(id).map(|s| s.kind) else {
            return;
        };
        if kind == StyleKind::Page {
            return;
        }
        self.showing.chosen = Some(id);
        self.showing.kind = Some(kind);
        // A face's screen shows each family in itself, so the families
        // kept until they're needed are loaded.
        if kind == StyleKind::Face
            && let Err(e) = self.res.load_families(&self.res.families())
        {
            self.message = format!("Couldn't load fonts: {e}");
        }
    }

    /// The styles another is based on, by name, if any: why `id` can't
    /// be deleted.
    fn based_on(&self, id: ObjectId) -> Option<String> {
        let by = self
            .document
            .styles()
            .iter()
            .copied()
            .find(|&s| s != id && self.document.style(s).is_some_and(|s| s.base == Some(id)))?;
        Some(name_of(&self.document, by))
    }

    /// What uses part `part`: the styles that name it, and the defaults
    /// and the document's settings if they do, by name.
    fn users(&self, part: ObjectId) -> Vec<String> {
        let names = |props: &Props| props.values().any(|v| *v == Value::Ref(part));
        let mut out: Vec<String> = self
            .document
            .styles()
            .iter()
            .filter(|&&id| self.document.style(id).is_some_and(|s| names(&s.props)))
            .map(|&id| name_of(&self.document, id))
            .collect();
        // The sheet's own: its defaults, the values every chain of styles
        // ends in, and what the whole document shares, such as how quotes
        // and code look and its tab stops.
        let shared = self.document.stylesheet().map_or_else(Vec::new, |s| {
            s.props
                .iter()
                .filter(|(_, v)| **v == Value::Ref(part))
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>()
        });
        if shared.iter().any(|k| [keys::FONT, keys::COLOR].contains(k)) {
            out.push("the defaults".to_string());
        }
        if shared
            .iter()
            .any(|k| ![keys::FONT, keys::COLOR].contains(k))
        {
            out.push("the document's quotes, code, tables or tabs".to_string());
        }
        out
    }

    /// Runs one of the commands of the Style tab's screens with `args`;
    /// false if `id` isn't one.
    pub(super) fn screen_command(&mut self, id: &str, args: &[Arg]) -> bool {
        let int = |i: usize| args.get(i).and_then(Arg::int);
        let object = |i: usize| {
            int(i)
                .and_then(|n| u64::try_from(n).ok())
                .filter(|&n| n > 0)
                .map(ObjectId)
        };
        let text = |i: usize| args.get(i).and_then(Arg::text);
        match id {
            "style-show" => match object(0) {
                Some(style) => self.choose_style(style),
                None => self.show_caret_style(),
            },
            "style-kind" => {
                if let Some(kind) = text(0).and_then(kind_of) {
                    self.choose_kind(kind);
                }
            }
            "style-follow" => {
                if let Some(part) = object(0) {
                    if let (_, Some(from)) = self.style_screen() {
                        self.showing.trail.push(from);
                    }
                    self.show(part);
                }
            }
            "style-back" => {
                if let Some(to) = object(0) {
                    while let Some(last) = self.showing.trail.pop() {
                        if last == to {
                            break;
                        }
                    }
                    self.show(to);
                }
            }
            "style-new" => {
                if let Some(kind) = text(0).and_then(kind_of) {
                    self.new_of(kind, object(1));
                }
            }
            "style-delete" => {
                if let Some(style) = object(0) {
                    self.delete_style(style);
                }
            }
            "style-option" => {
                if let (Some(style), Some(key), Some(option)) = (object(0), text(1), text(2)) {
                    self.set_option(style, key, option);
                }
            }
            "colour-set" => {
                if let (Some(colour), Some(typed)) = (object(0), text(1)) {
                    self.set_colour(colour, typed);
                }
            }
            "layout-stop-add" | "layout-stop-place" | "layout-stop-kind" | "layout-stop-leader"
            | "layout-stop-remove" => {
                if let Some(layout) = object(0) {
                    self.layout_command(id, layout, &args[1..]);
                }
            }
            "list-level" => {
                if let (Some(list), Some(level), Some(what), Some(value)) =
                    (object(0), int(1), text(2), args.get(3))
                {
                    self.set_level(list, level, what, value);
                }
            }
            _ => return false,
        }
        true
    }

    /// Commits `ops` to style `style` as one step called "Edit" and its
    /// name.
    fn edit_style(&mut self, style: ObjectId, ops: Vec<Op>) -> bool {
        let name = name_of(&self.document, style);
        self.commit(&format!("Edit {name}"), ops, false)
    }

    fn set_prop(&mut self, style: ObjectId, key: &str, value: Option<Value>) -> bool {
        self.edit_style(
            style,
            vec![Op::SetStyleProp {
                style,
                key: key.to_string(),
                value,
            }],
        )
    }

    /// A new style or part of `kind`, named as new ones are, in the sheet
    /// after `from`, which a style is based on and a part copies; with
    /// none, a style based on nothing, or a part as plain as can be. The
    /// tab shows it, to rename.
    fn new_of(&mut self, kind: StyleKind, from: Option<ObjectId>) {
        let Some(sheet) = self.document.sheet() else {
            self.message = "The document has no stylesheet to add to".to_string();
            return;
        };
        let from = from.filter(|&f| self.document.style(f).is_some_and(|s| s.kind == kind));
        let taken: Vec<String> = self
            .all_of(kind)
            .iter()
            .map(|&id| name_of(&self.document, id))
            .collect();
        let base = new_name(kind);
        let name = (1..)
            .map(|n| match n {
                1 => base.to_string(),
                n => format!("{base} {n}"),
            })
            .find(|n| !taken.contains(n))
            .unwrap_or_else(|| base.to_string());
        let mut props = Props::new();
        if kind.is_part() {
            match from.and_then(|f| self.document.style(f)) {
                // A copy of the part shown, but for its name and role.
                Some(style) => {
                    props = style.props.clone();
                    props.remove(keys::ROLE);
                }
                None => match kind {
                    StyleKind::Face => {
                        props.insert(
                            keys::FAMILY.to_string(),
                            Value::Text(self.sheet.defaults.family.clone()),
                        );
                    }
                    StyleKind::Colour => {
                        props.insert(
                            keys::COLOR.to_string(),
                            Value::Text(format_colour(self.sheet.defaults.color)),
                        );
                    }
                    StyleKind::Tabs => {
                        props.insert(keys::TABS.to_string(), Value::Text(String::new()));
                    }
                    _ => {
                        let look = self.sheet.list(false, None);
                        props.insert(keys::BULLETS.to_string(), Value::Text("•".to_string()));
                        props.insert(keys::LIST_INDENT.to_string(), Value::Int(look.indent));
                        props.insert(keys::MARKER_GAP.to_string(), Value::Int(look.gap));
                    }
                },
            }
        }
        props.insert(keys::NAME.to_string(), Value::Text(name.clone()));
        let after = from
            .or_else(|| self.all_of(kind).last().copied())
            .or_else(|| self.document.styles().last().copied());
        let mut scratch = self.document.clone();
        let id = scratch.new_id();
        let made = self.commit(
            &format!(
                "New {}",
                KINDS.iter().find(|k| k.0 == kind).map_or("Style", |k| k.2)
            ),
            vec![Op::InsertStyle {
                sheet,
                after,
                id,
                style: doc::Style {
                    sheet,
                    kind,
                    base: from.filter(|_| !kind.is_part()),
                    props,
                },
            }],
            false,
        );
        if made {
            self.showing.trail.clear();
            self.show(id);
            self.message = format!("Made {name}: type its name over");
        }
    }

    /// Deletes style or part `style`, one step; refused, and said, where
    /// another style is based on it, as the model refuses it.
    fn delete_style(&mut self, style: ObjectId) {
        let name = name_of(&self.document, style);
        if let Some(by) = self.based_on(style) {
            self.message = format!("{name} can't be deleted: {by} is based on it");
            return;
        }
        if self.commit(
            &format!("Delete {name}"),
            vec![Op::DeleteStyle { style }],
            false,
        ) {
            self.showing.chosen = None;
            self.message = format!("Deleted {name}");
        }
    }

    /// Sets `style`'s on-or-off value `key` as `option` says: "on",
    /// "off", or "unset", for a character style to leave it to what's
    /// under it. Ligatures are on or off as a whole.
    fn set_option(&mut self, style: ObjectId, key: &str, option: &str) {
        let value = match (key, option) {
            (_, "unset") => None,
            (keys::LIGATURES, "on") => Some(Value::Text(format_ligatures(Ligatures::default()))),
            (keys::LIGATURES, "off") => Some(Value::Text(format_ligatures(Ligatures::NONE))),
            (keys::LIGATURES, _) => return,
            (_, "on") => Some(Value::Bool(true)),
            (_, "off") => Some(Value::Bool(false)),
            _ => return,
        };
        self.set_prop(style, key, value);
    }

    /// Sets colour `colour`'s value to the one `typed`: `#rrggbb`, or
    /// without the `#`, or `#rgb`.
    fn set_colour(&mut self, colour: ObjectId, typed: &str) {
        let hex = typed.trim().trim_start_matches('#').to_ascii_lowercase();
        let hex = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect(),
            _ => hex,
        };
        match parse_colour(&format!("#{hex}")) {
            Some(rgb) => {
                self.set_prop(colour, keys::COLOR, Some(Value::Text(format_colour(rgb))));
            }
            None => {
                self.message = format!("{typed:?} isn't a colour: type it as #rrggbb");
            }
        }
    }

    /// How much further along the ruler a tab layout's stops read than
    /// they're kept: from the ruler's zero, for a paragraph straight in
    /// the story, as the Text tab reads a paragraph's.
    fn layout_shift(&self) -> i64 {
        let ruler = self.ruler_model();
        ruler.start - ruler.zero
    }

    /// Runs one of the commands that edit tab layout `layout`'s stops,
    /// with the rest of its arguments: as the Text tab's do a paragraph's.
    fn layout_command(&mut self, id: &str, layout: ObjectId, args: &[Arg]) {
        let int = |i: usize| args.get(i).and_then(Arg::int);
        let text = |i: usize| args.get(i).and_then(Arg::text);
        let Some(mut stops) = self.document.style(layout).and_then(|s| {
            match s.props.get(keys::TABS) {
                Some(Value::Text(t)) => story::parse_tabs(t),
                _ => Some(Vec::new()),
            }
            .filter(|_| s.kind == StyleKind::Tabs)
        }) else {
            return;
        };
        let shift = self.layout_shift();
        match id {
            "layout-stop-add" => {
                let interval = self.sheet.tabs.interval.max(text::units::MILLIMETRE);
                add_stop(&mut stops, None, StopKind::Start, interval);
            }
            "layout-stop-place" => {
                let (Some(from), Some(at)) = (int(0), int(1)) else {
                    return;
                };
                move_stop(&mut stops, from, (at - shift).max(0));
            }
            "layout-stop-kind" => {
                let (Some(at), Some(kind)) = (int(0), text(1).and_then(StopKind::from_id)) else {
                    return;
                };
                set_stop_kind(&mut stops, at, kind);
            }
            "layout-stop-leader" => {
                let (Some(at), Some(leader)) = (int(0), text(1)) else {
                    return;
                };
                let leader = leader.chars().next().filter(|&c| story::valid_leader(c));
                for stop in stops.iter_mut().filter(|s| s.position == at) {
                    stop.leader = leader;
                }
            }
            _ => {
                let Some(at) = int(0) else { return };
                stops.retain(|s| s.position != at);
            }
        }
        stops.retain(|s| (0..=MOST).contains(&s.position));
        self.set_prop(
            layout,
            keys::TABS,
            Some(Value::Text(story::format_tabs(&stops))),
        );
    }

    /// Sets list definition `list`'s level `level`'s `what` to `value`, as
    /// its screen gives it: its marker by name, its bullet, how its number
    /// reads, the number it starts at as typed, or its indent or gap as a
    /// length.
    fn set_level(&mut self, list: ObjectId, level: i64, what: &str, value: &Arg) {
        let Some(level) = usize::try_from(level)
            .ok()
            .filter(|l| (1..=keys::LEVELS).contains(l))
        else {
            return;
        };
        let value = match (what, value) {
            (keys::LEVEL_MARKER, Arg::Text(t)) if Numbering::of(t).is_some() => {
                Value::Text(t.clone())
            }
            (keys::LEVEL_BULLET, Arg::Text(t)) if valid_bullet(t.trim()) => {
                Value::Text(t.trim().to_string())
            }
            (keys::LEVEL_BULLET, _) => {
                self.message = "A bullet is a few characters, without spaces".to_string();
                return;
            }
            (keys::LEVEL_READS, Arg::Text(t)) if valid_reads(t.trim()) => {
                Value::Text(t.trim().to_string())
            }
            (keys::LEVEL_READS, _) => {
                self.message = "Put a 1 where the number goes, as in (1)".to_string();
                return;
            }
            (keys::LEVEL_START, Arg::Text(t)) => match t.trim().parse::<i64>() {
                Ok(n) if (0..=MOST_START).contains(&n) => Value::Int(n),
                _ => {
                    self.message = format!("A list starts at a number from 0 to {MOST_START}");
                    return;
                }
            },
            (keys::LEVEL_INDENT | keys::LEVEL_GAP, Arg::Int(n)) if (0..=INCH).contains(n) => {
                Value::Int(*n)
            }
            (keys::LEVEL_INDENT | keys::LEVEL_GAP, _) => {
                self.message = "A list's indent and gap are up to an inch".to_string();
                return;
            }
            _ => return,
        };
        self.set_prop(list, &keys::level(level, what), Some(value));
    }

    // ---- the screens

    /// The Text tab's row for the list the caret's paragraph is in, if
    /// it's in one: its label a link to the list definition it takes, in
    /// the Style tab, and the definition's name.
    pub(super) fn list_row(&self) -> Option<Row> {
        let k = self.index_of(self.focus.paragraph);
        let entry = self.laid.entries.get(k)?;
        let list = entry
            .place
            .path
            .iter()
            .rev()
            .find(|a| a.kind == doc::Kind::List)?;
        let own = match self
            .document
            .block(list.id)
            .and_then(|b| b.props.get(keys::LIST))
        {
            Some(Value::Ref(id)) => Some(*id),
            _ => None,
        };
        let definition = self.sheet.list(list.ordered, own);
        let id = definition.id?;
        Some(row(
            "",
            vec![
                button("style-follow", vec![id_arg(id)], true, "List", None),
                note(&definition.name),
            ],
        ))
    }

    /// The inspector's Style tab: what to show, and the screen of the
    /// style or part shown.
    pub(super) fn style_tab(&self) -> InspectorTab {
        let writable = self.writable();
        let (kind, shown) = self.style_screen();
        let mut sections = vec![self.head(kind, shown, writable)];
        match shown {
            None => sections.push(Section {
                title: KINDS
                    .iter()
                    .find(|k| k.0 == kind)
                    .map_or("Styles", |k| k.1)
                    .to_string(),
                rows: vec![row(
                    "",
                    vec![note("The stylesheet has none yet: New makes one")],
                )],
            }),
            Some(id) => sections.extend(match kind {
                StyleKind::Paragraph => self.paragraph_screen(id, writable),
                StyleKind::Character => self.character_screen(id, writable),
                StyleKind::Face => self.face_screen(id, writable),
                StyleKind::Colour => self.colour_screen(id, writable),
                StyleKind::Tabs => self.layout_screen(id, writable),
                StyleKind::List | StyleKind::Page => self.list_screen(id, writable),
            }),
        }
        InspectorTab {
            title: "Style".to_string(),
            sections,
        }
    }

    /// The head of the Style tab: back to where a part was followed from,
    /// the kind shown and one of it, and New and Delete; and why the one
    /// shown can't be deleted, if it can't.
    fn head(&self, kind: StyleKind, shown: Option<ObjectId>, writable: bool) -> Section {
        let mut rows = Vec::new();
        if let Some(&back) = self.showing.trail.last()
            && self.document.style(back).is_some()
        {
            rows.push(row(
                "",
                vec![button(
                    "style-back",
                    vec![id_arg(back)],
                    true,
                    &format!("Back to {}", name_of(&self.document, back)),
                    None,
                )],
            ));
        }
        let at = KINDS.iter().position(|k| k.0 == kind);
        rows.push(row(
            "Kind",
            vec![control(
                "style-kind",
                Vec::new(),
                true,
                choice(
                    KINDS
                        .iter()
                        .map(|k| Choice::new(Arg::Text(k.3.to_string()), k.1))
                        .collect(),
                    at,
                ),
            )],
        ));
        let all = self.all_of(kind);
        let choices: Vec<Choice> = all
            .iter()
            .map(|&id| {
                let name = name_of(&self.document, id);
                let mut c = Choice::new(id_arg(id), name);
                if let Some(part) = self.sheet.part(id) {
                    match kind {
                        StyleKind::Face => c = c.in_family(part.value.clone()),
                        StyleKind::Colour => {
                            if let Some(colour) = parse_colour(&part.value) {
                                c = c.with_swatch(rgb(colour));
                            }
                        }
                        _ => {}
                    }
                }
                c
            })
            .collect();
        let selected = shown.and_then(|s| all.iter().position(|&id| id == s));
        rows.push(row(
            at.map_or("Style", |k| KINDS[k].2),
            vec![control(
                "style-show",
                Vec::new(),
                !all.is_empty(),
                choice(choices, selected),
            )],
        ));
        let code = at.map_or("paragraph", |k| KINDS[k].3);
        let refused = shown.and_then(|id| self.based_on(id));
        rows.push(row(
            "",
            vec![
                button(
                    "style-new",
                    vec![
                        Arg::Text(code.to_string()),
                        Arg::Int(shown.map_or(0, |id| id.0 as i64)),
                    ],
                    writable && self.document.sheet().is_some(),
                    "New",
                    None,
                ),
                button(
                    "style-delete",
                    shown.map(id_arg).into_iter().collect(),
                    writable && shown.is_some() && refused.is_none(),
                    "Delete",
                    None,
                ),
            ],
        ));
        if let Some(by) = refused {
            rows.push(row(
                "",
                vec![note(&format!("It can't be deleted: {by} is based on it"))],
            ));
        }
        Section {
            title: "Styles".to_string(),
            rows,
        }
    }

    /// Where style `style`'s value of `key` comes from, as the Style tab
    /// notes it: "this style", the style up its chain that sets it, or
    /// the defaults; for a character style, whose chain ends in nothing,
    /// "not set" where none does. And whether the style sets it itself.
    fn source_note(&self, style: ObjectId, key: &str, defaults: bool) -> (String, bool) {
        match find(&self.document, Some(style), key, defaults, &valid(key)) {
            Some((_, source)) => match source.level {
                Level::Style(s) if s == style => ("this style".to_string(), true),
                Level::Style(s) => (name_of(&self.document, s), false),
                Level::Defaults => ("defaults".to_string(), false),
                Level::Direct | Level::Program => ("xWorks".to_string(), false),
            },
            None if defaults => ("xWorks".to_string(), false),
            None => ("not set".to_string(), false),
        }
    }

    /// The part style `style`'s value of `key` names, along its chain or
    /// in the defaults, if it names one.
    fn part_named(&self, style: ObjectId, key: &str, defaults: bool) -> Option<ObjectId> {
        find(&self.document, Some(style), key, defaults, &|_| true).and_then(|(_, s)| s.part)
    }

    /// A value's row of style `style`: its control, which runs `command`
    /// with the style and `key`, where the value comes from, and a button
    /// to let the value go where the style sets it itself. Where the value
    /// names a part, its label is a link that follows it to the part's
    /// screen.
    fn value_row(
        &self,
        style: ObjectId,
        label: &str,
        key: &str,
        command: &str,
        kind: ControlKind,
        defaults: bool,
    ) -> Row {
        let writable = self.writable();
        let args = vec![id_arg(style), Arg::Text(key.to_string())];
        let (source, own) = self.source_note(style, key, defaults);
        let mut controls = vec![
            control(command, args.clone(), writable, kind),
            note(&source),
        ];
        if own {
            controls.push(button(
                "style-reset",
                args,
                writable,
                "Take it from up the chain",
                Some("x"),
            ));
        }
        match self.part_named(style, key, defaults) {
            Some(part) => {
                controls.insert(
                    0,
                    button("style-follow", vec![id_arg(part)], true, label, None),
                );
                row("", controls)
            }
            None => row(label, controls),
        }
    }

    /// The choices of a style's face: the sheet's faces, each in its
    /// family, and, if the style names a family itself, that family;
    /// with "Not set" first for a character style. And which is set.
    fn face_choices(&self, style: ObjectId, defaults: bool) -> (Vec<Choice>, Option<usize>) {
        let faces: Vec<&story::Part> = self
            .sheet
            .parts
            .iter()
            .filter(|p| p.kind == StyleKind::Face)
            .collect();
        let mut choices = Vec::new();
        if !defaults {
            choices.push(Choice::new(Arg::Int(0), "Not set"));
        }
        let first = choices.len();
        choices.extend(
            faces
                .iter()
                .map(|p| Choice::new(id_arg(p.id), p.name.clone()).in_family(p.value.clone())),
        );
        let found = find(
            &self.document,
            Some(style),
            keys::FONT,
            defaults,
            &valid(keys::FONT),
        );
        let selected = match found {
            None => (!defaults).then_some(0),
            Some((_, source)) => match source.part {
                Some(part) => faces.iter().position(|p| p.id == part).map(|i| i + first),
                None => None,
            },
        };
        if selected.is_none()
            && let Some((Value::Text(family), _)) = found
        {
            choices.push(Choice::new(Arg::Text(family.clone()), family.clone()).in_family(family));
            let last = choices.len() - 1;
            return (choices, Some(last));
        }
        (choices, selected)
    }

    /// The choices of a style's colour: the sheet's colours, each with
    /// its swatch, and, if the style sets a colour itself, that colour;
    /// with "Not set" first for a character style. And which is set.
    fn colour_choices(&self, style: ObjectId, defaults: bool) -> (Vec<Choice>, Option<usize>) {
        let colours: Vec<&story::Part> = self
            .sheet
            .parts
            .iter()
            .filter(|p| p.kind == StyleKind::Colour)
            .collect();
        let mut choices = Vec::new();
        if !defaults {
            choices.push(Choice::new(Arg::Int(0), "Not set"));
        }
        let first = choices.len();
        choices.extend(colours.iter().map(|p| {
            let c = Choice::new(id_arg(p.id), p.name.clone());
            match parse_colour(&p.value) {
                Some(colour) => c.with_swatch(rgb(colour)),
                None => c,
            }
        }));
        let found = find(
            &self.document,
            Some(style),
            keys::COLOR,
            defaults,
            &valid(keys::COLOR),
        );
        let selected = match found {
            None => (!defaults).then_some(0),
            Some((_, source)) => source
                .part
                .and_then(|part| colours.iter().position(|p| p.id == part))
                .map(|i| i + first),
        };
        if selected.is_none()
            && let Some((Value::Text(hex), _)) = found
            && let Some(colour) = parse_colour(&hex)
        {
            choices.push(Choice::new(Arg::Text(hex.clone()), hex).with_swatch(rgb(colour)));
            let last = choices.len() - 1;
            return (choices, Some(last));
        }
        (choices, selected)
    }

    /// The screen of paragraph style `id`: its name, its base and what the
    /// selection sets over it; its text's face, size, weight, slant and
    /// colour; its paragraphs' alignment, spacing, line height, indents
    /// and tab stops; and the stylesheet it came from.
    fn paragraph_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let unit = self.res.settings.units();
        let style = self.sheet.paragraph_style(Some(id)).clone();
        let sid = id_arg(id);
        let set = |label: &str, key: &str, kind: ControlKind| {
            self.value_row(id, label, key, "style-set", kind, true)
        };
        let option = |label: &str, key: &str, options: [(&str, &str); 2], on: bool| {
            self.value_row(
                id,
                label,
                key,
                "style-option",
                choice(
                    options
                        .iter()
                        .map(|(value, label)| Choice::new(Arg::Text(value.to_string()), *label))
                        .collect(),
                    Some(usize::from(on)),
                ),
                true,
            )
        };
        // The style itself: its name, what it's based on, and what the
        // selection sets itself over it, to clear.
        let bases: Vec<&story::ParagraphStyle> = self
            .sheet
            .paragraph
            .iter()
            .filter(|s| {
                s.id.is_some_and(|b| b != id && !self.document.chain(b).contains(&id))
            })
            .collect();
        let mut base_choices = vec![Choice::new(Arg::Int(0), "Nothing")];
        base_choices.extend(
            bases
                .iter()
                .map(|s| Choice::new(Arg::Int(s.id.map_or(0, |b| b.0 as i64)), s.name.clone())),
        );
        let base_selected = match style.base {
            None => Some(0),
            Some(b) => bases.iter().position(|s| s.id == Some(b)).map(|i| i + 1),
        };
        let mut own_rows = vec![
            row(
                "Name",
                vec![control(
                    "style-rename",
                    vec![sid.clone()],
                    writable,
                    ControlKind::Text {
                        value: style.name.clone(),
                    },
                )],
            ),
            row(
                "Based on",
                vec![control(
                    "style-base",
                    vec![sid.clone()],
                    writable,
                    choice(base_choices, base_selected),
                )],
            ),
        ];
        let own = self.own_formatting();
        if !own.is_empty() {
            own_rows.push(row(
                "Its own",
                vec![
                    note(&own.join(", ")),
                    button(
                        "clear-formatting",
                        Vec::new(),
                        writable,
                        "Clear Formatting",
                        Some("x"),
                    ),
                ],
            ));
        }
        let (faces, face) = self.face_choices(id, true);
        let (colours, colour) = self.colour_choices(id, true);
        let text = vec![
            set("Face", keys::FONT, choice(faces, face)),
            set("Size", keys::SIZE, length(style.size, Unit::Point)),
            option(
                "Weight",
                keys::BOLD,
                [("off", "Regular"), ("on", "Bold")],
                style.bold,
            ),
            option(
                "Slant",
                keys::ITALIC,
                [("off", "Upright"), ("on", "Italic")],
                style.italic,
            ),
            set("Colour", keys::COLOR, choice(colours, colour)),
        ];
        // Its paragraphs.
        let aligns = ["start", "center", "end", "justify"];
        let align_labels = ["Start", "Centre", "End", "Justify"];
        let mut heights = LINE_HEIGHTS.to_vec();
        let line = style.line_height.unwrap_or(0);
        if !heights.contains(&line) {
            heights.push(line);
            heights.sort_unstable();
        }
        let layouts: Vec<&story::Part> = self
            .sheet
            .parts
            .iter()
            .filter(|p| p.kind == StyleKind::Tabs)
            .collect();
        let layout = self.part_named(id, keys::TABS, true);
        let mut tab_choices = vec![Choice::new(
            Arg::Int(0),
            if style.tabs.is_empty() {
                "None"
            } else {
                "Its own"
            },
        )];
        tab_choices.extend(
            layouts
                .iter()
                .map(|p| Choice::new(id_arg(p.id), p.name.clone())),
        );
        let tab_selected = match layout {
            Some(l) => layouts.iter().position(|p| p.id == l).map(|i| i + 1),
            None => Some(0),
        };
        let paragraph = vec![
            set(
                "Align",
                keys::ALIGN,
                choice(
                    aligns
                        .iter()
                        .zip(align_labels)
                        .map(|(a, label)| Choice::new(Arg::Text(a.to_string()), label))
                        .collect(),
                    aligns
                        .iter()
                        .position(|a| *a == story::sheet::align_name(style.align)),
                ),
            ),
            set(
                "Above",
                keys::SPACE_BEFORE,
                length(style.space_before, Unit::Point),
            ),
            set(
                "Below",
                keys::SPACE_AFTER,
                length(style.space_after, Unit::Point),
            ),
            set(
                "Line",
                keys::LINE_HEIGHT,
                choice(
                    heights
                        .iter()
                        .map(|&h| Choice::new(Arg::Int(h), line_height_label(h)))
                        .collect(),
                    heights.iter().position(|&h| h == line),
                ),
            ),
            set(
                "Indent",
                keys::INDENT_START,
                length(style.indents.start, unit),
            ),
            set(
                "First line",
                keys::INDENT_FIRST,
                length(style.indents.first, unit),
            ),
            set("Tab stops", keys::TABS, choice(tab_choices, tab_selected)),
        ];
        // The stylesheet: where it came from, and what can be done with it.
        let origin = match &self.sheet.origin {
            Some((name, version)) => format!("{name}, version {version}"),
            None => "its own".to_string(),
        };
        let mut sheet = vec![
            row("From", vec![note(&origin)]),
            row(
                "",
                vec![button(
                    "save-stylesheet",
                    Vec::new(),
                    writable,
                    "Keep as my own",
                    None,
                )],
            ),
        ];
        if let Some((_, newer)) = self.newer_stylesheet() {
            sheet.push(row(
                "",
                vec![button(
                    "update-stylesheet",
                    Vec::new(),
                    writable,
                    &format!("See what's new in {} {}", newer.name, newer.version),
                    None,
                )],
            ));
        }
        vec![
            Section {
                title: "Paragraph style".to_string(),
                rows: own_rows,
            },
            Section {
                title: "Text".to_string(),
                rows: text,
            },
            Section {
                title: "Paragraph".to_string(),
                rows: paragraph,
            },
            Section {
                title: "Stylesheet".to_string(),
                rows: sheet,
            },
        ]
    }

    /// The screen of character style `id`: its name and base, and each
    /// value it can set, as it sets it, as the style it's based on does,
    /// or not set, which leaves the text as its paragraph has it.
    fn character_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let Some(style) = self.sheet.character_style(id).cloned() else {
            return Vec::new();
        };
        let sid = id_arg(id);
        let values = &style.values;
        let bases: Vec<&story::CharacterStyle> = self
            .sheet
            .character
            .iter()
            .filter(|s| s.id != id && !self.document.chain(s.id).contains(&id))
            .collect();
        let mut base_choices = vec![Choice::new(Arg::Int(0), "Nothing")];
        base_choices.extend(
            bases
                .iter()
                .map(|s| Choice::new(id_arg(s.id), s.name.clone())),
        );
        let base_selected = match style.base {
            None => Some(0),
            Some(b) => bases.iter().position(|s| s.id == b).map(|i| i + 1),
        };
        // An on-or-off value: not set, or one way or the other.
        let option = |label: &str, key: &str, off: &str, on: &str, value: Option<bool>| {
            self.value_row(
                id,
                label,
                key,
                "style-option",
                choice(
                    [("unset", "Not set"), ("off", off), ("on", on)]
                        .iter()
                        .map(|(v, l)| Choice::new(Arg::Text(v.to_string()), *l))
                        .collect(),
                    Some(match value {
                        None => 0,
                        Some(false) => 1,
                        Some(true) => 2,
                    }),
                ),
                false,
            )
        };
        let (faces, face) = self.face_choices(id, false);
        let (colours, colour) = self.colour_choices(id, false);
        let size = ControlKind::Length {
            value: values.size.map(|value| Length {
                value,
                unit: Unit::Point,
            }),
            prefix: String::new(),
            plain: false,
        };
        let ligatures = values.ligatures.map(|l| l == Ligatures::default());
        let rows = vec![
            self.value_row(
                id,
                "Face",
                keys::FONT,
                "style-set",
                choice(faces, face),
                false,
            ),
            self.value_row(id, "Size", keys::SIZE, "style-set", size, false),
            option("Weight", keys::BOLD, "Regular", "Bold", values.bold),
            option("Slant", keys::ITALIC, "Upright", "Italic", values.italic),
            self.value_row(
                id,
                "Colour",
                keys::COLOR,
                "style-set",
                choice(colours, colour),
                false,
            ),
            option(
                "Underline",
                keys::UNDERLINE,
                "None",
                "Underlined",
                values.underline,
            ),
            option(
                "Strike",
                keys::STRIKETHROUGH,
                "None",
                "Struck through",
                values.strikethrough,
            ),
            option("Ligatures", keys::LIGATURES, "Off", "On", ligatures),
        ];
        vec![
            Section {
                title: "Character style".to_string(),
                rows: vec![
                    row(
                        "Name",
                        vec![control(
                            "style-rename",
                            vec![sid.clone()],
                            writable,
                            ControlKind::Text {
                                value: style.name.clone(),
                            },
                        )],
                    ),
                    row(
                        "Based on",
                        vec![control(
                            "style-base",
                            vec![sid],
                            writable,
                            choice(base_choices, base_selected),
                        )],
                    ),
                ],
            },
            Section {
                title: "Text".to_string(),
                rows,
            },
        ]
    }

    /// A part's name, to type over, and what uses it.
    fn part_rows(&self, id: ObjectId, writable: bool) -> Vec<Row> {
        let users = self.users(id);
        vec![
            row(
                "Name",
                vec![control(
                    "style-rename",
                    vec![id_arg(id)],
                    writable,
                    ControlKind::Text {
                        value: name_of(&self.document, id),
                    },
                )],
            ),
            row(
                "Used by",
                vec![note(&if users.is_empty() {
                    "nothing yet".to_string()
                } else {
                    users.join(", ")
                })],
            ),
        ]
    }

    /// The screen of face `id`: its name, its family, chosen from those
    /// xWorks has, each shown in itself, and what uses it.
    fn face_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let family = self
            .sheet
            .part(id)
            .map(|p| p.value.clone())
            .unwrap_or_default();
        let mut families = self.res.families();
        if !family.is_empty() && !families.contains(&family) {
            families.push(family.clone());
        }
        let selected = families.iter().position(|f| *f == family);
        let mut rows = self.part_rows(id, writable);
        rows.insert(
            1,
            row(
                "Family",
                vec![control(
                    "style-set",
                    vec![id_arg(id), Arg::Text(keys::FAMILY.to_string())],
                    writable,
                    choice(
                        families
                            .iter()
                            .map(|f| {
                                Choice::new(Arg::Text(f.clone()), f.clone()).in_family(f.clone())
                            })
                            .collect(),
                        selected,
                    ),
                )],
            ),
        );
        vec![Section {
            title: "Face".to_string(),
            rows,
        }]
    }

    /// The screen of colour `id`: its name, its value, typed as
    /// `#rrggbb` beside its swatch, and what uses it.
    fn colour_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let value = self
            .sheet
            .part(id)
            .and_then(|p| parse_colour(&p.value))
            .map(rgb);
        let mut rows = self.part_rows(id, writable);
        rows.insert(
            1,
            row(
                "Colour",
                vec![control(
                    "colour-set",
                    vec![id_arg(id)],
                    writable,
                    ControlKind::Colour { value },
                )],
            ),
        );
        vec![Section {
            title: "Colour".to_string(),
            rows,
        }]
    }

    /// The screen of tab layout `id`: its name, what uses it, and its
    /// stops, each its place on the ruler, read from its zero in its
    /// unit, its alignment and its leader, to move, change and remove,
    /// and one more to add, as the Text tab has a paragraph's.
    fn layout_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let unit = self.res.settings.units();
        let stops: Vec<TabStop> = match self.sheet.part(id) {
            Some(p) => story::parse_tabs(&p.value).unwrap_or_default(),
            None => Vec::new(),
        };
        let shift = self.layout_shift();
        let kinds = [
            StopKind::Start,
            StopKind::Center,
            StopKind::End,
            StopKind::Decimal('.'),
            StopKind::Decimal(','),
        ];
        let mut rows: Vec<Row> = stops
            .iter()
            .map(|stop| {
                let at = vec![id_arg(id), Arg::Int(stop.position)];
                let kind = stop_kind(stop.align);
                let mut options = kinds.to_vec();
                if !options.contains(&kind) {
                    options.push(kind);
                }
                let mut leaders: Vec<(String, String)> = LEADERS
                    .iter()
                    .map(|(c, l)| (c.to_string(), l.to_string()))
                    .collect();
                let leader = stop.leader.map(String::from).unwrap_or_default();
                if !leaders.iter().any(|(c, _)| *c == leader) {
                    leaders.push((leader.clone(), format!("Leader {leader}")));
                }
                row(
                    "",
                    vec![
                        control(
                            "layout-stop-place",
                            at.clone(),
                            writable,
                            ControlKind::Length {
                                value: Some(Length {
                                    value: stop.position + shift,
                                    unit,
                                }),
                                prefix: String::new(),
                                plain: true,
                            },
                        ),
                        control(
                            "layout-stop-kind",
                            at.clone(),
                            writable,
                            ControlKind::Choice {
                                selected: options.iter().position(|k| *k == kind),
                                choices: options
                                    .iter()
                                    .map(|k| Choice::new(Arg::Text(k.id()), stop_label(*k)))
                                    .collect(),
                                plain: true,
                            },
                        ),
                        control(
                            "layout-stop-leader",
                            at.clone(),
                            writable,
                            ControlKind::Choice {
                                selected: leaders.iter().position(|(c, _)| *c == leader),
                                choices: leaders
                                    .iter()
                                    .map(|(c, l)| Choice::new(Arg::Text(c.clone()), l.clone()))
                                    .collect(),
                                plain: true,
                            },
                        ),
                        button("layout-stop-remove", at, writable, "Remove", Some("x")),
                    ],
                )
            })
            .collect();
        rows.push(row(
            "",
            vec![button(
                "layout-stop-add",
                vec![id_arg(id)],
                writable,
                "Add a tab stop",
                None,
            )],
        ));
        vec![
            Section {
                title: "Tab layout".to_string(),
                rows: self.part_rows(id, writable),
            },
            Section {
                title: "Tab stops".to_string(),
                rows,
            },
        ]
    }

    /// The screen of list definition `id`: its name and what it's for, and
    /// its levels, at least three, each its marker, a bullet or a
    /// numbering, its bullet or how its number reads and where it starts,
    /// its indent and the room after its marker; each with a button to let
    /// what the level sets itself go back to the definition's.
    fn list_screen(&self, id: ObjectId, writable: bool) -> Vec<Section> {
        let unit = self.res.settings.units();
        let Some(list) = self.sheet.lists.iter().find(|l| l.id == Some(id)) else {
            return Vec::new();
        };
        let props = self
            .document
            .style(id)
            .map(|s| s.props.clone())
            .unwrap_or_default();
        let numbered = match list.role.as_deref() {
            Some("bullets") => false,
            Some("numbers" | "headings") => true,
            _ => !list.numbers.is_empty(),
        };
        let what = match list.role.as_deref() {
            Some("bullets") => "Bulleted lists",
            Some("numbers") => "Numbered lists",
            Some("headings") => "Numbered headings",
            _ => "Lists that name it",
        };
        let mut sections = vec![Section {
            title: "List".to_string(),
            rows: vec![
                row(
                    "Name",
                    vec![control(
                        "style-rename",
                        vec![id_arg(id)],
                        writable,
                        ControlKind::Text {
                            value: list.name.clone(),
                        },
                    )],
                ),
                row("For", vec![note(what)]),
            ],
        }];
        let count = list.levels().clamp(3, keys::LEVELS);
        for k in 1..=count {
            let level = list.level(k - 1);
            let numbering = level.numbering(numbered);
            let args =
                |what: &str| vec![id_arg(id), Arg::Int(k as i64), Arg::Text(what.to_string())];
            // A level's row: its control, and a button to let it go where
            // the level sets it itself.
            let with_reset = |label: &str, what: &str, kind: ControlKind| {
                let mut controls = vec![control("list-level", args(what), writable, kind)];
                let key = keys::level(k, what);
                if props.contains_key(&key) {
                    controls.push(button(
                        "style-reset",
                        vec![id_arg(id), Arg::Text(key)],
                        writable,
                        "Take it from the list",
                        Some("x"),
                    ));
                }
                row(label, controls)
            };
            let markers: Vec<Choice> = Numbering::ALL
                .iter()
                .map(|n| {
                    let label = match n {
                        Numbering::Bullet => "Bullet".to_string(),
                        n => (1..=3).map(|i| n.number(i)).collect::<Vec<_>>().join(", "),
                    };
                    Choice::new(Arg::Text(n.code().to_string()), label)
                })
                .collect();
            let mut rows = vec![with_reset(
                "Marker",
                keys::LEVEL_MARKER,
                choice(markers, Numbering::ALL.iter().position(|n| *n == numbering)),
            )];
            if numbering == Numbering::Bullet {
                rows.push(with_reset(
                    "Bullet",
                    keys::LEVEL_BULLET,
                    ControlKind::Text {
                        value: level.bullet.clone(),
                    },
                ));
            } else {
                rows.push(with_reset(
                    "Reads",
                    keys::LEVEL_READS,
                    ControlKind::Text {
                        value: if level.reads.is_empty() {
                            "1.".to_string()
                        } else {
                            level.reads.clone()
                        },
                    },
                ));
                rows.push(with_reset(
                    "Starts at",
                    keys::LEVEL_START,
                    ControlKind::Text {
                        value: level.start.to_string(),
                    },
                ));
            }
            rows.push(with_reset(
                "Indent",
                keys::LEVEL_INDENT,
                length(level.indent, unit),
            ));
            rows.push(with_reset(
                "After marker",
                keys::LEVEL_GAP,
                length(level.gap, unit),
            ));
            sections.push(Section {
                title: format!("Level {k}"),
                rows,
            });
        }
        sections
    }
}
