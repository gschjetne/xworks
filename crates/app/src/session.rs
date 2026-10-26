//! Scripted sessions (`spec/sessions/`): input played into a window, as a
//! person would give it, chrome and all, for tests and for `shot`'s
//! frames, and as the session recorder writes what a person did
//! ([`crate::recorder`]).

use std::path::PathBuf;
use std::sync::Arc;

use chrome::{Key, Modifiers, Shortcut};
use doc::Value;
use serde::{Deserialize, Serialize};

use crate::{Choice, Editor, EditorError, Effect, Event, Resources, Window};

/// A session as `spec/sessions/` gives it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    /// The window's size, in logical pixels.
    pub size: (f64, f64),
    /// What the person does, in order.
    pub events: Vec<Step>,
    /// The paragraphs afterwards, in reading order: each one's style and
    /// text.
    pub expect: Vec<(String, String)>,
    /// The story's tree afterwards, if the session says, as
    /// `story::outline` gives it: a line an object, indented a level a
    /// container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<String>>,
    /// Stretches of text, each with the properties set on every character
    /// of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<(String, Vec<String>)>,
    /// Stretches of text, each with properties set on no character of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmarked: Vec<(String, Vec<String>)>,
    /// What the clipboard holds afterwards, if the session copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard: Option<String>,
    /// What the status bar says afterwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The system's language, which the new document is in, as a BCP 47
    /// tag; none if it's left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Stretches of text, each with the language every character of it is
    /// in, as a BCP 47 tag: its own, or its paragraph's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<(String, String)>,
    /// Whether shortcuts read as on a Mac, as where it was recorded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mac: bool,
    /// Where it starts, if not in a new, empty document: as the session
    /// recorder found the window when what it kept begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Start>,
    /// The document's name afterwards, as the title bar shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The name of its file afterwards, without the folder or extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// What's asked of the window itself, in order, as their names: Drag,
    /// ToggleMaximize, Minimize, Close, Resize(..).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    /// Commands that are on afterwards, such as the inspector's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,
    /// Commands that are off afterwards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    /// Paragraph properties afterwards: a stretch of the paragraph's text,
    /// the property, and its value as text, or none for unset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<(String, String, Option<String>)>,
    /// A file the session opens, written into the documents folder first,
    /// if it doesn't start in a new, empty document: its name and what it
    /// holds, as a Markdown file's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<(String, String)>,
    /// Files copied into the documents folder before it starts, such as
    /// pictures to place: each from its path in the workspace to its name
    /// there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copy: Vec<(String, String)>,
    /// Files in the documents folder afterwards: each one's name, and what
    /// it holds, where that's said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<(String, Option<String>)>,
}

/// Where a recorded session starts: the document, its history, and the
/// window's state that what follows depends on, which a new window then
/// takes up ([`Editor::resume`]).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Start {
    /// The document.
    pub document: doc::Snapshot,
    /// What Undo takes back, oldest first.
    pub undo: Vec<doc::Transaction>,
    /// What Redo brings back, oldest first.
    pub redo: Vec<doc::Transaction>,
    /// Where the selection starts and ends, which is where the caret is:
    /// each a paragraph's ID, an offset in characters, and whether it
    /// belongs with the character before.
    pub anchor: (u64, u32, bool),
    /// The selection's end, as `anchor` gives its start.
    pub focus: (u64, u32, bool),
    /// How far the pages are scrolled, in layout units.
    pub scroll: i64,
    /// Where Up and Down aim across lines, if a run of them is going.
    pub goal: Option<i64>,
    /// Whether a run of typing is going, so what's typed next undoes with
    /// it.
    pub typing: bool,
    /// Properties for the next text typed, set by Bold and the like.
    pub pending: doc::Props,
    /// The window's chrome as it was: the story as a draft, the inspector
    /// open, the ruler shown, dark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<Chrome>,
}

/// What a window showed when a recorded session starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Chrome {
    /// Whether the story was shown as a draft, not on pages.
    pub draft: bool,
    /// Whether the inspector was open, and its tab.
    pub inspector: Option<usize>,
    /// Whether the ruler was shown.
    pub ruler: bool,
    /// Whether the window was dark.
    pub dark: bool,
    /// The unit the ruler measured in, which markers dragged on it snap
    /// to; centimetres if it's left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<chrome::Unit>,
}

