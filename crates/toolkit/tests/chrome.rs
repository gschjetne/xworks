//! The toolkit's widgets hit what they draw: the title bar's menus, name,
//! buttons and window buttons; a popup's rows; the ruler's stops and
//! indents; the inspector's tabs and controls; the palette's commands. And
//! a field edits as documents' text does.

use std::sync::OnceLock;

use chrome::{
    Arg, Choice, CommandState, Commands, Control, ControlKind, Indents, Inspector, InspectorTab,
    Key, Length, Modifiers, Row, Ruler, Section, Segment, Stop, StopKind, Unit,
};
use text::Fonts;
use text::units::PIXEL;
use toolkit::{
    ControlRef, Field, FieldKey, Icons, InspectorHit, InspectorView, Kit, Labels, PaletteHit,
    PaletteView, Popup, PopupRow, RulerHit, RulerView, Theme, TitleBar, TitleHit, TitleModel,
};

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            fonts.add(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        fonts
    })
}

fn spec(name: &str) -> String {
    testkit::specs("themes")
        .expect("spec/themes")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("spec/themes/{name}.ron"))
        .1
}

fn theme(look: &str) -> Theme {
    Theme::new(
        ron::from_str(&spec("chrome")).expect("chrome.ron"),
        ron::from_str(&spec(look)).expect("a look"),
    )
}

fn icons() -> &'static Icons {
    static ICONS: OnceLock<Icons> = OnceLock::new();
    ICONS.get_or_init(|| {
        let (icons, bad) = Icons::from_svgs(testkit::icons().expect("icons/lucide"));
        assert!(bad.is_empty(), "{bad:?}");
        icons
    })
}

fn px(n: i64) -> i64 {
    n * PIXEL
}

fn commands() -> Commands {
    let source = testkit::specs("commands")
        .expect("spec/commands")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("m1.ron")
        .1;
    Commands::from_spec(&ron::from_str(&source).expect("a spec")).expect("it holds")
}

#[test]
fn both_looks_read_and_differ() {
    let (light, dark) = (theme("light"), theme("dark"));
    assert_ne!(light.colors.desk, dark.colors.desk);
    // Pages are white in both.
    assert_eq!(light.colors.page, dark.colors.page);
    assert_eq!(light.sizes.family, "Inter");
}

#[test]
fn the_title_bar_hits_what_it_draws() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let state = |_: &str| CommandState {
        enabled: true,
        checked: false,
    };
    let menus = commands().menu_bar(false, state, |_| Vec::new());
    let buttons = [
        Control {
            command: "toggle-inspector".to_string(),
            args: Vec::new(),
            enabled: true,
            kind: ControlKind::Toggle {
                icon: "panel-right".to_string(),
                label: "Inspector".to_string(),
                on: true,
            },
        },
        Control {
            command: String::new(),
            args: Vec::new(),
            enabled: true,
            kind: ControlKind::Segments {
                segments: ["Draft", "Pages"]
                    .iter()
                    .map(|l| Segment {
                        command: format!("view-{}", l.to_lowercase()),
                        label: l.to_string(),
                        icon: None,
                    })
                    .collect(),
                selected: Some(1),
            },
        },
    ];
    let model = TitleModel {
        menus: &menus,
        open: None,
        access: true,
        name: "A walking skeleton",
        renaming: None,
        buttons: &buttons,
        maximized: false,
    };
    let width = px(1280);
    let bar = TitleBar::new(&kit, &model, width);
    let mid = bar.height() / 2;
    assert_eq!(bar.height(), px(40));
    // The menus from the left, File first.
    assert_eq!(bar.hit(px(20), mid), Some(TitleHit::Menu(0)));
    let format = bar.menu(4).expect("Format");
    let (fx, fy) = format.centre();
    assert_eq!(bar.hit(fx, fy), Some(TitleHit::Menu(4)));
    // The window's own buttons at the right: close last.
    assert_eq!(bar.hit(width - px(10), mid), Some(TitleHit::Close));
    assert_eq!(bar.hit(width - px(60), mid), Some(TitleHit::Maximize));
    assert_eq!(bar.hit(width - px(110), mid), Some(TitleHit::Minimize));
    // The inspector's button, then Draft and Pages, from the right.
    let right = width - px(3 * 46 + 12);
    assert_eq!(bar.hit(right - px(14), mid), Some(TitleHit::Button(0, 0)));
    assert_eq!(
        bar.hit(right - px(28 + 10 + 32), mid),
        Some(TitleHit::Button(1, 1))
    );
    assert_eq!(
        bar.hit(right - px(28 + 10 + 96), mid),
        Some(TitleHit::Button(1, 0))
    );
    // The name in the middle, and the rest drags the window.
    assert_eq!(bar.hit(width / 2, mid), Some(TitleHit::Name));
    assert_eq!(bar.hit(width / 2 - px(200), mid), Some(TitleHit::Drag));
    assert_eq!(bar.hit(px(20), px(41)), None);
    let drawn = bar.draw(&kit, &model, Some(TitleHit::Close));
    assert!(drawn.len() > 20);
    // Renaming draws a field in the name's place.
    let field = Field::new("A walking skeleton");
    let renaming = TitleModel {
        renaming: Some(&field),
        ..model
    };
    assert!(bar.draw(&kit, &renaming, None).len() > drawn.len() - 2);
}

