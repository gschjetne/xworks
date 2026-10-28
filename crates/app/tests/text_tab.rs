//! The Text tab finishes what stage 9's follow-up began (M2's exit, review
//! 7fnfdhb5): a paragraph's own tab stops take leaders, as a tab layout's
//! do, each shown on the ruler. Each is one edit Undo takes back, and a
//! Markdown file asks before what it can't hold.

use std::sync::{Arc, OnceLock};

use app::{Editor, Effect, Event, Resources, Window};
use chrome::{Arg, ControlKind, Key, Modifiers};
use part::Part;

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
