//! What the inspector and the ruler show of the selection, and the
//! commands with arguments they run: a paragraph's style, alignment,
//! spacing, line height, indents and tab stops with their leaders, and its
//! text's font, size and language.

use chrome::{
    Arg, Choice, Control, ControlKind, Indents, Inspector, InspectorTab, Length, Row, Ruler,
    Section, Segment, Stop, StopKind, Unit,
};
use doc::{ObjectId, Op, Value};
use story::keys;
use text::units::{MILLIMETRE, POINT};
use text::{TabAlign, TabStop};

use super::{Editor, Effect, View};
use crate::settings::RulerFrom;

/// Line heights the inspector offers, in thousandths of the font; 0 is
/// as tall as the fonts make them.
const LINE_HEIGHTS: &[i64] = &[0, 1000, 1150, 1200, 1500, 2000];

/// How a line height reads: "1.2 × the font".
pub(super) fn line_height_label(per_mille: i64) -> String {
    if per_mille == 0 {
        return "As its fonts".to_string();
    }
    let whole = per_mille / 1000;
    let part = per_mille % 1000;
    let number = if part == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{part:03}").trim_end_matches('0'))
    };
    format!("{number} × the font")
}

/// What a kind of tab stop is called in the inspector: "Decimal (,)" for
/// one on a comma.
pub(super) fn stop_label(kind: StopKind) -> String {
    match kind {
        StopKind::Decimal(c) if c != '.' => format!("Decimal ({c})"),
        _ => kind.name().to_string(),
    }
}

/// The ruler's kind of tab stop, as text's.
pub(super) fn stop_kind(align: TabAlign) -> StopKind {
    match align {
        TabAlign::Start => StopKind::Start,
        TabAlign::Center => StopKind::Center,
        TabAlign::End => StopKind::End,
        TabAlign::Decimal(c) => StopKind::Decimal(c),
    }
}

/// Text's kind of tab stop, as the ruler's.
pub(super) fn tab_align(kind: StopKind) -> TabAlign {
    match kind {
        StopKind::Start => TabAlign::Start,
        StopKind::Center => TabAlign::Center,
        StopKind::End => TabAlign::End,
        StopKind::Decimal(c) => TabAlign::Decimal(c),
    }
}

/// The most a length typed into the inspector may be: a metre.
pub(super) const MOST: i64 = 1000 * MILLIMETRE;

/// Adds a stop of `kind` to `stops` at `at`, or after the last at the next
/// multiple of `interval`, in place of one already there.
pub(super) fn add_stop(stops: &mut Vec<TabStop>, at: Option<i64>, kind: StopKind, interval: i64) {
    let position = at.unwrap_or_else(|| {
        let last = stops.iter().map(|s| s.position).max().unwrap_or(0);
        (last / interval + 1) * interval
    });
    stops.retain(|s| s.position != position);
    stops.push(TabStop {
        position,
        align: tab_align(kind),
        leader: None,
    });
}

/// Moves the stop of `stops` at `from` to `to`, in place of one there,
/// keeping its kind and leader.
pub(super) fn move_stop(stops: &mut Vec<TabStop>, from: i64, to: i64) {
    if let Some(i) = stops.iter().position(|s| s.position == from) {
        let stop = stops.remove(i);
        stops.retain(|s| s.position != to);
        stops.push(TabStop {
            position: to,
            ..stop
        });
    }
}

/// Makes the stop of `stops` at `at` of `kind`.
pub(super) fn set_stop_kind(stops: &mut [TabStop], at: i64, kind: StopKind) {
    for stop in stops.iter_mut().filter(|s| s.position == at) {
        stop.align = tab_align(kind);
    }
}

/// The leaders the inspector offers a tab stop, the Text tab a
/// paragraph's and the Style tab a tab layout's: none, or a character,
/// each as it's given to commands and what it's called.
const LEADERS: [(&str, &str); 4] = [
    ("", "No leader"),
    (".", "Dots"),
    ("-", "Dashes"),
    ("_", "Line"),
];