#[test]
fn popups_hit_their_rows_and_stay_in_the_window() {
    let theme = theme("dark");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let state = |id: &str| CommandState {
        enabled: id != "redo",
        checked: false,
    };
    let menus = commands().menu_bar(false, state, |_| Vec::new());
    let rows = PopupRow::of_menu(&menus.menus[1].items);
    let window = (px(800), px(600));
    let popup = Popup::new(&kit, &rows, (px(40), px(40)), px(200), window);
    let row = |i: usize| popup.row(i).expect("a row").centre();
    let (x, y) = row(0);
    assert_eq!(popup.hit(x, y), Some(Some(0)));
    let (x, y) = row(2);
    assert_eq!(popup.hit(x, y), Some(Some(2)), "the separator is a row too");
    assert_eq!(popup.hit(px(5), px(5)), None);
    assert!(popup.rect().width >= px(200));
    assert!(!popup.draw(&kit, &rows, Some(0), true).is_empty());
    // Opened near the corner, it moves back inside.
    let late = Popup::new(&kit, &rows, (px(790), px(590)), 0, window);
    assert!(late.rect().right() <= px(800) && late.rect().bottom() <= px(600));
}

#[test]
fn the_ruler_hits_its_stops_and_indents() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let model = ruler_model(Unit::Centimetre);
    let cm = Unit::Centimetre.units();
    let ruler = RulerView::new(&kit, &model, px(40), px(1000));
    let foot = px(40 + 20);
    let head = px(40 + 6);
    assert_eq!(ruler.hit(px(100) + 5 * cm, foot), Some(RulerHit::Stop(0)));
    assert_eq!(
        ruler.hit(px(100) + 9 * cm + px(2), foot),
        Some(RulerHit::Stop(1))
    );
    assert_eq!(ruler.hit(px(100) + 2 * cm, head), Some(RulerHit::First));
    assert_eq!(ruler.hit(px(100) + cm, foot), Some(RulerHit::Start));
    assert_eq!(ruler.hit(px(100) + 15 * cm, foot), Some(RulerHit::End));
    assert_eq!(
        ruler.hit(px(100) + 7 * cm, foot),
        Some(RulerHit::Foot(7 * cm))
    );
    assert_eq!(ruler.hit(px(50), head), Some(RulerHit::Bar));
    assert_eq!(ruler.hit(px(50), px(10)), None);
    assert!(ruler.draw(&kit).len() > 40);
}

/// A ruler over a column 15 cm wide from 100 pixels, on a page 2.5 cm
/// wider each side, with its first line indented 1 cm, and stops at 5 cm
/// and 9 cm, measured in `unit`.
fn ruler_model(unit: Unit) -> Ruler {
    let cm = Unit::Centimetre.units();
    Ruler {
        page_start: px(100) - 5 * cm / 2,
        page_width: 20 * cm,
        start: px(100),
        width: 15 * cm,
        origin: px(100),
        extent: 15 * cm,
        unit,
        indents: Some(Indents {
            start: cm,
            end: 0,
            first: cm,
        }),
        stops: vec![
            Stop {
                position: 5 * cm,
                kind: StopKind::Start,
            },
            Stop {
                position: 9 * cm,
                kind: StopKind::Decimal('.'),
            },
        ],
        enabled: true,
    }
}

