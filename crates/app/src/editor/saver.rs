//! A Markdown file saved on a worker, gathered (review jy4a2tx5): each
//! edit hands the file's new bytes over, and a thread of the document's
//! own writes the latest a moment after the first edit not yet written,
//! `spec/editing.ron` says how long, so it's on disk within a second and
//! the keystroke doesn't wait for it. It's written whole and flushed to
//! disk before it's renamed into place (`store::write_whole`), so a
//! crash loses at most the last second, never the file.
//!
//! Before it writes, it looks at the file: if another program changed it
//! since xWorks last read or wrote it, it writes nothing, and holds what
//! it has until the person says what to do ([`Saver::conflict`]). A file
//! another program took away is written again.
//!
//! [`Saver::flush`] writes what's waiting at once, as closing the
//! document and quitting do; dropped, the saver writes it too, or, if the
//! file changed under it, writes it as a copy beside it, so nothing typed
//! is lost. What it can't write then is logged, as no window is left to
//! say so.
//!
//! It's a thread, not a task on the worker pool: it sleeps between edits
//! and while it gathers them, which would hold one of the pool's threads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use store::Stamp;
use tracing::error;

/// The bytes to write, and where.
struct Job {
    path: PathBuf,
    bytes: Vec<u8>,
}

/// What the saver and its thread share.
#[derive(Default)]
struct State {
    /// The latest bytes not yet written.
    job: Option<Job>,
    /// When the first edit not yet written came.
    since: Option<Instant>,
    /// Whether a write is under way.
    writing: bool,
    /// Whether to write what's waiting at once.
    now: bool,
    /// The file as xWorks last read or wrote it.
    stamp: Option<Stamp>,
    /// Whether the file changed under xWorks: what's waiting isn't
    /// written until the person says.
    conflict: bool,
    /// Why the last write failed, if it did.
    error: Option<String>,
    /// Whether the file was gone and was written again.
    recreated: bool,
    /// How many writes have been tried.
    tries: u64,
    /// Whether the saver is ending.
    quit: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the saver says has happened since it was last asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct News {
    /// The file changed under xWorks, and what's waiting is held.
    pub conflict: bool,
    /// Why the last write failed, if it did.
    pub error: Option<String>,
    /// The file was gone, and was written again.
    pub recreated: bool,
}

/// A Markdown file's saver: its thread, and what it shares with it.
pub(super) struct Saver {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    /// A saver for a file that's `stamp` on disk as it's read, writing
    /// each edit `after` the first not yet written.
    pub(super) fn new(stamp: Option<Stamp>, after: Duration) -> Saver {
        let shared = Arc::new(Shared::default());
        shared.lock().stamp = stamp;
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("xworks-save".to_string())
            .spawn(move || run(&worker, after))
            .ok();
        Saver { shared, thread }
    }

    /// Hands over `bytes`, the file at `path` as it now is, to be written
    /// in a moment, in place of what's waiting.
    pub(super) fn put(&self, path: &Path, bytes: Vec<u8>) {
        let mut state = self.shared.lock();
        state.job = Some(Job {
            path: path.to_path_buf(),
            bytes,
        });
        state.since.get_or_insert_with(Instant::now);
        drop(state);
        self.shared.wake.notify_all();
        if self.thread.is_none() {
            // No thread could be had: written here, at once; a failure is
            // news, said as the window next asks.
            let _ = self.flush();
        }
    }

    /// Whether everything handed over is on disk: nothing waits, and no
    /// write is under way.
    pub(super) fn idle(&self) -> bool {
        let state = self.shared.lock();
        state.job.is_none() && !state.writing
    }

    /// The file as xWorks last read or wrote it.
    pub(super) fn stamp(&self) -> Option<Stamp> {
        self.shared.lock().stamp.clone()
    }

