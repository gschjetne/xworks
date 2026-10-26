//! Frames of the real chrome (M2's stage 5), as the approved mockups show
//! it (`mockups.rs`, review 3ce247c2): the sample document in a window
//! 1280 by 800 at 96 to the inch, on pages with the ruler and the
//! inspector, light and dark; as a draft, dark; and with the command
//! palette open. They're drawn by the app's own window, to compare with
//! the mockups by eye and by difference (`shot --diff`). And the top of
//! the window, its ruler in each unit the person can choose (review
//! ydqhfds0).

use std::path::PathBuf;
use std::sync::Arc;

use app::{Event, Window};
use chrome::{Arg, Key, Modifiers};
use scene::Page;

/// The window's size, in pixels.
const SIZE: (f64, f64) = (1280.0, 800.0);

/// A folder of its own for each window drawn: tests draw scenes in
/// several processes at once.
fn folder() -> PathBuf {
    static DRAWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = DRAWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    testkit::scratch_dir(&format!("shot-chrome-{}-{n}", std::process::id())).expect("a folder")
}

/// A window on the sample document (`spec/documents/m1-sample.ron`),
/// saved as `name`, at 1280 by 800, its caret in its first paragraph of
/// body text, which has the mockups' tab stops and is in English (UK).
fn sample(name: &str, dark: bool) -> Window {
    let root = testkit::workspace_root();
    let documents = folder();
    let res = Arc::new(app::Resources::load(&root, documents.clone(), false).expect("resources"));
    let paragraphs: Vec<(String, Vec<(String, doc::Props)>)> =
        ron::from_str(&crate::pages::spec("documents", "m1-sample")).expect("a document");
    let document = story::document(&crate::pages::neutral(), &paragraphs);
    let path = documents.join(format!("{name}.xw"));
    drop(store::Store::create(&path, &document, "story").expect("the file"));
    let mut window = Window::open(res, &path).expect("it opens");
    window.update(Event::Theme { dark });
    window.update(Event::Resize {
        width: SIZE.0,
        height: SIZE.1,
    });
    let key = |key| Event::Key {
        key,
        modifiers: Modifiers::default(),
    };
    window.update(key(Key::Down));
    window.update(key(Key::Home));
    window.update(Event::CommandWith(
        "language".to_string(),
        vec![Arg::Text("en-GB".to_string())],
    ));
    // The mockups' stops: 3.5 cm, start; 7.5, centred; 11.5, end; 13.75,
    // on the decimal point.
    for (cm, kind) in [
        (350, "start"),
        (750, "center"),
        (1150, "end"),
        (1375, "decimal:."),
    ] {
        window.update(Event::CommandWith(
            "tab-stop-add".to_string(),
            vec![Arg::Int(cm * 3600), Arg::Text(kind.to_string())],
        ));
    }
    window
}

/// The sample on pages, with the ruler and the inspector open.
fn pages(dark: bool) -> Page {
    let mut window = sample("A walking skeleton", dark);
    window.update(Event::Command("toggle-inspector".to_string()));
    window.view()
}

/// The sample on pages with the inspector's Style tab open (M2's stage
/// 9): the caret's paragraph's style, Body, each of its values and where
/// it comes from, and the stylesheet it was copied from.
pub(crate) fn style_tab() -> Page {
    let mut window = sample("A walking skeleton", false);
    window.update(Event::Command("toggle-inspector".to_string()));
    // The Style tab's title, the second of three across the inspector's
    // top.
    for event in [
        Event::Press {
            x: 1130.0,
            y: 66.0,
            shift: false,
            count: 1,
        },
        Event::Release,
    ] {
        window.update(event);
    }
    window.view()
}

/// The real chrome: a document on pages, with the ruler and the
/// inspector, as `mockup-pages` shows it.
pub(crate) fn pages_light() -> Page {
    pages(false)
}

/// The same, dark, as `mockup-pages-dark` shows it.
pub(crate) fn pages_dark() -> Page {
    pages(true)
}

/// The sample as a draft, dark, the inspector shut, as
/// `mockup-draft-dark` shows it.
pub(crate) fn draft_dark() -> Page {
    let mut window = sample("README", true);
    window.update(Event::Command("view-draft".to_string()));
    window.view()
}

/// The top of the window on the sample on pages, the inspector shut: the
/// title bar, the ruler as `commands` from View, Units have it, and the
/// top of the page, 1280 by 120.
fn ruler(commands: &[&str]) -> Page {
    let mut window = sample("A walking skeleton", false);
    for command in commands {
        window.update(Event::Command(command.to_string()));
    }
    let view = window.view();
    let mut top = Page::new(view.width, 120 * text::units::PIXEL);
    top.items = view.items;
    top
}

/// The ruler in centimetres.
pub(crate) fn ruler_centimetres() -> Page {
    ruler(&["units-centimetres"])
}

/// The ruler in inches.
pub(crate) fn ruler_inches() -> Page {
    ruler(&["units-inches"])
}