/// One thing a person does.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Step {
    /// Types text.
    Type(String),
    /// Presses keys, as a shortcut reads: "Shift+Left", "Primary+B",
    /// "Enter".
    Key(String),
    /// Presses a key that many times.
    Repeat(String, usize),
    /// Runs a command by ID.
    Command(String),
    /// Clicks at a point, in logical pixels.
    Click(f64, f64),
    /// Clicks with Shift held, extending the selection.
    ShiftClick(f64, f64),
    /// Clicks twice in one place, selecting a word.
    DoubleClick(f64, f64),
    /// Clicks three times in one place, selecting a sentence.
    TripleClick(f64, f64),
    /// Drags from one point to another.
    Drag((f64, f64), (f64, f64)),
    /// Moves the pointer.
    Hover(f64, f64),
    /// An input method composes text.
    Preedit(String),
    /// Scrolls, in logical pixels.
    Scroll(f64),
    /// Closes the document and opens it again.
    Reopen,
    /// Checks the paragraphs at this point, each one's style and text, as
    /// `expect` does at the end.
    Expect(Vec<(String, String)>),
    /// Checks the story's tree at this point, as `outline` does at the
    /// end.
    Outline(Vec<String>),
    /// Presses the pointer at a point, in logical pixels, with Shift held
    /// or not, as the given press of a run of quick presses in one place,
    /// as the shell reports it.
    Press(f64, f64, bool, u32),
    /// Lets the pointer's button go.
    Release,
    /// The window takes a size, in logical pixels.
    Resize(f64, f64),
    /// What the clipboard holds from here on, which pasting gives.
    Clipboard(String),
    /// A file chosen in the system's dialog: for a picture ("picture"), a
    /// document to open ("open") or where to move one ("move-to"), and the
    /// file, if one was chosen.
    Chosen(String, Option<String>),
    /// Runs a command with arguments: text, or a whole number.
    CommandWith(String, Vec<SessionArg>),
    /// The system's look turns dark, or light.
    Dark(bool),
    /// The window is maximised, or restored.
    Maximized(bool),
    /// Answers the last file the window asked for, in the system's dialog:
    /// a file in the documents folder, or none.
    Choose(Option<String>),
    /// Checks the document's name, as the title bar shows it, at this
    /// point.
    Name(String),
    /// Checks what a file in the documents folder holds at this point: its
    /// name, and its text.
    Saved(String, String),
    /// Checks what the status bar says of the last thing done, at this
    /// point.
    Message(String),
    /// The window comes forward, as when the person switches back to it.
    Focused,
    /// Another program changes a file in the documents folder: its name,
    /// and what it holds after. Give it another size than before, as the
    /// system's clock may tell a file's times apart only by the second.
    Change(String, String),
}

/// A command's argument, as a session writes it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum SessionArg {
    /// Text.
    Text(String),
    /// A whole number.
    Int(i64),
}

impl Step {
    /// The step that gives `event`, as the session recorder writes it;
    /// none for what a step doesn't give, text pasted, which comes from
    /// the clipboard ([`Step::Clipboard`]).
    pub fn of(event: &Event) -> Option<Step> {
        Some(match event {
            Event::Key { key, modifiers } => Step::Key(key_name(*key, *modifiers)),
            Event::Text(text) => Step::Type(text.clone()),
            Event::Preedit(text) => Step::Preedit(text.clone()),
            Event::Press { x, y, shift, count } => Step::Press(*x, *y, *shift, *count),
            Event::Move { x, y } => Step::Hover(*x, *y),
            Event::Release => Step::Release,
            Event::Scroll(dy) => Step::Scroll(*dy),
            Event::Resize { width, height } => Step::Resize(*width, *height),
            Event::Command(id) => Step::Command(id.clone()),
            Event::CommandWith(id, args) => Step::CommandWith(
                id.clone(),
                args.iter()
                    .map(|a| match a {
                        chrome::Arg::Text(t) => SessionArg::Text(t.clone()),
                        chrome::Arg::Int(n) => SessionArg::Int(*n),
                    })
                    .collect(),
            ),
            Event::Theme { dark } => Step::Dark(*dark),
            Event::Maximized(maximized) => Step::Maximized(*maximized),
            Event::Focused => Step::Focused,
            Event::Pasted(_) => return None,
            Event::Chosen(choice, path) => Step::Chosen(
                choice_name(*choice).to_string(),
                path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            ),
        })
    }
}