/// The choices of a stop's leader, which is `leader`: none, dots, dashes
/// or a line, and another character it has, as itself; and which it is.
pub(super) fn leader_choices(leader: Option<char>) -> (Vec<Choice>, Option<usize>) {
    let mut leaders: Vec<(String, String)> = LEADERS
        .iter()
        .map(|(c, l)| (c.to_string(), l.to_string()))
        .collect();
    let leader = leader.map(String::from).unwrap_or_default();
    if !leaders.iter().any(|(c, _)| *c == leader) {
        leaders.push((leader.clone(), format!("Leader {leader}")));
    }
    let selected = leaders.iter().position(|(c, _)| *c == leader);
    (
        leaders
            .into_iter()
            .map(|(c, l)| Choice::new(Arg::Text(c), l))
            .collect(),
        selected,
    )
}

/// Gives the stop of `stops` at `at` the leader `typed` names: its
/// character, if it's one a leader can be, or none, as for "".
pub(super) fn set_stop_leader(stops: &mut [TabStop], at: i64, typed: &str) {
    let leader = typed.chars().next().filter(|&c| story::valid_leader(c));
    for stop in stops.iter_mut().filter(|s| s.position == at) {
        stop.leader = leader;
    }
}

fn control(command: &str, args: Vec<Arg>, enabled: bool, kind: ControlKind) -> Control {
    Control {
        command: command.to_string(),
        args,
        enabled,
        kind,
    }
}

fn length(value: i64, unit: Unit) -> Option<Length> {
    Some(Length { value, unit })
}

