//! The Text tab finishes what stage 9's follow-up began (M2's exit, review
//! 7fnfdhb5): a paragraph's own tab stops take leaders, as a tab layout's
//! do, each shown on the ruler; and a list chooses its list definition,
//! which survives pasting and stylesheet updates. Each is one edit Undo
//! takes back, and a Markdown file asks before what it can't hold. And
//! since M2's exit review (p7yct3zf), a paragraph's stops show as a tab
//! layout's do, a section each, for people and screen readers alike.

use std::sync::{Arc, OnceLock};

use app::{Editor, Effect, Event, Resources, Window};
use chrome::{Arg, ControlKind, Key, Modifiers};
use doc::{ObjectId, StyleKind, Value};
use part::Part;
use story::keys;
use text::units::MILLIMETRE;

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
    res.own = Arc::new(app::OwnSheets::load(res.documents.join("stylesheets")));
    Arc::new(res)
}

fn with(window: &mut Window, id: &str, args: Vec<Arg>) -> Vec<Effect> {
    window.update(Event::CommandWith(id.to_string(), args))
}

fn command(window: &mut Window, id: &str) -> Vec<Effect> {
    window.update(Event::Command(id.to_string()))
}

fn key(window: &mut Window, key: Key) {
    window.update(Event::Key {
        key,
        modifiers: Modifiers::default(),
    });
}

fn id(id: ObjectId) -> Arg {
    Arg::Int(id.0 as i64)
}

fn text(t: &str) -> Arg {
    Arg::Text(t.to_string())
}

/// A window on `editor`, 1280 by 800, its inspector open.
fn window_on(res: Arc<Resources>, editor: Editor) -> Window {
    let mut window = Window::with(res, editor);
    window.update(Event::Resize {
        width: 1280.0,
        height: 800.0,
    });
    command(&mut window, "toggle-inspector");
    window
}

/// The ID of the document's style or part of `kind` named `name`.
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

/// The tab stops of the caret's paragraph, as its text lays out with them.
fn stops(window: &Window) -> Vec<text::TabStop> {
    let editor = window.editor();
    let (_, focus) = editor.selection();
    let p = editor.document().paragraph(focus.paragraph).unwrap();
    story::paragraph_tabs(p, editor.sheet()).stops
}

/// The Text tab's section titled `title`.
fn text_section(window: &Window, title: &str) -> chrome::Section {
    window.editor().inspector().tabs[0]
        .sections
        .iter()
        .find(|s| s.title == title)
        .unwrap_or_else(|| panic!("no section {title}"))
        .clone()
}

/// The Text tab's control that runs `command` in the section of its `n`th
/// tab stop, "Tab stop `n`".
fn stop_control(window: &Window, n: usize, command: &str) -> chrome::Control {
    text_section(window, &format!("Tab stop {n}"))
        .rows
        .iter()
        .flat_map(|r| &r.controls)
        .find(|c| c.command == command)
        .unwrap_or_else(|| panic!("no {command} in tab stop {n}"))
        .clone()
}

/// The choices of a list control, and the label of the one chosen.
fn choices(control: &chrome::Control) -> (Vec<String>, Option<String>) {
    let ControlKind::Choice {
        choices, selected, ..
    } = &control.kind
    else {
        panic!("a choice: {control:?}");
    };
    (
        choices.iter().map(|c| c.label.clone()).collect(),
        selected
            .and_then(|k| choices.get(k))
            .map(|c| c.label.clone()),
    )
}

