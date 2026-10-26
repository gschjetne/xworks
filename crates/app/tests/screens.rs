//! The inspector's Style tab edits every kind of style and part (project
//! owner, 2026-10-26, review sjj5ak5a): a face's family, a colour's value,
//! a tab layout's stops, a list definition's levels and a character
//! style's values, each followed by what uses it; styles and parts made,
//! renamed and deleted; each edit undone. And the inspector scrolls, for
//! the pointer, the keyboard, screen readers and drawing alike.

use std::sync::{Arc, OnceLock};

use app::{Effect, Event, Resources, Window};
use chrome::{Arg, ControlKind, Key, Modifiers};
use doc::{ObjectId, StyleKind, Value};
use part::Part;
use part::accesskit::Role;
use story::keys;
use text::units::{MILLIMETRE, PIXEL, POINT};

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

fn res(folder: &str) -> Arc<Resources> {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir(folder).expect("a folder");
    Arc::new(res)
}

/// A window on a new document, 1280 by 800, in the stylesheet `sheet`
/// names ("app:Neutral" and so on), with `text` typed in it.
fn window(folder: &str, sheet: &str, text: &str) -> Window {
    let res = res(folder);
    let mut window = Window::create(res.clone()).expect("a document");
    if sheet != "app:Neutral" {
        let effects = with(
            &mut window,
            "new-with-stylesheet",
            vec![Arg::Text(sheet.into())],
        );
        let Some(Effect::Open(Some(path))) = effects.first() else {
            panic!("a document in {sheet}: {effects:?}");
        };
        window = Window::open(res, path).expect("it opens");
    }
    window.update(Event::Resize {
        width: 1280.0,
        height: 800.0,
    });
    window.update(Event::Text(text.to_string()));
    window
}

fn with(window: &mut Window, id: &str, args: Vec<Arg>) -> Vec<Effect> {
    window.update(Event::CommandWith(id.to_string(), args))
}

fn command(window: &mut Window, id: &str) {
    window.update(Event::Command(id.to_string()));
}

fn id(id: ObjectId) -> Arg {
    Arg::Int(id.0 as i64)
}

fn text(t: &str) -> Arg {
    Arg::Text(t.to_string())
}

/// The ID of the window's document's style or part of `kind` named
/// `name`.
fn named(window: &Window, kind: StyleKind, name: &str) -> ObjectId {
    let document = window.editor().document();
    document
        .styles()
        .iter()
        .copied()
        .find(|&s| {
            let style = document.style(s).unwrap();
            style.kind == kind && style.props.get(keys::NAME) == Some(&Value::Text(name.into()))
        })
        .unwrap_or_else(|| panic!("no {} {name}", kind.name()))
}

fn paragraph_style(window: &Window, name: &str) -> story::ParagraphStyle {
    let id = named(window, StyleKind::Paragraph, name);
    window.editor().sheet().paragraph_style(Some(id)).clone()
}

/// The labels of the Style tab's sections, and its rows' labels.
fn style_tab(window: &Window) -> chrome::InspectorTab {
    window
        .editor()
        .inspector()
        .tabs
        .into_iter()
        .find(|t| t.title == "Style")
        .expect("a Style tab")
}

/// Whether `row` is labelled `label`: as its label, or as the link in its
/// label's place that follows its value to the part it names.
fn labelled(row: &chrome::Row, label: &str) -> bool {
    row.label == label
        || row.label.is_empty()
            && row.controls.first().is_some_and(|c| {
                c.command == "style-follow"
                    && matches!(&c.kind, ControlKind::Button { label: l, .. } if l == label)
            })
}

