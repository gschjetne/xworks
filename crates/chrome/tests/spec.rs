//! M1's commands (spec/commands/m1.ron) hold together.

#[test]
fn m1s_commands_hold_together() {
    let source = testkit::specs("commands")
        .expect("spec/commands")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("spec/commands/m1.ron")
        .1;
    let spec: chrome::Spec = ron::from_str(&source).expect("a commands spec");
    let commands = chrome::Commands::from_spec(&spec).expect("it holds together");
    // Everything the roadmap lists is there (docs/ROADMAP.md, M1 stage 9).
    for id in [
        "new",
        "open",
        "save-version",
        "export-pdf",
        "quit",
        "undo",
        "redo",
        "bold",
        "italic",
        "style-body",
        "style-heading-1",
        "style-heading-2",
        "style-heading-3",
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
    ] {
        assert!(commands.get(id).is_some(), "{id}");
    }
    let bar = commands.menu_bar(false, |_| chrome::CommandState {
        enabled: true,
        checked: false,
    });
    let titles: Vec<&str> = bar.menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["File", "Edit", "Format", "Language"]);
}
