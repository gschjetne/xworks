//! Commands and the chrome model (docs/ARCHITECTURE.md, section 8): the
//! commands an app offers, as data from `spec/commands/`, their keyboard
//! shortcuts, and the menus they're in. The toolkit draws the model; the app
//! acts on commands by ID.
//!
//! - [`Commands`]: the commands and menus of a spec, checked.
//! - [`Key`], [`Modifiers`], [`Shortcut`]: keys as shells report them, and
//!   matching them to commands.
//! - [`MenuBar`]: the menus as they stand, with each command enabled or not.
//!
//! Invariants:
//!
//! - Command IDs are unique, every menu item names a command, and no two
//!   commands share a shortcut on one platform.
//! - "Primary" is Command on a Mac and Ctrl elsewhere; shortcuts match the
//!   key as it is without Shift, so they don't depend on the keyboard
//!   layout's shifted characters.
//! - A shortcut that differs on a Mac, beyond Primary, is given for the
//!   Mac on its own: moving by words is Ctrl and an arrow on Windows and
//!   Linux, Option and an arrow on a Mac.

use serde::Deserialize;

/// The commands and menus as `spec/commands/` gives them.
#[derive(Clone, Debug, Deserialize)]
pub struct Spec {
    /// The commands.
    pub commands: Vec<CommandSpec>,
    /// The menus, left to right.
    pub menus: Vec<MenuSpec>,
}

/// A command as a spec gives it.
#[derive(Clone, Debug, Deserialize)]
pub struct CommandSpec {
    /// Its ID, which the app acts on.
    pub id: String,
    /// Its title in menus.
    pub title: String,
    /// Its shortcut, such as "Primary+Shift+Z".
    pub shortcut: Option<String>,
    /// Its shortcut on a Mac, where that's another; otherwise `shortcut`
    /// is a Mac's too.
    #[serde(default)]
    pub mac: Option<String>,
}

/// A menu as a spec gives it.
#[derive(Clone, Debug, Deserialize)]
pub struct MenuSpec {
    /// Its title in the menu bar.
    pub title: String,
    /// Its items: command IDs, and "-" for separators.
    pub items: Vec<String>,
}

/// Why a spec doesn't hold together.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Two commands with one ID.
    #[error("two commands are called {0:?}")]
    Duplicate(String),
    /// A menu item naming no command.
    #[error("the {menu} menu lists {item:?}, which isn't a command")]
    Unknown {
        /// The menu.
        menu: String,
        /// The item.
        item: String,
    },
    /// A shortcut that can't be read.
    #[error("{command}'s shortcut {shortcut:?} isn't one")]
    BadShortcut {
        /// The command.
        command: String,
        /// The shortcut.
        shortcut: String,
    },
    /// Two commands with one shortcut.
    #[error("{0:?} and {1:?} have the same shortcut")]
    SameShortcut(String, String),
}

/// A key, as it is without Shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A key that types a character, lower case.
    Char(char),
    /// Enter or Return.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete, forward.
    Delete,
    /// Escape.
    Escape,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}

/// The modifier keys held with a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Ctrl, or Command on a Mac.
    pub primary: bool,
    /// Shift.
    pub shift: bool,
    /// Alt, or Option on a Mac.
    pub alt: bool,
}

/// A key with modifiers that runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// The modifiers.
    pub modifiers: Modifiers,
    /// The key.
    pub key: Key,
}