#[test]
fn a_paragraphs_own_stops_take_leaders_from_the_text_tab() {
    let res = res("app-text-tab-leaders");
    let editor = Editor::create(res.clone()).expect("a document");
    let mut window = window_on(res, editor);
    window.update(Event::Text("Beans\t3.40".to_string()));
    let at = 120 * MILLIMETRE;
    with(
        &mut window,
        "tab-stop-add",
        vec![Arg::Int(at), text("decimal:.")],
    );
    // In the stop's section, a list of its leaders: none yet.
    let leader = stop_control(&window, 1, "tab-stop-leader");
    assert_eq!(leader.args, vec![Arg::Int(at)]);
    let (labels, chosen) = choices(&leader);
    assert_eq!(labels, ["No leader", "Dots", "Dashes", "Line"]);
    assert_eq!(chosen.as_deref(), Some("No leader"));
    // Dots, as the list gives them: the stop has them, the ruler shows
    // them, and the line's tab is filled with them.
    let ControlKind::Choice { choices: all, .. } = &leader.kind else {
        unreachable!()
    };
    let args = leader.with(all[1].value.clone());
    with(&mut window, "tab-stop-leader", args);
    assert_eq!(stops(&window)[0].leader, Some('.'));
    let ruler = Part::ruler(window.editor()).expect("a ruler");
    assert_eq!(ruler.stops[0].leader, Some('.'));
    let (_, chosen) = choices(&stop_control(&window, 1, "tab-stop-leader"));
    assert_eq!(chosen.as_deref(), Some("Dots"));
    // A leader is glyphs without text, no part of the line's.
    let leaders = |window: &Window| {
        let mut stack: Vec<scene::Item> = window.editor().pages().pages[0].items.clone();
        let mut found = 0;
        while let Some(item) = stack.pop() {
            match item {
                scene::Item::Group(g) => stack.extend(g.items),
                scene::Item::Glyphs(g) if g.text.is_empty() && g.glyphs.len() > 3 => found += 1,
                _ => {}
            }
        }
        found
    };
    assert_eq!(leaders(&window), 1, "the tab is filled with dots");
    // One edit, which Undo takes back; the stop stays.
    command(&mut window, "undo");
    assert_eq!(window.editor().message(), "Undid Leader");
    assert_eq!(stops(&window)[0].leader, None);
    assert_eq!(stops(&window)[0].position, at);
    command(&mut window, "redo");
    // None takes it away; a leader that can't be one is none.
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text("_")],
    );
    assert_eq!(stops(&window)[0].leader, Some('_'));
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text(" ")],
    );
    assert_eq!(stops(&window)[0].leader, None);
    // A stop moved keeps its leader.
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text("-")],
    );
    with(
        &mut window,
        "tab-stop-move",
        vec![Arg::Int(at), Arg::Int(at + 10 * MILLIMETRE)],
    );
    assert_eq!(stops(&window)[0].leader, Some('-'));
    // Where no stop is, nothing changes: Undo takes back the move.
    let before = window.editor().document().dump();
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text(".")],
    );
    assert_eq!(window.editor().document().dump(), before);
    command(&mut window, "undo");
    assert_eq!(window.editor().message(), "Undid Tab Stop");
}

