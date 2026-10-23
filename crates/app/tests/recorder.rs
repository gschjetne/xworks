//! The session recorder (docs/ROADMAP.md, M2 stage 7; project owner,
//! review a23vvez8): what a person does in a window is kept, the last few
//! minutes of it, and written as a session that plays back headless to the
//! same document.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use app::recorder::{self, Recorder, Recordings, Spec};
use app::{Choice, Effect, Event, Resources, Window, session};
use chrome::{Key, Modifiers, Shortcut};

fn resources(name: &str) -> Arc<Resources> {
    static RES: OnceLock<Resources> = OnceLock::new();
    let res = RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    });
    let mut res = res.clone();
    res.documents = testkit::scratch_dir(&format!("app-recorder-{name}")).expect("a folder");
    Arc::new(res)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pictures")
        .join(name)
}

/// A window as a shell drives it: events in, effects carried out, the
/// clipboard its own.
struct Shell {
    window: Window,
    clipboard: String,
}

impl Shell {
    fn feed(&mut self, event: Event) {
        let mut effects = self.window.update(event);
        while let Some(effect) = effects.pop() {
            match effect {
                Effect::Copy(text) => self.clipboard = text,
                Effect::Paste => {
                    effects.extend(self.window.update(Event::Pasted(self.clipboard.clone())))
                }
                _ => {}
            }
        }
    }

    fn key(&mut self, keys: &str) {
        let shortcut = Shortcut::parse(keys).expect("a shortcut");
        self.feed(Event::Key {
            key: shortcut.key,
            modifiers: shortcut.modifiers,
        });
    }

    fn click(&mut self, x: f64, y: f64, count: u32) {
        for n in 1..=count {
            self.feed(Event::Press {
                x,
                y,
                shift: false,
                count: n,
            });
            self.feed(Event::Release);
        }
    }
}

/// Something a person does in a window.
type Doing = Box<dyn Fn(&mut Shell)>;

/// Everything a person might do, as events, and the time each is done
/// after the one before.
fn everything(window: &mut Shell, recorder: &Recorder, tick: Duration) {
    let gradient = fixture("gradient.jpg");
    let steps: Vec<Doing> = vec![
        Box::new(|w| {
            w.feed(Event::Resize {
                width: 900.0,
                height: 700.0,
            })
        }),
        Box::new(|w| w.feed(Event::Text("Hello world".to_string()))),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.feed(Event::Text("Second line".to_string()))),
        Box::new(|w| w.key("Shift+Home")),
        Box::new(|w| w.key("Primary+B")),
        Box::new(|w| w.key("End")),
        Box::new(|w| w.feed(Event::Text(" more".to_string()))),
        Box::new(|w| w.key("Primary+Z")),
        Box::new(|w| w.key("Primary+Shift+Z")),
        Box::new(|w| w.click(300.0, 125.0, 1)),
        Box::new(|w| w.feed(Event::Text("[clicked]".to_string()))),
        Box::new(|w| w.click(250.0, 125.0, 2)),
        Box::new(|w| w.key("Primary+C")),
        Box::new(|w| w.key("Primary+End")),
        Box::new(|w| w.key("Primary+V")),
        Box::new(|w| w.feed(Event::Scroll(120.0))),
        Box::new(|w| w.feed(Event::Scroll(-120.0))),
        Box::new(|w| w.feed(Event::Preedit("か".to_string()))),
        Box::new(|w| w.feed(Event::Preedit("かな".to_string()))),
        Box::new(|w| w.feed(Event::Text("かな".to_string()))),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.feed(Event::Command("list-bullets".to_string()))),
        Box::new(|w| w.feed(Event::Text("An item".to_string()))),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.key("Tab")),
        Box::new(|w| w.feed(Event::Text("Nested".to_string()))),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.key("Enter")),
        Box::new(|w| w.feed(Event::Command("insert-picture".to_string()))),
        Box::new(move |w| {
            w.feed(Event::Chosen(Choice::Picture, Some(gradient.clone())));
        }),
        Box::new(|w| w.feed(Event::Move { x: 110.0, y: 13.0 })),
        Box::new(|w| w.feed(Event::Text(" after it".to_string()))),
        Box::new(|w| w.key("Primary+A")),
        Box::new(|w| w.key("Primary+X")),
        Box::new(|w| w.key("Primary+V")),
        Box::new(|w| w.key("Primary+V")),
        Box::new(|w| w.key("Primary+Z")),
        Box::new(|w| {
            w.feed(Event::Resize {
                width: 700.0,
                height: 500.0,
            })
        }),
        Box::new(|w| w.feed(Event::Command("align-center".to_string()))),
        Box::new(|w| w.key("Primary+Home")),
        Box::new(|w| w.key("Shift+Down")),
        Box::new(|w| w.key("Backspace")),
    ];
    for step in steps {
        recorder.tick(tick);
        step(window);
    }
}

/// Plays `session` into its own folder, as a bug report would be.
fn play_back(session: &session::Session, name: &str) -> session::Played {
    let played = session::play(resources(name), session).expect("the session plays");
    let problems = session::check(session, &played);
    assert!(problems.is_empty(), "{problems:#?}");
    played
}