/// The Style tab's control in the row labelled `label` of the section
/// titled `section`, its first after the link in its label's place, if
/// it has one.
fn control_in(window: &Window, section: &str, label: &str) -> chrome::Control {
    let tab = style_tab(window);
    let section = tab
        .sections
        .iter()
        .find(|s| s.title == section)
        .unwrap_or_else(|| panic!("no section {section}: {tab:#?}"));
    let row = section
        .rows
        .iter()
        .find(|r| labelled(r, label))
        .unwrap_or_else(|| panic!("no row {label} in {}", section.title));
    let link = usize::from(row.label.is_empty() && row.controls.len() > 1);
    row.controls[link].clone()
}

/// What the status bar says of the last thing done.
fn message(window: &Window) -> String {
    window.editor().message().to_string()
}

#[test]
fn a_faces_family_changed_restyles_every_style_that_uses_it() {
    let mut window = window("app-screens-face", "app:Neutral", "A heading");
    command(&mut window, "style-heading-1");
    let heading = named(&window, StyleKind::Face, "Heading");
    // Shown, its family offered among those xWorks has, each in itself.
    with(&mut window, "style-kind", vec![text("face")]);
    assert_eq!(
        window.editor().style_screen(),
        (
            StyleKind::Face,
            Some(named(&window, StyleKind::Face, "Text"))
        )
    );
    with(&mut window, "style-show", vec![id(heading)]);
    let family = control_in(&window, "Face", "Family");
    let ControlKind::Choice {
        choices, selected, ..
    } = &family.kind
    else {
        panic!("a choice of families");
    };
    assert_eq!(choices[selected.unwrap()].label, "Inter");
    assert!(choices.iter().any(|c| c.label == "EB Garamond"));
    assert!(
        choices
            .iter()
            .all(|c| c.family.as_deref() == Some(c.label.as_str())),
        "each shown in itself"
    );
    assert_eq!(
        control_in(&window, "Face", "Used by").kind,
        ControlKind::Note {
            text: "Heading".to_string()
        }
    );
    // Changed: every heading follows, through the Heading style that
    // names it; the body, in the Text face, doesn't.
    let before = paragraph_style(&window, "Body").family;
    with(
        &mut window,
        "style-set",
        vec![id(heading), text(keys::FAMILY), text("EB Garamond")],
    );
    for h in ["Heading 1", "Heading 2", "Heading 6"] {
        assert_eq!(paragraph_style(&window, h).family, "EB Garamond", "{h}");
    }
    assert_eq!(paragraph_style(&window, "Body").family, before);
    let garamond = resources()
        .fonts()
        .faces()
        .any(|(_, f)| f.family() == "EB Garamond");
    assert!(garamond, "EB Garamond loaded, as the face sets text in it");
    command(&mut window, "undo");
    assert!(
        message(&window).contains("Undid Edit Heading"),
        "{}",
        message(&window)
    );
    assert_eq!(paragraph_style(&window, "Heading 1").family, "Inter");
}

#[test]
fn a_colours_value_changed_recolours_what_uses_it() {
    let mut window = window("app-screens-colour", "app:Neutral", "Inked");
    let ink = named(&window, StyleKind::Colour, "Ink");
    with(&mut window, "style-show", vec![id(ink)]);
    assert_eq!(
        control_in(&window, "Colour", "Colour").kind,
        ControlKind::Colour {
            value: Some([0, 0, 0])
        }
    );
    with(&mut window, "colour-set", vec![id(ink), text("#336699")]);
    assert_eq!(paragraph_style(&window, "Body").color, (0x33, 0x66, 0x99));
    assert_eq!(
        paragraph_style(&window, "Heading 1").color,
        (0x33, 0x66, 0x99)
    );
    // Typed without its #, or short.
    with(&mut window, "colour-set", vec![id(ink), text("C0F")]);
    assert_eq!(paragraph_style(&window, "Body").color, (0xcc, 0x00, 0xff));
    // What isn't a colour changes nothing, and says why.
    with(&mut window, "colour-set", vec![id(ink), text("#12345")]);
    assert!(
        message(&window).contains("isn't a colour"),
        "{}",
        message(&window)
    );
    assert_eq!(paragraph_style(&window, "Body").color, (0xcc, 0x00, 0xff));
    command(&mut window, "undo");
    command(&mut window, "undo");
    assert_eq!(paragraph_style(&window, "Body").color, (0, 0, 0));
}

