//! The session recorder (project owner, 2026-10-23, review a23vvez8):
//! xWorks keeps the last few minutes of what a person does in a window, in
//! memory, as a session (`spec/sessions/`), and writes it out when asked
//! (Save Recent Session) or when xWorks fails, for bug reports. A
//! recorded session plays back headless ([`crate::session::play`]) to the
//! same document.
//!
//! What it keeps holds the keys pressed, and so the document's text. It's
//! kept in memory only, written only to a folder the person can find
//! ([`folder`]: "xWorks sessions" in the documents folder), and never sent
//! anywhere.
//!
//! How: every event an editor handles is a step. Now and then, when the
//! window is at rest (no menu, panel, drag or composition open), the
//! editor's state is noted as a checkpoint: its document and history,
//! which share what's unchanged, and where the caret is. What's kept
//! starts at the newest checkpoint at least [`Spec::minutes`] old, so a
//! session holds between that and a checkpoint's interval more; older
//! steps go. A session starts from its checkpoint, written whole.
//!
//! Invariants:
//!
//! - Text pasted is kept as what the clipboard held when the key or
//!   command that pasted it was pressed, so playing it back pastes it.
//! - No checkpoint falls between an effect and the event that answers it,
//!   a paste or a file chosen, so the answer is never cut from what it
//!   answers.
//! - Writing when xWorks fails takes no lock another thread holds, and
//!   writes each recording once.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use doc::{Document, History, Props};
use serde::Deserialize;
use story::StyleSheet;

use crate::session::{self, Session, Start, Step};
use crate::{Editor, Event};

/// How much the recorder keeps, from `spec/recorder.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Spec {
    /// How many minutes of what a person does it keeps, at least.
    pub minutes: u64,
    /// How often, in seconds, it notes where a session could start.
    pub checkpoint_seconds: u64,
}

/// The most steps kept between checkpoints: a window that's never at rest
/// for that long keeps nothing until it is again.
const MOST_STEPS: usize = 200_000;

/// The folder sessions are written to: "xWorks sessions" in the documents
/// folder `documents`, where the person finds their documents.
pub fn folder(documents: &Path) -> PathBuf {
    documents.join("xWorks sessions")
}

/// A window's recording, shared: cloning shares it.
#[derive(Clone)]
pub struct Recorder(Arc<Mutex<Recording>>);

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Recorder")
    }
}

/// An editor's state at rest, where a recorded session can start.
#[derive(Clone)]
pub(crate) struct Checkpoint {
    pub(crate) document: Document,
    pub(crate) history: History,
    pub(crate) anchor: (u64, u32, bool),
    pub(crate) focus: (u64, u32, bool),
    pub(crate) scroll: i64,
    pub(crate) goal: Option<i64>,
    pub(crate) typing: bool,
    pub(crate) pending: Props,
    /// The window's size, in logical pixels.
    pub(crate) size: (f64, f64),
}

impl Checkpoint {
    fn start(&self) -> Start {
        let (undo, redo) = self.history.stacks();
        Start {
            document: self.document.to_snapshot(),
            undo: undo.to_vec(),
            redo: redo.to_vec(),
            anchor: self.anchor,
            focus: self.focus,
            scroll: self.scroll,
            goal: self.goal,
            typing: self.typing,
            pending: self.pending.clone(),
        }
    }
}

enum Clock {
    /// Time since this, as it passes.
    Real(Instant),
    /// Time as it's told, for tests.
    Told(Duration),
}

struct Recording {
    keep: Duration,
    every: Duration,
    clock: Clock,
    /// The steps kept, the first of them the `first`th recorded.
    steps: VecDeque<Step>,
    first: u64,
    /// Where sessions can start: when, the index of the step after, and
    /// the editor's state. The first is where what's kept starts.
    checkpoints: VecDeque<(Duration, u64, Checkpoint)>,
    /// The document after the last event handled.
    ended: Option<Document>,
    sheet: Option<StyleSheet>,
    title: String,
    language: Option<String>,
    mac: bool,
}