impl Shortcut {
    /// Reads "Primary+Shift+Z" and the like.
    pub fn parse(text: &str) -> Option<Shortcut> {
        let mut modifiers = Modifiers::default();
        let mut parts: Vec<&str> = text.split('+').collect();
        // "Primary++" would be Plus; M1 has none, so a trailing empty
        // part is an error.
        let key = parts.pop()?;
        for part in parts {
            match part {
                "Primary" => modifiers.primary = true,
                "Shift" => modifiers.shift = true,
                "Alt" => modifiers.alt = true,
                _ => return None,
            }
        }
        let key = match key {
            "Enter" => Key::Enter,
            "Tab" => Key::Tab,
            "Backspace" => Key::Backspace,
            "Delete" => Key::Delete,
            "Escape" => Key::Escape,
            "Left" => Key::Left,
            "Right" => Key::Right,
            "Up" => Key::Up,
            "Down" => Key::Down,
            "Home" => Key::Home,
            "End" => Key::End,
            "PageUp" => Key::PageUp,
            "PageDown" => Key::PageDown,
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Char(c.to_ascii_lowercase()),
                    _ => return None,
                }
            }
        };
        Some(Shortcut { modifiers, key })
    }

    /// How menus show it: "Ctrl+Shift+Z", or "⇧⌘Z" on a Mac.
    pub fn label(&self, mac: bool) -> String {
        let key = match self.key {
            Key::Char(c) => c.to_uppercase().to_string(),
            other => format!("{other:?}"),
        };
        let m = self.modifiers;
        if mac {
            format!(
                "{}{}{}{key}",
                if m.alt { "⌥" } else { "" },
                if m.shift { "⇧" } else { "" },
                if m.primary { "⌘" } else { "" },
            )
        } else {
            let mut parts = Vec::new();
            if m.primary {
                parts.push("Ctrl".to_string());
            }
            if m.alt {
                parts.push("Alt".to_string());
            }
            if m.shift {
                parts.push("Shift".to_string());
            }
            parts.push(key);
            parts.join("+")
        }
    }
}

/// A command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Its ID.
    pub id: String,
    /// Its title in menus.
    pub title: String,
    /// Its shortcut on Windows and Linux.
    pub shortcut: Option<Shortcut>,
    /// Its shortcut on a Mac.
    pub mac: Option<Shortcut>,
}

impl Command {
    /// Its shortcut on a Mac, or elsewhere.
    pub fn shortcut_on(&self, mac: bool) -> Option<Shortcut> {
        if mac { self.mac } else { self.shortcut }
    }
}

/// The commands and menus of a spec, checked.
#[derive(Clone, Debug)]
pub struct Commands {
    commands: Vec<Command>,
    menus: Vec<MenuSpec>,
}

/// Whether a command can run now, and whether it's on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandState {
    /// Whether it can run.
    pub enabled: bool,
    /// Whether it's on, such as Bold over bold text.
    pub checked: bool,
}

/// The menu bar as it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuBar {
    /// Its menus, left to right.
    pub menus: Vec<Menu>,
}

/// A menu as it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// Its title.
    pub title: String,
    /// Its items, top to bottom.
    pub items: Vec<MenuItem>,
}

/// An item of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// A command.
    Command {
        /// Its ID.
        id: String,
        /// Its title.
        title: String,
        /// Its shortcut, as the menu shows it.
        shortcut: Option<String>,
        /// Whether it can run.
        enabled: bool,
        /// Whether it's on.
        checked: bool,
    },
    /// A line between groups.
    Separator,
}

impl Commands {
    /// The commands and menus `spec` gives, if it holds together.
    pub fn from_spec(spec: &Spec) -> Result<Commands, Error> {
        let mut commands: Vec<Command> = Vec::new();
        for c in &spec.commands {
            if commands.iter().any(|d| d.id == c.id) {
                return Err(Error::Duplicate(c.id.clone()));
            }
            let parse = |s: &String| {
                Shortcut::parse(s).ok_or_else(|| Error::BadShortcut {
                    command: c.id.clone(),
                    shortcut: s.clone(),
                })
            };
            let shortcut = c.shortcut.as_ref().map(parse).transpose()?;
            let mac = match &c.mac {
                Some(s) => Some(parse(s)?),
                None => shortcut,
            };
            // No two commands share a shortcut on one platform.
            if let Some(other) = commands.iter().find(|d| {
                (shortcut.is_some() && d.shortcut == shortcut) || (mac.is_some() && d.mac == mac)
            }) {
                return Err(Error::SameShortcut(other.id.clone(), c.id.clone()));
            }
            commands.push(Command {
                id: c.id.clone(),
                title: c.title.clone(),
                shortcut,
                mac,
            });
        }
        for menu in &spec.menus {
            for item in &menu.items {
                if item != "-" && !commands.iter().any(|c| &c.id == item) {
                    return Err(Error::Unknown {
                        menu: menu.title.clone(),
                        item: item.clone(),
                    });
                }
            }
        }
        Ok(Commands {
            commands,
            menus: spec.menus.clone(),
        })
    }