#[test]
fn a_tab_layouts_stop_moved_moves_every_paragraphs_stops() {
    let mut window = window("app-screens-tabs", "app:Neutral", "Name\t12");
    // A new tab layout, shown, with no stops yet; one added.
    with(&mut window, "style-new", vec![text("tabs"), Arg::Int(0)]);
    let layout = named(&window, StyleKind::Tabs, "New Tab Layout");
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Tabs, Some(layout))
    );
    with(&mut window, "layout-stop-add", vec![id(layout)]);
    let interval = window.editor().sheet().tabs.interval;
    // Body takes it: the paragraph's stops are the layout's.
    let body = named(&window, StyleKind::Paragraph, "Body");
    with(
        &mut window,
        "style-set",
        vec![id(body), text(keys::TABS), id(layout)],
    );
    let stops = |window: &Window| {
        let p = window.editor().document().paragraphs()[0];
        let paragraph = window.editor().document().paragraph(p).unwrap().clone();
        story::paragraph_tabs(&paragraph, window.editor().sheet()).stops[0]
    };
    assert_eq!(stops(&window).position, interval);
    // Moved, read from the ruler's zero, the margin: the paragraph's stop
    // follows. Then right-aligned, with dots before it.
    with(
        &mut window,
        "layout-stop-place",
        vec![id(layout), Arg::Int(interval), Arg::Int(100 * MILLIMETRE)],
    );
    assert_eq!(stops(&window).position, 100 * MILLIMETRE);
    with(
        &mut window,
        "layout-stop-kind",
        vec![id(layout), Arg::Int(100 * MILLIMETRE), text("end")],
    );
    with(
        &mut window,
        "layout-stop-leader",
        vec![id(layout), Arg::Int(100 * MILLIMETRE), text(".")],
    );
    let stop = stops(&window);
    assert_eq!(stop.align, text::TabAlign::End);
    assert_eq!(stop.leader, Some('.'));
    // The stop's row on the layout's screen: where it is, its kind, its
    // leader, and a button to remove it.
    let tab = style_tab(&window);
    let rows = &tab
        .sections
        .iter()
        .find(|s| s.title == "Tab stops")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 2, "a stop and Add");
    let ControlKind::Length { value, .. } = &rows[0].controls[0].kind else {
        panic!("a length");
    };
    assert_eq!(value.unwrap().value, 100 * MILLIMETRE);
    assert_eq!(rows[0].controls[3].command, "layout-stop-remove");
    // Its leader is drawn on the page.
    let dots = window.view().items.len();
    with(
        &mut window,
        "layout-stop-remove",
        vec![id(layout), Arg::Int(100 * MILLIMETRE)],
    );
    assert!(window.view().items.len() < dots);
    command(&mut window, "undo");
    assert_eq!(stops(&window).leader, Some('.'));
    command(&mut window, "undo");
    command(&mut window, "undo");
    command(&mut window, "undo");
    assert_eq!(stops(&window).position, interval);
}