/// A choice as a session names it.
fn choice_name(choice: Choice) -> &'static str {
    match choice {
        Choice::Open => "open",
        Choice::MoveTo => "move-to",
        Choice::Picture => "picture",
    }
}

/// The choice a session names `name`.
fn choice_of(name: &str) -> Option<Choice> {
    match name {
        "open" => Some(Choice::Open),
        "move-to" => Some(Choice::MoveTo),
        "picture" => Some(Choice::Picture),
        _ => None,
    }
}

/// A key and its modifiers, as a shortcut reads: "Primary+Shift+Left".
/// The plus key is "Plus".
fn key_name(key: Key, modifiers: Modifiers) -> String {
    let mut out = String::new();
    for (held, name) in [
        (modifiers.primary, "Primary+"),
        (modifiers.shift, "Shift+"),
        (modifiers.alt, "Alt+"),
    ] {
        if held {
            out.push_str(name);
        }
    }
    match key {
        Key::Char('+') => out.push_str("Plus"),
        Key::Char(c) => out.push(c),
        other => out.push_str(&format!("{other:?}")),
    }
    out
}

/// A session played: the window as it ends, the clipboard, and what was
/// asked of the window itself.
pub struct Played {
    /// The window.
    pub window: Window,
    /// The clipboard's text.
    pub clipboard: String,
    /// What was wrong at the session's [`Step::Expect`]s.
    pub problems: Vec<String>,
    /// What was asked of the window itself, in order.
    pub actions: Vec<crate::WindowAction>,
}

impl Played {
    /// The document's part.
    pub fn editor(&self) -> &Editor {
        self.window.editor()
    }
}

