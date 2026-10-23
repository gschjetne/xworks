//! Scripted sessions (`spec/sessions/`): input played into a window, as a
//! person would give it, chrome and all, for tests and for `shot`'s frames.

use std::sync::Arc;

use chrome::{Key, Modifiers, Shortcut};
use doc::Value;
use serde::Deserialize;

use crate::{Choice, Editor, EditorError, Effect, Event, Resources, Window};

/// A session as `spec/sessions/` gives it.
#[derive(Clone, Debug, Deserialize)]
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
    #[serde(default)]
    pub outline: Option<Vec<String>>,
    /// Stretches of text, each with the properties set on every character
    /// of it.
    #[serde(default)]
    pub marks: Vec<(String, Vec<String>)>,
    /// What the clipboard holds afterwards, if the session copies.
    #[serde(default)]
    pub clipboard: Option<String>,
    /// What the status bar says afterwards.
    #[serde(default)]
    pub status: Option<String>,
    /// The system's language, which the new document is in, as a BCP 47
    /// tag; none if it's left out.
    #[serde(default)]
    pub language: Option<String>,
    /// Stretches of text, each with the language every character of it is
    /// in, as a BCP 47 tag: its own, or its paragraph's.
    #[serde(default)]
    pub languages: Vec<(String, String)>,
    /// Whether it's played as on a Mac: its shortcuts, its words, and no
    /// access keys.
    #[serde(default)]
    pub mac: bool,
    /// The document's name afterwards, as the title bar shows it.
    #[serde(default)]
    pub name: Option<String>,
    /// The name of its file afterwards, without the folder or extension.
    #[serde(default)]
    pub file: Option<String>,
    /// What's asked of the window itself, in order, as their names: Drag,
    /// ToggleMaximize, Minimize, Close, Resize(..).
    #[serde(default)]
    pub actions: Option<Vec<String>>,
    /// Commands that are on afterwards, such as the inspector's.
    #[serde(default)]
    pub on: Vec<String>,
    /// Commands that are off afterwards.
    #[serde(default)]
    pub off: Vec<String>,
    /// Paragraph properties afterwards: a stretch of the paragraph's text,
    /// the property, and its value as text, or none for unset.
    #[serde(default)]
    pub props: Vec<(String, String, Option<String>)>,
}

/// One thing a person does.
#[derive(Clone, Debug, Deserialize)]
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
    /// Runs a command with arguments: text, or a whole number.
    CommandWith(String, Vec<SessionArg>),
    /// The system's look turns dark, or light.
    Dark(bool),
    /// Answers the last file the window asked for, in the system's dialog:
    /// a file in the documents folder, or none.
    Choose(Option<String>),
    /// Checks the document's name, as the title bar shows it, at this
    /// point.
    Name(String),
}

/// A command's argument, as a session writes it.
#[derive(Clone, Debug, Deserialize)]
pub enum SessionArg {
    /// Text.
    Text(String),
    /// A whole number.
    Int(i64),
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

/// Plays `session` into a new document in `res`'s documents folder.
pub fn play(res: Arc<Resources>, session: &Session) -> Result<Played, EditorError> {
    let mut res = (*res).clone();
    if let Some(tag) = &session.language {
        res.language = text::Language::parse(tag).unwrap_or_default();
    }
    res.mac = session.mac;
    let res = Arc::new(res);
    let mut window = Window::create(res.clone())?;
    let mut clipboard = String::new();
    let mut problems = Vec::new();
    let mut actions = Vec::new();
    let mut choosing: Option<Choice> = None;
    let (width, height) = session.size;
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
                let got = story::outline(window.editor().document(), &res.sheet);
                if &got != want {
                    problems.push(format!(
                        "at step {n}, the outline is {got:#?}, expected {want:#?}"
                    ));
                }
                Vec::new()
            }
        };
        for event in events {
            let mut queue = window.update(event);
            while let Some(effect) = queue.pop() {
                match effect {
                    Effect::Copy(text) => clipboard = text,
                    Effect::Paste => queue.extend(window.update(Event::Pasted(clipboard.clone()))),
                    Effect::Choose(choice) => choosing = Some(choice),
                    Effect::Window(action) => actions.push(action),
                    Effect::Open(_)
                    | Effect::OpenExternally(_)
                    | Effect::Reveal(_)
                    | Effect::Quit
                    | Effect::Pointer(_) => {}
                }
            }
        }
    }
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

/// The key event a shortcut-like string names.
fn key(keys: &str) -> Event {
    let shortcut = Shortcut::parse(keys).unwrap_or(Shortcut {
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
        let outline = story::outline(document, &editor.resources().sheet);
        if &outline != want {
            problems.push(format!("the outline is {outline:#?}, expected {want:#?}"));
        }
    }
    for (stretch, keys) in &session.marks {
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
                if !run.props.get(key).is_some_and(|v| *v != Value::Bool(false)) {
                    problems.push(format!("{stretch:?} isn't all {key}"));
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
        let got = editor.title();
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
            });
        if &got != want {
            problems.push(format!("{stretch:?}'s {key} is {got:?}, expected {want:?}"));
        }
    }
    problems
}

/// Each paragraph's style and text.
fn paragraphs(document: &doc::Document) -> Vec<(String, String)> {
    document
        .paragraphs()
        .iter()
        .map(|&id| {
            let p = document.paragraph(id).expect("a paragraph");
            let style = match p.props.get(story::keys::STYLE) {
                Some(Value::Text(name)) => name.clone(),
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