/// The ruler in picas.
pub(crate) fn ruler_picas() -> Page {
    ruler(&["units-picas"])
}

/// The ruler in centimetres, numbered from the paper's edge.
pub(crate) fn ruler_paper_edge() -> Page {
    ruler(&["units-centimetres", "ruler-from-edge"])
}

/// The command palette, open over the sample on pages, "head" typed, as
/// `mockup-palette` shows it.
pub(crate) fn palette() -> Page {
    let mut window = sample("A walking skeleton", false);
    window.update(Event::Command("command-palette".to_string()));
    window.update(Event::Text("head".to_string()));
    window.view()
}

/// A window on the sample Markdown file, `spec/documents/m2-markdown.md`,
/// with every block and mark Markdown holds, as it opens: in the draft
/// view (review frhewnk0), its picture linked beside it, at 1280 by 800.
fn markdown(dark: bool) -> Page {
    let root = testkit::workspace_root();
    let documents = folder();
    let res = Arc::new(app::Resources::load(&root, documents.clone(), false).expect("resources"));
    std::fs::create_dir_all(documents.join("images")).expect("a folder");
    std::fs::copy(
        root.join("crates/app/tests/pictures/gradient.jpg"),
        documents.join("images/gradient.jpg"),
    )
    .expect("the picture");
    let path = documents.join("The garden in spring.md");
    std::fs::copy(root.join("spec/documents/m2-markdown.md"), &path).expect("the file");
    let mut window = Window::open(res, &path).expect("it opens");
    window.update(Event::Theme { dark });
    window.update(Event::Resize {
        width: SIZE.0,
        height: SIZE.1,
    });
    window.view()
}

/// The sample Markdown file as it opens, light.
pub(crate) fn markdown_light() -> Page {
    markdown(false)
}

/// The same, dark.
pub(crate) fn markdown_dark() -> Page {
    markdown(true)
}

// ---- the Style tab's screens (stage 9's follow-up, review sjj5ak5a)

/// The ID of `window`'s document's style or part of `kind` named `name`.
fn part(window: &Window, kind: doc::StyleKind, name: &str) -> doc::ObjectId {
    let document = window.editor().document();
    document
        .styles()
        .iter()
        .copied()
        .find(|&s| {
            let style = document.style(s).expect("a style");
            style.kind == kind
                && style.props.get(story::keys::NAME) == Some(&doc::Value::Text(name.into()))
        })
        .unwrap_or_else(|| panic!("no {} {name}", kind.name()))
}

fn id(id: doc::ObjectId) -> Arg {
    Arg::Int(id.0 as i64)
}

fn run(window: &mut Window, command: &str, args: Vec<Arg>) {
    window.update(Event::CommandWith(command.to_string(), args));
}

/// Opens `window`'s inspector on its Style tab: its title, the second of
/// three across the inspector's top.
fn open_style_tab(window: &mut Window) {
    window.update(Event::Command("toggle-inspector".to_string()));
    for event in [
        Event::Press {
            x: 1130.0,
            y: 66.0,
            shift: false,
            count: 1,
        },
        Event::Release,
    ] {
        window.update(event);
    }
}

/// A character style's screen: the sample's Code, as it sets its face and
/// ligatures and leaves the rest not set.
pub(crate) fn style_character() -> Page {
    let mut window = sample("A walking skeleton", false);
    open_style_tab(&mut window);
    let code = part(&window, doc::StyleKind::Character, "Code");
    run(
        &mut window,
        "style-kind",
        vec![Arg::Text("character".into())],
    );
    run(&mut window, "style-show", vec![id(code)]);
    window.view()
}

/// The sample's Style tab dark, scrolled part of the way, its scroll bar
/// in the dark look's colours, the pointer over it.
pub(crate) fn style_dark() -> Page {
    let mut window = sample("A walking skeleton", true);
    open_style_tab(&mut window);
    window.update(Event::Move {
        x: 1100.0,
        y: 400.0,
    });
    window.update(Event::Scroll(150.0));
    window.update(Event::Move {
        x: 1273.0,
        y: 400.0,
    });
    window.view()
}

/// The sample with the Style tab showing its Heading face, its list of
/// families open, each family shown in itself.
pub(crate) fn style_face() -> Page {
    let mut window = sample("A walking skeleton", false);
    open_style_tab(&mut window);
    let heading = part(&window, doc::StyleKind::Face, "Heading");
    run(&mut window, "style-kind", vec![Arg::Text("face".into())]);
    run(&mut window, "style-show", vec![id(heading)]);
    // The Family field: the face's section's second row.
    for event in [
        Event::Press {
            x: 1160.0,
            y: 304.0,
            shift: false,
            count: 1,
        },
        Event::Release,
    ] {
        window.update(event);
    }
    window.view()
}

/// The sample with the Style tab showing its Link colour, its value as
/// `#rrggbb` beside its swatch, and what uses it.
pub(crate) fn style_colour() -> Page {
    let mut window = sample("A walking skeleton", false);
    open_style_tab(&mut window);
    let link = part(&window, doc::StyleKind::Colour, "Link");
    run(&mut window, "style-kind", vec![Arg::Text("colour".into())]);
    run(&mut window, "style-show", vec![id(link)]);
    window.view()
}