/// Markdown has no tab stops, so no leaders: a Markdown file asks before a
/// stop comes, as for anything it can't hold, and made an `.xw` file, its
/// stop takes a leader.
#[test]
fn a_markdown_file_asks_before_a_stop_and_its_leader() {
    let res = res("app-text-tab-leaders-md");
    let path = res.documents.join("Beans.md");
    std::fs::create_dir_all(&res.documents).unwrap();
    std::fs::write(&path, "Beans and peas\n").unwrap();
    let editor = Editor::open(res.clone(), &path).expect("it opens");
    let mut window = window_on(res, editor);
    let at = 50 * MILLIMETRE;
    assert!(stops(&window).is_empty());
    // Where no stop is, a leader changes nothing and asks nothing.
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text(".")],
    );
    assert!(window.editor().panel().is_none());
    with(&mut window, "tab-stop-add", vec![Arg::Int(at), text("end")]);
    let (title, _, _) = window.editor().panel().expect("it asks");
    assert!(title.contains("tab stops"), "{title}");
    assert!(stops(&window).is_empty(), "not made until it's answered");
    // Made an xWorks document: the stop is made, and takes a leader.
    key(&mut window, Key::Enter);
    assert!(!window.editor().is_markdown());
    assert_eq!(stops(&window).len(), 1);
    with(
        &mut window,
        "tab-stop-leader",
        vec![Arg::Int(at), text(".")],
    );
    assert_eq!(stops(&window)[0].leader, Some('.'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Beans and peas\n");
}

/// The sections inspector tab `tab` shows of a set of tab stops: "Tab
/// stop 1" and on, or "Tab stops" where there's none. Each control's
/// command is named without `prefix`, what it edits, so "place" for
/// `tab-stop-place` and `layout-stop-place`, and runs without `before`,
/// what comes ahead of a stop's place: what the Text tab and a tab
/// layout's screen share.
fn stop_sections(window: &Window, tab: &str, prefix: &str, before: &[Arg]) -> Vec<chrome::Section> {
    let mut sections: Vec<chrome::Section> = window
        .editor()
        .inspector()
        .tabs
        .into_iter()
        .find(|t| t.title == tab)
        .unwrap_or_else(|| panic!("no {tab} tab"))
        .sections
        .into_iter()
        .filter(|s| s.title.starts_with("Tab stop"))
        .collect();
    let controls = sections
        .iter_mut()
        .flat_map(|s| &mut s.rows)
        .flat_map(|r| &mut r.controls);
    for control in controls {
        if let Some(what) = control.command.strip_prefix(prefix) {
            control.command = what.to_string();
        }
        if let Some(rest) = control.args.strip_prefix(before) {
            control.args = rest.to_vec();
        }
    }
    sections
}

/// The Text tab shows a paragraph's tab stops as the Style tab shows a tab
/// layout's (project owner, 2026-10-29, review p7yct3zf): for the same
/// stops, the same sections of the same rows, each control showing the
/// same, but for the commands they run, on the paragraph or the layout.
#[test]
fn a_paragraphs_stops_show_as_a_tab_layouts_do() {
    let res = res("app-text-tab-as-layout");
    let editor = Editor::create(res.clone()).expect("a document");
    let mut window = window_on(res, editor);
    window.update(Event::Text("Beans\tBroad\t3.40".to_string()));
    with(&mut window, "style-new", vec![text("tabs"), Arg::Int(0)]);
    let layout = named(&window, StyleKind::Tabs, "New Tab Layout");
    let same = |window: &Window| {
        let text_tab = stop_sections(window, "Text", "tab-stop-", &[]);
        let screen = stop_sections(window, "Style", "layout-stop-", &[id(layout)]);
        assert_eq!(text_tab, screen);
        text_tab
    };
    // None yet: each says so, over the button that adds one.
    let none = same(&window);
    assert_eq!(none.len(), 1);
    assert_eq!(none[0].title, "Tab stops");
    assert_eq!(none[0].rows.len(), 2);
    assert_eq!(none[0].rows[1].controls[0].command, "add");
    // The same three stops, each added and set by its own commands: a
    // start stop with dashes, an end stop with a line, and one on a colon,
    // which isn't a kind offered, with a leader that isn't one either. The
    // layout's is added at the next default stop and placed.
    let layout_stops = |window: &Window| {
        let part = window.editor().sheet().part(layout).expect("the layout");
        story::parse_tabs(&part.value).expect("its stops")
    };
    for (at, kind, leader) in [
        (40, "start", "-"),
        (110, "end", "_"),
        (150, "decimal::", "="),
    ] {
        let at = at * MILLIMETRE;
        with(&mut window, "tab-stop-add", vec![Arg::Int(at), text(kind)]);
        with(
            &mut window,
            "tab-stop-leader",
            vec![Arg::Int(at), text(leader)],
        );
        with(&mut window, "layout-stop-add", vec![id(layout)]);
        let added = layout_stops(&window).last().expect("a stop").position;
        let on = |rest: Vec<Arg>| [vec![id(layout), Arg::Int(added)], rest].concat();
        with(&mut window, "layout-stop-place", on(vec![Arg::Int(at)]));
        let at_ = |rest: Vec<Arg>| [vec![id(layout), Arg::Int(at)], rest].concat();
        with(&mut window, "layout-stop-kind", at_(vec![text(kind)]));
        with(&mut window, "layout-stop-leader", at_(vec![text(leader)]));
    }
    assert_eq!(stops(&window), layout_stops(&window));
    let three = same(&window);
    let titles: Vec<&str> = three.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Tab stop 1", "Tab stop 2", "Tab stop 3"]);
    for (k, section) in three.iter().enumerate() {
        let labels: Vec<&str> = section.rows.iter().map(|r| r.label.as_str()).collect();
        let expected: &[&str] = if k == 2 {
            &["Position", "Alignment", "Leader", ""]
        } else {
            &["Position", "Alignment", "Leader"]
        };
        assert_eq!(labels, expected);
    }
    // Each reads as it is: where, from the ruler's zero, how it aligns,
    // and its leader.
    let shown: Vec<(String, String, String)> = three
        .iter()
        .map(|s| {
            let ControlKind::Length { value, plain, .. } = &s.rows[0].controls[0].kind else {
                panic!("a length: {:?}", s.rows[0]);
            };
            assert!(!plain, "a field");
            assert_eq!(s.rows[0].controls[1].command, "remove");
            let (_, kind) = choices(&s.rows[1].controls[0]);
            let (_, leader) = choices(&s.rows[2].controls[0]);
            (
                value.expect("a place").to_string(),
                kind.expect("a kind"),
                leader.expect("a leader"),
            )
        })
        .collect();
    let s = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        shown,
        [
            s("4 cm", "Left", "Dashes"),
            s("11 cm", "Right", "Line"),
            s("15 cm", "Decimal (:)", "Leader ="),
        ]
    );
}

