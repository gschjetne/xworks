//! The window beyond what sessions show: what screen readers get, the
//! documents opened lately, and the editor behind the canvas contract.

use std::sync::{Arc, OnceLock};

use app::{Effect, Event, Resources, Window};
use chrome::{Arg, Key, Modifiers};
use part::accesskit::Role;

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

fn window(folder: &str) -> Window {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir(folder).expect("a folder");
    let mut window = Window::create(Arc::new(res)).expect("a document");
    window.update(Event::Resize {
        width: 900.0,
        height: 700.0,
    });
    window
}

fn key(key: Key, shift: bool) -> Event {
    Event::Key {
        key,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
    }
}

/// Screen readers get the text's lines as AccessKit's text runs, each with
/// its graphemes and where its words start, inside the paragraphs, and the
/// selection in them: not the document as one string.
#[test]
fn screen_readers_get_lines_words_and_the_selection() {
    let mut window = window("app-access");
    let long = "Screen readers read this paragraph line by line and word by word, \
                as it is laid out on the page, so it runs on long enough to wrap.";
    window.update(Event::Text(long.to_string()));
    window.update(key(Key::Enter, false));
    window.update(Event::Text("Second one.".to_string()));
    for _ in 0..4 {
        window.update(key(Key::Left, true));
    }
    let tree = window.access();
    let node = |id| {
        tree.nodes
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, node)| node)
            .expect("a node")
    };
    let (text_id, text) = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::MultilineTextInput)
        .expect("the document's text");
    assert_eq!(tree.focus, *text_id);
    let paragraphs = text.children();
    assert_eq!(paragraphs.len(), 2);
    let first: Vec<_> = node(paragraphs[0]).children().to_vec();
    assert!(first.len() >= 2, "the first paragraph wraps");
    let mut read = String::new();
    for &run in &first {
        let run = node(run);
        assert_eq!(run.role(), Role::TextRun);
        let value = run.value().expect("its text");
        let lengths: usize = run
            .character_lengths()
            .iter()
            .map(|&l| usize::from(l))
            .sum();
        assert_eq!(lengths, value.len(), "graphemes cover {value:?}");
        assert!(!run.word_starts().is_empty(), "words start in {value:?}");
        assert!(run.bounds().is_some());
        read.push_str(value);
    }
    assert_eq!(read, format!("{long}\n"));
    // The selection: the last four characters of the second paragraph.
    let selection = text.text_selection().expect("a selection");
    let second = node(paragraphs[1]).children()[0];
    assert_eq!(selection.anchor.node, second);
    assert_eq!(selection.focus.node, second);
    assert_eq!(selection.anchor.character_index, 11);
    assert_eq!(selection.focus.character_index, 7);
}

/// File, Open Recent offers the documents opened lately, and opens one.
#[test]
fn recent_documents_open_again() {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-recent").expect("a folder");
    res.recent = Arc::new(app::Recent::default());
    let res = Arc::new(res);
    let first = Window::create(res.clone()).expect("a document");
    let path = first.editor().path().to_path_buf();
    let mut second = Window::create(res.clone()).expect("another");
    assert!(second.state("open-recent").enabled);
    let effects = second.update(Event::CommandWith(
        "open-recent".to_string(),
        vec![Arg::Text(path.to_string_lossy().into_owned())],
    ));
    assert_eq!(effects, vec![Effect::Open(Some(path.clone()))]);
    assert_eq!(res.recent.paths()[1], path);
}