/// A window on a new document, 1280 by 800, its text typed as `lines`,
/// a line a paragraph.
fn new_document(name: &str, lines: &[&str]) -> Window {
    let documents = folder();
    let root = testkit::workspace_root();
    let res = Arc::new(app::Resources::load(&root, documents.clone(), false).expect("resources"));
    let mut window = Window::create(res).expect("a document");
    let _ = name;
    window.update(Event::Resize {
        width: SIZE.0,
        height: SIZE.1,
    });
    for (k, line) in lines.iter().enumerate() {
        if k > 0 {
            window.update(Event::Key {
                key: Key::Enter,
                modifiers: Modifiers::default(),
            });
        }
        window.update(Event::Text(line.to_string()));
    }
    window
}

/// A table of contents whose page numbers sit at a tab layout's stop at
/// the column's end, after dots: the layout's screen, its stop's place,
/// alignment and leader, and Body taking it.
pub(crate) fn style_tabs() -> Page {
    let mut window = new_document(
        "Contents",
        &[
            "Contents",
            "Introduction\t1",
            "Layout in fixed-point units\t3",
            "Text in every script\t6",
            "Pictures and drawing\t11",
            "Styles and stylesheets\t17",
        ],
    );
    window.update(Event::Key {
        key: Key::Home,
        modifiers: Modifiers {
            primary: true,
            ..Modifiers::default()
        },
    });
    window.update(Event::Command("style-heading-1".to_string()));
    open_style_tab(&mut window);
    run(
        &mut window,
        "style-new",
        vec![Arg::Text("tabs".into()), Arg::Int(0)],
    );
    let layout = part(&window, doc::StyleKind::Tabs, "New Tab Layout");
    run(
        &mut window,
        "style-rename",
        vec![id(layout), Arg::Text("Contents".into())],
    );
    run(&mut window, "layout-stop-add", vec![id(layout)]);
    // The default stops are every 12.5 mm: the first, moved to the
    // column's end, 160 mm, right-aligned after dots.
    let first = 125 * text::units::MILLIMETRE / 10;
    let end = 160 * text::units::MILLIMETRE;
    run(
        &mut window,
        "layout-stop-place",
        vec![id(layout), Arg::Int(first), Arg::Int(end)],
    );
    run(
        &mut window,
        "layout-stop-kind",
        vec![id(layout), Arg::Int(end), Arg::Text("end".into())],
    );
    run(
        &mut window,
        "layout-stop-leader",
        vec![id(layout), Arg::Int(end), Arg::Text(".".into())],
    );
    let body = part(&window, doc::StyleKind::Paragraph, "Body");
    run(
        &mut window,
        "style-set",
        vec![id(body), Arg::Text("tabs".into()), id(layout)],
    );
    window.view()
}

/// A numbered list three deep, its second level lettered in brackets and
/// its third in small Roman numerals, with the Numbers list definition's
/// screen.
pub(crate) fn style_list() -> Page {
    let mut window = new_document("Steps", &["Steps"]);
    window.update(Event::Command("style-heading-1".to_string()));
    let key = |key| Event::Key {
        key,
        modifiers: Modifiers::default(),
    };
    window.update(key(Key::Enter));
    window.update(Event::Text("Lay out the paragraph".into()));
    window.update(Event::Command("list-numbers".to_string()));
    for (tab, text) in [
        (false, "Shape its text"),
        (true, "Break it into lines"),
        (false, "Align each line"),
        (true, "Justified lines stretch at their spaces"),
        (false, "Others sit at their start"),
    ] {
        window.update(key(Key::Enter));
        if tab {
            window.update(key(Key::Tab));
        }
        window.update(Event::Text(text.into()));
    }
    open_style_tab(&mut window);
    let numbers = part(&window, doc::StyleKind::List, "Numbers");
    run(&mut window, "style-kind", vec![Arg::Text("list".into())]);
    run(&mut window, "style-show", vec![id(numbers)]);
    let level = |k: i64, what: &str, value: &str| {
        vec![
            id(numbers),
            Arg::Int(k),
            Arg::Text(what.into()),
            Arg::Text(value.into()),
        ]
    };
    run(&mut window, "list-level", level(2, "marker", "a"));
    run(&mut window, "list-level", level(2, "reads", "(1)"));
    run(&mut window, "list-level", level(3, "marker", "i"));
    window.view()
}

/// The sample's Style tab scrolled to its end: the paragraph style's
/// last values, and the stylesheet's, under the tabs, which stay.
pub(crate) fn style_scrolled() -> Page {
    let mut window = sample("A walking skeleton", false);
    open_style_tab(&mut window);
    window.update(Event::Move {
        x: 1100.0,
        y: 400.0,
    });
    window.update(Event::Scroll(10_000.0));
    window.view()
}