/// Screen readers get each stop's section as a group, "Tab stop 1" and
/// on, of its fields and lists, each labelled as its row is, Position,
/// Alignment and Leader, with its value; and its button to remove it.
#[test]
fn screen_readers_get_each_stops_fields_by_their_labels() {
    use part::accesskit::Role;
    let res = res("app-text-tab-access");
    let editor = Editor::create(res.clone()).expect("a document");
    let mut window = window_on(res, editor);
    window.update(Event::Text("Beans\t3.40".to_string()));
    for (at, kind, leader) in [(40, "start", "-"), (110, "end", ".")] {
        let at = at * MILLIMETRE;
        with(&mut window, "tab-stop-add", vec![Arg::Int(at), text(kind)]);
        with(
            &mut window,
            "tab-stop-leader",
            vec![Arg::Int(at), text(leader)],
        );
    }
    let tree = window.access();
    let node =
        |id: part::accesskit::NodeId| &tree.nodes.iter().find(|(n, _)| *n == id).expect("a node").1;
    let members = |title: &str| -> Vec<(Role, String, Option<String>)> {
        let (_, group) = tree
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::Group && n.label() == Some(title))
            .unwrap_or_else(|| panic!("no group {title}"));
        group
            .children()
            .iter()
            .map(|&id| {
                let n = node(id);
                assert!(!n.is_hidden(), "{title} is in sight");
                (
                    n.role(),
                    n.label().unwrap_or_default().to_string(),
                    n.value().map(str::to_string),
                )
            })
            .collect()
    };
    for (title, place, kind, leader) in [
        ("Tab stop 1", "4 cm", "Left", "Dashes"),
        ("Tab stop 2", "11 cm", "Right", "Dots"),
    ] {
        let members = members(title);
        let fields: Vec<(Role, &str, Option<&str>)> = members
            .iter()
            .filter(|(role, ..)| *role != Role::Button)
            .map(|(role, label, value)| (*role, label.as_str(), value.as_deref()))
            .collect();
        assert_eq!(
            fields,
            [
                (Role::TextInput, "Position", Some(place)),
                (Role::ComboBox, "Alignment", Some(kind)),
                (Role::ComboBox, "Leader", Some(leader)),
            ],
            "{title}"
        );
        assert!(
            members
                .iter()
                .any(|(role, label, _)| *role == Role::Button && label.contains("Remove")),
            "{title}: {members:?}"
        );
    }
    // Add, under the last.
    assert!(
        members("Tab stop 2")
            .iter()
            .any(|(role, label, _)| *role == Role::Button && label == "Add a tab stop")
    );
}

/// Each list item's marker, in reading order.
fn markers(window: &Window) -> Vec<String> {
    let editor = window.editor();
    story::flow(editor.document(), editor.sheet(), 100 * MILLIMETRE)
        .entries
        .iter()
        .filter_map(|e| e.place.marker.clone())
        .collect()
}