#[test]
fn a_lists_numbering_changed_renumbers_its_items() {
    let mut window = window("app-screens-list", "app:Neutral", "One");
    command(&mut window, "list-numbers");
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    window.update(Event::Text("Two".into()));
    let markers = |window: &Window| -> Vec<Option<String>> {
        let editor = window.editor();
        story::flow(editor.document(), editor.sheet(), 100 * MILLIMETRE)
            .entries
            .iter()
            .map(|e| e.place.marker.clone())
            .collect()
    };
    let m = |s: &str| Some(s.to_string());
    assert_eq!(markers(&window), [m("1."), m("2.")]);
    let numbers = named(&window, StyleKind::List, "Numbers");
    // The Text tab says which list definition the caret's list takes, its
    // label a link to the definition's screen in the Style tab.
    command(&mut window, "toggle-inspector");
    let text_tab = window.editor().inspector().tabs[0].clone();
    let list = text_tab.sections[0]
        .rows
        .iter()
        .find(|r| labelled(r, "List"))
        .expect("the list's row");
    assert_eq!(
        list.controls[1].kind,
        ControlKind::Note {
            text: "Numbers".to_string()
        }
    );
    let link = list.controls[0].clone();
    window.update(Event::CommandWith(link.command, link.args));
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::List, Some(numbers))
    );
    let tabs = window.access();
    assert!(
        tabs.nodes.iter().any(|(_, n)| n.role() == Role::Tab
            && n.label() == Some("Style")
            && n.is_selected() == Some(true)),
        "the Style tab shows it"
    );
    let level = |what: &str, value: Arg| vec![id(numbers), Arg::Int(1), text(what), value];
    with(
        &mut window,
        "list-level",
        level(keys::LEVEL_MARKER, text("a")),
    );
    with(
        &mut window,
        "list-level",
        level(keys::LEVEL_READS, text("(1)")),
    );
    with(
        &mut window,
        "list-level",
        level(keys::LEVEL_START, text("3")),
    );
    assert_eq!(markers(&window), [m("(c)"), m("(d)")]);
    // What can't be is refused, and said.
    with(
        &mut window,
        "list-level",
        level(keys::LEVEL_READS, text("()")),
    );
    assert!(message(&window).contains("Put a 1"), "{}", message(&window));
    with(
        &mut window,
        "list-level",
        level(keys::LEVEL_START, text("x")),
    );
    assert_eq!(markers(&window), [m("(c)"), m("(d)")]);
    // The level's screen: its marker, how its number reads and where it
    // starts, and a button to let each go back to the list's.
    let marker = control_in(&window, "Level 1", "Marker");
    let ControlKind::Choice {
        choices, selected, ..
    } = &marker.kind
    else {
        panic!("a choice of markers");
    };
    assert_eq!(choices[selected.unwrap()].label, "a, b, c");
    let tab = style_tab(&window);
    let reads = &tab.sections[2].rows[1];
    assert_eq!(reads.label, "Reads");
    assert_eq!(reads.controls[1].command, "style-reset");
    command(&mut window, "undo");
    assert_eq!(markers(&window), [m("(a)"), m("(b)")]);
}

#[test]
fn technicals_heading_numbers_are_a_list_definition() {
    let mut window = window("app-screens-headings", "app:Technical", "Scope");
    command(&mut window, "style-heading-1");
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    window.update(Event::Text("Terms".into()));
    command(&mut window, "style-heading-2");
    let markers = |window: &Window| -> Vec<Option<String>> {
        let editor = window.editor();
        story::flow(editor.document(), editor.sheet(), 100 * MILLIMETRE)
            .entries
            .iter()
            .map(|e| e.place.marker.clone())
            .collect()
    };
    let m = |s: &str| Some(s.to_string());
    assert_eq!(markers(&window), [m("1"), m("1.1")]);
    let headings = named(&window, StyleKind::List, "Headings");
    with(&mut window, "style-kind", vec![text("list")]);
    with(&mut window, "style-show", vec![id(headings)]);
    assert_eq!(
        control_in(&window, "List", "For").kind,
        ControlKind::Note {
            text: "Numbered headings".to_string()
        }
    );
    with(
        &mut window,
        "list-level",
        vec![
            id(headings),
            Arg::Int(1),
            text(keys::LEVEL_MARKER),
            text("I"),
        ],
    );
    assert_eq!(markers(&window), [m("I"), m("I.1")]);
}

