//! The toolkit's chrome hits what it draws: menu titles, commands, and a
//! list's rows; disabled commands and separators hit nothing.

use std::sync::OnceLock;

use chrome::{CommandState, Commands};
use text::Fonts;
use text::units::PIXEL;
use toolkit::{List, MenuBar, MenuHit, Theme, ThemeSpec};

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

fn theme() -> Theme {
    let source = testkit::specs("themes")
        .expect("spec/themes")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("m1.ron")
        .1;
    Theme::from_spec(&ron::from_str::<ThemeSpec>(&source).expect("a theme"))
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
fn menus_hit_what_they_draw() {
    let theme = theme();
    let bar = commands().menu_bar(
        false,
        |id| CommandState {
            enabled: id != "redo",
            checked: id == "bold",
        },
        |_| Vec::new(),
    );
    let closed = MenuBar::new(fonts(), &theme, bar.clone(), 800 * PIXEL, None);
    let mid = theme.bar_height / 2;
    assert_eq!(closed.hit(&theme, 20 * PIXEL, mid), Some(MenuHit::Title(0)));
    assert_eq!(closed.hit(&theme, 790 * PIXEL, mid), None);
    assert_eq!(
        closed.hit(&theme, 20 * PIXEL, theme.bar_height + 5 * PIXEL),
        None
    );
    assert!(!closed.draw(fonts(), &theme, None).is_empty());

    // The Edit menu, open: Undo is its first row, Redo, disabled, its second.
    let open = MenuBar::new(fonts(), &theme, bar, 800 * PIXEL, Some(1));
    assert_eq!(open.open(), Some(1));
    let row = |n: i64| {
        theme.bar_height + theme.padding / 3 + n * theme.item_height + theme.item_height / 2
    };
    let x = 100 * PIXEL;
    let title = open.hit(&theme, x, mid);
    assert!(matches!(title, Some(MenuHit::Title(_))));
    let left = match open.hit(&theme, 60 * PIXEL, row(0)) {
        Some(MenuHit::Command(id)) => id,
        other => panic!("{other:?}"),
    };
    assert_eq!(left, "undo");
    assert_eq!(open.hit(&theme, 60 * PIXEL, row(1)), Some(MenuHit::Menu));
    // Below the menu is nothing of it.
    assert_eq!(open.hit(&theme, 60 * PIXEL, 700 * PIXEL), None);
    let drawn = open.draw(fonts(), &theme, Some(&MenuHit::Command("undo".to_string())));
    assert!(drawn.len() > 10);
}

#[test]
fn lists_hit_their_rows() {
    let theme = theme();
    let list = List::new(
        &theme,
        "Open",
        vec!["A.xw".to_string(), "B.xw".to_string()],
        800 * PIXEL,
        600 * PIXEL,
    );
    let probe = |row: i64| {
        // Rows follow the title row; probe each one's middle, across the
        // panel's middle.
        (0..800 * PIXEL).step_by(PIXEL as usize * 10).find_map(|x| {
            (0..600 * PIXEL)
                .step_by(PIXEL as usize)
                .find_map(|y| match list.hit(&theme, x, y) {
                    Some(Some(r)) if r as i64 == row => Some((x, y)),
                    _ => None,
                })
        })
    };
    assert!(probe(0).is_some());
    assert!(probe(1).is_some());
    assert!(probe(2).is_none());
    assert_eq!(list.hit(&theme, 0, 0), None);
    assert!(!list.draw(fonts(), &theme, Some(1)).is_empty());
    let (items, height) = toolkit::status_bar(
        fonts(),
        &theme,
        "Page 1 of 1",
        "Saved",
        800 * PIXEL,
        600 * PIXEL,
    );
    assert_eq!(height, theme.bar_height);
    assert!(items.len() > 2);
}