impl Editor {
    /// Shows the story on pages or as a draft.
    pub(super) fn set_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.scroll = 0;
        self.relayout(true);
        self.reveal();
    }

    /// The paragraphs the selection touches, in reading order.
    pub(crate) fn selected_paragraphs(&self) -> Vec<ObjectId> {
        let (start, end) = self.ordered();
        let (a, b) = (self.index_of(start.paragraph), self.index_of(end.paragraph));
        self.ids()[a..=b].to_vec()
    }

    /// Sets paragraph property `key` on every paragraph selected.
    fn set_paragraphs(&mut self, name: &str, key: &str, value: Option<Value>) {
        let ops = self
            .selected_paragraphs()
            .into_iter()
            .map(|paragraph| Op::SetParaProp {
                paragraph,
                key: key.to_string(),
                value: value.clone(),
            })
            .collect();
        self.commit(name, ops, false);
    }

    /// Changes the tab stops of every paragraph selected, each from its
    /// own, with `change`, as one edit; a paragraph whose stops it leaves
    /// as they were is left as it was.
    fn change_tabs(&mut self, name: &str, change: impl Fn(&mut Vec<TabStop>)) {
        let ops = self
            .selected_paragraphs()
            .into_iter()
            .filter_map(|paragraph| {
                let p = self.paragraph(paragraph);
                let before = story::paragraph_tabs(p, &self.sheet).stops;
                let mut stops = before.clone();
                change(&mut stops);
                stops.retain(|s| (0..=MOST).contains(&s.position));
                (stops != before).then(|| Op::SetParaProp {
                    paragraph,
                    key: keys::TABS.to_string(),
                    value: Some(Value::Text(story::format_tabs(&stops))),
                })
            })
            .collect();
        self.commit(name, ops, false);
    }

    /// Runs command `id` with `args`, as the inspector, the ruler or a
    /// script give them.
    pub(super) fn command_with(&mut self, id: &str, args: &[Arg]) -> Vec<Effect> {
        if !self.state(id).enabled {
            return Vec::new();
        }
        let int = |i: usize| args.get(i).and_then(Arg::int);
        let text = |i: usize| args.get(i).and_then(Arg::text);
        if self.screen_command(id, args) || self.style_command(id, args) {
            return Vec::new();
        }
        match id {
            "new-with-stylesheet" => {
                if let Some(key) = text(0) {
                    return self.new_with_stylesheet(key);
                }
            }
            "replace-stylesheet" => {
                if let Some(key) = text(0) {
                    self.replace_stylesheet(key);
                }
            }
            "paragraph-style" => {
                // A style by ID, as the inspector gives it, or by name, as a
                // script may; "clear" after it takes the text's own
                // formatting away too.
                let style = match args.first() {
                    Some(Arg::Int(id)) => u64::try_from(*id)
                        .ok()
                        .map(ObjectId)
                        .filter(|id| self.sheet.paragraph.iter().any(|s| s.id == Some(*id))),
                    Some(Arg::Text(name)) => self
                        .sheet
                        .paragraph
                        .iter()
                        .find(|s| s.name == *name)
                        .and_then(|s| s.id),
                    _ => None,
                };
                let clear = text(1) == Some("clear");
                if let Some(style) = style {
                    self.set_style(style, clear);
                }
            }
            "font" => {
                if let Some(family) = text(0) {
                    let value = Value::Text(family.to_string());
                    if self.collapsed() {
                        self.pending.insert(keys::FONT.to_string(), value);
                        self.typing = None;
                    } else {
                        self.set_char_prop("Font", keys::FONT, Some(value));
                    }
                }
            }
            "size" => {
                if let Some(size) = int(0).filter(|s| (POINT..=1000 * POINT).contains(s)) {
                    self.set_size(size);
                }
            }
            "language" => {
                if let Some(tag) = text(0) {
                    self.set_language(tag);
                }
            }
            "space-before" | "space-after" | "indent-start" | "indent-end" | "indent-first" => {
                if let Some(value) = int(0).filter(|v| v.abs() <= MOST) {
                    let (name, key) = match id {
                        "space-before" => ("Spacing", keys::SPACE_BEFORE),
                        "space-after" => ("Spacing", keys::SPACE_AFTER),
                        "indent-start" => ("Indent", keys::INDENT_START),
                        "indent-end" => ("Indent", keys::INDENT_END),
                        _ => ("Indent", keys::INDENT_FIRST),
                    };
                    let value = if id.starts_with("space") {
                        value.max(0)
                    } else {
                        value
                    };
                    self.set_paragraphs(name, key, Some(Value::Int(value)));
                }
            }
            "line-height" => {
                if let Some(value) = int(0).filter(|v| *v == 0 || (250..=10_000).contains(v)) {
                    let value = (value > 0).then_some(Value::Int(value));
                    self.set_paragraphs("Line Height", keys::LINE_HEIGHT, value);
                }
            }
            "tab-stop-add" => {
                let kind = text(1)
                    .and_then(StopKind::from_id)
                    .unwrap_or(StopKind::Start);
                let interval = self.sheet.tabs.interval.max(MILLIMETRE);
                let at = int(0);
                self.change_tabs("Tab Stop", |stops| add_stop(stops, at, kind, interval));
            }
            "tab-stop-move" => {
                if let (Some(from), Some(to)) = (int(0), int(1)) {
                    self.change_tabs("Tab Stop", |stops| move_stop(stops, from, to));
                }
            }
            "tab-stop-place" => {
                if let (Some(from), Some(at)) = (int(0), int(1)) {
                    let to = (at - self.stops_shift()).max(0);
                    self.change_tabs("Tab Stop", |stops| move_stop(stops, from, to));
                }
            }
            "tab-stop-kind" => {
                if let (Some(at), Some(kind)) = (int(0), text(1).and_then(StopKind::from_id)) {
                    self.change_tabs("Tab Stop", |stops| set_stop_kind(stops, at, kind));
                }
            }
            "tab-stop-leader" => {
                if let (Some(at), Some(leader)) = (int(0), text(1)) {
                    self.change_tabs("Leader", |stops| set_stop_leader(stops, at, leader));
                }
            }
            "tab-stop-remove" => {
                if let Some(at) = int(0) {
                    self.change_tabs("Tab Stop", |stops| stops.retain(|s| s.position != at));
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// How much further along the ruler the caret's paragraph's tab stops
    /// read in the inspector than they're kept: from the ruler's zero, the
    /// margin or the paper's edge, rather than the paragraph's place,
    /// which a list or a quote moves in (review 77nh3z47).
    fn stops_shift(&self) -> i64 {
        let ruler = self.ruler_model();
        ruler.origin - ruler.zero
    }

    /// The ruler over the caret's paragraph: the page, or the draft's
    /// column, which is its page; the text's column, the paragraph's
    /// place in it, and its indents and tab stops; numbered from the
    /// column's start or the page's, as the person chose.
    pub(super) fn ruler_model(&self) -> Ruler {
        let style = self.page_style();
        let (x, _) = self.page_origin(0);
        let start = x + style.left;
        let k = self.index_of(self.focus.paragraph);
        let laid = &self.laid.paragraphs[k];
        let paragraph = self.paragraph(self.focus.paragraph);
        let own = story::indents_at(paragraph, &self.sheet, &laid.place);
        Ruler {
            page_start: x,
            page_width: style.width,
            start,
            width: style.body_width(),
            zero: match self.res.settings.ruler_from() {
                RulerFrom::Margin => start,
                RulerFrom::Edge => x,
            },
            origin: start + laid.place.indent + laid.inset,
            extent: laid.place.width - 2 * laid.inset,
            unit: self.res.settings.units(),
            indents: Some(Indents {
                start: own.start,
                end: own.end,
                first: own.first,
            }),
            stops: story::paragraph_tabs(paragraph, &self.sheet)
                .stops
                .iter()
                .map(|s| Stop {
                    position: s.position,
                    kind: stop_kind(s.align),
                    leader: s.leader,
                })
                .collect(),
            enabled: self.writable(),
        }
    }

    /// The inspector's fields for the selection: the Text tab, its
    /// paragraph's, its characters' and its tab stops; and the Page tab.
    /// Lengths of the page and the paragraph are in the person's unit;
    /// the space above and below a paragraph, and type sizes, are in
    /// points, as type is set (review 77nh3z47).
    pub(super) fn inspector_model(&self) -> Inspector {
        let writable = self.writable();
        let unit = self.res.settings.units();
        let at = self.focus;
        let paragraph = self.paragraph(at.paragraph);
        let sheet = &self.sheet;

        // The paragraph: its style, by ID, with a mark after its name where
        // the selection has formatting of its own (project owner,
        // 2026-10-24, review 30dmn1js), as InDesign's "+".
        let style = sheet.style_of(paragraph).id;
        let marked = self.has_direct_formatting();
        let styles: Vec<Choice> = sheet
            .paragraph
            .iter()
            .map(|s| {
                Choice::new(
                    Arg::Int(s.id.map_or(0, |id| id.0 as i64)),
                    if marked && s.id == style {
                        format!("{} +", s.name)
                    } else {
                        s.name.clone()
                    },
                )
            })
            .collect();
        let same_style = self
            .selected_paragraphs()
            .iter()
            .all(|&id| sheet.style_of(self.paragraph(id)).id == style);
        let style_row = Row {
            label: "Style".to_string(),
            controls: vec![control(
                "paragraph-style",
                Vec::new(),
                writable,
                ControlKind::Choice {
                    selected: same_style
                        .then(|| sheet.paragraph.iter().position(|s| s.id == style))
                        .flatten(),
                    choices: styles,
                    plain: false,
                },
            )],
        };
        let aligns = ["start", "center", "end", "justify"];
        let align_row = Row {
            label: "Align".to_string(),
            controls: vec![control(
                "",
                Vec::new(),
                writable,
                ControlKind::Segments {
                    segments: aligns
                        .iter()
                        .map(|a| Segment {
                            command: format!("align-{a}"),
                            label: self
                                .res
                                .commands
                                .get(&format!("align-{a}"))
                                .map_or_else(String::new, |c| c.title.clone()),
                            icon: Some(format!("text-align-{a}")),
                        })
                        .collect(),
                    selected: aligns
                        .iter()
                        .position(|a| self.state(&format!("align-{a}")).checked),
                },
            )],
        };
        let (before, after) = story::spacing(paragraph, sheet);
        let spacing_row = Row {
            label: "Spacing".to_string(),
            controls: vec![
                control(
                    "space-before",
                    Vec::new(),
                    writable,
                    ControlKind::Length {
                        value: length(before, Unit::Point),
                        prefix: "↑".to_string(),
                        plain: false,
                    },
                ),
                control(
                    "space-after",
                    Vec::new(),
                    writable,
                    ControlKind::Length {
                        value: length(after, Unit::Point),
                        prefix: "↓".to_string(),
                        plain: false,
                    },
                ),
            ],
        };
        let line = story::line_height(paragraph, &self.sheet).unwrap_or(0);
        let mut heights = LINE_HEIGHTS.to_vec();
        if !heights.contains(&line) {
            heights.push(line);
            heights.sort_unstable();
        }
        let line_row = Row {
            label: "Line".to_string(),
            controls: vec![control(
                "line-height",
                Vec::new(),
                writable,
                ControlKind::Choice {
                    selected: heights.iter().position(|&h| h == line),
                    choices: heights
                        .iter()
                        .map(|&h| Choice::new(Arg::Int(h), line_height_label(h)))
                        .collect(),
                    plain: false,
                },
            )],
        };

        // The characters.
        let props = self.typing_props(self.ordered().0);
        let family = match props.get(keys::FONT) {
            Some(Value::Text(family)) => family.clone(),
            _ => sheet.style_of(paragraph).family.clone(),
        };
        let mut families: Vec<String> = self
            .res
            .fonts()
            .faces()
            .map(|(_, face)| face.family().to_string())
            .collect();
        families.sort();
        families.dedup();
        if !families.contains(&family) {
            families.push(family.clone());
        }
        let font_row = Row {
            label: "Font".to_string(),
            controls: vec![control(
                "font",
                Vec::new(),
                writable,
                ControlKind::Choice {
                    selected: families.iter().position(|f| *f == family),
                    choices: families
                        .iter()
                        .map(|f| Choice::new(Arg::Text(f.clone()), f.clone()))
                        .collect(),
                    plain: false,
                },
            )],
        };
        let toggle = |id: &str, icon: &str| {
            control(
                id,
                Vec::new(),
                writable,
                ControlKind::Toggle {
                    icon: icon.to_string(),
                    label: self
                        .res
                        .commands
                        .get(id)
                        .map_or_else(String::new, |c| c.title.clone()),
                    on: self.state(id).checked,
                },
            )
        };
        let size_row = Row {
            label: "Size".to_string(),
            controls: vec![
                control(
                    "size",
                    Vec::new(),
                    writable,
                    ControlKind::Length {
                        value: length(self.size(), Unit::Point),
                        prefix: String::new(),
                        plain: false,
                    },
                ),
                toggle("bold", "bold"),
                toggle("italic", "italic"),
            ],
        };
        let languages: Vec<(String, String)> = self
            .res
            .commands
            .all()
            .iter()
            .filter_map(|c| {
                c.id.strip_prefix("language-")
                    .map(|tag| (tag.to_string(), c.title.clone()))
            })
            .collect();
        let language = self.language_at(at);
        let language_row = Row {
            label: "Language".to_string(),
            controls: vec![control(
                "language",
                Vec::new(),
                writable,
                ControlKind::Choice {
                    selected: languages.iter().position(|(tag, _)| {
                        text::Language::parse(tag)
                            .ok()
                            .is_some_and(|l| Some(l) == language)
                    }),
                    choices: languages
                        .into_iter()
                        .map(|(tag, title)| Choice::new(Arg::Text(tag), title))
                        .collect(),
                    plain: false,
                },
            )],
        };

        // The tab stops, each with its kind, and one more to add.
        let kinds = [
            StopKind::Start,
            StopKind::Center,
            StopKind::End,
            StopKind::Decimal('.'),
            StopKind::Decimal(','),
        ];
        let stops = story::paragraph_tabs(paragraph, sheet).stops;
        let shift = self.stops_shift();
        let mut tab_rows: Vec<Row> = stops
            .iter()
            .map(|stop| {
                let kind = stop_kind(stop.align);
                let leader = leader_choices(stop.leader);
                let mut options: Vec<StopKind> = kinds.to_vec();
                if !options.contains(&kind) {
                    options.push(kind);
                }
                Row {
                    label: String::new(),
                    controls: vec![
                        control(
                            "tab-stop-place",
                            vec![Arg::Int(stop.position)],
                            writable,
                            ControlKind::Length {
                                value: length(stop.position + shift, unit),
                                prefix: String::new(),
                                plain: true,
                            },
                        ),
                        control(
                            "tab-stop-kind",
                            vec![Arg::Int(stop.position)],
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
                            "tab-stop-leader",
                            vec![Arg::Int(stop.position)],
                            writable,
                            ControlKind::Choice {
                                selected: leader.1,
                                choices: leader.0,
                                plain: true,
                            },
                        ),
                        control(
                            "tab-stop-remove",
                            vec![Arg::Int(stop.position)],
                            writable,
                            ControlKind::Button {
                                label: "Remove".to_string(),
                                icon: Some("x".to_string()),
                            },
                        ),
                    ],
                }
            })
            .collect();
        tab_rows.push(Row {
            label: String::new(),
            controls: vec![control(
                "tab-stop-add",
                Vec::new(),
                writable,
                ControlKind::Button {
                    label: "Add a tab stop".to_string(),
                    icon: None,
                },
            )],
        });

        // The page: the document's page style, its size and margins
        // (project owner, 2026-10-24, review 30dmn1js).
        let page = &self.sheet.page;
        let sizes = super::styling::PAGE_SIZES;
        let page_writable = writable && self.document.sheet().is_some();
        let size = sizes.iter().position(|(_, w, h)| {
            (w - page.width).abs() < MILLIMETRE && (h - page.height).abs() < MILLIMETRE
        });
        let margin = |label: &str, side: &str, value: i64| {
            control(
                "page-margin",
                vec![Arg::Text(side.to_string())],
                page_writable,
                ControlKind::Length {
                    value: length(value, unit),
                    prefix: label.to_string(),
                    plain: false,
                },
            )
        };
        let page_tab = InspectorTab {
            title: "Page".to_string(),
            sections: vec![
                Section {
                    title: "Page".to_string(),
                    rows: vec![Row {
                        label: "Size".to_string(),
                        controls: vec![control(
                            "page-size",
                            Vec::new(),
                            page_writable,
                            ControlKind::Choice {
                                selected: size,
                                choices: sizes
                                    .iter()
                                    .map(|(name, ..)| {
                                        Choice::new(Arg::Text(name.to_string()), name.to_string())
                                    })
                                    .collect(),
                                plain: false,
                            },
                        )],
                    }],
                },
                Section {
                    title: "Margins".to_string(),
                    rows: vec![
                        Row {
                            label: "Top".to_string(),
                            controls: vec![
                                margin("↑", "top", page.top),
                                margin("↓", "bottom", page.bottom),
                            ],
                        },
                        Row {
                            label: "Sides".to_string(),
                            controls: vec![
                                margin("←", "left", page.left),
                                margin("→", "right", page.right),
                            ],
                        },
                    ],
                },
            ],
        };
        let mut paragraph_rows = vec![style_row, align_row, spacing_row, line_row];
        paragraph_rows.extend(self.list_row());
        Inspector {
            tabs: vec![
                InspectorTab {
                    title: "Text".to_string(),
                    sections: vec![
                        Section {
                            title: "Paragraph".to_string(),
                            rows: paragraph_rows,
                        },
                        Section {
                            title: "Character".to_string(),
                            rows: vec![font_row, size_row, language_row],
                        },
                        Section {
                            title: "Tab stops".to_string(),
                            rows: tab_rows,
                        },
                    ],
                },
                self.style_tab(),
                page_tab,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_heights_read_as_multiples_of_the_font() {
        assert_eq!(line_height_label(0), "As its fonts");
        assert_eq!(line_height_label(1000), "1 × the font");
        assert_eq!(line_height_label(1200), "1.2 × the font");
        assert_eq!(line_height_label(1150), "1.15 × the font");
    }
}