#[test]
fn a_character_styles_values_are_set_and_let_go() {
    let mut window = window("app-screens-character", "app:Neutral", "Stressed");
    let emphasis = named(&window, StyleKind::Character, "Emphasis");
    with(&mut window, "style-kind", vec![text("character")]);
    with(&mut window, "style-show", vec![id(emphasis)]);
    let values = |window: &Window| {
        window
            .editor()
            .sheet()
            .character_style(emphasis)
            .unwrap()
            .values
            .clone()
    };
    // Not set is a value of its own: Emphasis sets italic, not weight.
    let weight = control_in(&window, "Text", "Weight");
    let ControlKind::Choice { selected, .. } = weight.kind else {
        panic!("a choice");
    };
    assert_eq!(selected, Some(0), "not set");
    let tab = style_tab(&window);
    let text_rows = &tab
        .sections
        .iter()
        .find(|s| s.title == "Text")
        .unwrap()
        .rows;
    let slant = text_rows.iter().find(|r| r.label == "Slant").unwrap();
    assert_eq!(
        slant.controls[1].kind,
        ControlKind::Note {
            text: "this style".to_string()
        }
    );
    let weight_row = text_rows.iter().find(|r| r.label == "Weight").unwrap();
    assert_eq!(
        weight_row.controls[1].kind,
        ControlKind::Note {
            text: "not set".to_string()
        }
    );
    // Set, and let go.
    with(
        &mut window,
        "style-option",
        vec![id(emphasis), text(keys::BOLD), text("on")],
    );
    assert_eq!(values(&window).bold, Some(true));
    with(
        &mut window,
        "style-set",
        vec![id(emphasis), text(keys::SIZE), Arg::Int(14 * POINT)],
    );
    assert_eq!(values(&window).size, Some(14 * POINT));
    with(
        &mut window,
        "style-reset",
        vec![id(emphasis), text(keys::SIZE)],
    );
    assert_eq!(values(&window).size, None);
    with(
        &mut window,
        "style-option",
        vec![id(emphasis), text(keys::ITALIC), text("unset")],
    );
    assert_eq!(values(&window).italic, None);
    // A colour from the sheet's, then not set again.
    let link = named(&window, StyleKind::Colour, "Link");
    with(
        &mut window,
        "style-set",
        vec![id(emphasis), text(keys::COLOR), id(link)],
    );
    assert_eq!(values(&window).color, Some((0x19, 0x50, 0xaa)));
    with(
        &mut window,
        "style-set",
        vec![id(emphasis), text(keys::COLOR), Arg::Int(0)],
    );
    assert_eq!(values(&window).color, None);
    for _ in 0..6 {
        command(&mut window, "undo");
    }
    assert_eq!(values(&window).italic, Some(true));
    assert_eq!(values(&window).bold, None);
}

