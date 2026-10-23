//! Scripted sessions (`spec/sessions/`): input played into an editor, as a
//! person would give it, for tests and for `shot`'s frames, and as the
//! session recorder writes what a person did ([`crate::recorder`]).

use std::path::PathBuf;
use std::sync::Arc;

use chrome::{Key, Modifiers, Shortcut};
use doc::Value;
use serde::{Deserialize, Serialize};

use crate::{Choice, Editor, EditorError, Effect, Event, Resources};

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

/// A session played: the editor as it ends, and the clipboard.
pub struct Played {
    /// The editor.
    pub editor: Editor,
    /// The clipboard's text.
    pub clipboard: String,
    /// What was wrong at the session's [`Step::Expect`]s.
    pub problems: Vec<String>,
}

/// Plays `session` into a new document in `res`'s documents folder: an
/// empty one, or the one it starts from.
pub fn play(res: Arc<Resources>, session: &Session) -> Result<Played, EditorError> {
    let res = if session.language.is_some() || session.mac != res.mac {
        let mut res = (*res).clone();
        if let Some(tag) = &session.language {
            res.language = text::Language::parse(tag).unwrap_or_default();
        }
        res.mac = session.mac;
        Arc::new(res)
    } else {
        res
    };
    let mut editor = match &session.start {
        Some(start) => Editor::resume(res.clone(), start, session.size)?,
        None => Editor::create(res.clone())?,
    };
    let mut clipboard = String::new();
    let mut problems = Vec::new();
    let (mut width, mut height) = session.size;
    editor.update(Event::Resize { width, height });
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
                let path = editor.path().to_path_buf();
                drop(editor);
                editor = Editor::open(res.clone(), &path)?;
                editor.update(Event::Resize { width, height });
                Vec::new()
            }
            Step::Expect(want) => {
                let got = paragraphs(editor.document());
                if &got != want {
                    problems.push(format!(
                        "at step {n}, paragraphs {got:?}, expected {want:?}"
                    ));
                }
                Vec::new()
            }
            Step::Outline(want) => {
                let got = story::outline(editor.document(), &res.sheet);
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
        };
        for event in events {
            let mut queue = editor.update(event);
            while let Some(effect) = queue.pop() {
                match effect {
                    Effect::Copy(text) => clipboard = text,
                    Effect::Paste => queue.extend(editor.update(Event::Pasted(clipboard.clone()))),
                    Effect::Open(_)
                    | Effect::OpenVersion(..)
                    | Effect::OpenExternally(_)
                    | Effect::Reveal(_)
                    | Effect::Choose(_)
                    | Effect::Quit => {}
                }
            }
        }
    }
    Ok(Played {
        editor,
        clipboard,
        problems,
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
    let document = played.editor.document();
    let got = paragraphs(document);
    if got != session.expect {
        problems.push(format!("paragraphs {got:?}, expected {:?}", session.expect));
    }
    if let Some(want) = &session.outline {
        let outline = story::outline(document, &played.editor.resources().sheet);
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
        let got = played.editor.status();
        if &got != want {
            problems.push(format!("the status bar says {got:?}, expected {want:?}"));
        }
    }
    problems
}

/// Each paragraph's style and text.
pub(crate) fn paragraphs(document: &doc::Document) -> Vec<(String, String)> {
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

/// `session` as RON, as [`read`] reads it back.
pub fn write(session: &Session) -> Result<String, String> {
    let config = ron::ser::PrettyConfig::default().struct_names(false);
    ron::ser::to_string_pretty(session, config).map_err(|e| e.to_string())
}
