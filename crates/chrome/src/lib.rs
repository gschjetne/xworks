//! Commands and the chrome model (docs/ARCHITECTURE.md, section 8): the
//! commands an app offers, as data from `spec/commands/`, their keyboard
//! shortcuts, their arguments, and the menus they're in; and the rest of
//! what a window's chrome holds, described rather than drawn. The toolkit
//! draws the model, and a port to a platform's own widgets would build
//! them from it; the app acts on commands by ID.
//!
//! - [`Commands`]: the commands and menus of a spec, checked, and found
//!   by name for the command palette ([`Commands::find`]).
//! - [`Key`], [`Modifiers`], [`Shortcut`]: keys as shells report them, and
//!   matching them to commands.
//! - [`MenuBar`]: the menus as they stand, with each command enabled or
//!   not, their access keys, and the submenus the app fills, such as
//!   recent documents.
//! - [`Arg`]: a command's typed arguments, such as a length typed into the
//!   inspector or a document chosen from a menu.
//! - [`Inspector`]: the selection's properties as typed fields, in tabs
//!   and sections ([`inspector`]).
//! - [`Ruler`]: a text column with its indents and tab stops.
//! - [`Length`] and [`Unit`]: lengths shown and typed with their units.
//!
//! Invariants:
//!
//! - Command IDs are unique, every menu item names a command, and no two
//!   commands share a shortcut on one platform.
//! - A command that takes an argument has no shortcut and is no plain
//!   menu item: it's run with its argument, from the inspector, the ruler,
//!   a submenu the app fills, or a script.
//! - "Primary" is Command on a Mac and Ctrl elsewhere; shortcuts match the
//!   key as it is without Shift, so they don't depend on the keyboard
//!   layout's shifted characters.
//! - A shortcut that differs on a Mac, beyond Primary, is given for the
//!   Mac on its own: moving by words is Ctrl and an arrow on Windows and
//!   Linux, Option and an arrow on a Mac.
//! - Titles mark their access key with `&` before it, as Windows does, and
//!   `&&` for an ampersand; menus show the title without it. A Mac has no
//!   access keys.

pub mod inspector;
mod length;
mod ruler;

use serde::Deserialize;

pub use inspector::{Choice, Control, ControlKind, Inspector, InspectorTab, Row, Section, Segment};
pub use length::{Length, Unit};
pub use ruler::{Indents, Ruler, Stop, StopKind};

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
    /// Its title in menus and the palette, its access key marked with
    /// `&`.
    pub title: String,
    /// Its shortcut, such as "Primary+Shift+Z".
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Its shortcut on a Mac, where that's another; otherwise `shortcut`
    /// is a Mac's too.
    #[serde(default)]
    pub mac: Option<String>,
    /// What it is, as the palette says beside its title, such as
    /// "Paragraph style"; otherwise the title of the first menu it's in.
    #[serde(default)]
    pub kind: Option<String>,
    /// The argument it takes, if it takes one.
    #[serde(default)]
    pub arg: Option<ArgKind>,
}

/// A menu as a spec gives it.
#[derive(Clone, Debug, Deserialize)]
pub struct MenuSpec {
    /// Its title in the menu bar, its access key marked with `&`.
    pub title: String,
    /// Its items: command IDs, "-" for separators, and ">" before the ID
    /// of a command whose submenu the app fills, titled as the command.
    pub items: Vec<String>,
}

/// What kind of argument a command takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ArgKind {
    /// Text, such as a style's name or a document's path.
    Text,
    /// A whole number, such as a length in layout units.
    Int,
    /// Several, as the command says.
    List,
}

/// An argument a command is run with.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Arg {
    /// Text.
    Text(String),
    /// A whole number, such as a length in layout units.
    Int(i64),
}

impl Arg {
    /// The text, if it's text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Arg::Text(t) => Some(t),
            Arg::Int(_) => None,
        }
    }

    /// The number, if it's a number.
    pub fn int(&self) -> Option<i64> {
        match self {
            Arg::Int(n) => Some(*n),
            Arg::Text(_) => None,
        }
    }
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
    /// A command that takes an argument, with a shortcut or as a plain
    /// menu item, where nothing gives it its argument.
    #[error("{0:?} takes an argument, so it can't have a shortcut or be a plain menu item")]
    NeedsArgument(String),
    /// Two items of one menu, or two menus, with one access key.
    #[error("{0:?} has two items with the access key {1:?}")]
    SameAccessKey(String, char),
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
    /// Alt pressed and let go with no other key: on Windows and Linux,
    /// the menus' access keys.
    Alt,
    /// F10, which also goes to the menus on Windows and Linux.
    F10,
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
        // "Primary++" would be Plus; none has it, so a trailing empty part
        // is an error.
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
            "Alt" => Key::Alt,
            "F10" => Key::F10,
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