#[test]
fn styles_and_parts_are_made_renamed_and_deleted() {
    let mut window = window("app-screens-new", "app:Neutral", "Made");
    let ink = named(&window, StyleKind::Colour, "Ink");
    // New, from the colour shown: a copy of it, shown, to rename.
    with(&mut window, "style-show", vec![id(ink)]);
    with(&mut window, "style-new", vec![text("colour"), id(ink)]);
    let new = named(&window, StyleKind::Colour, "New Colour");
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Colour, Some(new))
    );
    assert_eq!(window.editor().sheet().part(new).unwrap().value, "#000000");
    // Renamed, one edit.
    with(&mut window, "style-rename", vec![id(new), text("Sea")]);
    assert_eq!(named(&window, StyleKind::Colour, "Sea"), new);
    // A name another colour has is refused.
    with(&mut window, "style-rename", vec![id(new), text("Ink")]);
    assert_eq!(named(&window, StyleKind::Colour, "Sea"), new);
    // Deleted, and the screen shows another colour.
    with(&mut window, "style-delete", vec![id(new)]);
    assert!(window.editor().document().style(new).is_none());
    assert_eq!(window.editor().style_screen().0, StyleKind::Colour);
    command(&mut window, "undo");
    assert!(
        message(&window).contains("Undid Delete Sea"),
        "{}",
        message(&window)
    );
    assert!(window.editor().document().style(new).is_some());
    command(&mut window, "undo");
    command(&mut window, "undo");
    assert!(
        message(&window).contains("Undid New Colour"),
        "{}",
        message(&window)
    );
    assert!(window.editor().document().style(new).is_none());
    // New paragraph and character styles are based on the one shown.
    let body = named(&window, StyleKind::Paragraph, "Body");
    with(&mut window, "style-new", vec![text("paragraph"), id(body)]);
    let made = named(&window, StyleKind::Paragraph, "New Style");
    assert_eq!(paragraph_style(&window, "New Style").base, Some(body));
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Paragraph, Some(made))
    );
    // A style another is based on can't be deleted: the screen says why,
    // and Delete can't be used.
    let heading = named(&window, StyleKind::Paragraph, "Heading");
    with(&mut window, "style-show", vec![id(heading)]);
    let tab = style_tab(&window);
    let head = &tab.sections[0];
    let delete = head
        .rows
        .iter()
        .flat_map(|r| &r.controls)
        .find(|c| c.command == "style-delete")
        .unwrap();
    assert!(!delete.enabled);
    assert!(head.rows.iter().flat_map(|r| &r.controls).any(|c| matches!(
        &c.kind,
        ControlKind::Note { text } if text.contains("Heading 1 is based on it")
    )));
    with(&mut window, "style-delete", vec![id(heading)]);
    assert!(window.editor().document().style(heading).is_some());
    assert!(
        message(&window).contains("can't be deleted: Heading 1 is based on it"),
        "{}",
        message(&window)
    );
}

