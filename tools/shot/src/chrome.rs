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
    let document = story::document(&paragraphs);
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
/// title bar, the ruler in the unit command `units` chooses, and the top
/// of the page, 1280 by 120.
fn ruler(units: &str) -> Page {
    let mut window = sample("A walking skeleton", false);
    window.update(Event::Command(units.to_string()));
    let view = window.view();
    let mut top = Page::new(view.width, 120 * text::units::PIXEL);
    top.items = view.items;
    top
}

/// The ruler in centimetres.
pub(crate) fn ruler_centimetres() -> Page {
    ruler("units-centimetres")
}

/// The ruler in inches.
pub(crate) fn ruler_inches() -> Page {
    ruler("units-inches")
}

/// The ruler in picas.
pub(crate) fn ruler_picas() -> Page {
    ruler("units-picas")
}

/// The command palette, open over the sample on pages, "head" typed, as
/// `mockup-palette` shows it.
pub(crate) fn palette() -> Page {
    let mut window = sample("A walking skeleton", false);
    window.update(Event::Command("command-palette".to_string()));
    window.update(Event::Text("head".to_string()));
    window.view()
}
