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
        let lengths: usize = run.character_lengths().iter().map(|&l| usize::from(l)).sum();
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