#[test]
fn the_style_tab_follows_the_caret_until_one_is_chosen_and_parts_are_followed() {
    let mut window = window("app-screens-follow", "app:Neutral", "Followed");
    command(&mut window, "toggle-inspector");
    let body = named(&window, StyleKind::Paragraph, "Body");
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Paragraph, Some(body))
    );
    command(&mut window, "style-heading-2");
    let h2 = named(&window, StyleKind::Paragraph, "Heading 2");
    assert_eq!(window.editor().style_screen().1, Some(h2), "it follows");
    // Body's face followed to the Text face; and back.
    command(&mut window, "style-body");
    let face = control_in(&window, "Text", "Face");
    assert_eq!(face.command, "style-set");
    let tab = style_tab(&window);
    let follow = tab
        .sections
        .iter()
        .find(|s| s.title == "Text")
        .unwrap()
        .rows[0]
        .controls
        .iter()
        .find(|c| c.command == "style-follow")
        .expect("an arrow to the face")
        .clone();
    window.update(Event::CommandWith(follow.command, follow.args));
    let text_face = named(&window, StyleKind::Face, "Text");
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Face, Some(text_face))
    );
    let back = style_tab(&window).sections[0].rows[0].controls[0].clone();
    assert_eq!(back.command, "style-back");
    assert_eq!(
        back.kind,
        ControlKind::Button {
            label: "Back to Body".to_string(),
            icon: None
        }
    );
    window.update(Event::CommandWith(back.command, back.args));
    assert_eq!(
        window.editor().style_screen(),
        (StyleKind::Paragraph, Some(body))
    );
    // Chosen, it stays as the caret moves; opening the tab again goes
    // back to the caret's.
    let ink = named(&window, StyleKind::Colour, "Ink");
    with(&mut window, "style-show", vec![id(ink)]);
    command(&mut window, "style-heading-2");
    assert_eq!(window.editor().style_screen().1, Some(ink));
    window.update(Event::Press {
        x: 1130.0,
        y: 66.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
    assert_eq!(window.editor().style_screen().1, Some(h2));
}

/// The bounds of the screen readers' node for the inspector's field
/// labelled `label`, in logical pixels, and whether it's hidden.
fn field(window: &Window, label: &str) -> Option<(f64, f64, bool)> {
    let tree = window.access();
    tree.nodes.iter().find_map(|(_, node)| {
        (node.role() == Role::TextInput && node.label() == Some(label)).then(|| {
            let b = node.bounds().expect("bounds");
            (b.x0, b.y0, node.is_hidden())
        })
    })
}

/// Presses and lets go at (`x`, `y`).
fn click(window: &mut Window, x: f64, y: f64) {
    window.update(Event::Press {
        x,
        y,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
}

#[test]
fn the_inspector_scrolls_for_the_wheel_the_bar_the_keyboard_and_screen_readers() {
    let mut window = window("app-screens-scroll", "app:Neutral", "Scrolled");
    // A window 600 pixels high: the inspector's body ends at 574.
    window.update(Event::Resize {
        width: 1280.0,
        height: 600.0,
    });
    let bottom = 574.0;
    command(&mut window, "toggle-inspector");
    // The Style tab, taller than the window.
    click(&mut window, 1130.0, 66.0);
    let (_, y, hidden) = field(&window, "First line").expect("First line's field");
    assert!(hidden && y > bottom, "below the panel at first: {y}");
    // A row of buttons is each button's: New and Delete, not "New:
    // Delete"; a link in a label's place labels its row's value.
    let tree = window.access();
    let labelled = |role: Role, label: &str| {
        tree.nodes
            .iter()
            .any(|(_, n)| n.role() == role && n.label() == Some(label))
    };
    assert!(labelled(Role::Button, "New") && labelled(Role::Button, "Delete"));
    assert!(labelled(Role::ComboBox, "Face"), "the face's value");
    let (_, name_y, _) = field(&window, "Name").expect("the name");
    // The wheel over the inspector scrolls it, not the page; what changes
    // is the inspector's.
    let before = window.view();
    window.update(Event::Move {
        x: 1100.0,
        y: 400.0,
    });
    window.update(Event::Scroll(200.0));
    let after = window.view();
    let (_, moved, hidden) = field(&window, "First line").unwrap();
    assert_eq!(moved, y - 200.0);
    assert!(!hidden);
    assert_eq!(field(&window, "Name").unwrap().1, name_y - 200.0);
    for r in scene::changed(&before, &after) {
        assert!(r.x >= 980 * PIXEL - PIXEL, "{r:?} outside the inspector");
    }
    // Scrolled past its end, it stops there.
    window.update(Event::Scroll(10_000.0));
    let (_, end, _) = field(&window, "First line").unwrap();
    window.update(Event::Scroll(100.0));
    assert_eq!(field(&window, "First line").unwrap().1, end);
    // A press lands where the field is drawn now, and typing there sets
    // the style's first-line indent.
    let (x_now, y_now, _) = field(&window, "First line").unwrap();
    window.update(Event::Press {
        x: x_now + 20.0,
        y: y_now + 10.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
    window.update(Event::Text("1 cm".into()));
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        paragraph_style(&window, "Body").indents.first,
        10 * MILLIMETRE
    );
    // The bar: a press on its track above the thumb pages up, and the
    // thumb dragged to the top scrolls to the top.
    click(&mut window, 1273.0, 100.0);
    let (_, paged, _) = field(&window, "First line").unwrap();
    assert!(paged > y_now, "paged up: {paged} from {y_now}");
    let thumb = (100..574)
        .map(f64::from)
        .find(|&ty| {
            window.update(Event::Move { x: 1273.0, y: ty });
            // The thumb lights under the pointer.
            thumb_lit(&window)
        })
        .expect("the thumb");
    window.update(Event::Press {
        x: 1273.0,
        y: thumb,
        shift: false,
        count: 1,
    });
    window.update(Event::Move {
        x: 1273.0,
        y: thumb - 600.0,
    });
    window.update(Event::Release);
    assert_eq!(field(&window, "Name").unwrap().1, name_y);
    // Tab moves the keyboard field to field, each scrolled into view.
    click(&mut window, 1150.0, name_y + 10.0);
    for _ in 0..5 {
        window.update(Event::Key {
            key: Key::Tab,
            modifiers: Modifiers::default(),
        });
    }
    let (_, y, hidden) = field(&window, "First line").unwrap();
    assert!(
        !hidden && y + 28.0 <= bottom,
        "Tab reached First line and showed it: {y}"
    );
    // The field being typed in is First line's: typing goes there.
    window.update(Event::Text("2 cm".into()));
    window.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        paragraph_style(&window, "Body").indents.first,
        20 * MILLIMETRE
    );
}

/// A recorded session that starts with the Style tab showing a style
/// chosen, scrolled, plays back as it was done: where a click lands in the
/// inspector depends on both, so the session's start keeps them.
#[test]
fn a_recorded_session_takes_up_the_style_tab_as_it_was() {
    use app::recorder::{Recorder, Spec};
    use std::time::Duration;
    let recorded = res("app-screens-recorded");
    let recorder = Recorder::told(&Spec {
        minutes: 1,
        checkpoint_seconds: 1,
    });
    let mut window = Window::create(recorded).expect("a document");
    window.record(recorder.clone());
    let tick = Duration::from_secs(7);
    let step = |window: &mut Window, event: Event| {
        recorder.tick(tick);
        window.update(event);
    };
    step(
        &mut window,
        Event::Resize {
            width: 1280.0,
            height: 600.0,
        },
    );
    step(&mut window, Event::Text("Recorded".into()));
    step(&mut window, Event::Command("toggle-inspector".into()));
    for event in [
        Event::Press {
            x: 1130.0,
            y: 66.0,
            shift: false,
            count: 1,
        },
        Event::Release,
    ] {
        step(&mut window, event);
    }
    let h1 = named(&window, StyleKind::Paragraph, "Heading 1");
    step(
        &mut window,
        Event::CommandWith("style-show".into(), vec![id(h1)]),
    );
    step(
        &mut window,
        Event::Move {
            x: 1100.0,
            y: 300.0,
        },
    );
    step(&mut window, Event::Scroll(200.0));
    // A minute and more of the pointer moving about, so the session kept
    // starts after all that.
    for k in 0..12 {
        step(
            &mut window,
            Event::Move {
                x: 1100.0 + f64::from(k),
                y: 300.0,
            },
        );
    }
    // Heading 1's first-line indent, where it is now.
    let (x, y, hidden) = field(&window, "First line").expect("its field");
    assert!(!hidden);
    for event in [
        Event::Press {
            x: x + 20.0,
            y: y + 10.0,
            shift: false,
            count: 1,
        },
        Event::Release,
        Event::Text("1 cm".into()),
        Event::Key {
            key: Key::Enter,
            modifiers: Modifiers::default(),
        },
    ] {
        step(&mut window, event);
    }
    assert_eq!(
        paragraph_style(&window, "Heading 1").indents.first,
        10 * MILLIMETRE
    );
    let session = recorder.session().expect("a session");
    let start = session.start.as_ref().expect("a start");
    let chrome = start.chrome.expect("the chrome as it was");
    assert_eq!(chrome.style, Some(h1.0));
    assert!(chrome.scroll > 0);
    let written = app::session::write(&session).expect("it writes");
    let read = app::session::read(&written).expect("it reads back");
    assert!(read.events.len() < 20, "{} steps kept", read.events.len());
    let played = app::session::play(res("app-screens-recorded-played"), &read).expect("it plays");
    assert_eq!(
        played.editor().document().dump(),
        window.editor().document().dump()
    );
}

/// Whether the inspector's scroll bar's thumb is drawn lit, as it is
/// under the pointer: in the look's secondary colour, at the panel's edge.
fn thumb_lit(window: &Window) -> bool {
    let lit = resources().light.secondary;
    window.view().items.iter().any(|item| match item {
        scene::Item::Fill { color, path } if *color == lit => path
            .0
            .iter()
            .any(|s| matches!(s, scene::Segment::MoveTo(p) if p.x > 1268 * PIXEL)),
        _ => false,
    })
}
