//! Lists that pop up scroll where they're taller than the room they have
//! (M2's exit, review 7fnfdhb5). The case is the list of families a face
//! is set in, every family xWorks has, on the Style tab's screen of a
//! face, in a short window: it opens on the family chosen, scrolls with
//! the wheel, its scroll bar and the keys, and a press, and screen
//! readers, find each family where it's drawn as scrolled.

use std::sync::{Arc, OnceLock};

use app::{Event, Resources, Window};
use chrome::{ControlKind, Key, Modifiers};
use part::Part;
use part::accesskit::Role;

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

/// A window on a new document, 1280 by `height`, its inspector open on
/// the Style tab's screen of its first face.
fn window(folder: &str, height: f64) -> Window {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir(folder).expect("a folder");
    let mut window = Window::create(Arc::new(res)).expect("a document");
    window.update(Event::Resize {
        width: 1280.0,
        height,
    });
    window.update(Event::Command("toggle-inspector".to_string()));
    window.update(Event::CommandWith(
        "style-kind".to_string(),
        vec![chrome::Arg::Text("face".to_string())],
    ));
    window
}

fn key(window: &mut Window, key: Key) {
    window.update(Event::Key {
        key,
        modifiers: Modifiers::default(),
    });
}

fn click(window: &mut Window, (x, y): (f64, f64)) {
    window.update(Event::Press {
        x,
        y,
        shift: false,
        count: 1,
    });
    window.update(Event::Release);
}

/// The middle of the screen readers' node of `role` labelled `label`.
fn centre_of(window: &Window, role: Role, label: &str) -> Option<(f64, f64)> {
    window.access().nodes.iter().find_map(|(_, n)| {
        (n.role() == role && n.label() == Some(label)).then(|| {
            let b = n.bounds().expect("bounds");
            ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0)
        })
    })
}

/// The open list as screen readers see it: its bounds, how far it's
/// scrolled, and each face's label, whether it shows, and its middle;
/// none if no list is open.
#[allow(clippy::type_complexity, reason = "what a test reads")]
fn list(window: &Window) -> Option<((f64, f64, f64, f64), f64, Vec<(String, bool, (f64, f64))>)> {
    let tree = window.access();
    let (_, listbox) = tree.nodes.iter().find(|(_, n)| n.role() == Role::ListBox)?;
    let b = listbox.bounds().expect("bounds");
    let options = listbox
        .children()
        .iter()
        .map(|id| {
            let (_, n) = tree.nodes.iter().find(|(i, _)| i == id).expect("a node");
            let r = n.bounds().expect("bounds");
            (
                n.label().unwrap_or_default().to_string(),
                !n.is_hidden(),
                ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0),
            )
        })
        .collect();
    Some((
        (b.x0, b.y0, b.x1, b.y1),
        listbox.scroll_y().unwrap_or(0.0),
        options,
    ))
}

/// The family the face's Family field says.
fn family(window: &Window) -> String {
    let inspector = window.editor().inspector();
    let row = inspector
        .tabs
        .iter()
        .find(|t| t.title == "Style")
        .expect("the Style tab")
        .sections
        .iter()
        .flat_map(|s| &s.rows)
        .find(|r| r.label == "Family")
        .expect("the Family row");
    let ControlKind::Choice {
        choices, selected, ..
    } = &row.controls[0].kind
    else {
        panic!("a list");
    };
    choices[selected.expect("one chosen")].label.clone()
}

