//! What the inspector and the ruler show of the selection, and the
//! commands with arguments they run: a paragraph's style, alignment,
//! spacing, line height, indents and tab stops, and its text's font, size
//! and language.

use chrome::{
    Arg, Choice, Control, ControlKind, Indents, Inspector, InspectorTab, Length, Row, Ruler,
    Section, Segment, Stop, StopKind, Unit,
};
use doc::{ObjectId, Op, Value};
use story::keys;
use text::units::{MILLIMETRE, POINT};
use text::{TabAlign, TabStop};

use super::{Editor, Effect, View};

/// Line heights the inspector offers, in thousandths of the font; 0 is
/// as tall as the fonts make them.
const LINE_HEIGHTS: &[i64] = &[0, 1000, 1150, 1200, 1500, 2000];

/// How a line height reads: "1.2 × the font".
fn line_height_label(per_mille: i64) -> String {
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

/// The ruler's kind of tab stop, as text's.
fn stop_kind(align: TabAlign) -> StopKind {
    match align {
        TabAlign::Start => StopKind::Start,
        TabAlign::Center => StopKind::Center,
        TabAlign::End => StopKind::End,
        TabAlign::Decimal(c) => StopKind::Decimal(c),
    }
}

/// Text's kind of tab stop, as the ruler's.
fn tab_align(kind: StopKind) -> TabAlign {
    match kind {
        StopKind::Start => TabAlign::Start,
        StopKind::Center => TabAlign::Center,
        StopKind::End => TabAlign::End,
        StopKind::Decimal(c) => TabAlign::Decimal(c),
    }
}

/// The most a length typed into the inspector may be: a metre.
const MOST: i64 = 1000 * MILLIMETRE;

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
    fn selected_paragraphs(&self) -> Vec<ObjectId> {
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
    /// own, with `change`.
    fn change_tabs(&mut self, name: &str, change: impl Fn(&mut Vec<TabStop>)) {
        let ops = self
            .selected_paragraphs()
            .into_iter()
            .map(|paragraph| {
                let p = self.paragraph(paragraph);
                let mut stops = story::paragraph_tabs(p, &self.res.sheet).stops;
                change(&mut stops);
                stops.retain(|s| (0..=MOST).contains(&s.position));
                Op::SetParaProp {
                    paragraph,
                    key: keys::TABS.to_string(),
                    value: Some(Value::Text(story::format_tabs(&stops))),
                }
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
        match id {
            "paragraph-style" => {
                if let Some(name) =
                    text(0).filter(|n| self.res.sheet.paragraph.iter().any(|s| s.name == *n))
                {
                    self.set_style(name);
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
                let interval = self.res.sheet.tabs.interval.units().max(MILLIMETRE);
                let at = int(0);
                self.change_tabs("Tab Stop", |stops| {
                    let position = at.unwrap_or_else(|| {
                        let last = stops.iter().map(|s| s.position).max().unwrap_or(0);
                        (last / interval + 1) * interval
                    });
                    stops.retain(|s| s.position != position);
                    stops.push(TabStop {
                        position,
                        align: tab_align(kind),
                    });
                });
            }
            "tab-stop-move" => {
                if let (Some(from), Some(to)) = (int(0), int(1)) {
                    self.change_tabs("Tab Stop", |stops| {
                        let moved = stops.iter().position(|s| s.position == from);
                        if let Some(i) = moved {
                            let align = stops[i].align;
                            stops.remove(i);
                            stops.retain(|s| s.position != to);
                            stops.push(TabStop {
                                position: to,
                                align,
                            });
                        }
                    });
                }
            }
            "tab-stop-kind" => {
                if let (Some(at), Some(kind)) = (int(0), text(1).and_then(StopKind::from_id)) {
                    self.change_tabs("Tab Stop", |stops| {
                        for stop in stops.iter_mut().filter(|s| s.position == at) {
                            stop.align = tab_align(kind);
                        }
                    });
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

    /// The ruler over the caret's paragraph: the text's column, the
    /// paragraph's place in it, and its indents and tab stops.
    pub(super) fn ruler_model(&self) -> Ruler {
        let style = self.page_style();
        let (x, _) = self.page_origin(0);
        let start = x + style.left;
        let k = self.index_of(self.focus.paragraph);
        let laid = &self.laid.paragraphs[k];
        let paragraph = self.paragraph(self.focus.paragraph);
        let own = story::indents(paragraph);
        Ruler {
            start,
            width: style.body_width(),
            origin: start + laid.place.indent + laid.inset,
            extent: laid.place.width - 2 * laid.inset,
            unit: Unit::Centimetre,
            indents: Some(Indents {
                start: own.start,
                end: own.end,
                first: own.first,
            }),
            stops: story::paragraph_tabs(paragraph, &self.res.sheet)
                .stops
                .iter()
                .map(|s| Stop {
                    position: s.position,
                    kind: stop_kind(s.align),
                })
                .collect(),
            enabled: self.writable(),
        }
    }

    /// The inspector's fields for the selection: the Text tab, its
    /// paragraph's, its characters' and its tab stops; and the Page tab.
    pub(super) fn inspector_model(&self) -> Inspector {
        let writable = self.writable();
        let at = self.focus;
        let paragraph = self.paragraph(at.paragraph);
        let sheet = &self.res.sheet;

        // The paragraph.
        let style = &sheet.style_of(paragraph).name;
        let styles: Vec<Choice> = sheet
            .paragraph
            .iter()
            .map(|s| Choice {
                value: Arg::Text(s.name.clone()),
                label: s.name.clone(),
            })
            .collect();
        let same_style = self
            .selected_paragraphs()
            .iter()
            .all(|&id| &sheet.style_of(self.paragraph(id)).name == style);
        let style_row = Row {
            label: "Style".to_string(),
            controls: vec![control(
                "paragraph-style",
                Vec::new(),
                writable,
                ControlKind::Choice {
                    selected: same_style
                        .then(|| sheet.paragraph.iter().position(|s| &s.name == style))
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
        let line = story::line_height(paragraph).unwrap_or(0);
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
                        .map(|&h| Choice {
                            value: Arg::Int(h),
                            label: line_height_label(h),
                        })
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
                        .map(|f| Choice {
                            value: Arg::Text(f.clone()),
                            label: f.clone(),
                        })
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
                        .map(|(tag, title)| Choice {
                            value: Arg::Text(tag),
                            label: title,
                        })
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
        let mut tab_rows: Vec<Row> = stops
            .iter()
            .map(|stop| {
                let kind = stop_kind(stop.align);
                let mut options: Vec<StopKind> = kinds.to_vec();
                if !options.contains(&kind) {
                    options.push(kind);
                }
                Row {
                    label: String::new(),
                    controls: vec![
                        control(
                            "tab-stop-move",
                            vec![Arg::Int(stop.position)],
                            writable,
                            ControlKind::Length {
                                value: length(stop.position, Unit::Centimetre),
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
                                    .map(|k| Choice {
                                        value: Arg::Text(k.id()),
                                        label: match k {
                                            StopKind::Decimal(c) if *c != '.' => {
                                                format!("Decimal ({c})")
                                            }
                                            _ => k.name().to_string(),
                                        },
                                    })
                                    .collect(),
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

        // The page: the document can't hold its own size and margins yet,
        // so they're shown, and can't be changed.
        let page = &self.res.page;
        let sizes = [
            ("A4", 210 * MILLIMETRE, 297 * MILLIMETRE),
            ("A5", 148 * MILLIMETRE, 210 * MILLIMETRE),
            ("US Letter", 2159 * MILLIMETRE / 10, 2794 * MILLIMETRE / 10),
            ("US Legal", 2159 * MILLIMETRE / 10, 3556 * MILLIMETRE / 10),
        ];
        let size = sizes.iter().position(|(_, w, h)| {
            (w - page.width).abs() < MILLIMETRE && (h - page.height).abs() < MILLIMETRE
        });
        let margin = |label: &str, side: &str, value: i64| {
            control(
                "page-margin",
                vec![Arg::Text(side.to_string())],
                false,
                ControlKind::Length {
                    value: length(value, Unit::Millimetre),
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
                            false,
                            ControlKind::Choice {
                                selected: size,
                                choices: sizes
                                    .iter()
                                    .map(|(name, ..)| Choice {
                                        value: Arg::Text(name.to_string()),
                                        label: name.to_string(),
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
        Inspector {
            tabs: vec![
                InspectorTab {
                    title: "Text".to_string(),
                    sections: vec![
                        Section {
                            title: "Paragraph".to_string(),
                            rows: vec![style_row, align_row, spacing_row, line_row],
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