/// A title as specs write it, its access key marked with `&`: the title
/// without the mark, and the key, lower case.
pub fn access(title: &str) -> (String, Option<char>) {
    let mut out = String::with_capacity(title.len());
    let mut key = None;
    let mut chars = title.chars();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('&') => out.push('&'),
            Some(next) => {
                if key.is_none() {
                    key = Some(next.to_lowercase().next().unwrap_or(next));
                }
                out.push(next);
            }
            None => {}
        }
    }
    (out, key)
}

/// A command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Its ID.
    pub id: String,
    /// Its title in menus and the palette.
    pub title: String,
    /// Its access key in menus on Windows and Linux, lower case.
    pub access: Option<char>,
    /// Its shortcut on Windows and Linux.
    pub shortcut: Option<Shortcut>,
    /// Its shortcut on a Mac.
    pub mac: Option<Shortcut>,
    /// What the palette says it is.
    pub kind: String,
    /// The argument it takes, if any.
    pub arg: Option<ArgKind>,
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
    /// Its access key, lower case.
    pub access: Option<char>,
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
        /// What it's run with, for a command that takes an argument.
        args: Vec<Arg>,
        /// Its title.
        title: String,
        /// Its access key, lower case.
        access: Option<char>,
        /// Its shortcut, as the menu shows it.
        shortcut: Option<String>,
        /// Whether it can run.
        enabled: bool,
        /// Whether it's on.
        checked: bool,
    },
    /// A menu within the menu, which the app fills.
    Submenu {
        /// Its title.
        title: String,
        /// Its access key, lower case.
        access: Option<char>,
        /// Its items; none leaves it disabled.
        items: Vec<MenuItem>,
    },
    /// A line between groups.
    Separator,
}

impl MenuItem {
    /// Whether it can be chosen: a command that's enabled, or a submenu
    /// with something in it.
    pub fn enabled(&self) -> bool {
        match self {
            MenuItem::Command { enabled, .. } => *enabled,
            MenuItem::Submenu { items, .. } => !items.is_empty(),
            MenuItem::Separator => false,
        }
    }

    /// Its access key.
    pub fn access(&self) -> Option<char> {
        match self {
            MenuItem::Command { access, .. } | MenuItem::Submenu { access, .. } => *access,
            MenuItem::Separator => None,
        }
    }
}