/// The ruler is marked in its unit from the page's one edge to the other,
/// margins too, and numbered out from the column's start both ways:
/// across the window, nothing marks it beyond the page.
#[test]
fn the_ruler_marks_the_page_in_its_unit() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    for unit in [Unit::Centimetre, Unit::Inch, Unit::Pica] {
        let model = ruler_model(unit);
        let (left, right) = (model.page_start, model.page_start + model.page_width);
        let marks = unit
            .marks()
            .between(left - model.start, right - model.start);
        let numbers = marks
            .iter()
            .filter(|(_, m)| matches!(m, chrome::Mark::Number(_)))
            .count();
        let drawn = RulerView::new(&kit, &model, px(40), px(1000)).draw(&kit);
        // The ticks: hairlines in the secondary ink.
        let ticks: Vec<scene::Rect> = drawn
            .iter()
            .filter(|item| {
                matches!(item, scene::Item::Fill { color, .. } if *color == theme.colors.secondary)
            })
            .filter_map(scene::Item::bounds)
            .filter(|r| r.width == PIXEL)
            .collect();
        assert_eq!(ticks.len(), marks.len() - numbers, "{unit:?}");
        assert!(
            ticks.iter().all(|r| r.x >= left && r.x <= right),
            "{unit:?}: ticked beyond the page"
        );
        assert!(
            ticks.iter().any(|r| r.x < model.start),
            "{unit:?}: the margin isn't ticked"
        );
        // The numbers, each its own run of glyphs.
        let glyphs = drawn
            .iter()
            .filter(|item| matches!(item, scene::Item::Glyphs(_)))
            .count();
        assert_eq!(glyphs, numbers, "{unit:?}");
    }
    // Twenty centimetres: from 2 in the left margin to 17 at the page's
    // right edge, and numbered out both ways from the column's start.
    let marks = Unit::Centimetre.marks().between(
        -5 * Unit::Centimetre.units() / 2,
        35 * Unit::Centimetre.units() / 2,
    );
    let numbers: Vec<i64> = marks
        .iter()
        .filter_map(|(_, m)| match m {
            chrome::Mark::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(
        numbers,
        [
            2, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17
        ]
    );
}

fn inspector() -> Inspector {
    let length = |command: &str, value: i64, prefix: &str, plain: bool| Control {
        command: command.to_string(),
        args: Vec::new(),
        enabled: true,
        kind: ControlKind::Length {
            value: Some(Length {
                value,
                unit: Unit::Point,
            }),
            prefix: prefix.to_string(),
            plain,
        },
    };
    let toggle = |command: &str, icon: &str, on| Control {
        command: command.to_string(),
        args: Vec::new(),
        enabled: true,
        kind: ControlKind::Toggle {
            icon: icon.to_string(),
            label: command.to_string(),
            on,
        },
    };
    let style = Control {
        command: "paragraph-style".to_string(),
        args: Vec::new(),
        enabled: true,
        kind: ControlKind::Choice {
            choices: ["Body", "Heading 1"]
                .iter()
                .map(|s| Choice {
                    value: Arg::Text(s.to_string()),
                    label: s.to_string(),
                })
                .collect(),
            selected: Some(0),
            plain: false,
        },
    };
    let remove = Control {
        command: "tab-stop-remove".to_string(),
        args: vec![Arg::Int(5)],
        enabled: true,
        kind: ControlKind::Button {
            label: "Remove".to_string(),
            icon: Some("x".to_string()),
        },
    };
    Inspector {
        tabs: vec![
            InspectorTab {
                title: "Text".to_string(),
                sections: vec![
                    Section {
                        title: "Paragraph".to_string(),
                        rows: vec![
                            Row {
                                label: "Style".to_string(),
                                controls: vec![style],
                            },
                            Row {
                                label: "Spacing".to_string(),
                                controls: vec![
                                    length("space-before", 0, "↑", false),
                                    length("space-after", 76_200, "↓", false),
                                ],
                            },
                            Row {
                                label: "Size".to_string(),
                                controls: vec![
                                    length("size", 139_700, "", false),
                                    toggle("bold", "bold", true),
                                    toggle("italic", "italic", false),
                                ],
                            },
                        ],
                    },
                    Section {
                        title: "Tab stops".to_string(),
                        rows: vec![Row {
                            label: String::new(),
                            controls: vec![length("tab-stop-move", 5, "", true), remove],
                        }],
                    },
                ],
            },
            InspectorTab {
                title: "Page".to_string(),
                sections: Vec::new(),
            },
        ],
    }
}

#[test]
fn the_inspector_hits_its_controls() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let model = inspector();
    let view = InspectorView::new(&kit, &model, 0, px(1280), px(40), px(774));
    assert_eq!(view.rect().x, px(980));
    // The tabs, Text and Page.
    assert_eq!(view.hit(px(1000), px(66)), Some(InspectorHit::Tab(0)));
    assert_eq!(view.hit(px(1250), px(66)), Some(InspectorHit::Tab(1)));
    let at = |section, row, control| ControlRef {
        section,
        row,
        control,
    };
    let centre = |r: ControlRef| view.control(r).expect("a control").centre();
    for r in [
        at(0, 0, 0),
        at(0, 1, 0),
        at(0, 1, 1),
        at(0, 2, 0),
        at(0, 2, 1),
        at(0, 2, 2),
    ] {
        let (x, y) = centre(r);
        assert_eq!(view.hit(x, y), Some(InspectorHit::Control(r, 0)), "{r:?}");
    }
    // Space above and below share their row; the toggles share half of
    // theirs, each its own part.
    let (a, b) = (
        view.control(at(0, 1, 0)).unwrap(),
        view.control(at(0, 1, 1)).unwrap(),
    );
    assert_eq!(a.width, b.width);
    assert!(a.right() < b.x);
    let (bold, italic) = (
        view.control(at(0, 2, 1)).unwrap(),
        view.control(at(0, 2, 2)).unwrap(),
    );
    assert_eq!(bold.right(), italic.x);
    // A tab stop's row: its position where a label would be, and its
    // button at the right.
    assert_eq!(view.control(at(1, 0, 0)).unwrap().x, px(980 + 16));
    assert_eq!(view.control(at(1, 0, 1)).unwrap().right(), px(1280 - 16));
    assert_eq!(view.hit(px(990), px(700)), Some(InspectorHit::Panel));
    assert_eq!(view.hit(px(900), px(100)), None);
    let field = Field::new("12 pt");
    let drawn = view.draw(&kit, &model, 0, None, Some((at(0, 2, 0), &field)));
    assert!(drawn.len() > 20);
}