    /// The command `id`.
    pub fn get(&self, id: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Every command, in the spec's order.
    pub fn all(&self) -> &[Command] {
        &self.commands
    }

    /// The command whose shortcut is `key` with `modifiers`, on a Mac or
    /// elsewhere, if any.
    pub fn for_key(&self, key: Key, modifiers: Modifiers, mac: bool) -> Option<&Command> {
        let key = match key {
            Key::Char(c) => Key::Char(c.to_ascii_lowercase()),
            other => other,
        };
        let wanted = Shortcut { modifiers, key };
        self.commands
            .iter()
            .find(|c| c.shortcut_on(mac) == Some(wanted))
    }

    /// The menu bar, with each command's state from `state`, and shortcuts
    /// labelled for a Mac or not.
    pub fn menu_bar(&self, mac: bool, state: impl Fn(&str) -> CommandState) -> MenuBar {
        MenuBar {
            menus: self
                .menus
                .iter()
                .map(|menu| Menu {
                    title: menu.title.clone(),
                    items: menu
                        .items
                        .iter()
                        .map(|item| match self.get(item) {
                            None => MenuItem::Separator,
                            Some(c) => {
                                let s = state(&c.id);
                                MenuItem::Command {
                                    id: c.id.clone(),
                                    title: c.title.clone(),
                                    shortcut: c.shortcut_on(mac).map(|s| s.label(mac)),
                                    enabled: s.enabled,
                                    checked: s.checked,
                                }
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(commands: &[(&str, Option<&str>)], menus: &[(&str, &[&str])]) -> Spec {
        Spec {
            commands: commands
                .iter()
                .map(|(id, s)| CommandSpec {
                    id: id.to_string(),
                    title: id.to_uppercase(),
                    shortcut: s.map(str::to_string),
                    mac: None,
                })
                .collect(),
            menus: menus
                .iter()
                .map(|(t, items)| MenuSpec {
                    title: t.to_string(),
                    items: items.iter().map(|s| s.to_string()).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn shortcuts_read_match_and_label() {
        let s = Shortcut::parse("Primary+Shift+Z").unwrap();
        assert_eq!(
            s,
            Shortcut {
                modifiers: Modifiers {
                    primary: true,
                    shift: true,
                    alt: false
                },
                key: Key::Char('z')
            }
        );
        assert_eq!(s.label(false), "Ctrl+Shift+Z");
        assert_eq!(s.label(true), "⇧⌘Z");
        assert_eq!(Shortcut::parse("Primary+Enter").unwrap().key, Key::Enter);
        for bad in ["", "Primary+", "Super+Z", "Primary+ZZ"] {
            assert_eq!(Shortcut::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn keys_find_their_commands() {
        let commands = Commands::from_spec(&spec(
            &[
                ("undo", Some("Primary+Z")),
                ("redo", Some("Primary+Shift+Z")),
                ("plain", None),
            ],
            &[("Edit", &["undo", "-", "redo", "plain"])],
        ))
        .unwrap();
        let primary = Modifiers {
            primary: true,
            ..Modifiers::default()
        };
        let both = Modifiers {
            shift: true,
            ..primary
        };
        assert_eq!(
            commands.for_key(Key::Char('z'), primary, false).unwrap().id,
            "undo"
        );
        assert_eq!(
            commands.for_key(Key::Char('Z'), both, false).unwrap().id,
            "redo"
        );
        assert!(
            commands
                .for_key(Key::Char('z'), Modifiers::default(), false)
                .is_none()
        );

        let bar = commands.menu_bar(false, |id| CommandState {
            enabled: id != "redo",
            checked: false,
        });
        assert_eq!(bar.menus.len(), 1);
        assert_eq!(
            bar.menus[0].items,
            [
                MenuItem::Command {
                    id: "undo".to_string(),
                    title: "UNDO".to_string(),
                    shortcut: Some("Ctrl+Z".to_string()),
                    enabled: true,
                    checked: false
                },
                MenuItem::Separator,
                MenuItem::Command {
                    id: "redo".to_string(),
                    title: "REDO".to_string(),
                    shortcut: Some("Ctrl+Shift+Z".to_string()),
                    enabled: false,
                    checked: false
                },
                MenuItem::Command {
                    id: "plain".to_string(),
                    title: "PLAIN".to_string(),
                    shortcut: None,
                    enabled: true,
                    checked: false
                },
            ]
        );
    }

    #[test]
    fn a_mac_can_have_its_own_shortcut() {
        let mut spec = spec(
            &[
                ("word-right", Some("Primary+Right")),
                ("end", Some("Alt+Right")),
            ],
            &[("Edit", &["word-right"])],
        );
        spec.commands[0].mac = Some("Alt+Right".to_string());
        // Alt+Right is the word's on a Mac, and another command's
        // elsewhere: no clash.
        spec.commands[1].mac = Some("Primary+Right".to_string());
        let commands = Commands::from_spec(&spec).unwrap();
        let primary = Modifiers {
            primary: true,
            ..Modifiers::default()
        };
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(
            commands.for_key(Key::Right, primary, false).unwrap().id,
            "word-right"
        );
        assert_eq!(
            commands.for_key(Key::Right, alt, true).unwrap().id,
            "word-right"
        );
        assert_eq!(commands.for_key(Key::Right, alt, false).unwrap().id, "end");
        let label = |mac| match &commands
            .menu_bar(mac, |_| CommandState {
                enabled: true,
                checked: false,
            })
            .menus[0]
            .items[0]
        {
            MenuItem::Command { shortcut, .. } => shortcut.clone(),
            MenuItem::Separator => None,
        };
        assert_eq!(label(false).as_deref(), Some("Ctrl+Right"));
        assert_eq!(label(true).as_deref(), Some("⌥Right"));
        // A shortcut that clashes on a Mac alone is refused.
        let mut clash = spec.clone();
        clash.commands[1].mac = Some("Alt+Right".to_string());
        assert_eq!(
            Commands::from_spec(&clash).unwrap_err(),
            Error::SameShortcut("word-right".to_string(), "end".to_string())
        );
    }

    #[test]
    fn specs_that_dont_hold_together_are_refused() {
        assert_eq!(
            Commands::from_spec(&spec(&[("a", None), ("a", None)], &[])).unwrap_err(),
            Error::Duplicate("a".to_string())
        );
        assert_eq!(
            Commands::from_spec(&spec(&[("a", None)], &[("File", &["b"])])).unwrap_err(),
            Error::Unknown {
                menu: "File".to_string(),
                item: "b".to_string()
            }
        );
        assert!(matches!(
            Commands::from_spec(&spec(&[("a", Some("Hyper+A"))], &[])),
            Err(Error::BadShortcut { .. })
        ));
        assert_eq!(
            Commands::from_spec(&spec(
                &[("a", Some("Primary+A")), ("b", Some("Primary+a"))],
                &[]
            ))
            .unwrap_err(),
            Error::SameShortcut("a".to_string(), "b".to_string())
        );
    }
}
