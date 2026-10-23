//! xWorks' commands (spec/commands/m1.ron) hold together, and the menus
//! hold every command (review dcn5ejrd).

fn commands() -> chrome::Commands {
    let source = testkit::specs("commands")
        .expect("spec/commands")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("spec/commands/m1.ron")
        .1;
    let spec: chrome::Spec = ron::from_str(&source).expect("a commands spec");
    chrome::Commands::from_spec(&spec).expect("it holds together")
}

#[test]
fn the_commands_hold_together() {
    let commands = commands();
    // Everything the roadmap lists is there (docs/ROADMAP.md, M1 stage 9,
    // and M2's stages 2, 3 and 5).
    for id in [
        "new",
        "open",
        "open-recent",
        "rename",
        "move-to",
        "save-version",
        "export-pdf",
        "close",
        "quit",
        "undo",
        "redo",
        "bold",
        "italic",
        "code",
        "style-body",
        "style-heading-1",
        "style-heading-2",
        "style-heading-3",
        "style-code",
        "list-bullets",
        "list-numbers",
        "quote",
        "insert-table",
        "size-12",
        "word-left",
        "word-right",
        "select-word-left",
        "select-word-right",
        "sentence-left",
        "sentence-right",
        "align-justify",
        "language-en-US",
        "language-ja",
        "view-draft",
        "view-pages",
        "toggle-inspector",
        "toggle-ruler",
        "command-palette",
    ] {
        assert!(commands.get(id).is_some(), "{id}");
    }
    let state = |_: &str| chrome::CommandState {
        enabled: true,
        checked: false,
    };
    let bar = commands.menu_bar(false, state, |_| Vec::new());
    let titles: Vec<&str> = bar.menus.iter().map(|m| m.title.as_str()).collect();
    // The mockups' menus (review 3ce247c2).
    assert_eq!(titles, ["File", "Edit", "View", "Insert", "Format", "Help"]);
    let keys: Vec<Option<char>> = bar.menus.iter().map(|m| m.access).collect();
    assert!(keys.iter().all(Option::is_some), "{keys:?}");
}

/// Every command that runs without an argument is in a menu, but for
/// moving the caret, which is keys, and languages and sizes, which the
/// inspector sets; the palette finds them all.
#[test]
fn the_menus_hold_every_command() {
    let commands = commands();
    let state = |_: &str| chrome::CommandState {
        enabled: true,
        checked: false,
    };
    let bar = commands.menu_bar(false, state, |_| Vec::new());
    let mut listed = Vec::new();
    for menu in &bar.menus {
        for item in &menu.items {
            if let chrome::MenuItem::Command { id, .. } = item {
                listed.push(id.clone());
            }
        }
    }
    for command in commands.all() {
        if command.arg.is_some() || ["Moving", "Language", "Font size"].contains(&&*command.kind) {
            continue;
        }
        assert!(listed.contains(&command.id), "{} is in no menu", command.id);
    }
    let found = commands.find("", false, |_| true);
    assert_eq!(
        found.len(),
        commands.all().iter().filter(|c| c.arg.is_none()).count()
    );
}