impl Recorder {
    /// A recorder keeping what `spec` says, by the clock.
    pub fn new(spec: &Spec) -> Recorder {
        Recorder::with(spec, Clock::Real(Instant::now()))
    }

    /// A recorder keeping what `spec` says, whose clock moves only when
    /// told ([`Recorder::tick`]): for tests.
    pub fn told(spec: &Spec) -> Recorder {
        Recorder::with(spec, Clock::Told(Duration::ZERO))
    }

    fn with(spec: &Spec, clock: Clock) -> Recorder {
        Recorder(Arc::new(Mutex::new(Recording {
            keep: Duration::from_secs(spec.minutes.saturating_mul(60)),
            every: Duration::from_secs(spec.checkpoint_seconds.max(1)),
            clock,
            steps: VecDeque::new(),
            first: 0,
            checkpoints: VecDeque::new(),
            ended: None,
            sheet: None,
            title: String::new(),
            language: None,
            mac: false,
        })))
    }

    /// Moves a told clock on by `by`.
    pub fn tick(&self, by: Duration) {
        if let Clock::Told(now) = &mut self.lock().clock {
            *now += by;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Recording> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts recording `editor`'s window, from its state now.
    pub(crate) fn begin(&self, editor: &Editor) {
        let mut r = self.lock();
        let res = editor.resources();
        r.sheet = Some(res.sheet.clone());
        r.language = (res.language != text::Language::und()).then(|| res.language.to_string());
        r.mac = res.mac;
        r.title = editor.title();
        r.ended = Some(editor.document().clone());
        if let Some(checkpoint) = editor.checkpoint() {
            let now = r.now();
            let index = r.first + r.steps.len() as u64;
            r.checkpoints.push_back((now, index, checkpoint));
        }
    }

    /// Notes `event`, about to be handled by `editor`: a checkpoint first,
    /// if one is due and the window is at rest, then its step.
    pub(crate) fn before(&self, editor: &Editor, event: &Event) {
        // Saving the session isn't part of it.
        if matches!(event, Event::Command(id) if id == "save-session") {
            return;
        }
        let mut r = self.lock();
        let now = r.now();
        let answers = matches!(event, Event::Pasted(_) | Event::Chosen(..));
        let due = r
            .checkpoints
            .back()
            .is_none_or(|(at, ..)| now.saturating_sub(*at) >= r.every);
        if !answers
            && due
            && let Some(checkpoint) = editor.checkpoint()
        {
            let index = r.first + r.steps.len() as u64;
            r.checkpoints.push_back((now, index, checkpoint));
            r.trim(now);
        }
        if r.checkpoints.is_empty() {
            // Nothing to start from: nothing kept until there is.
            return;
        }
        match event {
            // What was pasted is what the clipboard held when the key or
            // command that asked for it was pressed: the last step.
            Event::Pasted(text) => {
                let at = r.steps.len().saturating_sub(1);
                r.steps.insert(at, Step::Clipboard(text.clone()));
            }
            event => {
                if let Some(step) = Step::of(event) {
                    r.steps.push_back(step);
                }
            }
        }
        if r.steps.len() > MOST_STEPS {
            r.steps.clear();
            r.checkpoints.clear();
            r.first = 0;
        }
    }

    /// Notes the document `editor` has after an event.
    pub(crate) fn after(&self, editor: &Editor) {
        let mut r = self.lock();
        r.ended = Some(editor.document().clone());
        r.title = editor.title();
    }

    /// What's kept, as a session: from its first checkpoint, expecting
    /// the document as it is after the last event. None if nothing's kept.
    pub fn session(&self) -> Option<Session> {
        self.lock().session()
    }

    /// Writes what's kept to a file of its own in `folder`, named after
    /// the window's document, its first lines saying `note`. Returns its
    /// path.
    pub fn write_to(&self, folder: &Path, note: &str) -> std::io::Result<PathBuf> {
        self.lock().write_to(folder, note)
    }
}

impl Recording {
    fn now(&self) -> Duration {
        match &self.clock {
            Clock::Real(since) => since.elapsed(),
            Clock::Told(now) => *now,
        }
    }

    /// Lets go of what's older than needed: every checkpoint before the
    /// newest one at least `keep` old, and the steps before it.
    fn trim(&mut self, now: Duration) {
        let cut = now.saturating_sub(self.keep);
        while self.checkpoints.len() >= 2 && self.checkpoints[1].0 <= cut {
            self.checkpoints.pop_front();
        }
        if let Some(&(_, index, _)) = self.checkpoints.front() {
            while self.first < index && !self.steps.is_empty() {
                self.steps.pop_front();
                self.first += 1;
            }
        }
    }

    fn session(&self) -> Option<Session> {
        let (_, index, checkpoint) = self.checkpoints.front()?;
        let skip = usize::try_from(index.saturating_sub(self.first)).unwrap_or(usize::MAX);
        let ended = self
            .ended
            .clone()
            .unwrap_or_else(|| checkpoint.document.clone());
        Some(Session {
            size: checkpoint.size,
            events: self.steps.iter().skip(skip).cloned().collect(),
            expect: session::paragraphs(&ended),
            outline: self.sheet.as_ref().map(|s| story::outline(&ended, s)),
            marks: Vec::new(),
            clipboard: None,
            status: None,
            language: self.language.clone(),
            languages: Vec::new(),
            mac: self.mac,
            start: Some(checkpoint.start()),
        })
    }

    fn write_to(&self, folder: &Path, note: &str) -> std::io::Result<PathBuf> {
        let session = self
            .session()
            .ok_or_else(|| std::io::Error::other("nothing has been recorded"))?;
        let ron = session::write(&session).map_err(std::io::Error::other)?;
        let mut text = String::new();
        for line in note.lines() {
            text.push_str(&format!("// {line}\n"));
        }
        text.push_str(&format!(
            "// What was done in \"{}\" over its last few minutes, recorded by\n\
             // xWorks as a session (spec/sessions/), which plays back headless.\n\
             // It holds what was typed, and so the document's text: share it\n\
             // only as you would share the document.\n",
            self.title.replace(['\n', '\r'], " ")
        ));
        text.push_str(&ron);
        text.push('\n');
        std::fs::create_dir_all(folder)?;
        let name = file_name(&self.title);
        let mut n = 1;
        loop {
            let path = if n == 1 {
                folder.join(format!("{name}.ron"))
            } else {
                folder.join(format!("{name} {n}.ron"))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    std::io::Write::write_all(&mut file, text.as_bytes())?;
                    return Ok(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e),
            }
        }
    }
}

/// `title` as a file's name: what a file's name can't hold made dashes.
fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    if name.is_empty() {
        "Untitled".to_string()
    } else {
        name.to_string()
    }
}

/// Every window's recording, for writing them all when xWorks fails:
/// those of windows closed go.
#[derive(Clone, Default)]
pub struct Recordings {
    list: Arc<Mutex<Vec<Weak<Mutex<Recording>>>>>,
    written: Arc<AtomicBool>,
}

impl Recordings {
    /// Adds a window's recording.
    pub fn add(&self, recorder: &Recorder) {
        let mut list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        list.retain(|w| w.strong_count() > 0);
        list.push(Arc::downgrade(&recorder.0));
    }

    /// Writes every window's recording to `folder`, each its first lines
    /// saying `note`, for when xWorks fails: once, and taking no lock
    /// another thread holds, so it can't wait on what failed. Returns the
    /// files written.
    pub fn write_all(&self, folder: &Path, note: &str) -> Vec<PathBuf> {
        if self.written.swap(true, Ordering::SeqCst) {
            return Vec::new();
        }
        let Ok(list) = self.list.try_lock() else {
            return Vec::new();
        };
        let mut written = Vec::new();
        for recording in list.iter().filter_map(Weak::upgrade) {
            let r = match recording.try_lock() {
                Ok(r) => r,
                Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => continue,
            };
            if let Ok(path) = r.write_to(folder, note) {
                written.push(path);
            }
        }
        written
    }
}