#[test]
fn the_palette_hits_what_it_found() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let found = commands().find("head", false, |_| true);
    assert!(found.len() >= 3);
    let view = PaletteView::new(&kit, found.len(), px(1280), px(40));
    assert_eq!(view.rect().width, px(560));
    let (x, top) = (px(640), view.rect().y);
    assert_eq!(view.hit(x, top + px(20)), Some(PaletteHit::Field));
    assert_eq!(view.hit(x, top + px(52 + 6 + 17)), Some(PaletteHit::Row(0)));
    assert_eq!(
        view.hit(x, top + px(52 + 6 + 34 + 17)),
        Some(PaletteHit::Row(1))
    );
    assert_eq!(view.hit(px(10), top), None);
    let field = Field::new("head");
    assert!(view.draw(&kit, &field, &found, 0, 0).len() > 10);
}

#[test]
fn fields_edit_as_documents_do() {
    let theme = theme("light");
    let labels = Labels::new();
    let kit = Kit {
        fonts: fonts(),
        theme: &theme,
        icons: icons(),
        labels: &labels,
    };
    let ty = kit.ty(theme.sizes.value, 400);
    let mut field = Field::new("A walking skeleton");
    let key = |field: &mut Field, key: Key, modifiers: Modifiers, mac: bool| {
        let layout = kit.layout(field.text(), ty);
        field.key(&layout, key, modifiers, mac)
    };
    let none = Modifiers::default();
    let primary = Modifiers {
        primary: true,
        ..none
    };
    let alt = Modifiers { alt: true, ..none };
    // All of it selected at first: typing replaces it.
    field.insert("Draft notes");
    assert_eq!(field.text(), "Draft notes");
    assert_eq!(key(&mut field, Key::Home, none, false), FieldKey::Moved);
    // Ctrl+Right to the next word's start on Windows and Linux.
    key(&mut field, Key::Right, primary, false);
    assert_eq!(field.caret(), 6);
    // Option+Right to the word's end on a Mac.
    key(&mut field, Key::Home, none, true);
    key(&mut field, Key::Right, alt, true);
    assert_eq!(field.caret(), 5);
    // Backspace takes a grapheme; with Ctrl, a word.
    key(&mut field, Key::End, none, false);
    assert_eq!(
        key(&mut field, Key::Backspace, none, false),
        FieldKey::Changed
    );
    assert_eq!(field.text(), "Draft note");
    key(&mut field, Key::Backspace, primary, false);
    assert_eq!(field.text(), "Draft ");
    // Shift extends; typing replaces what's selected.
    key(&mut field, Key::Home, none, false);
    let shift = Modifiers {
        shift: true,
        ..none
    };
    for _ in 0..5 {
        key(&mut field, Key::Right, shift, false);
    }
    assert_eq!(field.selected(), "Draft");
    field.insert("Final");
    assert_eq!(field.text(), "Final ");
    // Line breaks don't go in.
    field.insert("a\nb");
    assert_eq!(field.text(), "Finalab ");
    assert_eq!(key(&mut field, Key::Enter, none, false), FieldKey::Enter);
    assert_eq!(key(&mut field, Key::Escape, none, false), FieldKey::Escape);
    key(&mut field, Key::Char('a'), primary, false);
    assert_eq!(field.selected(), "Finalab ");
    // A click puts the caret where it lands.
    let layout = kit.layout(field.text(), ty);
    field.press(&layout, 0, false);
    assert_eq!(field.caret(), 0);
}