    /// The file as it is now, read or written by xWorks: what's waiting
    /// is written to it from here on, over whatever it held.
    pub(super) fn set_stamp(&self, stamp: Option<Stamp>) {
        let mut state = self.shared.lock();
        state.stamp = stamp;
        state.conflict = false;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Holds what's waiting, the file having changed under xWorks, until
    /// the person says what to do.
    pub(super) fn hold(&self) {
        self.shared.lock().conflict = true;
    }

    /// Whether what's waiting is held, the file having changed under
    /// xWorks.
    pub(super) fn conflict(&self) -> bool {
        self.shared.lock().conflict
    }

    /// Takes what's waiting away, unwritten: its bytes, if there were any.
    pub(super) fn take(&self) -> Option<Vec<u8>> {
        let mut state = self.shared.lock();
        state.since = None;
        state.job.take().map(|j| j.bytes)
    }

    /// What's happened since this was last asked: the error and the file
    /// written again are said once.
    pub(super) fn news(&self) -> News {
        let mut state = self.shared.lock();
        News {
            conflict: state.conflict,
            error: state.error.take(),
            recreated: std::mem::take(&mut state.recreated),
        }
    }

    /// Writes what's waiting at once, and waits for it: what closing the
    /// document and quitting do. Not while it's held, the file having
    /// changed under xWorks. An error if the write fails.
    pub(super) fn flush(&self) -> Result<(), String> {
        let mut state = self.shared.lock();
        if self.thread.is_none() {
            // No thread: written here.
            let Some(job) = state.job.take() else {
                return Ok(());
            };
            state.since = None;
            let expected = state.stamp.clone();
            drop(state);
            let result = write(&job, expected.as_ref());
            let mut state = self.shared.lock();
            return settle(&mut state, job, result);
        }
        let tries = state.tries;
        state.now = true;
        self.shared.wake.notify_all();
        loop {
            let done = state.job.is_none() && !state.writing;
            if done || state.conflict || (state.tries > tries && !state.writing) {
                break;
            }
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        match (&state.error, state.job.is_none()) {
            (Some(e), false) => Err(e.clone()),
            _ => Ok(()),
        }
    }
}

impl Drop for Saver {
    /// What's waiting is written, or as a copy beside the file if it
    /// changed under xWorks; then the thread ends.
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.quit = true;
        drop(state);
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        } else {
            let mut state = self.shared.lock();
            if let Some(job) = state.job.take() {
                let (expected, conflict) = (state.stamp.clone(), state.conflict);
                drop(state);
                last(&job, expected.as_ref(), conflict);
            }
        }
    }
}

/// What writing `job` came to.
enum Wrote {
    /// Written: the file as it then was.
    Done(Stamp),
    /// The file was gone, and is written again.
    Again(Stamp),
    /// Another program changed it since xWorks read or wrote it: not
    /// written.
    Changed,
    /// The write failed.
    Failed(String),
}

/// Writes `job`, if the file is as `expected`, as xWorks last read or
/// wrote it, or gone.
fn write(job: &Job, expected: Option<&Stamp>) -> Wrote {
    let now = store::stamp(&job.path);
    if now.is_some() && expected.is_some() && now.as_ref() != expected {
        return Wrote::Changed;
    }
    match store::write_whole(&job.path, &job.bytes) {
        Ok(stamp) if now.is_none() && expected.is_some() => Wrote::Again(stamp),
        Ok(stamp) => Wrote::Done(stamp),
        Err(e) => Wrote::Failed(e.to_string()),
    }
}

/// `state` after `job` was written, as `result` says: what's waiting
/// still, if the write didn't happen and nothing newer came.
fn settle(state: &mut State, job: Job, result: Wrote) -> Result<(), String> {
    state.tries += 1;
    let keep = |state: &mut State| {
        if state.job.is_none() {
            state.job = Some(job);
            state.since.get_or_insert_with(Instant::now);
        }
    };
    match result {
        Wrote::Done(stamp) => {
            state.stamp = Some(stamp);
            state.error = None;
            Ok(())
        }
        Wrote::Again(stamp) => {
            state.stamp = Some(stamp);
            state.error = None;
            state.recreated = true;
            Ok(())
        }
        Wrote::Changed => {
            state.conflict = true;
            keep(state);
            Ok(())
        }
        Wrote::Failed(e) => {
            state.error = Some(e.clone());
            keep(state);
            Err(e)
        }
    }
}

/// Writes `job` as the saver ends, the file expected to be as `expected`
/// says: or, if it changed under xWorks, whether the window knew
/// (`conflict`) or only this last look finds it, leaves the file as the
/// other program made it and writes `job` as a copy beside it. No window
/// is left to say what went wrong: it's logged.
fn last(job: &Job, expected: Option<&Stamp>, conflict: bool) {
    let changed = conflict
        || match write(job, expected) {
            Wrote::Failed(e) => {
                error!("writing {}: {e}", job.path.display());
                false
            }
            Wrote::Changed => true,
            Wrote::Done(_) | Wrote::Again(_) => false,
        };
    if changed {
        let copy = copy_path(&job.path);
        if let Err(e) = store::write_whole(&copy, &job.bytes) {
            error!("writing {}: {e}", copy.display());
        }
    }
}

/// The saver's thread: it waits for an edit, gathers those that come
/// `after` the first, writes the latest, and goes on until it's told to
/// end, when it writes what's waiting, or a copy of it if the file
/// changed under it.
fn run(shared: &Shared, after: Duration) {
    let mut state = shared.lock();
    loop {
        if state.quit {
            if let Some(job) = state.job.take() {
                let (expected, conflict) = (state.stamp.clone(), state.conflict);
                drop(state);
                last(&job, expected.as_ref(), conflict);
            }
            shared.wake.notify_all();
            return;
        }
        let due = match (&state.job, state.since, state.conflict) {
            (Some(_), Some(since), false) => since + after,
            _ => {
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            }
        };
        let now = Instant::now();
        if !state.now && now < due {
            state = shared
                .wake
                .wait_timeout(state, due - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            continue;
        }
        let Some(job) = state.job.take() else {
            continue;
        };
        state.since = None;
        state.now = false;
        state.writing = true;
        let expected = state.stamp.clone();
        drop(state);
        let result = write(&job, expected.as_ref());
        state = shared.lock();
        state.writing = false;
        let _ = settle(&mut state, job, result);
        if state.error.is_some() && state.job.is_some() {
            // Tried again after as long, not at once.
            state.since = Some(Instant::now());
        }
        shared.wake.notify_all();
    }
}

/// Where what's waiting for the file at `path` goes when the file changed
/// under xWorks and the person doesn't say: beside it, "NAME (xWorks).md",
/// or "NAME (xWorks 2).md" and so on.
pub(super) fn copy_path(path: &Path) -> PathBuf {
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let extension = path
        .extension()
        .map_or_else(|| "md".to_string(), |e| e.to_string_lossy().into_owned());
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem} (xWorks).{extension}")
        } else {
            format!("{stem} (xWorks {n}).{extension}")
        };
        let candidate = folder.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}
