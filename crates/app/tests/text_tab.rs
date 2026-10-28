//! The Text tab finishes what stage 9's follow-up began (M2's exit, review
//! 7fnfdhb5): a paragraph's own tab stops take leaders, as a tab layout's
//! do, each shown on the ruler; and a list chooses its list definition,
//! which survives pasting and stylesheet updates. Each is one edit Undo
//! takes back, and a Markdown file asks before what it can't hold.

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
    // Beside the stop, a list of its leaders: none yet.
    let row = text_section(&window, "Tab stops").rows[0].clone();
    let leader = row
        .controls
        .iter()
        .find(|c| c.command == "tab-stop-leader")
        .expect("a list of leaders")
        .clone();
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
    let (_, chosen) = choices(
        &text_section(&window, "Tab stops").rows[0]
            .controls
            .iter()
            .find(|c| c.command == "tab-stop-leader")
            .unwrap()
            .clone(),
    );
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