#[test]
fn a_recorded_session_plays_back_to_the_same_document() {
    // Kept whole, and kept from a checkpoint taken at almost every step:
    // either plays back to the same document.
    for (tick, minutes, name) in [
        (Duration::ZERO, 5, "whole"),
        (Duration::from_secs(7), 1, "trimmed"),
    ] {
        let res = resources(name);
        let recorder = Recorder::told(&Spec {
            minutes,
            checkpoint_seconds: 1,
        });
        let mut window = Shell {
            window: Window::create(res).expect("a document"),
            clipboard: String::new(),
        };
        window.window.record(recorder.clone());
        everything(&mut window, &recorder, tick);

        let session = recorder.session().expect("a session");
        let written = session::write(&session).expect("it writes");
        let read = session::read(&written).expect("it reads back");
        if minutes == 1 {
            // A minute and a few seconds of what was done 7 seconds apart,
            // a click two steps, a double click four.
            assert!(
                (8..=20).contains(&read.events.len()),
                "{} steps kept",
                read.events.len()
            );
        } else {
            assert!(read.events.len() > 40, "{} steps kept", read.events.len());
        }
        let played = play_back(&read, &format!("{name}-played"));
        assert_eq!(
            played.editor().document().dump(),
            window.window.editor().document().dump(),
            "{name}: played back otherwise"
        );
    }
}

#[test]
fn a_run_of_typing_cut_by_what_was_kept_still_undoes_whole() {
    let res = resources("run");
    let recorder = Recorder::told(&Spec {
        minutes: 1,
        checkpoint_seconds: 10,
    });
    let mut window = Shell {
        window: Window::create(res).expect("a document"),
        clipboard: String::new(),
    };
    window.window.record(recorder.clone());
    window.feed(Event::Text("Before. ".to_string()));
    window.key("Left");
    window.key("Right");
    // Five minutes of typing, a letter a second, then Undo, which takes
    // the whole run back.
    for n in 0..300 {
        recorder.tick(Duration::from_secs(1));
        let letter = char::from(b'a' + (n % 26) as u8);
        window.feed(Event::Text(letter.to_string()));
    }
    window.key("Primary+Z");
    let session = recorder.session().expect("a session");
    assert!(session.events.len() <= 72, "{} steps", session.events.len());
    let start = session.start.as_ref().expect("a start");
    assert!(start.typing, "it starts in the run");
    let played = play_back(&session, "run-played");
    assert_eq!(
        played.editor().document().dump(),
        window.window.editor().document().dump()
    );
}

#[test]
fn save_recent_session_writes_where_the_person_finds_it() {
    let res = resources("save");
    let documents = res.documents.clone();
    let mut editor = Window::create(res.clone()).expect("a document");
    editor.record(Recorder::new(&res.recorder));
    editor.update(Event::Text("Something went wrong here.".to_string()));
    editor.update(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    editor.update(Event::Command("save-session".to_string()));
    let folder = recorder::folder(&documents);
    // Named after the document, which is named after its first line
    // (review a23vvez8).
    let path = folder.join("Something went wrong here.ron");
    assert_eq!(
        editor.editor().message(),
        format!("Saved the last few minutes to {}", path.display())
    );
    let text = std::fs::read_to_string(&path).expect("the session");
    assert!(
        text.starts_with("// Saved by Save Recent Session.\n"),
        "{text}"
    );
    let session = session::read(&text).expect("it reads");
    assert!(
        !session
            .events
            .iter()
            .any(|s| matches!(s, session::Step::Command(id) if id == "save-session")),
        "saving isn't part of it"
    );
    let played = play_back(&session, "save-played");
    assert_eq!(
        played.editor().document().dump(),
        editor.editor().document().dump()
    );
    // Saved again, it takes another name.
    editor.update(Event::Command("save-session".to_string()));
    assert!(folder.join("Something went wrong here 2.ron").exists());
}

#[test]
fn when_xworks_fails_every_window_is_written_once() {
    let res = resources("failed");
    let folder = recorder::folder(&res.documents);
    let recordings = Recordings::default();
    let mut editors = Vec::new();
    for text in ["One window.", "Another."] {
        let mut editor = Window::create(res.clone()).expect("a document");
        let recorder = Recorder::new(&res.recorder);
        editor.record(recorder.clone());
        recordings.add(&recorder);
        editor.update(Event::Text(text.to_string()));
        editors.push(editor);
    }
    // A window closed is gone from them.
    {
        let mut closed = Window::create(res.clone()).expect("a document");
        let recorder = Recorder::new(&res.recorder);
        closed.record(recorder.clone());
        recordings.add(&recorder);
    }
    let written = recordings.write_all(&folder, "xWorks failed: a test\nat its end");
    assert_eq!(written.len(), 2, "{written:?}");
    for path in &written {
        let text = std::fs::read_to_string(path).expect("written");
        assert!(text.starts_with("// xWorks failed: a test\n// at its end\n"));
        session::read(&text).expect("a session");
    }
    assert!(recordings.write_all(&folder, "again").is_empty(), "once");
}