/// What falls in the document's area reaches the editor through the
/// canvas contract, moved into its own coordinates; what's in the chrome
/// doesn't.
#[test]
fn the_editor_gets_what_falls_in_its_area() {
    let mut window = window("app-part");
    window.update(Event::Text("Clicked".to_string()));
    // On the ruler, which is chrome: the caret stays at the end.
    window.update(Event::Press {
        x: 100.0,
        y: 50.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
    window.update(Event::Text("!".to_string()));
    // Left of the first line, on the page: the caret goes to its start.
    window.update(Event::Press {
        x: 100.0,
        y: 191.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
    window.update(Event::Text(">".to_string()));
    let editor = window.editor();
    let p = editor.document().paragraphs()[0];
    assert_eq!(editor.document().paragraph(p).unwrap().text, ">Clicked!");
    // The pointer over the text is a beam; the window says so once.
    let effects = window.update(Event::Move { x: 300.0, y: 191.0 });
    assert!(effects.contains(&Effect::Pointer(app::PointerShape::Text)));
    let effects = window.update(Event::Move { x: 301.0, y: 191.0 });
    assert!(!effects.iter().any(|e| matches!(e, Effect::Pointer(_))));
}

/// The canvas contract's promise (`part`): the same input gives the same
/// frames, the same accessibility tree and the same requests. Two editors
/// of new documents, in folders of their own, are given the same input
/// through the contract alone.
#[test]
fn the_same_input_gives_the_same_frames_and_tree() {
    use part::{Input, Part, Surface};
    let editor = |folder: &str| {
        let mut res = resources().clone();
        res.documents = testkit::scratch_dir(folder).expect("a folder");
        app::Editor::create(Arc::new(res)).expect("a document")
    };
    let (mut a, mut b) = (editor("app-part-same-a"), editor("app-part-same-b"));
    let inputs = [
        Input::Text("The same input, given twice.".to_string()),
        Input::Key {
            key: Key::Enter,
            modifiers: Modifiers::default(),
        },
        Input::Text("A second paragraph".to_string()),
        Input::Press {
            x: 120 * text::units::PIXEL,
            y: 60 * text::units::PIXEL,
            shift: false,
            count: 2,
        },
        Input::Release,
        Input::Command("bold".to_string()),
        Input::Scroll(40 * text::units::PIXEL),
        Input::Command("copy".to_string()),
    ];
    let look = |editor: &app::Editor| -> String {
        let surroundings = part::Look {
            desk: scene::Color::rgb(240, 240, 238),
            page: scene::Color::WHITE,
            page_ink: scene::Color::BLACK,
            ink: scene::Color::BLACK,
            selection: scene::Color::rgb(180, 200, 255),
            formatting: scene::Color::rgb(250, 200, 100),
            shadow: scene::Color::rgb(200, 200, 200),
            line: scene::Color::rgb(160, 160, 160),
            tint: scene::Color::rgb(230, 230, 230),
            accent: scene::Color::rgb(30, 90, 200),
            caret: true,
        };
        format!(
            "{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
            editor.frame(&surroundings),
            editor.access(100),
            editor.caret(),
            editor.ruler(),
            editor.status()
        )
    };
    for editor in [&mut a, &mut b] {
        editor.resize(Surface {
            width: 800 * text::units::PIXEL,
            height: 600 * text::units::PIXEL,
        });
    }
    for input in inputs {
        let requests = a.input(input.clone());
        assert_eq!(requests, b.input(input.clone()), "after {input:?}");
        assert_eq!(look(&a), look(&b), "after {input:?}");
    }
}

/// File, Versions lists the named versions over the document, and one
/// chosen from the keyboard asks for a window on it.
#[test]
fn versions_are_chosen_from_a_list_over_the_document() {
    let mut window = window("app-versions");
    window.update(Event::Text("Versioned".to_string()));
    window.update(Event::Command("save-version".to_string()));
    window.update(Event::Command("versions".to_string()));
    let effects = window.update(key(Key::Enter, false));
    let path = window.editor().path().to_path_buf();
    assert!(
        matches!(&effects[..], [Effect::OpenVersion(p, _)] if *p == path),
        "{effects:?}"
    );
}

/// Renaming an open document moves its file, and the lock that keeps it
/// from being open twice goes with it (review a23vvez8): the database is
/// closed first, so nothing of it is left beside the old name.
#[test]
fn a_document_renamed_keeps_its_lock() {
    let mut window = window("app-rename");
    window.update(Event::Text("Locked".to_string()));
    let before = window.editor().path().to_path_buf();
    assert!(store::lock_path(&before).exists(), "locked while open");
    window.update(Event::Command("rename".to_string()));
    window.update(Event::Text("Renamed".to_string()));
    window.update(key(Key::Enter, false));
    let after = window.editor().path().to_path_buf();
    assert_eq!(after.file_stem().unwrap(), "Renamed");
    assert!(!before.exists() && !store::lock_path(&before).exists());
    for side in ["-wal", "-shm"] {
        let mut name = before.file_name().unwrap().to_os_string();
        name.push(side);
        assert!(!before.with_file_name(name).exists(), "{side} left behind");
    }
    assert!(store::lock_path(&after).exists(), "locked where it went");
    let other = store::Store::open(&after).expect("it opens");
    assert!(
        matches!(other.store.read_only(), Some(store::ReadOnly::InUse(_))),
        "another opens it read-only"
    );
    // Typing goes on into the file where it is.
    drop(other);
    window.update(Event::Text(" on".to_string()));
    let reopened = store::Store::open(&after).expect("it opens");
    let p = reopened.document.paragraphs()[0];
    assert_eq!(reopened.document.paragraph(p).unwrap().text, "Locked on");
}

/// The document's area and the chrome round it cover the whole window,
/// with nothing under them: in light and dark, pages and draft, the
/// inspector open and shut, and the ruler shown and hidden.
#[test]
fn the_window_is_covered_everywhere() {
    let mut window = window("app-covered");
    window.update(Event::Text("Covered".to_string()));
    let steps = [
        None,
        Some("toggle-inspector"),
        Some("view-draft"),
        Some("toggle-ruler"),
        Some("toggle-inspector"),
        Some("view-pages"),
    ];
    for dark in [false, true] {
        window.update(Event::Theme { dark });
        for step in steps {
            if let Some(id) = step {
                window.update(Event::Command(id.to_string()));
            }
            let image = render_cpu::render(&window.view(), 96.0, render_cpu::Level::Portable);
            // Opaque, but for a step or two of rounding where a
            // translucent colour, such as a shadow's, is laid over.
            let bare = image.pixels.chunks(4).position(|p| p[3] < 250);
            assert_eq!(
                bare, None,
                "dark {dark}, after {step:?}: a pixel isn't covered"
            );
        }
    }
}

/// The ruler is numbered from the text column's start, the left margin,
/// until the person chooses the paper's edge, beside the units in View,
/// Units (review 77nh3z47); the choice is every window's. In the draft,
/// which has no paper, the column is the page, so the two are one.
#[test]
fn the_ruler_counts_from_the_margin_or_the_paper_edge() {
    use part::Part;
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-ruler-from").expect("a folder");
    res.settings = Arc::new(app::Settings::default());
    let res = Arc::new(res);
    let mut window = Window::create(res.clone()).expect("a document");
    window.update(Event::Resize {
        width: 1280.0,
        height: 800.0,
    });
    window.update(Event::Command("view-pages".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    assert!(ruler.page_start < ruler.start, "{ruler:?}");
    assert_eq!(ruler.zero, ruler.start);
    assert!(window.state("ruler-from-margin").checked);
    assert!(!window.state("ruler-from-edge").checked);

    window.update(Event::Command("ruler-from-edge".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    assert_eq!(ruler.zero, ruler.page_start);
    assert!(window.state("ruler-from-edge").checked);
    assert!(!window.state("ruler-from-margin").checked);
    let other = Window::create(res.clone()).expect("another");
    assert!(other.state("ruler-from-edge").checked);

    window.update(Event::Command("view-draft".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    assert_eq!((ruler.zero, ruler.page_start), (ruler.start, ruler.start));

    window.update(Event::Command("view-pages".to_string()));
    window.update(Event::Command("ruler-from-margin".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    assert_eq!(ruler.zero, ruler.start);
    assert_eq!(res.settings.changes(), 2);
}

/// The inspector's tab stops read as the ruler does, from its zero, the
/// margin or the paper's edge, in a list too, and a place typed there is
/// where the stop goes on the ruler (review 77nh3z47). A stop still
/// belongs to its paragraph's place, which a list moves in.
#[test]
fn the_inspectors_tab_stops_read_as_the_ruler_does() {
    use chrome::ControlKind;
    use part::Part;
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-stops-from").expect("a folder");
    res.settings = Arc::new(app::Settings::default());
    let res = Arc::new(res);
    let mut window = Window::create(res.clone()).expect("a document");
    window.update(Event::Resize {
        width: 1280.0,
        height: 800.0,
    });
    window.update(Event::Command("view-pages".to_string()));
    let cm = chrome::Unit::Centimetre.units();
    window.update(Event::CommandWith(
        "tab-stop-add".to_string(),
        vec![Arg::Int(7 * cm / 2)],
    ));
    // The stops the inspector shows: what each runs with, and its place.
    let shown = |window: &Window| -> Vec<(Vec<Arg>, i64)> {
        let mut out = Vec::new();
        for tab in window.editor().inspector().tabs {
            for row in tab.sections.into_iter().flat_map(|s| s.rows) {
                for control in row.controls {
                    if control.command == "tab-stop-place"
                        && let ControlKind::Length { value: Some(v), .. } = control.kind
                    {
                        out.push((control.args.clone(), v.value));
                    }
                }
            }
        }
        out
    };
    let stored = |window: &Window| {
        let p = window.editor().document().paragraphs()[0];
        window
            .editor()
            .document()
            .paragraph(p)
            .unwrap()
            .props
            .get("tabs")
            .cloned()
    };
    // From the margin, as stored.
    assert_eq!(shown(&window), [(vec![Arg::Int(7 * cm / 2)], 7 * cm / 2)]);

    // From the paper's edge, the margin further: where the ruler has it.
    window.update(Event::Command("ruler-from-edge".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    let margin = ruler.start - ruler.page_start;
    assert!(margin > 0);
    assert_eq!(
        shown(&window),
        [(vec![Arg::Int(7 * cm / 2)], 7 * cm / 2 + margin)]
    );
    // Placed at 8 cm on the ruler, it's stored 8 cm less the margin in.
    window.update(Event::CommandWith(
        "tab-stop-place".to_string(),
        vec![Arg::Int(7 * cm / 2), Arg::Int(8 * cm)],
    ));
    assert_eq!(
        stored(&window),
        Some(doc::Value::Text(format!("start {}", 8 * cm - margin)))
    );
    assert_eq!(shown(&window)[0].1, 8 * cm);

    // In a list, from the margin, the stop reads from the margin, not
    // from the item's place, and stays where it is as the list moves it.
    window.update(Event::Command("ruler-from-margin".to_string()));
    window.update(Event::Command("list-bullets".to_string()));
    let ruler = window.editor().ruler().expect("a ruler");
    let inset = ruler.origin - ruler.zero;
    assert!(inset > 0, "{ruler:?}");
    assert_eq!(shown(&window)[0].1, 8 * cm - margin + inset);
    assert_eq!(
        stored(&window),
        Some(doc::Value::Text(format!("start {}", 8 * cm - margin)))
    );
}

/// The ruler and the inspector measure the page and the paragraph in the
/// unit the person chose from View, Units (review ydqhfds0), but for the
/// space above and below a paragraph, which is set in points as type is,
/// and type sizes (review 77nh3z47); the choice is every window's. A
/// length left as it shows changes nothing, though it reads back a hair
/// off.
#[test]
fn lengths_show_in_the_unit_chosen() {
    use chrome::{ControlKind, Length, Unit};
    use part::Part;
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-units").expect("a folder");
    res.settings = Arc::new(app::Settings::default());
    let res = Arc::new(res);
    let mut window = Window::create(res.clone()).expect("a document");
    window.update(Event::Resize {
        width: 1280.0,
        height: 800.0,
    });
    window.update(Event::CommandWith(
        "tab-stop-add".to_string(),
        vec![Arg::Int(1_260_000)],
    ));
    // Every length the inspector shows, by its command.
    let lengths = |window: &Window| -> Vec<(String, Length)> {
        let mut out = Vec::new();
        for tab in window.editor().inspector().tabs {
            for row in tab.sections.into_iter().flat_map(|s| s.rows) {
                for control in row.controls {
                    if let ControlKind::Length { value: Some(v), .. } = control.kind {
                        out.push((control.command.clone(), v));
                    }
                }
            }
        }
        out
    };
    let units = |window: &Window| -> Vec<(String, Unit)> {
        lengths(window)
            .into_iter()
            .map(|(c, l)| (c, l.unit))
            .collect()
    };
    let expect = |unit| {
        [
            ("space-before", Unit::Point),
            ("space-after", Unit::Point),
            ("size", Unit::Point),
            ("tab-stop-place", unit),
            // The Style tab's: the paragraph style's size and spacing, in
            // points, and its indents (M2's stage 9).
            ("style-set", Unit::Point),
            ("style-set", Unit::Point),
            ("style-set", Unit::Point),
            ("style-set", unit),
            ("style-set", unit),
            ("page-margin", unit),
            ("page-margin", unit),
            ("page-margin", unit),
            ("page-margin", unit),
        ]
        .map(|(c, u)| (c.to_string(), u))
        .to_vec()
    };
    assert!(window.state("units-centimetres").checked);
    assert_eq!(units(&window), expect(Unit::Centimetre));
    assert_eq!(window.editor().ruler().unwrap().unit, Unit::Centimetre);

    window.update(Event::Command("units-picas".to_string()));
    assert_eq!(units(&window), expect(Unit::Pica));
    assert_eq!(window.editor().ruler().unwrap().unit, Unit::Pica);
    let stop = lengths(&window)
        .into_iter()
        .find(|(c, _)| c == "tab-stop-place")
        .unwrap()
        .1;
    assert_eq!(stop.to_string(), "8p3.21");
    // Another window has the person's choice too.
    let other = Window::create(res.clone()).expect("another");
    assert!(other.state("units-picas").checked);
    assert!(!other.state("units-centimetres").checked);
    assert_eq!(res.settings.changes(), 1);

    // The stop's length, opened and left as it shows: it stays at 3.5 cm.
    window.update(Event::Command("toggle-inspector".to_string()));
    let tabs_of = |window: &Window| {
        let p = window.editor().document().paragraphs()[0];
        window
            .editor()
            .document()
            .paragraph(p)
            .unwrap()
            .props
            .get("tabs")
            .cloned()
    };
    let before = tabs_of(&window);
    window.update(Event::Press {
        x: 1010.0,
        y: 452.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
    window.update(key(Key::Enter, false));
    assert_eq!(tabs_of(&window), before);
}

/// A blink costs only the caret: the window draws again only what changed
/// (scene::redraw), and a caret gone or back changes its own few pixels,
/// the document's, and a field's being typed in, such as the palette's.
#[test]
fn a_blink_changes_only_the_caret() {
    let mut window = window("app-blink");
    window.update(Event::Text("Blinking".to_string()));
    assert!(window.caret_shown(), "headless, the caret shows");
    let (x, top, bottom) = {
        use part::Part;
        window.editor().caret().expect("a caret")
    };
    let shown = window.view();
    window.show_caret(false);
    let gone = window.view();
    let changed = scene::changed(&shown, &gone);
    assert_eq!(changed.len(), 1, "{changed:?}");
    let caret = changed[0];
    // The caret, a pixel and a half wide, where the editor says it is,
    // below the title bar and the ruler.
    assert_eq!(caret.width, 3 * text::units::PIXEL / 2);
    assert_eq!(caret.height, bottom - top);
    assert_eq!(caret.x, x - text::units::PIXEL / 2);
    assert!(caret.y > top, "{caret:?} for {top}");
    let pixels: u64 = scene::redraw(&shown, &gone, 96.0, 900, 700)
        .iter()
        .map(|r| u64::from(r.width) * u64::from(r.height))
        .sum();
    // A few pixels of the window's 630 000: the caret's, and a pixel
    // round it for anti-aliasing.
    assert!(pixels < 200, "{pixels} pixels drawn again");

    // With the palette open, its field's caret blinks with the document's.
    window.show_caret(true);
    window.update(Event::Command("command-palette".to_string()));
    let shown = window.view();
    window.show_caret(false);
    assert_eq!(scene::changed(&shown, &window.view()).len(), 2);
}