/// The first list's block in the document.
fn first_list(window: &Window) -> ObjectId {
    let document = window.editor().document();
    document
        .subtree(document.root())
        .into_iter()
        .find(|&b| document.block(b).is_some_and(|b| b.kind == doc::Kind::List))
        .expect("a list")
}

/// The list's own property `key`.
fn list_prop(window: &Window, key: &str) -> Option<Value> {
    let list = first_list(window);
    window
        .editor()
        .document()
        .block(list)
        .unwrap()
        .props
        .get(key)
        .cloned()
}

/// A window on a new document holding a numbered list of three, and a
/// list definition of its own, Roman, its first level numbered i, ii.
fn roman_list(folder: &str) -> (Window, Arc<Resources>) {
    let res = res(folder);
    let editor = Editor::create(res.clone()).expect("a document");
    let mut window = window_on(res.clone(), editor);
    window.update(Event::Text("One".to_string()));
    command(&mut window, "list-numbers");
    for item in ["Two", "Three"] {
        key(&mut window, Key::Enter);
        window.update(Event::Text(item.to_string()));
    }
    let numbers = named(&window, StyleKind::List, "Numbers");
    with(&mut window, "style-new", vec![text("list"), id(numbers)]);
    let made = named(&window, StyleKind::List, "New List");
    with(&mut window, "style-rename", vec![id(made), text("Roman")]);
    with(
        &mut window,
        "list-level",
        vec![id(made), Arg::Int(1), text(keys::LEVEL_MARKER), text("i")],
    );
    (window, res)
}

/// The Text tab's List row's list of definitions.
fn list_control(window: &Window) -> chrome::Control {
    let row = text_section(window, "Paragraph")
        .rows
        .into_iter()
        .find(|r| {
            r.controls.first().is_some_and(
                |c| matches!(&c.kind, ControlKind::Button { label, .. } if label == "List"),
            )
        })
        .expect("the List row");
    row.controls[1].clone()
}

#[test]
fn a_list_chooses_its_list_definition() {
    let (mut window, _) = roman_list("app-text-tab-list");
    let roman = named(&window, StyleKind::List, "Roman");
    assert_eq!(markers(&window), ["1.", "2.", "3."]);
    // The List row lists the document's definitions, the list's chosen.
    let control = list_control(&window);
    assert_eq!(control.command, "list-definition");
    let (labels, chosen) = choices(&control);
    assert_eq!(labels, ["Bullets", "Numbers", "Roman"]);
    assert_eq!(chosen.as_deref(), Some("Numbers"));
    // Roman: the list names it and renumbers, in one edit.
    let ControlKind::Choice { choices: all, .. } = &control.kind else {
        unreachable!()
    };
    with(
        &mut window,
        "list-definition",
        control.with(all[2].value.clone()),
    );
    assert_eq!(list_prop(&window, keys::LIST), Some(Value::Ref(roman)));
    assert_eq!(markers(&window), ["i.", "ii.", "iii."]);
    assert_eq!(choices(&list_control(&window)).1.as_deref(), Some("Roman"));
    command(&mut window, "undo");
    assert_eq!(window.editor().message(), "Undid Roman");
    assert_eq!(list_prop(&window, keys::LIST), None);
    assert_eq!(markers(&window), ["1.", "2.", "3."]);
    command(&mut window, "redo");
    assert_eq!(markers(&window), ["i.", "ii.", "iii."]);
    // Numbers, the one numbered lists take, is named by none.
    with(&mut window, "list-definition", vec![text("Numbers")]);
    assert_eq!(list_prop(&window, keys::LIST), None);
    assert_eq!(markers(&window), ["1.", "2.", "3."]);
    // A definition of the other kind makes the list that kind, and says so.
    with(&mut window, "list-definition", vec![text("Bullets")]);
    assert_eq!(list_prop(&window, keys::ORDERED), None);
    assert_eq!(list_prop(&window, keys::LIST), None);
    assert_eq!(markers(&window), ["•", "•", "•"]);
    assert_eq!(
        window.editor().message(),
        "Bulleted the list, as Bullets bullets its items"
    );
    with(&mut window, "list-definition", vec![text("Roman")]);
    assert_eq!(list_prop(&window, keys::ORDERED), Some(Value::Bool(true)));
    assert_eq!(list_prop(&window, keys::LIST), Some(Value::Ref(roman)));
    assert_eq!(markers(&window), ["i.", "ii.", "iii."]);
    assert_eq!(
        window.editor().message(),
        "Numbered the list, as Roman numbers its items"
    );
    // That was one edit too.
    command(&mut window, "undo");
    assert_eq!(markers(&window), ["•", "•", "•"]);
}

