//! Scripted sessions (`spec/sessions/`): input played into an editor, as a
//! person would give it, for tests and for `shot`'s frames.

use std::sync::Arc;

use chrome::{Key, Modifiers, Shortcut};
use doc::Value;
use serde::Deserialize;

use crate::{Editor, EditorError, Effect, Event, Resources};

/// A session as `spec/sessions/` gives it.
#[derive(Clone, Debug, Deserialize)]
pub struct Session {
    /// The window's size, in logical pixels.
    pub size: (f64, f64),
    /// What the person does, in order.
    pub events: Vec<Step>,
    /// The paragraphs afterwards: each one's style and text.
    pub expect: Vec<(String, String)>,
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

/// Plays `session` into a new document in `res`'s documents folder.
pub fn play(res: Arc<Resources>, session: &Session) -> Result<Played, EditorError> {
    let mut editor = Editor::create(res.clone())?;
    let mut clipboard = String::new();
    let mut problems = Vec::new();
    let (width, height) = session.size;
    editor.update(Event::Resize { width, height });
    for (n, step) in session.events.iter().enumerate() {
        let events = match step {
            Step::Type(text) => vec![Event::Text(text.clone())],
            Step::Key(keys) => vec![key(keys)],
            Step::Repeat(keys, n) => vec![key(keys); *n],
            Step::Command(id) => vec![Event::Command(id.clone())],
            Step::Click(x, y) => vec![
                Event::Press {
                    x: *x,
                    y: *y,
                    shift: false,
                },
                Event::Release,
            ],
            Step::ShiftClick(x, y) => vec![
                Event::Press {
                    x: *x,
                    y: *y,
                    shift: true,
                },
                Event::Release,
            ],
            Step::Drag((x1, y1), (x2, y2)) => vec![
                Event::Press {
                    x: *x1,
                    y: *y1,
                    shift: false,
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
        };
        for event in events {
            let mut queue = editor.update(event);
            while let Some(effect) = queue.pop() {
                match effect {
                    Effect::Copy(text) => clipboard = text,
                    Effect::Paste => queue.extend(editor.update(Event::Pasted(clipboard.clone()))),
                    Effect::Open(_)
                    | Effect::OpenExternally(_)
                    | Effect::Reveal(_)
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
    let document = played.editor.document();
    let got = paragraphs(document);
    if got != session.expect {
        problems.push(format!("paragraphs {got:?}, expected {:?}", session.expect));
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