/// A command the palette found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Its ID.
    pub id: String,
    /// Its title.
    pub title: String,
    /// What it is.
    pub kind: String,
    /// Its shortcut, as menus show it.
    pub shortcut: Option<String>,
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
            if c.arg.is_some() && (shortcut.is_some() || mac.is_some()) {
                return Err(Error::NeedsArgument(c.id.clone()));
            }
            // No two commands share a shortcut on one platform.
            if let Some(other) = commands.iter().find(|d| {
                (shortcut.is_some() && d.shortcut == shortcut) || (mac.is_some() && d.mac == mac)
            }) {
                return Err(Error::SameShortcut(other.id.clone(), c.id.clone()));
            }
            let (title, access) = access(&c.title);
            let kind = c.kind.clone().unwrap_or_else(|| {
                spec.menus
                    .iter()
                    .find(|m| {
                        m.items
                            .iter()
                            .any(|i| i.trim_start_matches('>') == c.id.as_str())
                    })
                    .map_or_else(String::new, |m| self::access(&m.title).0)
            });
            commands.push(Command {
                id: c.id.clone(),
                title,
                access,
                shortcut,
                mac,
                kind,
                arg: c.arg,
            });
        }
        let mut titles = Vec::new();
        for menu in &spec.menus {
            let (title, key) = access(&menu.title);
            if let Some(key) = key {
                if titles.contains(&key) {
                    return Err(Error::SameAccessKey("the menu bar".to_string(), key));
                }
                titles.push(key);
            }
            let mut keys = Vec::new();
            for item in &menu.items {
                if item == "-" {
                    continue;
                }
                let (id, submenu) = match item.strip_prefix('>') {
                    Some(id) => (id, true),
                    None => (item.as_str(), false),
                };
                let Some(command) = commands.iter().find(|c| c.id == id) else {
                    return Err(Error::Unknown {
                        menu: title,
                        item: item.clone(),
                    });
                };
                if command.arg.is_some() && !submenu {
                    return Err(Error::NeedsArgument(command.id.clone()));
                }
                if let Some(key) = command.access {
                    if keys.contains(&key) {
                        return Err(Error::SameAccessKey(title, key));
                    }
                    keys.push(key);
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

    /// The menu bar, with each command's state from `state`, shortcuts
    /// labelled for a Mac or not, and each submenu's items from `fill`,
    /// given the ID of the command it's titled as. A Mac has no access
    /// keys.
    pub fn menu_bar(
        &self,
        mac: bool,
        state: impl Fn(&str) -> CommandState,
        fill: impl Fn(&str) -> Vec<MenuItem>,
    ) -> MenuBar {
        let keyed = |key: Option<char>| if mac { None } else { key };
        MenuBar {
            menus: self
                .menus
                .iter()
                .map(|menu| {
                    let (title, key) = access(&menu.title);
                    Menu {
                        title,
                        access: keyed(key),
                        items: menu
                            .items
                            .iter()
                            .map(|item| {
                                if let Some(id) = item.strip_prefix('>')
                                    && let Some(c) = self.get(id)
                                {
                                    return MenuItem::Submenu {
                                        title: c.title.clone(),
                                        access: keyed(c.access),
                                        items: fill(id),
                                    };
                                }
                                match self.get(item) {
                                    None => MenuItem::Separator,
                                    Some(c) => {
                                        let s = state(&c.id);
                                        MenuItem::Command {
                                            id: c.id.clone(),
                                            args: Vec::new(),
                                            title: c.title.clone(),
                                            access: keyed(c.access),
                                            shortcut: c.shortcut_on(mac).map(|s| s.label(mac)),
                                            enabled: s.enabled,
                                            checked: s.checked,
                                        }
                                    }
                                }
                            })
                            .collect(),
                    }
                })
                .collect(),
        }
    }

    /// The commands whose titles match `query`, as the command palette
    /// finds them, best first: titles that start with it, then those with
    /// a word that does, then those that hold it, then those with its
    /// letters in order; each group in the spec's order. Case doesn't
    /// count, and commands that take an argument, or that `shown` refuses,
    /// aren't found. An empty query finds every command.
    pub fn find(&self, query: &str, mac: bool, shown: impl Fn(&Command) -> bool) -> Vec<Found> {
        let query = query.trim().to_lowercase();
        let mut found: Vec<(u8, usize, &Command)> = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c.arg.is_none() && shown(c))
            .filter_map(|(i, c)| rank(&c.title.to_lowercase(), &query).map(|r| (r, i, c)))
            .collect();
        found.sort_by_key(|&(r, i, _)| (r, i));
        found
            .into_iter()
            .map(|(_, _, c)| Found {
                id: c.id.clone(),
                title: c.title.clone(),
                kind: c.kind.clone(),
                shortcut: c.shortcut_on(mac).map(|s| s.label(mac)),
            })
            .collect()
    }
}

/// How well `title` matches `query`, both lower case: 0 if it starts with
/// it, 1 if a word in it does, 2 if it holds it, 3 if it has its letters
/// in order, and `None` if not.
fn rank(title: &str, query: &str) -> Option<u8> {
    if query.is_empty() || title.starts_with(query) {
        return Some(0);
    }
    let words = title
        .char_indices()
        .filter(|&(i, _)| {
            i > 0
                && title[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| !c.is_alphanumeric())
        })
        .any(|(i, _)| title[i..].starts_with(query));
    if words {
        return Some(1);
    }
    if title.contains(query) {
        return Some(2);
    }
    let mut letters = title.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|q| letters.any(|c| c == q))
        .then_some(3)
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
                    kind: None,
                    arg: None,
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

    fn enabled(_: &str) -> CommandState {
        CommandState {
            enabled: true,
            checked: false,
        }
    }

    fn none(_: &str) -> Vec<MenuItem> {
        Vec::new()
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
        assert_eq!(Shortcut::parse("F10").unwrap().key, Key::F10);
        for bad in ["", "Primary+", "Super+Z", "Primary+ZZ"] {
            assert_eq!(Shortcut::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn access_keys_are_marked_with_ampersands() {
        assert_eq!(access("&File"), ("File".to_string(), Some('f')));
        assert_eq!(access("F&ormat"), ("Format".to_string(), Some('o')));
        assert_eq!(
            access("Save && &Quit"),
            ("Save & Quit".to_string(), Some('q'))
        );
        assert_eq!(access("Plain"), ("Plain".to_string(), None));
        assert_eq!(access("Trailing&"), ("Trailing".to_string(), None));
    }

    #[test]
    fn keys_find_their_commands() {
        let commands = Commands::from_spec(&spec(
            &[
                ("undo", Some("Primary+Z")),
                ("redo", Some("Primary+Shift+Z")),
                ("plain", None),
            ],
            &[("&Edit", &["undo", "-", "redo", "plain"])],
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

        let bar = commands.menu_bar(
            false,
            |id| CommandState {
                enabled: id != "redo",
                checked: false,
            },
            none,
        );
        assert_eq!(bar.menus.len(), 1);
        assert_eq!(bar.menus[0].title, "Edit");
        assert_eq!(bar.menus[0].access, Some('e'));
        let command = |id: &str, shortcut: Option<&str>, enabled| MenuItem::Command {
            id: id.to_string(),
            args: Vec::new(),
            title: id.to_uppercase(),
            access: None,
            shortcut: shortcut.map(str::to_string),
            enabled,
            checked: false,
        };
        assert_eq!(
            bar.menus[0].items,
            [
                command("undo", Some("Ctrl+Z"), true),
                MenuItem::Separator,
                command("redo", Some("Ctrl+Shift+Z"), false),
                command("plain", None, true),
            ]
        );
        // A Mac has no access keys.
        assert_eq!(commands.menu_bar(true, enabled, none).menus[0].access, None);
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
        let label = |mac| match &commands.menu_bar(mac, enabled, none).menus[0].items[0] {
            MenuItem::Command { shortcut, .. } => shortcut.clone(),
            _ => None,
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
    fn submenus_are_filled_by_the_app() {
        let mut spec = spec(&[("open-recent", None)], &[("&File", &[">open-recent"])]);
        spec.commands[0].title = "Open &Recent".to_string();
        spec.commands[0].arg = Some(ArgKind::Text);
        let commands = Commands::from_spec(&spec).unwrap();
        let recent = MenuItem::Command {
            id: "open-recent".to_string(),
            args: vec![Arg::Text("/a.xw".to_string())],
            title: "a".to_string(),
            access: None,
            shortcut: None,
            enabled: true,
            checked: false,
        };
        let bar = commands.menu_bar(false, enabled, |id| {
            assert_eq!(id, "open-recent");
            vec![recent.clone()]
        });
        assert_eq!(
            bar.menus[0].items,
            [MenuItem::Submenu {
                title: "Open Recent".to_string(),
                access: Some('r'),
                items: vec![recent],
            }]
        );
        // Not as a plain item, nor with a shortcut: nothing would give it
        // its argument.
        let mut plain = spec.clone();
        plain.menus[0].items = vec!["open-recent".to_string()];
        assert_eq!(
            Commands::from_spec(&plain).unwrap_err(),
            Error::NeedsArgument("open-recent".to_string())
        );
        let mut keyed = spec.clone();
        keyed.commands[0].shortcut = Some("Primary+R".to_string());
        assert_eq!(
            Commands::from_spec(&keyed).unwrap_err(),
            Error::NeedsArgument("open-recent".to_string())
        );
    }

    #[test]
    fn the_palette_finds_commands_by_name() {
        let mut spec = spec(
            &[
                ("style-heading-1", Some("Primary+1")),
                ("go-to-heading", None),
                ("headers", None),
                ("bold", Some("Primary+B")),
                ("ahead", None),
                ("open-recent", None),
            ],
            &[("F&ormat", &["style-heading-1", "bold"])],
        );
        let titles = [
            "Heading &1",
            "Go to Heading…",
            "Headers and Footers",
            "&Bold",
            "Shead",
            "Open Recent",
        ];
        for (c, t) in spec.commands.iter_mut().zip(titles) {
            c.title = t.to_string();
        }
        spec.commands[0].kind = Some("Paragraph style".to_string());
        spec.commands[5].arg = Some(ArgKind::Text);
        let commands = Commands::from_spec(&spec).unwrap();
        let found = commands.find("Head", false, |_| true);
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            ["style-heading-1", "headers", "go-to-heading", "ahead"]
        );
        assert_eq!(found[0].title, "Heading 1");
        assert_eq!(found[0].kind, "Paragraph style");
        assert_eq!(found[0].shortcut.as_deref(), Some("Ctrl+1"));
        // Its letters in order, and the menu it's in as its kind.
        let bold = commands.find("bd", false, |_| true);
        assert_eq!(bold[0].id, "bold");
        assert_eq!(bold[0].kind, "Format");
        // Everything for nothing, but what takes an argument or isn't
        // shown.
        let all = commands.find("", true, |c| c.id != "bold");
        assert_eq!(all.len(), 4);
        assert!(commands.find("zzz", false, |_| true).is_empty());
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
        let mut keys = spec(&[("a", None), ("b", None)], &[("File", &["a", "b"])]);
        keys.commands[0].title = "&Apple".to_string();
        keys.commands[1].title = "&Avocado".to_string();
        assert_eq!(
            Commands::from_spec(&keys).unwrap_err(),
            Error::SameAccessKey("File".to_string(), 'a')
        );
        let menus = spec(&[], &[("&File", &[]), ("&Format", &[])]);
        assert_eq!(
            Commands::from_spec(&menus).unwrap_err(),
            Error::SameAccessKey("the menu bar".to_string(), 'f')
        );
    }
}