/// A list's chosen definition comes along when it's pasted into another
/// document, matched there by name as styles are, and stays through
/// reopening.
#[test]
fn a_lists_definition_survives_pasting_and_reopening() {
    let (mut window, res) = roman_list("app-text-tab-list-paste");
    with(&mut window, "list-definition", vec![text("Roman")]);
    command(&mut window, "select-all");
    let effects = command(&mut window, "copy");
    let Some(Effect::Copy(copied)) = effects.first().cloned() else {
        panic!("copied: {effects:?}");
    };
    let other = Editor::create(res.clone()).expect("another document");
    let mut other = window_on(res.clone(), other);
    other.update(Event::Pasted(copied.clone()));
    let roman = named(&other, StyleKind::List, "Roman");
    assert_eq!(list_prop(&other, keys::LIST), Some(Value::Ref(roman)));
    assert_eq!(markers(&other), ["i.", "ii.", "iii."]);
    // Into a Markdown file, which can't hold the definition nor the list
    // naming it: it asks, the file as it was.
    let path = res.documents.join("List.md");
    std::fs::write(&path, "Before\n").unwrap();
    let md = Editor::open(res.clone(), &path).expect("it opens");
    let mut md = window_on(res.clone(), md);
    key(&mut md, Key::End);
    md.update(Event::Pasted(copied));
    let (title, _, _) = md.editor().panel().expect("it asks");
    assert!(title.starts_with("Markdown can't hold"), "{title}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Before\n");
    // The document pasted into reopens with the list as it was.
    let path = other.editor().path().to_path_buf();
    other.close();
    drop(other);
    let reopened = Editor::open(res.clone(), &path).expect("it reopens");
    let reopened = window_on(res, reopened);
    assert_eq!(markers(&reopened), ["i.", "ii.", "iii."]);
}

/// A list naming a definition of its document's stylesheet keeps it when
/// the document takes a newer version of the sheet, three-way.
#[test]
fn a_lists_definition_survives_a_stylesheet_update() {
    let res = res("app-text-tab-list-update");
    let technical = res
        .kept
        .iter()
        .find(|s| s.name == "Technical" && s.version == 1)
        .expect("Technical 1 is kept")
        .clone();
    let document = story::document(&technical, &[("Body", vec![("Step", doc::Props::new())])]);
    let path = res.documents.join("Steps.xw");
    drop(store::Store::create(&path, &document, "story").expect("written"));
    let editor = Editor::open(res.clone(), &path).expect("it opens");
    let mut window = window_on(res, editor);
    key(&mut window, Key::End);
    command(&mut window, "list-numbers");
    key(&mut window, Key::Enter);
    window.update(Event::Text("Another".to_string()));
    let headings = named(&window, StyleKind::List, "Headings");
    with(&mut window, "list-definition", vec![id(headings)]);
    assert_eq!(list_prop(&window, keys::LIST), Some(Value::Ref(headings)));
    let chosen = markers(&window);
    assert_ne!(chosen, ["1.", "2."], "numbered as Headings numbers");
    assert!(window.editor().state("update-stylesheet").enabled);
    command(&mut window, "update-stylesheet");
    assert!(window.editor().panel().is_some(), "what changed");
    key(&mut window, Key::Enter);
    assert_eq!(
        window
            .editor()
            .sheet()
            .origin
            .as_ref()
            .map(|(n, v)| (n.as_str(), *v)),
        Some(("Technical", 2))
    );
    assert_eq!(list_prop(&window, keys::LIST), Some(Value::Ref(headings)));
    assert_eq!(markers(&window), chosen);
}