/// Plays `session` into a new document in `res`'s documents folder: an
/// empty one, or the one it starts from. It's played as by a person who
/// hasn't chosen any settings, such as the ruler's unit, or as the
/// recorded window had them where it starts.
pub fn play(res: Arc<Resources>, session: &Session) -> Result<Played, EditorError> {
    let mut res = (*res).clone();
    if let Some(tag) = &session.language {
        res.language = text::Language::parse(tag).unwrap_or_default();
    }
    res.mac = session.mac;
    res.settings = Arc::new(crate::Settings::default());
    // A Markdown file is written only when a step asks, Saved or Reopen,
    // and at the end, not a moment after an edit, so a session plays the
    // same however fast it's played.
    res.editing.markdown_saved_after = 3_600_000;
    let res = Arc::new(res);
    let folder_error = |path: &std::path::Path, source| EditorError::Io {
        path: path.to_path_buf(),
        source,
    };
    for (from, to) in &session.copy {
        let to = res.documents.join(to);
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(|e| folder_error(folder, e))?;
        }
        std::fs::copy(res.root.join(from), &to).map_err(|e| folder_error(&to, e))?;
    }
    let mut window = match (&session.start, &session.open) {
        (Some(start), _) => Window::resume(res.clone(), start, session.size)?,
        (None, Some((name, text))) => {
            let path = res.documents.join(name);
            std::fs::create_dir_all(&res.documents).map_err(|e| folder_error(&res.documents, e))?;
            std::fs::write(&path, text).map_err(|e| folder_error(&path, e))?;
            Window::open(res.clone(), &path)?
        }
        (None, None) => Window::create(res.clone())?,
    };
    let mut clipboard = String::new();
    let mut problems = Vec::new();
    let mut actions = Vec::new();
    let mut choosing: Option<Choice> = None;
    let (mut width, mut height) = session.size;
    window.update(Event::Resize { width, height });
    for (n, step) in session.events.iter().enumerate() {
        let events = match step {
            Step::Type(text) => vec![Event::Text(text.clone())],
            Step::Key(keys) => vec![key(keys)],
            Step::Repeat(keys, n) => vec![key(keys); *n],
            Step::Command(id) => vec![Event::Command(id.clone())],
            Step::Click(x, y) => clicks(*x, *y, false, 1),
            Step::ShiftClick(x, y) => clicks(*x, *y, true, 1),
            Step::DoubleClick(x, y) => clicks(*x, *y, false, 2),
            Step::TripleClick(x, y) => clicks(*x, *y, false, 3),
            Step::Drag((x1, y1), (x2, y2)) => vec![
                Event::Press {
                    x: *x1,
                    y: *y1,
                    shift: false,
                    count: 1,
                },
                Event::Move { x: *x2, y: *y2 },
                Event::Release,
            ],
            Step::Hover(x, y) => vec![Event::Move { x: *x, y: *y }],
            Step::Preedit(text) => vec![Event::Preedit(text.clone())],
            Step::Scroll(dy) => vec![Event::Scroll(*dy)],
            Step::Reopen => {
                window.close();
                let path = window.editor().path().to_path_buf();
                drop(window);
                window = Window::open(res.clone(), &path)?;
                window.update(Event::Resize { width, height });
                Vec::new()
            }
            Step::Expect(want) => {
                let got = paragraphs(window.editor().document());
                if &got != want {
                    problems.push(format!(
                        "at step {n}, paragraphs {got:?}, expected {want:?}"
                    ));
                }
                Vec::new()
            }
            Step::Outline(want) => {
                let got = story::outline(window.editor().document(), window.editor().sheet());
                if &got != want {
                    problems.push(format!(
                        "at step {n}, the outline is {got:#?}, expected {want:#?}"
                    ));
                }
                Vec::new()
            }
            Step::Press(x, y, shift, count) => vec![Event::Press {
                x: *x,
                y: *y,
                shift: *shift,
                count: *count,
            }],
            Step::Release => vec![Event::Release],
            Step::Resize(w, h) => {
                (width, height) = (*w, *h);
                vec![Event::Resize { width, height }]
            }
            Step::Clipboard(text) => {
                clipboard = text.clone();
                Vec::new()
            }
            Step::Chosen(choice, path) => match choice_of(choice) {
                Some(choice) => vec![Event::Chosen(choice, path.as_ref().map(PathBuf::from))],
                None => {
                    problems.push(format!("at step {n}, nothing is chosen for {choice:?}"));
                    Vec::new()
                }
            },
            Step::CommandWith(id, args) => vec![Event::CommandWith(
                id.clone(),
                args.iter()
                    .map(|a| match a {
                        SessionArg::Text(t) => chrome::Arg::Text(t.clone()),
                        SessionArg::Int(n) => chrome::Arg::Int(*n),
                    })
                    .collect(),
            )],
            Step::Dark(dark) => vec![Event::Theme { dark: *dark }],
            Step::Maximized(maximized) => vec![Event::Maximized(*maximized)],
            Step::Choose(file) => match choosing.take() {
                Some(choice) => vec![Event::Chosen(
                    choice,
                    file.as_ref().map(|f| res.documents.join(f)),
                )],
                None => {
                    problems.push(format!("at step {n}, nothing asked for a file"));
                    Vec::new()
                }
            },
            Step::Name(want) => {
                let got = window.editor().name();
                if &got != want {
                    problems.push(format!(
                        "at step {n}, the name is {got:?}, expected {want:?}"
                    ));
                }
                Vec::new()
            }
            Step::Saved(name, want) => {
                // What's waiting to be written, written, as a moment
                // passing would.
                window.flush();
                let got = std::fs::read(res.documents.join(name))
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                match got {
                    Ok(got) if &got == want => {}
                    Ok(got) => problems.push(format!(
                        "at step {n}, {name} holds {got:?}, expected {want:?}"
                    )),
                    Err(e) => problems.push(format!("at step {n}, {name}: {e}")),
                }
                Vec::new()
            }
            Step::Message(want) => {
                let got = window.editor().message();
                if got != want {
                    problems.push(format!(
                        "at step {n}, the status bar says {got:?}, expected {want:?}"
                    ));
                }
                Vec::new()
            }
            Step::Focused => vec![Event::Focused],
            Step::Change(name, text) => {
                let path = res.documents.join(name);
                if let Err(e) = std::fs::write(&path, text) {
                    problems.push(format!("at step {n}, {name}: {e}"));
                }
                Vec::new()
            }
        };
        for event in events {
            let mut queue = window.update(event);
            // What the work away from the window has to say, as the shell
            // asks before it waits.
            window.poll();
            while let Some(effect) = queue.pop() {
                match effect {
                    Effect::Copy(text) => clipboard = text,
                    Effect::Paste => queue.extend(window.update(Event::Pasted(clipboard.clone()))),
                    Effect::Choose(choice) => choosing = Some(choice),
                    Effect::Window(action) => actions.push(action),
                    Effect::Open(_)
                    | Effect::OpenVersion(..)
                    | Effect::OpenExternally(_)
                    | Effect::Reveal(_)
                    | Effect::Quit
                    | Effect::Pointer(_) => {}
                }
            }
        }
    }
    // What's waiting to be written, written, as a moment passing would.
    window.flush();
    window.poll();
    Ok(Played {
        window,
        clipboard,
        problems,
        actions,
    })
}