#[test]
fn the_family_list_scrolls_in_a_short_window() {
    let mut window = window("app-popups-faces", 480.0);
    let field = centre_of(&window, Role::ComboBox, "Family").expect("the Family field");
    click(&mut window, field);
    let ((x0, y0, x1, y1), scroll, faces) = list(&window).expect("the list is open");
    // It fits the window, under or over its field, and doesn't show all.
    assert!(y0 >= 0.0 && y1 <= 480.0, "{y0} {y1}");
    assert!(y0 >= field.1 || y1 <= field.1, "not over its field");
    assert!(faces.len() > 10, "{}", faces.len());
    assert!(
        faces.iter().any(|(_, shows, _)| !shows),
        "some out of sight"
    );
    // It opens on the family chosen, in sight.
    let chosen = family(&window);
    let at = faces
        .iter()
        .position(|(f, _, _)| *f == chosen)
        .expect("chosen");
    assert!(faces[at].1, "{chosen} shows");
    assert!(scroll > 0.0, "scrolled to {chosen}, the {at}th");
    // The wheel over it scrolls it, not the inspector, and it stays open.
    window.update(Event::Move {
        x: (x0 + x1) / 2.0,
        y: (y0 + y1) / 2.0,
    });
    window.update(Event::Scroll(-10_000.0));
    let (_, top, faces) = list(&window).expect("still open");
    assert_eq!(top, 0.0);
    assert!(faces[0].1 && !faces[faces.len() - 1].1);
    // A press lands on the family drawn there, as scrolled.
    let (name, _, (fx, fy)) = faces[1].clone();
    // End lights the last, scrolled into view; Home the first.
    key(&mut window, Key::End);
    let (_, end, faces) = list(&window).unwrap();
    assert!(end > 0.0 && faces[faces.len() - 1].1);
    key(&mut window, Key::Home);
    let (_, home, faces) = list(&window).unwrap();
    assert_eq!(home, 0.0);
    assert!(faces[0].1);
    // Page Down lights a page on, Down one more, each in sight.
    key(&mut window, Key::PageDown);
    key(&mut window, Key::PageDown);
    key(&mut window, Key::PageDown);
    let (_, paged, _) = list(&window).unwrap();
    assert!(paged > 0.0, "paged down");
    key(&mut window, Key::Up);
    key(&mut window, Key::PageUp);
    // The scroll bar: its track pages, its thumb drags.
    window.update(Event::Scroll(-10_000.0));
    click(&mut window, (x1 - 7.0, y1 - 12.0));
    let (_, after, _) = list(&window).expect("a press on the bar keeps it open");
    assert!(after > 0.0, "paged by the track");
    window.update(Event::Scroll(-10_000.0));
    window.update(Event::Press {
        x: x1 - 7.0,
        y: y0 + 12.0,
        shift: false,
        count: 1,
    });
    window.update(Event::Move {
        x: x1 - 7.0,
        y: y1 + 400.0,
    });
    window.update(Event::Release);
    let (_, dragged, faces) = list(&window).unwrap();
    assert_eq!(dragged, end, "dragged to the end");
    assert!(faces[faces.len() - 1].1);
    // The wheel away from the list doesn't scroll it.
    window.update(Event::Scroll(-10_000.0));
    assert_eq!(list(&window).unwrap().1, end);
    // Back at the top, a press on the second family chooses it.
    window.update(Event::Move { x: fx, y: fy });
    window.update(Event::Scroll(-10_000.0));
    click(&mut window, (fx, fy));
    assert!(list(&window).is_none(), "chosen, it closes");
    assert_eq!(family(&window), name);
}

/// Enter chooses the family the keys lit, however far it was scrolled; a
/// tall window shows the whole list, unscrolled.
#[test]
fn the_keys_choose_what_they_light() {
    let mut window = window("app-popups-keys", 480.0);
    let field = centre_of(&window, Role::ComboBox, "Family").expect("the Family field");
    click(&mut window, field);
    let (_, _, faces) = list(&window).unwrap();
    key(&mut window, Key::End);
    key(&mut window, Key::Up);
    key(&mut window, Key::Enter);
    assert_eq!(family(&window), faces[faces.len() - 2].0);
    // Tall enough, nothing scrolls.
    let mut tall = self::window("app-popups-tall", 1400.0);
    let field = centre_of(&tall, Role::ComboBox, "Family").unwrap();
    click(&mut tall, field);
    let (_, scroll, faces) = list(&tall).unwrap();
    assert_eq!(scroll, 0.0);
    assert!(faces.iter().all(|(_, shows, _)| *shows));
    key(&mut tall, Key::Escape);
    assert!(list(&tall).is_none());
}