/// `count` clicks at (`x`, `y`), as the shell reports them: each press
/// numbered in its run.
fn clicks(x: f64, y: f64, shift: bool, count: u32) -> Vec<Event> {
    (1..=count)
        .flat_map(|n| {
            [
                Event::Press {
                    x,
                    y,
                    shift,
                    count: n,
                },
                Event::Release,
            ]
        })
        .collect()
}

/// The key event a shortcut-like string names; "Plus" is the plus key.
fn key(keys: &str) -> Event {
    // The modifiers before "Plus" end in "+", if there are any.
    let parsed = match keys.strip_suffix("Plus") {
        Some(mods) => Shortcut::parse(&format!("{mods}x")).map(|s| Shortcut {
            key: Key::Char('+'),
            ..s
        }),
        None => Shortcut::parse(keys),
    };
    let shortcut = parsed.unwrap_or(Shortcut {
        modifiers: Modifiers::default(),
        key: Key::Escape,
    });
    Event::Key {
        key: shortcut.key,
        modifiers: shortcut.modifiers,
    }
}

/// Everything wrong with how `played` ended, against what `session`
/// expects.
pub fn check(session: &Session, played: &Played) -> Vec<String> {
    let mut problems = played.problems.clone();
    let editor = played.editor();
    let document = editor.document();
    let got = paragraphs(document);
    if got != session.expect {
        problems.push(format!("paragraphs {got:?}, expected {:?}", session.expect));
    }
    if let Some(want) = &session.outline {
        let outline = story::outline(document, editor.sheet());
        if &outline != want {
            problems.push(format!("the outline is {outline:#?}, expected {want:#?}"));
        }
    }
    let marked = session.marks.iter().map(|m| (m, true));
    let unmarked = session.unmarked.iter().map(|m| (m, false));
    for ((stretch, keys), want) in marked.chain(unmarked) {
        let Some((id, start)) = document.paragraphs().iter().find_map(|&id| {
            let text = &document.paragraph(id)?.text;
            text.find(stretch.as_str())
                .map(|b| (id, text[..b].chars().count() as u32))
        }) else {
            problems.push(format!("{stretch:?} isn't in the document"));
            continue;
        };
        let p = document.paragraph(id).expect("a paragraph");
        let end = start + stretch.chars().count() as u32;
        let mut pos = 0;
        for run in &p.runs {
            let (s, e) = (pos, pos + run.len);
            pos = e;
            if e <= start || s >= end {
                continue;
            }
            for key in keys {
                let set = run.props.get(key).is_some_and(|v| *v != Value::Bool(false));
                if set != want {
                    let not = if want { "isn't all" } else { "is partly" };
                    problems.push(format!("{stretch:?} {not} {key}"));
                }
            }
        }
    }
    for (stretch, want) in &session.languages {
        let Some((id, start)) = document.paragraphs().iter().find_map(|&id| {
            let text = &document.paragraph(id)?.text;
            text.find(stretch.as_str())
                .map(|b| (id, text[..b].chars().count() as u32))
        }) else {
            problems.push(format!("{stretch:?} isn't in the document"));
            continue;
        };
        let p = document.paragraph(id).expect("a paragraph");
        let tag = |props: &doc::Props| match props.get(story::keys::LANGUAGE) {
            Some(Value::Text(tag)) => text::Language::parse(tag).ok(),
            _ => None,
        };
        let want = text::Language::parse(want).ok();
        let end = start + stretch.chars().count() as u32;
        let mut pos = 0;
        for run in &p.runs {
            let (s, e) = (pos, pos + run.len);
            pos = e;
            if e <= start || s >= end {
                continue;
            }
            let got = tag(&run.props).or_else(|| tag(&p.props));
            if got != want {
                problems.push(format!(
                    "{stretch:?} is in {}, expected {want:?}",
                    got.map_or("no language".to_string(), |l| l.to_string())
                ));
                break;
            }
        }
    }
    if let Some(want) = &session.clipboard
        && &played.clipboard != want
    {
        problems.push(format!(
            "the clipboard holds {:?}, expected {want:?}",
            played.clipboard
        ));
    }
    if let Some(want) = &session.status {
        let got = editor.status();
        if &got != want {
            problems.push(format!("the status bar says {got:?}, expected {want:?}"));
        }
    }
    if let Some(want) = &session.name
        && &editor.name() != want
    {
        problems.push(format!(
            "the name is {:?}, expected {want:?}",
            editor.name()
        ));
    }
    if let Some(want) = &session.file {
        let got = editor.stem();
        if &got != want {
            problems.push(format!("the file is {got:?}, expected {want:?}"));
        }
    }
    if let Some(want) = &session.actions {
        let got: Vec<String> = played.actions.iter().map(|a| format!("{a:?}")).collect();
        if &got != want {
            problems.push(format!(
                "the window was asked to {got:?}, expected {want:?}"
            ));
        }
    }
    for (id, on) in session
        .on
        .iter()
        .map(|id| (id, true))
        .chain(session.off.iter().map(|id| (id, false)))
    {
        if played.window.state(id).checked != on {
            problems.push(format!("{id} is {}, expected {}", !on, on));
        }
    }
    for (name, want) in &session.files {
        let path = editor.resources().documents.join(name);
        match (std::fs::read(&path), want) {
            (Err(e), _) => problems.push(format!("{name}: {e}")),
            (Ok(got), Some(want)) if String::from_utf8_lossy(&got) != *want => {
                problems.push(format!(
                    "{name} holds {:?}, expected {want:?}",
                    String::from_utf8_lossy(&got)
                ))
            }
            _ => {}
        }
    }
    for (stretch, key, want) in &session.props {
        let Some(id) = document.paragraphs().into_iter().find(|&id| {
            document
                .paragraph(id)
                .is_some_and(|p| p.text.contains(stretch.as_str()))
        }) else {
            problems.push(format!("{stretch:?} isn't in the document"));
            continue;
        };
        let got = document
            .paragraph(id)
            .and_then(|p| p.props.get(key.as_str()))
            .map(|v| match v {
                Value::Text(t) => t.clone(),
                Value::Int(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Ref(id) => id.to_string(),
            });
        if &got != want {
            problems.push(format!("{stretch:?}'s {key} is {got:?}, expected {want:?}"));
        }
    }
    problems
}

/// Each paragraph's style, by its name in the document's stylesheet, or
/// nothing for a paragraph that names none, and its text.
pub(crate) fn paragraphs(document: &doc::Document) -> Vec<(String, String)> {
    document
        .paragraphs()
        .iter()
        .map(|&id| {
            let p = document.paragraph(id).expect("a paragraph");
            let style = match p.props.get(story::keys::STYLE) {
                Some(Value::Ref(style)) => match document
                    .style(*style)
                    .and_then(|s| s.props.get(story::keys::NAME))
                {
                    Some(Value::Text(name)) => name.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            (style, p.text.clone())
        })
        .collect()
}

/// Reads a session from its RON.
pub fn read(source: &str) -> Result<Session, String> {
    ron::from_str(source).map_err(|e| e.to_string())
}

/// `session` as RON, as [`read`] reads it back.
pub fn write(session: &Session) -> Result<String, String> {
    let config = ron::ser::PrettyConfig::default().struct_names(false);
    ron::ser::to_string_pretty(session, config).map_err(|e| e.to_string())
}
